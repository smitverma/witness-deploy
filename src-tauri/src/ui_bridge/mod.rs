use std::{
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    ai::{AiConnectionResult, AiInferenceRequest, AiInferenceResponse},
    comparer::DiffResult,
    database::{
        Database, FuzzScanRecord, HistoryDetail, HistoryFilter, Identity, IdentityBundle,
        IdentityGroup, IdentityGroupInput, IdentityInjectionDescriptor, IdentityInput,
        OrganizerBundle, OrganizerFolder, OrganizerItem, OrganizerItemInput,
    },
    decoder::DecodeResult,
    event_bus::{Event, LogEvent, ProjectEvent, ProxyEvent, RepeaterEvent},
    history::{start_history_recorder, HistoryEntry},
    logging,
    project::{start_autosave, ProjectInfo, ProjectManager, RecentProject, PROJECT_EXTENSION},
    proxy::{InterceptionResolution, ProxyEngine},
    repeater::RepeaterResponse,
    scope::{validate as validate_scope, ScopeEntry, ScopeSnapshot},
    state::{
        AppSnapshot, AppState, InterceptionRule, MatchReplaceRule, ProxyTask, SettingsState,
        TrafficStatsSnapshot,
    },
    tls::CertificateAuthority,
    worker::{Task, TaskResult},
};

type CommandResult<T> = std::result::Result<T, String>;

/// Single source of truth for the "no project" IPC error.
pub(crate) const NO_PROJECT_MSG: &str = "No project is open";

/// IPC payload caps to bound memory on the Tauri bridge.
const MAX_IMPORT_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_DECODER_INPUT_BYTES: usize = 5 * 1024 * 1024;
const MAX_COMPARE_INPUT_BYTES: usize = 1 * 1024 * 1024;

/// Locks the database or returns [`NO_PROJECT_MSG`] when no project is open.
/// Returns the guard (checked non-empty) so callers use `.as_ref().expect(..)`
/// / `.as_mut().expect(..)` without re-checking the error string.
async fn require_db(
    state: &AppState,
) -> CommandResult<tokio::sync::MutexGuard<'_, Option<Database>>> {
    let guard = state.database.lock().await;
    if guard.is_none() {
        return Err(NO_PROJECT_MSG.to_string());
    }
    Ok(guard)
}

/// Marks the current project dirty for autosave.
async fn mark_dirty(state: &AppState) {
    state.project.write().await.dirty = true;
}

/// Returns the current working-copy path or [`NO_PROJECT_MSG`] when no
/// project (WNS working copy) is open.
async fn ensure_wns(state: &AppState) -> CommandResult<PathBuf> {
    state
        .project
        .read()
        .await
        .current_project_path
        .clone()
        .ok_or_else(|| NO_PROJECT_MSG.to_string())
}

const AI_CREDENTIAL_OPERATION_IDLE: u8 = 0;
const AI_CREDENTIAL_OPERATION_SAVE: u8 = 1;
const AI_CREDENTIAL_OPERATION_DELETE: u8 = 2;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    theme: Option<String>,
    proxy_port: Option<u16>,
    proxy_bind_address: Option<String>,
    proxy_intercepting: Option<bool>,
    proxy_intercept_mode: Option<String>,
    intercept_content_types: Option<Vec<String>>,
    request_interception_rules: Option<Vec<InterceptionRule>>,
    response_interception_rules: Option<Vec<InterceptionRule>>,
    certificate_directory: Option<String>,
    autosave_interval_seconds: Option<u64>,
    compression_mode: Option<String>,
    intercept_in_scope_only: Option<bool>,
    upstream_timeout_seconds: Option<u64>,
    upstream_proxy: Option<crate::state::UpstreamProxyConfig>,
    worker_threads: Option<usize>,
    history_limit: Option<usize>,
    font_size: Option<u8>,
    message_editor_font_size: Option<u8>,
    layout_split_percent: Option<u8>,
    shortcut_modifier: Option<String>,
    show_logs_tab: Option<bool>,
    ai_enabled: Option<bool>,
    ai_base_url: Option<String>,
    ai_model_name: Option<String>,
    ai_request_timeout_seconds: Option<u64>,
    ai_turn_step_limit: Option<usize>,
    ai_enter_to_send: Option<bool>,
    match_replace_rules: Option<Vec<MatchReplaceRule>>,
}

impl SettingsPatch {
    fn apply(self, settings: &mut SettingsState) {
        macro_rules! apply {
            ($field:ident) => {
                if let Some(value) = self.$field {
                    settings.$field = value;
                }
            };
        }
        apply!(theme);
        apply!(proxy_port);
        apply!(proxy_bind_address);
        apply!(proxy_intercepting);
        apply!(proxy_intercept_mode);
        apply!(intercept_content_types);
        apply!(request_interception_rules);
        apply!(response_interception_rules);
        apply!(certificate_directory);
        apply!(autosave_interval_seconds);
        apply!(compression_mode);
        apply!(intercept_in_scope_only);
        apply!(upstream_timeout_seconds);
        apply!(upstream_proxy);
        apply!(worker_threads);
        apply!(history_limit);
        apply!(font_size);
        apply!(message_editor_font_size);
        apply!(layout_split_percent);
        apply!(shortcut_modifier);
        apply!(show_logs_tab);
        apply!(ai_enabled);
        apply!(ai_base_url);
        apply!(ai_model_name);
        apply!(ai_request_timeout_seconds);
        apply!(ai_turn_step_limit);
        apply!(ai_enter_to_send);
        apply!(match_replace_rules);
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateInfo {
    certificate_path: String,
    generated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiKeyStatus {
    configured: bool,
    prefix: String,
    suffix: String,
    pending: bool,
    operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRuntimeStatus {
    ready: bool,
    initializing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn ai_key_status(state: &AppState, settings: &SettingsState) -> AiKeyStatus {
    let operation = match state.ai_credentials_operation.load(Ordering::Acquire) {
        AI_CREDENTIAL_OPERATION_SAVE => Some("save".to_string()),
        AI_CREDENTIAL_OPERATION_DELETE => Some("delete".to_string()),
        _ => None,
    };
    let error = state
        .ai_credentials_error
        .lock()
        .ok()
        .and_then(|error| error.clone());
    AiKeyStatus {
        configured: settings.ai_api_key_configured,
        prefix: settings.ai_api_key_prefix.clone(),
        suffix: settings.ai_api_key_suffix.clone(),
        pending: operation.is_some(),
        operation,
        error,
    }
}

fn ai_runtime_status(state: &AppState) -> AiRuntimeStatus {
    let store_ready = state
        .ai_credentials
        .lock()
        .map(|credentials| credentials.is_some())
        .unwrap_or(false);
    if store_ready {
        return AiRuntimeStatus {
            ready: true,
            initializing: false,
            error: None,
        };
    }

    match state.ai_credentials_ready.borrow().clone() {
        None => AiRuntimeStatus {
            ready: false,
            initializing: true,
            error: None,
        },
        Some(Ok(())) => AiRuntimeStatus {
            ready: false,
            initializing: false,
            error: Some("AI credential store is unavailable".into()),
        },
        Some(Err(error)) => AiRuntimeStatus {
            ready: false,
            initializing: false,
            error: Some(error),
        },
    }
}

fn begin_ai_credential_operation(state: &AppState, operation: u8) -> CommandResult<()> {
    state
        .ai_credentials_error
        .lock()
        .map_err(|_| "AI credential operation state lock poisoned".to_string())?
        .take();
    state
        .ai_credentials_operation
        .compare_exchange(
            AI_CREDENTIAL_OPERATION_IDLE,
            operation,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .map(|_| ())
        .map_err(|_| "another AI credential operation is already in progress".to_string())
}

fn complete_ai_credential_operation(state: &AppState, result: CommandResult<()>) {
    if let Err(error) = result {
        tracing::error!(module = "credentials", %error, "AI credential operation failed");
        if let Ok(mut saved_error) = state.ai_credentials_error.lock() {
            *saved_error = Some(error);
        }
    }
    state
        .ai_credentials_operation
        .store(AI_CREDENTIAL_OPERATION_IDLE, Ordering::Release);
}

async fn save_ai_key_settings(
    state: &AppState,
    prefix: String,
    suffix: String,
) -> CommandResult<()> {
    let mut settings = state.settings.write().await;
    let mut updated = settings.clone();
    updated.ai_api_key_configured = true;
    updated.ai_api_key_prefix = prefix;
    updated.ai_api_key_suffix = suffix;
    crate::settings::save_global(&updated).map_err(|error| error.to_string())?;
    *settings = updated;
    Ok(())
}

async fn clear_ai_key_settings(state: &AppState) -> CommandResult<()> {
    let mut settings = state.settings.write().await;
    let mut updated = settings.clone();
    updated.ai_api_key_configured = false;
    updated.ai_api_key_prefix.clear();
    updated.ai_api_key_suffix.clear();
    crate::settings::save_global(&updated).map_err(|error| error.to_string())?;
    *settings = updated;
    Ok(())
}

fn ensure_ai_credentials_idle(state: &AppState) -> CommandResult<()> {
    if state.ai_credentials_operation.load(Ordering::Acquire) != AI_CREDENTIAL_OPERATION_IDLE {
        return Err("an AI credential operation is still in progress; please wait".to_string());
    }
    Ok(())
}

async fn wait_for_ai_credentials(state: &AppState) -> CommandResult<()> {
    if state
        .ai_credentials
        .lock()
        .map_err(|_| "AI credential store lock poisoned".to_string())?
        .is_some()
    {
        return Ok(());
    }
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut ready = state.ai_credentials_ready.subscribe();
        loop {
            if let Some(result) = ready.borrow().clone() {
                return result
                    .map_err(|error| format!("AI credential store is unavailable: {error}"));
            }
            ready
                .changed()
                .await
                .map_err(|_| "AI credential store initialization task stopped".to_string())?;
        }
    })
    .await
    .map_err(|_| {
        "AI credential store is still initializing; please try again shortly".to_string()
    })?
}

async fn delete_ai_credentials(state: &AppState) -> CommandResult<()> {
    let credentials = state.ai_credentials.clone();
    let credential_paths = state.ai_credentials_paths.clone();
    let credentials_generation = state.ai_credentials_generation.clone();
    tracing::info!(module = "credentials", "AI credential deletion requested");
    tokio::task::spawn_blocking(move || {
        let credentials = credentials
            .lock()
            .map_err(|_| "AI credential store lock poisoned".to_string())?;
        if let Some(store) = credentials.as_ref() {
            store.delete_key().map_err(|error| error.to_string())?;
        } else {
            // Initialization may still be decrypting an old snapshot. Invalidate
            // that result before removing the file so it cannot reinstall the
            // deleted key when it finishes.
            credentials_generation.fetch_add(1, Ordering::AcqRel);
            let snapshot = credential_paths
                .lock()
                .map_err(|_| "AI credential path lock poisoned".to_string())?
                .as_ref()
                .map(|(snapshot, _)| snapshot.clone());
            if let Some(snapshot) = snapshot {
                match std::fs::remove_file(snapshot) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(format!("could not remove AI credential snapshot: {error}"));
                    }
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|error| format!("AI credential deletion task failed: {error}"))??;
    tracing::info!(module = "credentials", "AI credential deletion completed");
    Ok(())
}

fn validate_interception_rules(rules: &[InterceptionRule], label: &str) -> CommandResult<()> {
    if rules.len() > 100 {
        return Err(format!(
            "{label} interception rules cannot exceed 100 entries"
        ));
    }
    for rule in rules {
        if rule.id.trim().is_empty() || rule.id.len() > 128 {
            return Err(format!("{label} interception rules require a valid id"));
        }
        if !matches!(rule.operator.as_str(), "and" | "or") {
            return Err(format!(
                "invalid boolean operator in {label} interception rules"
            ));
        }
        if !matches!(
            rule.match_type.as_str(),
            "url"
                | "domain"
                | "ipAddress"
                | "protocol"
                | "fileExtension"
                | "httpMethod"
                | "contentType"
                | "request"
                | "cookieName"
                | "cookieValue"
                | "anyHeader"
                | "body"
                | "paramName"
                | "paramValue"
                | "listenerPort"
                | "inScope"
        ) {
            return Err(format!("invalid match type in {label} interception rules"));
        }
        let scope_rule = rule.match_type == "inScope";
        if scope_rule && !matches!(rule.relationship.as_str(), "isInScope" | "isNotInScope") {
            return Err("scope rules must use an in-scope relationship".to_string());
        }
        if !scope_rule
            && !matches!(
                rule.relationship.as_str(),
                "matches" | "doesNotMatch" | "contains" | "doesNotContain"
            )
        {
            return Err(format!(
                "invalid relationship in {label} interception rules"
            ));
        }
        if !scope_rule && (rule.condition.trim().is_empty() || rule.condition.len() > 512) {
            return Err(format!(
                "{label} interception rule conditions must be 1 to 512 characters"
            ));
        }
        if matches!(rule.relationship.as_str(), "matches" | "doesNotMatch") {
            regex::RegexBuilder::new(&rule.condition)
                .case_insensitive(true)
                .build()
                .map_err(|error| {
                    format!("invalid regular expression in {label} interception rules: {error}")
                })?;
        }
    }
    Ok(())
}

fn validate_match_replace_rules(rules: &[MatchReplaceRule]) -> CommandResult<()> {
    if rules.len() > 100 {
        return Err("match/replace rules cannot exceed 100 entries".into());
    }
    const ALLOWED: &[&str] = &[
        "requestHost",
        "requestHeader",
        "requestBody",
        "requestParamName",
        "requestParamValue",
        "responseHeader",
        "responseBody",
        "responseParamName",
        "responseParamValue",
    ];
    for rule in rules {
        if rule.id.trim().is_empty() || rule.id.len() > 128 {
            return Err("match/replace rules require a valid id".into());
        }
        let effective = rule.effective_type();
        if !ALLOWED.contains(&effective) {
            // Legacy check: allow old location-only rules
            if !rule.rule_type.is_empty()
                || !matches!(rule.location.as_str(), "request" | "response")
            {
                return Err(format!(
                    "invalid match/replace type '{}'; must be one of {}",
                    rule.rule_type,
                    ALLOWED.join(", ")
                ));
            }
        }
        if rule.match_str.is_empty() || rule.match_str.len() > 2048 {
            return Err("match/replace match must be 1 to 2048 characters".into());
        }
        if rule.replace.len() > 4096 {
            return Err("match/replace replacement cannot exceed 4096 characters".into());
        }
        if rule.is_regex {
            // Bound regex compile cost (mirrors match_replace::MAX_REGEX_LEN).
            if rule.match_str.len() > crate::proxy::match_replace::MAX_REGEX_LEN {
                return Err(format!(
                    "match/replace regex must not exceed {} characters",
                    crate::proxy::match_replace::MAX_REGEX_LEN
                ));
            }
            regex::Regex::new(&rule.match_str).map_err(|error| {
                format!("invalid regular expression in match/replace rule: {error}")
            })?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_log_entries(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> CommandResult<Vec<LogEvent>> {
    let _operation = logging::OperationGuard::new("command.get_log_entries");
    let limit = limit
        .unwrap_or(logging::DEFAULT_LOG_DISPLAY_LIMIT)
        .clamp(1, logging::MAX_LOG_DISPLAY_LIMIT);
    Ok(state.logs.entries(limit))
}

#[tauri::command]
pub async fn get_traffic_stats(state: State<'_, AppState>) -> CommandResult<TrafficStatsSnapshot> {
    let _operation = logging::OperationGuard::new("command.get_traffic_stats");
    Ok(state.traffic_stats.snapshot())
}

#[tauri::command]
pub async fn get_organizer(state: State<'_, AppState>) -> CommandResult<OrganizerBundle> {
    let _operation = logging::OperationGuard::new("command.get_organizer");
    require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .organizer_snapshot()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_organizer_folder(
    state: State<'_, AppState>,
    name: String,
    parent_id: Option<String>,
) -> CommandResult<OrganizerFolder> {
    let _operation = logging::OperationGuard::new("command.create_organizer_folder");
    let _ = ensure_wns(state.inner()).await?;
    let _save_guard = state.project_save_lock.lock().await;
    let folder = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .create_organizer_folder(&name, parent_id.as_deref())
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(folder)
}

#[tauri::command]
pub async fn update_organizer_folder(
    state: State<'_, AppState>,
    id: String,
    name: String,
    parent_id: Option<String>,
) -> CommandResult<OrganizerFolder> {
    let _operation = logging::OperationGuard::new("command.update_organizer_folder");
    let _ = ensure_wns(state.inner()).await?;
    let _save_guard = state.project_save_lock.lock().await;
    let folder = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .update_organizer_folder(&id, &name, parent_id.as_deref())
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(folder)
}

#[tauri::command]
pub async fn delete_organizer_folder(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<bool> {
    let _operation = logging::OperationGuard::new("command.delete_organizer_folder");
    let _ = ensure_wns(state.inner()).await?;
    let _save_guard = state.project_save_lock.lock().await;
    let deleted = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .delete_organizer_folder(&id)
        .map_err(|error| error.to_string())?;
    if deleted {
        mark_dirty(state.inner()).await;
    }
    Ok(deleted)
}

#[tauri::command]
pub async fn create_organizer_item(
    state: State<'_, AppState>,
    input: OrganizerItemInput,
) -> CommandResult<OrganizerItem> {
    let _operation = logging::OperationGuard::new("command.create_organizer_item");
    let _ = ensure_wns(state.inner()).await?;
    let _save_guard = state.project_save_lock.lock().await;
    let item = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .create_organizer_item(&input)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(item)
}

#[tauri::command]
pub async fn update_organizer_item(
    state: State<'_, AppState>,
    id: String,
    input: OrganizerItemInput,
) -> CommandResult<OrganizerItem> {
    let _operation = logging::OperationGuard::new("command.update_organizer_item");
    let _ = ensure_wns(state.inner()).await?;
    let _save_guard = state.project_save_lock.lock().await;
    let item = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .update_organizer_item(&id, &input)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(item)
}

#[tauri::command]
pub async fn delete_organizer_item(state: State<'_, AppState>, id: String) -> CommandResult<bool> {
    let _operation = logging::OperationGuard::new("command.delete_organizer_item");
    let _ = ensure_wns(state.inner()).await?;
    let _save_guard = state.project_save_lock.lock().await;
    let deleted = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .delete_organizer_item(&id)
        .map_err(|error| error.to_string())?;
    if deleted {
        mark_dirty(state.inner()).await;
    }
    Ok(deleted)
}

#[tauri::command]
pub async fn import_organizer(
    state: State<'_, AppState>,
    bundle: OrganizerBundle,
) -> CommandResult<usize> {
    let _operation = logging::OperationGuard::new("command.import_organizer");
    let _ = ensure_wns(state.inner()).await?;
    let _save_guard = state.project_save_lock.lock().await;
    let imported = state
        .database
        .lock()
        .await
        .as_mut()
        .ok_or_else(|| NO_PROJECT_MSG.to_string())?
        .import_organizer(&bundle)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(imported)
}

#[tauri::command]
pub async fn export_organizer_json(state: State<'_, AppState>) -> CommandResult<Option<String>> {
    let _operation = logging::OperationGuard::new("command.export_organizer_json");
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export organizer JSON")
        .add_filter("JSON", &["json"])
        .set_file_name("witness-organizer.json")
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let bundle = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .organizer_snapshot()
        .map_err(|error| error.to_string())?;
    let content = serde_json::to_vec_pretty(&bundle).map_err(|error| error.to_string())?;
    tokio::fs::write(file.path(), content)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(file.path().display().to_string()))
}

#[tauri::command]
pub async fn import_organizer_json(state: State<'_, AppState>) -> CommandResult<Option<usize>> {
    let _operation = logging::OperationGuard::new("command.import_organizer_json");
    let _save_guard = state.project_save_lock.lock().await;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Import organizer JSON")
        .add_filter("JSON", &["json"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let content = tokio::fs::read(file.path())
        .await
        .map_err(|error| error.to_string())?;
    let bundle: OrganizerBundle = serde_json::from_slice(&content)
        .map_err(|error| format!("invalid organizer JSON: {error}"))?;
    let imported = state
        .database
        .lock()
        .await
        .as_mut()
        .ok_or_else(|| NO_PROJECT_MSG.to_string())?
        .import_organizer(&bundle)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(Some(imported))
}

#[tauri::command]
pub async fn get_identity_groups(state: State<'_, AppState>) -> CommandResult<IdentityBundle> {
    let _operation = logging::OperationGuard::new("command.get_identity_groups");
    require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .identity_snapshot()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_identity_group(
    state: State<'_, AppState>,
    input: IdentityGroupInput,
) -> CommandResult<IdentityGroup> {
    let _operation = logging::OperationGuard::new("command.create_identity_group");
    let _ = ensure_wns(state.inner()).await?;
    let _save_guard = state.project_save_lock.lock().await;
    let group = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .create_identity_group(&input)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(group)
}

#[tauri::command]
pub async fn update_identity_group(
    state: State<'_, AppState>,
    id: String,
    input: IdentityGroupInput,
) -> CommandResult<IdentityGroup> {
    let _operation = logging::OperationGuard::new("command.update_identity_group");
    let _ = ensure_wns(state.inner()).await?;
    let _save_guard = state.project_save_lock.lock().await;
    let group = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .update_identity_group(&id, &input)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(group)
}

#[tauri::command]
pub async fn delete_identity_group(state: State<'_, AppState>, id: String) -> CommandResult<bool> {
    let _operation = logging::OperationGuard::new("command.delete_identity_group");
    let _ = ensure_wns(state.inner()).await?;
    let _save_guard = state.project_save_lock.lock().await;
    let deleted = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .delete_identity_group(&id)
        .map_err(|error| error.to_string())?;
    if deleted {
        mark_dirty(state.inner()).await;
    }
    Ok(deleted)
}

#[tauri::command]
pub async fn create_identity(
    state: State<'_, AppState>,
    input: IdentityInput,
) -> CommandResult<Identity> {
    let _operation = logging::OperationGuard::new("command.create_identity");
    let _save_guard = state.project_save_lock.lock().await;
    let identity = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .create_identity(&input)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(identity)
}

#[tauri::command]
pub async fn update_identity(
    state: State<'_, AppState>,
    id: String,
    input: IdentityInput,
) -> CommandResult<Identity> {
    let _operation = logging::OperationGuard::new("command.update_identity");
    let _save_guard = state.project_save_lock.lock().await;
    let identity = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .update_identity(&id, &input)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(identity)
}

#[tauri::command]
pub async fn delete_identity(state: State<'_, AppState>, id: String) -> CommandResult<bool> {
    let _operation = logging::OperationGuard::new("command.delete_identity");
    let _save_guard = state.project_save_lock.lock().await;
    let deleted = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .delete_identity(&id)
        .map_err(|error| error.to_string())?;
    if deleted {
        mark_dirty(state.inner()).await;
    }
    Ok(deleted)
}

#[tauri::command]
pub async fn resolve_identity_injection(
    state: State<'_, AppState>,
    identity_id: String,
) -> CommandResult<IdentityInjectionDescriptor> {
    let _operation = logging::OperationGuard::new("command.resolve_identity_injection");
    require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .identity_injection_descriptor(&identity_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn import_identities(
    state: State<'_, AppState>,
    bundle: IdentityBundle,
) -> CommandResult<usize> {
    let _operation = logging::OperationGuard::new("command.import_identities");
    let _save_guard = state.project_save_lock.lock().await;
    let imported = state
        .database
        .lock()
        .await
        .as_mut()
        .ok_or_else(|| NO_PROJECT_MSG.to_string())?
        .import_identities(&bundle)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(imported)
}

#[tauri::command]
pub async fn export_identities_json(state: State<'_, AppState>) -> CommandResult<Option<String>> {
    let _operation = logging::OperationGuard::new("command.export_identities_json");
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Export identities JSON")
        .add_filter("JSON", &["json"])
        .set_file_name("witness-identities.json")
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let bundle = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .identity_snapshot()
        .map_err(|error| error.to_string())?;
    let content = serde_json::to_vec_pretty(&bundle).map_err(|error| error.to_string())?;
    tokio::fs::write(file.path(), content)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(file.path().display().to_string()))
}

#[tauri::command]
pub async fn import_identities_json(state: State<'_, AppState>) -> CommandResult<Option<usize>> {
    let _operation = logging::OperationGuard::new("command.import_identities_json");
    let _save_guard = state.project_save_lock.lock().await;
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_title("Import identities JSON")
        .add_filter("JSON", &["json"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let content = tokio::fs::read(file.path())
        .await
        .map_err(|error| error.to_string())?;
    let bundle: IdentityBundle = serde_json::from_slice(&content)
        .map_err(|error| format!("invalid identities JSON: {error}"))?;
    let imported = state
        .database
        .lock()
        .await
        .as_mut()
        .ok_or_else(|| NO_PROJECT_MSG.to_string())?
        .import_identities(&bundle)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(Some(imported))
}

#[tauri::command]
pub async fn clear_log_entries(state: State<'_, AppState>) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.clear_log_entries");
    state.logs.clear();
    Ok(())
}

#[tauri::command]
pub async fn decoder_transform(
    input: String,
    operation: String,
    padding: bool,
) -> CommandResult<DecodeResult> {
    let _operation = logging::OperationGuard::new("command.decoder_transform");
    if input.len() > MAX_DECODER_INPUT_BYTES {
        return Err("decoder input exceeds 5 MiB".into());
    }
    crate::decoder::transform(&input, &operation, padding).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn compare_text(
    left: String,
    right: String,
    granularity: String,
) -> CommandResult<DiffResult> {
    let _operation = logging::OperationGuard::new("command.compare_text");
    if left.len() > MAX_COMPARE_INPUT_BYTES || right.len() > MAX_COMPARE_INPUT_BYTES {
        return Err("compare input exceeds 1 MiB".into());
    }
    Ok(crate::comparer::compare(&left, &right, &granularity))
}

#[tauri::command]
pub async fn save_workspace(state: State<'_, AppState>, workspace: String) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.save_workspace");
    const MAX_WORKSPACE_BYTES: usize = 512 * 1024 * 1024;
    let workspace_bytes = workspace.len();
    if workspace.len() > MAX_WORKSPACE_BYTES {
        tracing::error!(
            target: "witness_lib::project",
            operation = "workspace_save",
            phase = "validate",
            bytes = workspace_bytes,
            error = "workspace snapshot is too large to save",
            "workspace save rejected"
        );
        return Err("workspace snapshot is too large to save".into());
    }
    if let Err(error) = serde_json::from_str::<serde_json::Value>(&workspace) {
        tracing::error!(
            target: "witness_lib::project",
            operation = "workspace_save",
            phase = "validate",
            bytes = workspace_bytes,
            error = %error,
            "workspace save rejected because the snapshot is invalid JSON"
        );
        return Err(format!("invalid workspace snapshot: {error}"));
    }
    let _save_guard = state.project_save_lock.lock().await;
    let path = ensure_wns(state.inner()).await.map_err(|error| {
        tracing::error!(
            target: "witness_lib::project",
            operation = "workspace_save",
            phase = "resolve_working_copy",
            error = NO_PROJECT_MSG,
            "workspace save rejected because no project is open"
        );
        error
    })?;
    let target = path.join("workspace.json");
    let temporary = path.join("workspace.json.part");
    tracing::info!(
        target: "witness_lib::project",
        operation = "workspace_save",
        phase = "started",
        working_path = %path.display(),
        target = %target.display(),
        temporary = %temporary.display(),
        bytes = workspace_bytes,
        "workspace save started"
    );
    if let Err(error) = tokio::fs::write(&temporary, workspace).await {
        tracing::error!(
            target: "witness_lib::project",
            operation = "workspace_save",
            phase = "write_temporary_snapshot",
            temporary = %temporary.display(),
            error = %error,
            "workspace save failed while writing the temporary snapshot"
        );
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error.to_string());
    }
    // Reuse the shared atomic-replace helper (backup + restore on failure)
    // instead of duplicating the rename dance here.
    let replace_target = target.clone();
    let replace_temp = temporary.clone();
    let replace_result = tokio::task::spawn_blocking(move || {
        crate::export::replace_file(replace_temp, &replace_target)
    })
    .await
    .map_err(|error| error.to_string())?;
    if let Err(error) = replace_result {
        tracing::error!(
            target: "witness_lib::project",
            operation = "workspace_save",
            phase = "install_snapshot",
            target = %target.display(),
            error = %error,
            "workspace save failed while installing the snapshot"
        );
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error.to_string());
    }
    mark_dirty(state.inner()).await;
    tracing::info!(
        target: "witness_lib::project",
        operation = "workspace_save",
        phase = "completed",
        working_path = %path.display(),
        target = %target.display(),
        bytes = workspace_bytes,
        "workspace save completed"
    );
    Ok(())
}

#[tauri::command]
pub async fn get_workspace(state: State<'_, AppState>) -> CommandResult<Option<String>> {
    let _operation = logging::OperationGuard::new("command.get_workspace");
    let _save_guard = state.project_save_lock.lock().await;
    let Some(path) = state.project.read().await.current_project_path.clone() else {
        return Ok(None);
    };
    let workspace = match tokio::fs::read_to_string(path.join("workspace.json")).await {
        Ok(workspace) => workspace,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if workspace.len() > 512 * 1024 * 1024 {
        return Err("workspace snapshot is too large to load".into());
    }
    Ok(Some(workspace))
}

fn path_is_within(source: &Path, candidate: &Path) -> bool {
    let Some(source) = normalize_path(source) else {
        return false;
    };
    let Some(candidate) = normalize_path(candidate) else {
        return false;
    };
    candidate == source || candidate.starts_with(source)
}

async fn restore_database_after_cleanup_failure(
    state: &AppState,
    path: &Path,
    database_was_open: bool,
    operation_name: &str,
) -> Option<String> {
    if !database_was_open {
        return None;
    }
    let database_path = path.join("witness.sqlite3");
    if !database_path.is_file() {
        let error = format!(
            "project database file is missing at {}",
            database_path.display()
        );
        tracing::error!(
            target: "witness_lib::project",
            operation = operation_name,
            phase = "database_restore_after_cleanup_failure",
            working_path = %path.display(),
            database = %database_path.display(),
            error = %error,
            "project database could not be restored after cleanup failed"
        );
        return Some(error);
    }
    match Database::open(path) {
        Ok(database) => {
            *state.database.lock().await = Some(database);
            tracing::info!(
                target: "witness_lib::project",
                operation = operation_name,
                phase = "database_restored_after_cleanup_failure",
                working_path = %path.display(),
                "project database restored after cleanup failed"
            );
            None
        }
        Err(error) => {
            tracing::error!(
                target: "witness_lib::project",
                operation = operation_name,
                phase = "database_restore_after_cleanup_failure",
                working_path = %path.display(),
                error = %error,
                "project database could not be restored after cleanup failed"
            );
            Some(error.to_string())
        }
    }
}

fn normalize_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = if path.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir().ok()?
    };
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Normal(value) => normalized.push(value),
        }
    }
    Some(normalized)
}

#[tauri::command]
pub async fn save_project(
    state: State<'_, AppState>,
    destination: Option<String>,
) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.save_project");
    let project = state.project.read().await.clone();
    let source = match project.current_project_path.clone() {
        Some(source) => source,
        None => {
            tracing::error!(
                target: "witness_lib::project",
                operation = "save",
                phase = "resolve_working_copy",
                error = NO_PROJECT_MSG,
                "project save rejected because no project is open"
            );
            return Err(NO_PROJECT_MSG.into());
        }
    };
    let raw_destination = destination
        .or_else(|| {
            project
                .archive_path
                .as_ref()
                .map(|path| path.display().to_string())
        })
        .ok_or_else(|| "choose a destination .wns file".to_string())?;
    let raw_destination = raw_destination.trim();
    if raw_destination.is_empty() {
        tracing::error!(
            target: "witness_lib::project",
            operation = "save",
            phase = "validate_destination",
            working_path = %source.display(),
            error = "choose a destination .wns file",
            "project save rejected because the destination is empty"
        );
        return Err("choose a destination .wns file".into());
    }
    let mut destination = PathBuf::from(raw_destination);
    if destination.extension().is_none() {
        destination.set_extension(PROJECT_EXTENSION);
    }
    let is_witness = destination
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(PROJECT_EXTENSION));
    if !is_witness {
        tracing::error!(
            target: "witness_lib::project",
            operation = "save",
            phase = "validate_destination",
            working_path = %source.display(),
            destination = %destination.display(),
            error = "project files must use the .wns extension",
            "project save rejected because the destination is not an .wns file"
        );
        return Err("project files must use the .wns extension".into());
    }
    if destination.is_dir() {
        tracing::error!(
            target: "witness_lib::project",
            operation = "save",
            phase = "validate_destination",
            working_path = %source.display(),
            destination = %destination.display(),
            error = "destination is a directory",
            "project save rejected because the destination is a directory"
        );
        return Err("choose a .wns file path outside the active project working copy".into());
    }
    if destination == source || destination.starts_with(&source) {
        tracing::error!(
            target: "witness_lib::project",
            operation = "save",
            phase = "validate_destination",
            working_path = %source.display(),
            destination = %destination.display(),
            error = "destination is inside the active project working copy",
            "project save rejected because the destination is inside the working copy"
        );
        return Err("choose a .wns file path outside the active project working copy".into());
    }
    if path_is_within(&source, &destination) {
        tracing::error!(
            target: "witness_lib::project",
            operation = "save",
            phase = "validate_destination",
            working_path = %source.display(),
            destination = %destination.display(),
            error = "destination resolves inside the active project working copy",
            "project save rejected because the destination resolves inside the working copy"
        );
        return Err("choose a .wns file path outside the active project working copy".into());
    }
    if !project.temporary {
        let Some(current_archive) = project.archive_path.as_ref() else {
            tracing::error!(
                target: "witness_lib::project",
                operation = "save",
                phase = "validate_destination",
                working_path = %source.display(),
                destination = %destination.display(),
                error = "persistent project has no archive destination",
                "persistent project save rejected because no archive destination is configured"
            );
            return Err("persistent project has no archive destination".into());
        };
        if normalize_path(&destination) != normalize_path(current_archive) {
            tracing::error!(
                target: "witness_lib::project",
                operation = "save",
                phase = "validate_destination",
                working_path = %source.display(),
                current_archive = %current_archive.display(),
                destination = %destination.display(),
                error = "persistent project destination cannot change",
                "persistent project save rejected because the destination differs from its existing archive"
            );
            return Err(
                "persistent projects can only be saved to their existing .wns archive".into(),
            );
        }
    }
    let name = project
        .name
        .clone()
        .unwrap_or_else(|| "Witness Project".into());
    tracing::info!(
        target: "witness_lib::project",
        operation = "save",
        phase = "started",
        working_path = %source.display(),
        destination = %destination.display(),
        project_name = %name,
        "project save started"
    );
    if let Some(parent) = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        if let Err(error) = tokio::fs::create_dir_all(parent).await {
            tracing::error!(
                target: "witness_lib::project",
                operation = "save",
                phase = "create_destination_parent",
                parent = %parent.display(),
                destination = %destination.display(),
                error = %error,
                "project save failed while creating the destination directory"
            );
            return Err(error.to_string());
        }
    }
    tracing::info!(
        target: "witness_lib::project",
        operation = "save",
        phase = "waiting_for_save_lock",
        working_path = %source.display(),
        destination = %destination.display(),
        "project save waiting for the save lock"
    );
    let _save_guard = state.project_save_lock.lock().await;
    tracing::info!(
        target: "witness_lib::project",
        operation = "save",
        phase = "save_lock_acquired",
        working_path = %source.display(),
        destination = %destination.display(),
        "project save acquired the save lock"
    );
    let current_project = state.project.read().await.clone();
    if current_project.current_project_path.as_ref() != Some(&source) {
        tracing::error!(
            target: "witness_lib::project",
            operation = "save",
            phase = "validate_project_state",
            working_path = %source.display(),
            destination = %destination.display(),
            error = "project changed while save was waiting for the save lock",
            "project save rejected because the working copy changed"
        );
        return Err("project changed while save was waiting; please retry".into());
    }
    if !current_project.temporary {
        let Some(current_archive) = current_project.archive_path.as_ref() else {
            return Err("persistent project has no archive destination".into());
        };
        if normalize_path(&destination) != normalize_path(current_archive) {
            tracing::error!(
                target: "witness_lib::project",
                operation = "save",
                phase = "validate_destination",
                working_path = %source.display(),
                current_archive = %current_archive.display(),
                destination = %destination.display(),
                error = "persistent project destination changed while save was waiting",
                "project save rejected because the destination differs from the current archive"
            );
            return Err(
                "persistent projects can only be saved to their existing .wns archive".into(),
            );
        }
    }
    let name = current_project
        .name
        .clone()
        .unwrap_or_else(|| "Witness Project".into());
    let database_guard = state.database.lock().await;
    if let Some(database) = database_guard.as_ref() {
        if let Err(error) = database.checkpoint() {
            tracing::error!(
                target: "witness_lib::project",
                operation = "save",
                phase = "checkpoint",
                working_path = %source.display(),
                destination = %destination.display(),
                error = %error,
                "project save failed while checkpointing the database"
            );
            return Err(error.to_string());
        }
        tracing::info!(
            target: "witness_lib::project",
            operation = "save",
            phase = "checkpoint_completed",
            working_path = %source.display(),
            destination = %destination.display(),
            "project database checkpoint completed"
        );
    } else {
        tracing::warn!(
            target: "witness_lib::project",
            operation = "save",
            phase = "checkpoint_skipped",
            working_path = %source.display(),
            destination = %destination.display(),
            "project save is proceeding without an open database"
        );
    }
    let source_for_task = source.clone();
    let destination_for_task = destination.clone();
    let project_name_for_task = name.clone();
    let archive_result = match tokio::task::spawn_blocking(move || {
        crate::export::export_witness_archive(
            &source_for_task,
            &destination_for_task,
            &project_name_for_task,
        )
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            tracing::error!(
                target: "witness_lib::project",
                operation = "save",
                phase = "archive_task",
                working_path = %source.display(),
                destination = %destination.display(),
                error = %error,
                "project save archive task failed to complete"
            );
            return Err(error.to_string());
        }
    };
    if let Err(error) = archive_result {
        tracing::error!(
            target: "witness_lib::project",
            operation = "save",
            phase = "archive_export",
            working_path = %source.display(),
            destination = %destination.display(),
            error = %error,
            "project save failed while exporting the .wns archive"
        );
        return Err(error.to_string());
    }
    tracing::info!(
        target: "witness_lib::project",
        operation = "save",
        phase = "archive_installed",
        working_path = %source.display(),
        destination = %destination.display(),
        "project archive installed successfully"
    );
    drop(database_guard);
    {
        let mut project = state.project.write().await;
        project.archive_path = Some(destination.clone());
        if project.temporary {
            project.working_path_owned = true;
        }
        project.temporary = false;
        project.dirty = false;
    }
    if let Err(error) =
        crate::settings::record_recent_project(&project.name.clone().unwrap_or(name), &destination)
    {
        tracing::warn!(%error, "failed to update recent projects");
    }
    state.event_bus.publish(Event::Project(ProjectEvent::Saved {
        path: destination.display().to_string(),
    }));
    tracing::info!(
        target: "witness_lib::project",
        operation = "save",
        phase = "completed",
        working_path = %source.display(),
        destination = %destination.display(),
        "project save completed"
    );
    Ok(())
}

#[tauri::command]
pub async fn import_request_file(path: String) -> CommandResult<Vec<u8>> {
    let _operation = logging::OperationGuard::new("command.import_request_file");
    // Cap imports at 10 MiB: check metadata before reading into memory.
    let metadata = tokio::fs::metadata(&path)
        .await
        .map_err(|error| error.to_string())?;
    if metadata.len() > MAX_IMPORT_FILE_BYTES {
        return Err("request file exceeds 10 MiB".into());
    }
    tokio::fs::read(path)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_in_repeater(
    state: State<'_, AppState>,
    raw: Vec<u8>,
    tls: bool,
) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.open_in_repeater");
    state.event_bus.publish(Event::Repeater(RepeaterEvent {
        request_id: uuid::Uuid::new_v4().to_string(),
        status: "openTab".into(),
        raw: Some(raw),
        tls: Some(tls),
    }));
    Ok(())
}

#[tauri::command]
pub async fn get_app_snapshot(state: State<'_, AppState>) -> CommandResult<AppSnapshot> {
    let _operation = logging::OperationGuard::new("command.get_app_snapshot");
    Ok(state.snapshot().await)
}

/// Re-applies interception enablement after a settings change and returns how
/// many pending interceptions were force-forwarded. Pending messages are only
/// flushed when the change can affect how traffic is matched: toggling the
/// "in-scope only" filter while the scope has no in-scope or out-of-scope
/// rules cannot alter filtering, so the pending queue is left untouched.
async fn reconfigure_interception(
    state: &AppState,
    matcher_changed: bool,
    scope_only_changed: bool,
    enabled: bool,
) -> usize {
    let flush_pending = matcher_changed || (scope_only_changed && !state.scope.is_empty().await);
    if !flush_pending {
        state.interceptions.set_enabled(enabled).await;
        return 0;
    }
    let forwarded = state.interceptions.set_enabled(false).await;
    state.interceptions.set_enabled(enabled).await;
    forwarded
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> CommandResult<SettingsState> {
    let _operation = logging::OperationGuard::new("command.update_settings");
    let previous = state.settings.read().await.clone();
    let mut settings = previous.clone();
    patch.apply(&mut settings);
    if !matches!(settings.theme.as_str(), "dark" | "light") {
        return Err("theme must be dark or light".into());
    }
    if !(10..=24).contains(&settings.font_size) {
        return Err("interface font size must be between 10 and 24 pixels".into());
    }
    if !(9..=24).contains(&settings.message_editor_font_size) {
        return Err("message editor font size must be between 9 and 24 pixels".into());
    }
    if !(20..=75).contains(&settings.layout_split_percent) {
        return Err("history split must be between 20 and 75 percent".into());
    }
    if cfg!(target_os = "macos") {
        if !matches!(settings.shortcut_modifier.as_str(), "command" | "control") {
            return Err("shortcut modifier must be command or control".into());
        }
    } else if settings.shortcut_modifier != "control" {
        return Err("shortcut modifier must be control on this platform".into());
    }
    if settings.proxy_port == 0 || settings.proxy_bind_address.trim().is_empty() {
        return Err("proxy bind address and port are required".into());
    }
    if settings.certificate_directory.trim().is_empty() {
        return Err("certificate directory is required".into());
    }
    if !(100..=1_000_000).contains(&settings.history_limit) {
        return Err("history limit must be between 100 and 1,000,000 entries".into());
    }
    if settings.autosave_interval_seconds == 0 || settings.upstream_timeout_seconds == 0 {
        return Err("autosave and timeout values must be greater than zero".into());
    }
    if settings.ai_base_url.len() > 2_048 {
        return Err("AI Base URL is too long".into());
    }
    if !settings.ai_base_url.trim().is_empty() {
        crate::ai::validate_base_url(&settings.ai_base_url).map_err(|error| error.to_string())?;
    }
    if settings.ai_model_name.len() > 256 {
        return Err("AI model name is too long".into());
    }
    if !(1..=600).contains(&settings.ai_request_timeout_seconds) {
        return Err("AI request timeout must be between 1 and 600 seconds".into());
    }
    if !(1..=32).contains(&settings.ai_turn_step_limit) {
        return Err("AI turn step limit must be between 1 and 32".into());
    }
    if settings.ai_api_key_prefix.chars().count() > 3
        || settings.ai_api_key_suffix.chars().count() > 3
    {
        return Err("AI API key display markers cannot exceed three characters".into());
    }
    if !matches!(settings.upstream_proxy.kind.as_str(), "http" | "socks5") {
        return Err("upstream proxy type must be http or socks5".into());
    }
    if settings.upstream_proxy.enabled {
        if settings.upstream_proxy.host.trim().is_empty() {
            return Err("Configure upstream proxy before enabling it".into());
        }
        if settings.upstream_proxy.port == 0 {
            return Err("upstream proxy port must be between 1 and 65535".into());
        }
    }
    if !matches!(
        settings.compression_mode.as_str(),
        "decompressAll" | "decompressText" | "passThrough"
    ) {
        return Err("invalid compression mode".into());
    }
    if !matches!(
        settings.proxy_intercept_mode.as_str(),
        "allRequests" | "allResponses" | "requestsAndResponses" | "none"
    ) {
        return Err("invalid interception mode".into());
    }
    if settings.intercept_content_types.len() > 10
        || settings.intercept_content_types.iter().any(|content_type| {
            !matches!(
                content_type.as_str(),
                "html"
                    | "javascript"
                    | "css"
                    | "json"
                    | "xml"
                    | "images"
                    | "fonts"
                    | "media"
                    | "documents"
                    | "other"
            )
        })
    {
        return Err("invalid interception content-type filter".into());
    }
    validate_interception_rules(&settings.request_interception_rules, "request")?;
    validate_interception_rules(&settings.response_interception_rules, "response")?;
    validate_match_replace_rules(&settings.match_replace_rules)?;
    let network_changed = previous.proxy_port != settings.proxy_port
        || previous.proxy_bind_address != settings.proxy_bind_address;
    let certificate_changed = previous.certificate_directory != settings.certificate_directory;
    if state.proxy.read().await.running && (network_changed || certificate_changed) {
        return Err("stop the proxy before applying network or certificate changes".into());
    }
    if certificate_changed {
        *state.certificate_authority.write().await = None;
        state.proxy.write().await.certificate_status = "missing; generate a CA certificate".into();
    }
    let interception_matcher_changed = previous.proxy_intercept_mode
        != settings.proxy_intercept_mode
        || previous.intercept_content_types != settings.intercept_content_types
        || previous.request_interception_rules != settings.request_interception_rules
        || previous.response_interception_rules != settings.response_interception_rules;
    let scope_only_changed = previous.intercept_in_scope_only != settings.intercept_in_scope_only;
    let forwarded = reconfigure_interception(
        state.inner(),
        interception_matcher_changed,
        scope_only_changed,
        settings.interception_enabled(),
    )
    .await;
    if forwarded > 0 {
        tracing::info!(
            module = "interception",
            forwarded,
            "pending interceptions forwarded because interception was disabled"
        );
    }
    {
        let mut proxy = state.proxy.write().await;
        proxy.port = settings.proxy_port;
        proxy.bind_address = settings.proxy_bind_address.clone();
        proxy.intercepting = settings.interception_enabled();
    }
    state.project.write().await.autosave_interval_seconds =
        settings.autosave_interval_seconds.max(1);
    state
        .history
        .set_capacity(settings.history_limit.max(100))
        .await;
    crate::settings::save_global(&settings).map_err(|error| error.to_string())?;
    *state.settings.write().await = settings.clone();
    tracing::info!(module = "settings", "application settings updated");
    Ok(settings)
}

#[tauri::command]
pub async fn set_ai_api_key(
    state: State<'_, AppState>,
    api_key: String,
) -> CommandResult<AiKeyStatus> {
    let _operation = logging::OperationGuard::new("command.set_ai_api_key");
    if api_key.is_empty() {
        return Err("AI API key cannot be empty".into());
    }
    let (prefix, suffix) = crate::ai::mask_key(&api_key);
    begin_ai_credential_operation(state.inner(), AI_CREDENTIAL_OPERATION_SAVE)?;
    let app_state = (*state).clone();
    let operation_prefix = prefix.clone();
    let operation_suffix = suffix.clone();
    tokio::spawn(async move {
        let result: CommandResult<()> = async {
            wait_for_ai_credentials(&app_state).await?;
            let credentials = app_state.ai_credentials.clone();
            tracing::info!(
                module = "credentials",
                "AI credential snapshot save started"
            );
            tokio::task::spawn_blocking(move || {
                let credentials = credentials
                    .lock()
                    .map_err(|_| "AI credential store lock poisoned".to_string())?;
                credentials
                    .as_ref()
                    .ok_or_else(|| "AI credential store is not initialized".to_string())?
                    .save_key(&api_key)
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| format!("AI credential save task failed: {error}"))??;
            tracing::info!(
                module = "credentials",
                "AI credential snapshot save completed"
            );
            save_ai_key_settings(&app_state, operation_prefix, operation_suffix).await
        }
        .await;
        complete_ai_credential_operation(&app_state, result);
    });
    let settings = state.settings.read().await.clone();
    Ok(ai_key_status(state.inner(), &settings))
}

#[tauri::command]
pub async fn delete_ai_api_key(state: State<'_, AppState>) -> CommandResult<AiKeyStatus> {
    let _operation = logging::OperationGuard::new("command.delete_ai_api_key");
    begin_ai_credential_operation(state.inner(), AI_CREDENTIAL_OPERATION_DELETE)?;
    let app_state = (*state).clone();
    tokio::spawn(async move {
        let result: CommandResult<()> = async {
            delete_ai_credentials(&app_state).await?;
            clear_ai_key_settings(&app_state).await
        }
        .await;
        complete_ai_credential_operation(&app_state, result);
    });
    let settings = state.settings.read().await.clone();
    Ok(ai_key_status(state.inner(), &settings))
}

#[tauri::command]
pub async fn get_ai_api_key_status(state: State<'_, AppState>) -> CommandResult<AiKeyStatus> {
    let _operation = logging::OperationGuard::new("command.get_ai_api_key_status");
    let settings = state.settings.read().await.clone();
    Ok(ai_key_status(state.inner(), &settings))
}

#[tauri::command]
pub async fn get_ai_runtime_status(state: State<'_, AppState>) -> CommandResult<AiRuntimeStatus> {
    let _operation = logging::OperationGuard::new("command.get_ai_runtime_status");
    Ok(ai_runtime_status(state.inner()))
}

#[tauri::command]
pub async fn ai_infer(
    state: State<'_, AppState>,
    request: AiInferenceRequest,
) -> CommandResult<AiInferenceResponse> {
    let _operation = logging::OperationGuard::new("command.ai_infer");
    let request_id = request
        .request_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let cancellation = CancellationToken::new();
    {
        let mut cancellations = state.ai_inference_cancellations.lock().await;
        if cancellations.contains_key(&request_id) {
            return Err("an AI inference request with this ID is already running".into());
        }
        cancellations.insert(request_id.clone(), cancellation.clone());
    }

    let result = async {
        ensure_ai_credentials_idle(state.inner())?;
        tokio::select! {
            _ = cancellation.cancelled() => Err("AI inference cancelled".to_string()),
            result = wait_for_ai_credentials(state.inner()) => result,
        }?;
        ensure_ai_credentials_idle(state.inner())?;
        let settings = state.settings.read().await.clone();
        if !settings.ai_enabled {
            return Err("AI Controller is disabled in Settings".into());
        }
        // `AiCredentialStore::key()` does blocking Stronghold I/O; clone the
        // key on the blocking pool instead of holding a std Mutex guard across
        // `.key()` on the async runtime.
        let credentials = state.ai_credentials.clone();
        let api_key = tokio::task::spawn_blocking(move || {
            let guard = credentials
                .lock()
                .map_err(|_| "AI credential store lock poisoned".to_string())?;
            guard
                .as_ref()
                .ok_or_else(|| "AI credential store is not initialized".to_string())?
                .key()
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| format!("AI credential read task failed: {error}"))??;
        crate::ai::infer(&settings, request, api_key, cancellation)
            .await
            .map_err(|error| error.to_string())
    }
    .await;

    state
        .ai_inference_cancellations
        .lock()
        .await
        .remove(&request_id);
    result
}

#[tauri::command]
pub async fn cancel_ai_infer(state: State<'_, AppState>, request_id: String) -> CommandResult<()> {
    if let Some(cancellation) = state
        .ai_inference_cancellations
        .lock()
        .await
        .get(&request_id)
        .cloned()
    {
        cancellation.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn test_ai_connection(state: State<'_, AppState>) -> CommandResult<AiConnectionResult> {
    let _operation = logging::OperationGuard::new("command.test_ai_connection");
    ensure_ai_credentials_idle(state.inner())?;
    wait_for_ai_credentials(state.inner()).await?;
    ensure_ai_credentials_idle(state.inner())?;
    let settings = state.settings.read().await.clone();
    let credentials = state.ai_credentials.clone();
    let api_key = tokio::task::spawn_blocking(move || {
        let guard = credentials
            .lock()
            .map_err(|_| "AI credential store lock poisoned".to_string())?;
        guard
            .as_ref()
            .ok_or_else(|| "AI credential store is not initialized".to_string())?
            .key()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("AI credential read task failed: {error}"))??;
    crate::ai::test_connection(&settings, api_key)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn pick_project_file() -> CommandResult<Option<String>> {
    let _operation = logging::OperationGuard::new("command.pick_project_file");
    Ok(rfd::AsyncFileDialog::new()
        .set_title("Open a Witness project")
        .add_filter("Witness project", &[PROJECT_EXTENSION])
        .pick_file()
        .await
        .map(|file| file.path().display().to_string()))
}

#[tauri::command]
pub async fn pick_project_save_path() -> CommandResult<Option<String>> {
    let _operation = logging::OperationGuard::new("command.pick_project_save_path");
    Ok(rfd::AsyncFileDialog::new()
        .set_title("Save Witness project")
        .add_filter("Witness project", &[PROJECT_EXTENSION])
        .set_file_name(format!("witness-project.{PROJECT_EXTENSION}"))
        .save_file()
        .await
        .map(|file| file.path().display().to_string()))
}

#[tauri::command]
pub async fn generate_ca_certificate(state: State<'_, AppState>) -> CommandResult<CertificateInfo> {
    let _operation = logging::OperationGuard::new("command.generate_ca_certificate");
    let directory = state.settings.read().await.certificate_directory.clone();
    let certificate_path = PathBuf::from(&directory).join("witness-ca.pem");
    let generated = !certificate_path.exists();
    let authority =
        CertificateAuthority::load_or_create(&directory).map_err(|error| error.to_string())?;
    *state.certificate_authority.write().await = Some(authority);
    let status = "present; install witness-ca.pem in your browser".to_string();
    state.proxy.write().await.certificate_status = status.clone();
    state
        .event_bus
        .publish(Event::Proxy(ProxyEvent::TlsStatus { status }));
    Ok(CertificateInfo {
        certificate_path: certificate_path.display().to_string(),
        generated,
    })
}

#[tauri::command]
pub async fn start_proxy(state: State<'_, AppState>) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.start_proxy");
    let _lifecycle = state.proxy_lifecycle.lock().await;
    let mut guard = state.proxy_task.lock().await;
    if guard.is_some() || state.proxy.read().await.running {
        return Err("proxy is already running".into());
    }
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let app_state = (*state).clone();
    let cleanup_state = app_state.clone();
    let handle = tauri::async_runtime::spawn(async move {
        if let Err(error) = ProxyEngine::run(app_state.clone(), task_cancellation).await {
            app_state
                .event_bus
                .publish(Event::Proxy(crate::event_bus::ProxyEvent::Error {
                    message: error.to_string(),
                }));
            app_state.proxy.write().await.running = false;
        }
        *cleanup_state.proxy_task.lock().await = None;
    });
    *guard = Some(ProxyTask {
        cancellation,
        handle,
    });
    Ok(())
}

#[tauri::command]
pub async fn stop_proxy(state: State<'_, AppState>) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.stop_proxy");
    let _lifecycle = state.proxy_lifecycle.lock().await;
    let restore_interception = state.settings.read().await.interception_enabled();
    state.interceptions.set_enabled(false).await;
    if let Some(task) = state.proxy_task.lock().await.take() {
        task.cancellation.cancel();
        task.handle.abort();
        let _ = task.handle.await;
    }

    let publish_stopped = {
        let mut proxy = state.proxy.write().await;
        let publish_stopped = proxy.running;
        proxy.running = false;
        proxy.connection_count = 0;
        publish_stopped
    };
    if publish_stopped {
        state.event_bus.publish(Event::Proxy(ProxyEvent::Stopped));
    }
    state.interceptions.set_enabled(restore_interception).await;
    Ok(())
}

async fn activate_project(
    state: &AppState,
    info: ProjectInfo,
    temporary: bool,
    created: bool,
) -> CommandResult<()> {
    let _save_guard = state.project_save_lock.lock().await;
    let activation = (|| -> CommandResult<(Database, ScopeSnapshot)> {
        let database = Database::open(&info.path).map_err(|error| error.to_string())?;
        if info.owned_path {
            database
                .rebase_body_paths_to_project()
                .map_err(|error| error.to_string())?;
        }
        if !database
            .integrity_check()
            .map_err(|error| error.to_string())?
        {
            return Err("project database failed its integrity check".into());
        }
        database
            .register_project(&info.name)
            .map_err(|error| error.to_string())?;
        let scope = database.load_scope().map_err(|error| error.to_string())?;
        Ok((database, scope))
    })();
    let (database, scope) = match activation {
        Ok(value) => value,
        Err(error) => {
            if info.owned_path {
                if let Err(cleanup_error) = tokio::fs::remove_dir_all(&info.path).await {
                    if cleanup_error.kind() != std::io::ErrorKind::NotFound {
                        tracing::warn!(
                            target: "witness_lib::project",
                            operation = "working_copy_cleanup",
                            phase = "activation_failure",
                            working_path = %info.path.display(),
                            error = %cleanup_error,
                            "failed to remove the materialized project after activation failed"
                        );
                    }
                }
            }
            return Err(error);
        }
    };
    let has_current_project = state.project.read().await.current_project_path.is_some()
        || state.database.lock().await.is_some();
    if has_current_project {
        if let Err(error) = close_project_locked(state).await {
            if info.owned_path {
                if let Err(cleanup_error) = tokio::fs::remove_dir_all(&info.path).await {
                    if cleanup_error.kind() != std::io::ErrorKind::NotFound {
                        tracing::warn!(
                            target: "witness_lib::project",
                            operation = "working_copy_cleanup",
                            phase = "activation_close_failure",
                            working_path = %info.path.display(),
                            error = %cleanup_error,
                            "failed to remove the replacement project after the current project could not close"
                        );
                    }
                }
            }
            return Err(error);
        }
    }
    let display = info.display_path.display().to_string();
    *state.database.lock().await = Some(database);
    state.scope.replace(scope).await;
    {
        let mut project = state.project.write().await;
        project.current_project_path = Some(info.path.clone());
        project.archive_path = info.archive_path.clone();
        project.name = Some(info.name.clone());
        project.temporary = temporary;
        project.dirty = false;
        project.working_path_owned = info.owned_path;
    }
    state.project_generation.fetch_add(1, Ordering::AcqRel);
    if temporary || info.owned_path {
        state
            .temporary_project_cleanup
            .replace(Some(info.path.clone()));
    }
    if !temporary {
        if let Err(error) = crate::settings::record_recent_project(&info.name, &info.display_path) {
            tracing::warn!(%error, "failed to update recent projects");
        }
    }
    start_history_recorder(state.clone());
    start_autosave(state.clone()).await;
    let event = if created {
        ProjectEvent::Created { path: display }
    } else {
        ProjectEvent::Opened { path: display }
    };
    state.event_bus.publish(Event::Project(event));
    Ok(())
}

#[tauri::command]
pub async fn get_recent_projects() -> CommandResult<Vec<RecentProject>> {
    let _operation = logging::OperationGuard::new("command.get_recent_projects");
    let mut projects =
        crate::settings::load_recent_projects().map_err(|error| error.to_string())?;
    projects.retain(|project| ProjectManager::is_project(&PathBuf::from(&project.path)));
    Ok(projects)
}

#[tauri::command]
pub async fn create_project(
    state: State<'_, AppState>,
    name: String,
    path: String,
) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.create_project");
    let info = ProjectManager::create_project(&name, &PathBuf::from(&path))
        .await
        .map_err(|error| error.to_string())?;
    activate_project(state.inner(), info, false, true).await?;
    tracing::info!(project = %path, "project created");
    Ok(())
}

#[tauri::command]
pub async fn open_project(state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.open_project");
    let cancellation = CancellationToken::new();
    let info = ProjectManager::load(&PathBuf::from(&path), &cancellation)
        .await
        .map_err(|error| error.to_string())?;
    activate_project(state.inner(), info, false, false).await?;
    tracing::info!(project = %path, "project opened");
    Ok(())
}

#[tauri::command]
pub async fn create_temporary_project(state: State<'_, AppState>) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.create_temporary_project");
    let info = ProjectManager::create_temporary_project()
        .await
        .map_err(|error| error.to_string())?;
    let path = info.path.display().to_string();
    activate_project(state.inner(), info, true, true).await?;
    tracing::info!(project = %path, "temporary project created");
    Ok(())
}

#[tauri::command]
pub async fn save_temporary_project(
    state: State<'_, AppState>,
    name: String,
    path: String,
) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.save_temporary_project");
    let _save_guard = state.project_save_lock.lock().await;
    let project = state.project.read().await.clone();
    if !project.temporary {
        tracing::error!(
            target: "witness_lib::project",
            operation = "temporary_save",
            phase = "validate_project",
            error = "the current project is not a temporary session",
            "temporary project save rejected for a permanent project"
        );
        return Err("the current project is not a temporary session".into());
    }
    let source = match project.current_project_path {
        Some(source) => source,
        None => {
            tracing::error!(
                target: "witness_lib::project",
                operation = "temporary_save",
                phase = "resolve_working_copy",
                error = "no temporary project is open",
                "temporary project save rejected because the working copy is missing"
            );
            return Err("no temporary project is open".into());
        }
    };
    if let Some(cancellation) = state.project_autosave_cancellation.lock().await.take() {
        cancellation.cancel();
        tracing::info!(
            target: "witness_lib::project",
            operation = "temporary_save",
            phase = "autosave_cancelled",
            working_path = %source.display(),
            "temporary project save cancelled the active autosave loop"
        );
    }
    let name = name.trim().to_string();
    let destination = PathBuf::from(path.trim());
    tracing::info!(
        target: "witness_lib::project",
        operation = "temporary_save",
        phase = "started",
        working_path = %source.display(),
        destination = %destination.display(),
        project_name = %name,
        "temporary project save started"
    );
    let result = async {
        if name.is_empty() {
            return Err("project name is required".into());
        }
        if !destination
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case(PROJECT_EXTENSION))
        {
            return Err("project files must use the .wns extension".into());
        }
        if destination.is_dir() || path_is_within(&source, &destination) {
            return Err("choose a .wns file path outside the temporary session".into());
        }
        if let Some(parent) = destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| error.to_string())?;
        }
        ProjectManager::rename_materialized_project(&source, &name)
            .await
            .map_err(|error| error.to_string())?;
        let database_guard = state.database.lock().await;
        if let Some(database) = database_guard.as_ref() {
            database
                .register_project(&name)
                .map_err(|error| error.to_string())?;
            database.checkpoint().map_err(|error| error.to_string())?;
        }
        let source_for_task = source.clone();
        let destination_for_task = destination.clone();
        let project_name_for_task = name.clone();
        tokio::task::spawn_blocking(move || {
            crate::export::export_witness_archive(
                &source_for_task,
                &destination_for_task,
                &project_name_for_task,
            )
        })
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
        drop(database_guard);
        {
            let mut project = state.project.write().await;
            project.archive_path = Some(destination.clone());
            project.name = Some(name.clone());
            project.temporary = false;
            project.working_path_owned = true;
            project.dirty = false;
        }
        if let Err(error) = crate::settings::record_recent_project(&name, &destination) {
            tracing::warn!(%error, "failed to update recent projects");
        }
        start_autosave((*state).clone()).await;
        state.event_bus.publish(Event::Project(ProjectEvent::Saved {
            path: destination.display().to_string(),
        }));
        tracing::info!(project = %destination.display(), "temporary project saved");
        Ok(())
    }
    .await;
    match &result {
        Ok(()) => tracing::info!(
            target: "witness_lib::project",
            operation = "temporary_save",
            phase = "completed",
            working_path = %source.display(),
            destination = %destination.display(),
            "temporary project save completed"
        ),
        Err(error) => tracing::error!(
            target: "witness_lib::project",
            operation = "temporary_save",
            phase = "failed",
            working_path = %source.display(),
            destination = %destination.display(),
            error = %error,
            "temporary project save failed"
        ),
    }
    if result.is_err() {
        start_autosave((*state).clone()).await;
    }
    result
}

async fn close_project_locked(state: &AppState) -> CommandResult<()> {
    tracing::info!(
        target: "witness_lib::project",
        operation = "close",
        phase = "started",
        "project close started"
    );
    let autosave_was_active =
        if let Some(cancellation) = state.project_autosave_cancellation.lock().await.take() {
            cancellation.cancel();
            tracing::info!(
                target: "witness_lib::project",
                operation = "close",
                phase = "autosave_cancelled",
                "project autosave cancellation requested"
            );
            true
        } else {
            tracing::info!(
                target: "witness_lib::project",
                operation = "close",
                phase = "autosave_not_running",
                "project close found no active autosave loop"
            );
            false
        };
    let project = state.project.read().await.clone();
    let path = project.current_project_path.clone();
    tracing::info!(
        target: "witness_lib::project",
        operation = "close",
        phase = "state_loaded",
        working_path = ?project.current_project_path,
        archive_path = ?project.archive_path,
        temporary = project.temporary,
        working_path_owned = project.working_path_owned,
        dirty = project.dirty,
        "project close loaded current project state"
    );
    let mut database_guard = state.database.lock().await;
    if path.is_none() && project.archive_path.is_none() && database_guard.is_none() {
        tracing::info!(
            target: "witness_lib::project",
            operation = "close",
            phase = "no_project",
            "project close is a no-op because no project is open"
        );
        drop(database_guard);
        return Ok(());
    }
    if let Some(database) = database_guard.as_ref() {
        if let Err(error) = database.checkpoint() {
            tracing::error!(
                target: "witness_lib::project",
                operation = "close",
                phase = "checkpoint",
                working_path = ?path,
                archive_path = ?project.archive_path,
                error = %error,
                "project close failed while checkpointing the database"
            );
            drop(database_guard);
            if autosave_was_active {
                start_autosave((*state).clone()).await;
            }
            return Err(error.to_string());
        }
        tracing::info!(
            target: "witness_lib::project",
            operation = "close",
            phase = "checkpoint_completed",
            working_path = ?path,
            "project database checkpoint completed before close"
        );
    } else {
        tracing::warn!(
            target: "witness_lib::project",
            operation = "close",
            phase = "checkpoint_skipped",
            working_path = ?path,
            "project close is proceeding without an open database"
        );
    }

    if (project.temporary || project.working_path_owned) && path.is_none() {
        tracing::error!(
            target: "witness_lib::project",
            operation = "close",
            phase = "working_copy_cleanup",
            error = "working copy path is missing",
            "project close cannot safely clean up the owned working copy"
        );
        drop(database_guard);
        if autosave_was_active {
            start_autosave((*state).clone()).await;
        }
        return Err(
            "project close could not clean up the working copy because its path is missing".into(),
        );
    }

    if !project.temporary {
        let source = match path.as_ref() {
            Some(source) => source,
            None => {
                drop(database_guard);
                if autosave_was_active {
                    start_autosave((*state).clone()).await;
                }
                return Err(
                    "project close could not export the archive because the working path is missing"
                        .into(),
                );
            }
        };
        let destination = match project.archive_path.as_ref() {
            Some(destination) => destination,
            None => {
                drop(database_guard);
                if autosave_was_active {
                    start_autosave((*state).clone()).await;
                }
                return Err(
                    "project close could not export the archive because its destination is missing"
                        .into(),
                );
            }
        };
        let source_for_task = source.clone();
        let destination_for_task = destination.clone();
        let project_name = project
            .name
            .clone()
            .unwrap_or_else(|| "Witness Project".into());
        let export_result = match tokio::task::spawn_blocking(move || {
            crate::export::export_witness_archive(
                &source_for_task,
                &destination_for_task,
                &project_name,
            )
        })
        .await
        {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        if let Err(error) = export_result {
            tracing::error!(
                target: "witness_lib::project",
                operation = "close",
                phase = "archive_export",
                working_path = %source.display(),
                archive_path = %destination.display(),
                error = %error,
                "project close failed while exporting the persistent archive"
            );
            drop(database_guard);
            if autosave_was_active {
                start_autosave((*state).clone()).await;
            }
            return Err(error);
        }
        tracing::info!(
            target: "witness_lib::project",
            operation = "close",
            phase = "archive_export_completed",
            working_path = %source.display(),
            archive_path = %destination.display(),
            "project close exported the persistent archive before cleanup"
        );
    }

    let database_was_open = database_guard.is_some();
    *database_guard = None;
    drop(database_guard);
    tracing::info!(
        target: "witness_lib::project",
        operation = "close",
        phase = "database_released",
        working_path = ?path,
        "project database released"
    );
    if project.temporary || project.working_path_owned {
        let path = path.as_ref().expect("owned project path validated above");
        tracing::info!(
            target: "witness_lib::project",
            operation = "close",
            phase = "working_copy_cleanup_started",
            working_path = %path.display(),
            "project close started working-copy cleanup"
        );
        if let Err(error) = ProjectManager::delete_materialized_project(path).await {
            tracing::error!(
                target: "witness_lib::project",
                operation = "close",
                phase = "working_copy_cleanup",
                working_path = %path.display(),
                error = %error,
                "project close could not remove the materialized working copy"
            );
            let cleanup_message = error.to_string();
            if let Some(restore_error) =
                restore_database_after_cleanup_failure(state, path, database_was_open, "close")
                    .await
            {
                if autosave_was_active {
                    start_autosave((*state).clone()).await;
                }
                return Err(format!(
                    "{cleanup_message}; unable to restore the project database: {restore_error}"
                ));
            }
            if autosave_was_active {
                start_autosave((*state).clone()).await;
            }
            return Err(cleanup_message);
        }
        tracing::info!(
            target: "witness_lib::project",
            operation = "close",
            phase = "working_copy_removed",
            working_path = %path.display(),
            "project close removed the materialized working copy"
        );
        state.temporary_project_cleanup.take();
    } else {
        tracing::info!(
            target: "witness_lib::project",
            operation = "close",
            phase = "working_copy_cleanup_skipped",
            working_path = ?path,
            "project close found no owned working copy to remove"
        );
    }
    state.scope.replace(Default::default()).await;
    *state.project.write().await = crate::state::ProjectState::default();
    state
        .event_bus
        .publish(Event::Project(ProjectEvent::Closed));
    tracing::info!(
        target: "witness_lib::project",
        operation = "close",
        phase = "completed",
        working_path = ?path,
        archive_path = ?project.archive_path,
        "project close completed"
    );
    Ok(())
}

#[tauri::command]
pub async fn close_project(state: State<'_, AppState>) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.close_project");
    let _save_guard = state.project_save_lock.lock().await;
    let result = close_project_locked(state.inner()).await;
    if result.is_ok() {
        state.project_generation.fetch_add(1, Ordering::AcqRel);
    }
    result
}

#[tauri::command]
pub async fn delete_project(state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.delete_project");
    let path = PathBuf::from(path);
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(PROJECT_EXTENSION))
    {
        return Err("project deletion requires an .wns project file".into());
    }
    let _save_guard = state.project_save_lock.lock().await;
    let current = state.project.read().await.clone();
    let is_current = current.archive_path.as_ref() == Some(&path);
    if is_current {
        let autosave_was_active =
            if let Some(cancellation) = state.project_autosave_cancellation.lock().await.take() {
                cancellation.cancel();
                true
            } else {
                false
            };
        let working_path = current
            .working_path_owned
            .then(|| current.current_project_path.clone())
            .flatten()
            .filter(|working| working != &path);
        let database_was_open;
        {
            let mut database = state.database.lock().await;
            if let Some(open_database) = database.as_ref() {
                if let Err(error) = open_database.checkpoint() {
                    drop(database);
                    if autosave_was_active {
                        start_autosave((*state).clone()).await;
                    }
                    return Err(error.to_string());
                }
            }
            database_was_open = database.is_some();
            *database = None;
        }
        let delete_result = ProjectManager::delete_project(&path).await;
        if let Err(error) = delete_result {
            let delete_message = error.to_string();
            if let Some(working_path) = working_path.as_ref() {
                if let Some(restore_error) = restore_database_after_cleanup_failure(
                    state.inner(),
                    working_path,
                    database_was_open,
                    "delete",
                )
                .await
                {
                    if autosave_was_active {
                        start_autosave((*state).clone()).await;
                    }
                    return Err(format!(
                        "{delete_message}; unable to restore the project database: {restore_error}"
                    ));
                }
            }
            if autosave_was_active {
                start_autosave((*state).clone()).await;
            }
            return Err(delete_message);
        }
        if let Some(working_path) = working_path.as_ref() {
            if let Err(error) = ProjectManager::delete_materialized_project(working_path).await {
                tracing::error!(
                    target: "witness_lib::project",
                    operation = "delete",
                    phase = "working_copy_cleanup",
                    working_path = %working_path.display(),
                    error = %error,
                    "project deletion could not remove the materialized working copy"
                );
                let cleanup_message = error.to_string();
                if let Some(restore_error) = restore_database_after_cleanup_failure(
                    state.inner(),
                    working_path,
                    database_was_open,
                    "delete",
                )
                .await
                {
                    if autosave_was_active {
                        start_autosave((*state).clone()).await;
                    }
                    return Err(format!(
                        "{cleanup_message}; unable to restore the project database: {restore_error}"
                    ));
                }
                if autosave_was_active {
                    start_autosave((*state).clone()).await;
                }
                return Err(cleanup_message);
            }
        }
        state.temporary_project_cleanup.take();
        state.scope.replace(Default::default()).await;
        *state.project.write().await = crate::state::ProjectState::default();
        state.project_generation.fetch_add(1, Ordering::AcqRel);
    } else {
        ProjectManager::delete_project(&path)
            .await
            .map_err(|error| error.to_string())?;
    }
    state
        .event_bus
        .publish(Event::Project(ProjectEvent::Deleted {
            path: path.display().to_string(),
        }));
    Ok(())
}

#[tauri::command]
pub async fn query_history(
    state: State<'_, AppState>,
    filter: Option<HistoryFilter>,
    offset: usize,
    limit: usize,
) -> CommandResult<Vec<HistoryEntry>> {
    let _operation = logging::OperationGuard::new("command.query_history");
    let cancellation = CancellationToken::new();
    if let Some(previous) = state
        .history_query_cancellation
        .lock()
        .await
        .replace(cancellation.clone())
    {
        previous.cancel();
    }
    if cancellation.is_cancelled() {
        return Err("history query cancelled".into());
    }
    let project_path = state
        .project
        .read()
        .await
        .current_project_path
        .clone()
        .ok_or_else(|| "open or create a project to view history".to_string())?;
    let response = state
        .task_queue
        .enqueue(
            Task::QueryHistory {
                project_path,
                filter: filter.unwrap_or_default(),
                offset,
                limit,
            },
            cancellation.clone(),
        )
        .await
        .map_err(|error| error.to_string())?
        .await
        .map_err(|_| "history worker stopped".to_string())?
        .map_err(|error| error.to_string())?;
    if cancellation.is_cancelled() {
        Err("history query cancelled".into())
    } else {
        match response {
            TaskResult::HistoryEntries(entries) => Ok(entries),
            _ => Err("history worker returned an unexpected result".into()),
        }
    }
}

#[tauri::command]
pub async fn get_history_detail(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<Option<HistoryDetail>> {
    let _operation = logging::OperationGuard::new("command.get_history_detail");
    // `get_detail` does blocking SQLite + `std::fs::read` I/O; run it on the
    // blocking pool so the async runtime never stalls.
    let database = state.database.clone();
    tokio::task::spawn_blocking(move || {
        let guard = database.blocking_lock();
        let db = guard.as_ref().ok_or_else(|| NO_PROJECT_MSG.to_string())?;
        db.get_detail(&id).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn delete_history_entry(state: State<'_, AppState>, id: String) -> CommandResult<bool> {
    let _operation = logging::OperationGuard::new("command.delete_history_entry");
    let _save_guard = state.project_save_lock.lock().await;
    let deleted = state
        .database
        .lock()
        .await
        .as_mut()
        .ok_or_else(|| NO_PROJECT_MSG.to_string())?
        .delete_exchange(&id)
        .map_err(|error| error.to_string())?;
    if deleted {
        mark_dirty(state.inner()).await;
        state
            .event_bus
            .publish(Event::History(crate::event_bus::HistoryEvent::Deleted {
                id,
            }));
    }
    Ok(deleted)
}

#[tauri::command]
pub async fn clear_history(state: State<'_, AppState>) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.clear_history");
    let _save_guard = state.project_save_lock.lock().await;
    state
        .database
        .lock()
        .await
        .as_mut()
        .ok_or_else(|| NO_PROJECT_MSG.to_string())?
        .clear_history()
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    state
        .event_bus
        .publish(Event::History(crate::event_bus::HistoryEvent::Cleared));
    Ok(())
}

#[tauri::command]
pub async fn create_fuzz_scan(
    state: State<'_, AppState>,
    id: String,
    source_tab_id: i64,
    name: String,
    started_at: String,
) -> CommandResult<FuzzScanRecord> {
    let _operation = logging::OperationGuard::new("command.create_fuzz_scan");
    let _save_guard = state.project_save_lock.lock().await;
    let record = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .create_fuzz_scan(&id, source_tab_id, &name, &started_at)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(record)
}

#[tauri::command]
pub async fn complete_fuzz_scan(
    state: State<'_, AppState>,
    id: String,
    completed_at: String,
) -> CommandResult<FuzzScanRecord> {
    let _operation = logging::OperationGuard::new("command.complete_fuzz_scan");
    let _save_guard = state.project_save_lock.lock().await;
    let record = require_db(state.inner())
        .await?
        .as_ref()
        .expect(NO_PROJECT_MSG)
        .complete_fuzz_scan(&id, &completed_at)
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(record)
}

#[tauri::command]
pub async fn resolve_interception(
    state: State<'_, AppState>,
    id: String,
    action: String,
    raw: Option<Vec<u8>>,
) -> CommandResult<bool> {
    let _operation = logging::OperationGuard::new("command.resolve_interception");
    let resolution = match action.as_str() {
        "forward" => InterceptionResolution::Forward,
        "drop" => InterceptionResolution::Drop,
        "modify" => InterceptionResolution::Modify(
            raw.ok_or_else(|| "modified interception requires raw HTTP bytes".to_string())?,
        ),
        _ => return Err(format!("unknown interception action: {action}")),
    };
    Ok(state.interceptions.resolve(&id, resolution).await)
}

#[tauri::command]
pub async fn get_scope(state: State<'_, AppState>) -> CommandResult<ScopeSnapshot> {
    let _operation = logging::OperationGuard::new("command.get_scope");
    Ok(state.scope.snapshot().await)
}

#[tauri::command]
pub async fn add_scope_entry(
    state: State<'_, AppState>,
    pattern: String,
    is_regex: bool,
    include_subdomains: bool,
    is_in_scope: bool,
) -> CommandResult<ScopeEntry> {
    let _operation = logging::OperationGuard::new("command.add_scope_entry");
    let _save_guard = state.project_save_lock.lock().await;
    validate_scope(&pattern, is_regex).map_err(|error| error.to_string())?;
    let entry = state
        .database
        .lock()
        .await
        .as_ref()
        .ok_or_else(|| "open a project before changing scope".to_string())?
        .add_scope(&pattern, is_regex, include_subdomains, is_in_scope)
        .map_err(|error| error.to_string())?;
    state
        .scope
        .add(entry.clone(), &state.event_bus)
        .await
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(entry)
}

#[tauri::command]
pub async fn remove_scope_entry(state: State<'_, AppState>, id: i64) -> CommandResult<bool> {
    let _operation = logging::OperationGuard::new("command.remove_scope_entry");
    let _save_guard = state.project_save_lock.lock().await;
    let removed = state
        .database
        .lock()
        .await
        .as_ref()
        .ok_or_else(|| "open a project before changing scope".to_string())?
        .remove_scope(id)
        .map_err(|error| error.to_string())?;
    if removed {
        state.scope.remove(id, &state.event_bus).await;
        mark_dirty(state.inner()).await;
        tracing::info!(scope_id = id, "scope entry removed");
    }
    Ok(removed)
}

#[tauri::command]
pub async fn update_scope_entry(
    state: State<'_, AppState>,
    id: i64,
    pattern: String,
    is_regex: bool,
    include_subdomains: bool,
    is_in_scope: bool,
) -> CommandResult<ScopeEntry> {
    let _operation = logging::OperationGuard::new("command.update_scope_entry");
    let _save_guard = state.project_save_lock.lock().await;
    let pattern = pattern.trim().to_string();
    validate_scope(&pattern, is_regex).map_err(|error| error.to_string())?;
    let entry = state
        .database
        .lock()
        .await
        .as_ref()
        .ok_or_else(|| "open a project before changing scope".to_string())?
        .update_scope(id, &pattern, is_regex, include_subdomains, is_in_scope)
        .map_err(|error| error.to_string())?;
    state
        .scope
        .update(entry.clone(), &state.event_bus)
        .await
        .map_err(|error| error.to_string())?;
    mark_dirty(state.inner()).await;
    Ok(entry)
}

#[tauri::command]
pub async fn import_scope_entries(
    state: State<'_, AppState>,
    entries: Vec<String>,
) -> CommandResult<ScopeSnapshot> {
    let _operation = logging::OperationGuard::new("command.import_scope_entries");
    let _save_guard = state.project_save_lock.lock().await;
    let mut changed = false;
    for pattern in entries
        .into_iter()
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
    {
        validate_scope(&pattern, false).map_err(|error| error.to_string())?;
        let entry = state
            .database
            .lock()
            .await
            .as_ref()
            .ok_or_else(|| "open a project before changing scope".to_string())?
            .add_scope(&pattern, false, true, true)
            .map_err(|error| error.to_string())?;
        let snapshot = state.scope.snapshot().await;
        if !snapshot
            .entries
            .iter()
            .any(|existing| existing.id == entry.id)
        {
            state
                .scope
                .add(entry, &state.event_bus)
                .await
                .map_err(|error| error.to_string())?;
            changed = true;
        }
    }
    if changed {
        mark_dirty(state.inner()).await;
    }
    Ok(state.scope.snapshot().await)
}

#[tauri::command]
pub async fn send_repeater_request(
    state: State<'_, AppState>,
    request_id: String,
    raw: Vec<u8>,
    tls: bool,
    injection: Option<IdentityInjectionDescriptor>,
) -> CommandResult<RepeaterResponse> {
    let _operation = logging::OperationGuard::new("command.send_repeater_request");
    if raw.len() > crate::http::MAX_MESSAGE_SIZE {
        return Err("repeater request exceeds 100 MiB".into());
    }
    let cancellation = CancellationToken::new();
    let operation_id = Uuid::new_v4().to_string();
    if let Some(previous) = state.repeater_cancellations.lock().await.insert(
        request_id.clone(),
        crate::state::RepeaterCancellation {
            operation_id: operation_id.clone(),
            token: cancellation.clone(),
        },
    ) {
        previous.token.cancel();
    }
    let upstream_proxy = state.upstream_proxy().await;
    let upstream_timeout = state.upstream_timeout().await;
    let compression_mode = state.settings.read().await.compression_mode.clone();
    let result = match state
        .task_queue
        .enqueue(
            Task::SendRepeater {
                request_id: request_id.clone(),
                raw,
                tls,
                injection,
                upstream_proxy,
                compression_mode,
                timeout: upstream_timeout,
            },
            cancellation,
        )
        .await
    {
        Ok(response) => match response.await {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(_) => Err("repeater worker stopped".to_string()),
        },
        Err(error) => Err(error.to_string()),
    };
    let mut cancellations = state.repeater_cancellations.lock().await;
    if cancellations
        .get(&request_id)
        .is_some_and(|entry| entry.operation_id == operation_id)
    {
        cancellations.remove(&request_id);
    }
    drop(cancellations);
    match result? {
        TaskResult::RepeaterResponse(response) => Ok(response),
        _ => Err("repeater worker returned an unexpected result".into()),
    }
}

#[tauri::command]
pub async fn cancel_repeater_request(
    state: State<'_, AppState>,
    request_id: String,
) -> CommandResult<bool> {
    let _operation = logging::OperationGuard::new("command.cancel_repeater_request");
    let cancellation = state
        .repeater_cancellations
        .lock()
        .await
        .remove(&request_id);
    if let Some(cancellation) = cancellation {
        cancellation.token.cancel();
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub async fn export_ca_certificate(
    state: State<'_, AppState>,
    destination: String,
) -> CommandResult<()> {
    let _operation = logging::OperationGuard::new("command.export_ca_certificate");
    let configured = state.certificate_authority.read().await.clone();
    let directory = state.settings.read().await.certificate_directory.clone();
    configured
        .map(Ok)
        .unwrap_or_else(|| CertificateAuthority::load_or_create(directory))
        .and_then(|authority| authority.export_certificate(&PathBuf::from(destination)))
        .map_err(|error| error.to_string())
}

pub fn forward_events_to_ui(state: AppState, app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut receiver = state.event_bus.subscribe(None);
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    // Forward events are consumed by the history recorder, not the UI. Avoid
                    // serializing their full request/response bodies across the Tauri bridge.
                    if matches!(&event, Event::Forward(_)) {
                        continue;
                    }
                    if let Err(error) = app.emit("witness-event", &event) {
                        if !matches!(event, Event::Log(_)) {
                            tracing::warn!(%error, "failed to emit event to UI");
                        }
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    // A lost interception event would otherwise leave its browser connection
                    // waiting forever. Forward all pending messages, then resume interception.
                    let enabled = state.settings.read().await.interception_enabled();
                    let forwarded = state.interceptions.set_enabled(false).await;
                    state.interceptions.set_enabled(enabled).await;
                    tracing::warn!(
                        skipped,
                        forwarded,
                        "UI event relay lagged; pending interceptions forwarded"
                    );
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_patch_changes_only_the_requested_field() {
        let patch: SettingsPatch = serde_json::from_value(serde_json::json!({
            "theme": "light",
            "messageEditorFontSize": 17,
            "showLogsTab": true
        }))
        .unwrap();
        let mut settings = SettingsState::default();
        let original_port = settings.proxy_port;
        let original_interface_font_size = settings.font_size;

        patch.apply(&mut settings);

        assert_eq!(settings.theme, "light");
        assert_eq!(settings.message_editor_font_size, 17);
        assert!(settings.show_logs_tab);
        assert_eq!(settings.font_size, original_interface_font_size);
        assert_eq!(settings.proxy_port, original_port);
    }

    #[test]
    fn archive_destination_cannot_be_inside_project_folder() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("project");
        std::fs::create_dir_all(&source).unwrap();
        let nested = source.join("exports").join("copy.wns");
        assert!(path_is_within(&source, &nested));
        assert!(!path_is_within(&source, &root.path().join("copy.wns")));
    }

    #[tokio::test]
    async fn credential_readiness_is_retained_when_startup_has_no_waiter() {
        let (sender, _) = tokio::sync::watch::channel::<Option<Result<(), String>>>(None);
        sender.send_replace(Some(Ok(())));

        let receiver = sender.subscribe();
        assert_eq!(receiver.borrow().clone(), Some(Ok(())));
    }

    async fn spawn_pending_interception(state: &AppState) {
        state.interceptions.set_enabled(true).await;
        let manager = state.interceptions.clone();
        let bus = state.event_bus.clone();
        let task = tokio::spawn(async move {
            manager
                .intercept_request(
                    b"pending request".to_vec(),
                    "https://example.test/".into(),
                    &bus,
                )
                .await
        });
        for _ in 0..200 {
            if state.interceptions.pending_count().await > 0 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        assert_eq!(state.interceptions.pending_count().await, 1);
        task.abort();
    }

    #[tokio::test]
    async fn toggling_in_scope_only_with_empty_scope_keeps_pending_queue() {
        let state = AppState::new();
        assert!(state.scope.is_empty().await);

        spawn_pending_interception(&state).await;

        reconfigure_interception(&state, false, true, true).await;

        assert_eq!(
            state.interceptions.pending_count().await,
            1,
            "empty-scope in-scope-only toggle must not flush pending interceptions"
        );
    }

    #[tokio::test]
    async fn toggling_in_scope_only_with_rules_still_flushes_pending_queue() {
        let state = AppState::new();
        let bus = state.event_bus.clone();
        state
            .scope
            .add(
                crate::scope::ScopeEntry {
                    id: 1,
                    pattern: "example.test".into(),
                    is_regex: false,
                    include_subdomains: false,
                    is_in_scope: true,
                },
                &bus,
            )
            .await
            .unwrap();

        spawn_pending_interception(&state).await;

        let forwarded = reconfigure_interception(&state, false, true, true).await;

        assert_eq!(forwarded, 1);
        assert_eq!(state.interceptions.pending_count().await, 0);
    }

    #[tokio::test]
    async fn matcher_changes_always_flush_pending_queue() {
        let state = AppState::new();

        spawn_pending_interception(&state).await;

        let forwarded = reconfigure_interception(&state, true, false, true).await;

        assert_eq!(forwarded, 1);
        assert_eq!(state.interceptions.pending_count().await, 0);
    }

    #[test]
    fn no_project_message_is_unified() {
        assert_eq!(NO_PROJECT_MSG, "No project is open");
    }

    #[tokio::test]
    async fn db_and_working_copy_helpers_reject_when_no_project_open() {
        let state = AppState::new();
        assert!(state.database.lock().await.is_none());
        let db_err = require_db(&state).await.err().unwrap();
        assert_eq!(db_err, NO_PROJECT_MSG);
        let wns_err = ensure_wns(&state).await.err().unwrap();
        assert_eq!(wns_err, NO_PROJECT_MSG);
    }

    #[tokio::test]
    async fn mark_dirty_helper_sets_project_dirty() {
        let state = AppState::new();
        assert!(!state.project.read().await.dirty);
        mark_dirty(&state).await;
        assert!(state.project.read().await.dirty);
    }

    #[test]
    fn ipc_caps_are_enforced() {
        assert_eq!(MAX_IMPORT_FILE_BYTES, 10 * 1024 * 1024);
        assert_eq!(MAX_DECODER_INPUT_BYTES, 5 * 1024 * 1024);
        assert_eq!(MAX_COMPARE_INPUT_BYTES, 1 * 1024 * 1024);
        assert!("x".repeat(MAX_DECODER_INPUT_BYTES + 1).len() > MAX_DECODER_INPUT_BYTES);
        assert!("x".repeat(MAX_COMPARE_INPUT_BYTES + 1).len() > MAX_COMPARE_INPUT_BYTES);
    }

    #[test]
    fn overlong_match_replace_regex_is_rejected() {
        let long = "a".repeat(crate::proxy::match_replace::MAX_REGEX_LEN + 1);
        let rule = MatchReplaceRule {
            id: "r".into(),
            enabled: true,
            location: "request".into(),
            rule_type: "requestBody".into(),
            match_str: long,
            replace: String::new(),
            is_regex: true,
        };
        assert!(validate_match_replace_rules(&[rule]).is_err());
    }
}
