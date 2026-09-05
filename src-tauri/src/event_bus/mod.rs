use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "camelCase")]
pub enum Event {
    Proxy(ProxyEvent),
    History(HistoryEvent),
    Interception(InterceptionEvent),
    Log(LogEvent),
    Project(ProjectEvent),
    Scope(ScopeEvent),
    Forward(ForwardEvent),
    Worker(WorkerEvent),
    Repeater(RepeaterEvent),
}

impl Event {
    pub fn category(&self) -> EventCategory {
        match self {
            Self::Proxy(_) => EventCategory::Proxy,
            Self::History(_) => EventCategory::History,
            Self::Interception(_) => EventCategory::Interception,
            Self::Log(_) => EventCategory::Log,
            Self::Project(_) => EventCategory::Project,
            Self::Scope(_) => EventCategory::Scope,
            Self::Forward(_) => EventCategory::Forward,
            Self::Worker(_) => EventCategory::Worker,
            Self::Repeater(_) => EventCategory::Repeater,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCategory {
    Proxy,
    History,
    Interception,
    Log,
    Project,
    Scope,
    Forward,
    Worker,
    Repeater,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProxyEvent {
    Started { address: String },
    Stopped,
    Error { message: String },
    ConnectionCount { count: usize },
    TlsStatus { status: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HistoryEvent {
    NewEntry { id: String },
    Deleted { id: String },
    Cleared,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InterceptionEvent {
    Request {
        id: String,
        raw: Vec<u8>,
        url: String,
    },
    Response {
        id: String,
        raw: Vec<u8>,
        #[serde(rename = "requestRaw")]
        request_raw: Vec<u8>,
        url: String,
    },
    Resolved {
        id: String,
        action: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEvent {
    pub level: String,
    pub module: String,
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ProjectEvent {
    Created { path: String },
    Opened { path: String },
    Closed,
    Deleted { path: String },
    Saved { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ScopeEvent {
    Changed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardEvent {
    pub id: String,
    pub project_generation: u64,
    pub method: String,
    pub url: String,
    pub host: String,
    pub ip: Option<String>,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
    pub status: u16,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerEvent {
    pub task_id: String,
    pub progress: u8,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepeaterEvent {
    pub request_id: String,
    pub status: String,
    pub raw: Option<Vec<u8>>,
    pub tls: Option<bool>,
}

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Event>,
    forward_sender: mpsc::Sender<ForwardEvent>,
    forward_receiver: Arc<Mutex<Option<mpsc::Receiver<ForwardEvent>>>>,
    forward_active: Arc<AtomicBool>,
}

impl EventBus {
    const FORWARD_QUEUE_CAPACITY: usize = 256;

    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        let (forward_sender, forward_receiver) = mpsc::channel(Self::FORWARD_QUEUE_CAPACITY);
        Self {
            sender,
            forward_sender,
            forward_receiver: Arc::new(Mutex::new(Some(forward_receiver))),
            forward_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn publish(&self, event: Event) {
        let _ = self.sender.send(event);
    }

    pub async fn publish_forward(&self, forward: ForwardEvent) {
        // Take by value and clone only when the mpsc forward path is active:
        // clone for the mpsc queue, move the original into broadcast. When
        // inactive, move directly with no clone.
        if self.forward_active.load(Ordering::Acquire) {
            let _ = self.forward_sender.send(forward.clone()).await;
        }
        let _ = self.sender.send(Event::Forward(forward));
    }

    pub fn take_forward_receiver(&self) -> Option<mpsc::Receiver<ForwardEvent>> {
        // Poison-tolerant: recover instead of panicking via expect.
        let receiver = self
            .forward_receiver
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if receiver.is_some() {
            self.forward_active.store(true, Ordering::Release);
        }
        receiver
    }

    pub fn subscribe(&self, filter: Option<EventCategory>) -> EventReceiver {
        EventReceiver {
            receiver: self.sender.subscribe(),
            filter,
        }
    }
}

pub struct EventReceiver {
    receiver: broadcast::Receiver<Event>,
    filter: Option<EventCategory>,
}

impl EventReceiver {
    pub async fn recv(&mut self) -> std::result::Result<Event, broadcast::error::RecvError> {
        loop {
            let event = self.receiver.recv().await?;
            // MSRV-compatible: avoid `Option::is_none_or` (stabilized 1.82).
            if self
                .filter
                .map_or(true, |filter| event.category() == filter)
            {
                return Ok(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscriber_receives_matching_event() {
        let bus = EventBus::new(8);
        let mut receiver = bus.subscribe(Some(EventCategory::Proxy));
        bus.publish(Event::History(HistoryEvent::Cleared));
        bus.publish(Event::Proxy(ProxyEvent::Stopped));
        assert!(matches!(receiver.recv().await.unwrap(), Event::Proxy(_)));
    }

    #[test]
    fn response_interception_serializes_corresponding_request() {
        let event = Event::Interception(InterceptionEvent::Response {
            id: "response-1".into(),
            raw: b"HTTP/1.1 200 OK\r\n\r\n".to_vec(),
            request_raw: b"GET / HTTP/1.1\r\n\r\n".to_vec(),
            url: "https://example.com/".into(),
        });
        let json = serde_json::to_value(event).unwrap();

        assert_eq!(json["payload"]["type"], "response");
        assert_eq!(json["payload"]["url"], "https://example.com/");
        assert!(json["payload"]["requestRaw"].is_array());
        assert!(json["payload"].get("request_raw").is_none());
    }
}
