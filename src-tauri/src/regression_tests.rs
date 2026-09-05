use ::http::Response;

use crate::{
    comparer, decoder,
    http::{parse_response, serialize_response},
};

#[test]
fn pipelined_bytes_survive_chunked_trailers() {
    let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n0\r\nX-End: yes\r\n\r\nHTTP/1.1";
    let (response, consumed) = parse_response(raw).unwrap().unwrap();
    assert_eq!(response.body(), b"a");
    assert_eq!(&raw[consumed..], b"HTTP/1.1");
}

#[test]
fn binary_http_serialisation_is_lossless() {
    let bytes = vec![0, 0xff, 0xfe, 0x7f];
    let response = Response::builder().body(bytes.clone()).unwrap();
    let raw = serialize_response(&response);
    assert_eq!(parse_response(&raw).unwrap().unwrap().0.body(), &bytes);
}

#[test]
fn unicode_and_empty_diff_edges_remain_stable() {
    let encoded = decoder::transform("🔨 café", "unicodeEncode", true)
        .unwrap()
        .output;
    assert_eq!(
        decoder::transform(&encoded, "unicodeDecode", true)
            .unwrap()
            .output,
        "🔨 café"
    );
    let diff = comparer::compare("", "", "character");
    assert_eq!((diff.additions, diff.deletions, diff.unchanged), (0, 0, 0));
}
