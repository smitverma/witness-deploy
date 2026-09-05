use base64::{engine::general_purpose, Engine as _};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{Result, WitnessError};

const URL_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'!')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'=')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b']');

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecodeResult {
    pub output: String,
    pub detected: String,
    pub steps: Vec<String>,
}

/// Maximum accepted decoder input (5 MiB). Bounds CPU/memory on hostile
/// pastes before any codec runs.
pub const MAX_DECODER_INPUT: usize = 5 * 1024 * 1024;

/// Apply a single decoder operation.
///
/// `pad_base64_output` (historically named `padding`) only affects
/// `base64Encode` (`STANDARD` vs `STANDARD_NO_PAD`). Every other operation
/// ignores it; it is kept for backwards compatibility with existing callers.
pub fn transform(input: &str, operation: &str, pad_base64_output: bool) -> Result<DecodeResult> {
    if input.len() > MAX_DECODER_INPUT {
        return Err(WitnessError::Other(anyhow::anyhow!(
            "decoder input exceeds 5 MiB"
        )));
    }
    let output = match operation {
        "urlEncode" => utf8_percent_encode(input, URL_ENCODE_SET).to_string(),
        "urlDecode" => percent_decode_str(input)
            .decode_utf8()
            .map_err(|error| WitnessError::Other(anyhow::anyhow!(error)))?
            .into_owned(),
        "base64Encode" => {
            if pad_base64_output {
                general_purpose::STANDARD.encode(input)
            } else {
                general_purpose::STANDARD_NO_PAD.encode(input)
            }
        }
        "base64Decode" => decode_base64(input)?,
        "base64UrlEncode" => general_purpose::URL_SAFE_NO_PAD.encode(input),
        "base64UrlDecode" => decode_base64_url(input)?,
        "hexEncode" => input
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        "hexDecode" => decode_hex(input)?,
        "htmlEncode" => html_escape::encode_safe(input).to_string(),
        "htmlDecode" => html_escape::decode_html_entities(input).to_string(),
        "unicodeEncode" => input
            .encode_utf16()
            .map(|unit| format!("\\u{unit:04X}"))
            .collect(),
        "unicodeDecode" => decode_unicode(input)?,
        "formEncode" => form_encode(input),
        "formDecode" => form_decode(input)?,
        "jsonPretty" => format_json(input, true)?,
        "jsonMinify" => format_json(input, false)?,
        "queryToJson" => query_to_json(input)?,
        "jsonToQuery" => json_to_query(input)?,
        "jwtDecode" => decode_jwt(input)?,
        "smartDecode" => return smart_decode(input),
        "detect" => input.to_string(),
        _ => {
            return Err(WitnessError::Other(anyhow::anyhow!(
                "unknown decoder operation: {operation}"
            )))
        }
    };
    Ok(DecodeResult {
        detected: detect_encoding(if operation == "detect" {
            input
        } else {
            &output
        }),
        output,
        steps: vec![operation.into()],
    })
}

pub fn detect_encoding(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.split('.').count() == 3
        && trimmed
            .split('.')
            .take(2)
            .all(|part| general_purpose::URL_SAFE_NO_PAD.decode(part).is_ok())
    {
        return "JWT".into();
    }
    if let Some(hash) = detect_hash(trimmed) {
        return hash;
    }
    if trimmed.contains("%") && percent_decode_str(trimmed).decode_utf8().is_ok() {
        return "URL encoded".into();
    }
    if trimmed.contains("&") && trimmed.contains(';') {
        return "HTML entities".into();
    }
    if trimmed.contains("\\u") {
        return "Unicode escapes".into();
    }
    if is_hex(trimmed) {
        return "Hex".into();
    }
    if trimmed.len() >= 4 && decode_base64(trimmed).is_ok() {
        return "Base64".into();
    }
    "Plain text".into()
}

pub fn detect_hash(input: &str) -> Option<String> {
    if !input.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let name = match input.len() {
        32 => "MD5 / NTLM",
        40 => "SHA-1",
        56 => "SHA-224",
        64 => "SHA-256",
        96 => "SHA-384",
        128 => "SHA-512",
        _ => return None,
    };
    Some(format!("{name} candidate ({} bits)", input.len() * 4))
}

fn smart_decode(input: &str) -> Result<DecodeResult> {
    let mut value = input.to_string();
    let mut steps = Vec::new();
    for _ in 0..8 {
        let candidate = if value.contains('%') {
            percent_decode_str(&value)
                .decode_utf8()
                .ok()
                .map(|value| value.into_owned())
                .filter(|next| next != &value)
                .map(|value| (value, "URL"))
        } else if value.contains("&") && value.contains(';') {
            let next = html_escape::decode_html_entities(&value).to_string();
            (next != value).then_some((next, "HTML"))
        } else if value.contains("\\u") {
            decode_unicode(&value)
                .ok()
                .filter(|next| next != &value)
                .map(|value| (value, "Unicode"))
        } else if is_hex(value.trim()) {
            decode_hex(&value)
                .ok()
                .filter(|next| next != &value)
                .filter(|next| is_likely_text(next))
                .map(|value| (value, "Hex"))
        } else if value.trim().len() >= 4 {
            decode_base64(&value)
                .ok()
                .filter(|next| next != &value)
                .filter(|next| is_likely_text(next))
                .map(|value| (value, "Base64"))
        } else {
            None
        };
        let Some((next, step)) = candidate else { break };
        if next == value || next.chars().any(|character| character == '\0') {
            break;
        }
        value = next;
        steps.push(step.into());
    }
    Ok(DecodeResult {
        detected: detect_encoding(&value),
        output: value,
        steps,
    })
}

fn decode_base64(input: &str) -> Result<String> {
    let input = input.trim();
    let bytes = general_purpose::STANDARD
        .decode(input)
        .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(input))
        .or_else(|_| general_purpose::URL_SAFE.decode(input))
        .or_else(|_| general_purpose::URL_SAFE_NO_PAD.decode(input))
        .map_err(|error| WitnessError::Other(anyhow::anyhow!(error)))?;
    String::from_utf8(bytes).map_err(|error| WitnessError::Other(error.into()))
}

fn decode_base64_url(input: &str) -> Result<String> {
    let input = input.trim();
    let bytes = general_purpose::URL_SAFE
        .decode(input)
        .or_else(|_| general_purpose::URL_SAFE_NO_PAD.decode(input))
        .map_err(|error| WitnessError::Other(anyhow::anyhow!(error)))?;
    String::from_utf8(bytes).map_err(|error| WitnessError::Other(error.into()))
}

fn form_encode(input: &str) -> String {
    utf8_percent_encode(input, URL_ENCODE_SET)
        .to_string()
        .replace("%20", "+")
}

fn form_decode(input: &str) -> Result<String> {
    percent_decode_str(&input.replace('+', " "))
        .decode_utf8()
        .map_err(|error| WitnessError::Other(anyhow::anyhow!(error)))
        .map(|value| value.into_owned())
}

fn format_json(input: &str, pretty: bool) -> Result<String> {
    let value: Value = serde_json::from_str(input)
        .map_err(|error| WitnessError::Other(anyhow::anyhow!("invalid JSON: {error}")))?;
    let result = if pretty {
        serde_json::to_string_pretty(&value)
    } else {
        serde_json::to_string(&value)
    };
    result.map_err(|error| WitnessError::Other(error.into()))
}

fn query_to_json(input: &str) -> Result<String> {
    let query = input.trim().strip_prefix('?').unwrap_or(input.trim());
    let mut object = Map::new();
    for item in query.split('&').filter(|item| !item.is_empty()) {
        let (raw_name, raw_value) = item.split_once('=').unwrap_or((item, ""));
        let name = form_decode(raw_name)?;
        let value = Value::String(form_decode(raw_value)?);
        match object.get_mut(&name) {
            Some(Value::Array(values)) => values.push(value),
            Some(existing) => {
                let previous = existing.take();
                *existing = Value::Array(vec![previous, value]);
            }
            None => {
                object.insert(name, value);
            }
        }
    }
    serde_json::to_string_pretty(&Value::Object(object))
        .map_err(|error| WitnessError::Other(error.into()))
}

fn json_to_query(input: &str) -> Result<String> {
    let value: Value = serde_json::from_str(input)
        .map_err(|error| WitnessError::Other(anyhow::anyhow!("invalid JSON: {error}")))?;
    let Value::Object(object) = value else {
        return Err(WitnessError::Other(anyhow::anyhow!(
            "query encoding expects a JSON object"
        )));
    };

    let mut pairs = Vec::new();
    for (name, value) in object {
        match value {
            Value::Array(values) => {
                for value in values {
                    pairs.push(query_pair(&name, &value)?);
                }
            }
            value => pairs.push(query_pair(&name, &value)?),
        }
    }
    Ok(pairs.join("&"))
}

fn query_pair(name: &str, value: &Value) -> Result<String> {
    let value = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => {
            return Err(WitnessError::Other(anyhow::anyhow!(
                "query values must be strings, numbers, booleans, null, or arrays of those values"
            )))
        }
    };
    Ok(format!("{}={}", form_encode(name), form_encode(&value)))
}

fn is_likely_text(value: &str) -> bool {
    let characters = value.chars().count();
    characters > 0
        && value
            .chars()
            .filter(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
            .count()
            * 8
            <= characters
}

fn is_hex(input: &str) -> bool {
    let compact: String = input
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    compact.len() >= 2
        && compact.len().is_multiple_of(2)
        && compact.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn decode_hex(input: &str) -> Result<String> {
    let compact: String = input
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    if !is_hex(&compact) {
        return Err(WitnessError::Other(anyhow::anyhow!(
            "hex input must contain complete byte pairs"
        )));
    }
    let bytes = compact
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|value| u8::from_str_radix(value, 16).ok())
                .ok_or_else(|| WitnessError::Other(anyhow::anyhow!("invalid hex byte")))
        })
        .collect::<Result<Vec<_>>>()?;
    String::from_utf8(bytes).map_err(|error| WitnessError::Other(error.into()))
}

fn decode_unicode(input: &str) -> Result<String> {
    let mut units = Vec::new();
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if index + 6 <= bytes.len() && &bytes[index..index + 2] == b"\\u" {
            let value = std::str::from_utf8(&bytes[index + 2..index + 6])
                .ok()
                .and_then(|value| u16::from_str_radix(value, 16).ok())
                .ok_or_else(|| WitnessError::Other(anyhow::anyhow!("invalid Unicode escape")))?;
            units.push(value);
            index += 6;
        } else {
            let remaining = std::str::from_utf8(&bytes[index..])
                .map_err(|error| WitnessError::Other(error.into()))?;
            let character = remaining.chars().next().expect("non-empty remainder");
            let mut encoded = [0_u16; 2];
            units.extend_from_slice(character.encode_utf16(&mut encoded));
            index += character.len_utf8();
        }
    }
    String::from_utf16(&units).map_err(|error| WitnessError::Other(error.into()))
}

fn decode_jwt(input: &str) -> Result<String> {
    let parts: Vec<_> = input.trim().split('.').collect();
    if parts.len() != 3 {
        return Err(WitnessError::Other(anyhow::anyhow!(
            "JWT must have three segments"
        )));
    }
    let decode_json = |part: &str| -> Result<serde_json::Value> {
        let bytes = general_purpose::URL_SAFE_NO_PAD
            .decode(part)
            .map_err(|error| WitnessError::Other(error.into()))?;
        serde_json::from_slice(&bytes).map_err(|error| WitnessError::Other(error.into()))
    };
    serde_json::to_string_pretty(&serde_json::json!({
        "header": decode_json(parts[0])?,
        "payload": decode_json(parts[1])?,
        "signature": parts[2],
        "verified": false,
    }))
    .map_err(|error| WitnessError::Other(error.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reversible_encoding_round_trips() {
        for (encode, decode) in [
            ("urlEncode", "urlDecode"),
            ("base64Encode", "base64Decode"),
            ("hexEncode", "hexDecode"),
            ("htmlEncode", "htmlDecode"),
            ("unicodeEncode", "unicodeDecode"),
        ] {
            let encoded = transform("Witness & café", encode, true).unwrap().output;
            assert_eq!(
                transform(&encoded, decode, true).unwrap().output,
                "Witness & café"
            );
        }
    }

    #[test]
    fn jwt_is_decoded_without_verification() {
        let jwt = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJ3aXRuZXNzIn0.";
        let output = transform(jwt, "jwtDecode", true).unwrap().output;
        assert!(output.contains("witness"));
        assert!(output.contains("\"verified\": false"));
    }

    #[test]
    fn smart_decode_stops_when_plain() {
        let output = transform("SGVsbG8lMjBXb3JsZA==", "smartDecode", true).unwrap();
        assert_eq!(output.output, "Hello World");
        assert_eq!(output.steps, ["Base64", "URL"]);
    }

    #[test]
    fn web_data_helpers_round_trip() {
        assert_eq!(
            transform("hello world", "base64UrlEncode", true)
                .unwrap()
                .output,
            "aGVsbG8gd29ybGQ"
        );
        assert_eq!(
            transform("aGVsbG8gd29ybGQ", "base64UrlDecode", true)
                .unwrap()
                .output,
            "hello world"
        );
        assert_eq!(
            transform("a b&c", "formEncode", true).unwrap().output,
            "a+b%26c"
        );
        assert_eq!(
            transform("a+b%26c", "formDecode", true).unwrap().output,
            "a b&c"
        );
        assert_eq!(
            transform("{\"b\":2,\"a\":1}", "jsonMinify", true)
                .unwrap()
                .output,
            "{\"a\":1,\"b\":2}"
        );
        assert_eq!(
            transform("a=1&a=2&q=hello+world", "queryToJson", true)
                .unwrap()
                .output,
            "{\n  \"a\": [\n    \"1\",\n    \"2\"\n  ],\n  \"q\": \"hello world\"\n}"
        );
        assert_eq!(
            transform(
                "{\"a\":[\"1\",\"2\"],\"q\":\"hello world\"}",
                "jsonToQuery",
                true
            )
            .unwrap()
            .output,
            "a=1&a=2&q=hello+world"
        );
    }

    #[test]
    fn oversized_input_is_rejected() {
        let big = "a".repeat(MAX_DECODER_INPUT + 1);
        assert!(transform(&big, "base64Encode", true).is_err());
    }

    #[test]
    fn padding_flag_only_affects_base64_encode() {
        assert_eq!(
            transform("hi", "base64Encode", true).unwrap().output,
            "aGk="
        );
        assert_eq!(
            transform("hi", "base64Encode", false).unwrap().output,
            "aGk"
        );
        // Other ops ignore the flag.
        assert_eq!(
            transform("a+b", "formDecode", true).unwrap().output,
            transform("a+b", "formDecode", false).unwrap().output,
        );
    }
}
