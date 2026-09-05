use std::time::{Duration, Instant};

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::TlsConnector;
use tokio_util::sync::CancellationToken;

use crate::{
    database::{IdentityInjectionDescriptor, IdentityInjectionType},
    error::{Result, WitnessError},
    http::{
        apply_response_compression, parse_replay_request, parse_response_for_method,
        response_needs_decompression, serialize_request, serialize_response, MAX_MESSAGE_SIZE,
    },
    logging,
    proxy::{send_http2_upstream, upstream},
    state::{TrafficStats, UpstreamProxyConfig},
    tls::CertificateAuthority,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepeaterResponse {
    pub raw: Vec<u8>,
    pub status: u16,
    pub duration_ms: u64,
    pub size: usize,
}

pub struct RepeaterRequestStats<'a> {
    pub traffic_stats: Option<&'a TrafficStats>,
    pub timeout_duration: Duration,
}

#[derive(Default)]
pub struct Repeater;

impl Repeater {
    pub async fn send_request(
        &self,
        raw: Vec<u8>,
        tls: bool,
        injection: Option<IdentityInjectionDescriptor>,
        upstream_proxy: Option<UpstreamProxyConfig>,
        compression_mode: &str,
        cancellation: CancellationToken,
    ) -> Result<RepeaterResponse> {
        self.send_request_with_timeout(
            raw,
            tls,
            injection,
            upstream_proxy,
            compression_mode,
            cancellation,
            Duration::from_secs(30),
        )
        .await
    }

    pub async fn send_request_with_timeout(
        &self,
        raw: Vec<u8>,
        tls: bool,
        injection: Option<IdentityInjectionDescriptor>,
        upstream_proxy: Option<UpstreamProxyConfig>,
        compression_mode: &str,
        cancellation: CancellationToken,
        timeout_duration: Duration,
    ) -> Result<RepeaterResponse> {
        self.send_request_with_stats(
            raw,
            tls,
            injection,
            upstream_proxy,
            compression_mode,
            cancellation,
            RepeaterRequestStats {
                traffic_stats: None,
                timeout_duration,
            },
        )
        .await
    }

    pub async fn send_request_with_stats(
        &self,
        raw: Vec<u8>,
        tls: bool,
        injection: Option<IdentityInjectionDescriptor>,
        upstream_proxy: Option<UpstreamProxyConfig>,
        compression_mode: &str,
        cancellation: CancellationToken,
        request_stats: RepeaterRequestStats<'_>,
    ) -> Result<RepeaterResponse> {
        let _operation = logging::OperationGuard::new("repeater.send_request");
        // Burp's HTTP/2 copy format is a readable HTTP/1-style message whose
        // start line ends in `HTTP/2`. It is not an HTTP/2 wire preface, so
        // normalize that representation before handing it to the HTTP/1
        // parser. The negotiated TLS protocol below still determines whether
        // the request is sent as HTTP/2 or HTTP/1.1 upstream. Intruder uses
        // this same send path for every generated request.
        let (mut request, _) = parse_replay_request(&raw)?
            .ok_or_else(|| WitnessError::InvalidHttp("incomplete repeater request".into()))?;
        let (method, target, header_count, body_bytes) = logging::request_metadata(&request);
        tracing::info!(
            target: "witness_lib::network::repeater",
            phase = "request_received",
            method = %method,
            target = %target,
            header_count,
            body_bytes,
            input_bytes = raw.len(),
            tls_selected = tls,
            "repeater request received"
        );
        if let Some(injection) = injection.as_ref() {
            tracing::debug!(
                target: "witness_lib::network::repeater",
                phase = "identity_injection_started",
                injection_type = ?injection.injection_type,
                key = %injection.injection_key,
                value_configured = !injection.auth_value.is_empty(),
                "repeater identity injection started"
            );
            apply_injection(&mut request, injection)?;
            tracing::debug!(
                target: "witness_lib::network::repeater",
                phase = "identity_injection_completed",
                injection_type = ?injection.injection_type,
                "repeater identity injection completed"
            );
        }
        let host_header = request
            .uri()
            .authority()
            .map(|value| value.as_str())
            .or_else(|| {
                request
                    .headers()
                    .get("host")
                    .and_then(|value| value.to_str().ok())
            })
            .ok_or_else(|| WitnessError::InvalidHttp("request has no Host".into()))?;
        let tls = resolve_tls(&request, tls)?;
        let default_port = if tls { 443 } else { 80 };
        let (host, port) = parse_authority(host_header, default_port)?;
        let request_method = request.method().clone();
        let started = Instant::now();
        request
            .headers_mut()
            .insert("connection", ::http::HeaderValue::from_static("close"));
        let outbound = serialize_request(&request);
        tracing::info!(
            target: "witness_lib::network::repeater",
            phase = "request_prepared",
            host,
            port,
            method = %request_method,
            target = %logging::safe_url(request.uri().to_string().as_str()),
            request_bytes = outbound.len(),
            tls,
            route = if upstream_proxy.is_some() { "upstream_proxy" } else { "direct" },
            "repeater request prepared for upstream"
        );
        if let Some(traffic_stats) = request_stats.traffic_stats {
            traffic_stats.record_processed();
            traffic_stats.record_sent(outbound.len());
        }

        let response_result = tokio::select! {
            _ = cancellation.cancelled() => Err(WitnessError::Cancelled),
            result = tokio::time::timeout(request_stats.timeout_duration, async {
                tracing::debug!(
                    target: "witness_lib::network::repeater",
                    phase = "upstream_connect_started",
                    host,
                    port,
                    "repeater upstream connection started"
                );
                let stream = upstream::connect_with_context(
                    upstream_proxy.as_ref(),
                    &host,
                    port,
                    None,
                )
                .await?;
                tracing::debug!(
                    target: "witness_lib::network::repeater",
                    phase = "upstream_connected",
                    host,
                    port,
                    peer = ?stream.peer_addr().ok(),
                    "repeater upstream connection established"
                );
                if tls {
                    let name = rustls::pki_types::ServerName::try_from(host.clone())
                        .map_err(|error| WitnessError::Tls(error.to_string()))?;
                    let mut stream = TlsConnector::from(CertificateAuthority::client_config())
                        .connect(name, stream).await
                        .map_err(|error| WitnessError::Tls(error.to_string()))?;
                    let alpn = stream
                        .get_ref()
                        .1
                        .alpn_protocol()
                        .map(|protocol| String::from_utf8_lossy(protocol).into_owned())
                        .unwrap_or_else(|| "none".into());
                    tracing::debug!(
                        target: "witness_lib::network::repeater",
                        phase = "upstream_tls_handshake_completed",
                        host,
                        port,
                        alpn = %alpn,
                        "repeater upstream TLS handshake completed"
                    );
                    if alpn == "h2" {
                        tracing::debug!(
                            target: "witness_lib::network::repeater",
                            phase = "upstream_protocol_selected",
                            protocol = "h2",
                            "repeater selected HTTP/2 upstream"
                        );
                        let response = send_http2_upstream(stream, request, &host, port).await?;
                        Ok(serialize_response(&response))
                    } else {
                        tracing::debug!(
                            target: "witness_lib::network::repeater",
                            phase = "upstream_protocol_selected",
                            protocol = "http/1.1",
                            "repeater selected HTTP/1.1 upstream"
                        );
                        stream.write_all(&outbound).await?;
                        tracing::debug!(
                            target: "witness_lib::network::repeater",
                            phase = "request_sent_upstream",
                            protocol = "http/1.1",
                            request_bytes = outbound.len(),
                            "repeater request sent upstream"
                        );
                        read_limited(&mut stream, &request_method).await
                    }
                } else {
                    let mut stream = stream;
                    tracing::debug!(
                        target: "witness_lib::network::repeater",
                        phase = "upstream_protocol_selected",
                        protocol = "http/1.1",
                        "repeater selected HTTP/1.1 upstream"
                    );
                    stream.write_all(&outbound).await?;
                    tracing::debug!(
                        target: "witness_lib::network::repeater",
                        phase = "request_sent_upstream",
                        protocol = "http/1.1",
                        request_bytes = outbound.len(),
                        "repeater request sent upstream"
                    );
                    read_limited(&mut stream, &request_method).await
                }
            }) => match result {
                Ok(result) => result,
                Err(_) => Err(WitnessError::Proxy(
                    "repeater upstream request timed out".into(),
                )),
            },
        };
        let response = match response_result {
            Ok(response) => response,
            Err(error) => {
                tracing::error!(
                    target: "witness_lib::network::repeater",
                    phase = "request_failed",
                    host,
                    port,
                    error = %error,
                    duration_ms = started.elapsed().as_millis() as u64,
                    "repeater request failed"
                );
                return Err(error);
            }
        };
        if let Some(traffic_stats) = request_stats.traffic_stats {
            traffic_stats.record_received(response.len());
        }
        tracing::debug!(
            target: "witness_lib::network::repeater",
            phase = "response_received_upstream",
            host,
            port,
            response_bytes = response.len(),
            "repeater response received upstream"
        );
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut parser = httparse::Response::new(&mut headers);
        parser
            .parse(&response)
            .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?;
        let status = parser.code.unwrap_or(0);
        let response = prepare_response(response, &request_method, compression_mode)?;
        tracing::info!(
            target: "witness_lib::network::repeater",
            phase = "request_completed",
            host,
            port,
            status,
            response_bytes = response.len(),
            duration_ms = started.elapsed().as_millis() as u64,
            "repeater request completed"
        );
        Ok(RepeaterResponse {
            size: response.len(),
            raw: response,
            status,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }
}

fn prepare_response(
    raw: Vec<u8>,
    request_method: &::http::Method,
    compression_mode: &str,
) -> Result<Vec<u8>> {
    if compression_mode == "passThrough" {
        return Ok(raw);
    }
    let response = parse_response_for_method(&raw, request_method)?
        .ok_or_else(|| WitnessError::InvalidHttp("incomplete upstream response".into()))?
        .0;
    if !response_needs_decompression(&response, compression_mode) {
        return Ok(raw);
    }
    Ok(serialize_response(&apply_response_compression(
        response,
        compression_mode,
    )?))
}

fn apply_injection(
    request: &mut ::http::Request<Vec<u8>>,
    descriptor: &IdentityInjectionDescriptor,
) -> Result<()> {
    match descriptor.injection_type {
        IdentityInjectionType::Cookie => apply_cookie_injection(request, descriptor),
        IdentityInjectionType::Header => apply_header_injection(request, descriptor),
        IdentityInjectionType::QueryParameter => apply_query_injection(request, descriptor),
    }
}

fn apply_cookie_injection(
    request: &mut ::http::Request<Vec<u8>>,
    descriptor: &IdentityInjectionDescriptor,
) -> Result<()> {
    let key = descriptor.injection_key.as_str();
    if key.is_empty()
        || key
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || matches!(byte, b';' | b'=' | b','))
    {
        return Err(WitnessError::InvalidHttp(
            "invalid injected cookie key".into(),
        ));
    }
    let mut cookies = Vec::new();
    for value in request.headers().get_all(::http::header::COOKIE).iter() {
        let value = value
            .to_str()
            .map_err(|_| WitnessError::InvalidHttp("invalid Cookie header".into()))?;
        for part in value.split(';') {
            let part = part.trim();
            let configured_cookie = part
                .split_once('=')
                .is_some_and(|(name, _)| name.trim() == key);
            if !part.is_empty() && !configured_cookie {
                cookies.push(part.to_owned());
            }
        }
    }
    if !descriptor.auth_value.is_empty() {
        cookies.push(format!("{key}={}", descriptor.auth_value));
    }
    request.headers_mut().remove(::http::header::COOKIE);
    if !cookies.is_empty() {
        let value = ::http::HeaderValue::from_str(&cookies.join("; "))
            .map_err(|_| WitnessError::InvalidHttp("invalid injected cookie value".into()))?;
        request.headers_mut().insert(::http::header::COOKIE, value);
    }
    Ok(())
}

fn apply_header_injection(
    request: &mut ::http::Request<Vec<u8>>,
    descriptor: &IdentityInjectionDescriptor,
) -> Result<()> {
    let header = ::http::header::HeaderName::from_bytes(descriptor.injection_key.as_bytes())
        .map_err(|_| WitnessError::InvalidHttp("invalid injected header name".into()))?;
    request.headers_mut().remove(&header);
    if !descriptor.auth_value.is_empty() {
        let value = ::http::HeaderValue::from_str(&descriptor.auth_value)
            .map_err(|_| WitnessError::InvalidHttp("invalid injected header value".into()))?;
        request.headers_mut().insert(header, value);
    }
    Ok(())
}

fn apply_query_injection(
    request: &mut ::http::Request<Vec<u8>>,
    descriptor: &IdentityInjectionDescriptor,
) -> Result<()> {
    if descriptor.injection_key.is_empty()
        || descriptor.injection_key.contains('\r')
        || descriptor.injection_key.contains('\n')
    {
        return Err(WitnessError::InvalidHttp(
            "invalid injected query parameter key".into(),
        ));
    }
    let uri = request.uri();
    let mut parameters = uri
        .query()
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty())
        .filter(|pair| !query_parameter_matches(pair, &descriptor.injection_key))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !descriptor.auth_value.is_empty() {
        parameters.push(format!(
            "{}={}",
            encode_query_component(&descriptor.injection_key),
            encode_query_component(&descriptor.auth_value),
        ));
    }
    let mut target = match (uri.scheme_str(), uri.authority()) {
        (Some(scheme), Some(authority)) => format!("{scheme}://{authority}{}", uri.path()),
        _ => uri.path().to_owned(),
    };
    if !parameters.is_empty() {
        target.push('?');
        target.push_str(&parameters.join("&"));
    }
    *request.uri_mut() = target
        .parse()
        .map_err(|_| WitnessError::InvalidHttp("could not update request URI".into()))?;
    Ok(())
}

fn query_parameter_matches(pair: &str, key: &str) -> bool {
    let name = pair.split_once('=').map_or(pair, |(name, _)| name);
    percent_decode_str(name)
        .decode_utf8()
        .is_ok_and(|name| name == key)
}

/// Percent-encodes a query component while preserving RFC 3986 unreserved
/// characters (`A-Z a-z 0-9 - _ . ~`) and encoding space as `+`. Mirrors
/// `proxy::match_replace::encode_param` so identity injection and
/// match/replace produce identical query strings (avoids over-encoding that
/// `NON_ALPHANUMERIC` would cause).
fn encode_query_component(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '~') {
            out.push(ch);
        } else if ch == ' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn resolve_tls(request: &::http::Request<Vec<u8>>, selected_tls: bool) -> Result<bool> {
    match request.uri().scheme_str() {
        Some("https") => Ok(true),
        Some("http") => Ok(false),
        Some(scheme) => Err(WitnessError::InvalidHttp(format!(
            "unsupported repeater URL scheme: {scheme}"
        ))),
        None => Ok(selected_tls),
    }
}

fn parse_authority(authority: &str, default_port: u16) -> Result<(String, u16)> {
    // Canonical authority parsing lives in `proxy::split_host_port`; keep this
    // thin wrapper for backwards compatibility so both call sites share one
    // implementation (avoids drift between proxy and repeater).
    // NOTE: `http` module is owned by another workstream, so we delegate to
    // the proxy helper instead of adding a new `http::split_authority`.
    crate::proxy::split_host_port(authority, default_port).map_err(|error| {
        // Normalize port errors to the repeater-specific message.
        let message = error.to_string();
        if message.contains("port") {
            WitnessError::InvalidHttp("invalid repeater port".into())
        } else {
            error
        }
    })
}

#[derive(Clone, Copy)]
enum ResponseFraming {
    NoBody { end: usize },
    ContentLength { total: usize },
    Chunked,
    UntilEof,
}

fn response_framing(
    bytes: &[u8],
    request_method: &::http::Method,
) -> Result<Option<ResponseFraming>> {
    let mut raw_headers = [httparse::EMPTY_HEADER; 128];
    let mut parsed = httparse::Response::new(&mut raw_headers);
    let header_end = match parsed
        .parse(bytes)
        .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?
    {
        httparse::Status::Complete(length) => length,
        httparse::Status::Partial => return Ok(None),
    };
    if parsed.version != Some(1) {
        return Err(WitnessError::Http2Unsupported);
    }
    let status = parsed
        .code
        .ok_or_else(|| WitnessError::InvalidHttp("missing response status".into()))?;
    if request_method == ::http::Method::HEAD
        || (100..=199).contains(&status)
        || status == 204
        || status == 304
    {
        return Ok(Some(ResponseFraming::NoBody { end: header_end }));
    }

    let mut chunked = false;
    let mut content_length = None;
    for header in parsed.headers.iter() {
        if header.name.eq_ignore_ascii_case("transfer-encoding") {
            let value = std::str::from_utf8(header.value)
                .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?;
            chunked = value
                .split(',')
                .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"));
        } else if header.name.eq_ignore_ascii_case("content-length") && content_length.is_none() {
            let value = std::str::from_utf8(header.value)
                .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?;
            content_length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| WitnessError::InvalidHttp("invalid Content-Length".into()))?,
            );
        }
    }

    if chunked {
        return Ok(Some(ResponseFraming::Chunked));
    }
    if let Some(length) = content_length {
        let total = header_end
            .checked_add(length)
            .ok_or_else(|| WitnessError::InvalidHttp("response is too large".into()))?;
        if total > MAX_MESSAGE_SIZE {
            return Err(WitnessError::InvalidHttp("message exceeds 100 MiB".into()));
        }
        return Ok(Some(ResponseFraming::ContentLength { total }));
    }
    Ok(Some(ResponseFraming::UntilEof))
}

async fn read_limited<R: AsyncReadExt + Unpin>(
    stream: &mut R,
    request_method: &::http::Method,
) -> Result<Vec<u8>> {
    let mut response = Vec::new();
    let mut framing = None;
    let mut buffer = [0_u8; 8 * 1024];

    loop {
        // Framing is derived once headers are complete and then cached; only
        // the chunked fast-path re-parses the buffered body (bounded by
        // MAX_MESSAGE_SIZE) instead of re-scanning headers every iteration.
        if framing.is_none() {
            framing = response_framing(&response, request_method)?;
        }
        if let Some(current) = framing {
            match current {
                ResponseFraming::NoBody { end } if response.len() >= end => {
                    return Ok(response[..end].to_vec());
                }
                ResponseFraming::ContentLength { total } if response.len() >= total => {
                    return Ok(response[..total].to_vec());
                }
                ResponseFraming::Chunked => {
                    if let Some((_, consumed)) =
                        parse_response_for_method(&response, request_method)?
                    {
                        return Ok(response[..consumed].to_vec());
                    }
                }
                ResponseFraming::UntilEof
                | ResponseFraming::NoBody { .. }
                | ResponseFraming::ContentLength { .. } => {}
            }
        }

        if response.len() >= MAX_MESSAGE_SIZE {
            return Err(WitnessError::InvalidHttp("message exceeds 100 MiB".into()));
        }
        let remaining = MAX_MESSAGE_SIZE - response.len();
        let read_size = remaining.min(buffer.len());
        let read = stream.read(&mut buffer[..read_size]).await?;
        if read == 0 {
            return match framing {
                Some(ResponseFraming::UntilEof) => Ok(response),
                Some(ResponseFraming::NoBody { end }) if response.len() >= end => {
                    Ok(response[..end].to_vec())
                }
                Some(ResponseFraming::ContentLength { total }) if response.len() >= total => {
                    Ok(response[..total].to_vec())
                }
                _ => Err(WitnessError::InvalidHttp(
                    "incomplete upstream response".into(),
                )),
            };
        }
        response.extend_from_slice(&buffer[..read]);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use flate2::{write::GzEncoder, Compression};
    use tokio::net::TcpListener;

    #[test]
    fn parses_ipv4_dns_and_ipv6_authorities() {
        assert_eq!(
            parse_authority("example.test:8080", 80).unwrap(),
            ("example.test".into(), 8080)
        );
        assert_eq!(
            parse_authority("example.test", 443).unwrap(),
            ("example.test".into(), 443)
        );
        assert_eq!(
            parse_authority("[::1]:8443", 443).unwrap(),
            ("::1".into(), 8443)
        );
    }

    #[test]
    fn selected_transport_is_used_for_origin_form_requests() {
        let request = ::http::Request::builder()
            .uri("/resource")
            .body(Vec::new())
            .unwrap();
        assert!(resolve_tls(&request, true).unwrap());
        assert!(!resolve_tls(&request, false).unwrap());
    }

    #[test]
    fn absolute_request_url_overrides_selected_transport() {
        let https = ::http::Request::builder()
            .uri("https://example.test/resource")
            .body(Vec::new())
            .unwrap();
        let http = ::http::Request::builder()
            .uri("http://example.test/resource")
            .body(Vec::new())
            .unwrap();
        assert!(resolve_tls(&https, false).unwrap());
        assert!(!resolve_tls(&http, true).unwrap());
    }

    #[test]
    fn prepares_compressed_responses_using_shared_policy() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(br#"{"data":[1]}"#).unwrap();
        let compressed = encoder.finish().unwrap();
        let mut raw =
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\n\r\n"
                .to_vec();
        raw.extend_from_slice(&compressed);

        let prepared =
            prepare_response(raw.clone(), &::http::Method::GET, "decompressAll").unwrap();
        assert!(prepared.starts_with(b"HTTP/1.1 200 OK"));
        assert!(!prepared
            .windows(b"content-encoding:".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-encoding:")));
        assert!(prepared.ends_with(br#"{"data":[1]}"#));
        assert_eq!(
            prepare_response(raw.clone(), &::http::Method::GET, "passThrough").unwrap(),
            raw
        );
    }

    #[tokio::test]
    async fn request_response_round_trip() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1_024];
            let read = stream.read(&mut request).await.unwrap();
            assert!(request[..read].starts_with(b"GET /repeat HTTP/1.1"));
            stream
                .write_all(
                    b"HTTP/1.1 202 Accepted\r\nContent-Length: 8\r\nConnection: close\r\n\r\nrepeated",
                )
                .await
                .unwrap();
        });
        let raw = format!(
            "GET http://127.0.0.1:{}/repeat HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            address.port(),
            address.port()
        )
        .into_bytes();
        let response = Repeater
            .send_request(
                raw,
                false,
                None,
                None,
                "decompressAll",
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(response.status, 202);
        assert!(response.raw.ends_with(b"repeated"));
        assert_eq!(response.size, response.raw.len());
    }

    #[tokio::test]
    async fn burp_http2_copy_format_round_trips_through_send_path() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1_024];
            let read = stream.read(&mut request).await.unwrap();
            assert!(request[..read].starts_with(b"GET /repeat HTTP/1.1"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nhandled",
                )
                .await
                .unwrap();
        });
        let raw = format!(
            "GET http://127.0.0.1:{}/repeat HTTP/2\r\nHost: 127.0.0.1:{}\r\nTe: trailers\r\n\r\n",
            address.port(),
            address.port()
        )
        .into_bytes();
        let response = Repeater
            .send_request(
                raw,
                false,
                None,
                None,
                "decompressAll",
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(response.status, 200);
        assert!(response.raw.ends_with(b"handled"));
    }
}
