use std::{
    collections::VecDeque,
    fmt,
    sync::{Arc, Mutex},
    time::Instant,
};

use chrono::Utc;
use http::{HeaderMap, Request, Response};
use tracing::{field::Visit, Event as TracingEvent, Subscriber};
use tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};

use crate::event_bus::{Event, EventBus, LogEvent};

pub const DEFAULT_LOG_DISPLAY_LIMIT: usize = 50;
pub const MAX_LOG_DISPLAY_LIMIT: usize = 200;

#[derive(Clone)]
pub struct LogStore {
    entries: Arc<Mutex<VecDeque<LogEvent>>>,
    event_bus: Arc<Mutex<Option<EventBus>>>,
    capacity: usize,
}

impl LogStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            event_bus: Arc::new(Mutex::new(None)),
            capacity,
        }
    }

    pub fn connect(&self, event_bus: EventBus) {
        // Poison-tolerant: recover the inner guard instead of panicking.
        *self
            .event_bus
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(event_bus);
    }

    pub fn record(&self, level: &str, module: &str, message: String) {
        let entry = LogEvent {
            level: level.to_ascii_lowercase(),
            module: module.to_string(),
            message,
            timestamp: Utc::now().to_rfc3339(),
        };
        {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            entries.push_back(entry.clone());
            while entries.len() > self.capacity {
                entries.pop_front();
            }
        }
        if let Some(event_bus) = self
            .event_bus
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            event_bus.publish(Event::Log(entry));
        }
    }

    pub fn entries(&self, limit: usize) -> Vec<LogEvent> {
        // `unwrap_or_else` recovers from poisoning, so this never panics.
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let start = entries.len().saturating_sub(limit.min(self.capacity));
        entries.iter().skip(start).cloned().collect()
    }

    pub fn clear(&self) {
        // Never panic: a poisoned ring buffer is still clearable via recovery.
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new(2_000)
    }
}

/// Records the lifetime of an operation without requiring every early return
/// to be rewritten. The guard is intentionally lightweight so it can be used
/// at command, worker, and network boundaries.
pub struct OperationGuard {
    operation: String,
    started: Instant,
}

impl OperationGuard {
    pub fn new(operation: impl Into<String>) -> Self {
        let operation = operation.into();
        tracing::debug!(
            target: "witness_lib::operation",
            operation = %operation,
            phase = "started",
            "operation started"
        );
        Self {
            operation,
            started: Instant::now(),
        }
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        tracing::debug!(
            target: "witness_lib::operation",
            operation = %self.operation,
            phase = "finished",
            duration_ms = self.started.elapsed().as_millis() as u64,
            "operation finished"
        );
    }
}

/// Removes query and fragment values from URLs before they reach diagnostics.
/// The presence of each component is retained because it is useful when
/// diagnosing routing and redirect behavior.
pub fn safe_url(value: &str) -> String {
    let (without_fragment, has_fragment) = match value.split_once('#') {
        Some((prefix, _)) => (prefix, true),
        None => (value, false),
    };
    let (without_query, has_query) = match without_fragment.split_once('?') {
        Some((prefix, _)) => (prefix, true),
        None => (without_fragment, false),
    };

    let without_credentials = if let Some(scheme_end) = without_query.find("://") {
        let authority_start = scheme_end + 3;
        let authority_end = without_query[authority_start..]
            .find('/')
            .map(|offset| authority_start + offset)
            .unwrap_or(without_query.len());
        let authority = &without_query[authority_start..authority_end];
        let authority = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        format!(
            "{}{}{}",
            &without_query[..authority_start],
            authority,
            &without_query[authority_end..]
        )
    } else {
        without_query.to_string()
    };

    format!(
        "{}{}{}",
        without_credentials,
        if has_query { "?[redacted]" } else { "" },
        if has_fragment { "#[redacted]" } else { "" }
    )
}

/// Returns an authority with user information removed.
pub fn safe_authority(value: &str) -> String {
    value
        .rsplit_once('@')
        .map_or(value, |(_, host)| host)
        .to_string()
}

/// Logs header presence without logging header values.
pub fn header_names(headers: &HeaderMap) -> String {
    let mut names = headers
        .keys()
        .map(|name| name.as_str().to_string())
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.dedup();
    names.join(",")
}

pub fn request_metadata(request: &Request<Vec<u8>>) -> (String, String, usize, usize) {
    let target = safe_url(request.uri().to_string().as_str());
    (
        request.method().to_string(),
        target,
        request.headers().len(),
        request.body().len(),
    )
}

pub fn response_metadata(response: &Response<Vec<u8>>) -> (u16, usize, usize) {
    (
        response.status().as_u16(),
        response.headers().len(),
        response.body().len(),
    )
}

pub struct LogLayer {
    store: LogStore,
}

impl LogLayer {
    pub fn new(store: LogStore) -> Self {
        Self { store }
    }
}

impl<S> Layer<S> for LogLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &TracingEvent<'_>, _context: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        self.store.record(
            metadata.level().as_str(),
            metadata.target(),
            visitor.finish(),
        );
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: Option<String>,
    fields: Vec<String>,
}

impl MessageVisitor {
    fn finish(self) -> String {
        let message = self.message.unwrap_or_else(|| "event".into());
        if self.fields.is_empty() {
            message
        } else {
            format!("{message} ({})", self.fields.join(", "))
        }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        let value = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(truncate(value.trim_matches('"')));
        } else {
            self.fields.push(format!(
                "{}={}",
                field.name(),
                sanitize_field(field.name(), &value)
            ));
        }
    }
}

fn sanitize_field(name: &str, value: &str) -> String {
    if is_sensitive_field_name(name) {
        "[redacted]".into()
    } else {
        truncate(value)
    }
}

/// Exact header/field-name matching for sensitive values.
///
/// Previously this used substring `contains`, which over-redacted benign
/// names such as `monkey` (contains `key`). Matching is now exact against a
/// conservative allowlist plus a `-key`/`-token`/`-secret` suffix rule.
fn is_sensitive_field_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "api-key"
            | "credential"
            | "password"
            | "secret"
            | "token"
            | "raw"
            | "key"
    ) || lower.ends_with("-key")
        || lower.ends_with("-token")
        || lower.ends_with("-secret")
}

fn truncate(value: &str) -> String {
    const MAX_LOG_VALUE: usize = 512;
    if value.len() <= MAX_LOG_VALUE {
        return value.to_string();
    }
    let end = value
        .char_indices()
        .find(|(index, _)| *index >= MAX_LOG_VALUE)
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    format!("{}…[truncated]", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_retains_only_recent_entries() {
        let store = LogStore::new(2);
        store.record("INFO", "test", "one".into());
        store.record("WARN", "test", "two".into());
        store.record("ERROR", "test", "three".into());
        let entries = store.entries(2);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].message, "two");
        assert_eq!(entries[1].level, "error");
    }

    #[test]
    fn safe_url_removes_credentials_queries_and_fragments() {
        assert_eq!(
            safe_url("https://user:password@example.test/path?token=secret#section"),
            "https://example.test/path?[redacted]#[redacted]"
        );
        assert_eq!(
            safe_url("http://example.test/path"),
            "http://example.test/path"
        );
    }

    #[test]
    fn log_value_truncation_preserves_utf8_boundaries() {
        let value = "é".repeat(400);
        let truncated = truncate(&value);
        assert!(truncated.ends_with("…[truncated]"));
        assert!(truncated.is_char_boundary(truncated.len()));
    }

    #[test]
    fn sensitive_field_matching_is_exact_not_substring() {
        // Exact sensitive names are redacted.
        for name in [
            "authorization",
            "proxy-authorization",
            "cookie",
            "set-cookie",
            "x-api-key",
            "api-key",
            "credential",
            "password",
            "secret",
            "token",
            "raw",
            "key",
            "X-Custom-Key",
            "session-token",
            "client-secret",
        ] {
            assert_eq!(
                sanitize_field(name, "value"),
                "[redacted]",
                "expected {name} to be redacted"
            );
        }
        // Substring matches must NOT redact (e.g. "monkey" contains "key").
        for name in [
            "monkey",
            "keyboard",
            "tokenizer",
            "monkey-business",
            "rawhide",
        ] {
            assert_ne!(
                sanitize_field(name, "value"),
                "[redacted]",
                "expected {name} to pass through"
            );
        }
    }

    #[test]
    fn poisoned_log_store_never_panics() {
        let store = LogStore::new(2);
        store.record("INFO", "test", "one".into());
        // Poison the ring-buffer mutex, then verify all accessors recover.
        let poisoned = std::sync::Arc::clone(&store.entries);
        let _ = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = poisoned.lock().unwrap();
                    panic!("intentional poison for test");
                })
                .join()
                .unwrap_err();
        });
        store.record("INFO", "test", "recovered".into());
        let _ = store.entries(10);
        store.clear();
        store.connect(crate::event_bus::EventBus::new(8));
    }
}
