use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use http::{header, HeaderName, HeaderValue, Request, Response};
use regex::Regex;

use crate::state::MatchReplaceRule;

/// Maximum regex pattern length enforced for match/replace and interception
/// rules. Longer patterns are rejected (no-match + log) to bound compile cost.
pub const MAX_REGEX_LEN: usize = 512;

/// Cache of compiled regexes keyed by pattern string. Avoids recompiling the
/// same pattern on every request/response (hot path).
static REGEX_CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();

fn regex_cache() -> &'static Mutex<HashMap<String, Regex>> {
    REGEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_regex(pattern: &str) -> Option<Regex> {
    if pattern.len() > MAX_REGEX_LEN {
        tracing::warn!(
            target: "witness_lib::network::match_replace",
            pattern_len = pattern.len(),
            max_len = MAX_REGEX_LEN,
            "match/replace regex too long; skipping rule"
        );
        return None;
    }
    if let Ok(cache) = regex_cache().lock() {
        if let Some(regex) = cache.get(pattern) {
            return Some(regex.clone());
        }
    }
    let compiled = Regex::new(pattern).ok()?;
    if let Ok(mut cache) = regex_cache().lock() {
        // Bound cache size to avoid unbounded growth from unique patterns.
        if cache.len() < 512 {
            cache.insert(pattern.to_owned(), compiled.clone());
        }
    }
    Some(compiled)
}

/// Apply match/replace rules to a request based on granular `type`.
/// Supported types: requestHost, requestHeader, requestBody, requestParamName, requestParamValue
pub fn apply_to_request(request: &mut Request<Vec<u8>>, rules: &[MatchReplaceRule]) {
    for rule in rules.iter().filter(|r| r.enabled) {
        let t = rule.effective_type();
        if !t.starts_with("request") {
            continue;
        }
        if rule.match_str.is_empty() {
            continue;
        }
        match t {
            "requestHost" => apply_request_host(request, rule),
            "requestHeader" => apply_request_header(request, rule),
            "requestBody" => apply_request_body(request, rule),
            "requestParamName" => apply_request_param_name(request, rule),
            "requestParamValue" => apply_request_param_value(request, rule),
            _ => {
                // Legacy fallback: if rule had only location= request without type, we already mapped to requestBody via effective_type,
                // but handle it as body.
                if t == "requestBody" {
                    apply_request_body(request, rule)
                }
            }
        }
    }
}

/// Apply match/replace rules to a response based on granular `type`.
/// Supported types: responseHeader, responseBody, responseParamName, responseParamValue
pub fn apply_to_response(response: &mut Response<Vec<u8>>, rules: &[MatchReplaceRule]) {
    for rule in rules.iter().filter(|r| r.enabled) {
        let t = rule.effective_type();
        if !t.starts_with("response") {
            continue;
        }
        if rule.match_str.is_empty() {
            continue;
        }
        match t {
            "responseHeader" => apply_response_header(response, rule),
            "responseBody" => apply_response_body(response, rule),
            "responseParamName" => apply_response_param_name(response, rule),
            "responseParamValue" => apply_response_param_value(response, rule),
            _ => {
                if t == "responseBody" {
                    apply_response_body(response, rule)
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn apply_match(haystack: &str, rule: &MatchReplaceRule) -> String {
    if rule.is_regex {
        match cached_regex(&rule.match_str) {
            Some(re) => re.replace_all(haystack, rule.replace.as_str()).into_owned(),
            None => haystack.to_owned(),
        }
    } else {
        haystack.replace(&rule.match_str, &rule.replace)
    }
}

fn is_chunked(headers: &http::HeaderMap) -> bool {
    headers
        .get(header::TRANSFER_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .any(|enc| enc.trim().eq_ignore_ascii_case("chunked"))
        })
}

// Request host: replace in URI authority host and Host header
fn apply_request_host(request: &mut Request<Vec<u8>>, rule: &MatchReplaceRule) {
    // Determine current host (without port) from URI authority or Host header
    let uri_host = request.uri().authority().map(|a| a.host().to_string());
    let header_host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(':').next().unwrap_or(s).to_string());
    let current_host = uri_host.clone().or(header_host.clone());
    let Some(current_host) = current_host else {
        return;
    };
    let new_host = apply_match(&current_host, rule);
    if new_host == current_host {
        return;
    }
    // Update URI authority if present
    if let Some(auth) = request.uri().authority().cloned() {
        let auth_str = auth.as_str().to_string();
        // Replace host part inside authority
        let new_auth_str = auth_str.replacen(&current_host, &new_host, 1);
        let uri_str = request.uri().to_string();
        let new_uri_str = uri_str.replacen(&auth_str, &new_auth_str, 1);
        if let Ok(new_uri) = new_uri_str.parse() {
            *request.uri_mut() = new_uri;
        }
    }
    // Update Host header
    if let Some(host_val) = request.headers().get(header::HOST).cloned() {
        if let Ok(val_str) = host_val.to_str() {
            let new_val = val_str.replace(&current_host, &new_host);
            // Also handle generic replacement (in case host appears elsewhere)
            let new_val2 = apply_match(val_str, rule);
            let final_val = if new_val != val_str {
                new_val
            } else {
                new_val2
            };
            if final_val != val_str {
                if let Ok(new_hv) = HeaderValue::from_str(&final_val) {
                    request.headers_mut().insert(header::HOST, new_hv);
                }
            }
        }
    }
}

// Request header: replace in both name and value (key/value)
fn apply_request_header(request: &mut Request<Vec<u8>>, rule: &MatchReplaceRule) {
    let headers = request.headers().clone();
    let mut new_headers = http::HeaderMap::new();
    let mut changed = false;
    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        let new_name_str = apply_match(name_str, rule);
        let val_str = value.to_str().unwrap_or_default();
        let new_val_str = apply_match(val_str, rule);
        let name_changed = new_name_str != name_str;
        let val_changed = new_val_str != val_str;
        if name_changed || val_changed {
            changed = true;
        }
        let final_name = if name_changed {
            HeaderName::from_bytes(new_name_str.as_bytes()).unwrap_or_else(|_| name.clone())
        } else {
            name.clone()
        };
        let final_val = if val_changed {
            HeaderValue::from_str(&new_val_str).unwrap_or_else(|_| value.clone())
        } else {
            value.clone()
        };
        new_headers.append(final_name, final_val);
    }
    if changed {
        *request.headers_mut() = new_headers;
    }
}

// Request body: literal/regex on full body string
fn apply_request_body(request: &mut Request<Vec<u8>>, rule: &MatchReplaceRule) {
    let body_str = String::from_utf8_lossy(request.body()).into_owned();
    let new_body_str = apply_match(&body_str, rule);
    if new_body_str != body_str {
        *request.body_mut() = new_body_str.into_bytes();
        if !is_chunked(request.headers()) {
            let len = request.body().len();
            if let Ok(val) = HeaderValue::from_str(&len.to_string()) {
                request.headers_mut().insert(header::CONTENT_LENGTH, val);
            }
        }
    }
}

// Request param name: replace in query param names (URL) and form body param names
fn apply_request_param_name(request: &mut Request<Vec<u8>>, rule: &MatchReplaceRule) {
    // URL query
    if let Some(query) = request.uri().query().map(|s| s.to_string()) {
        let mut pairs = parse_query(&query);
        let mut changed = false;
        for (name, _) in pairs.iter_mut() {
            let new_name = apply_match(name, rule);
            if new_name != *name {
                *name = new_name;
                changed = true;
            }
        }
        if changed {
            let new_query = build_query(&pairs);
            rebuild_request_query(request, &new_query);
        }
    }
    // Body form params if urlencoded
    if is_form_urlencoded(request.headers()) {
        let body_str = String::from_utf8_lossy(request.body()).into_owned();
        if body_str.contains('=') {
            let mut pairs = parse_query(&body_str);
            let mut changed = false;
            for (name, _) in pairs.iter_mut() {
                let new_name = apply_match(name, rule);
                if new_name != *name {
                    *name = new_name;
                    changed = true;
                }
            }
            if changed {
                let new_body = build_query(&pairs);
                *request.body_mut() = new_body.into_bytes();
                if !is_chunked(request.headers()) {
                    let len = request.body().len();
                    if let Ok(val) = HeaderValue::from_str(&len.to_string()) {
                        request.headers_mut().insert(header::CONTENT_LENGTH, val);
                    }
                }
            }
        }
    }
}

fn apply_request_param_value(request: &mut Request<Vec<u8>>, rule: &MatchReplaceRule) {
    // URL query values
    if let Some(query) = request.uri().query().map(|s| s.to_string()) {
        let mut pairs = parse_query(&query);
        let mut changed = false;
        for (_, value) in pairs.iter_mut() {
            let new_val = apply_match(value, rule);
            if new_val != *value {
                *value = new_val;
                changed = true;
            }
        }
        if changed {
            let new_query = build_query(&pairs);
            rebuild_request_query(request, &new_query);
        }
    }
    // Body form
    if is_form_urlencoded(request.headers()) {
        let body_str = String::from_utf8_lossy(request.body()).into_owned();
        if body_str.contains('=') {
            let mut pairs = parse_query(&body_str);
            let mut changed = false;
            for (_, value) in pairs.iter_mut() {
                let new_val = apply_match(value, rule);
                if new_val != *value {
                    *value = new_val;
                    changed = true;
                }
            }
            if changed {
                let new_body = build_query(&pairs);
                *request.body_mut() = new_body.into_bytes();
                if !is_chunked(request.headers()) {
                    let len = request.body().len();
                    if let Ok(val) = HeaderValue::from_str(&len.to_string()) {
                        request.headers_mut().insert(header::CONTENT_LENGTH, val);
                    }
                }
            }
        }
    }
}

// Response header: both key/value
fn apply_response_header(response: &mut Response<Vec<u8>>, rule: &MatchReplaceRule) {
    let headers = response.headers().clone();
    let mut new_headers = http::HeaderMap::new();
    let mut changed = false;
    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        let new_name_str = apply_match(name_str, rule);
        let val_str = value.to_str().unwrap_or_default();
        let new_val_str = apply_match(val_str, rule);
        let name_changed = new_name_str != name_str;
        let val_changed = new_val_str != val_str;
        if name_changed || val_changed {
            changed = true;
        }
        let final_name = if name_changed {
            HeaderName::from_bytes(new_name_str.as_bytes()).unwrap_or_else(|_| name.clone())
        } else {
            name.clone()
        };
        let final_val = if val_changed {
            HeaderValue::from_str(&new_val_str).unwrap_or_else(|_| value.clone())
        } else {
            value.clone()
        };
        new_headers.append(final_name, final_val);
    }
    if changed {
        *response.headers_mut() = new_headers;
    }
}

fn apply_response_body(response: &mut Response<Vec<u8>>, rule: &MatchReplaceRule) {
    let body_str = String::from_utf8_lossy(response.body()).into_owned();
    let new_body_str = apply_match(&body_str, rule);
    if new_body_str != body_str {
        *response.body_mut() = new_body_str.into_bytes();
        if !is_chunked(response.headers()) {
            let len = response.body().len();
            if let Ok(val) = HeaderValue::from_str(&len.to_string()) {
                response.headers_mut().insert(header::CONTENT_LENGTH, val);
            }
        }
    }
}

fn apply_response_param_name(response: &mut Response<Vec<u8>>, rule: &MatchReplaceRule) {
    // Treat body as query string if it looks like form data or contains =
    let body_str = String::from_utf8_lossy(response.body()).into_owned();
    if !body_str.contains('=') {
        return;
    }
    let mut pairs = parse_query(&body_str);
    if pairs.is_empty() {
        return;
    }
    let mut changed = false;
    for (name, _) in pairs.iter_mut() {
        let new_name = apply_match(name, rule);
        if new_name != *name {
            *name = new_name;
            changed = true;
        }
    }
    if changed {
        let new_body = build_query(&pairs);
        *response.body_mut() = new_body.into_bytes();
        if !is_chunked(response.headers()) {
            let len = response.body().len();
            if let Ok(val) = HeaderValue::from_str(&len.to_string()) {
                response.headers_mut().insert(header::CONTENT_LENGTH, val);
            }
        }
    }
}

fn apply_response_param_value(response: &mut Response<Vec<u8>>, rule: &MatchReplaceRule) {
    let body_str = String::from_utf8_lossy(response.body()).into_owned();
    if !body_str.contains('=') {
        return;
    }
    let mut pairs = parse_query(&body_str);
    if pairs.is_empty() {
        return;
    }
    let mut changed = false;
    for (_, value) in pairs.iter_mut() {
        let new_val = apply_match(value, rule);
        if new_val != *value {
            *value = new_val;
            changed = true;
        }
    }
    if changed {
        let new_body = build_query(&pairs);
        *response.body_mut() = new_body.into_bytes();
        if !is_chunked(response.headers()) {
            let len = response.body().len();
            if let Ok(val) = HeaderValue::from_str(&len.to_string()) {
                response.headers_mut().insert(header::CONTENT_LENGTH, val);
            }
        }
    }
}

// Helpers for query parsing/building

fn parse_query(query: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        // Decode '+' and percent
        let dec_name = percent_decode(&name.replace('+', " "));
        let dec_value = percent_decode(&value.replace('+', " "));
        pairs.push((dec_name, dec_value));
    }
    pairs
}

fn build_query(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", encode_param(k), encode_param(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

fn encode_param(s: &str) -> String {
    // Encode query component: percent-encode non-unreserved, space as +
    // For simplicity, use utf8_percent_encode with NON_ALPHANUMERIC but keep unreserved
    // We'll manually encode
    let mut out = String::new();
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else if c == ' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn is_form_urlencoded(headers: &http::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.to_ascii_lowercase()
                .contains("application/x-www-form-urlencoded")
        })
}

fn rebuild_request_query(request: &mut Request<Vec<u8>>, new_query: &str) {
    let uri = request.uri().clone();
    let path = uri.path().to_string();
    let authority = uri.authority().map(|a| a.as_str().to_string());
    let scheme = uri.scheme_str().map(|s| s.to_string());
    let mut new_uri_str = String::new();
    if let Some(s) = scheme {
        new_uri_str.push_str(&s);
        new_uri_str.push_str("://");
    }
    if let Some(auth) = authority {
        new_uri_str.push_str(&auth);
    }
    new_uri_str.push_str(&path);
    if !new_query.is_empty() {
        new_uri_str.push('?');
        new_uri_str.push_str(new_query);
    }
    if let Ok(new_uri) = new_uri_str.parse() {
        *request.uri_mut() = new_uri;
    } else {
        // Fallback: try building with just path?query
        let mut fallback = path;
        if !new_query.is_empty() {
            fallback.push('?');
            fallback.push_str(new_query);
        }
        if let Ok(new_uri) = fallback.parse() {
            *request.uri_mut() = new_uri;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::{Method, Request, Response, StatusCode};

    fn rule(id: &str, target: &str, m: &str, replace: &str, is_regex: bool) -> MatchReplaceRule {
        MatchReplaceRule {
            id: id.into(),
            enabled: true,
            location: if target.starts_with("request") {
                "request".into()
            } else {
                "response".into()
            },
            rule_type: target.into(),
            match_str: m.into(),
            replace: replace.into(),
            is_regex,
        }
    }

    #[test]
    fn request_header_literal_replace_key_and_value() {
        let mut req = Request::builder()
            .method(Method::GET)
            .uri("http://example.test/")
            .header("X-Custom", "foo-value")
            .header("X-Foo", "bar")
            .body(b"hello".to_vec())
            .unwrap();
        let rules = vec![rule("1", "requestHeader", "foo", "bar", false)];
        apply_to_request(&mut req, &rules);
        // Name X-Foo -> X-bar, value foo-value -> bar-value
        assert!(
            req.headers().get("X-bar").is_some()
                || req.headers().get("X-Custom").unwrap() == "bar-value"
        );
        assert_eq!(req.headers().get("X-Custom").unwrap(), "bar-value");
    }

    #[test]
    fn request_body_replace_updates_content_length() {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri("http://example.test/api")
            .header("Content-Type", "text/plain")
            .body(b"hello world".to_vec())
            .unwrap();
        req.headers_mut()
            .insert(header::CONTENT_LENGTH, HeaderValue::from_static("11"));
        let rules = vec![rule("1", "requestBody", "world", "Rust", false)];
        apply_to_request(&mut req, &rules);
        assert_eq!(req.body(), b"hello Rust");
        assert_eq!(req.headers().get(header::CONTENT_LENGTH).unwrap(), "10");
    }

    #[test]
    fn request_host_replace_updates_uri_and_host_header() {
        let mut req = Request::builder()
            .method(Method::GET)
            .uri("http://example.test/oldpath?q=1")
            .header("Host", "example.test")
            .body(Vec::new())
            .unwrap();
        let rules = vec![rule("1", "requestHost", "example", "evil", false)];
        apply_to_request(&mut req, &rules);
        assert!(req.uri().to_string().contains("evil.test"));
        assert_eq!(req.headers().get(header::HOST).unwrap(), "evil.test");
    }

    #[test]
    fn request_param_name_replace() {
        let mut req = Request::builder()
            .method(Method::GET)
            .uri("http://example.test/api?user=alice&token=secret")
            .body(Vec::new())
            .unwrap();
        let rules = vec![rule("1", "requestParamName", "user", "admin", false)];
        apply_to_request(&mut req, &rules);
        assert!(req.uri().query().unwrap().contains("admin=alice"));
        assert!(!req.uri().query().unwrap().contains("user="));
    }

    #[test]
    fn request_param_value_replace() {
        let mut req = Request::builder()
            .method(Method::GET)
            .uri("http://example.test/api?user=alice&token=secret")
            .body(Vec::new())
            .unwrap();
        let rules = vec![rule("1", "requestParamValue", "secret", "public", false)];
        apply_to_request(&mut req, &rules);
        assert!(req.uri().query().unwrap().contains("token=public"));
    }

    #[test]
    fn request_param_value_replace_in_body() {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri("http://example.test/api")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(b"user=alice&token=secret".to_vec())
            .unwrap();
        let rules = vec![rule("1", "requestParamValue", "alice", "bob", false)];
        apply_to_request(&mut req, &rules);
        assert_eq!(req.body(), b"user=bob&token=secret");
    }

    #[test]
    fn regex_replace_in_response_body() {
        let mut resp = Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/plain")
            .body(b"user=123-abc-456".to_vec())
            .unwrap();
        let rules = vec![rule("1", "responseBody", r"\d+", "XXX", true)];
        apply_to_response(&mut resp, &rules);
        assert_eq!(resp.body(), b"user=XXX-abc-XXX");
    }

    #[test]
    fn response_header_replace_key_and_value() {
        let mut resp = Response::builder()
            .status(StatusCode::OK)
            .header("X-Powered-By", "foo-server")
            .body(Vec::new())
            .unwrap();
        let rules = vec![rule("1", "responseHeader", "foo", "bar", false)];
        apply_to_response(&mut resp, &rules);
        assert_eq!(resp.headers().get("X-Powered-By").unwrap(), "bar-server");
    }

    #[test]
    fn response_param_value_replace() {
        let mut resp = Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(b"token=secret&user=alice".to_vec())
            .unwrap();
        let rules = vec![rule("1", "responseParamValue", "secret", "public", false)];
        apply_to_response(&mut resp, &rules);
        assert_eq!(resp.body(), b"token=public&user=alice");
    }

    #[test]
    fn disabled_rule_is_ignored() {
        let mut req = Request::builder()
            .method(Method::GET)
            .uri("http://example.test/")
            .header("X-Test", "foo")
            .body(Vec::new())
            .unwrap();
        let mut r = rule("1", "requestHeader", "foo", "bar", false);
        r.enabled = false;
        apply_to_request(&mut req, &[r]);
        assert_eq!(req.headers().get("X-Test").unwrap(), "foo");
    }

    #[test]
    fn literal_replace_all_occurrences_body() {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri("http://example.test/")
            .body(b"aaa".to_vec())
            .unwrap();
        let rules = vec![rule("1", "requestBody", "a", "b", false)];
        apply_to_request(&mut req, &rules);
        assert_eq!(req.body(), b"bbb");
    }

    #[test]
    fn chunked_request_body_does_not_set_content_length() {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri("http://example.test/")
            .header("Transfer-Encoding", "chunked")
            .body(b"hello".to_vec())
            .unwrap();
        let rules = vec![rule("1", "requestBody", "hello", "hi", false)];
        apply_to_request(&mut req, &rules);
        assert_eq!(req.body(), b"hi");
        assert!(!req.headers().contains_key(header::CONTENT_LENGTH));
    }

    #[test]
    fn wrong_location_rule_not_applied_to_request() {
        let mut req = Request::builder()
            .method(Method::GET)
            .uri("http://example.test/")
            .header("X-Test", "foo")
            .body(b"foo".to_vec())
            .unwrap();
        let rules = vec![rule("1", "responseHeader", "foo", "bar", false)];
        apply_to_request(&mut req, &rules);
        assert_eq!(req.headers().get("X-Test").unwrap(), "foo");
        assert_eq!(req.body(), b"foo");
    }

    #[test]
    fn overlong_regex_is_rejected_without_matching() {
        let long = "a".repeat(MAX_REGEX_LEN + 1);
        assert!(cached_regex(&long).is_none());
        let mut req = Request::builder()
            .method(Method::POST)
            .uri("http://example.test/")
            .body(b"aaa".to_vec())
            .unwrap();
        let rules = vec![rule("1", "requestBody", &long, "b", true)];
        apply_to_request(&mut req, &rules);
        assert_eq!(req.body(), b"aaa");
    }

    #[test]
    fn regex_cache_reuses_compiled_patterns() {
        let first = cached_regex(r"\d+").unwrap();
        let second = cached_regex(r"\d+").unwrap();
        assert_eq!(first.as_str(), second.as_str());
    }
}
