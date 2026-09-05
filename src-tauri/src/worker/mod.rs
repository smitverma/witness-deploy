use std::{path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    database::{Database, HistoryFilter, IdentityInjectionDescriptor},
    error::{Result, WitnessError},
    event_bus::{Event, EventBus, RepeaterEvent, WorkerEvent},
    history::HistoryEntry,
    logging,
    repeater::{Repeater, RepeaterRequestStats, RepeaterResponse},
    state::TrafficStats,
};

#[derive(Debug)]
pub enum Task {
    SaveResponse {
        path: PathBuf,
        body: Vec<u8>,
    },
    SendRepeater {
        request_id: String,
        raw: Vec<u8>,
        tls: bool,
        injection: Option<IdentityInjectionDescriptor>,
        upstream_proxy: Option<crate::state::UpstreamProxyConfig>,
        compression_mode: String,
        timeout: std::time::Duration,
    },
    QueryHistory {
        project_path: PathBuf,
        filter: HistoryFilter,
        offset: usize,
        limit: usize,
    },
    SaveProject {
        path: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
pub enum TaskResult {
    Saved,
    RepeaterResponse(RepeaterResponse),
    HistoryEntries(Vec<HistoryEntry>),
}

pub(crate) struct TaskEnvelope {
    id: String,
    task: Task,
    cancellation: CancellationToken,
    response: oneshot::Sender<Result<TaskResult>>,
}

#[derive(Clone)]
pub struct TaskQueue {
    sender: mpsc::Sender<TaskEnvelope>,
}

impl TaskQueue {
    pub(crate) fn new(capacity: usize) -> (Self, mpsc::Receiver<TaskEnvelope>) {
        let (sender, receiver) = mpsc::channel(capacity);
        (Self { sender }, receiver)
    }

    pub async fn enqueue(
        &self,
        task: Task,
        cancellation: CancellationToken,
    ) -> Result<oneshot::Receiver<Result<TaskResult>>> {
        let (response, receiver) = oneshot::channel();
        let id = Uuid::new_v4().to_string();
        tracing::debug!(
            target: "witness_lib::worker",
            phase = "task_enqueued",
            task_id = %id,
            task = %task_name(&task),
            "worker task enqueued"
        );
        self.sender
            .send(TaskEnvelope {
                id,
                task,
                cancellation,
                response,
            })
            .await
            .map_err(|_| WitnessError::WorkerClosed)?;
        Ok(receiver)
    }
}

pub struct WorkerPool;

impl WorkerPool {
    pub(crate) fn spawn(
        worker_count: usize,
        mut receiver: mpsc::Receiver<TaskEnvelope>,
        event_bus: EventBus,
        traffic_stats: TrafficStats,
    ) {
        // Single dispatcher owns the `Receiver` (mpsc receivers cannot be
        // cloned). Previously workers shared `Arc<Mutex<Receiver>>` and held
        // the lock across `recv().await`, serializing dequeue and risking
        // hangs. Now one task drains the queue and spawns executions bounded
        // by a semaphore, preserving `worker_count` parallelism without
        // holding any lock across an await.
        let permits = Arc::new(tokio::sync::Semaphore::new(worker_count.max(1)));
        tauri::async_runtime::spawn(async move {
            while let Some(envelope) = receiver.recv().await {
                let Ok(permit) = permits.clone().acquire_owned().await else {
                    break;
                };
                let event_bus = event_bus.clone();
                let traffic_stats = traffic_stats.clone();
                tauri::async_runtime::spawn(async move {
                    let _permit = permit;
                    let result = execute(&envelope, &event_bus, &traffic_stats).await;
                    match &result {
                        Ok(_) => tracing::debug!(
                            target: "witness_lib::worker",
                            phase = "task_result_ready",
                            task_id = %envelope.id,
                            task = %task_name(&envelope.task),
                            "worker task completed"
                        ),
                        Err(error) => tracing::warn!(
                            target: "witness_lib::worker",
                            phase = "task_failed",
                            task_id = %envelope.id,
                            task = %task_name(&envelope.task),
                            error = %error,
                            "worker task failed"
                        ),
                    }
                    let _ = envelope.response.send(result);
                });
            }
        });
    }
}

async fn execute(
    envelope: &TaskEnvelope,
    bus: &EventBus,
    traffic_stats: &TrafficStats,
) -> Result<TaskResult> {
    let _operation = logging::OperationGuard::new(format!("worker.{}", task_name(&envelope.task)));
    tracing::debug!(
        target: "witness_lib::worker",
        phase = "task_started",
        task_id = %envelope.id,
        task = %task_name(&envelope.task),
        "worker task started"
    );
    check_cancelled(&envelope.cancellation)?;
    publish_progress(bus, &envelope.id, 0, "started");

    let result = match &envelope.task {
        Task::SaveResponse { path, body } => {
            tracing::debug!(
                target: "witness_lib::worker",
                phase = "response_body_write_started",
                task_id = %envelope.id,
                bytes = body.len(),
                "response body file write started"
            );
            tokio::fs::write(path, body).await?;
            tracing::debug!(
                target: "witness_lib::worker",
                phase = "response_body_write_completed",
                task_id = %envelope.id,
                bytes = body.len(),
                "response body file write completed"
            );
            TaskResult::Saved
        }
        Task::SendRepeater {
            request_id,
            raw,
            tls,
            injection,
            upstream_proxy,
            compression_mode,
            timeout,
        } => {
            tracing::info!(
                target: "witness_lib::worker",
                phase = "repeater_task_started",
                task_id = %envelope.id,
                request_id = %request_id,
                input_bytes = raw.len(),
                tls,
                "repeater worker task started"
            );
            bus.publish(Event::Repeater(RepeaterEvent {
                request_id: request_id.clone(),
                status: "sending".into(),
                raw: None,
                tls: None,
            }));
            match Repeater
                .send_request_with_stats(
                    // Single intentional clone: `raw` is borrowed from
                    // `&Task`, but the repeater takes ownership for async send.
                    raw.clone(),
                    *tls,
                    injection.clone(),
                    upstream_proxy.clone(),
                    compression_mode,
                    envelope.cancellation.clone(),
                    RepeaterRequestStats {
                        traffic_stats: Some(traffic_stats),
                        timeout_duration: *timeout,
                    },
                )
                .await
            {
                Ok(response) => {
                    tracing::info!(
                        target: "witness_lib::worker",
                        phase = "repeater_task_completed",
                        task_id = %envelope.id,
                        request_id = %request_id,
                        status = response.status,
                        response_bytes = response.size,
                        duration_ms = response.duration_ms,
                        "repeater worker task completed"
                    );
                    bus.publish(Event::Repeater(RepeaterEvent {
                        request_id: request_id.clone(),
                        status: "complete".into(),
                        raw: None,
                        tls: None,
                    }));
                    TaskResult::RepeaterResponse(response)
                }
                Err(error) => {
                    tracing::warn!(
                        target: "witness_lib::worker",
                        phase = "repeater_task_failed",
                        task_id = %envelope.id,
                        request_id = %request_id,
                        error = %error,
                        "repeater worker task failed"
                    );
                    bus.publish(Event::Repeater(RepeaterEvent {
                        request_id: request_id.clone(),
                        status: if error.client_cancelled() {
                            "cancelled"
                        } else {
                            "failed"
                        }
                        .into(),
                        raw: None,
                        tls: None,
                    }));
                    return Err(error);
                }
            }
        }
        Task::QueryHistory {
            project_path,
            filter,
            offset,
            limit,
        } => {
            tracing::debug!(
                target: "witness_lib::worker",
                phase = "history_query_started",
                task_id = %envelope.id,
                offset,
                limit,
                "history query worker task started"
            );
            check_cancelled(&envelope.cancellation)?;
            let database = Database::open(project_path)?;
            let entries = database.query_history(filter, *offset, *limit)?;
            check_cancelled(&envelope.cancellation)?;
            tracing::debug!(
                target: "witness_lib::worker",
                phase = "history_query_completed",
                task_id = %envelope.id,
                offset,
                limit,
                result_count = entries.len(),
                "history query worker task completed"
            );
            TaskResult::HistoryEntries(entries)
        }
        Task::SaveProject { path } => {
            tracing::debug!(
                target: "witness_lib::worker",
                phase = "project_directory_create_started",
                task_id = %envelope.id,
                "project directory creation worker task started"
            );
            tokio::fs::create_dir_all(path).await?;
            TaskResult::Saved
        }
    };

    publish_progress(bus, &envelope.id, 100, "complete");
    Ok(result)
}

pub fn check_cancelled(token: &CancellationToken) -> Result<()> {
    if token.is_cancelled() {
        Err(WitnessError::Cancelled)
    } else {
        Ok(())
    }
}

fn publish_progress(bus: &EventBus, task_id: &str, progress: u8, message: &str) {
    tracing::debug!(%task_id, progress, %message, "worker task progress");
    bus.publish(Event::Worker(WorkerEvent {
        task_id: task_id.into(),
        progress,
        message: message.into(),
    }));
}

fn task_name(task: &Task) -> &'static str {
    match task {
        Task::SaveResponse { .. } => "save_response",
        Task::SendRepeater { .. } => "send_repeater",
        Task::QueryHistory { .. } => "query_history",
        Task::SaveProject { .. } => "save_project",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_is_propagated() {
        let token = CancellationToken::new();
        token.cancel();
        assert!(matches!(
            check_cancelled(&token),
            Err(WitnessError::Cancelled)
        ));
    }
}
