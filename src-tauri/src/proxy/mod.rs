use std::{
    collections::HashMap,
    future::poll_fn,
    sync::{atomic::Ordering, Arc, Mutex, OnceLock},
    time::Instant,
};

use ::http::{header, HeaderValue, Request, Response, StatusCode};
use bytes::{Buf, Bytes};
use percent_encoding::percent_decode_str;
use tokio::task::JoinSet;
use tokio::{
    io::{copy_bidirectional, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

mod interception;
pub mod match_replace;
pub mod upstream;
pub use interception::{InterceptionManager, InterceptionResolution};

use crate::{
    error::{Result, WitnessError},
    event_bus::{Event, ForwardEvent, ProxyEvent},
    http::{
        apply_response_compression, is_chunked, is_keep_alive, parse_request,
        parse_response_for_method, serialize_request, serialize_response, MAX_MESSAGE_SIZE,
    },
    logging,
    state::{AppState, InterceptionRule},
    tls::{CertificateAuthority, CertificateStatus},
};

#[cfg(test)]
use crate::http::parse_response;

const BAD_REQUEST: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 11\r\n\r\nBad Request";
const BAD_GATEWAY: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 11\r\n\r\nBad Gateway";
const GATEWAY_TIMEOUT: &[u8] = b"HTTP/1.1 504 Gateway Timeout\r\nConnection: close\r\nContent-Length: 15\r\n\r\nGateway Timeout";

/// Maximum interception regex length (mirrors `match_replace::MAX_REGEX_LEN`).
/// Longer conditions are treated as no-match + logged to bound compile cost.
pub const MAX_REGEX_LEN: usize = 512;

/// Cache for case-insensitive interception regexes (policy hot path).
static INTERCEPTION_REGEX_CACHE: OnceLock<Mutex<HashMap<String, regex::Regex>>> = OnceLock::new();

fn interception_regex_cache() -> &'static Mutex<HashMap<String, regex::Regex>> {
    INTERCEPTION_REGEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_interception_regex(condition: &str) -> Option<regex::Regex> {
    if condition.len() > MAX_REGEX_LEN {
        tracing::warn!(
            target: "witness_lib::network::interception",
            condition_len = condition.len(),
            max_len = MAX_REGEX_LEN,
            "interception regex too long; treating as no-match"
        );
        return None;
    }
    if let Ok(cache) = interception_regex_cache().lock() {
        if let Some(regex) = cache.get(condition) {
            return Some(regex.clone());
        }
    }
    let compiled = regex::RegexBuilder::new(condition)
        .case_insensitive(true)
        .build()
        .ok()?;
    if let Ok(mut cache) = interception_regex_cache().lock() {
        if cache.len() < 512 {
            cache.insert(condition.to_owned(), compiled.clone());
        }
    }
    Some(compiled)
}

#[derive(Debug, Clone)]
pub enum ForwardAction<T> {
    Forward(T),
    Drop,
    Modify(T),
}

pub struct ProxyEngine;

impl ProxyEngine {
    pub async fn run(state: AppState, cancellation: CancellationToken) -> Result<()> {
        let _operation = logging::OperationGuard::new("proxy.run");
        let configuration = state.proxy.read().await.clone();
        tracing::info!(
            target: "witness_lib::network",
            phase = "listener_bind_started",
            bind_address = %configuration.bind_address,
            configured_port = configuration.port,
            "proxy listener bind started"
        );
        let listener = match TcpListener::bind((
            configuration.bind_address.as_str(),
            configuration.port,
        ))
        .await
        {
            Ok(listener) => listener,
            Err(error) => {
                tracing::error!(
                    target: "witness_lib::network",
                    phase = "listener_bind_failed",
                    bind_address = %configuration.bind_address,
                    configured_port = configuration.port,
                    error = %error,
                    "proxy listener bind failed"
                );
                return Err(WitnessError::Proxy(error.to_string()));
            }
        };
        let address = listener.local_addr()?;
        {
            let mut proxy = state.proxy.write().await;
            proxy.running = true;
            proxy.port = address.port();
        }
        state.event_bus.publish(Event::Proxy(ProxyEvent::Started {
            address: address.to_string(),
        }));
        tracing::info!(
            target: "witness_lib::network",
            phase = "listener_started",
            address = %address,
            "proxy listener started"
        );

        let configured_authority = state.certificate_authority.read().await.clone();
        let certificate_directory =
            std::path::PathBuf::from(&state.settings.read().await.certificate_directory);
        let status_before_load = if configured_authority.is_some() {
            CertificateStatus::Present
        } else {
            CertificateAuthority::status(&certificate_directory)
        };
        let authority = match configured_authority
            .map(Ok)
            .unwrap_or_else(|| CertificateAuthority::load_or_create(&certificate_directory))
        {
            Ok(authority) => {
                *state.certificate_authority.write().await = Some(authority.clone());
                let status = match status_before_load {
                    CertificateStatus::Missing => "generated; install the CA certificate",
                    CertificateStatus::Expired => "expired; a replacement was generated",
                    CertificateStatus::Present => "present",
                };
                state.proxy.write().await.certificate_status = status.into();
                state.event_bus.publish(Event::Proxy(ProxyEvent::TlsStatus {
                    status: status.into(),
                }));
                tracing::info!(
                    target: "witness_lib::network",
                    phase = "tls_authority_ready",
                    certificate_status = status,
                    "proxy TLS authority ready"
                );
                Some(authority)
            }
            Err(error) => {
                state.proxy.write().await.certificate_status = "unavailable".into();
                tracing::warn!(
                    target: "witness_lib::network",
                    phase = "tls_authority_unavailable",
                    error = %error,
                    "proxy TLS interception is unavailable"
                );
                state.event_bus.publish(Event::Proxy(ProxyEvent::TlsStatus {
                    status: format!("unavailable: {error}"),
                }));
                None
            }
        };

        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                completed = connections.join_next(), if !connections.is_empty() => {
                    if let Some(Err(error)) = completed {
                        tracing::error!(%error, "proxy connection task panicked");
                        state.event_bus.publish(Event::Proxy(ProxyEvent::Error {
                            message: "a proxy connection task ended unexpectedly".into(),
                        }));
                    }
                }
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, peer)) => {
                            let connection_id = Uuid::new_v4().to_string();
                            update_connection_count(&state, 1).await;
                            tracing::info!(
                                target: "witness_lib::network",
                                phase = "client_connection_accepted",
                                connection_id = %connection_id,
                                peer = %peer,
                                "proxy client connection accepted"
                            );
                            let connection_state = state.clone();
                            let connection_ca = authority.clone();
                            let connection_cancel = cancellation.child_token();
                            let connection_id_for_task = connection_id.clone();
                            connections.spawn(async move {
                                if let Err(error) = handle_client(
                                    stream,
                                    connection_state.clone(),
                                    connection_ca,
                                    connection_cancel,
                                    &connection_id_for_task,
                                    peer,
                                ).await {
                                    if is_routine_tls_disconnect(&error) {
                                        tracing::debug!(
                                            target: "witness_lib::network",
                                            phase = "client_connection_closed",
                                            connection_id = %connection_id_for_task,
                                            %peer,
                                            reason = "tls_close_notify_omitted",
                                            "proxy client connection closed"
                                        );
                                    } else {
                                        tracing::warn!(
                                            target: "witness_lib::network",
                                            phase = "client_connection_failed",
                                            connection_id = %connection_id_for_task,
                                            %peer,
                                            error = %error,
                                            "proxy client connection ended with an error"
                                        );
                                    }
                                } else {
                                    tracing::debug!(
                                        target: "witness_lib::network",
                                        phase = "client_connection_closed",
                                        connection_id = %connection_id_for_task,
                                        %peer,
                                        "proxy client connection closed"
                                    );
                                }
                                update_connection_count(&connection_state, -1).await;
                            });
                        }
                        Err(error) => {
                            tracing::error!(
                                target: "witness_lib::network",
                                phase = "client_connection_accept_failed",
                                error = %error,
                                "proxy failed to accept client connection"
                            );
                            state.event_bus.publish(Event::Proxy(ProxyEvent::Error {
                                message: error.to_string(),
                            }));
                        }
                    }
                }
            }
        }

        cancellation.cancel();
        connections.abort_all();
        while let Some(completed) = connections.join_next().await {
            if let Err(error) = completed {
                tracing::error!(%error, "proxy connection task failed during shutdown");
            }
        }

        {
            let mut proxy = state.proxy.write().await;
            proxy.running = false;
            proxy.connection_count = 0;
        }
        state.event_bus.publish(Event::Proxy(ProxyEvent::Stopped));
        tracing::info!(target: "witness_lib::network", phase = "listener_stopped", "proxy listener stopped");
        Ok(())
    }
}

async fn update_connection_count(state: &AppState, change: isize) {
    let count = {
        let mut proxy = state.proxy.write().await;
        proxy.connection_count = proxy.connection_count.saturating_add_signed(change);
        proxy.connection_count
    };
    state
        .event_bus
        .publish(Event::Proxy(ProxyEvent::ConnectionCount { count }));
    tracing::debug!(
        target: "witness_lib::network",
        phase = "connection_count_changed",
        change,
        count,
        "proxy connection count changed"
    );
}

fn is_routine_tls_disconnect(error: &WitnessError) -> bool {
    matches!(
        error,
        WitnessError::Io(error)
            if error.kind() == std::io::ErrorKind::UnexpectedEof
                && error.to_string().contains("close_notify")
    )
}

async fn handle_client(
    mut client: TcpStream,
    state: AppState,
    authority: Option<Arc<CertificateAuthority>>,
    cancellation: CancellationToken,
    connection_id: &str,
    peer: std::net::SocketAddr,
) -> Result<()> {
    let _operation = logging::OperationGuard::new("proxy.handle_client");
    tracing::debug!(
        target: "witness_lib::network",
        phase = "client_handler_started",
        connection_id,
        %peer,
        "proxy client handler started"
    );
    let mut buffer = Vec::new();
    loop {
        let request = tokio::select! {
            _ = cancellation.cancelled() => return Ok(()),
            result = read_request(&mut client, &mut buffer) => result,
        };
        let request = match request {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(error) => {
                tracing::warn!(
                    target: "witness_lib::network",
                    phase = "request_parse_failed",
                    connection_id,
                    %peer,
                    error = %error,
                    "malformed client request received"
                );
                let _ = client.write_all(BAD_REQUEST).await;
                return Err(error);
            }
        };

        let exchange_id = Uuid::new_v4().to_string();
        let (method, target, header_count, body_bytes) = logging::request_metadata(&request);
        tracing::info!(
            target: "witness_lib::network",
            phase = "request_received",
            connection_id,
            exchange_id = %exchange_id,
            peer = %peer,
            transport = "http",
            version = ?request.version(),
            %method,
            target = %target,
            header_count,
            body_bytes,
            "proxy request received"
        );

        if request.method() == ::http::Method::CONNECT {
            let Some(authority) = authority else {
                tracing::error!(
                    target: "witness_lib::network",
                    phase = "connect_rejected",
                    connection_id,
                    exchange_id = %exchange_id,
                    reason = "tls_authority_unavailable",
                    "CONNECT request rejected because TLS authority is unavailable"
                );
                client.write_all(BAD_GATEWAY).await?;
                return Err(WitnessError::Tls(
                    "certificate authority is unavailable".into(),
                ));
            };
            tracing::info!(
                target: "witness_lib::network",
                phase = "connect_received",
                connection_id,
                exchange_id = %exchange_id,
                target = %logging::safe_authority(request.uri().to_string().as_str()),
                "CONNECT tunnel request received"
            );
            return handle_connect(
                client,
                request,
                state,
                authority,
                cancellation,
                connection_id,
            )
            .await;
        }

        if is_websocket_upgrade(&request) {
            return handle_websocket(
                &mut client,
                request,
                std::mem::take(&mut buffer),
                &state,
                false,
                &cancellation,
                exchange_id,
            )
            .await;
        }

        let keep_alive = is_keep_alive(request.headers());
        match forward_request(request, &state, false, &cancellation, &exchange_id).await {
            Ok(Some(response)) => {
                let raw_response = serialize_response(&response);
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "response_written_to_client",
                    connection_id,
                    exchange_id = %exchange_id,
                    status = response.status().as_u16(),
                    response_bytes = raw_response.len(),
                    "proxy response written to client"
                );
                client.write_all(&raw_response).await?
            }
            Ok(None) => return Ok(()),
            Err(WitnessError::Cancelled) => return Ok(()),
            Err(error) => {
                tracing::warn!(
                    target: "witness_lib::network",
                    phase = "request_failed",
                    connection_id,
                    exchange_id = %exchange_id,
                    error = %error,
                    "upstream request failed"
                );
                let payload = if error.to_string().contains("timed out") {
                    GATEWAY_TIMEOUT
                } else {
                    BAD_GATEWAY
                };
                client.write_all(payload).await?;
                return Err(error);
            }
        }
        if !keep_alive {
            tracing::debug!(
                target: "witness_lib::network",
                phase = "client_keep_alive_finished",
                connection_id,
                "client requested connection close"
            );
            return Ok(());
        }
    }
}

async fn handle_connect(
    mut client: TcpStream,
    connect_request: Request<Vec<u8>>,
    state: AppState,
    authority: Arc<CertificateAuthority>,
    cancellation: CancellationToken,
    connection_id: &str,
) -> Result<()> {
    let _operation = logging::OperationGuard::new("proxy.handle_connect");
    let authority_text = connect_request
        .uri()
        .authority()
        .map(|value| value.as_str())
        .unwrap_or_else(|| connect_request.uri().path());
    let (host, _) = split_host_port(authority_text, 443)?;
    tracing::info!(
        target: "witness_lib::network",
        phase = "connect_target_resolved",
        connection_id,
        target_host = %host,
        "CONNECT target resolved"
    );
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    tracing::debug!(
        target: "witness_lib::network",
        phase = "connect_established",
        connection_id,
        target_host = %host,
        "CONNECT tunnel acknowledged"
    );

    // The CONNECT authority, rather than ClientHello SNI, selects the leaf
    // certificate. Browsers can omit SNI or encrypt it with ECH, but still
    // expect the certificate for the CONNECT destination.
    let acceptor = TlsAcceptor::from(authority.server_config_for_host(&host)?);
    let client_tls = tokio::select! {
        _ = cancellation.cancelled() => return Err(WitnessError::Cancelled),
        result = acceptor.accept(client) => result.map_err(|error| WitnessError::Tls(error.to_string())),
    };
    let mut client_tls = match client_tls {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!(
                target: "witness_lib::network",
                phase = "client_tls_handshake_failed",
                connection_id,
                target_host = %host,
                error = %error,
                "client TLS handshake failed"
            );
            return Err(error);
        }
    };
    let alpn = client_tls
        .get_ref()
        .1
        .alpn_protocol()
        .map(|protocol| String::from_utf8_lossy(protocol).into_owned())
        .unwrap_or_else(|| "none".into());
    tracing::info!(
        target: "witness_lib::network",
        phase = "client_tls_handshake_completed",
        connection_id,
        target_host = %host,
        alpn = %alpn,
        "client TLS handshake completed"
    );
    if alpn == "h2" {
        return handle_http2_client(client_tls, state, cancellation, connection_id).await;
    }

    let mut buffer = Vec::new();
    loop {
        let request = tokio::select! {
            _ = cancellation.cancelled() => break,
            result = read_request(&mut client_tls, &mut buffer) => result?,
        };
        let Some(request) = request else {
            break;
        };
        let exchange_id = Uuid::new_v4().to_string();
        let (method, target, header_count, body_bytes) = logging::request_metadata(&request);
        tracing::info!(
            target: "witness_lib::network",
            phase = "request_received",
            connection_id,
            exchange_id = %exchange_id,
            transport = "https",
            version = ?request.version(),
            %method,
            target = %target,
            header_count,
            body_bytes,
            "HTTPS request received inside CONNECT tunnel"
        );
        if is_websocket_upgrade(&request) {
            return handle_websocket(
                &mut client_tls,
                request,
                std::mem::take(&mut buffer),
                &state,
                true,
                &cancellation,
                exchange_id,
            )
            .await;
        }
        let keep_alive = is_keep_alive(request.headers());
        match forward_request(request, &state, true, &cancellation, &exchange_id).await {
            Ok(Some(response)) => {
                let raw_response = serialize_response(&response);
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "response_written_to_client",
                    connection_id,
                    exchange_id = %exchange_id,
                    status = response.status().as_u16(),
                    response_bytes = raw_response.len(),
                    "HTTPS response written to client"
                );
                client_tls.write_all(&raw_response).await?
            }
            Ok(None) | Err(WitnessError::Cancelled) => break,
            Err(error) => {
                tracing::warn!(
                    target: "witness_lib::network",
                    phase = "request_failed",
                    connection_id,
                    exchange_id = %exchange_id,
                    error = %error,
                    "HTTPS upstream request failed"
                );
                let payload = if error.to_string().contains("timed out") {
                    GATEWAY_TIMEOUT
                } else {
                    BAD_GATEWAY
                };
                client_tls.write_all(payload).await?;
                let _ = client_tls.shutdown().await;
                return Err(error);
            }
        }
        if !keep_alive {
            break;
        }
    }
    let _ = client_tls.shutdown().await;
    Ok(())
}

async fn handle_http2_client<T>(
    client: T,
    state: AppState,
    cancellation: CancellationToken,
    connection_id: &str,
) -> Result<()>
where
    T: AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let _operation = logging::OperationGuard::new("proxy.handle_http2_client");
    tracing::info!(
        target: "witness_lib::network",
        phase = "http2_connection_started",
        connection_id,
        "HTTP/2 client connection started"
    );
    let mut connection = match h2::server::handshake(client).await.map_err(http2_error) {
        Ok(connection) => connection,
        Err(error) => {
            tracing::warn!(
                target: "witness_lib::network",
                phase = "http2_connection_handshake_failed",
                connection_id,
                error = %error,
                "HTTP/2 client connection handshake failed"
            );
            return Err(error);
        }
    };
    let mut streams = JoinSet::new();

    loop {
        tokio::select! {
            _ = cancellation.cancelled() => break,
            completed = streams.join_next(), if !streams.is_empty() => {
                if let Some(Err(error)) = completed {
                    tracing::warn!(%error, "HTTP/2 stream task panicked");
                }
            }
            incoming = connection.accept() => {
                let Some(incoming) = incoming else {
                    break;
                };
                let (request, mut responder) = incoming.map_err(http2_error)?;
                let stream_state = state.clone();
                let stream_cancellation = cancellation.child_token();
                let exchange_id = Uuid::new_v4().to_string();
                let connection_id_for_stream = connection_id.to_string();
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "http2_stream_received",
                    connection_id,
                    exchange_id = %exchange_id,
                    "HTTP/2 request stream received"
                );
                streams.spawn(async move {
                    let request = match collect_http2_request(request).await {
                        Ok(request) => request,
                        Err(error) => {
                            tracing::warn!(
                                target: "witness_lib::network",
                                phase = "http2_request_body_failed",
                                connection_id = %connection_id_for_stream,
                                exchange_id = %exchange_id,
                                error = %error,
                                "HTTP/2 request body could not be collected"
                            );
                            responder.send_reset(h2::Reason::PROTOCOL_ERROR);
                            return;
                        }
                    };
                    let (method, target, header_count, body_bytes) = logging::request_metadata(&request);
                    tracing::info!(
                        target: "witness_lib::network",
                        phase = "request_received",
                        connection_id = %connection_id_for_stream,
                        exchange_id = %exchange_id,
                        transport = "https",
                        version = ?request.version(),
                        %method,
                        target = %target,
                        header_count,
                        body_bytes,
                        protocol = "h2",
                        "HTTP/2 request received"
                    );
                    match forward_request(request, &stream_state, true, &stream_cancellation, &exchange_id).await {
                        Ok(Some(response)) => {
                            let status = response.status().as_u16();
                            let response_bytes = serialize_response(&response).len();
                            match send_http2_response(responder, response).await {
                                Ok(()) => {
                                    tracing::debug!(
                                        target: "witness_lib::network",
                                        phase = "response_written_to_client",
                                        connection_id = %connection_id_for_stream,
                                        exchange_id = %exchange_id,
                                        status,
                                        response_bytes,
                                        protocol = "h2",
                                        "HTTP/2 response written to client"
                                    );
                                }
                                Err(error) if error.client_cancelled() => {
                                    tracing::debug!(
                                        target: "witness_lib::network",
                                        phase = "response_delivery_cancelled",
                                        connection_id = %connection_id_for_stream,
                                        exchange_id = %exchange_id,
                                        status,
                                        response_bytes,
                                        error = %error,
                                        "HTTP/2 client cancelled response delivery"
                                    );
                                }
                                Err(error) => {
                                    tracing::warn!(
                                        target: "witness_lib::network",
                                        phase = "response_delivery_error",
                                        connection_id = %connection_id_for_stream,
                                        exchange_id = %exchange_id,
                                        status,
                                        response_bytes,
                                        error = %error,
                                        "HTTP/2 response delivery failed"
                                    );
                                }
                            }
                        }
                        Ok(None) | Err(WitnessError::Cancelled) => {
                            tracing::debug!(
                                target: "witness_lib::network",
                                phase = "http2_stream_cancelled",
                                connection_id = %connection_id_for_stream,
                                exchange_id = %exchange_id,
                                "HTTP/2 request stream cancelled or dropped"
                            );
                            responder.send_reset(h2::Reason::CANCEL);
                        }
                        Err(error) => {
                            tracing::warn!(
                                target: "witness_lib::network",
                                phase = "request_failed",
                                connection_id = %connection_id_for_stream,
                                exchange_id = %exchange_id,
                                error = %error,
                                protocol = "h2",
                                "HTTP/2 upstream request failed"
                            );
                            let status = if error.to_string().contains("timed out") {
                                StatusCode::GATEWAY_TIMEOUT
                            } else {
                                StatusCode::BAD_GATEWAY
                            };
                            let body = if status == StatusCode::GATEWAY_TIMEOUT {
                                b"Gateway Timeout".to_vec()
                            } else {
                                b"Bad Gateway".to_vec()
                            };
                            let response = Response::builder()
                                .status(status)
                                .body(body)
                                .expect("static HTTP/2 error response");
                            let _ = send_http2_response(responder, response).await;
                        }
                    }
                });
            }
        }
    }

    cancellation.cancel();
    streams.abort_all();
    while let Some(completed) = streams.join_next().await {
        if let Err(error) = completed {
            tracing::warn!(
                target: "witness_lib::network",
                phase = "http2_stream_shutdown_failed",
                connection_id,
                error = %error,
                "HTTP/2 stream task failed during shutdown"
            );
        }
    }
    tracing::info!(
        target: "witness_lib::network",
        phase = "http2_connection_stopped",
        connection_id,
        "HTTP/2 client connection stopped"
    );
    Ok(())
}

async fn collect_http2_request(request: Request<h2::RecvStream>) -> Result<Request<Vec<u8>>> {
    let (mut parts, body) = request.into_parts();
    parts.version = ::http::Version::HTTP_2;
    let bytes = collect_http2_body(body).await?;
    Ok(Request::from_parts(parts, bytes))
}

async fn collect_http2_body(mut body: h2::RecvStream) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.map_err(http2_error)?;
        if output.len().saturating_add(chunk.len()) > MAX_MESSAGE_SIZE {
            return Err(WitnessError::InvalidHttp(
                "HTTP/2 message body exceeds 100 MiB".into(),
            ));
        }
        output.extend_from_slice(&chunk);
        body.flow_control()
            .release_capacity(chunk.len())
            .map_err(http2_error)?;
    }
    Ok(output)
}

async fn send_http2_response(
    mut responder: h2::server::SendResponse<Bytes>,
    response: Response<Vec<u8>>,
) -> Result<()> {
    let (mut parts, body) = response.into_parts();
    parts.version = ::http::Version::HTTP_2;
    remove_connection_headers(&mut parts.headers);
    let response = Response::from_parts(parts, ());
    let end_stream = body.is_empty();
    let mut stream = responder
        .send_response(response, end_stream)
        .map_err(http2_error)?;
    if !end_stream {
        send_http2_body(&mut stream, Bytes::from(body)).await?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum InterceptionKind {
    Request,
    Response,
}

struct InterceptionContext {
    url: String,
    domain: String,
    ip_address: String,
    protocol: String,
    method: String,
    content_type: String,
    request: String,
    cookie_names: String,
    cookie_values: String,
    headers: String,
    body: String,
    param_names: String,
    param_values: String,
    listener_port: String,
    in_scope: bool,
}

async fn should_intercept(
    state: &AppState,
    kind: InterceptionKind,
    mut context: InterceptionContext,
) -> bool {
    let direction = match kind {
        InterceptionKind::Request => "request",
        InterceptionKind::Response => "response",
    };
    let proxy = state.proxy.read().await;
    let proxy_intercepting = proxy.intercepting;
    context.listener_port = proxy.port.to_string();
    drop(proxy);
    let settings = state.settings.read().await.clone();
    let matches_mode = matches!(
        (kind, settings.proxy_intercept_mode.as_str()),
        (InterceptionKind::Request, "allRequests")
            | (InterceptionKind::Response, "allResponses")
            | (InterceptionKind::Request, "requestsAndResponses")
            | (InterceptionKind::Response, "requestsAndResponses")
    );
    if !proxy_intercepting || !matches_mode {
        tracing::debug!(
            target: "witness_lib::network::interception",
            phase = "decision",
            direction,
            url = %logging::safe_url(context.url.as_str()),
            enabled = proxy_intercepting,
            mode_matches = matches_mode,
            decision = false,
            reason = "disabled_or_mode_mismatch",
            "interception decision evaluated"
        );
        return false;
    }
    let in_scope = state.scope.is_in_scope(&context.domain).await;
    if settings.intercept_in_scope_only && !in_scope {
        tracing::debug!(
            target: "witness_lib::network::interception",
            phase = "decision",
            direction,
            url = %logging::safe_url(context.url.as_str()),
            in_scope,
            decision = false,
            reason = "out_of_scope",
            "interception decision evaluated"
        );
        return false;
    }
    context.in_scope = in_scope;
    if !matches_content_type_filters(&settings.intercept_content_types, &context) {
        tracing::debug!(
            target: "witness_lib::network::interception",
            phase = "decision",
            direction,
            url = %logging::safe_url(context.url.as_str()),
            content_type = %context.content_type,
            decision = false,
            reason = "content_type_filter",
            "interception decision evaluated"
        );
        return false;
    }
    let rules = match kind {
        InterceptionKind::Request => &settings.request_interception_rules,
        InterceptionKind::Response => &settings.response_interception_rules,
    };
    let decision = rules_match(rules, &context);
    tracing::debug!(
        target: "witness_lib::network::interception",
        phase = "decision",
        direction,
        url = %logging::safe_url(context.url.as_str()),
        in_scope,
        content_type = %context.content_type,
        enabled_rules = rules.iter().filter(|rule| rule.enabled).count(),
        decision,
        reason = if decision { "rules_match" } else { "rules_no_match" },
        "interception decision evaluated"
    );
    decision
}

fn matches_content_type_filters(filters: &[String], context: &InterceptionContext) -> bool {
    filters.is_empty()
        || filters
            .iter()
            .any(|filter| matches_content_type_filter(filter, context))
}

fn matches_content_type_filter(filter: &str, context: &InterceptionContext) -> bool {
    let content_type = context.content_type.to_ascii_lowercase();
    let extension = file_extension(&context.url);
    match filter {
        "html" => matches!(extension, "html" | "htm" | "xhtml") || content_type.contains("html"),
        "javascript" => {
            matches!(extension, "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx")
                || content_type.contains("javascript")
                || content_type.contains("ecmascript")
        }
        "css" => extension == "css" || content_type.contains("text/css"),
        "json" => extension == "json" || content_type.contains("json"),
        "xml" => matches!(extension, "xml" | "svg") || content_type.contains("xml"),
        "images" => {
            matches!(
                extension,
                "avif" | "bmp" | "gif" | "ico" | "jpeg" | "jpg" | "png" | "webp"
            ) || content_type.starts_with("image/")
        }
        "fonts" => {
            matches!(extension, "eot" | "otf" | "ttf" | "woff" | "woff2")
                || content_type.starts_with("font/")
                || content_type.contains("font")
        }
        "media" => {
            matches!(
                extension,
                "aac" | "m4a" | "mkv" | "mov" | "mp3" | "mp4" | "ogg" | "wav" | "webm"
            ) || content_type.starts_with("audio/")
                || content_type.starts_with("video/")
        }
        "documents" => {
            matches!(
                extension,
                "csv" | "doc" | "docx" | "pdf" | "ppt" | "pptx" | "txt" | "xls" | "xlsx"
            ) || content_type.contains("pdf")
                || content_type.contains("msword")
                || content_type.contains("spreadsheet")
                || content_type.contains("presentation")
                || content_type.starts_with("text/plain")
        }
        "other" => ![
            "html",
            "javascript",
            "css",
            "json",
            "xml",
            "images",
            "fonts",
            "media",
            "documents",
        ]
        .iter()
        .any(|known| matches_content_type_filter(known, context)),
        _ => false,
    }
}

fn file_extension(url: &str) -> &str {
    let resource = url
        .split(['?', '#'])
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .next()
        .unwrap_or_default();
    resource
        .rsplit_once('.')
        .map(|(_, extension)| extension)
        .unwrap_or_default()
}

fn rules_match(rules: &[InterceptionRule], context: &InterceptionContext) -> bool {
    let mut result = None;
    for rule in rules.iter().filter(|rule| rule.enabled) {
        let matches = rule_matches(rule, context);
        result = Some(match result {
            None => matches,
            Some(current) if rule.operator == "or" => current || matches,
            Some(current) => current && matches,
        });
    }
    result.unwrap_or(true)
}

fn rule_matches(rule: &InterceptionRule, context: &InterceptionContext) -> bool {
    if rule.match_type == "inScope" {
        return match rule.relationship.as_str() {
            "isInScope" => context.in_scope,
            "isNotInScope" => !context.in_scope,
            _ => false,
        };
    }
    let value = match rule.match_type.as_str() {
        "url" => &context.url,
        "domain" => &context.domain,
        "ipAddress" => &context.ip_address,
        "protocol" => &context.protocol,
        "fileExtension" => file_extension(&context.url),
        "httpMethod" => &context.method,
        "contentType" => &context.content_type,
        "request" => &context.request,
        "cookieName" => &context.cookie_names,
        "cookieValue" => &context.cookie_values,
        "anyHeader" => &context.headers,
        "body" => &context.body,
        "paramName" => &context.param_names,
        "paramValue" => &context.param_values,
        "listenerPort" => &context.listener_port,
        _ => return false,
    };
    // Overlong patterns are no-match (logged inside the cache helper) so a
    // single huge regex cannot stall the proxy policy hot path.
    let regex_matches = if rule.condition.len() > MAX_REGEX_LEN {
        tracing::warn!(
            target: "witness_lib::network::interception",
            condition_len = rule.condition.len(),
            max_len = MAX_REGEX_LEN,
            "interception regex too long; treating as no-match"
        );
        false
    } else {
        cached_interception_regex(&rule.condition).is_some_and(|regex| regex.is_match(value))
    };
    let contains = rule
        .condition
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .any(|part| {
            value
                .to_ascii_lowercase()
                .contains(&part.to_ascii_lowercase())
        });
    match rule.relationship.as_str() {
        "matches" => regex_matches,
        "doesNotMatch" => !regex_matches,
        "contains" => contains,
        "doesNotContain" => !contains,
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_interception_context(
    url: &str,
    domain: &str,
    ip_address: &str,
    protocol: &str,
    method: &str,
    content_type: &str,
    headers: &::http::HeaderMap,
    body: &[u8],
    request_raw: &[u8],
    response: bool,
) -> InterceptionContext {
    let request = String::from_utf8_lossy(request_raw).into_owned();
    let (param_names, param_values) = parameter_fields(url, &request);
    let (cookie_names, cookie_values) = cookie_fields(headers, response);
    InterceptionContext {
        url: url.into(),
        domain: domain.into(),
        ip_address: ip_address.into(),
        protocol: protocol.into(),
        method: method.into(),
        content_type: content_type.into(),
        request,
        cookie_names,
        cookie_values,
        headers: headers_text(headers),
        body: String::from_utf8_lossy(body).into_owned(),
        param_names,
        param_values,
        listener_port: String::new(),
        in_scope: false,
    }
}

fn headers_text(headers: &::http::HeaderMap) -> String {
    headers
        .iter()
        .map(|(name, value)| format!("{}: {}", name.as_str(), value.to_str().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn cookie_fields(headers: &::http::HeaderMap, response: bool) -> (String, String) {
    let header_name = if response {
        header::SET_COOKIE
    } else {
        header::COOKIE
    };
    let mut names = Vec::new();
    let mut values = Vec::new();
    for value in headers.get_all(header_name).iter() {
        let raw = value.to_str().unwrap_or_default();
        let pairs = if response {
            raw.split(';').take(1).collect::<Vec<_>>()
        } else {
            raw.split(';').collect::<Vec<_>>()
        };
        for pair in pairs {
            if let Some((name, value)) = pair.trim().split_once('=') {
                names.push(name.trim());
                values.push(value.trim());
            }
        }
    }
    (names.join(","), values.join(","))
}

fn parameter_fields(url: &str, request: &str) -> (String, String) {
    let mut pairs = Vec::new();
    if let Some((_, query)) = url.split_once('?') {
        collect_form_pairs(query, &mut pairs);
    }
    let (head, body) = request
        .split_once("\r\n\r\n")
        .or_else(|| request.split_once("\n\n"))
        .unwrap_or((request, ""));
    let form_encoded = head.lines().any(|line| {
        line.to_ascii_lowercase()
            .starts_with("content-type: application/x-www-form-urlencoded")
    });
    if form_encoded {
        collect_form_pairs(body, &mut pairs);
    }
    let names = pairs
        .iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let values = pairs
        .iter()
        .map(|(_, value)| value.as_str())
        .collect::<Vec<_>>()
        .join(",");
    (names, values)
}

fn collect_form_pairs(value: &str, pairs: &mut Vec<(String, String)>) {
    for part in value.split('&').filter(|part| !part.is_empty()) {
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        let decode = |part: &str| {
            percent_decode_str(&part.replace('+', " "))
                .decode_utf8_lossy()
                .into_owned()
        };
        pairs.push((decode(name), decode(value)));
    }
}

fn literal_ip_address(host: &str) -> String {
    host.trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
        .map(|address| address.to_string())
        .unwrap_or_default()
}

fn header_has_token(headers: &::http::HeaderMap, name: header::HeaderName, token: &str) -> bool {
    headers
        .get_all(name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|value| value.trim().eq_ignore_ascii_case(token))
}

fn is_websocket_upgrade(request: &Request<Vec<u8>>) -> bool {
    request.version() == ::http::Version::HTTP_11
        && request.method() == ::http::Method::GET
        && header_has_token(request.headers(), header::CONNECTION, "upgrade")
        && header_has_token(request.headers(), header::UPGRADE, "websocket")
}

fn is_websocket_switch(response: &Response<Vec<u8>>) -> bool {
    response.status() == StatusCode::SWITCHING_PROTOCOLS
        && header_has_token(response.headers(), header::CONNECTION, "upgrade")
        && header_has_token(response.headers(), header::UPGRADE, "websocket")
}

async fn handle_websocket<C>(
    client: &mut C,
    request: Request<Vec<u8>>,
    client_remainder: Vec<u8>,
    state: &AppState,
    tls: bool,
    cancellation: &CancellationToken,
    exchange_id: String,
) -> Result<()>
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    let _operation = logging::OperationGuard::new("proxy.handle_websocket");
    let project_generation = state.project_generation.load(Ordering::Acquire);
    let started = Instant::now();
    tracing::info!(
        target: "witness_lib::network",
        phase = "websocket_handshake_started",
        exchange_id = %exchange_id,
        transport = if tls { "wss" } else { "ws" },
        "WebSocket handshake processing started"
    );
    state.traffic_stats.record_processed();
    let scheme = if tls { "wss" } else { "ws" };
    let (original_host, _) = request_destination(&request, if tls { 443 } else { 80 })?;
    let original_path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let original_url = format!("{scheme}://{original_host}{original_path}");
    let request_raw_for_rules = serialize_request(&request);
    let request = if should_intercept(
        state,
        InterceptionKind::Request,
        build_interception_context(
            &original_url,
            &original_host,
            &literal_ip_address(&original_host),
            scheme,
            request.method().as_str(),
            "",
            request.headers(),
            request.body(),
            &request_raw_for_rules,
            false,
        ),
    )
    .await
    {
        match state
            .interceptions
            .intercept_request(request_raw_for_rules, original_url, &state.event_bus)
            .await?
        {
            InterceptionResolution::Forward => {
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "request_interception_resolved",
                    exchange_id = %exchange_id,
                    action = "forward",
                    "WebSocket request interception forwarded"
                );
                request
            }
            InterceptionResolution::Drop => {
                tracing::info!(
                    target: "witness_lib::network",
                    phase = "request_dropped",
                    exchange_id = %exchange_id,
                    "WebSocket request dropped by interception"
                );
                return Ok(());
            }
            InterceptionResolution::Modify(raw) => {
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "request_modified",
                    exchange_id = %exchange_id,
                    modified_bytes = raw.len(),
                    "WebSocket request modified by interception"
                );
                parse_request(&raw)?
                    .ok_or_else(|| {
                        WitnessError::InvalidHttp(
                            "modified WebSocket handshake request is incomplete".into(),
                        )
                    })?
                    .0
            }
        }
    } else {
        request
    };
    if !is_websocket_upgrade(&request) {
        return Err(WitnessError::InvalidHttp(
            "modified WebSocket handshake no longer requests an upgrade".into(),
        ));
    }

    let (host, port) = request_destination(&request, if tls { 443 } else { 80 })?;
    let path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let url = format!("{scheme}://{host}{path}");
    let mut request = request;
    request.headers_mut().remove("proxy-connection");
    let raw_request = serialize_request(&request);
    state.traffic_stats.record_sent(raw_request.len());
    let timeout_duration = state.upstream_timeout().await;
    let upstream_proxy = state.upstream_proxy().await;
    tracing::info!(
        target: "witness_lib::network",
        phase = "request_sent_upstream",
        exchange_id = %exchange_id,
        host = %host,
        port,
        method = %request.method(),
        target = %logging::safe_url(url.as_str()),
        request_bytes = raw_request.len(),
        route = if upstream_proxy.is_some() { "upstream_proxy" } else { "direct" },
        "WebSocket handshake sent upstream"
    );
    let mut upstream = tokio::select! {
        _ = cancellation.cancelled() => return Err(WitnessError::Cancelled),
        result = timeout(
            timeout_duration,
            upstream::connect_with_context(
                upstream_proxy.as_ref(),
                &host,
                port,
                Some(&exchange_id),
            ),
        ) => result
            .map_err(|_| WitnessError::Proxy("WebSocket upstream connection timed out".into()))??,
    };
    let upstream_ip = upstream
        .peer_addr()
        .ok()
        .map(|address| address.ip().to_string());

    if tls {
        let server_name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|error| WitnessError::Tls(error.to_string()))?;
        let mut tls_config = {
            let config = state.upstream_tls_config.read().await;
            config.as_ref().clone()
        };
        // RFC 6455 uses HTTP/1.1 Upgrade, so do not negotiate h2 for this
        // dedicated upstream connection.
        tls_config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let connector = TlsConnector::from(Arc::new(tls_config));
        let mut upstream = tokio::select! {
            _ = cancellation.cancelled() => return Err(WitnessError::Cancelled),
            result = timeout(timeout_duration, connector.connect(server_name, upstream)) => result
                .map_err(|_| WitnessError::Proxy("WebSocket TLS handshake timed out".into()))?
                .map_err(|error| WitnessError::Tls(error.to_string()))?,
        };
        proxy_websocket_stream(
            client,
            &mut upstream,
            request,
            raw_request,
            client_remainder,
            url,
            host,
            upstream_ip,
            state,
            cancellation,
            started,
            &exchange_id,
            project_generation,
        )
        .await
    } else {
        proxy_websocket_stream(
            client,
            &mut upstream,
            request,
            raw_request,
            client_remainder,
            url,
            host,
            upstream_ip,
            state,
            cancellation,
            started,
            &exchange_id,
            project_generation,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn proxy_websocket_stream<C, U>(
    client: &mut C,
    upstream: &mut U,
    request: Request<Vec<u8>>,
    raw_request: Vec<u8>,
    client_remainder: Vec<u8>,
    url: String,
    host: String,
    upstream_ip: Option<String>,
    state: &AppState,
    cancellation: &CancellationToken,
    started: Instant,
    exchange_id: &str,
    project_generation: u64,
) -> Result<()>
where
    C: AsyncRead + AsyncWrite + Unpin,
    U: AsyncRead + AsyncWrite + Unpin,
{
    let _operation = logging::OperationGuard::new("proxy.proxy_websocket_stream");
    let timeout_duration = state.upstream_timeout().await;
    let (raw_upstream_response, upstream_remainder) = tokio::select! {
        _ = cancellation.cancelled() => return Err(WitnessError::Cancelled),
        result = timeout(timeout_duration, async {
            upstream.write_all(&raw_request).await?;
            read_upstream_response_with_remainder(upstream, request.method()).await
        }) => result
            .map_err(|_| WitnessError::Proxy("WebSocket upgrade timed out".into()))??,
    };
    state
        .traffic_stats
        .record_received(raw_upstream_response.len());
    let mut response = parse_response_for_method(&raw_upstream_response, request.method())?
        .ok_or_else(|| WitnessError::InvalidHttp("incomplete WebSocket handshake response".into()))?
        .0;
    let upstream_switched = is_websocket_switch(&response);
    let (status, response_headers, response_body) = logging::response_metadata(&response);
    tracing::info!(
        target: "witness_lib::network",
        phase = "response_received_upstream",
        exchange_id,
        status,
        header_count = response_headers,
        body_bytes = response_body,
        response_bytes = raw_upstream_response.len(),
        protocol = "http/1.1",
        "WebSocket handshake response received upstream"
    );
    let response_content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if should_intercept(
        state,
        InterceptionKind::Response,
        build_interception_context(
            &url,
            &host,
            upstream_ip.as_deref().unwrap_or_default(),
            if url.starts_with("wss:") { "wss" } else { "ws" },
            request.method().as_str(),
            response_content_type,
            response.headers(),
            response.body(),
            &raw_request,
            true,
        ),
    )
    .await
    {
        let raw = serialize_response(&response);
        response = match state
            .interceptions
            .intercept_response(raw, raw_request.clone(), url.clone(), &state.event_bus)
            .await?
        {
            InterceptionResolution::Forward => {
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "response_interception_resolved",
                    exchange_id,
                    action = "forward",
                    "WebSocket response interception forwarded"
                );
                response
            }
            InterceptionResolution::Drop => {
                tracing::info!(
                    target: "witness_lib::network",
                    phase = "response_dropped",
                    exchange_id,
                    "WebSocket response dropped by interception"
                );
                return Ok(());
            }
            InterceptionResolution::Modify(raw) => {
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "response_modified",
                    exchange_id,
                    modified_bytes = raw.len(),
                    "WebSocket response modified by interception"
                );
                parse_response_for_method(&raw, request.method())?
                    .ok_or_else(|| {
                        WitnessError::InvalidHttp(
                            "modified WebSocket handshake response is incomplete".into(),
                        )
                    })?
                    .0
            }
        };
    }

    let status = response.status().as_u16();
    let raw_response = serialize_response(&response);
    tracing::info!(
        target: "witness_lib::network",
        phase = "response_returned_to_client",
        exchange_id,
        status,
        response_bytes = raw_response.len(),
        duration_ms = started.elapsed().as_millis() as u64,
        "WebSocket handshake response returned to client"
    );
    state
        .event_bus
        .publish_forward(ForwardEvent {
            id: exchange_id.to_string(),
            project_generation,
            method: request.method().to_string(),
            url,
            host,
            ip: upstream_ip,
            request: raw_request,
            response: raw_response.clone(),
            status,
            duration_ms: started.elapsed().as_millis() as u64,
        })
        .await;
    client.write_all(&raw_response).await?;

    if !upstream_switched || !is_websocket_switch(&response) {
        return Ok(());
    }
    if !upstream_remainder.is_empty() {
        client.write_all(&upstream_remainder).await?;
    }
    if !client_remainder.is_empty() {
        upstream.write_all(&client_remainder).await?;
    }
    tokio::select! {
        _ = cancellation.cancelled() => {
            tracing::debug!(
                target: "witness_lib::network",
                phase = "websocket_tunnel_cancelled",
                exchange_id,
                "WebSocket tunnel cancelled"
            );
            Ok(())
        },
        result = copy_bidirectional(client, upstream) => {
            let (client_to_upstream, upstream_to_client) = result?;
            tracing::info!(
                target: "witness_lib::network",
                phase = "websocket_tunnel_closed",
                exchange_id,
                client_to_upstream,
                upstream_to_client,
                duration_ms = started.elapsed().as_millis() as u64,
                "WebSocket tunnel closed"
            );
            Ok(())
        }
    }
}

async fn forward_request(
    request: Request<Vec<u8>>,
    state: &AppState,
    tls: bool,
    cancellation: &CancellationToken,
    exchange_id: &str,
) -> Result<Option<Response<Vec<u8>>>> {
    let started = Instant::now();
    let result = forward_request_inner(request, state, tls, cancellation, exchange_id).await;
    match &result {
        Ok(Some(response)) => tracing::debug!(
            target: "witness_lib::network",
            phase = "forwarding_completed",
            exchange_id,
            status = response.status().as_u16(),
            duration_ms = started.elapsed().as_millis() as u64,
            "request forwarding completed"
        ),
        Ok(None) => tracing::info!(
            target: "witness_lib::network",
            phase = "forwarding_completed_without_response",
            exchange_id,
            duration_ms = started.elapsed().as_millis() as u64,
            "request forwarding completed without a response"
        ),
        Err(error) => tracing::error!(
            target: "witness_lib::network",
            phase = "forwarding_failed",
            exchange_id,
            duration_ms = started.elapsed().as_millis() as u64,
            error = %error,
            "request forwarding failed"
        ),
    }
    result
}

async fn forward_request_inner(
    request: Request<Vec<u8>>,
    state: &AppState,
    tls: bool,
    cancellation: &CancellationToken,
    exchange_id: &str,
) -> Result<Option<Response<Vec<u8>>>> {
    let _operation = logging::OperationGuard::new("proxy.forward_request");
    let project_generation = state.project_generation.load(Ordering::Acquire);
    let started = Instant::now();
    let (received_method, received_target, received_headers, received_body) =
        logging::request_metadata(&request);
    tracing::info!(
        target: "witness_lib::network",
        phase = "forwarding_started",
        exchange_id,
        transport = if tls { "https" } else { "http" },
        method = %received_method,
        target = %received_target,
        header_count = received_headers,
        body_bytes = received_body,
        headers = %logging::header_names(request.headers()),
        "request forwarding started"
    );
    state.traffic_stats.record_processed();
    let (original_host, _) = request_destination(&request, if tls { 443 } else { 80 })?;
    let scheme = if tls { "https" } else { "http" };
    let original_path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let original_url = format!("{scheme}://{original_host}{original_path}");
    let request_content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let request_raw_for_rules = serialize_request(&request);
    let should_intercept_request = should_intercept(
        state,
        InterceptionKind::Request,
        build_interception_context(
            &original_url,
            &original_host,
            &literal_ip_address(&original_host),
            scheme,
            request.method().as_str(),
            request_content_type,
            request.headers(),
            request.body(),
            &request_raw_for_rules,
            false,
        ),
    )
    .await;
    tracing::debug!(
        target: "witness_lib::network",
        phase = "request_interception_evaluated",
        exchange_id,
        enabled = should_intercept_request,
        "request interception decision evaluated"
    );
    let mut request = if should_intercept_request {
        match state
            .interceptions
            .intercept_request(request_raw_for_rules, original_url, &state.event_bus)
            .await?
        {
            InterceptionResolution::Forward => {
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "request_interception_resolved",
                    exchange_id,
                    action = "forward",
                    "request interception forwarded"
                );
                request
            }
            InterceptionResolution::Drop => {
                tracing::info!(
                    target: "witness_lib::network",
                    phase = "request_dropped",
                    exchange_id,
                    reason = "request_interception",
                    "request dropped by interception"
                );
                return Ok(None);
            }
            InterceptionResolution::Modify(raw) => {
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "request_modified",
                    exchange_id,
                    modified_bytes = raw.len(),
                    "request modified by interception"
                );
                parse_request(&raw)?
                    .ok_or_else(|| {
                        WitnessError::InvalidHttp("modified request is incomplete".into())
                    })?
                    .0
            }
        }
    } else {
        request
    };
    // Automatic match/replace for requests — configured in Proxy settings below Traffic handling
    {
        let rules = state.settings.read().await.match_replace_rules.clone();
        if !rules.is_empty() {
            let before = serialize_request(&request).len();
            match_replace::apply_to_request(&mut request, &rules);
            let after = serialize_request(&request).len();
            if before != after {
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "request_match_replace_applied",
                    exchange_id,
                    before_bytes = before,
                    after_bytes = after,
                    "request match/replace rules applied"
                );
            }
        }
    }
    let (host, port) = request_destination(&request, if tls { 443 } else { 80 })?;
    let path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let url = format!("{scheme}://{host}{path}");
    let request_method = request.method().clone();
    let method = request_method.to_string();
    let mut request = apply_request_action(ForwardAction::Forward(request))?;
    request.headers_mut().remove("proxy-connection");
    request
        .headers_mut()
        .insert(header::CONNECTION, HeaderValue::from_static("close"));
    let raw_request = serialize_request(&request);
    state.traffic_stats.record_sent(raw_request.len());

    let timeout_duration = state.upstream_timeout().await;
    let upstream_proxy = state.upstream_proxy().await;
    tracing::info!(
        target: "witness_lib::network",
        phase = "upstream_request_prepared",
        exchange_id,
        host = %host,
        port,
        method = %method,
        target = %logging::safe_url(url.as_str()),
        request_bytes = raw_request.len(),
        timeout_ms = timeout_duration.as_millis() as u64,
        route = if upstream_proxy.is_some() { "upstream_proxy" } else { "direct" },
        "upstream request prepared"
    );
    let upstream_response = async {
        if tls && upstream_proxy.is_none() {
            let http3_port = {
                let origins = state.http3_origins.read().await;
                origins.get(&host).copied()
            };
            if let Some(http3_port) = http3_port {
                let http3_timeout = timeout_duration.min(std::time::Duration::from_secs(3));
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "http3_attempt_started",
                    exchange_id,
                    host = %host,
                    port = http3_port,
                    timeout_ms = http3_timeout.as_millis() as u64,
                    "HTTP/3 upstream attempt started"
                );
                match timeout(
                    http3_timeout,
                    send_http3_upstream_with_context(
                        state,
                        &request,
                        &host,
                        http3_port,
                        Some(exchange_id),
                    ),
                )
                .await
                {
                    Ok(Ok(result)) => {
                        tracing::info!(
                            target: "witness_lib::network",
                            phase = "http3_selected",
                            exchange_id,
                            host = %host,
                            port = http3_port,
                            "HTTP/3 upstream request completed"
                        );
                        return Ok::<_, WitnessError>(result);
                    }
                    Ok(Err(error)) => {
                        tracing::debug!(
                            target: "witness_lib::network",
                            phase = "http3_fallback",
                            exchange_id,
                            %host,
                            error = %error,
                            reason = "request_failed",
                            "HTTP/3 upstream failed; falling back"
                        );
                        {
                            let mut origins = state.http3_origins.write().await;
                            origins.remove(&host);
                        }
                    }
                    Err(_) => {
                        tracing::debug!(
                            target: "witness_lib::network",
                            phase = "http3_fallback",
                            exchange_id,
                            %host,
                            reason = "timeout",
                            "HTTP/3 upstream timed out; falling back"
                        );
                        {
                            let mut origins = state.http3_origins.write().await;
                            origins.remove(&host);
                        }
                    }
                }
            }
        }
        timeout(timeout_duration, async {
            tracing::debug!(
                target: "witness_lib::network",
                phase = "upstream_connect_started",
                exchange_id,
                host = %host,
                port,
                route = if upstream_proxy.is_some() { "upstream_proxy" } else { "direct" },
                "upstream connection started"
            );
            let stream = upstream::connect_with_context(
                upstream_proxy.as_ref(),
                &host,
                port,
                Some(exchange_id),
            )
            .await?;
            let ip = stream
                .peer_addr()
                .ok()
                .map(|address| address.ip().to_string());
            tracing::info!(
                target: "witness_lib::network",
                phase = "upstream_connected",
                exchange_id,
                host = %host,
                port,
                ip = ip.as_deref().unwrap_or("unknown"),
                "upstream TCP connection established"
            );
            if tls {
                let server_name = rustls::pki_types::ServerName::try_from(host.clone())
                    .map_err(|error| WitnessError::Tls(error.to_string()))?;
                let tls_config = {
                    let config = state.upstream_tls_config.read().await;
                    config.clone()
                };
                let connector = TlsConnector::from(tls_config);
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "upstream_tls_handshake_started",
                    exchange_id,
                    host = %host,
                    "upstream TLS handshake started"
                );
                let mut stream = connector
                    .connect(server_name, stream)
                    .await
                    .map_err(|error| WitnessError::Tls(error.to_string()))?;
                let alpn = stream
                    .get_ref()
                    .1
                    .alpn_protocol()
                    .map(|protocol| String::from_utf8_lossy(protocol).into_owned())
                    .unwrap_or_else(|| "none".into());
                tracing::info!(
                    target: "witness_lib::network",
                    phase = "upstream_tls_handshake_completed",
                    exchange_id,
                    host = %host,
                    alpn = %alpn,
                    "upstream TLS handshake completed"
                );
                let response = if alpn == "h2" {
                    tracing::info!(
                        target: "witness_lib::network",
                        phase = "upstream_protocol_selected",
                        exchange_id,
                        protocol = "h2",
                        "HTTP/2 selected for upstream request"
                    );
                    send_http2_upstream_with_context(
                        stream,
                        request,
                        &host,
                        port,
                        Some(exchange_id),
                    )
                    .await?
                } else {
                    tracing::info!(
                        target: "witness_lib::network",
                        phase = "upstream_protocol_selected",
                        exchange_id,
                        protocol = "http/1.1",
                        "HTTP/1.1 selected for upstream request"
                    );
                    stream.write_all(&raw_request).await?;
                    tracing::debug!(
                        target: "witness_lib::network",
                        phase = "request_sent_upstream",
                        exchange_id,
                        protocol = "http/1.1",
                        request_bytes = raw_request.len(),
                        "request bytes sent upstream"
                    );
                    let raw = read_upstream_response(&mut stream, &request_method).await?;
                    tracing::debug!(
                        target: "witness_lib::network",
                        phase = "response_received_upstream",
                        exchange_id,
                        protocol = "http/1.1",
                        response_bytes = raw.len(),
                        "response bytes received upstream"
                    );
                    parse_response_for_method(&raw, &request_method)?
                        .ok_or_else(|| {
                            WitnessError::InvalidHttp("incomplete upstream response".into())
                        })?
                        .0
                };
                Ok::<_, WitnessError>((response, ip))
            } else {
                let mut stream = stream;
                tracing::info!(
                    target: "witness_lib::network",
                    phase = "upstream_protocol_selected",
                    exchange_id,
                    protocol = "http/1.1",
                    "HTTP/1.1 selected for upstream request"
                );
                stream.write_all(&raw_request).await?;
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "request_sent_upstream",
                    exchange_id,
                    protocol = "http/1.1",
                    request_bytes = raw_request.len(),
                    "request bytes sent upstream"
                );
                let raw = read_upstream_response(&mut stream, &request_method).await?;
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "response_received_upstream",
                    exchange_id,
                    protocol = "http/1.1",
                    response_bytes = raw.len(),
                    "response bytes received upstream"
                );
                let response = parse_response_for_method(&raw, &request_method)?
                    .ok_or_else(|| {
                        WitnessError::InvalidHttp("incomplete upstream response".into())
                    })?
                    .0;
                Ok::<_, WitnessError>((response, ip))
            }
        })
        .await
        .map_err(|_| WitnessError::Proxy("upstream request timed out".into()))?
    };

    let (mut response, upstream_ip) = tokio::select! {
        _ = cancellation.cancelled() => return Err(WitnessError::Cancelled),
        result = upstream_response => result?,
    };
    let (upstream_status, upstream_headers, upstream_body) = logging::response_metadata(&response);
    tracing::info!(
        target: "witness_lib::network",
        phase = "response_received_upstream",
        exchange_id,
        status = upstream_status,
        header_count = upstream_headers,
        body_bytes = upstream_body,
        ip = upstream_ip.as_deref().unwrap_or("unknown"),
        "upstream response received"
    );
    state
        .traffic_stats
        .record_received(serialize_response(&response).len());
    if tls {
        update_http3_origin(state, &host, port, response.headers()).await;
    }
    let compression_mode = state.settings.read().await.compression_mode.clone();
    response = apply_response_compression(response, &compression_mode)?;
    // Automatic match/replace for responses — after decompression, before interception
    {
        let rules = state.settings.read().await.match_replace_rules.clone();
        if !rules.is_empty() {
            let before = serialize_response(&response).len();
            match_replace::apply_to_response(&mut response, &rules);
            let after = serialize_response(&response).len();
            if before != after {
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "response_match_replace_applied",
                    exchange_id,
                    before_bytes = before,
                    after_bytes = after,
                    "response match/replace rules applied"
                );
            }
        }
    }
    response.headers_mut().remove(header::CONNECTION);
    let response_content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    if should_intercept(
        state,
        InterceptionKind::Response,
        build_interception_context(
            &url,
            &host,
            upstream_ip.as_deref().unwrap_or_default(),
            scheme,
            request_method.as_str(),
            &response_content_type,
            response.headers(),
            response.body(),
            &raw_request,
            true,
        ),
    )
    .await
    {
        let raw = serialize_response(&response);
        response = match state
            .interceptions
            .intercept_response(raw, raw_request.clone(), url.clone(), &state.event_bus)
            .await?
        {
            InterceptionResolution::Forward => {
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "response_interception_resolved",
                    exchange_id,
                    action = "forward",
                    "response interception forwarded"
                );
                response
            }
            InterceptionResolution::Drop => {
                tracing::info!(
                    target: "witness_lib::network",
                    phase = "response_dropped",
                    exchange_id,
                    reason = "response_interception",
                    "response dropped by interception"
                );
                return Ok(None);
            }
            InterceptionResolution::Modify(raw) => {
                tracing::debug!(
                    target: "witness_lib::network",
                    phase = "response_modified",
                    exchange_id,
                    modified_bytes = raw.len(),
                    "response modified by interception"
                );
                parse_response_for_method(&raw, &request_method)?
                    .ok_or_else(|| {
                        WitnessError::InvalidHttp("modified response is incomplete".into())
                    })?
                    .0
            }
        };
    }
    let status = response.status().as_u16();
    let response = apply_response_action(ForwardAction::Forward(response))?;
    let raw_response = serialize_response(&response);

    tracing::info!(
        target: "witness_lib::network",
        phase = "response_returned_to_client",
        exchange_id,
        method = %method,
        url = %logging::safe_url(url.as_str()),
        host = %host,
        ip = upstream_ip.as_deref().unwrap_or("unknown"),
        status,
        response_bytes = raw_response.len(),
        duration_ms = started.elapsed().as_millis() as u64,
        "request forwarded and response prepared for client"
    );
    state
        .event_bus
        .publish_forward(ForwardEvent {
            id: exchange_id.to_string(),
            project_generation,
            method,
            url,
            host,
            ip: upstream_ip,
            request: raw_request,
            response: raw_response.clone(),
            status,
            duration_ms: started.elapsed().as_millis() as u64,
        })
        .await;
    Ok(Some(response))
}

pub(crate) async fn send_http2_upstream<T>(
    stream: T,
    request: Request<Vec<u8>>,
    host: &str,
    port: u16,
) -> Result<Response<Vec<u8>>>
where
    T: AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    send_http2_upstream_with_context(stream, request, host, port, None).await
}

pub(crate) async fn send_http2_upstream_with_context<T>(
    stream: T,
    request: Request<Vec<u8>>,
    host: &str,
    port: u16,
    exchange_id: Option<&str>,
) -> Result<Response<Vec<u8>>>
where
    T: AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let exchange_id = exchange_id.unwrap_or("untracked");
    let (method, target, header_count, body_bytes) = logging::request_metadata(&request);
    let started = Instant::now();
    tracing::info!(
        target: "witness_lib::network::http2",
        phase = "upstream_request_started",
        exchange_id,
        host,
        port,
        method = %method,
        target = %target,
        header_count,
        body_bytes,
        "HTTP/2 upstream request started"
    );
    let authority = request
        .uri()
        .authority()
        .map(|value| value.as_str().to_owned())
        .or_else(|| {
            request
                .headers()
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| format_authority(host, port, 443));
    let path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let uri: ::http::Uri = format!("https://{authority}{path}")
        .parse()
        .map_err(|error| WitnessError::InvalidHttp(format!("invalid HTTP/2 URI: {error}")))?;

    let (mut parts, body) = request.into_parts();
    parts.uri = uri;
    parts.version = ::http::Version::HTTP_2;
    remove_connection_headers(&mut parts.headers);
    parts.headers.remove(header::HOST);
    let request = Request::from_parts(parts, ());

    let (mut sender, connection) = h2::client::handshake(stream).await.map_err(http2_error)?;
    tracing::debug!(
        target: "witness_lib::network::http2",
        phase = "connection_handshake_completed",
        exchange_id,
        host,
        port,
        "HTTP/2 upstream connection handshake completed"
    );
    let driver = tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::debug!(%error, "upstream HTTP/2 connection closed");
        }
    });

    let result = async {
        sender = sender.ready().await.map_err(http2_error)?;
        let end_stream = body.is_empty();
        let (response, mut request_body) = sender
            .send_request(request, end_stream)
            .map_err(http2_error)?;
        tracing::debug!(
            target: "witness_lib::network::http2",
            phase = "request_headers_sent",
            exchange_id,
            host,
            port,
            body_bytes,
            end_stream,
            "HTTP/2 upstream request headers sent"
        );
        if !end_stream {
            send_http2_body(&mut request_body, Bytes::from(body)).await?;
            tracing::debug!(
                target: "witness_lib::network::http2",
                phase = "request_body_sent",
                exchange_id,
                host,
                port,
                body_bytes,
                "HTTP/2 upstream request body sent"
            );
        }
        let response = response.await.map_err(http2_error)?;
        let status = response.status().as_u16();
        let response_headers = response.headers().len();
        let (mut parts, body) = response.into_parts();
        parts.version = ::http::Version::HTTP_2;
        let body = collect_http2_body(body).await?;
        tracing::debug!(
            target: "witness_lib::network::http2",
            phase = "response_body_received",
            exchange_id,
            host,
            port,
            status,
            header_count = response_headers,
            body_bytes = body.len(),
            "HTTP/2 upstream response body received"
        );
        Ok(Response::from_parts(parts, body))
    }
    .await;
    driver.abort();
    match &result {
        Ok(response) => tracing::info!(
            target: "witness_lib::network::http2",
            phase = "upstream_request_completed",
            exchange_id,
            host,
            port,
            status = response.status().as_u16(),
            response_bytes = serialize_response(response).len(),
            duration_ms = started.elapsed().as_millis() as u64,
            "HTTP/2 upstream request completed"
        ),
        Err(error) => tracing::warn!(
            target: "witness_lib::network::http2",
            phase = "upstream_request_failed",
            exchange_id,
            host,
            port,
            error = %error,
            duration_ms = started.elapsed().as_millis() as u64,
            "HTTP/2 upstream request failed"
        ),
    }
    result
}

async fn send_http3_upstream_with_context(
    state: &AppState,
    request: &Request<Vec<u8>>,
    host: &str,
    port: u16,
    exchange_id: Option<&str>,
) -> Result<(Response<Vec<u8>>, Option<String>)> {
    let exchange_id = exchange_id.unwrap_or("untracked");
    let (method, target, header_count, body_bytes) = logging::request_metadata(request);
    let started = Instant::now();
    tracing::info!(
        target: "witness_lib::network::http3",
        phase = "upstream_request_started",
        exchange_id,
        host,
        port,
        method = %method,
        target = %target,
        header_count,
        body_bytes,
        "HTTP/3 upstream request started"
    );
    let address = tokio::net::lookup_host((host, port))
        .await?
        .next()
        .ok_or_else(|| WitnessError::Proxy(format!("HTTP/3 origin {host} did not resolve")))?;
    tracing::debug!(
        target: "witness_lib::network::http3",
        phase = "dns_resolved",
        exchange_id,
        host,
        port,
        address = %address,
        "HTTP/3 origin address resolved"
    );
    let bind_address = if address.is_ipv6() {
        "[::]:0".parse().expect("static IPv6 bind address")
    } else {
        "0.0.0.0:0".parse().expect("static IPv4 bind address")
    };
    let mut endpoint = quinn::Endpoint::client(bind_address)
        .map_err(|error| WitnessError::Proxy(format!("HTTP/3 endpoint error: {error}")))?;
    let mut tls_config = {
        let config = state.upstream_tls_config.read().await;
        config.as_ref().clone()
    };
    tls_config.alpn_protocols = vec![b"h3".to_vec()];
    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls_config)
        .map_err(|error| WitnessError::Tls(format!("HTTP/3 TLS configuration error: {error}")))?;
    endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(quic_crypto)));
    let connection = endpoint
        .connect(address, host)
        .map_err(|error| WitnessError::Proxy(format!("HTTP/3 connection error: {error}")))?
        .await
        .map_err(|error| WitnessError::Proxy(format!("HTTP/3 handshake error: {error}")))?;
    tracing::debug!(
        target: "witness_lib::network::http3",
        phase = "connection_handshake_completed",
        exchange_id,
        host,
        port,
        address = %address,
        "HTTP/3 QUIC handshake completed"
    );
    let quic = h3_quinn::Connection::new(connection);
    let (mut driver, mut sender) = h3::client::new(quic)
        .await
        .map_err(|error| WitnessError::Proxy(format!("HTTP/3 setup error: {error}")))?;
    let driver_task = tokio::spawn(async move {
        let error = poll_fn(|context| driver.poll_close(context)).await;
        tracing::debug!(%error, "upstream HTTP/3 connection closed");
    });

    let authority = request
        .uri()
        .authority()
        .map(|value| value.as_str().to_owned())
        .or_else(|| {
            request
                .headers()
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| format_authority(host, port, 443));
    let path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let uri: ::http::Uri = format!("https://{authority}{path}")
        .parse()
        .map_err(|error| WitnessError::InvalidHttp(format!("invalid HTTP/3 URI: {error}")))?;
    let mut builder = Request::builder()
        .method(request.method().clone())
        .uri(uri)
        .version(::http::Version::HTTP_3);
    let headers = builder
        .headers_mut()
        .ok_or_else(|| WitnessError::InvalidHttp("invalid HTTP/3 request builder".into()))?;
    *headers = request.headers().clone();
    remove_connection_headers(headers);
    headers.remove(header::HOST);
    let request_head = builder
        .body(())
        .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?;
    let body = Bytes::copy_from_slice(request.body());

    let result =
        async {
            let mut stream = sender
                .send_request(request_head)
                .await
                .map_err(|error| WitnessError::Proxy(format!("HTTP/3 request error: {error}")))?;
            tracing::debug!(
                target: "witness_lib::network::http3",
                phase = "request_headers_sent",
                exchange_id,
                host,
                port,
                body_bytes,
                "HTTP/3 request headers sent"
            );
            if !body.is_empty() {
                stream.send_data(body).await.map_err(|error| {
                    WitnessError::Proxy(format!("HTTP/3 body send error: {error}"))
                })?;
                tracing::debug!(
                    target: "witness_lib::network::http3",
                    phase = "request_body_sent",
                    exchange_id,
                    host,
                    port,
                    body_bytes,
                    "HTTP/3 request body sent"
                );
            }
            stream.finish().await.map_err(|error| {
                WitnessError::Proxy(format!("HTTP/3 request finish error: {error}"))
            })?;
            let response = stream
                .recv_response()
                .await
                .map_err(|error| WitnessError::Proxy(format!("HTTP/3 response error: {error}")))?;
            let status = response.status().as_u16();
            let response_headers = response.headers().len();
            let (mut parts, _) = response.into_parts();
            parts.version = ::http::Version::HTTP_3;
            let mut body = Vec::new();
            while let Some(mut chunk) = stream.recv_data().await.map_err(|error| {
                WitnessError::Proxy(format!("HTTP/3 body receive error: {error}"))
            })? {
                if body.len().saturating_add(chunk.remaining()) > MAX_MESSAGE_SIZE {
                    return Err(WitnessError::InvalidHttp(
                        "HTTP/3 message body exceeds 100 MiB".into(),
                    ));
                }
                while chunk.has_remaining() {
                    let length = chunk.remaining();
                    body.extend_from_slice(&chunk.copy_to_bytes(length));
                }
            }
            tracing::debug!(
                target: "witness_lib::network::http3",
                phase = "response_body_received",
                exchange_id,
                host,
                port,
                status,
                header_count = response_headers,
                body_bytes = body.len(),
                "HTTP/3 response body received"
            );
            Ok(Response::from_parts(parts, body))
        }
        .await;
    endpoint.close(quinn::VarInt::from_u32(0), b"request complete");
    driver_task.abort();
    match &result {
        Ok(response) => tracing::info!(
            target: "witness_lib::network::http3",
            phase = "upstream_request_completed",
            exchange_id,
            host,
            port,
            status = response.status().as_u16(),
            response_bytes = serialize_response(response).len(),
            duration_ms = started.elapsed().as_millis() as u64,
            "HTTP/3 upstream request completed"
        ),
        Err(error) => tracing::warn!(
            target: "witness_lib::network::http3",
            phase = "upstream_request_failed",
            exchange_id,
            host,
            port,
            error = %error,
            duration_ms = started.elapsed().as_millis() as u64,
            "HTTP/3 upstream request failed"
        ),
    }
    result.map(|response| (response, Some(address.ip().to_string())))
}

enum Http3Advertisement {
    Unchanged,
    Remove,
    Port(u16),
}

fn parse_http3_advertisement(
    headers: &::http::HeaderMap,
    origin_host: &str,
    default_port: u16,
) -> Http3Advertisement {
    for value in headers.get_all("alt-svc").iter() {
        let Ok(value) = value.to_str() else {
            continue;
        };
        if value.trim().eq_ignore_ascii_case("clear") {
            return Http3Advertisement::Remove;
        }
        for alternative in value.split(',') {
            let mut segments = alternative.split(';');
            let Some(service) = segments.next() else {
                continue;
            };
            let Some((protocol, authority)) = service.trim().split_once('=') else {
                continue;
            };
            if !protocol.trim().eq_ignore_ascii_case("h3") {
                continue;
            }
            if segments.any(|parameter| {
                parameter
                    .trim()
                    .split_once('=')
                    .is_some_and(|(name, value)| {
                        name.trim().eq_ignore_ascii_case("ma") && value.trim() == "0"
                    })
            }) {
                return Http3Advertisement::Remove;
            }
            let authority = authority.trim().trim_matches('"');
            let port = if let Some(port) = authority.strip_prefix(':') {
                port.parse().ok()
            } else if let Some((advertised_host, port)) = authority.rsplit_once(':') {
                advertised_host
                    .trim_matches(['[', ']'])
                    .eq_ignore_ascii_case(origin_host)
                    .then(|| port.parse().ok())
                    .flatten()
            } else if authority.is_empty() || authority.eq_ignore_ascii_case(origin_host) {
                Some(default_port)
            } else {
                None
            };
            if let Some(port) = port {
                return Http3Advertisement::Port(port);
            }
        }
    }
    Http3Advertisement::Unchanged
}

async fn update_http3_origin(
    state: &AppState,
    host: &str,
    default_port: u16,
    headers: &::http::HeaderMap,
) {
    match parse_http3_advertisement(headers, host, default_port) {
        Http3Advertisement::Unchanged => {}
        Http3Advertisement::Remove => {
            state.http3_origins.write().await.remove(host);
            tracing::info!(
                target: "witness_lib::network::http3",
                phase = "origin_advertisement_removed",
                host,
                "HTTP/3 origin advertisement removed"
            );
        }
        Http3Advertisement::Port(port) => {
            state
                .http3_origins
                .write()
                .await
                .insert(host.to_owned(), port);
            tracing::info!(
                target: "witness_lib::network::http3",
                phase = "origin_advertisement_updated",
                host,
                port,
                "HTTP/3 origin advertisement updated"
            );
        }
    }
}

async fn send_http2_body(stream: &mut h2::SendStream<Bytes>, mut body: Bytes) -> Result<()> {
    stream.reserve_capacity(body.remaining());
    while body.has_remaining() {
        let capacity = poll_fn(|context| stream.poll_capacity(context))
            .await
            .ok_or_else(|| WitnessError::Http2 {
                message: "stream closed while sending body".into(),
                client_cancelled: true,
            })?
            .map_err(http2_error)?;
        if capacity == 0 {
            continue;
        }
        let length = capacity.min(body.remaining());
        let chunk = body.split_to(length);
        stream
            .send_data(chunk, body.is_empty())
            .map_err(http2_error)?;
    }
    Ok(())
}

fn remove_connection_headers(headers: &mut ::http::HeaderMap) {
    let nominated: Vec<_> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| ::http::HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect();
    for name in nominated {
        headers.remove(name);
    }
    for name in [
        header::CONNECTION,
        header::TRANSFER_ENCODING,
        header::UPGRADE,
        header::TE,
    ] {
        headers.remove(name);
    }
    headers.remove("proxy-connection");
    headers.remove("keep-alive");
    headers.remove("http2-settings");
}

fn format_authority(host: &str, port: u16, default_port: u16) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    if port == default_port {
        host
    } else {
        format!("{host}:{port}")
    }
}

fn http2_error(error: h2::Error) -> WitnessError {
    let client_cancelled = error.is_reset() || error.reason() == Some(h2::Reason::CANCEL);
    WitnessError::Http2 {
        message: error.to_string(),
        client_cancelled,
    }
}

pub fn apply_request_action(action: ForwardAction<Request<Vec<u8>>>) -> Result<Request<Vec<u8>>> {
    match action {
        ForwardAction::Forward(request) | ForwardAction::Modify(request) => Ok(request),
        ForwardAction::Drop => Err(WitnessError::Cancelled),
    }
}

pub fn apply_response_action(
    action: ForwardAction<Response<Vec<u8>>>,
) -> Result<Response<Vec<u8>>> {
    match action {
        ForwardAction::Forward(response) | ForwardAction::Modify(response) => Ok(response),
        ForwardAction::Drop => Err(WitnessError::Cancelled),
    }
}

async fn read_request<R>(stream: &mut R, buffer: &mut Vec<u8>) -> Result<Option<Request<Vec<u8>>>>
where
    R: AsyncRead + Unpin,
{
    loop {
        if let Some((request, consumed)) = parse_request(buffer)? {
            buffer.drain(..consumed);
            return Ok(Some(request));
        }
        if buffer.len() >= MAX_MESSAGE_SIZE {
            return Err(WitnessError::InvalidHttp("request exceeds 100 MiB".into()));
        }
        let mut chunk = [0_u8; 16 * 1024];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return if buffer.is_empty() {
                Ok(None)
            } else {
                Err(WitnessError::InvalidHttp(
                    "unexpected end of request".into(),
                ))
            };
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

#[cfg(test)]
async fn read_to_end_limited<R>(stream: &mut R) -> Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > MAX_MESSAGE_SIZE {
            return Err(WitnessError::InvalidHttp("response exceeds 100 MiB".into()));
        }
        output.extend_from_slice(&chunk[..read]);
    }
}

async fn read_upstream_response<R>(
    stream: &mut R,
    request_method: &::http::Method,
) -> Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    Ok(
        read_upstream_response_with_remainder(stream, request_method)
            .await?
            .0,
    )
}

async fn read_upstream_response_with_remainder<R>(
    stream: &mut R,
    request_method: &::http::Method,
) -> Result<(Vec<u8>, Vec<u8>)>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        if let Some((response, consumed)) = parse_response_for_method(&output, request_method)? {
            if response.status().is_informational()
                && response.status() != StatusCode::SWITCHING_PROTOCOLS
            {
                output.drain(..consumed);
                continue;
            }
            let explicitly_framed = request_method == ::http::Method::HEAD
                || response.status().is_informational()
                || response.status() == StatusCode::NO_CONTENT
                || response.status() == StatusCode::NOT_MODIFIED
                || response.headers().contains_key(header::CONTENT_LENGTH)
                || is_chunked(response.headers());
            if explicitly_framed {
                let remainder = output.split_off(consumed);
                return Ok((output, remainder));
            }
        }

        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok((output, Vec::new()));
        }
        if output.len().saturating_add(read) > MAX_MESSAGE_SIZE {
            return Err(WitnessError::InvalidHttp("response exceeds 100 MiB".into()));
        }
        output.extend_from_slice(&chunk[..read]);
    }
}

fn request_destination(request: &Request<Vec<u8>>, default_port: u16) -> Result<(String, u16)> {
    let authority = request
        .uri()
        .authority()
        .map(|value| value.as_str())
        .or_else(|| {
            request
                .headers()
                .get(header::HOST)
                .and_then(|value| value.to_str().ok())
        })
        .ok_or_else(|| WitnessError::InvalidHttp("request has no Host".into()))?;
    split_host_port(authority, default_port)
}

/// Canonical `host[:port]` splitter shared by the proxy and repeater.
/// `pub(crate)` so `repeater::parse_authority` can delegate instead of
/// duplicating IPv6/port logic.
pub(crate) fn split_host_port(authority: &str, default_port: u16) -> Result<(String, u16)> {
    if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| WitnessError::InvalidHttp("invalid IPv6 authority".into()))?;
        let host = authority[1..end].to_string();
        let port = authority[end + 1..]
            .strip_prefix(':')
            .map(|value| value.parse())
            .transpose()
            .map_err(|_| WitnessError::InvalidHttp("invalid port".into()))?
            .unwrap_or(default_port);
        return Ok((host, port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => Ok((
            host.to_string(),
            port.parse()
                .map_err(|_| WitnessError::InvalidHttp("invalid port".into()))?,
        )),
        _ => Ok((authority.to_string(), default_port)),
    }
}

pub fn gateway_timeout_response() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::GATEWAY_TIMEOUT)
        .body(b"Gateway Timeout".to_vec())
        .expect("static gateway timeout response")
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::event_bus::{EventCategory, ProxyEvent};
    use flate2::{write::GzEncoder, Compression};
    use rustls::{
        pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
        ClientConfig, RootCertStore, ServerConfig,
    };

    #[test]
    fn parses_destinations() {
        assert_eq!(
            split_host_port("example.test:8443", 443).unwrap(),
            ("example.test".into(), 8443)
        );
        assert_eq!(
            split_host_port("[::1]:8080", 80).unwrap(),
            ("::1".into(), 8080)
        );
    }

    #[test]
    fn discovers_and_clears_http3_alt_svc() {
        let mut headers = ::http::HeaderMap::new();
        headers.insert(
            "alt-svc",
            HeaderValue::from_static("h2=\":443\"; ma=60, h3=\":8443\"; ma=3600"),
        );
        assert!(matches!(
            parse_http3_advertisement(&headers, "example.test", 443),
            Http3Advertisement::Port(8443)
        ));

        headers.insert("alt-svc", HeaderValue::from_static("h3=\":443\"; ma=0"));
        assert!(matches!(
            parse_http3_advertisement(&headers, "example.test", 443),
            Http3Advertisement::Remove
        ));

        headers.insert(
            "alt-svc",
            HeaderValue::from_static("h3=\"other.example:443\"; ma=3600"),
        );
        assert!(matches!(
            parse_http3_advertisement(&headers, "example.test", 443),
            Http3Advertisement::Unchanged
        ));
    }

    #[tokio::test]
    async fn http3_request_round_trips_to_advertised_upstream() {
        let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let certificate = certified.cert.der().clone();
        let private_key =
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der()));
        let mut server_tls = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate.clone()], private_key)
            .unwrap();
        server_tls.alpn_protocols = vec![b"h3".to_vec()];
        let server_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(server_tls).unwrap();
        let server_config = quinn::ServerConfig::with_crypto(Arc::new(server_crypto));
        let endpoint =
            quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap()).unwrap();
        let address = endpoint.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let connection = endpoint.accept().await.unwrap().await.unwrap();
            let mut connection = h3::server::Connection::new(h3_quinn::Connection::new(connection))
                .await
                .unwrap();
            let resolver = connection.accept().await.unwrap().unwrap();
            let (request, mut stream) = resolver.resolve_request().await.unwrap();
            assert_eq!(request.version(), ::http::Version::HTTP_3);
            assert_eq!(request.uri().path(), "/over-h3");
            while stream.recv_data().await.unwrap().is_some() {}
            stream
                .send_response(
                    Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "text/plain")
                        .body(())
                        .unwrap(),
                )
                .await
                .unwrap();
            stream
                .send_data(Bytes::from_static(b"http3-ok"))
                .await
                .unwrap();
            stream.finish().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        });

        let mut roots = RootCertStore::empty();
        roots.add(certificate).unwrap();
        let mut client_tls = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        client_tls.alpn_protocols = vec![b"h3".to_vec()];
        let state = AppState::new();
        *state.upstream_tls_config.write().await = Arc::new(client_tls);
        let request = Request::builder()
            .method("GET")
            .uri(format!("https://127.0.0.1:{}/over-h3", address.port()))
            .header("Host", format!("127.0.0.1:{}", address.port()))
            .body(Vec::new())
            .unwrap();

        let (response, ip) =
            send_http3_upstream_with_context(&state, &request, "127.0.0.1", address.port(), None)
                .await
                .unwrap();
        assert_eq!(response.version(), ::http::Version::HTTP_3);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.body(), b"http3-ok");
        assert_eq!(ip.as_deref(), Some("127.0.0.1"));
        server.await.unwrap();
    }

    #[test]
    fn content_type_filters_match_extensions_and_mime_types() {
        let script = InterceptionContext {
            url: "https://example.test/assets/app.mjs".into(),
            domain: "example.test".into(),
            ip_address: String::new(),
            protocol: "https".into(),
            method: "GET".into(),
            content_type: "application/javascript".into(),
            request: String::new(),
            cookie_names: String::new(),
            cookie_values: String::new(),
            headers: String::new(),
            body: String::new(),
            param_names: String::new(),
            param_values: String::new(),
            listener_port: String::new(),
            in_scope: true,
        };
        assert!(matches_content_type_filters(
            &["javascript".into()],
            &script
        ));
        assert!(!matches_content_type_filters(&["css".into()], &script));

        let stylesheet = InterceptionContext {
            url: "https://example.test/assets/site".into(),
            domain: "example.test".into(),
            ip_address: String::new(),
            protocol: "https".into(),
            method: "GET".into(),
            content_type: "text/css; charset=utf-8".into(),
            request: String::new(),
            cookie_names: String::new(),
            cookie_values: String::new(),
            headers: String::new(),
            body: String::new(),
            param_names: String::new(),
            param_values: String::new(),
            listener_port: String::new(),
            in_scope: true,
        };
        assert!(matches_content_type_filters(&["css".into()], &stylesheet));
        assert!(matches_content_type_filters(&[], &stylesheet));
    }

    #[test]
    fn interception_rules_apply_in_order_with_boolean_operators() {
        let context = InterceptionContext {
            url: "https://api.example.test/v1/account".into(),
            domain: "api.example.test".into(),
            ip_address: "203.0.113.10".into(),
            protocol: "https".into(),
            method: "GET".into(),
            content_type: "application/json".into(),
            request: "GET /v1/account?include=profile HTTP/1.1".into(),
            cookie_names: "session".into(),
            cookie_values: "abc123".into(),
            headers: "Authorization: Bearer token".into(),
            body: "grant_type=token".into(),
            param_names: "include".into(),
            param_values: "profile".into(),
            listener_port: "8080".into(),
            in_scope: true,
        };
        let rules = vec![
            InterceptionRule {
                id: "account-url".into(),
                enabled: true,
                operator: "and".into(),
                match_type: "url".into(),
                relationship: "contains".into(),
                condition: "account".into(),
            },
            InterceptionRule {
                id: "in-scope".into(),
                enabled: true,
                operator: "and".into(),
                match_type: "inScope".into(),
                relationship: "isInScope".into(),
                condition: String::new(),
            },
            InterceptionRule {
                id: "get-request".into(),
                enabled: true,
                operator: "or".into(),
                match_type: "httpMethod".into(),
                relationship: "matches".into(),
                condition: "^GET$".into(),
            },
        ];
        assert!(rules_match(&rules, &context));

        let out_of_scope = InterceptionContext {
            in_scope: false,
            ..context
        };
        assert!(rules_match(&rules, &out_of_scope));
        assert!(!rules_match(&rules[..2], &out_of_scope));
    }

    #[test]
    fn extended_rule_fields_match_live_request_metadata() {
        let mut headers = ::http::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("session=abc123; theme=dark"),
        );
        headers.insert("x-trace-id", HeaderValue::from_static("trace-42"));
        let raw = b"POST /v1/account?include=profile HTTP/1.1\r\nHost: api.example.test\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\nemail=user%40example.test&role=admin";
        let mut context = build_interception_context(
            "https://api.example.test/v1/account?include=profile",
            "api.example.test",
            "203.0.113.10",
            "https",
            "POST",
            "application/x-www-form-urlencoded",
            &headers,
            b"email=user%40example.test&role=admin",
            raw,
            false,
        );
        context.listener_port = "8080".into();
        let matches = |match_type: &str, condition: &str| {
            rule_matches(
                &InterceptionRule {
                    id: match_type.into(),
                    enabled: true,
                    operator: "and".into(),
                    match_type: match_type.into(),
                    relationship: "contains".into(),
                    condition: condition.into(),
                },
                &context,
            )
        };
        assert!(matches("ipAddress", "203.0.113"));
        assert!(matches("protocol", "https"));
        assert!(matches("request", "POST /v1/account"));
        assert!(matches("cookieName", "session"));
        assert!(matches("cookieValue", "abc123"));
        assert!(matches("anyHeader", "trace-42"));
        assert!(matches("body", "role=admin"));
        assert!(matches("paramName", "email"));
        assert!(matches("paramValue", "user@example.test"));
        assert!(matches("listenerPort", "8080"));
    }

    #[test]
    fn tls_close_without_close_notify_is_a_routine_disconnect() {
        let error = WitnessError::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "peer closed connection without sending TLS close_notify",
        ));
        assert!(is_routine_tls_disconnect(&error));
    }

    #[tokio::test]
    async fn framed_upstream_response_does_not_wait_for_connection_close() {
        let (mut client, mut server) = tokio::io::duplex(4_096);
        let server_task = tokio::spawn(async move {
            server
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        });

        let raw = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            read_upstream_response(&mut client, &::http::Method::GET),
        )
        .await
        .expect("framed response should complete before the connection closes")
        .unwrap();
        assert!(raw.ends_with(b"\r\n\r\nok"));
        server_task.abort();
    }

    #[tokio::test]
    async fn informational_response_is_skipped_when_final_response_is_already_buffered() {
        let (mut client, mut server) = tokio::io::duplex(4_096);
        let server_task = tokio::spawn(async move {
            server
                .write_all(
                    b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
                )
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        });

        let raw = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            read_upstream_response(&mut client, &::http::Method::POST),
        )
        .await
        .expect("final response should be parsed from the existing buffer")
        .unwrap();
        assert!(raw.starts_with(b"HTTP/1.1 200 OK"));
        assert!(raw.ends_with(b"\r\n\r\nok"));
        server_task.abort();
    }

    #[tokio::test]
    async fn http_request_round_trips_through_proxy() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut connection, _) = upstream.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = connection.read(&mut request).await.unwrap();
            connection
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
        });

        let state = AppState::new();
        let proxy_certificates = tempfile::tempdir().unwrap();
        *state.certificate_authority.write().await =
            Some(CertificateAuthority::load_or_create(proxy_certificates.path()).unwrap());
        state.proxy.write().await.port = 0;
        let cancellation = CancellationToken::new();
        let mut events = state.event_bus.subscribe(Some(EventCategory::Proxy));
        let runner = tokio::spawn(ProxyEngine::run(state.clone(), cancellation.clone()));
        let proxy_address = loop {
            if let Event::Proxy(ProxyEvent::Started { address }) = events.recv().await.unwrap() {
                break address;
            }
        };
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        let request = format!(
            "GET http://127.0.0.1:{}/test HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            upstream_address.port(), upstream_address.port()
        );
        client.write_all(request.as_bytes()).await.unwrap();
        let response = read_to_end_limited(&mut client).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        assert!(response.ends_with(b"ok"));
        cancellation.cancel();
        runner.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn compressed_http_response_is_decoded_by_proxy() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            let (mut connection, _) = upstream.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = connection.read(&mut request).await.unwrap();
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(br#"{"ok":true}"#).unwrap();
            let body = encoder.finish().unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            connection.write_all(response.as_bytes()).await.unwrap();
            connection.write_all(&body).await.unwrap();
        });

        let state = AppState::new();
        let proxy_certificates = tempfile::tempdir().unwrap();
        *state.certificate_authority.write().await =
            Some(CertificateAuthority::load_or_create(proxy_certificates.path()).unwrap());
        state.proxy.write().await.port = 0;
        let cancellation = CancellationToken::new();
        let mut events = state.event_bus.subscribe(Some(EventCategory::Proxy));
        let runner = tokio::spawn(ProxyEngine::run(state.clone(), cancellation.clone()));
        let proxy_address = loop {
            if let Event::Proxy(ProxyEvent::Started { address }) = events.recv().await.unwrap() {
                break address;
            }
        };

        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(
                format!(
                    "GET http://127.0.0.1:{}/json HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                    upstream_address.port(),
                    upstream_address.port()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let response = read_to_end_limited(&mut client).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        assert!(!response
            .windows(b"content-encoding:".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-encoding:")));
        assert!(response.ends_with(br#"{"ok":true}"#));

        cancellation.cancel();
        upstream_task.await.unwrap();
        runner.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn websocket_upgrade_and_frames_round_trip_through_proxy() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            let (mut connection, _) = upstream.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = connection.read(&mut chunk).await.unwrap();
                assert_ne!(read, 0);
                request.extend_from_slice(&chunk[..read]);
            }
            let request_text = String::from_utf8_lossy(&request).to_ascii_lowercase();
            assert!(request_text.contains("\r\nconnection: upgrade\r\n"));
            assert!(request_text.contains("\r\nupgrade: websocket\r\n"));
            connection
                .write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\nearly",
                )
                .await
                .unwrap();
            let mut frame = [0_u8; 4];
            connection.read_exact(&mut frame).await.unwrap();
            assert_eq!(&frame, b"ping");
            connection.write_all(b"pong").await.unwrap();
        });

        let state = AppState::new();
        let proxy_certificates = tempfile::tempdir().unwrap();
        *state.certificate_authority.write().await =
            Some(CertificateAuthority::load_or_create(proxy_certificates.path()).unwrap());
        state.proxy.write().await.port = 0;
        let cancellation = CancellationToken::new();
        let mut events = state.event_bus.subscribe(Some(EventCategory::Proxy));
        let runner = tokio::spawn(ProxyEngine::run(state.clone(), cancellation.clone()));
        let proxy_address = loop {
            if let Event::Proxy(ProxyEvent::Started { address }) = events.recv().await.unwrap() {
                break address;
            }
        };
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(
                format!(
                    "GET http://127.0.0.1:{}/socket HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
                    upstream_address.port(),
                    upstream_address.port(),
                )
                .as_bytes(),
            )
            .await
            .unwrap();

        let mut handshake = Vec::new();
        let mut chunk = [0_u8; 1024];
        let header_end = loop {
            if let Some(index) = handshake
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
            {
                break index + 4;
            }
            let read = client.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0);
            handshake.extend_from_slice(&chunk[..read]);
        };
        assert!(handshake.starts_with(b"HTTP/1.1 101 Switching Protocols"));
        let mut early = handshake[header_end..].to_vec();
        while early.len() < 5 {
            let read = client.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0);
            early.extend_from_slice(&chunk[..read]);
        }
        assert_eq!(&early[..5], b"early");

        client.write_all(b"ping").await.unwrap();
        let mut pong = [0_u8; 4];
        client.read_exact(&mut pong).await.unwrap();
        assert_eq!(&pong, b"pong");

        cancellation.cancel();
        upstream_task.await.unwrap();
        runner.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn client_keep_alive_and_chunked_responses_work() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        tokio::spawn(async move {
            for index in 0..2 {
                let (mut connection, _) = upstream.accept().await.unwrap();
                let mut request = [0_u8; 4096];
                let _ = connection.read(&mut request).await.unwrap();
                let response: &[u8] = if index == 0 {
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nfirst\r\n0\r\n\r\n"
                } else {
                    b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecond"
                };
                connection.write_all(response).await.unwrap();
            }
        });

        let state = AppState::new();
        let proxy_certificates = tempfile::tempdir().unwrap();
        *state.certificate_authority.write().await =
            Some(CertificateAuthority::load_or_create(proxy_certificates.path()).unwrap());
        state.proxy.write().await.port = 0;
        let cancellation = CancellationToken::new();
        let mut events = state.event_bus.subscribe(Some(EventCategory::Proxy));
        let runner = tokio::spawn(ProxyEngine::run(state.clone(), cancellation.clone()));
        let proxy_address = loop {
            if let Event::Proxy(ProxyEvent::Started { address }) = events.recv().await.unwrap() {
                break address;
            }
        };
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        let mut response_buffer = Vec::new();
        client
            .write_all(
                format!(
                    "GET http://127.0.0.1:{}/one HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: keep-alive\r\n\r\n",
                    upstream_address.port(), upstream_address.port()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let first = read_parsed_response(&mut client, &mut response_buffer)
            .await
            .unwrap();
        assert_eq!(first.body(), b"first");

        client
            .write_all(
                format!(
                    "GET http://127.0.0.1:{}/two HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                    upstream_address.port(), upstream_address.port()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let second = read_parsed_response(&mut client, &mut response_buffer)
            .await
            .unwrap();
        assert_eq!(second.body(), b"second");
        cancellation.cancel();
        runner.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn stopping_proxy_cancels_active_connections() {
        let state = AppState::new();
        let proxy_certificates = tempfile::tempdir().unwrap();
        *state.certificate_authority.write().await =
            Some(CertificateAuthority::load_or_create(proxy_certificates.path()).unwrap());
        state.proxy.write().await.port = 0;
        let cancellation = CancellationToken::new();
        let mut events = state.event_bus.subscribe(Some(EventCategory::Proxy));
        let runner = tokio::spawn(ProxyEngine::run(state.clone(), cancellation.clone()));
        let proxy_address = loop {
            if let Event::Proxy(ProxyEvent::Started { address }) = events.recv().await.unwrap() {
                break address;
            }
        };
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        cancellation.cancel();
        runner.await.unwrap().unwrap();
        assert_eq!(state.proxy.read().await.connection_count, 0);
        let mut byte = [0_u8; 1];
        let read = tokio::time::timeout(std::time::Duration::from_secs(1), client.read(&mut byte))
            .await
            .unwrap();
        match read {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            other => panic!("active connection remained open: {other:?}"),
        }
    }

    #[tokio::test]
    async fn request_and_response_interception_resolve_through_event_bus() {
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut connection, _) = upstream.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = connection.read(&mut request).await.unwrap();
            connection
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\noriginal",
                )
                .await
                .unwrap();
        });

        let state = AppState::new();
        state.proxy.write().await.port = 0;
        state.proxy.write().await.intercepting = true;
        {
            let mut settings = state.settings.write().await;
            settings.proxy_intercepting = true;
            settings.proxy_intercept_mode = "requestsAndResponses".into();
        }
        state.interceptions.set_enabled(true).await;
        let proxy_certificates = tempfile::tempdir().unwrap();
        *state.certificate_authority.write().await =
            Some(CertificateAuthority::load_or_create(proxy_certificates.path()).unwrap());
        let mut proxy_events = state.event_bus.subscribe(Some(EventCategory::Proxy));
        let mut interception_events = state.event_bus.subscribe(Some(EventCategory::Interception));
        let cancellation = CancellationToken::new();
        let runner = tokio::spawn(ProxyEngine::run(state.clone(), cancellation.clone()));
        let resolver = state.interceptions.clone();
        tokio::spawn(async move {
            let request_id = match interception_events.recv().await.unwrap() {
                Event::Interception(crate::event_bus::InterceptionEvent::Request {
                    id, ..
                }) => id,
                event => panic!("expected request interception, got {event:?}"),
            };
            assert!(
                resolver
                    .resolve(&request_id, InterceptionResolution::Forward)
                    .await
            );
            let response_id = loop {
                if let Event::Interception(crate::event_bus::InterceptionEvent::Response {
                    id,
                    ..
                }) = interception_events.recv().await.unwrap()
                {
                    break id;
                }
            };
            assert!(
                resolver
                    .resolve(
                        &response_id,
                        InterceptionResolution::Modify(
                            b"HTTP/1.1 201 Created\r\nContent-Length: 7\r\n\r\nchanged".to_vec(),
                        ),
                    )
                    .await
            );
        });
        let proxy_address = loop {
            if let Event::Proxy(ProxyEvent::Started { address }) =
                proxy_events.recv().await.unwrap()
            {
                break address;
            }
        };
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(
                format!(
                    "GET http://127.0.0.1:{}/ HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                    upstream_address.port(), upstream_address.port()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let response = read_to_end_limited(&mut client).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 201 Created"));
        assert!(response.ends_with(b"changed"));
        cancellation.cancel();
        runner.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn https_request_round_trips_through_mitm_proxy() {
        let upstream_certificates = tempfile::tempdir().unwrap();
        let upstream_authority =
            CertificateAuthority::load_or_create(upstream_certificates.path()).unwrap();
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_acceptor = TlsAcceptor::from(upstream_authority.server_config());
        tokio::spawn(async move {
            let (connection, _) = upstream.accept().await.unwrap();
            let mut connection = upstream_acceptor.accept(connection).await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = connection.read(&mut request).await.unwrap();
            connection
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecure",
                )
                .await
                .unwrap();
            connection.shutdown().await.unwrap();
        });

        let state = AppState::new();
        let mut upstream_roots = RootCertStore::empty();
        upstream_roots.add(upstream_authority.ca_der()).unwrap();
        let mut upstream_client = ClientConfig::builder()
            .with_root_certificates(upstream_roots)
            .with_no_client_auth();
        upstream_client.alpn_protocols = vec![b"http/1.1".to_vec()];
        *state.upstream_tls_config.write().await = Arc::new(upstream_client);

        let proxy_certificates = tempfile::tempdir().unwrap();
        let proxy_authority =
            CertificateAuthority::load_or_create(proxy_certificates.path()).unwrap();
        *state.certificate_authority.write().await = Some(proxy_authority.clone());
        state.proxy.write().await.port = 0;
        let cancellation = CancellationToken::new();
        let mut events = state.event_bus.subscribe(Some(EventCategory::Proxy));
        let runner = tokio::spawn(ProxyEngine::run(state.clone(), cancellation.clone()));
        let proxy_address = loop {
            if let Event::Proxy(ProxyEvent::Started { address }) = events.recv().await.unwrap() {
                break address;
            }
        };

        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(
                format!(
                    "CONNECT localhost:{} HTTP/1.1\r\nHost: localhost:{}\r\n\r\n",
                    upstream_address.port(),
                    upstream_address.port()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let established = read_header_exact(&mut client).await.unwrap();
        assert!(established.starts_with(b"HTTP/1.1 200"));

        let mut proxy_roots = RootCertStore::empty();
        proxy_roots.add(proxy_authority.ca_der()).unwrap();
        let mut proxy_client = ClientConfig::builder()
            .with_root_certificates(proxy_roots)
            .with_no_client_auth();
        proxy_client.alpn_protocols = vec![b"http/1.1".to_vec()];
        // The CONNECT authority must select the leaf even if SNI is absent.
        proxy_client.enable_sni = false;
        let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let mut tls_client = TlsConnector::from(Arc::new(proxy_client))
            .connect(name, client)
            .await
            .unwrap();
        tls_client
            .write_all(
                format!(
                    "GET /secure HTTP/1.1\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n",
                    upstream_address.port()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let response = read_to_end_limited(&mut tls_client).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        assert!(response.ends_with(b"secure"));
        assert_eq!(
            tls_client.get_ref().1.alpn_protocol(),
            Some(b"http/1.1".as_slice())
        );

        cancellation.cancel();
        runner.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn http2_request_round_trips_through_mitm_proxy_and_h2_upstream() {
        let upstream_certificates = tempfile::tempdir().unwrap();
        let upstream_authority =
            CertificateAuthority::load_or_create(upstream_certificates.path()).unwrap();
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_acceptor = TlsAcceptor::from(upstream_authority.server_config());
        let upstream_task = tokio::spawn(async move {
            let (connection, _) = upstream.accept().await.unwrap();
            let connection = upstream_acceptor.accept(connection).await.unwrap();
            assert_eq!(
                connection.get_ref().1.alpn_protocol(),
                Some(b"h2".as_slice())
            );
            let mut connection = h2::server::handshake(connection).await.unwrap();
            let (request, mut responder) = connection.accept().await.unwrap().unwrap();
            let stream_task = tokio::spawn(async move {
                assert_eq!(request.uri().path(), "/secure-h2");
                let request = collect_http2_request(request).await.unwrap();
                assert_eq!(request.body(), b"request-body");
                let response = Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/plain")
                    .body(())
                    .unwrap();
                let mut body = responder.send_response(response, false).unwrap();
                send_http2_body(&mut body, Bytes::from_static(b"secure-h2"))
                    .await
                    .unwrap();
            });
            while connection.accept().await.is_some() {}
            stream_task.await.unwrap();
        });

        let state = AppState::new();
        let mut upstream_roots = RootCertStore::empty();
        upstream_roots.add(upstream_authority.ca_der()).unwrap();
        let mut upstream_client = ClientConfig::builder()
            .with_root_certificates(upstream_roots)
            .with_no_client_auth();
        upstream_client.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        *state.upstream_tls_config.write().await = Arc::new(upstream_client);

        let proxy_certificates = tempfile::tempdir().unwrap();
        let proxy_authority =
            CertificateAuthority::load_or_create(proxy_certificates.path()).unwrap();
        *state.certificate_authority.write().await = Some(proxy_authority.clone());
        state.proxy.write().await.port = 0;
        let cancellation = CancellationToken::new();
        let mut events = state.event_bus.subscribe(Some(EventCategory::Proxy));
        let runner = tokio::spawn(ProxyEngine::run(state.clone(), cancellation.clone()));
        let proxy_address = loop {
            if let Event::Proxy(ProxyEvent::Started { address }) = events.recv().await.unwrap() {
                break address;
            }
        };

        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(
                format!(
                    "CONNECT localhost:{} HTTP/1.1\r\nHost: localhost:{}\r\n\r\n",
                    upstream_address.port(),
                    upstream_address.port()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        assert!(read_header_exact(&mut client)
            .await
            .unwrap()
            .starts_with(b"HTTP/1.1 200"));

        let mut proxy_roots = RootCertStore::empty();
        proxy_roots.add(proxy_authority.ca_der()).unwrap();
        let mut proxy_client = ClientConfig::builder()
            .with_root_certificates(proxy_roots)
            .with_no_client_auth();
        proxy_client.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let tls_client = TlsConnector::from(Arc::new(proxy_client))
            .connect(name, client)
            .await
            .unwrap();
        assert_eq!(
            tls_client.get_ref().1.alpn_protocol(),
            Some(b"h2".as_slice())
        );

        let (mut sender, connection) = h2::client::handshake(tls_client).await.unwrap();
        let connection_task = tokio::spawn(connection);
        let request = Request::builder()
            .method(::http::Method::POST)
            .uri(format!(
                "https://localhost:{}/secure-h2",
                upstream_address.port()
            ))
            .body(())
            .unwrap();
        let (response, mut request_body) = sender.send_request(request, false).unwrap();
        request_body
            .send_data(Bytes::from_static(b"request-body"), true)
            .unwrap();
        let response = response.await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = collect_http2_body(response.into_body()).await.unwrap();
        assert_eq!(body, b"secure-h2");

        drop(sender);
        cancellation.cancel();
        runner.await.unwrap().unwrap();
        connection_task.abort();
        upstream_task.await.unwrap();
    }

    #[tokio::test]
    async fn https_upstream_timeout_returns_gateway_timeout_over_tls() {
        let upstream_certificates = tempfile::tempdir().unwrap();
        let upstream_authority =
            CertificateAuthority::load_or_create(upstream_certificates.path()).unwrap();
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_acceptor = TlsAcceptor::from(upstream_authority.server_config());
        let upstream_task = tokio::spawn(async move {
            let (connection, _) = upstream.accept().await.unwrap();
            let mut connection = upstream_acceptor.accept(connection).await.unwrap();
            let mut request = [0_u8; 4_096];
            let _ = connection.read(&mut request).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        });

        let state = AppState::new();
        state.settings.write().await.upstream_timeout_seconds = 1;
        let mut upstream_roots = RootCertStore::empty();
        upstream_roots.add(upstream_authority.ca_der()).unwrap();
        let mut upstream_client = ClientConfig::builder()
            .with_root_certificates(upstream_roots)
            .with_no_client_auth();
        upstream_client.alpn_protocols = vec![b"http/1.1".to_vec()];
        *state.upstream_tls_config.write().await = Arc::new(upstream_client);

        let proxy_certificates = tempfile::tempdir().unwrap();
        let proxy_authority =
            CertificateAuthority::load_or_create(proxy_certificates.path()).unwrap();
        *state.certificate_authority.write().await = Some(proxy_authority.clone());
        state.proxy.write().await.port = 0;
        let cancellation = CancellationToken::new();
        let mut events = state.event_bus.subscribe(Some(EventCategory::Proxy));
        let runner = tokio::spawn(ProxyEngine::run(state.clone(), cancellation.clone()));
        let proxy_address = loop {
            if let Event::Proxy(ProxyEvent::Started { address }) = events.recv().await.unwrap() {
                break address;
            }
        };

        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(
                format!(
                    "CONNECT localhost:{} HTTP/1.1\r\nHost: localhost:{}\r\n\r\n",
                    upstream_address.port(),
                    upstream_address.port()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        assert!(read_header_exact(&mut client)
            .await
            .unwrap()
            .starts_with(b"HTTP/1.1 200"));

        let mut proxy_roots = RootCertStore::empty();
        proxy_roots.add(proxy_authority.ca_der()).unwrap();
        let mut proxy_client = ClientConfig::builder()
            .with_root_certificates(proxy_roots)
            .with_no_client_auth();
        proxy_client.alpn_protocols = vec![b"http/1.1".to_vec()];
        let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let mut tls_client = TlsConnector::from(Arc::new(proxy_client))
            .connect(name, client)
            .await
            .unwrap();
        tls_client
            .write_all(
                format!(
                    "GET /slow HTTP/1.1\r\nHost: localhost:{}\r\nConnection: close\r\n\r\n",
                    upstream_address.port()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let response = read_to_end_limited(&mut tls_client).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 504 Gateway Timeout"));
        assert!(response.ends_with(b"Gateway Timeout"));

        cancellation.cancel();
        runner.await.unwrap().unwrap();
        upstream_task.abort();
    }

    async fn read_header_exact<R: AsyncRead + Unpin>(stream: &mut R) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        let mut byte = [0_u8; 1];
        while !output.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).await? == 0 {
                return Err(WitnessError::InvalidHttp("unexpected EOF in header".into()));
            }
            output.push(byte[0]);
        }
        Ok(output)
    }

    async fn read_parsed_response<R: AsyncRead + Unpin>(
        stream: &mut R,
        buffer: &mut Vec<u8>,
    ) -> Result<Response<Vec<u8>>> {
        loop {
            if let Some((response, consumed)) = parse_response(buffer)? {
                buffer.drain(..consumed);
                return Ok(response);
            }
            let mut chunk = [0_u8; 4096];
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(WitnessError::InvalidHttp(
                    "unexpected EOF in response".into(),
                ));
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
    }
}
