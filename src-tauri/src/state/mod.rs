use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use rustls::ClientConfig;
use serde::{Deserialize, Serialize};
use tokio::sync::{watch, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::{
    database::Database,
    event_bus::EventBus,
    history::History,
    logging::LogStore,
    proxy::InterceptionManager,
    scope::ScopeManager,
    tls::CertificateAuthority,
    worker::{TaskQueue, WorkerPool},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyState {
    pub port: u16,
    pub bind_address: String,
    pub running: bool,
    pub connection_count: usize,
    pub intercepting: bool,
    pub certificate_status: String,
}

impl Default for ProxyState {
    fn default() -> Self {
        Self {
            port: 8080,
            bind_address: "127.0.0.1".into(),
            running: false,
            connection_count: 0,
            intercepting: false,
            certificate_status: "unknown".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectState {
    pub current_project_path: Option<PathBuf>,
    pub archive_path: Option<PathBuf>,
    pub name: Option<String>,
    pub temporary: bool,
    pub dirty: bool,
    pub autosave_interval_seconds: u64,
    #[serde(skip)]
    pub working_path_owned: bool,
}

impl Default for ProjectState {
    fn default() -> Self {
        Self {
            current_project_path: None,
            archive_path: None,
            name: None,
            temporary: false,
            dirty: false,
            autosave_interval_seconds: 30,
            working_path_owned: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct UpstreamProxyConfig {
    pub enabled: bool,
    /// "http" or "socks5".
    pub kind: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl Default for UpstreamProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: "http".into(),
            host: String::new(),
            port: 8080,
            username: String::new(),
            password: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct InterceptionRule {
    pub id: String,
    pub enabled: bool,
    pub operator: String,
    pub match_type: String,
    pub relationship: String,
    pub condition: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct MatchReplaceRule {
    pub id: String,
    pub enabled: bool,
    /// Legacy: "request" or "response" — kept for migration, prefer `rule_type`.
    #[serde(default)]
    pub location: String,
    /// Granular target: requestHost, requestHeader, requestBody, requestParamName,
    /// requestParamValue, responseHeader, responseBody, responseParamName, responseParamValue
    #[serde(rename = "type", default)]
    pub rule_type: String,
    #[serde(rename = "match")]
    pub match_str: String,
    pub replace: String,
    #[serde(default)]
    pub is_regex: bool,
}

impl MatchReplaceRule {
    pub fn effective_type(&self) -> &str {
        if !self.rule_type.is_empty() {
            &self.rule_type
        } else if self.location == "response" {
            "responseBody"
        } else {
            "requestBody"
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct SettingsState {
    pub theme: String,
    pub proxy_port: u16,
    pub proxy_bind_address: String,
    pub proxy_intercepting: bool,
    pub proxy_intercept_mode: String,
    pub intercept_content_types: Vec<String>,
    pub request_interception_rules: Vec<InterceptionRule>,
    pub response_interception_rules: Vec<InterceptionRule>,
    pub certificate_directory: String,
    pub autosave_interval_seconds: u64,
    pub compression_mode: String,
    pub intercept_in_scope_only: bool,
    pub upstream_timeout_seconds: u64,
    pub upstream_proxy: UpstreamProxyConfig,
    pub worker_threads: usize,
    pub history_limit: usize,
    pub font_size: u8,
    pub message_editor_font_size: u8,
    pub layout_split_percent: u8,
    #[serde(default = "default_shortcut_modifier")]
    pub shortcut_modifier: String,
    pub show_logs_tab: bool,
    #[serde(default)]
    pub match_replace_rules: Vec<MatchReplaceRule>,
    pub ai_enabled: bool,
    pub ai_base_url: String,
    pub ai_model_name: String,
    pub ai_request_timeout_seconds: u64,
    pub ai_turn_step_limit: usize,
    pub ai_enter_to_send: bool,
    pub ai_api_key_configured: bool,
    pub ai_api_key_prefix: String,
    pub ai_api_key_suffix: String,
}

fn default_shortcut_modifier() -> String {
    if cfg!(target_os = "macos") {
        "command".into()
    } else {
        "control".into()
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            proxy_port: 8080,
            proxy_bind_address: "127.0.0.1".into(),
            proxy_intercepting: false,
            proxy_intercept_mode: "none".into(),
            intercept_content_types: Vec::new(),
            request_interception_rules: Vec::new(),
            response_interception_rules: Vec::new(),
            certificate_directory: dirs::home_dir()
                .map(|path| path.join(".witness/certs").display().to_string())
                .unwrap_or_else(|| ".witness/certs".into()),
            autosave_interval_seconds: 30,
            compression_mode: "decompressAll".into(),
            intercept_in_scope_only: false,
            upstream_timeout_seconds: 30,
            upstream_proxy: UpstreamProxyConfig::default(),
            worker_threads: 4,
            history_limit: 100_000,
            font_size: 14,
            message_editor_font_size: 12,
            layout_split_percent: 46,
            shortcut_modifier: default_shortcut_modifier(),
            show_logs_tab: false,
            ai_enabled: false,
            ai_base_url: String::new(),
            ai_model_name: String::new(),
            ai_request_timeout_seconds: 60,
            ai_turn_step_limit: 8,
            ai_enter_to_send: true,
            ai_api_key_configured: false,
            ai_api_key_prefix: String::new(),
            ai_api_key_suffix: String::new(),
            match_replace_rules: Vec::new(),
        }
    }
}

impl SettingsState {
    pub fn interception_enabled(&self) -> bool {
        self.proxy_intercepting && self.proxy_intercept_mode != "none"
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub proxy: ProxyState,
    pub project: ProjectState,
    pub settings: SettingsState,
    pub memory_usage_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficStatsSnapshot {
    pub requests_processed: u64,
    pub total_requests_sent: u64,
    pub total_responses_received: u64,
    pub packet_loss_percent: f64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub volume_transferred_bytes: u64,
    pub uptime_seconds: u64,
}

#[derive(Clone)]
pub struct TrafficStats {
    started_at: Instant,
    requests_processed: Arc<AtomicU64>,
    total_requests_sent: Arc<AtomicU64>,
    total_responses_received: Arc<AtomicU64>,
    bytes_sent: Arc<AtomicU64>,
    bytes_received: Arc<AtomicU64>,
}

impl Default for TrafficStats {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            requests_processed: Arc::new(AtomicU64::new(0)),
            total_requests_sent: Arc::new(AtomicU64::new(0)),
            total_responses_received: Arc::new(AtomicU64::new(0)),
            bytes_sent: Arc::new(AtomicU64::new(0)),
            bytes_received: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl TrafficStats {
    pub fn record_processed(&self) {
        self.requests_processed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_sent(&self, bytes: usize) {
        self.total_requests_sent.fetch_add(1, Ordering::Relaxed);
        self.bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn record_received(&self, bytes: usize) {
        self.total_responses_received
            .fetch_add(1, Ordering::Relaxed);
        self.bytes_received
            .fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> TrafficStatsSnapshot {
        let total_requests_sent = self.total_requests_sent.load(Ordering::Relaxed);
        let total_responses_received = self.total_responses_received.load(Ordering::Relaxed);
        let bytes_sent = self.bytes_sent.load(Ordering::Relaxed);
        let bytes_received = self.bytes_received.load(Ordering::Relaxed);
        let lost_requests = total_requests_sent.saturating_sub(total_responses_received);
        let packet_loss_percent = if total_requests_sent == 0 {
            0.0
        } else {
            (lost_requests as f64 / total_requests_sent as f64) * 100.0
        };
        TrafficStatsSnapshot {
            requests_processed: self.requests_processed.load(Ordering::Relaxed),
            total_requests_sent,
            total_responses_received,
            packet_loss_percent,
            bytes_sent,
            bytes_received,
            volume_transferred_bytes: bytes_sent.saturating_add(bytes_received),
            uptime_seconds: self.started_at.elapsed().as_secs(),
        }
    }
}

#[derive(Default)]
pub struct TemporaryProjectCleanup {
    path: std::sync::Mutex<Option<PathBuf>>,
}

impl TemporaryProjectCleanup {
    pub fn replace(&self, path: Option<PathBuf>) -> Option<PathBuf> {
        let mut path_guard = self
            .path
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(&mut *path_guard, path)
    }

    pub fn take(&self) -> Option<PathBuf> {
        self.path
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }
}

impl Drop for TemporaryProjectCleanup {
    fn drop(&mut self) {
        if let Some(path) = self
            .path
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            if let Err(error) = std::fs::remove_dir_all(&path) {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(
                        target: "witness_lib::project",
                        operation = "working_copy_cleanup",
                        phase = "drop_fallback",
                        working_path = %path.display(),
                        error = %error,
                        "last-resort working-copy cleanup failed while application state was dropped"
                    );
                }
            }
        }
    }
}

#[derive(Clone)]
pub struct RepeaterCancellation {
    pub operation_id: String,
    pub token: CancellationToken,
}

#[derive(Clone)]
pub struct AppState {
    pub ai_credentials: Arc<std::sync::Mutex<Option<crate::ai::AiCredentialStore>>>,
    pub ai_credentials_paths: Arc<std::sync::Mutex<Option<(PathBuf, PathBuf)>>>,
    pub ai_credentials_generation: Arc<AtomicU64>,
    pub ai_credentials_ready: Arc<watch::Sender<Option<Result<(), String>>>>,
    pub ai_credentials_operation: Arc<AtomicU8>,
    pub ai_credentials_error: Arc<std::sync::Mutex<Option<String>>>,
    pub ai_inference_cancellations: Arc<Mutex<HashMap<String, CancellationToken>>>,
    pub proxy: Arc<RwLock<ProxyState>>,
    pub project: Arc<RwLock<ProjectState>>,
    pub settings: Arc<RwLock<SettingsState>>,
    pub event_bus: EventBus,
    pub task_queue: TaskQueue,
    pub proxy_lifecycle: Arc<Mutex<()>>,
    pub proxy_task: Arc<Mutex<Option<ProxyTask>>>,
    pub certificate_authority: Arc<RwLock<Option<Arc<CertificateAuthority>>>>,
    pub upstream_tls_config: Arc<RwLock<Arc<ClientConfig>>>,
    pub http3_origins: Arc<RwLock<HashMap<String, u16>>>,
    pub database: Arc<Mutex<Option<Database>>>,
    pub history: History,
    pub history_recorder_started: Arc<AtomicBool>,
    pub history_query_cancellation: Arc<Mutex<Option<CancellationToken>>>,
    pub repeater_cancellations: Arc<Mutex<HashMap<String, RepeaterCancellation>>>,
    /// Serializes project activation, saves, autosave, close, and history writes.
    pub project_save_lock: Arc<Mutex<()>>,
    pub project_generation: Arc<AtomicU64>,
    pub interceptions: InterceptionManager,
    pub project_autosave_cancellation: Arc<Mutex<Option<CancellationToken>>>,
    pub temporary_project_cleanup: Arc<TemporaryProjectCleanup>,
    pub scope: ScopeManager,
    pub logs: LogStore,
    pub traffic_stats: TrafficStats,
    memory_system: Arc<std::sync::Mutex<sysinfo::System>>,
}

pub struct ProxyTask {
    pub cancellation: CancellationToken,
    pub handle: tauri::async_runtime::JoinHandle<()>,
}

impl AppState {
    pub fn new() -> Self {
        let event_bus = EventBus::new(1_024);
        let logs = LogStore::default();
        logs.connect(event_bus.clone());
        #[cfg(not(test))]
        let settings = crate::settings::load_global().unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to load persisted settings; using defaults");
            SettingsState::default()
        });
        #[cfg(test)]
        let settings = SettingsState::default();
        let worker_threads = settings.worker_threads;
        let history_limit = settings.history_limit;
        let interception_enabled = settings.interception_enabled();
        let traffic_stats = TrafficStats::default();
        let interceptions = InterceptionManager::new(interception_enabled);
        let (task_queue, receiver) = TaskQueue::new(256);
        WorkerPool::spawn(
            worker_threads,
            receiver,
            event_bus.clone(),
            traffic_stats.clone(),
        );

        let proxy = ProxyState {
            port: settings.proxy_port,
            bind_address: settings.proxy_bind_address.clone(),
            intercepting: interception_enabled,
            ..ProxyState::default()
        };
        let project = ProjectState {
            autosave_interval_seconds: settings.autosave_interval_seconds,
            ..ProjectState::default()
        };

        let (ai_credentials_ready, _) = watch::channel(None);

        Self {
            ai_credentials: Arc::new(std::sync::Mutex::new(None)),
            ai_credentials_paths: Arc::new(std::sync::Mutex::new(None)),
            ai_credentials_generation: Arc::new(AtomicU64::new(0)),
            ai_credentials_ready: Arc::new(ai_credentials_ready),
            ai_credentials_operation: Arc::new(AtomicU8::new(0)),
            ai_credentials_error: Arc::new(std::sync::Mutex::new(None)),
            ai_inference_cancellations: Arc::new(Mutex::new(HashMap::new())),
            proxy: Arc::new(RwLock::new(proxy)),
            project: Arc::new(RwLock::new(project)),
            settings: Arc::new(RwLock::new(settings)),
            event_bus,
            task_queue,
            proxy_lifecycle: Arc::new(Mutex::new(())),
            proxy_task: Arc::new(Mutex::new(None)),
            certificate_authority: Arc::new(RwLock::new(None)),
            upstream_tls_config: Arc::new(RwLock::new(CertificateAuthority::client_config())),
            http3_origins: Arc::new(RwLock::new(HashMap::new())),
            database: Arc::new(Mutex::new(None)),
            history: History::new(history_limit),
            history_recorder_started: Arc::new(AtomicBool::new(false)),
            history_query_cancellation: Arc::new(Mutex::new(None)),
            repeater_cancellations: Arc::new(Mutex::new(HashMap::new())),
            project_save_lock: Arc::new(Mutex::new(())),
            project_generation: Arc::new(AtomicU64::new(0)),
            interceptions,
            project_autosave_cancellation: Arc::new(Mutex::new(None)),
            temporary_project_cleanup: Arc::new(TemporaryProjectCleanup::default()),
            scope: ScopeManager::default(),
            logs,
            traffic_stats,
            memory_system: Arc::new(std::sync::Mutex::new(sysinfo::System::new())),
        }
    }

    pub async fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            proxy: self.proxy.read().await.clone(),
            project: self.project.read().await.clone(),
            settings: self.settings.read().await.clone(),
            memory_usage_bytes: self.memory_usage_bytes(),
        }
    }

    fn memory_usage_bytes(&self) -> Option<u64> {
        let pid = sysinfo::Pid::from_u32(std::process::id());
        let mut system = self.memory_system.lock().ok()?;
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        system.process(pid).map(sysinfo::Process::memory)
    }

    pub async fn upstream_timeout(&self) -> Duration {
        Duration::from_secs(self.settings.read().await.upstream_timeout_seconds)
    }

    pub async fn upstream_proxy(&self) -> Option<UpstreamProxyConfig> {
        let config = self.settings.read().await.upstream_proxy.clone();
        (config.enabled && !config.host.trim().is_empty()).then_some(config)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn state_is_shared_safely() {
        let state = AppState::new();
        let cloned = state.clone();
        let task = tokio::spawn(async move {
            cloned.proxy.write().await.port = 9090;
        });
        task.await.unwrap();
        assert_eq!(state.proxy.read().await.port, 9090);
    }

    #[test]
    fn temporary_project_cleanup_removes_owned_directory() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(".session-test");
        std::fs::create_dir_all(&path).unwrap();
        {
            let cleanup = TemporaryProjectCleanup::default();
            cleanup.replace(Some(path.clone()));
        }
        assert!(!path.exists());
    }
}
