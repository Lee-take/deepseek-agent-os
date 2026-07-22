#![allow(dead_code)]

use crate::kernel::models::{ModelRoute, ThinkingLevel};
use crate::kernel::workflow::{OperationsBriefingSynthesis, OperationsBriefingSynthesizer};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::Instant;
use zeroize::{Zeroize, Zeroizing};

pub const DEEPSEEK_AUTO_LABEL: &str = "DeepSeek Auto";
pub const DEEPSEEK_FLASH_MODEL: &str = "deepseek-v4-flash";
pub const DEEPSEEK_PRO_MODEL: &str = "deepseek-v4-pro";
pub const DEEPSEEK_API_BASE_URL: &str = "https://api.deepseek.com";
pub const DEEPSEEK_CHAT_COMPLETIONS_PATH: &str = "/chat/completions";
pub const DEEPSEEK_USER_BALANCE_PATH: &str = "/user/balance";
pub const DEEPSEEK_MODELS_PATH: &str = "/models";
pub const DEEPSEEK_API_KEY_ENV: &str = "DEEPSEEK_API_KEY";
pub const DEEPSEEK_CHAT_HTTP_TIMEOUT_SECS: u64 = 90;
const DEEPSEEK_RESPONSE_MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekUserBalanceInfo {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekUserBalanceResponse {
    pub is_available: bool,
    #[serde(default)]
    pub balance_infos: Vec<DeepSeekUserBalanceInfo>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DeepSeekModelDescriptor {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DeepSeekModelListResponse {
    #[serde(default)]
    pub data: Vec<DeepSeekModelDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DeepSeekTransportFailure {
    #[error("deepseek_http_status_{0}")]
    HttpStatus(u16),
    #[error("deepseek_network_unavailable")]
    NetworkUnavailable,
    #[error("deepseek_network_timeout")]
    Timeout,
    #[error("deepseek_provider_protocol_error")]
    Protocol,
}

pub trait DeepSeekReadinessTransport {
    fn fetch_user_balance(
        &self,
        api_key: &str,
    ) -> Result<DeepSeekUserBalanceResponse, DeepSeekTransportFailure>;

    fn fetch_models(
        &self,
        api_key: &str,
    ) -> Result<DeepSeekModelListResponse, DeepSeekTransportFailure>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeepSeekChatRole {
    System,
    User,
    Assistant,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeepSeekThinkingMode {
    Enabled,
    Disabled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekThinking {
    #[serde(rename = "type")]
    pub mode: DeepSeekThinkingMode,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekChatMessage {
    pub role: DeepSeekChatRole,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekChatCompletionRequest {
    pub model: String,
    pub messages: Vec<DeepSeekChatMessage>,
    pub thinking: DeepSeekThinking,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    pub stream: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekChatCompletionResponse {
    pub model: String,
    pub choices: Vec<DeepSeekChatChoice>,
    #[serde(default)]
    pub usage: Option<DeepSeekChatCompletionUsage>,
}

impl DeepSeekChatCompletionResponse {
    pub fn from_text(model: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            choices: vec![DeepSeekChatChoice {
                message: DeepSeekChatMessage {
                    role: DeepSeekChatRole::Assistant,
                    content: content.into(),
                },
            }],
            usage: None,
        }
    }

    pub fn first_text(&self) -> Option<&str> {
        self.choices
            .first()
            .map(|choice| choice.message.content.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekChatChoice {
    pub message: DeepSeekChatMessage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekChatCompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepSeekChatCacheStatus {
    Disabled,
    Hit,
    Miss,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekChatTelemetry {
    pub id: uuid::Uuid,
    pub request_hash: String,
    pub model: String,
    pub cache_status: DeepSeekChatCacheStatus,
    pub elapsed_ms: u128,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
    pub estimated_cost_micro_usd: Option<u64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekChatCompletionExecution {
    pub response: DeepSeekChatCompletionResponse,
    pub telemetry: DeepSeekChatTelemetry,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekChatCacheState {
    pub entries: usize,
}

pub trait DeepSeekChatCompletionCache {
    fn get(&self, request_hash: &str) -> Option<DeepSeekChatCompletionResponse>;
    fn put(&self, request_hash: &str, response: &DeepSeekChatCompletionResponse);
}

#[derive(Default)]
pub struct DeepSeekMemoryChatCompletionCache {
    entries: Mutex<HashMap<String, DeepSeekChatCompletionResponse>>,
}

impl DeepSeekMemoryChatCompletionCache {
    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    pub fn state(&self) -> DeepSeekChatCacheState {
        DeepSeekChatCacheState {
            entries: self.len(),
        }
    }

    pub fn clear(&self) -> usize {
        self.entries
            .lock()
            .map(|mut entries| {
                let count = entries.len();
                entries.clear();
                count
            })
            .unwrap_or(0)
    }
}

impl DeepSeekChatCompletionCache for DeepSeekMemoryChatCompletionCache {
    fn get(&self, request_hash: &str) -> Option<DeepSeekChatCompletionResponse> {
        self.entries
            .lock()
            .ok()
            .and_then(|entries| entries.get(request_hash).cloned())
    }

    fn put(&self, request_hash: &str, response: &DeepSeekChatCompletionResponse) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(request_hash.to_string(), response.clone());
        }
    }
}

pub trait DeepSeekChatCompletionTransport {
    fn post_chat_completion(
        &self,
        endpoint: &str,
        api_key: &str,
        request: &DeepSeekChatCompletionRequest,
    ) -> Result<DeepSeekChatCompletionResponse, String>;
}

pub struct HttpDeepSeekChatCompletionTransport {
    client: reqwest::blocking::Client,
}

pub struct DeepSeekOperationsBriefingSynthesizer<'a, T: DeepSeekChatCompletionTransport> {
    transport: &'a T,
    cache: Option<&'a dyn DeepSeekChatCompletionCache>,
    telemetry_log: Mutex<Vec<DeepSeekChatTelemetry>>,
    api_key: String,
    route: ModelRoute,
    thinking: ThinkingLevel,
}

impl<T: DeepSeekChatCompletionTransport> Drop for DeepSeekOperationsBriefingSynthesizer<'_, T> {
    fn drop(&mut self) {
        self.api_key.zeroize();
    }
}

impl HttpDeepSeekChatCompletionTransport {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("DS-Agent/1.4.0 deepseek-v4")
            .timeout(std::time::Duration::from_secs(
                DEEPSEEK_CHAT_HTTP_TIMEOUT_SECS,
            ))
            .build()
            .map_err(|error| format!("deepseek chat client setup failed: {error}"))?;

        Ok(Self { client })
    }
}

impl DeepSeekChatCompletionTransport for HttpDeepSeekChatCompletionTransport {
    fn post_chat_completion(
        &self,
        endpoint: &str,
        api_key: &str,
        request: &DeepSeekChatCompletionRequest,
    ) -> Result<DeepSeekChatCompletionResponse, String> {
        let response = self
            .client
            .post(endpoint)
            .bearer_auth(api_key)
            .json(request)
            .send()
            .map_err(|error| safe_transport_failure(classify_reqwest_error(&error)))?;
        let status = response.status();
        if !status.is_success() {
            return Err(safe_transport_failure(
                DeepSeekTransportFailure::HttpStatus(status.as_u16()),
            ));
        }
        let mut body = read_bounded_response(response).map_err(safe_transport_failure)?;
        let parsed = serde_json::from_slice::<DeepSeekChatCompletionResponse>(&body)
            .map_err(|_| safe_transport_failure(DeepSeekTransportFailure::Protocol));
        body.zeroize();
        parsed
    }
}

impl HttpDeepSeekChatCompletionTransport {
    fn fetch_json<T: for<'de> Deserialize<'de>>(
        &self,
        endpoint: &str,
        api_key: &str,
    ) -> Result<T, DeepSeekTransportFailure> {
        let response = self
            .client
            .get(endpoint)
            .bearer_auth(api_key)
            .send()
            .map_err(|error| classify_reqwest_error(&error))?;
        let status = response.status();
        if !status.is_success() {
            return Err(DeepSeekTransportFailure::HttpStatus(status.as_u16()));
        }
        let mut body = read_bounded_response(response)?;
        let parsed = serde_json::from_slice(&body).map_err(|_| DeepSeekTransportFailure::Protocol);
        body.zeroize();
        parsed
    }
}

impl DeepSeekReadinessTransport for HttpDeepSeekChatCompletionTransport {
    fn fetch_user_balance(
        &self,
        api_key: &str,
    ) -> Result<DeepSeekUserBalanceResponse, DeepSeekTransportFailure> {
        self.fetch_json(
            &format!("{DEEPSEEK_API_BASE_URL}{DEEPSEEK_USER_BALANCE_PATH}"),
            api_key,
        )
    }

    fn fetch_models(
        &self,
        api_key: &str,
    ) -> Result<DeepSeekModelListResponse, DeepSeekTransportFailure> {
        self.fetch_json(
            &format!("{DEEPSEEK_API_BASE_URL}{DEEPSEEK_MODELS_PATH}"),
            api_key,
        )
    }
}

fn classify_reqwest_error(error: &reqwest::Error) -> DeepSeekTransportFailure {
    if error.is_timeout() {
        DeepSeekTransportFailure::Timeout
    } else if error.is_connect() {
        DeepSeekTransportFailure::NetworkUnavailable
    } else {
        DeepSeekTransportFailure::Protocol
    }
}

fn safe_transport_failure(error: DeepSeekTransportFailure) -> String {
    error.to_string()
}

fn read_bounded_response(
    mut response: reqwest::blocking::Response,
) -> Result<Vec<u8>, DeepSeekTransportFailure> {
    if response
        .content_length()
        .map(|length| length > DEEPSEEK_RESPONSE_MAX_BYTES as u64)
        .unwrap_or(false)
    {
        return Err(DeepSeekTransportFailure::Protocol);
    }
    let mut body = Vec::new();
    Read::take(&mut response, (DEEPSEEK_RESPONSE_MAX_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|_| DeepSeekTransportFailure::Protocol)?;
    if body.len() > DEEPSEEK_RESPONSE_MAX_BYTES {
        body.zeroize();
        return Err(DeepSeekTransportFailure::Protocol);
    }
    Ok(body)
}

impl<'a, T: DeepSeekChatCompletionTransport> DeepSeekOperationsBriefingSynthesizer<'a, T> {
    pub fn new(
        transport: &'a T,
        api_key: String,
        route: ModelRoute,
        thinking: ThinkingLevel,
    ) -> Self {
        Self {
            transport,
            cache: None,
            telemetry_log: Mutex::new(Vec::new()),
            api_key,
            route,
            thinking,
        }
    }

    pub fn new_with_cache(
        transport: &'a T,
        cache: &'a dyn DeepSeekChatCompletionCache,
        api_key: String,
        route: ModelRoute,
        thinking: ThinkingLevel,
    ) -> Self {
        Self {
            transport,
            cache: Some(cache),
            telemetry_log: Mutex::new(Vec::new()),
            api_key,
            route,
            thinking,
        }
    }

    pub fn take_telemetry(&self) -> Vec<DeepSeekChatTelemetry> {
        self.telemetry_log
            .lock()
            .map(|mut entries| std::mem::take(&mut *entries))
            .unwrap_or_default()
    }
}

impl<T: DeepSeekChatCompletionTransport> OperationsBriefingSynthesizer
    for DeepSeekOperationsBriefingSynthesizer<'_, T>
{
    fn synthesize_briefing(
        &self,
        manifest_excerpt: &str,
        evidence_ref: Option<&str>,
    ) -> Result<OperationsBriefingSynthesis, String> {
        let user_prompt = operations_briefing_user_prompt(manifest_excerpt, evidence_ref);
        let request = build_deepseek_chat_completion_request(
            self.route,
            self.thinking,
            OPERATIONS_BRIEFING_SYSTEM_PROMPT,
            &user_prompt,
        )?;
        let response = if let Some(cache) = self.cache {
            let execution = execute_deepseek_chat_completion_with_cache(
                self.transport,
                cache,
                &self.api_key,
                &request,
            )?;
            if let Ok(mut telemetry_log) = self.telemetry_log.lock() {
                telemetry_log.push(execution.telemetry);
            }
            execution.response
        } else {
            execute_deepseek_chat_completion(self.transport, &self.api_key, &request)?
        };
        let content = response
            .first_text()
            .ok_or_else(|| "deepseek response did not include assistant content".to_string())?;

        parse_operations_briefing_synthesis(content)
    }
}

const OPERATIONS_BRIEFING_SYSTEM_PROMPT: &str = "You are an operations briefing analyst. Return strict JSON only. The JSON object must contain summary, anomalies, action_plan, and warnings. Do not invent evidence beyond the provided manifest.";
const OPERATIONS_BRIEFING_MAX_MANIFEST_CHARS: usize = 12_000;

fn operations_briefing_user_prompt(manifest_excerpt: &str, evidence_ref: Option<&str>) -> String {
    let manifest_excerpt =
        truncate_for_prompt(manifest_excerpt, OPERATIONS_BRIEFING_MAX_MANIFEST_CHARS);
    let evidence_ref = evidence_ref.unwrap_or("not available");
    format!(
        "Evidence reference: {evidence_ref}\n\nEvidence manifest excerpt:\n{manifest_excerpt}\n\nReturn JSON with this shape:\n{{\"summary\":\"...\",\"anomalies\":[{{\"area\":\"...\",\"signal\":\"...\",\"evidence_ref\":\"...\"}}],\"action_plan\":[{{\"owner\":\"...\",\"action\":\"...\",\"due_hint\":\"...\"}}],\"warnings\":[]}}"
    )
}

fn parse_operations_briefing_synthesis(value: &str) -> Result<OperationsBriefingSynthesis, String> {
    let json = extract_json_object(value);
    let synthesis = serde_json::from_str::<OperationsBriefingSynthesis>(json).map_err(|error| {
        format!("operations briefing synthesis JSON could not be parsed: {error}")
    })?;

    if synthesis.summary.trim().is_empty() {
        return Err("operations briefing synthesis summary is required".to_string());
    }

    Ok(synthesis)
}

fn extract_json_object(value: &str) -> &str {
    let trimmed = value.trim();
    match (trimmed.find('{'), trimmed.rfind('}')) {
        (Some(start), Some(end)) if start < end => &trimmed[start..=end],
        _ => trimmed,
    }
}

fn truncate_for_prompt(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if index >= max_chars {
            output.push_str("\n[truncated]");
            return output;
        }
        output.push(character);
    }
    output
}

pub fn deepseek_chat_completions_url() -> String {
    format!("{DEEPSEEK_API_BASE_URL}{DEEPSEEK_CHAT_COMPLETIONS_PATH}")
}

pub fn effective_model(route: ModelRoute, thinking: ThinkingLevel) -> &'static str {
    match route {
        ModelRoute::Flash => DEEPSEEK_FLASH_MODEL,
        ModelRoute::Pro => DEEPSEEK_PRO_MODEL,
        ModelRoute::Auto => match thinking {
            ThinkingLevel::Fast => DEEPSEEK_FLASH_MODEL,
            ThinkingLevel::Auto | ThinkingLevel::Standard | ThinkingLevel::Deep => {
                DEEPSEEK_PRO_MODEL
            }
        },
    }
}

pub fn thinking_budget_name(thinking: ThinkingLevel) -> &'static str {
    match thinking {
        ThinkingLevel::Auto => "auto",
        ThinkingLevel::Fast => "none",
        ThinkingLevel::Standard => "high",
        ThinkingLevel::Deep => "max",
    }
}

pub fn build_deepseek_chat_completion_request(
    route: ModelRoute,
    thinking: ThinkingLevel,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<DeepSeekChatCompletionRequest, String> {
    let system_prompt = normalize_prompt(system_prompt, "system prompt")?;
    let user_prompt = normalize_prompt(user_prompt, "user prompt")?;
    let (thinking_mode, reasoning_effort) = deepseek_thinking_payload(thinking);

    Ok(DeepSeekChatCompletionRequest {
        model: effective_model(route, thinking).to_string(),
        messages: vec![
            DeepSeekChatMessage {
                role: DeepSeekChatRole::System,
                content: system_prompt,
            },
            DeepSeekChatMessage {
                role: DeepSeekChatRole::User,
                content: user_prompt,
            },
        ],
        thinking: DeepSeekThinking {
            mode: thinking_mode,
        },
        reasoning_effort: reasoning_effort.map(str::to_string),
        stream: false,
    })
}

pub fn execute_deepseek_chat_completion(
    transport: &impl DeepSeekChatCompletionTransport,
    api_key: &str,
    request: &DeepSeekChatCompletionRequest,
) -> Result<DeepSeekChatCompletionResponse, String> {
    let api_key = normalize_api_key(api_key)?;
    transport
        .post_chat_completion(&deepseek_chat_completions_url(), &api_key, request)
        .map_err(|error| redact_secret(&error, &api_key))
}

pub fn execute_deepseek_chat_completion_with_cache(
    transport: &impl DeepSeekChatCompletionTransport,
    cache: &(impl DeepSeekChatCompletionCache + ?Sized),
    api_key: &str,
    request: &DeepSeekChatCompletionRequest,
) -> Result<DeepSeekChatCompletionExecution, String> {
    let request_hash = deepseek_chat_request_hash(request)?;
    let started_at = Instant::now();

    if let Some(response) = cache.get(&request_hash) {
        let telemetry = deepseek_chat_telemetry(
            &request_hash,
            request,
            &response,
            DeepSeekChatCacheStatus::Hit,
            started_at.elapsed().as_millis(),
        );
        return Ok(DeepSeekChatCompletionExecution {
            response,
            telemetry,
        });
    }

    let response = execute_deepseek_chat_completion(transport, api_key, request)?;
    cache.put(&request_hash, &response);
    let telemetry = deepseek_chat_telemetry(
        &request_hash,
        request,
        &response,
        DeepSeekChatCacheStatus::Miss,
        started_at.elapsed().as_millis(),
    );

    Ok(DeepSeekChatCompletionExecution {
        response,
        telemetry,
    })
}

pub fn deepseek_chat_request_hash(
    request: &DeepSeekChatCompletionRequest,
) -> Result<String, String> {
    let request_json = serde_json::to_vec(request)
        .map_err(|error| format!("deepseek chat request could not be hashed: {error}"))?;
    let mut hasher = Sha256::new();
    hasher.update(request_json);
    Ok(hex::encode(hasher.finalize()))
}

fn deepseek_chat_telemetry(
    request_hash: &str,
    request: &DeepSeekChatCompletionRequest,
    response: &DeepSeekChatCompletionResponse,
    cache_status: DeepSeekChatCacheStatus,
    elapsed_ms: u128,
) -> DeepSeekChatTelemetry {
    DeepSeekChatTelemetry {
        id: uuid::Uuid::new_v4(),
        request_hash: request_hash.to_string(),
        model: if response.model.trim().is_empty() {
            request.model.clone()
        } else {
            response.model.clone()
        },
        cache_status,
        elapsed_ms,
        prompt_tokens: response.usage.as_ref().map(|usage| usage.prompt_tokens),
        completion_tokens: response.usage.as_ref().map(|usage| usage.completion_tokens),
        total_tokens: response.usage.as_ref().map(|usage| usage.total_tokens),
        estimated_cost_micro_usd: None,
        created_at: Utc::now(),
    }
}

fn deepseek_thinking_payload(
    thinking: ThinkingLevel,
) -> (DeepSeekThinkingMode, Option<&'static str>) {
    match thinking {
        ThinkingLevel::Fast => (DeepSeekThinkingMode::Disabled, None),
        ThinkingLevel::Auto => (DeepSeekThinkingMode::Enabled, None),
        ThinkingLevel::Standard => (DeepSeekThinkingMode::Enabled, Some("high")),
        ThinkingLevel::Deep => (DeepSeekThinkingMode::Enabled, Some("max")),
    }
}

fn normalize_prompt(value: &str, label: &str) -> Result<String, String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Err(format!("{label} is required"));
    }

    Ok(normalized)
}

fn normalize_api_key(value: &str) -> Result<Zeroizing<String>, String> {
    let normalized = Zeroizing::new(value.trim().to_string());
    if normalized.is_empty() {
        return Err(format!("{DEEPSEEK_API_KEY_ENV} is required"));
    }

    Ok(normalized)
}

fn redact_secret(message: &str, secret: &str) -> String {
    if secret.is_empty() {
        return message.to_string();
    }

    message.replace(secret, "[REDACTED]")
}

fn truncate_for_error(value: &str, max_chars: usize) -> String {
    let mut truncated = String::new();
    for (index, character) in value.chars().enumerate() {
        if index >= max_chars {
            truncated.push_str("...");
            return truncated;
        }
        truncated.push(character);
    }
    truncated
}

#[cfg(test)]
mod tests {
    use crate::kernel::models::{ModelRoute, ThinkingLevel};
    use crate::kernel::workflow::OperationsBriefingSynthesizer;
    use std::cell::RefCell;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    use super::{
        build_deepseek_chat_completion_request, effective_model, execute_deepseek_chat_completion,
        execute_deepseek_chat_completion_with_cache, thinking_budget_name, DeepSeekChatCacheStatus,
        DeepSeekChatCompletionCache, DeepSeekChatCompletionResponse,
        DeepSeekChatCompletionTransport, DeepSeekChatCompletionUsage,
        DeepSeekMemoryChatCompletionCache, DeepSeekOperationsBriefingSynthesizer,
        DeepSeekThinkingMode, DeepSeekUserBalanceResponse, HttpDeepSeekChatCompletionTransport,
        DEEPSEEK_API_BASE_URL, DEEPSEEK_API_KEY_ENV, DEEPSEEK_CHAT_COMPLETIONS_PATH,
        DEEPSEEK_CHAT_HTTP_TIMEOUT_SECS, DEEPSEEK_FLASH_MODEL, DEEPSEEK_PRO_MODEL,
    };

    #[test]
    fn deepseek_chat_http_timeout_allows_long_agent_followups() {
        assert!(DEEPSEEK_CHAT_HTTP_TIMEOUT_SECS >= 90);
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct FakeDeepSeekCall {
        endpoint: String,
        api_key: String,
        model: String,
        system_prompt: String,
        user_prompt: String,
    }

    struct RecordedHttpRequest {
        raw: String,
    }

    #[derive(Default)]
    struct FakeDeepSeekTransport {
        calls: RefCell<Vec<FakeDeepSeekCall>>,
        response: Option<DeepSeekChatCompletionResponse>,
        error: Option<String>,
    }

    impl FakeDeepSeekTransport {
        fn succeeding(content: &str) -> Self {
            Self {
                response: Some(DeepSeekChatCompletionResponse::from_text(
                    DEEPSEEK_PRO_MODEL,
                    content,
                )),
                ..Self::default()
            }
        }

        fn failing(error: &str) -> Self {
            Self {
                error: Some(error.to_string()),
                ..Self::default()
            }
        }
    }

    impl DeepSeekChatCompletionTransport for FakeDeepSeekTransport {
        fn post_chat_completion(
            &self,
            endpoint: &str,
            api_key: &str,
            request: &super::DeepSeekChatCompletionRequest,
        ) -> Result<DeepSeekChatCompletionResponse, String> {
            self.calls.borrow_mut().push(FakeDeepSeekCall {
                endpoint: endpoint.to_string(),
                api_key: api_key.to_string(),
                model: request.model.clone(),
                system_prompt: request.messages[0].content.clone(),
                user_prompt: request.messages[1].content.clone(),
            });

            if let Some(error) = &self.error {
                return Err(error.clone());
            }

            self.response
                .clone()
                .ok_or_else(|| "missing fake response".to_string())
        }
    }

    fn serve_one_deepseek_response(
        response_body: String,
    ) -> (String, JoinHandle<RecordedHttpRequest>) {
        let listener = TcpListener::bind("127.0.1.0:0").expect("bind fake deepseek server");
        let endpoint = format!("http://{}", listener.local_addr().expect("local addr"));
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept fake deepseek request");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .expect("set read timeout");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(bytes_read) => {
                        request.extend_from_slice(&buffer[..bytes_read]);
                        if http_request_is_complete(&request) {
                            break;
                        }
                    }
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            || error.kind() == std::io::ErrorKind::TimedOut =>
                    {
                        break;
                    }
                    Err(error) => panic!("read request failed: {error}"),
                }
            }
            let raw = String::from_utf8(request).expect("request utf8");
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write fake deepseek response");
            RecordedHttpRequest { raw }
        });

        (endpoint, handle)
    }

    fn http_request_is_complete(request: &[u8]) -> bool {
        let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            return false;
        };
        let header_text = String::from_utf8_lossy(&request[..header_end]);
        let content_length = header_text
            .lines()
            .find_map(|line| {
                line.strip_prefix("content-length:")
                    .or_else(|| line.strip_prefix("Content-Length:"))
            })
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        request.len() >= header_end + 4 + content_length
    }

    #[test]
    fn auto_fast_uses_flash_model() {
        assert_eq!(
            effective_model(ModelRoute::Auto, ThinkingLevel::Fast),
            DEEPSEEK_FLASH_MODEL
        );
    }

    #[test]
    fn auto_deep_uses_pro_model() {
        assert_eq!(
            effective_model(ModelRoute::Auto, ThinkingLevel::Deep),
            DEEPSEEK_PRO_MODEL
        );
    }

    #[test]
    fn thinking_levels_map_to_budget_names() {
        assert_eq!(thinking_budget_name(ThinkingLevel::Auto), "auto");
        assert_eq!(thinking_budget_name(ThinkingLevel::Fast), "none");
        assert_eq!(thinking_budget_name(ThinkingLevel::Standard), "high");
        assert_eq!(thinking_budget_name(ThinkingLevel::Deep), "max");
    }

    #[test]
    fn chat_completion_request_uses_route_model_messages_and_deep_thinking() {
        let request = build_deepseek_chat_completion_request(
            ModelRoute::Auto,
            ThinkingLevel::Deep,
            "You are a careful assistant.",
            "Summarize the evidence.",
        )
        .expect("request builds");
        let value = serde_json::to_value(&request).expect("request serializes");

        assert_eq!(request.model, DEEPSEEK_PRO_MODEL);
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.thinking.mode, DeepSeekThinkingMode::Enabled);
        assert_eq!(request.reasoning_effort.as_deref(), Some("max"));
        assert_eq!(value["model"], DEEPSEEK_PRO_MODEL);
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][1]["role"], "user");
        assert_eq!(value["thinking"]["type"], "enabled");
        assert_eq!(value["reasoning_effort"], "max");
        assert_eq!(value["stream"], false);
    }

    #[test]
    fn chat_completion_request_disables_thinking_for_fast_mode() {
        let request = build_deepseek_chat_completion_request(
            ModelRoute::Auto,
            ThinkingLevel::Fast,
            "You are concise.",
            "Return one sentence.",
        )
        .expect("request builds");
        let value = serde_json::to_value(&request).expect("request serializes");

        assert_eq!(request.model, DEEPSEEK_FLASH_MODEL);
        assert_eq!(request.thinking.mode, DeepSeekThinkingMode::Disabled);
        assert_eq!(request.reasoning_effort, None);
        assert_eq!(value["thinking"]["type"], "disabled");
        assert!(value.get("reasoning_effort").is_none());
    }

    #[test]
    fn chat_completion_request_rejects_blank_prompts() {
        let error = build_deepseek_chat_completion_request(
            ModelRoute::Auto,
            ThinkingLevel::Standard,
            "",
            "Summarize.",
        )
        .expect_err("blank system prompt is invalid");

        assert!(error.contains("system prompt"));
    }

    #[test]
    fn chat_completion_executor_posts_to_official_endpoint_with_api_key() {
        let request = build_deepseek_chat_completion_request(
            ModelRoute::Pro,
            ThinkingLevel::Deep,
            "You are careful.",
            "Return a tiny answer.",
        )
        .expect("request builds");
        let transport = FakeDeepSeekTransport::succeeding("done");
        let response = execute_deepseek_chat_completion(&transport, "test-secret-token", &request)
            .expect("request executes");
        let calls = transport.calls.borrow();

        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].endpoint,
            format!("{DEEPSEEK_API_BASE_URL}{DEEPSEEK_CHAT_COMPLETIONS_PATH}")
        );
        assert_eq!(calls[0].api_key, "test-secret-token");
        assert_eq!(calls[0].model, DEEPSEEK_PRO_MODEL);
        assert_eq!(response.first_text(), Some("done"));
    }

    #[test]
    fn http_deepseek_transport_uses_current_release_user_agent() {
        let request = build_deepseek_chat_completion_request(
            ModelRoute::Pro,
            ThinkingLevel::Fast,
            "You are concise.",
            "Return ok.",
        )
        .expect("request builds");
        let response_body = serde_json::json!({
            "model": "deepseek-v4-pro",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": "ok"
                    }
                }
            ]
        })
        .to_string();
        let (endpoint, handle) = serve_one_deepseek_response(response_body);
        let transport = HttpDeepSeekChatCompletionTransport::new().expect("http transport");

        let response = transport
            .post_chat_completion(&endpoint, "test-secret-token", &request)
            .expect("fake deepseek response");
        let recorded = handle.join().expect("fake deepseek server joins");
        let normalized_headers = recorded.raw.to_ascii_lowercase();

        assert_eq!(response.first_text(), Some("ok"));
        assert!(recorded.raw.starts_with("POST / HTTP/1.1"));
        assert!(normalized_headers.contains("user-agent: ds-agent/1.4.0 deepseek-v4"));
    }

    #[test]
    fn chat_completion_executor_rejects_missing_api_key_before_transport() {
        let request = build_deepseek_chat_completion_request(
            ModelRoute::Pro,
            ThinkingLevel::Deep,
            "You are careful.",
            "Return a tiny answer.",
        )
        .expect("request builds");
        let transport = FakeDeepSeekTransport::succeeding("done");
        let error = execute_deepseek_chat_completion(&transport, "  ", &request)
            .expect_err("blank key is rejected");

        assert!(error.contains(DEEPSEEK_API_KEY_ENV));
        assert!(transport.calls.borrow().is_empty());
    }

    #[test]
    fn chat_completion_executor_redacts_api_key_from_transport_errors() {
        let request = build_deepseek_chat_completion_request(
            ModelRoute::Pro,
            ThinkingLevel::Deep,
            "You are careful.",
            "Return a tiny answer.",
        )
        .expect("request builds");
        let transport = FakeDeepSeekTransport::failing("upstream rejected test-secret-token");
        let error = execute_deepseek_chat_completion(&transport, "test-secret-token", &request)
            .expect_err("transport error is returned");

        assert!(error.contains("[REDACTED]"));
        assert!(!error.contains("test-secret-token"));
    }

    #[test]
    fn user_balance_response_parses_official_schema() {
        let response = serde_json::from_str::<DeepSeekUserBalanceResponse>(
            r#"{
              "is_available": true,
              "balance_infos": [
                {
                  "currency": "CNY",
                  "total_balance": "110.00",
                  "granted_balance": "10.00",
                  "topped_up_balance": "100.00"
                }
              ]
            }"#,
        )
        .expect("official balance response parses");

        assert!(response.is_available);
        assert_eq!(response.balance_infos.len(), 1);
        assert_eq!(response.balance_infos[0].currency, "CNY");
        assert_eq!(response.balance_infos[0].total_balance, "110.00");
    }

    #[test]
    fn chat_completion_cache_records_miss_then_hit_without_second_transport_call() {
        let mut response = DeepSeekChatCompletionResponse::from_text(
            DEEPSEEK_PRO_MODEL,
            "{\"summary\":\"cached\"}",
        );
        response.usage = Some(DeepSeekChatCompletionUsage {
            prompt_tokens: 20,
            completion_tokens: 5,
            total_tokens: 25,
        });
        let transport = FakeDeepSeekTransport {
            response: Some(response),
            ..FakeDeepSeekTransport::default()
        };
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let request = build_deepseek_chat_completion_request(
            ModelRoute::Pro,
            ThinkingLevel::Standard,
            "System prompt",
            "User prompt with private evidence text",
        )
        .expect("request builds");

        let first = execute_deepseek_chat_completion_with_cache(
            &transport,
            &cache,
            "test-secret-token",
            &request,
        )
        .expect("first request succeeds");
        let second = execute_deepseek_chat_completion_with_cache(
            &transport,
            &cache,
            "test-secret-token",
            &request,
        )
        .expect("second request succeeds from cache");

        assert_eq!(first.telemetry.cache_status, DeepSeekChatCacheStatus::Miss);
        assert_eq!(second.telemetry.cache_status, DeepSeekChatCacheStatus::Hit);
        assert_eq!(transport.calls.borrow().len(), 1);
        assert_eq!(second.telemetry.prompt_tokens, Some(20));
        assert_eq!(second.telemetry.completion_tokens, Some(5));
        assert_eq!(second.telemetry.total_tokens, Some(25));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn chat_completion_cache_clear_returns_removed_entry_count() {
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let response =
            DeepSeekChatCompletionResponse::from_text(DEEPSEEK_FLASH_MODEL, "cached answer");

        cache.put("request-a", &response);
        cache.put("request-b", &response);

        assert_eq!(cache.len(), 2);
        assert_eq!(cache.clear(), 2);
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.clear(), 0);
    }

    #[test]
    fn chat_completion_telemetry_uses_hash_without_prompt_or_secret_text() {
        let transport = FakeDeepSeekTransport::succeeding("{\"summary\":\"ok\"}");
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let request = build_deepseek_chat_completion_request(
            ModelRoute::Pro,
            ThinkingLevel::Deep,
            "System prompt with private policy",
            "User prompt with private evidence text",
        )
        .expect("request builds");

        let execution = execute_deepseek_chat_completion_with_cache(
            &transport,
            &cache,
            "test-secret-token",
            &request,
        )
        .expect("request succeeds");
        let telemetry_json =
            serde_json::to_string(&execution.telemetry).expect("telemetry serializes");

        assert!(!execution.telemetry.request_hash.is_empty());
        assert!(execution.telemetry.estimated_cost_micro_usd.is_none());
        assert!(!telemetry_json.contains("private evidence"));
        assert!(!telemetry_json.contains("private policy"));
        assert!(!telemetry_json.contains("test-secret-token"));
    }

    #[test]
    fn operations_briefing_synthesizer_posts_evidence_prompt_and_parses_json() {
        let transport = FakeDeepSeekTransport::succeeding(
            r#"{
                "summary": "Model summary: occupancy pressure is visible.",
                "anomalies": [
                    {
                        "area": "Guest experience",
                        "signal": "West wing complaints increased.",
                        "evidence_ref": "fixtures/evidence"
                    }
                ],
                "action_plan": [
                    {
                        "owner": "Rooms",
                        "action": "Inspect west wing service recovery drivers.",
                        "due_hint": "48 hours"
                    }
                ],
                "warnings": []
            }"#,
        );
        let synthesizer = DeepSeekOperationsBriefingSynthesizer::new(
            &transport,
            "test-secret-token".to_string(),
            ModelRoute::Pro,
            ThinkingLevel::Standard,
        );

        let synthesis = synthesizer
            .synthesize_briefing(
                "Evidence manifest: 2 text files. revenue.md; complaints.txt.",
                Some("fixtures/evidence"),
            )
            .expect("model synthesis succeeds");
        let calls = transport.calls.borrow();

        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].api_key, "test-secret-token");
        assert_eq!(calls[0].model, DEEPSEEK_PRO_MODEL);
        assert!(calls[0].system_prompt.contains("operations briefing"));
        assert!(calls[0].user_prompt.contains("revenue.md"));
        assert!(calls[0].user_prompt.contains("fixtures/evidence"));
        assert!(!calls[0].user_prompt.contains("test-secret-token"));
        assert_eq!(
            synthesis.summary,
            "Model summary: occupancy pressure is visible."
        );
        assert_eq!(synthesis.anomalies[0].area, "Guest experience");
        assert_eq!(synthesis.action_plan[0].owner, "Rooms");
        assert!(synthesis.warnings.is_empty());
    }
}
