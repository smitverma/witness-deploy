use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::logging;
use crate::state::SettingsState;

const AI_CREDENTIAL_CLIENT: &[u8] = b"witness-ai";
const AI_CREDENTIAL_KEY: &[u8] = b"provider-api-key";
const AI_CREDENTIAL_PASSWORD: &str = "witness-ai-credentials-v1";

/// Shared HTTP client reused across inference calls (connection pooling).
/// A single 60s upper-bound timeout is baked in; per-request timeouts from
/// settings are enforced via `tokio::time::timeout` around the send so one
/// shared client serves all timeout configurations.
static AI_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn ai_client() -> &'static reqwest::Client {
    AI_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .expect("AI HTTP client must build")
    })
}

/// The credential store is kept in the native process so the webview never
/// receives a saved provider key. It uses the Stronghold plugin's encrypted
/// store and only exposes the key to the native inference request.
pub struct AiCredentialStore {
    stronghold: tauri_plugin_stronghold::stronghold::Stronghold,
    snapshot_path: PathBuf,
}

impl AiCredentialStore {
    pub fn open(snapshot_path: &Path, salt_path: &Path) -> Result<Self> {
        let password =
            tauri_plugin_stronghold::kdf::KeyDerivation::argon2(AI_CREDENTIAL_PASSWORD, salt_path);
        let stronghold =
            tauri_plugin_stronghold::stronghold::Stronghold::new(snapshot_path, password)
                .map_err(|error| anyhow!("could not open AI credential store: {error}"))?;
        if stronghold.load_client(AI_CREDENTIAL_CLIENT).is_err() {
            stronghold
                .create_client(AI_CREDENTIAL_CLIENT)
                .map_err(|error| anyhow!("could not create AI credential store: {error}"))?;
        }
        Ok(Self {
            stronghold,
            snapshot_path: snapshot_path.to_path_buf(),
        })
    }

    fn store(&self) -> Result<iota_stronghold::Store> {
        self.stronghold
            .get_client(AI_CREDENTIAL_CLIENT)
            .map(|client| client.store())
            .map_err(|error| anyhow!("could not access AI credential store: {error}"))
    }

    pub fn save_key(&self, key: &str) -> Result<()> {
        if key.is_empty() {
            return Err(anyhow!("AI API key cannot be empty"));
        }
        let store = self.store()?;
        let previous = store
            .get(AI_CREDENTIAL_KEY)
            .map_err(|error| anyhow!("could not read existing AI API key: {error}"))?;
        store
            .insert(AI_CREDENTIAL_KEY.to_vec(), key.as_bytes().to_vec(), None)
            .map_err(|error| anyhow!("could not save AI API key: {error}"))?;
        if let Err(error) = self.stronghold.save() {
            // Stronghold writes through a sibling temporary file, so a failed
            // save leaves the previous snapshot intact. Restore the in-memory
            // value as well so a failed replacement cannot be used by inference.
            match previous {
                Some(previous) => {
                    store
                        .insert(AI_CREDENTIAL_KEY.to_vec(), previous, None)
                        .map_err(|restore_error| {
                            anyhow!(
                                "could not commit AI API key ({error}) or restore the previous key in memory ({restore_error})"
                            )
                        })?;
                }
                None => {
                    store.delete(AI_CREDENTIAL_KEY).map_err(|restore_error| {
                        anyhow!(
                            "could not commit AI API key ({error}) or restore the empty key state in memory ({restore_error})"
                        )
                    })?;
                }
            }
            return Err(anyhow!("could not commit AI API key: {error}"));
        }
        Ok(())
    }

    pub fn key(&self) -> Result<String> {
        // Blocking Stronghold I/O: callers on the async runtime must wrap in
        // `spawn_blocking` (see `ui_bridge::ai_infer`) to avoid stalling the executor.
        let Some(value) = self
            .store()?
            .get(AI_CREDENTIAL_KEY)
            .map_err(|error| anyhow!("could not read AI API key: {error}"))?
        else {
            return Ok(String::new());
        };
        String::from_utf8(value).map_err(|_| anyhow!("stored AI API key is invalid"))
    }

    pub fn delete_key(&self) -> Result<()> {
        self.store()?
            .delete(AI_CREDENTIAL_KEY)
            .map_err(|error| anyhow!("could not delete AI API key: {error}"))?;
        match std::fs::remove_file(&self.snapshot_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(remove_error) => self.stronghold.save().map_err(|save_error| {
                anyhow!(
                    "could not remove AI credential snapshot ({remove_error}) or commit deletion ({save_error})"
                )
            }),
        }
    }
}

pub fn mask_key(key: &str) -> (String, String) {
    let characters: Vec<char> = key.chars().collect();
    if characters.len() < 7 {
        return ("•••".into(), "•••".into());
    }
    (
        characters.iter().take(3).collect(),
        characters
            .iter()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiChatMessage {
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "tool_calls")]
    pub tool_calls: Option<Vec<AiToolCall>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "tool_call_id"
    )]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: AiFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: AiFunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiFunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiInferenceRequest {
    #[serde(default)]
    pub request_id: Option<String>,
    pub messages: Vec<AiChatMessage>,
    #[serde(default)]
    pub tools: Vec<AiToolDefinition>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiInferenceResponse {
    pub message: AiChatMessage,
    pub finish_reason: Option<String>,
    pub usage: Option<AiUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsage {
    #[serde(alias = "prompt_tokens")]
    pub prompt_tokens: Option<u64>,
    #[serde(alias = "completion_tokens")]
    pub completion_tokens: Option<u64>,
    #[serde(alias = "total_tokens")]
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConnectionResult {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
struct ProviderResponse {
    choices: Vec<ProviderChoice>,
    #[serde(default)]
    usage: Option<AiUsage>,
}

#[derive(Debug, Deserialize)]
struct ProviderChoice {
    message: AiChatMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

fn provider_endpoint(base_url: &str) -> Result<String> {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(anyhow!("AI Base URL is not configured"));
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| anyhow!("AI Base URL is invalid"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("AI Base URL must include a host"))?;
    let local_http = parsed.scheme() == "http" && matches!(host, "localhost" | "127.0.0.1" | "::1");
    if parsed.scheme() != "https" && !local_http {
        return Err(anyhow!(
            "AI Base URL must use HTTPS, except for local endpoints"
        ));
    }
    if trimmed.ends_with("/chat/completions") {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("{trimmed}/chat/completions"))
    }
}

pub fn validate_base_url(base_url: &str) -> Result<()> {
    provider_endpoint(base_url).map(|_| ())
}

fn request_body(
    settings: &SettingsState,
    messages: &[AiChatMessage],
    tools: &[AiToolDefinition],
) -> Value {
    let provider_messages: Vec<Value> = messages
        .iter()
        .map(|message| {
            let mut value = json!({
                "role": message.role,
                "content": message.content,
            });
            if let Some(tool_calls) = &message.tool_calls {
                value["tool_calls"] =
                    serde_json::to_value(tool_calls).unwrap_or_else(|_| json!([]));
            }
            if let Some(tool_call_id) = &message.tool_call_id {
                value["tool_call_id"] = json!(tool_call_id);
            }
            value
        })
        .collect();
    let mut body = json!({
        "model": settings.ai_model_name.trim(),
        "messages": provider_messages,
        "temperature": 0.2,
    });
    if !tools.is_empty() {
        body["tools"] = serde_json::to_value(tools).unwrap_or_else(|_| json!([]));
        body["tool_choice"] = json!("auto");
    }
    body
}

pub async fn infer(
    settings: &SettingsState,
    request: AiInferenceRequest,
    api_key: String,
    cancellation: CancellationToken,
) -> Result<AiInferenceResponse> {
    let started = std::time::Instant::now();
    let _operation = logging::OperationGuard::new("ai.infer");
    if settings.ai_model_name.trim().is_empty() {
        return Err(anyhow!("AI model name is not configured"));
    }
    let endpoint = provider_endpoint(&settings.ai_base_url)?;
    if request.messages.is_empty() {
        return Err(anyhow!("AI inference requires at least one message"));
    }
    if request.messages.len() > 64 {
        return Err(anyhow!(
            "AI conversation is too long; start a new conversation"
        ));
    }
    let client = ai_client();
    let per_request_timeout =
        Duration::from_secs(settings.ai_request_timeout_seconds.clamp(1, 600));
    let parsed_endpoint = reqwest::Url::parse(&endpoint).ok();
    tracing::info!(
        target: "witness_lib::network::ai",
        phase = "request_started",
        host = parsed_endpoint.as_ref().and_then(reqwest::Url::host_str).unwrap_or("unknown"),
        path = parsed_endpoint.as_ref().map(reqwest::Url::path).unwrap_or("unknown"),
        method = "POST",
        message_count = request.messages.len(),
        tool_count = request.tools.len(),
        api_key_configured = !api_key.is_empty(),
        "AI provider request started"
    );
    let request_body = request_body(settings, &request.messages, &request.tools);
    let request_bytes = serde_json::to_vec(&request_body)
        .map(|body| body.len())
        .unwrap_or(0);
    let mut call = client.post(endpoint).json(&request_body);
    if !api_key.is_empty() {
        call = call.bearer_auth(api_key);
    }
    let response = match tokio::select! {
        _ = cancellation.cancelled() => return Err(anyhow!("AI inference cancelled")),
        result = tokio::time::timeout(per_request_timeout, call.send()) => match result {
            Ok(result) => result,
            Err(_) => return Err(anyhow!("AI provider request timed out")),
        },
    } {
        Ok(response) => response,
        Err(error) => {
            tracing::error!(
                target: "witness_lib::network::ai",
                phase = "transport_failed",
                request_bytes,
                error = %error,
                duration_ms = started.elapsed().as_millis() as u64,
                "AI provider transport request failed"
            );
            return Err(error.into());
        }
    };
    let status = response.status().as_u16();
    tracing::info!(
        target: "witness_lib::network::ai",
        phase = "response_headers_received",
        status,
        request_bytes,
        "AI provider response headers received"
    );
    if !response.status().is_success() {
        let status = response.status().as_u16();
        // Include the first 2k chars of the provider error body to aid
        // debugging without flooding logs with huge payloads.
        let body_text = response.text().await.unwrap_or_default();
        let truncated: String = body_text.chars().take(2000).collect();
        tracing::warn!(
            target: "witness_lib::network::ai",
            phase = "request_rejected",
            status,
            duration_ms = started.elapsed().as_millis() as u64,
            error_body = %truncated,
            "AI provider rejected the request"
        );
        if truncated.trim().is_empty() {
            return Err(anyhow!("AI provider returned HTTP {status}"));
        }
        return Err(anyhow!("AI provider returned HTTP {status}: {truncated}"));
    }
    let body_bytes = tokio::select! {
        _ = cancellation.cancelled() => return Err(anyhow!("AI inference cancelled")),
        result = response.bytes() => result?,
    };
    tracing::debug!(
        target: "witness_lib::network::ai",
        phase = "response_body_received",
        status,
        response_bytes = body_bytes.len(),
        "AI provider response body received"
    );
    let body: ProviderResponse = serde_json::from_slice(&body_bytes)
        .map_err(|_| anyhow!("AI provider returned an invalid inference response"))?;
    let choice = body
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("AI provider returned no response choices"))?;
    let result = AiInferenceResponse {
        message: choice.message,
        finish_reason: choice.finish_reason,
        usage: body.usage,
    };
    tracing::info!(
        target: "witness_lib::network::ai",
        phase = "request_completed",
        status,
        response_bytes = body_bytes.len(),
        duration_ms = started.elapsed().as_millis() as u64,
        "AI provider request completed"
    );
    Ok(result)
}

pub async fn test_connection(
    settings: &SettingsState,
    api_key: String,
) -> Result<AiConnectionResult> {
    let request = AiInferenceRequest {
        request_id: None,
        messages: vec![AiChatMessage {
            role: "user".into(),
            content: Some("Reply with OK.".into()),
            tool_calls: None,
            tool_call_id: None,
        }],
        tools: Vec::new(),
    };
    infer(settings, request, api_key, CancellationToken::new()).await?;
    Ok(AiConnectionResult {
        ok: true,
        message: "AI provider connection succeeded".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https_and_local_http_endpoints() {
        assert!(provider_endpoint("https://example.test/v1").is_ok());
        assert!(provider_endpoint("http://127.0.0.1:1234/v1").is_ok());
        assert!(provider_endpoint("http://example.test/v1").is_err());
    }

    #[test]
    fn appends_chat_completions_once() {
        assert_eq!(
            provider_endpoint("https://example.test/v1/chat/completions").unwrap(),
            "https://example.test/v1/chat/completions"
        );
    }

    #[test]
    fn masks_saved_keys_without_exposing_the_middle() {
        assert_eq!(mask_key("abcdefghi"), ("abc".into(), "ghi".into()));
        assert_eq!(mask_key("abcdef"), ("•••".into(), "•••".into()));
    }

    #[test]
    fn uses_provider_wire_names_for_tool_messages() {
        let settings = SettingsState {
            ai_model_name: "test-model".into(),
            ..SettingsState::default()
        };
        let body = request_body(
            &settings,
            &[AiChatMessage {
                role: "tool".into(),
                content: Some("done".into()),
                tool_calls: None,
                tool_call_id: Some("call-1".into()),
            }],
            &[],
        );
        assert_eq!(body["messages"][0]["tool_call_id"], "call-1");
        assert!(body["messages"][0].get("toolCallId").is_none());
    }

    #[test]
    fn deleting_an_empty_credential_store_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("ai-credentials.hold");
        let salt = directory.path().join("stronghold.salt");
        let store = AiCredentialStore::open(&snapshot, &salt).unwrap();

        store.delete_key().unwrap();

        assert!(!snapshot.exists());
    }
}
