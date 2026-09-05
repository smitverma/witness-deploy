//! Connects to an origin server, optionally tunneling the connection through
//! a configured upstream HTTP or SOCKS5 proxy.
//!
//! This is Witness's own *outbound* proxy setting: it controls how Witness
//! reaches origin servers, and is independent of the local listening proxy
//! that browsers point at Witness through.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::{
    error::{Result, WitnessError},
    state::UpstreamProxyConfig,
};

/// Opens a TCP connection to `host:port`, routed through `proxy` when present.
pub async fn connect(
    proxy: Option<&UpstreamProxyConfig>,
    host: &str,
    port: u16,
) -> Result<TcpStream> {
    connect_with_context(proxy, host, port, None).await
}

/// Opens a TCP connection and includes an optional exchange identifier in
/// transport diagnostics. Credentials are deliberately represented only by
/// whether authentication is configured, never by their values.
pub async fn connect_with_context(
    proxy: Option<&UpstreamProxyConfig>,
    host: &str,
    port: u16,
    exchange_id: Option<&str>,
) -> Result<TcpStream> {
    let exchange_id = exchange_id.unwrap_or("untracked");
    tracing::debug!(
        target: "witness_lib::network::upstream",
        phase = "connect_started",
        exchange_id,
        host,
        port,
        route = if proxy.is_some() { "proxy" } else { "direct" },
        "upstream transport connect started"
    );
    let Some(proxy) = proxy else {
        let result = TcpStream::connect((host, port))
            .await
            .map_err(WitnessError::from);
        match &result {
            Ok(stream) => tracing::info!(
                target: "witness_lib::network::upstream",
                phase = "tcp_connected",
                exchange_id,
                host,
                port,
                peer = ?stream.peer_addr().ok(),
                route = "direct",
                "direct upstream TCP connection established"
            ),
            Err(error) => tracing::warn!(
                target: "witness_lib::network::upstream",
                phase = "tcp_connect_failed",
                exchange_id,
                host,
                port,
                route = "direct",
                error = %error,
                "direct upstream TCP connection failed"
            ),
        }
        return result;
    };
    let proxy_kind = if proxy.kind == "socks5" {
        "socks5"
    } else {
        "http"
    };
    tracing::debug!(
        target: "witness_lib::network::upstream",
        phase = "proxy_route_selected",
        exchange_id,
        proxy_kind,
        proxy_host = %proxy.host,
        proxy_port = proxy.port,
        authentication_configured = !proxy.username.is_empty() || !proxy.password.is_empty(),
        "configured upstream proxy route selected"
    );
    let result = match proxy_kind {
        "socks5" => connect_via_socks5(proxy, host, port, exchange_id).await,
        _ => connect_via_http(proxy, host, port, exchange_id).await,
    };
    match &result {
        Ok(stream) => tracing::info!(
            target: "witness_lib::network::upstream",
            phase = "proxy_tunnel_ready",
            exchange_id,
            host,
            port,
            proxy_kind,
            peer = ?stream.peer_addr().ok(),
            "upstream proxy tunnel ready"
        ),
        Err(error) => tracing::warn!(
            target: "witness_lib::network::upstream",
            phase = "proxy_tunnel_failed",
            exchange_id,
            host,
            port,
            proxy_kind,
            error = %error,
            "upstream proxy tunnel failed"
        ),
    }
    result
}

async fn connect_via_http(
    proxy: &UpstreamProxyConfig,
    host: &str,
    port: u16,
    exchange_id: &str,
) -> Result<TcpStream> {
    // Reject CR/LF in host to prevent request-smuggling via the CONNECT line.
    if host.contains('\r') || host.contains('\n') {
        return Err(WitnessError::Proxy(
            "destination host contains an invalid CR or LF character".into(),
        ));
    }
    tracing::debug!(
        target: "witness_lib::network::upstream",
        phase = "http_proxy_connect_started",
        exchange_id,
        proxy_host = %proxy.host,
        proxy_port = proxy.port,
        destination_host = host,
        destination_port = port,
        "HTTP upstream proxy CONNECT started"
    );
    let mut stream = TcpStream::connect((proxy.host.as_str(), proxy.port))
        .await
        .map_err(WitnessError::from)?;
    let mut request = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
    if !proxy.username.is_empty() || !proxy.password.is_empty() {
        let credentials = STANDARD.encode(format!("{}:{}", proxy.username, proxy.password));
        request.push_str(&format!("Proxy-Authorization: Basic {credentials}\r\n"));
    }
    request.push_str("Proxy-Connection: keep-alive\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(WitnessError::from)?;
    tracing::debug!(
        target: "witness_lib::network::upstream",
        phase = "proxy_connect_request_sent",
        exchange_id,
        destination_host = host,
        destination_port = port,
        request_bytes = request.len(),
        authentication_configured = !proxy.username.is_empty() || !proxy.password.is_empty(),
        "HTTP proxy CONNECT request sent"
    );

    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(end) = find_header_terminator(&buffer) {
            let head = String::from_utf8_lossy(&buffer[..end]);
            let status_line = head.lines().next().unwrap_or_default();
            let status = status_line
                .split_whitespace()
                .nth(1)
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(0);
            tracing::debug!(
                target: "witness_lib::network::upstream",
                phase = "proxy_connect_response_received",
                exchange_id,
                destination_host = host,
                destination_port = port,
                status,
                response_bytes = end,
                "HTTP proxy CONNECT response received"
            );
            if !(200..300).contains(&status) {
                return Err(WitnessError::Proxy(format!(
                    "upstream proxy refused CONNECT to {host}:{port}: {status_line}"
                )));
            }
            return Ok(stream);
        }
        if buffer.len() > 16 * 1024 {
            return Err(WitnessError::Proxy(
                "upstream proxy CONNECT response is too large".into(),
            ));
        }
        let read = stream.read(&mut chunk).await.map_err(WitnessError::from)?;
        if read == 0 {
            return Err(WitnessError::Proxy(
                "upstream proxy closed the connection during CONNECT".into(),
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
}

fn find_header_terminator(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

async fn connect_via_socks5(
    proxy: &UpstreamProxyConfig,
    host: &str,
    port: u16,
    exchange_id: &str,
) -> Result<TcpStream> {
    tracing::debug!(
        target: "witness_lib::network::upstream",
        phase = "socks5_connect_started",
        exchange_id,
        proxy_host = %proxy.host,
        proxy_port = proxy.port,
        destination_host = host,
        destination_port = port,
        authentication_configured = !proxy.username.is_empty() || !proxy.password.is_empty(),
        "SOCKS5 upstream connection started"
    );
    let mut stream = TcpStream::connect((proxy.host.as_str(), proxy.port))
        .await
        .map_err(WitnessError::from)?;

    let use_auth = !proxy.username.is_empty() || !proxy.password.is_empty();
    let methods: &[u8] = if use_auth { &[0x00, 0x02] } else { &[0x00] };
    let mut greeting = vec![0x05_u8, methods.len() as u8];
    greeting.extend_from_slice(methods);
    stream
        .write_all(&greeting)
        .await
        .map_err(WitnessError::from)?;

    let mut selected = [0_u8; 2];
    stream
        .read_exact(&mut selected)
        .await
        .map_err(WitnessError::from)?;
    tracing::debug!(
        target: "witness_lib::network::upstream",
        phase = "socks5_method_selected",
        exchange_id,
        method = selected[1],
        "SOCKS5 authentication method selected"
    );
    if selected[0] != 0x05 {
        return Err(WitnessError::Proxy(
            "upstream SOCKS5 proxy returned an unexpected version".into(),
        ));
    }
    match selected[1] {
        0x00 => {}
        0x02 => authenticate_socks5(&mut stream, proxy, exchange_id).await?,
        0xFF => {
            return Err(WitnessError::Proxy(
                "upstream SOCKS5 proxy rejected all authentication methods".into(),
            ))
        }
        other => {
            return Err(WitnessError::Proxy(format!(
                "upstream SOCKS5 proxy selected an unsupported authentication method: {other}"
            )))
        }
    }

    let mut request = vec![0x05_u8, 0x01, 0x00, 0x03];
    let host_bytes = host.as_bytes();
    if host_bytes.len() > 255 {
        return Err(WitnessError::Proxy(
            "destination host name is too long for a SOCKS5 request".into(),
        ));
    }
    request.push(host_bytes.len() as u8);
    request.extend_from_slice(host_bytes);
    request.extend_from_slice(&port.to_be_bytes());
    stream
        .write_all(&request)
        .await
        .map_err(WitnessError::from)?;
    tracing::debug!(
        target: "witness_lib::network::upstream",
        phase = "socks5_request_sent",
        exchange_id,
        destination_host = host,
        destination_port = port,
        request_bytes = request.len(),
        "SOCKS5 destination request sent"
    );

    let mut reply_head = [0_u8; 4];
    stream
        .read_exact(&mut reply_head)
        .await
        .map_err(WitnessError::from)?;
    tracing::debug!(
        target: "witness_lib::network::upstream",
        phase = "socks5_reply_received",
        exchange_id,
        reply_code = reply_head[1],
        address_type = reply_head[3],
        "SOCKS5 destination reply received"
    );
    if reply_head[0] != 0x05 {
        return Err(WitnessError::Proxy(
            "upstream SOCKS5 proxy returned an unexpected reply version".into(),
        ));
    }
    if reply_head[1] != 0x00 {
        return Err(WitnessError::Proxy(format!(
            "upstream SOCKS5 proxy refused the connection to {host}:{port} (code {})",
            reply_head[1]
        )));
    }
    // Consume the bound address that follows, whose length depends on the address type.
    match reply_head[3] {
        0x01 => skip_exact(&mut stream, 4 + 2).await?,
        0x04 => skip_exact(&mut stream, 16 + 2).await?,
        0x03 => {
            let mut length = [0_u8; 1];
            stream
                .read_exact(&mut length)
                .await
                .map_err(WitnessError::from)?;
            skip_exact(&mut stream, length[0] as usize + 2).await?;
        }
        other => {
            return Err(WitnessError::Proxy(format!(
                "upstream SOCKS5 proxy returned an unsupported address type: {other}"
            )))
        }
    }

    Ok(stream)
}

async fn authenticate_socks5(
    stream: &mut TcpStream,
    proxy: &UpstreamProxyConfig,
    exchange_id: &str,
) -> Result<()> {
    if proxy.username.len() > 255 || proxy.password.len() > 255 {
        return Err(WitnessError::Proxy(
            "upstream SOCKS5 username and password must each be 255 bytes or fewer".into(),
        ));
    }
    let mut request = vec![0x01_u8, proxy.username.len() as u8];
    request.extend_from_slice(proxy.username.as_bytes());
    request.push(proxy.password.len() as u8);
    request.extend_from_slice(proxy.password.as_bytes());
    stream
        .write_all(&request)
        .await
        .map_err(WitnessError::from)?;
    tracing::debug!(
        target: "witness_lib::network::upstream",
        phase = "socks5_authentication_sent",
        exchange_id,
        username_bytes = proxy.username.len(),
        password_configured = !proxy.password.is_empty(),
        "SOCKS5 username/password authentication sent"
    );

    let mut response = [0_u8; 2];
    stream
        .read_exact(&mut response)
        .await
        .map_err(WitnessError::from)?;
    tracing::debug!(
        target: "witness_lib::network::upstream",
        phase = "socks5_authentication_response",
        exchange_id,
        status = response[1],
        "SOCKS5 authentication response received"
    );
    if response[1] != 0x00 {
        return Err(WitnessError::Proxy(
            "upstream SOCKS5 proxy rejected the supplied credentials".into(),
        ));
    }
    Ok(())
}

async fn skip_exact(stream: &mut TcpStream, length: usize) -> Result<()> {
    let mut buffer = vec![0_u8; length];
    stream
        .read_exact(&mut buffer)
        .await
        .map_err(WitnessError::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn http_connect_tunnels_through_upstream_proxy() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                let read = connection.read(&mut chunk).await.unwrap();
                buffer.extend_from_slice(&chunk[..read]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&buffer).to_string();
            assert!(head.starts_with("CONNECT example.test:443 HTTP/1.1"));
            assert!(head.contains("Proxy-Authorization: Basic"));
            connection
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .await
                .unwrap();
        });

        let proxy = UpstreamProxyConfig {
            enabled: true,
            kind: "http".into(),
            host: address.ip().to_string(),
            port: address.port(),
            username: "user".into(),
            password: "pass".into(),
        };
        connect(Some(&proxy), "example.test", 443).await.unwrap();
    }

    #[tokio::test]
    async fn http_connect_surfaces_non_success_status() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut chunk = [0_u8; 4096];
            let _ = connection.read(&mut chunk).await.unwrap();
            connection
                .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n")
                .await
                .unwrap();
        });

        let proxy = UpstreamProxyConfig {
            enabled: true,
            kind: "http".into(),
            host: address.ip().to_string(),
            port: address.port(),
            ..UpstreamProxyConfig::default()
        };
        let error = connect(Some(&proxy), "example.test", 443)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("407"));
    }

    #[tokio::test]
    async fn socks5_connect_completes_without_authentication() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut greeting = [0_u8; 2];
            connection.read_exact(&mut greeting).await.unwrap();
            let mut methods = vec![0_u8; greeting[1] as usize];
            connection.read_exact(&mut methods).await.unwrap();
            connection.write_all(&[0x05, 0x00]).await.unwrap();

            let mut head = [0_u8; 5];
            connection.read_exact(&mut head).await.unwrap();
            assert_eq!(&head[..4], [0x05, 0x01, 0x00, 0x03]);
            assert_eq!(head[4] as usize, "example.test".len());
            let mut rest = vec![0_u8; head[4] as usize - 1 + 2];
            connection.read_exact(&mut rest).await.unwrap();

            connection
                .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();
        });

        let proxy = UpstreamProxyConfig {
            enabled: true,
            kind: "socks5".into(),
            host: address.ip().to_string(),
            port: address.port(),
            ..UpstreamProxyConfig::default()
        };
        connect(Some(&proxy), "example.test", 443).await.unwrap();
    }

    #[tokio::test]
    async fn socks5_connect_authenticates_with_credentials() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut greeting = [0_u8; 2];
            connection.read_exact(&mut greeting).await.unwrap();
            let mut methods = vec![0_u8; greeting[1] as usize];
            connection.read_exact(&mut methods).await.unwrap();
            assert!(methods.contains(&0x02));
            connection.write_all(&[0x05, 0x02]).await.unwrap();

            let mut auth_head = [0_u8; 2];
            connection.read_exact(&mut auth_head).await.unwrap();
            let mut username = vec![0_u8; auth_head[1] as usize];
            connection.read_exact(&mut username).await.unwrap();
            assert_eq!(username, b"user");
            let mut password_length = [0_u8; 1];
            connection.read_exact(&mut password_length).await.unwrap();
            let mut password = vec![0_u8; password_length[0] as usize];
            connection.read_exact(&mut password).await.unwrap();
            assert_eq!(password, b"pass");
            connection.write_all(&[0x01, 0x00]).await.unwrap();

            let mut head = [0_u8; 5];
            connection.read_exact(&mut head).await.unwrap();
            let mut rest = vec![0_u8; head[4] as usize - 1 + 2];
            connection.read_exact(&mut rest).await.unwrap();
            connection
                .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();
        });

        let proxy = UpstreamProxyConfig {
            enabled: true,
            kind: "socks5".into(),
            host: address.ip().to_string(),
            port: address.port(),
            username: "user".into(),
            password: "pass".into(),
        };
        connect(Some(&proxy), "example.test", 443).await.unwrap();
    }

    #[tokio::test]
    async fn socks5_connect_surfaces_refusal() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut connection, _) = listener.accept().await.unwrap();
            let mut greeting = [0_u8; 2];
            connection.read_exact(&mut greeting).await.unwrap();
            let mut methods = vec![0_u8; greeting[1] as usize];
            connection.read_exact(&mut methods).await.unwrap();
            connection.write_all(&[0x05, 0x00]).await.unwrap();

            let mut head = [0_u8; 5];
            connection.read_exact(&mut head).await.unwrap();
            let mut rest = vec![0_u8; head[4] as usize - 1 + 2];
            connection.read_exact(&mut rest).await.unwrap();
            connection
                .write_all(&[0x05, 0x05, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();
        });

        let proxy = UpstreamProxyConfig {
            enabled: true,
            kind: "socks5".into(),
            host: address.ip().to_string(),
            port: address.port(),
            ..UpstreamProxyConfig::default()
        };
        let error = connect(Some(&proxy), "example.test", 443)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("refused"));
    }

    #[tokio::test]
    async fn http_connect_rejects_crlf_in_host() {
        let proxy = UpstreamProxyConfig {
            enabled: true,
            kind: "http".into(),
            host: "127.0.0.1".into(),
            port: 8080,
            ..UpstreamProxyConfig::default()
        };
        let error = connect(Some(&proxy), "evil.test\r\nInjected: yes", 443)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("CR or LF"));
    }
}
