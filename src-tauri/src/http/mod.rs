use std::{borrow::Cow, io::Read};

use ::http::{
    header, HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode, Version,
};
use brotli::Decompressor;
use bytes::Bytes;
use flate2::read::{GzDecoder, ZlibDecoder};

use crate::error::{Result, WitnessError};

const MAX_HEADERS: usize = 128;
pub const MAX_MESSAGE_SIZE: usize = 100 * 1024 * 1024;

pub fn parse_request(bytes: &[u8]) -> Result<Option<(Request<Vec<u8>>, usize)>> {
    reject_http2(bytes)?;
    let mut raw_headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut parsed = httparse::Request::new(&mut raw_headers);
    let header_end = match parsed
        .parse(bytes)
        .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?
    {
        httparse::Status::Complete(length) => length,
        httparse::Status::Partial => return Ok(None),
    };

    let method = parsed
        .method
        .ok_or_else(|| WitnessError::InvalidHttp("missing method".into()))?;
    let path = parsed
        .path
        .ok_or_else(|| WitnessError::InvalidHttp("missing request target".into()))?;
    let version = parsed
        .version
        .ok_or_else(|| WitnessError::InvalidHttp("missing version".into()))?;
    if version != 1 {
        return Err(WitnessError::Http2Unsupported);
    }

    let headers = convert_headers(parsed.headers)?;
    reject_upgrade_headers(&headers)?;
    let Some((body, consumed)) = extract_body(bytes, header_end, &headers)? else {
        return Ok(None);
    };
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .version(Version::HTTP_11);
    *builder
        .headers_mut()
        .ok_or_else(|| WitnessError::InvalidHttp("invalid request builder".into()))? = headers;
    let request = builder
        .body(body)
        .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?;
    Ok(Some((request, consumed)))
}

pub(crate) fn parse_replay_request(bytes: &[u8]) -> Result<Option<(Request<Vec<u8>>, usize)>> {
    let normalized = normalize_burp_http2_start_line(bytes);
    let added_length = normalized.len().saturating_sub(bytes.len());
    Ok(parse_request(normalized.as_ref())?
        .map(|(request, consumed)| (request, consumed.saturating_sub(added_length))))
}

pub fn parse_response(bytes: &[u8]) -> Result<Option<(Response<Vec<u8>>, usize)>> {
    parse_response_for_method(bytes, &Method::GET)
}

pub fn parse_response_for_method(
    bytes: &[u8],
    request_method: &Method,
) -> Result<Option<(Response<Vec<u8>>, usize)>> {
    let mut raw_headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
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
    let headers = convert_headers(parsed.headers)?;
    let status_code = StatusCode::from_u16(status)
        .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?;
    let (body, consumed) = if request_method == Method::HEAD
        || status_code.is_informational()
        || status_code == StatusCode::NO_CONTENT
        || status_code == StatusCode::NOT_MODIFIED
    {
        (Vec::new(), header_end)
    } else if !is_chunked(&headers) && !headers.contains_key(header::CONTENT_LENGTH) {
        (bytes[header_end..].to_vec(), bytes.len())
    } else {
        let Some(body) = extract_body(bytes, header_end, &headers)? else {
            return Ok(None);
        };
        body
    };
    let mut builder = Response::builder()
        .status(status_code)
        .version(Version::HTTP_11);
    *builder
        .headers_mut()
        .ok_or_else(|| WitnessError::InvalidHttp("invalid response builder".into()))? = headers;
    let response = builder
        .body(body)
        .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?;
    Ok(Some((response, consumed)))
}

fn convert_headers(headers: &[httparse::Header<'_>]) -> Result<HeaderMap> {
    let mut result = HeaderMap::new();
    for item in headers {
        let name = HeaderName::from_bytes(item.name.as_bytes())
            .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?;
        let value = HeaderValue::from_bytes(item.value)
            .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?;
        result.append(name, value);
    }
    Ok(result)
}

fn extract_body(
    bytes: &[u8],
    header_end: usize,
    headers: &HeaderMap,
) -> Result<Option<(Vec<u8>, usize)>> {
    if is_chunked(headers) {
        return decode_chunked(&bytes[header_end..])
            .map(|result| result.map(|(body, consumed)| (body, header_end + consumed)));
    }

    let length = content_length(headers)?;
    if length > MAX_MESSAGE_SIZE {
        return Err(WitnessError::InvalidHttp(
            "message body exceeds 100 MiB".into(),
        ));
    }
    if bytes.len() < header_end + length {
        return Ok(None);
    }
    Ok(Some((
        bytes[header_end..header_end + length].to_vec(),
        header_end + length,
    )))
}

fn content_length(headers: &HeaderMap) -> Result<usize> {
    match headers.get(header::CONTENT_LENGTH) {
        Some(value) => value
            .to_str()
            .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?
            .trim()
            .parse()
            .map_err(|_| WitnessError::InvalidHttp("invalid Content-Length".into())),
        None => Ok(0),
    }
}

pub fn serialize_request(request: &Request<Vec<u8>>) -> Vec<u8> {
    let target = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let mut output = format!("{} {} HTTP/1.1\r\n", request.method(), target).into_bytes();
    // HTTP/2 carries the destination in the `:authority` pseudo-header, which the
    // `http` crate represents on the URI rather than in the regular header map.
    // Materialize it when converting to raw HTTP/1.1 so interception edits and
    // HTTP/1.1 upstream fallbacks retain a routable Host header.
    if !request.headers().contains_key(header::HOST) {
        if let Some(authority) = request.uri().authority() {
            output.extend_from_slice(b"Host: ");
            output.extend_from_slice(authority.as_str().as_bytes());
            output.extend_from_slice(b"\r\n");
        }
    }
    serialize_headers_and_body(request.headers(), request.body(), &mut output, true);
    output
}

pub fn serialize_response(response: &Response<Vec<u8>>) -> Vec<u8> {
    let reason = response.status().canonical_reason().unwrap_or("Unknown");
    let mut output = format!("HTTP/1.1 {} {}\r\n", response.status().as_u16(), reason).into_bytes();
    let permits_body = !response.status().is_informational()
        && response.status() != StatusCode::NO_CONTENT
        && response.status() != StatusCode::NOT_MODIFIED;
    serialize_headers_and_body(
        response.headers(),
        response.body(),
        &mut output,
        permits_body,
    );
    output
}

fn serialize_headers_and_body(
    headers: &HeaderMap,
    body: &[u8],
    output: &mut Vec<u8>,
    add_implicit_length: bool,
) {
    let chunked = is_chunked(headers);
    let has_length = headers.contains_key(header::CONTENT_LENGTH);
    for (name, value) in headers {
        if name.as_str().eq_ignore_ascii_case("http2-settings") {
            continue;
        }
        output.extend_from_slice(name.as_str().as_bytes());
        output.extend_from_slice(b": ");
        output.extend_from_slice(value.as_bytes());
        output.extend_from_slice(b"\r\n");
    }
    if add_implicit_length && !chunked && !has_length {
        output.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    output.extend_from_slice(b"\r\n");
    if chunked {
        if !body.is_empty() {
            output.extend_from_slice(format!("{:X}\r\n", body.len()).as_bytes());
            output.extend_from_slice(body);
            output.extend_from_slice(b"\r\n");
        }
        output.extend_from_slice(b"0\r\n\r\n");
    } else {
        output.extend_from_slice(body);
    }
}

pub fn is_keep_alive(headers: &HeaderMap) -> bool {
    !headers
        .get(header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("close"))
}

pub fn is_chunked(headers: &HeaderMap) -> bool {
    headers
        .get(header::TRANSFER_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
        })
}

fn decode_chunked(input: &[u8]) -> Result<Option<(Vec<u8>, usize)>> {
    let mut cursor = 0;
    let mut output = Vec::new();
    loop {
        let Some(line_end) = find_bytes(&input[cursor..], b"\r\n") else {
            return Ok(None);
        };
        let size_text = std::str::from_utf8(&input[cursor..cursor + line_end])
            .map_err(|_| WitnessError::InvalidHttp("invalid chunk size".into()))?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| WitnessError::InvalidHttp("invalid chunk size".into()))?;
        cursor += line_end + 2;
        if size == 0 {
            if input[cursor..].starts_with(b"\r\n") {
                return Ok(Some((output, cursor + 2)));
            }
            let Some(trailer_end) = find_bytes(&input[cursor..], b"\r\n\r\n") else {
                return Ok(None);
            };
            return Ok(Some((output, cursor + trailer_end + 4)));
        }
        if output.len().saturating_add(size) > MAX_MESSAGE_SIZE {
            return Err(WitnessError::InvalidHttp(
                "message body exceeds 100 MiB".into(),
            ));
        }
        if input.len() < cursor + size + 2 {
            return Ok(None);
        }
        output.extend_from_slice(&input[cursor..cursor + size]);
        cursor += size;
        if &input[cursor..cursor + 2] != b"\r\n" {
            return Err(WitnessError::InvalidHttp("chunk missing terminator".into()));
        }
        cursor += 2;
    }
}

pub fn decompress_response(mut response: Response<Vec<u8>>) -> Result<Response<Vec<u8>>> {
    let Some(raw) = response
        .headers()
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .map(str::to_ascii_lowercase)
    else {
        return Ok(response);
    };
    // Handle stacked `Content-Encoding` values (e.g. `gzip, br`): split on
    // ',' and peel layers outside-in (reverse application order).
    let encodings: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    if encodings.is_empty()
        || !encodings
            .iter()
            .all(|encoding| matches!(encoding.as_str(), "gzip" | "deflate" | "br"))
    {
        return Ok(response);
    }
    let mut output = std::mem::take(response.body_mut());
    for encoding in encodings.iter().rev() {
        output = match encoding.as_str() {
            "gzip" => read_decompressed_limited(GzDecoder::new(output.as_slice()))?,
            "deflate" => read_decompressed_limited(ZlibDecoder::new(output.as_slice()))?,
            "br" => read_decompressed_limited(Decompressor::new(output.as_slice(), 4_096))?,
            _ => {
                *response.body_mut() = output;
                return Ok(response);
            }
        };
    }
    *response.body_mut() = output;
    response.headers_mut().remove(header::CONTENT_ENCODING);
    response.headers_mut().remove(header::TRANSFER_ENCODING);
    let length = HeaderValue::from_str(&response.body().len().to_string())
        .map_err(|error| WitnessError::InvalidHttp(error.to_string()))?;
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, length);
    Ok(response)
}

fn read_decompressed_limited<R: Read>(mut reader: R) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > MAX_MESSAGE_SIZE {
            return Err(WitnessError::InvalidHttp(
                "decompressed response exceeds 100 MiB".into(),
            ));
        }
        // `reserve` (not `reserve_exact`) amortizes growth across chunks.
        output.reserve(read);
        output.extend_from_slice(&buffer[..read]);
    }
}

pub fn response_needs_decompression(response: &Response<Vec<u8>>, compression_mode: &str) -> bool {
    let supported_encoding = response
        .headers()
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .map(str::to_ascii_lowercase)
        .is_some_and(|raw| {
            let encodings: Vec<&str> = raw
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect();
            !encodings.is_empty()
                && encodings
                    .iter()
                    .all(|encoding| matches!(*encoding, "gzip" | "deflate" | "br"))
        });
    if !supported_encoding {
        return false;
    }

    match compression_mode {
        "decompressAll" => true,
        "decompressText" => response_is_text(response),
        _ => false,
    }
}

pub fn apply_response_compression(
    response: Response<Vec<u8>>,
    compression_mode: &str,
) -> Result<Response<Vec<u8>>> {
    if response_needs_decompression(&response, compression_mode) {
        decompress_response(response)
    } else {
        Ok(response)
    }
}

fn response_is_text(response: &Response<Vec<u8>>) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_ascii_lowercase)
        .is_some_and(|value| {
            value.starts_with("text/")
                || value.contains("json")
                || value.contains("xml")
                || value.contains("javascript")
        })
}

pub fn to_bytes(body: Vec<u8>) -> Bytes {
    Bytes::from(body)
}

fn reject_http2(bytes: &[u8]) -> Result<()> {
    if bytes.starts_with(b"PRI * HTTP/2.0") {
        Err(WitnessError::Http2Unsupported)
    } else {
        Ok(())
    }
}

fn normalize_burp_http2_start_line(bytes: &[u8]) -> Cow<'_, [u8]> {
    let Some(line_end) = bytes.iter().position(|byte| *byte == b'\n') else {
        return Cow::Borrowed(bytes);
    };
    let line_end_without_lf = if line_end > 0 && bytes[line_end - 1] == b'\r' {
        line_end - 1
    } else {
        line_end
    };
    let line = &bytes[..line_end_without_lf];
    let Some(version_start) = line.iter().rposition(|byte| *byte == b' ') else {
        return Cow::Borrowed(bytes);
    };
    let version = &line[version_start + 1..];
    if version != b"HTTP/2" && version != b"HTTP/2.0" {
        return Cow::Borrowed(bytes);
    }

    let mut normalized = Vec::with_capacity(bytes.len() + 2);
    normalized.extend_from_slice(&bytes[..version_start + 1]);
    normalized.extend_from_slice(b"HTTP/1.1");
    normalized.extend_from_slice(&bytes[line_end_without_lf..]);
    Cow::Owned(normalized)
}

fn reject_upgrade_headers(headers: &HeaderMap) -> Result<()> {
    let upgrades_h2 = headers
        .get(header::UPGRADE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("h2c"));
    if upgrades_h2 || headers.contains_key("http2-settings") {
        Err(WitnessError::Http2Unsupported)
    } else {
        Ok(())
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use brotli::CompressorWriter;
    use flate2::{
        write::{GzEncoder, ZlibEncoder},
        Compression,
    };

    use super::*;

    #[test]
    fn request_round_trip() {
        let request = Request::builder()
            .method("POST")
            .uri("http://example.test/api?q=1")
            .header("Host", "example.test")
            .body(b"hello".to_vec())
            .unwrap();
        let raw = serialize_request(&request);
        let (parsed, consumed) = parse_request(&raw).unwrap().unwrap();
        assert_eq!(consumed, raw.len());
        assert_eq!(parsed.method(), "POST");
        assert_eq!(parsed.uri(), "/api?q=1");
        assert_eq!(parsed.body(), b"hello");
    }

    #[test]
    fn replay_parser_accepts_burps_textual_http2_request_format() {
        for version in ["HTTP/2", "HTTP/2.0"] {
            let raw = format!(
                "GET /replay?sample=true {version}\r\nHost: example.test\r\nTe: trailers\r\n\r\n"
            );
            let (request, consumed) = parse_replay_request(raw.as_bytes()).unwrap().unwrap();

            assert_eq!(consumed, raw.len());
            assert_eq!(request.method(), "GET");
            assert_eq!(
                request.uri().path_and_query().unwrap(),
                "/replay?sample=true"
            );
            assert_eq!(request.version(), Version::HTTP_11);
        }
    }

    #[test]
    fn http2_authority_is_materialized_as_host_when_serialized() {
        let request = Request::builder()
            .method("GET")
            .uri("https://example.test:8443/api?q=1")
            .version(Version::HTTP_2)
            .body(Vec::new())
            .unwrap();

        let raw = serialize_request(&request);
        assert!(raw
            .windows(b"\r\nHost: example.test:8443\r\n".len())
            .any(|window| window == b"\r\nHost: example.test:8443\r\n"));

        let (parsed, consumed) = parse_request(&raw).unwrap().unwrap();
        assert_eq!(consumed, raw.len());
        assert_eq!(parsed.uri(), "/api?q=1");
        assert_eq!(
            parsed.headers().get(header::HOST).unwrap(),
            "example.test:8443"
        );
    }

    #[test]
    fn serialization_does_not_duplicate_an_existing_host_header() {
        let request = Request::builder()
            .method("GET")
            .uri("https://uri.example.test/api")
            .version(Version::HTTP_2)
            .header(header::HOST, "header.example.test")
            .body(Vec::new())
            .unwrap();

        let raw = serialize_request(&request);
        let host_lines = raw
            .split(|byte| *byte == b'\n')
            .filter(|line| line.len() >= 5 && line[..5].eq_ignore_ascii_case(b"host:"))
            .count();
        assert_eq!(host_lines, 1);
        assert!(raw
            .windows(b"host: header.example.test\r\n".len())
            .any(|window| window.eq_ignore_ascii_case(b"host: header.example.test\r\n")));
    }

    #[test]
    fn websocket_upgrade_headers_survive_serialization() {
        let request = Request::builder()
            .method("GET")
            .uri("/socket")
            .header("Host", "example.test")
            .header("Connection", "keep-alive, Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .body(Vec::new())
            .unwrap();
        let raw = serialize_request(&request);

        assert!(raw
            .windows(b"\r\nupgrade: websocket\r\n".len())
            .any(|window| window.eq_ignore_ascii_case(b"\r\nupgrade: websocket\r\n")));
        let (parsed, _) = parse_request(&raw).unwrap().unwrap();
        assert_eq!(parsed.headers().get(header::UPGRADE).unwrap(), "websocket");

        let response = Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .body(Vec::new())
            .unwrap();
        let raw = serialize_response(&response);
        assert!(!raw
            .windows(b"content-length".len())
            .any(|window| window.eq_ignore_ascii_case(b"content-length")));
    }

    #[test]
    fn chunked_response_round_trip() {
        let response = Response::builder()
            .status(200)
            .header("Transfer-Encoding", "chunked")
            .body(b"chunk body".to_vec())
            .unwrap();
        let raw = serialize_response(&response);
        let (parsed, consumed) = parse_response(&raw).unwrap().unwrap();
        assert_eq!(consumed, raw.len());
        assert_eq!(parsed.body(), b"chunk body");
    }

    #[test]
    fn chunk_extensions_and_trailers_do_not_consume_next_message() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip, chunked\r\n\r\n4;name=value\r\ntest\r\n0\r\nX-Trace: complete\r\n\r\nNEXT";
        let (parsed, consumed) = parse_response(raw).unwrap().unwrap();
        assert_eq!(parsed.body(), b"test");
        assert_eq!(&raw[consumed..], b"NEXT");
    }

    #[test]
    fn binary_bodies_round_trip_without_utf8_conversion() {
        let body = vec![0, 0xff, 0x80, b'\r', b'\n'];
        let response = Response::builder()
            .status(200)
            .header("Content-Type", "application/octet-stream")
            .body(body.clone())
            .unwrap();
        let raw = serialize_response(&response);
        let (parsed, _) = parse_response(&raw).unwrap().unwrap();
        assert_eq!(parsed.body(), &body);
    }

    #[test]
    fn malformed_input_does_not_panic() {
        assert!(parse_request(b"not http\r\n\r\n").is_err());
        assert!(matches!(
            parse_request(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n"),
            Err(WitnessError::Http2Unsupported)
        ));
    }

    #[test]
    fn decompresses_gzip_and_updates_headers() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"compressed").unwrap();
        let response = Response::builder()
            .header("Content-Encoding", "gzip")
            .body(encoder.finish().unwrap())
            .unwrap();
        let response = decompress_response(response).unwrap();
        assert_eq!(response.body(), b"compressed");
        assert!(!response.headers().contains_key("Content-Encoding"));
    }

    #[test]
    fn decompresses_deflate() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"deflated").unwrap();
        let response = Response::builder()
            .header("Content-Encoding", "deflate")
            .body(encoder.finish().unwrap())
            .unwrap();
        assert_eq!(decompress_response(response).unwrap().body(), b"deflated");
    }

    #[test]
    fn decompresses_brotli() {
        let mut encoded = Vec::new();
        {
            let mut encoder = CompressorWriter::new(&mut encoded, 4_096, 5, 22);
            encoder.write_all(b"brotli").unwrap();
        }
        let response = Response::builder()
            .header("Content-Encoding", "br")
            .body(encoded)
            .unwrap();
        assert_eq!(decompress_response(response).unwrap().body(), b"brotli");
    }

    #[test]
    fn response_compression_policy_is_shared_across_transports() {
        let gzip_response = |content_type: &'static str| {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(b"compressed").unwrap();
            Response::builder()
                .header("Content-Type", content_type)
                .header("Content-Encoding", "gzip")
                .body(encoder.finish().unwrap())
                .unwrap()
        };

        let json = gzip_response("Application/JSON; charset=utf-8");
        assert!(response_needs_decompression(&json, "decompressAll"));
        assert!(response_needs_decompression(&json, "decompressText"));
        assert!(!response_needs_decompression(&json, "passThrough"));
        let json = apply_response_compression(json, "decompressText").unwrap();
        assert_eq!(json.body(), b"compressed");
        assert!(!json.headers().contains_key(header::CONTENT_ENCODING));

        let binary = gzip_response("application/octet-stream");
        assert!(!response_needs_decompression(&binary, "decompressText"));
        let binary = apply_response_compression(binary, "decompressText").unwrap();
        assert!(binary.headers().contains_key(header::CONTENT_ENCODING));
    }

    #[test]
    fn connection_close_body_is_reconstructed() {
        let raw = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nbody until eof";
        let (response, consumed) = parse_response(raw).unwrap().unwrap();
        assert_eq!(consumed, raw.len());
        assert_eq!(response.body(), b"body until eof");
    }

    #[test]
    fn head_response_does_not_wait_for_declared_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 42\r\n\r\n";
        assert!(parse_response(raw).unwrap().is_none());
        let (response, consumed) = parse_response_for_method(raw, &Method::HEAD)
            .unwrap()
            .unwrap();
        assert_eq!(consumed, raw.len());
        assert!(response.body().is_empty());
    }
}
