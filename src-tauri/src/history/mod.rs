use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::{
    // Canonical `split_http_message` lives in `database` (re-exported for
    // compat). It logically belongs in `http`, but `http` is owned by another
    // workstream, so history keeps the database import to avoid cross-branch
    // conflicts.
    database::split_http_message,
    database::{BodyKind, RequestMeta, ResponseMeta},
    error::{Result, WitnessError},
    event_bus::{Event, ForwardEvent, HistoryEvent},
    http::{parse_request, parse_response},
    logging,
    state::AppState,
    worker::{check_cancelled, Task},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub sequence: u64,
    pub id: String,
    pub url: String,
    pub method: String,
    pub host: String,
    pub path: String,
    pub status: u16,
    pub length: usize,
    pub mime_type: String,
    pub duration_ms: u64,
    pub timestamp: String,
    pub scoped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_snippet: Option<String>,
}

#[derive(Clone)]
pub struct History {
    entries: Arc<RwLock<VecDeque<HistoryEntry>>>,
    capacity: Arc<AtomicUsize>,
}

impl History {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(VecDeque::with_capacity(capacity.min(10_000)))),
            capacity: Arc::new(AtomicUsize::new(capacity)),
        }
    }

    pub async fn push(&self, entry: HistoryEntry) {
        let mut entries = self.entries.write().await;
        entries.push_front(entry);
        entries.truncate(self.capacity.load(Ordering::Relaxed));
    }

    pub async fn set_capacity(&self, capacity: usize) {
        self.capacity.store(capacity, Ordering::Relaxed);
        self.entries.write().await.truncate(capacity);
    }

    pub async fn load_entries(
        &self,
        offset: usize,
        limit: usize,
        cancellation: &CancellationToken,
    ) -> Result<Vec<HistoryEntry>> {
        check_cancelled(cancellation)?;
        let entries = self.entries.read().await;
        let mut page = Vec::with_capacity(limit);
        for entry in entries.iter().skip(offset).take(limit) {
            check_cancelled(cancellation)?;
            page.push(entry.clone());
        }
        Ok(page)
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new(10_000)
    }
}

pub fn start_history_recorder(state: AppState) {
    if state.history_recorder_started.swap(true, Ordering::SeqCst) {
        tracing::debug!(
            target: "witness_lib::history",
            phase = "recorder_already_started",
            "history recorder start request ignored"
        );
        return;
    }
    tracing::info!(
        target: "witness_lib::history",
        phase = "recorder_started",
        "history recorder started"
    );
    let Some(mut receiver) = state.event_bus.take_forward_receiver() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        while let Some(forward) = receiver.recv().await {
            if let Err(error) = record_forward_event(&state, forward).await {
                tracing::error!(%error, "failed to record proxy history");
            }
        }
    });
}

async fn record_forward_event(state: &AppState, forward: ForwardEvent) -> Result<()> {
    let _operation = logging::OperationGuard::new("history.record_forward_event");
    // NOTE: `project_save_lock` is intentionally NOT held across worker
    // enqueues/awaits below. It is only acquired for the short DB
    // insert+prune section at the end, so slow body writes never block saves.
    if forward.project_generation != state.project_generation.load(Ordering::Acquire) {
        tracing::debug!(
            target: "witness_lib::history",
            request_id = %forward.id,
            event_generation = forward.project_generation,
            current_generation = state.project_generation.load(Ordering::Acquire),
            "discarding a forward event from a previous project generation"
        );
        return Ok(());
    }
    let request_id = forward.id.clone();
    tracing::info!(
        target: "witness_lib::history",
        phase = "forward_event_received",
        request_id = %request_id,
        method = %forward.method,
        url = %logging::safe_url(forward.url.as_str()),
        host = %forward.host,
        status = forward.status,
        request_bytes = forward.request.len(),
        response_bytes = forward.response.len(),
        duration_ms = forward.duration_ms,
        "forward event received for history persistence"
    );
    let response_id = uuid::Uuid::new_v4().to_string();
    let (request_headers, request_body) = split_http_message(&forward.request);
    let (response_headers, response_body) = split_http_message(&forward.response);
    let (request_path, response_path) = {
        let database = state.database.lock().await;
        let Some(database) = database.as_ref() else {
            return Ok(());
        };
        (
            database.body_store().path(BodyKind::Request, &request_id),
            database.body_store().path(BodyKind::Response, &response_id),
        )
    };

    let request_write = state
        .task_queue
        .enqueue(
            Task::SaveResponse {
                path: request_path.clone(),
                body: request_body.to_vec(),
            },
            CancellationToken::new(),
        )
        .await?;
    let response_write = state
        .task_queue
        .enqueue(
            Task::SaveResponse {
                path: response_path.clone(),
                body: response_body.to_vec(),
            },
            CancellationToken::new(),
        )
        .await?;
    // Sequential enqueues without holding `project_save_lock` (worker module is
    // owned elsewhere, so no combined `SaveBodies` variant is added here).
    request_write
        .await
        .map_err(|_| WitnessError::Other(anyhow::anyhow!("request body worker stopped")))??;
    response_write
        .await
        .map_err(|_| WitnessError::Other(anyhow::anyhow!("response body worker stopped")))??;
    tracing::debug!(
        target: "witness_lib::history",
        phase = "body_files_persisted",
        request_id = %request_id,
        request_body_bytes = request_body.len(),
        response_body_bytes = response_body.len(),
        "history request and response bodies persisted"
    );

    let parsed_request = parse_request(&forward.request)?
        .ok_or_else(|| WitnessError::InvalidHttp("captured request is incomplete".into()))?;
    let parsed_response = parse_response(&forward.response)?
        .ok_or_else(|| WitnessError::InvalidHttp("captured response is incomplete".into()))?;
    let request = parsed_request.0;
    let response = parsed_response.0;
    let path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/")
        .to_string();
    let mime_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .split(';')
        .next()
        .unwrap_or("application/octet-stream")
        .to_string();
    let timestamp = chrono::Utc::now().to_rfc3339();
    let scoped = state.scope.is_in_scope(&forward.host).await;
    let entry = HistoryEntry {
        sequence: 0,
        id: request_id.clone(),
        url: forward.url.clone(),
        method: forward.method.clone(),
        host: forward.host.clone(),
        path: path.clone(),
        status: forward.status,
        length: response_body.len(),
        mime_type: mime_type.clone(),
        duration_ms: forward.duration_ms,
        timestamp: timestamp.clone(),
        scoped,
        match_snippet: None,
    };
    let request_meta = RequestMeta {
        id: request_id.clone(),
        url: forward.url,
        method: forward.method,
        host: forward.host,
        path,
        ip: forward.ip,
        timestamp,
        headers: request_headers.to_vec(),
        body_path: request_path,
        scoped,
    };
    let response_meta = ResponseMeta {
        id: response_id,
        request_id: request_id.clone(),
        status: forward.status,
        mime_type,
        duration_ms: forward.duration_ms,
        size: response_body.len(),
        headers: response_headers.to_vec(),
        body_path: response_path,
    };
    {
        // Narrow critical section: only DB insert+prune holds the save lock.
        let _save_guard = state.project_save_lock.lock().await;
        let mut database = state.database.lock().await;
        let database = database
            .as_mut()
            .ok_or_else(|| WitnessError::Project("no project is open".into()))?;
        database.insert_exchange(&request_meta, &response_meta)?;
        database.prune_history(state.settings.read().await.history_limit.max(100))?;
    }
    state.project.write().await.dirty = true;
    tracing::info!(
        target: "witness_lib::history",
        phase = "entry_persisted",
        id = %request_id,
        host = %entry.host,
        status = entry.status,
        response_body_bytes = entry.length,
        scoped = entry.scoped,
        "history entry persisted"
    );
    state.history.push(entry).await;
    state
        .event_bus
        .publish(Event::History(HistoryEvent::NewEntry { id: request_id }));
    Ok(())
}

#[derive(Default)]
pub struct SearchEngine;

impl SearchEngine {
    pub async fn search(
        &self,
        query: &str,
        values: &[String],
        cancellation: &CancellationToken,
    ) -> Result<Vec<usize>> {
        let query = query.to_ascii_lowercase();
        let mut matches = Vec::new();
        for (index, value) in values.iter().enumerate() {
            check_cancelled(cancellation)?;
            if value.to_ascii_lowercase().contains(&query) {
                matches.push(index);
            }
            tokio::task::yield_now().await;
        }
        Ok(matches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::{Database, HistoryFilter},
        event_bus::EventCategory,
    };

    #[tokio::test]
    async fn forward_events_are_recorded_with_file_backed_bodies() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::new();
        *state.database.lock().await = Some(Database::open(root.path()).unwrap());
        let mut events = state.event_bus.subscribe(Some(EventCategory::History));
        start_history_recorder(state.clone());
        state
            .event_bus
            .publish_forward(ForwardEvent {
                id: "request-one".into(),
                project_generation: 0,
                method: "GET".into(),
                url: "http://example.test/path".into(),
                host: "example.test".into(),
                ip: Some("127.0.0.1".into()),
                request:
                    b"GET /path HTTP/1.1\r\nHost: example.test\r\nContent-Length: 4\r\n\r\nbody"
                        .to_vec(),
                response:
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok"
                        .to_vec(),
                status: 200,
                duration_ms: 8,
            })
            .await;
        tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap();
        let database = state.database.lock().await;
        let entries = database
            .as_ref()
            .unwrap()
            .query_history(&HistoryFilter::default(), 0, 10)
            .unwrap();
        assert_eq!(entries.len(), 1);
        let detail = database
            .as_ref()
            .unwrap()
            .get_detail("request-one")
            .unwrap()
            .unwrap();
        assert!(detail.request.ends_with(b"body"));
        assert!(detail.response.ends_with(b"ok"));
    }
}
