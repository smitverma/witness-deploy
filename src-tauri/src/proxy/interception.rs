use std::time::Instant;
use std::{collections::HashMap, sync::Arc};

use tokio::sync::{oneshot, Mutex};
use uuid::Uuid;

use crate::{
    error::Result,
    event_bus::{Event, EventBus, InterceptionEvent},
    logging,
};

#[derive(Debug, Clone)]
pub enum InterceptionResolution {
    Forward,
    Drop,
    Modify(Vec<u8>),
}

struct InterceptionState {
    enabled: bool,
    pending: HashMap<String, oneshot::Sender<InterceptionResolution>>,
}

#[derive(Clone)]
pub struct InterceptionManager {
    state: Arc<Mutex<InterceptionState>>,
}

impl Default for InterceptionManager {
    fn default() -> Self {
        Self::new(true)
    }
}

impl InterceptionManager {
    pub fn new(enabled: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(InterceptionState {
                enabled,
                pending: HashMap::new(),
            })),
        }
    }

    pub async fn intercept_request(
        &self,
        raw: Vec<u8>,
        url: String,
        bus: &EventBus,
    ) -> Result<InterceptionResolution> {
        self.intercept(raw, None, url, bus, true).await
    }

    pub async fn intercept_response(
        &self,
        raw: Vec<u8>,
        request_raw: Vec<u8>,
        url: String,
        bus: &EventBus,
    ) -> Result<InterceptionResolution> {
        self.intercept(raw, Some(request_raw), url, bus, false)
            .await
    }

    async fn intercept(
        &self,
        raw: Vec<u8>,
        request_raw: Option<Vec<u8>>,
        url: String,
        bus: &EventBus,
        request: bool,
    ) -> Result<InterceptionResolution> {
        let direction = if request { "request" } else { "response" };
        let _operation = logging::OperationGuard::new(format!("proxy.interception.{direction}"));
        let id = Uuid::new_v4().to_string();
        let started = Instant::now();
        let message_bytes = raw.len();
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self.state.lock().await;
            if !state.enabled {
                tracing::debug!(
                    target: "witness_lib::network::interception",
                    phase = "bypassed",
                    direction,
                    interception_id = %id,
                    url = %logging::safe_url(url.as_str()),
                    message_bytes,
                    reason = "disabled",
                    "interception bypassed because interception is disabled"
                );
                return Ok(InterceptionResolution::Forward);
            }
            state.pending.insert(id.clone(), sender);
            tracing::info!(
                target: "witness_lib::network::interception",
                phase = "waiting_for_resolution",
                direction,
                interception_id = %id,
                url = %logging::safe_url(url.as_str()),
                message_bytes,
                pending_count = state.pending.len(),
                "interception paused traffic and is waiting for UI resolution"
            );
        }
        if request {
            bus.publish(Event::Interception(InterceptionEvent::Request {
                id: id.clone(),
                raw,
                url,
            }));
        } else {
            bus.publish(Event::Interception(InterceptionEvent::Response {
                id: id.clone(),
                raw,
                request_raw: request_raw.unwrap_or_default(),
                url,
            }));
        }
        let resolution = match receiver.await {
            Ok(resolution) => resolution,
            // The only normal sender removal is disabling interception, which
            // forwards all pending traffic before dropping its senders.
            Err(_) => InterceptionResolution::Forward,
        };
        self.state.lock().await.pending.remove(&id);
        let action = match &resolution {
            InterceptionResolution::Forward => "forward",
            InterceptionResolution::Drop => "drop",
            InterceptionResolution::Modify(_) => "modify",
        };
        let modified_bytes = match &resolution {
            InterceptionResolution::Modify(raw) => Some(raw.len()),
            _ => None,
        };
        tracing::info!(
            target: "witness_lib::network::interception",
            phase = "resolved",
            direction,
            interception_id = %id,
            action,
            modified_bytes,
            duration_ms = started.elapsed().as_millis() as u64,
            "interception resolved"
        );
        bus.publish(Event::Interception(InterceptionEvent::Resolved {
            id,
            action: action.into(),
        }));
        Ok(resolution)
    }

    pub async fn resolve(&self, id: &str, resolution: InterceptionResolution) -> bool {
        let action = match &resolution {
            InterceptionResolution::Forward => "forward",
            InterceptionResolution::Drop => "drop",
            InterceptionResolution::Modify(_) => "modify",
        };
        let modified_bytes = match &resolution {
            InterceptionResolution::Modify(raw) => Some(raw.len()),
            _ => None,
        };
        let resolved = self
            .state
            .lock()
            .await
            .pending
            .remove(id)
            .is_some_and(|sender| sender.send(resolution).is_ok());
        tracing::debug!(
            target: "witness_lib::network::interception",
            phase = "resolution_submitted",
            interception_id = %id,
            action,
            modified_bytes,
            resolved,
            "interception resolution submitted"
        );
        resolved
    }

    pub async fn set_enabled(&self, enabled: bool) -> usize {
        let pending = {
            let mut state = self.state.lock().await;
            state.enabled = enabled;
            if enabled {
                tracing::info!(
                    target: "witness_lib::network::interception",
                    phase = "enabled_state_changed",
                    enabled,
                    forwarded_pending = 0,
                    "interception enabled state changed"
                );
                return 0;
            }
            std::mem::take(&mut state.pending)
        };
        let forwarded = pending
            .into_values()
            .map(|sender| sender.send(InterceptionResolution::Forward).is_ok())
            .filter(|sent| *sent)
            .count();
        tracing::info!(
            target: "witness_lib::network::interception",
            phase = "enabled_state_changed",
            enabled,
            forwarded_pending = forwarded,
            "interception enabled state changed"
        );
        forwarded
    }

    pub async fn pending_count(&self) -> usize {
        self.state.lock().await.pending.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_interception_can_be_modified() {
        let manager = InterceptionManager::default();
        let resolver = manager.clone();
        let bus = EventBus::new(8);
        let mut events = bus.subscribe(None);
        let mut task = tokio::spawn(async move {
            manager
                .intercept_request(b"request".to_vec(), "https://example.com/".into(), &bus)
                .await
                .unwrap()
        });
        let id = match events.recv().await.unwrap() {
            Event::Interception(InterceptionEvent::Request { id, .. }) => id,
            _ => panic!("unexpected event"),
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut task)
                .await
                .is_err()
        );
        assert!(
            resolver
                .resolve(&id, InterceptionResolution::Modify(b"changed".to_vec()))
                .await
        );
        assert!(
            matches!(task.await.unwrap(), InterceptionResolution::Modify(raw) if raw == b"changed")
        );
    }

    #[tokio::test]
    async fn disabling_interception_forwards_every_pending_message() {
        let manager = InterceptionManager::default();
        let bus = EventBus::new(8);
        let mut events = bus.subscribe(None);
        let first_manager = manager.clone();
        let first_bus = bus.clone();
        let first = tokio::spawn(async move {
            first_manager
                .intercept_request(
                    b"request one".to_vec(),
                    "https://example.com/one".into(),
                    &first_bus,
                )
                .await
                .unwrap()
        });
        let second_manager = manager.clone();
        let second_bus = bus.clone();
        let second = tokio::spawn(async move {
            second_manager
                .intercept_request(
                    b"request two".to_vec(),
                    "https://example.com/two".into(),
                    &second_bus,
                )
                .await
                .unwrap()
        });
        events.recv().await.unwrap();
        events.recv().await.unwrap();

        assert_eq!(manager.set_enabled(false).await, 2);
        assert!(matches!(
            first.await.unwrap(),
            InterceptionResolution::Forward
        ));
        assert!(matches!(
            second.await.unwrap(),
            InterceptionResolution::Forward
        ));
        assert_eq!(manager.pending_count().await, 0);
        assert!(matches!(
            manager
                .intercept_request(
                    b"request three".to_vec(),
                    "https://example.com/three".into(),
                    &bus,
                )
                .await
                .unwrap(),
            InterceptionResolution::Forward
        ));
    }
}
