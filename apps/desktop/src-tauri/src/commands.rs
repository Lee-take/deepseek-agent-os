use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Instant;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::kernel::agent_context::{
    agent_loop_mode_descriptor, classify_agent_action_loop_mode, AgentContextReceipt, AgentLoopMode,
};
use crate::kernel::attachments::{stage_agent_attachment_paths, AgentAttachment};
use crate::kernel::capability::{
    run_browser_browse, run_browser_submit_boundary, run_computer_control_boundary,
    run_computer_screenshot, run_drive_read_boundary, run_drive_write_boundary,
    run_email_draft_boundary, run_email_read_boundary, run_email_send_boundary,
    run_evidence_folder_ingest, run_file_read, run_file_write_boundary,
    run_filesystem_mutation_boundary, run_network_search_boundary,
    run_terminal_read as run_terminal_read_capability, run_terminal_write_boundary,
    BrowserBrowseRequest, BrowserPageClient, BrowserSubmitRequest, CapabilityInvocation,
    CapabilityInvocationStatus, CodexBridgeComputerControlClient,
    CodexBridgeComputerScreenshotClient, CodexBridgeNetworkSearchClient, ComputerControlClient,
    ComputerControlRequest, ComputerScreenshotRequest, DriveReadRequest, DriveWriteExportFile,
    DriveWriteRequest, EmailDraftRequest, EmailReadRequest, EmailSendRequest,
    EvidenceFolderRequest, FileContentClient, FileReadRequest, FileSystemMutationOperation,
    FileSystemMutationRequest, FileWriteClient, FileWriteRequest, FileWriteResult,
    HttpBrowserPageClient, HttpNetworkSearchClient, LocalComputerControlClient,
    LocalComputerScreenshotClient, LocalDriveFolderClient, LocalEvidenceFolderClient,
    LocalFileContentClient, LocalFileSystemMutationClient, LocalTerminalReadClient,
    LocalWorkspaceFileWriteClient, NetworkSearchClient, NetworkSearchRequest, NetworkSearchResult,
    TerminalReadRequest, TerminalWriteRequest,
};
use crate::kernel::computer_use::{
    computer_use_backend_status_for_strategy, ComputerUseBackendStatus,
};
use crate::kernel::deepseek::{
    build_deepseek_chat_completion_request, current_deepseek_credential_status,
    execute_deepseek_chat_completion_with_cache, execute_deepseek_user_balance,
    DeepSeekChatCacheState, DeepSeekChatCacheStatus, DeepSeekChatCompletionTransport,
    DeepSeekChatTelemetry, DeepSeekCredentialStatus, DeepSeekMemoryChatCompletionCache,
    DeepSeekOperationsBriefingSynthesizer, DeepSeekUserBalanceResponse,
    HttpDeepSeekChatCompletionTransport, DEEPSEEK_API_KEY_ENV,
};
use crate::kernel::deepseek_pricing::{
    estimate_deepseek_chat_cost_micro_usd, load_deepseek_pricing_state,
    save_deepseek_pricing_settings as persist_deepseek_pricing_settings, DeepSeekPricingSettings,
    DeepSeekPricingState,
};
use crate::kernel::event_store::{EventStore, EventStoreError, EventStoreResult};
use crate::kernel::local_directory::{
    load_local_directory_state, local_directory_readiness_from_state,
    save_local_directory_settings as persist_local_directory_settings,
    LocalDirectoryReadinessStatus, LocalDirectorySettings, LocalDirectoryState,
    LOCAL_MEMORY_DIR_NAME,
};
use crate::kernel::models::FoundationState;
use crate::kernel::models::TaskRecord;
use crate::kernel::models::{AccessMode, ModelRoute, ThinkingLevel};
use crate::kernel::models::{
    ComputerControlBackend, ComputerScreenshotBackend, LargeModelProvider, NetworkSearchBackend,
    NetworkSearchSourceModel,
};
use crate::kernel::models::{
    MemoryCandidate, MemoryCandidateMergePreview, MemoryCandidateRecord,
    MemoryCandidateReplacePreview, MemoryCandidateResolution, MemoryCandidateSource,
    MemoryCandidateStatus, MemoryCandidateSuggestedAction, MemoryLifecycle, MemoryRecord,
    MemoryRecordDeletion, MemoryRecordLink, MemoryRecordUpdate, MemoryRelationKind, MemoryScope,
    MemorySensitivity, MemoryType,
};
use crate::kernel::network_search::{
    network_search_route_status_for_strategy, NetworkSearchRouteStatus,
};
use crate::kernel::office::{
    office_create_spec_from_action, office_update_spec_from_action, run_office_create_boundary,
    run_office_open_boundary, run_office_update_boundary, LocalOfficeArtifactClient, OfficeApp,
    OfficeArtifactClient, OfficeCreateRequest, OfficeCreateResult, OfficeCreateSpec,
    OfficeOpenClient, OfficeOpenRequest, OfficeOpenResult, OfficeUpdateClient, OfficeUpdateRequest,
    OfficeUpdateResult, OfficeUpdateSpec,
};
use crate::kernel::policy::{
    builtin_capability_catalog, decide as decide_capability_policy,
    request_capability_access as build_capability_access_request, CapabilityAccessRecord,
    CapabilityDescriptor, CapabilityGrantState, CapabilityKind, PermissionAuditEntry,
    PermissionResolution, PolicyDecision,
};
use crate::kernel::tool_strategy::{
    model_driven_tool_strategy_for_current_platform, ModelDrivenToolStrategy,
};
use crate::kernel::work_package::{
    export_work_package_with_tool_readiness as build_work_package_with_tool_readiness,
    parse_work_package_json, WorkPackage, WorkPackageImportPreview, WorkPackageImportSummary,
    WorkPackageToolReadiness,
};
use crate::kernel::workflow::{
    operations_briefing_html_report_file_name, operations_briefing_pdf_report_file_name,
    operations_briefing_report_file_name, render_operations_briefing_html_report,
    render_operations_briefing_pdf_report, render_operations_briefing_report,
    run_operations_briefing as build_operations_briefing_run,
    run_operations_briefing_template_seed as build_operations_briefing_template_seed,
    run_operations_briefing_with_synthesizer as build_operations_briefing_run_with_synthesizer,
    LocalOperationsBriefingTemplateSeeder, OperationsBriefingRequest, OperationsBriefingRun,
    OperationsBriefingTemplateSeedRequest,
};

pub struct AppState {
    event_store: Mutex<EventStore>,
    computer_control_unlock: Mutex<ComputerControlUnlockState>,
    deepseek_chat_cache: DeepSeekMemoryChatCompletionCache,
}

impl AppState {
    pub fn new(event_store: EventStore) -> Self {
        Self {
            event_store: Mutex::new(event_store),
            computer_control_unlock: Mutex::new(ComputerControlUnlockState::generated()),
            deepseek_chat_cache: DeepSeekMemoryChatCompletionCache::default(),
        }
    }
}

const COMPUTER_CONTROL_UNLOCK_TTL_MINUTES: i64 = 5;
const COMPUTER_CONTROL_UNLOCK_CHALLENGE_LENGTH: usize = 6;
const AGENT_CHAT_SYSTEM_PROMPT: &str = "You are the DeepSeek reasoning layer for DS Agent. DS Agent is the local execution layer. Read the full user message and return one structured agent envelope as JSON. Separate reply_to_user from agent_actions, missing_prerequisites, required_confirmations, artifact_targets, and memory_candidates. Do not claim local tools ran; propose actions for DS Agent to validate and execute.";
const AGENT_OFFICE_CREATE_EVIDENCE_TEXT_LIMIT: usize = 1200;
const APP_UPDATE_RELEASES_API_URL: &str =
    "https://api.github.com/repos/Lee-take/deepseek-agent-os/releases";
const APP_UPDATE_RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/Lee-take/deepseek-agent-os/releases/download/";
const APP_UPDATE_USER_AGENT: &str = "DS-Agent-Updater/0.1.0";
const APP_UPDATE_CURRENT_RELEASE_TAG: &str = "v0.1.0-rc.6";
const AGENT_SOUL_PROFILE_FILE_NAME: &str = "soul.md";
const AGENT_SOUL_PROFILE_CONTEXT_MAX_BYTES: usize = 800;
const AGENT_SOUL_PROFILE_MAX_BYTES: usize = 16 * 1024;
const AGENT_MEMORY_CONTEXT_MAX_RECORDS: usize = 3;
const AGENT_MEMORY_CONTEXT_MAX_BYTES: usize = 1200;
const AGENT_MEMORY_CONTEXT_SNIPPET_CHARS: usize = 220;
const AGENT_MEMORY_CANDIDATE_GATE_MAX_RECORDS: usize = 3;
const AGENT_MEMORY_CANDIDATE_EVIDENCE_CHARS: usize = 180;
const AGENT_MEMORY_CANDIDATE_REASON_CHARS: usize = 220;
const AGENT_SOUL_PROFILE_TEMPLATE: &str = "# DS Agent Soul\n\nschema_version: 1\n\n## User\n\n- preferred_name:\n- address_as:\n- language_preferences:\n- default_response_tone:\n- default_response_length:\n- formatting_preferences:\n- initiative_level:\n\n## DS Agent\n\n- user_calls_ds_agent:\n- ds_agent_should_refer_to_itself_as:\n- relationship_boundary:\n\n## Stable Preferences\n\n- workflow_preferences:\n- writing_preferences:\n- confirmation_preferences:\n- privacy_preferences:\n\n## Never Store\n\n- secrets\n- passwords\n- private account identifiers\n- sensitive personal data unless explicitly approved\n";
#[cfg(windows)]
const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentChatRequest {
    pub prompt: String,
    pub model_route: ModelRoute,
    pub thinking_level: ThinkingLevel,
    pub access_mode: AccessMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentChatRuntimeContext {
    workspace_ready: AgentChatReadiness,
    workspace_note: String,
    network_search_ready: AgentChatReadiness,
    network_search_note: String,
    network_search_source_model: Option<NetworkSearchSourceModel>,
    soul_profile: Option<AgentSoulProfileContext>,
    memory_context: AgentMemoryRuntimeContext,
    desktop_dir: Option<PathBuf>,
}

impl Default for AgentChatRuntimeContext {
    fn default() -> Self {
        Self {
            workspace_ready: AgentChatReadiness::Unknown,
            workspace_note: "workspace readiness unavailable in this test context".to_string(),
            network_search_ready: AgentChatReadiness::Unknown,
            network_search_note: "network search readiness unavailable in this test context"
                .to_string(),
            network_search_source_model: None,
            soul_profile: None,
            memory_context: AgentMemoryRuntimeContext::default(),
            desktop_dir: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentSoulProfileContext {
    lines: Vec<String>,
    used_bytes: usize,
    max_bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentSoulProfileState {
    pub exists: bool,
    pub content: String,
    pub summary_lines: Vec<String>,
    pub used_bytes: usize,
    pub max_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentMemoryRuntimeContext {
    selected: Vec<AgentSelectedMemory>,
    omissions: Vec<String>,
    used_bytes: usize,
    max_records: usize,
    max_bytes: usize,
    query_terms_count: usize,
    considered_records: usize,
    candidate_count: usize,
    filtered_sensitive: usize,
    filtered_archived: usize,
    omitted_by_budget: usize,
}

impl Default for AgentMemoryRuntimeContext {
    fn default() -> Self {
        Self {
            selected: Vec::new(),
            omissions: Vec::new(),
            used_bytes: 0,
            max_records: AGENT_MEMORY_CONTEXT_MAX_RECORDS,
            max_bytes: AGENT_MEMORY_CONTEXT_MAX_BYTES,
            query_terms_count: 0,
            considered_records: 0,
            candidate_count: 0,
            filtered_sensitive: 0,
            filtered_archived: 0,
            omitted_by_budget: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentSelectedMemory {
    id: Uuid,
    title: String,
    memory_type: MemoryType,
    scope: MemoryScope,
    match_reason: String,
    snippet: String,
    rank: usize,
    score: i32,
    inclusion_mode: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentChatReadiness {
    Ready,
    Missing,
    Unknown,
}

impl AgentChatReadiness {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Missing => "missing",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentChatResponse {
    pub id: Uuid,
    pub role: String,
    pub content: String,
    pub protocol_version: String,
    pub proposed_actions: Vec<AgentChatActionProposal>,
    pub missing_prerequisites: Vec<AgentChatMissingPrerequisite>,
    pub memory_candidates: Vec<MemoryCandidate>,
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
pub struct AgentChatActionProposal {
    pub action_type: String,
    pub title: Option<String>,
    pub reason: Option<String>,
    pub risk: Option<String>,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(
        default,
        alias = "url",
        alias = "href",
        alias = "path",
        alias = "command"
    )]
    pub target: Option<String>,
    #[serde(default, alias = "targetLocation", alias = "location")]
    pub target_location: Option<String>,
    #[serde(
        default,
        alias = "to",
        alias = "dest",
        alias = "destination",
        alias = "newPath",
        alias = "new_path",
        alias = "newName",
        alias = "new_name"
    )]
    pub destination: Option<String>,
    #[serde(default, alias = "preferredBrowser", alias = "browser")]
    pub preferred_browser: Option<String>,
    #[serde(
        default,
        alias = "body",
        alias = "text",
        alias = "input",
        deserialize_with = "deserialize_agent_action_content"
    )]
    pub content: Option<String>,
    pub capability: Option<CapabilityKind>,
    pub policy_decision: Option<PolicyDecision>,
    #[serde(default = "default_agent_action_execution_state")]
    pub execution_state: String,
    pub dispatch_note: Option<String>,
    pub permission_request_id: Option<Uuid>,
    pub capability_invocation_id: Option<Uuid>,
    pub workflow_run_id: Option<Uuid>,
    pub blocked_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentChatMissingPrerequisite {
    pub kind: String,
    #[serde(default, alias = "description", alias = "text")]
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppUpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub asset_name: Option<String>,
    pub release_url: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppUpdateDownloadResult {
    pub latest_version: String,
    pub asset_name: String,
    pub installer_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppUpdateInstallResult {
    pub installer_path: String,
    pub restart_scheduled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SilentUpdateInstallCommand {
    program: PathBuf,
    args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentChatMemoryCandidateProposal {
    #[serde(default)]
    pub title: String,
    #[serde(default, alias = "content", alias = "text")]
    pub body: String,
    #[serde(default, alias = "reason")]
    pub rationale: String,
    #[serde(default)]
    pub memory_type: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub sensitivity: Option<String>,
    #[serde(default)]
    pub lifecycle: Option<String>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentChatArtifactTargetProposal {
    #[serde(default, alias = "type", alias = "kind")]
    pub artifact_type: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default, alias = "path", alias = "file_path", alias = "relative_path")]
    pub target: String,
    #[serde(default, alias = "body", alias = "text")]
    pub content: String,
    #[serde(default, alias = "reason", alias = "description")]
    pub rationale: String,
    #[serde(default)]
    pub risk: Option<String>,
    #[serde(default)]
    pub requires_confirmation: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct AgentModelEnvelope {
    #[serde(
        default,
        alias = "version",
        deserialize_with = "deserialize_agent_protocol_version"
    )]
    protocol_version: Option<String>,
    #[serde(
        default,
        alias = "reply",
        alias = "user_reply",
        deserialize_with = "deserialize_agent_reply_to_user"
    )]
    reply_to_user: String,
    #[serde(default, alias = "proposed_actions")]
    agent_actions: Vec<AgentChatActionProposal>,
    #[serde(default)]
    workflow_calls: Vec<AgentChatActionProposal>,
    #[serde(default)]
    missing_prerequisites: Vec<AgentChatMissingPrerequisite>,
    #[serde(default)]
    artifact_targets: Vec<AgentChatArtifactTargetProposal>,
    #[serde(default)]
    memory_candidates: Vec<AgentChatMemoryCandidateProposal>,
}

fn deserialize_agent_reply_to_user<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(agent_reply_text_from_value(&value))
}

fn deserialize_agent_protocol_version<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Some(serde_json::to_string(&value).unwrap_or_default())
        }
    }))
}

fn agent_reply_text_from_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Object(object) => ["content", "text", "message", "body"]
            .iter()
            .find_map(|key| object.get(*key).and_then(serde_json::Value::as_str))
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn deserialize_agent_action_content<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Some(serde_json::to_string(&value).unwrap_or_default())
        }
    }))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentActionApprovalState {
    NoRequest,
    Pending,
    Approved(Uuid),
    Rejected,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerControlUnlockStatus {
    pub challenge: String,
    pub unlocked: bool,
    pub unlocked_until: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
struct ComputerControlUnlockState {
    challenge: String,
    unlocked_until: Option<DateTime<Utc>>,
}

enum AgentNetworkSearchClient {
    Bridge(CodexBridgeNetworkSearchClient),
    Http(HttpNetworkSearchClient),
    Unavailable(String),
}

enum AgentFileWriteClient {
    Local {
        file_client: LocalWorkspaceFileWriteClient,
        office_client: LocalOfficeArtifactClient,
    },
    Unavailable(String),
}

trait AgentWritableArtifactClient:
    FileWriteClient + OfficeArtifactClient + OfficeOpenClient + OfficeUpdateClient
{
}

impl<T> AgentWritableArtifactClient for T where
    T: FileWriteClient + OfficeArtifactClient + OfficeOpenClient + OfficeUpdateClient
{
}

impl NetworkSearchClient for AgentNetworkSearchClient {
    fn search(&self, query: &str, scope: &str) -> Result<NetworkSearchResult, String> {
        match self {
            Self::Bridge(client) => client.search(query, scope),
            Self::Http(client) => client.search(query, scope),
            Self::Unavailable(reason) => Err(reason.clone()),
        }
    }
}

impl FileWriteClient for AgentFileWriteClient {
    fn write_file(&self, path: &str, content: &str) -> Result<FileWriteResult, String> {
        match self {
            Self::Local { file_client, .. } => file_client.write_file(path, content),
            Self::Unavailable(reason) => Err(reason.clone()),
        }
    }
}

impl OfficeArtifactClient for AgentFileWriteClient {
    fn write_office_artifact(&self, spec: &OfficeCreateSpec) -> Result<OfficeCreateResult, String> {
        match self {
            Self::Local { office_client, .. } => office_client.write_office_artifact(spec),
            Self::Unavailable(reason) => Err(reason.clone()),
        }
    }
}

impl OfficeOpenClient for AgentFileWriteClient {
    fn open_office_artifact(
        &self,
        path: &str,
        preferred_app: Option<OfficeApp>,
    ) -> Result<OfficeOpenResult, String> {
        match self {
            Self::Local { office_client, .. } => {
                office_client.open_office_artifact(path, preferred_app)
            }
            Self::Unavailable(reason) => Err(reason.clone()),
        }
    }
}

impl OfficeUpdateClient for AgentFileWriteClient {
    fn update_office_artifact(
        &self,
        spec: &OfficeUpdateSpec,
    ) -> Result<OfficeUpdateResult, String> {
        match self {
            Self::Local { office_client, .. } => office_client.update_office_artifact(spec),
            Self::Unavailable(reason) => Err(reason.clone()),
        }
    }
}

fn agent_network_search_client(
    large_model_provider: LargeModelProvider,
    network_search_source_model: Option<NetworkSearchSourceModel>,
) -> AgentNetworkSearchClient {
    let tool_strategy = model_driven_tool_strategy_for_current_platform(
        large_model_provider,
        network_search_source_model,
    );

    match tool_strategy.network_search_backend {
        NetworkSearchBackend::NativeLargeModel => AgentNetworkSearchClient::Bridge(
            CodexBridgeNetworkSearchClient::from_env(large_model_provider),
        ),
        NetworkSearchBackend::SourceBackedModel | NetworkSearchBackend::DeepSeek => {
            match tool_strategy.network_search_source_model {
                Some(source_model) => match HttpNetworkSearchClient::new(source_model) {
                    Ok(client) => AgentNetworkSearchClient::Http(client),
                    Err(error) => AgentNetworkSearchClient::Unavailable(error),
                },
                None => AgentNetworkSearchClient::Unavailable(
                    "network search source model is not configured".to_string(),
                ),
            }
        }
    }
}

impl ComputerControlUnlockState {
    fn new(challenge: String) -> Self {
        Self {
            challenge,
            unlocked_until: None,
        }
    }

    fn generated() -> Self {
        Self::new(generate_computer_control_unlock_challenge())
    }

    fn status(&self, now: DateTime<Utc>) -> ComputerControlUnlockStatus {
        let unlocked_until = self
            .unlocked_until
            .as_ref()
            .filter(|until| **until > now)
            .cloned();
        ComputerControlUnlockStatus {
            challenge: self.challenge.clone(),
            unlocked: unlocked_until.is_some(),
            unlocked_until,
        }
    }

    fn unlock(
        &mut self,
        token: &str,
        now: DateTime<Utc>,
    ) -> Result<ComputerControlUnlockStatus, String> {
        if normalize_computer_control_unlock_token(token)
            != normalize_computer_control_unlock_token(&self.challenge)
        {
            return Err("invalid computer control unlock token".to_string());
        }

        self.unlocked_until = Some(now + Duration::minutes(COMPUTER_CONTROL_UNLOCK_TTL_MINUTES));
        Ok(self.status(now))
    }

    fn is_unlocked(&self, now: DateTime<Utc>) -> bool {
        self.unlocked_until
            .as_ref()
            .is_some_and(|unlocked_until| *unlocked_until > now)
    }
}

fn generate_computer_control_unlock_challenge() -> String {
    Uuid::new_v4()
        .simple()
        .to_string()
        .to_uppercase()
        .chars()
        .take(COMPUTER_CONTROL_UNLOCK_CHALLENGE_LENGTH)
        .collect()
}

fn normalize_computer_control_unlock_token(token: &str) -> String {
    token
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect()
}

fn should_require_computer_control_unlock(approval_granted: bool) -> bool {
    approval_granted
}

fn event_store_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn lock_error() -> String {
    "event store lock is unavailable".to_string()
}

fn computer_control_unlock_lock_error() -> String {
    "computer control unlock lock is unavailable".to_string()
}

fn current_work_package_tool_readiness(
    local_directories: LocalDirectoryReadinessStatus,
) -> WorkPackageToolReadiness {
    let deepseek = current_deepseek_credential_status();
    let foundation_state = FoundationState::default();
    let tool_strategy = model_driven_tool_strategy_for_current_platform(
        foundation_state.large_model_provider,
        foundation_state.network_search_source_model,
    );
    WorkPackageToolReadiness {
        network_search: network_search_route_status_for_strategy(
            &tool_strategy,
            deepseek.chat_completion_ready,
        ),
        deepseek,
        computer_use: computer_use_backend_status_for_strategy(&tool_strategy),
        local_directories,
        tool_strategy,
    }
}

fn normalize_release_version(version: &str) -> String {
    version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedReleaseVersion {
    core: Vec<u64>,
    prerelease: Option<ParsedPrereleaseVersion>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ParsedPrereleaseVersion {
    rank: u8,
    number: u64,
    label: String,
}

fn app_update_current_version() -> &'static str {
    match option_env!("DS_AGENT_RELEASE_TAG") {
        Some(value) if !value.is_empty() => value,
        _ => APP_UPDATE_CURRENT_RELEASE_TAG,
    }
}

fn parse_release_version(version: &str) -> Option<ParsedReleaseVersion> {
    let normalized = normalize_release_version(version).to_ascii_lowercase();
    let mut version_parts = normalized.splitn(2, '-');
    let core_text = version_parts.next()?.trim();
    if core_text.is_empty() {
        return None;
    }

    let mut core = Vec::new();
    for part in core_text.split('.') {
        if part.trim().is_empty() {
            core.push(0);
            continue;
        }
        core.push(part.parse::<u64>().ok()?);
    }
    while core.len() < 3 {
        core.push(0);
    }

    Some(ParsedReleaseVersion {
        core,
        prerelease: version_parts.next().and_then(parse_prerelease_version),
    })
}

fn parse_prerelease_version(value: &str) -> Option<ParsedPrereleaseVersion> {
    let tokens = value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let label = tokens
        .iter()
        .copied()
        .find(|token| {
            token
                .chars()
                .any(|character| character.is_ascii_alphabetic())
        })?
        .to_string();
    let number = tokens
        .iter()
        .copied()
        .find_map(|token| token.parse::<u64>().ok())
        .unwrap_or(0);
    let rank = match label.as_str() {
        "alpha" | "a" => 0,
        "beta" | "b" => 1,
        "rc" | "candidate" => 2,
        _ => 1,
    };

    Some(ParsedPrereleaseVersion {
        rank,
        number,
        label,
    })
}

fn compare_release_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let Some(left) = parse_release_version(left) else {
        return std::cmp::Ordering::Equal;
    };
    let Some(right) = parse_release_version(right) else {
        return std::cmp::Ordering::Equal;
    };

    let part_count = left.core.len().max(right.core.len());
    for index in 0..part_count {
        let left_part = *left.core.get(index).unwrap_or(&0);
        let right_part = *right.core.get(index).unwrap_or(&0);
        match left_part.cmp(&right_part) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }

    match (&left.prerelease, &right.prerelease) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(left), Some(right)) => left.cmp(right),
    }
}

fn is_newer_version(latest_version: &str, current_version: &str) -> bool {
    compare_release_versions(latest_version, current_version).is_gt()
}

fn is_windows_installer_asset(asset_name: &str) -> bool {
    let normalized = asset_name.to_ascii_lowercase();
    (normalized.ends_with(".exe") || normalized.ends_with(".msi"))
        && !normalized.contains("debug")
        && !normalized.contains("symbols")
}

fn release_asset_is_trusted(download_url: &str) -> bool {
    download_url.starts_with(APP_UPDATE_RELEASE_DOWNLOAD_PREFIX)
}

fn release_installable_asset(release: &GithubRelease) -> Option<&GithubReleaseAsset> {
    release.assets.iter().find(|asset| {
        is_windows_installer_asset(&asset.name)
            && release_asset_is_trusted(&asset.browser_download_url)
    })
}

fn app_update_http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(APP_UPDATE_USER_AGENT)
        .build()
        .map_err(|error| format!("failed to build update client: {error}"))
}

fn fetch_github_releases() -> Result<Vec<GithubRelease>, String> {
    app_update_http_client()?
        .get(APP_UPDATE_RELEASES_API_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .map_err(|error| format!("failed to check GitHub releases: {error}"))?
        .error_for_status()
        .map_err(|error| format!("GitHub releases check failed: {error}"))?
        .json::<Vec<GithubRelease>>()
        .map_err(|error| format!("failed to parse GitHub releases: {error}"))
}

fn sorted_releases_by_version(mut releases: Vec<GithubRelease>) -> Vec<GithubRelease> {
    releases.sort_by(|left, right| {
        compare_release_versions(&right.tag_name, &left.tag_name)
            .then_with(|| right.tag_name.cmp(&left.tag_name))
    });
    releases
}

fn update_status_from_releases(
    releases: Vec<GithubRelease>,
    current_version: &str,
) -> AppUpdateStatus {
    let releases = sorted_releases_by_version(releases);
    let latest_version = releases
        .first()
        .map(|release| normalize_release_version(&release.tag_name));
    let latest_update_release = releases.iter().find(|release| {
        is_newer_version(&release.tag_name, current_version)
            && release_installable_asset(release).is_some()
    });

    if let Some(release) = latest_update_release {
        let asset_name = release_installable_asset(release).map(|asset| asset.name.clone());
        return AppUpdateStatus {
            current_version: current_version.to_string(),
            latest_version: Some(normalize_release_version(&release.tag_name)),
            update_available: asset_name.is_some(),
            asset_name,
            release_url: Some(release.html_url.clone()),
            message: None,
        };
    }

    let has_newer_release = releases
        .iter()
        .any(|release| is_newer_version(&release.tag_name, current_version));
    AppUpdateStatus {
        current_version: current_version.to_string(),
        latest_version,
        update_available: false,
        asset_name: None,
        release_url: releases.first().map(|release| release.html_url.clone()),
        message: if has_newer_release {
            Some("latest release has no Windows installer asset".to_string())
        } else {
            None
        },
    }
}

#[cfg(test)]
fn update_status_from_release(release: GithubRelease) -> AppUpdateStatus {
    update_status_from_releases(vec![release], app_update_current_version())
}

fn latest_installable_update_release(
    releases: &[GithubRelease],
    current_version: &str,
) -> Option<GithubRelease> {
    let releases = sorted_releases_by_version(releases.to_vec());
    releases.into_iter().find(|release| {
        is_newer_version(&release.tag_name, current_version)
            && release_installable_asset(release).is_some()
    })
}

fn safe_update_asset_file_name(asset_name: &str) -> String {
    let sanitized: String = asset_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || character == '.'
                || character == '-'
                || character == '_'
            {
                character
            } else {
                '_'
            }
        })
        .collect();

    if sanitized.is_empty() {
        "ds-agent-update-installer.exe".to_string()
    } else {
        sanitized
    }
}

fn app_update_dir() -> PathBuf {
    std::env::temp_dir().join("ds-agent-updates")
}

fn download_release_asset(asset: &GithubReleaseAsset) -> Result<PathBuf, String> {
    if !release_asset_is_trusted(&asset.browser_download_url) {
        return Err("update asset URL is not trusted".to_string());
    }

    let file_name = safe_update_asset_file_name(&asset.name);
    let update_dir = app_update_dir();
    fs::create_dir_all(&update_dir)
        .map_err(|error| format!("failed to prepare update directory: {error}"))?;
    let installer_path = update_dir.join(file_name);
    let bytes = app_update_http_client()?
        .get(&asset.browser_download_url)
        .send()
        .map_err(|error| format!("failed to download update installer: {error}"))?
        .error_for_status()
        .map_err(|error| format!("update installer download failed: {error}"))?
        .bytes()
        .map_err(|error| format!("failed to read update installer: {error}"))?;
    fs::write(&installer_path, bytes)
        .map_err(|error| format!("failed to save update installer: {error}"))?;
    Ok(installer_path)
}

fn validate_downloaded_update_installer_path(installer_path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(installer_path);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "update installer path has no file name".to_string())?;
    if !is_windows_installer_asset(file_name) {
        return Err("downloaded update is not a Windows installer".to_string());
    }

    let update_dir = app_update_dir();
    fs::create_dir_all(&update_dir)
        .map_err(|error| format!("failed to prepare update directory: {error}"))?;
    let canonical_dir = fs::canonicalize(&update_dir)
        .map_err(|error| format!("failed to verify update directory: {error}"))?;
    let canonical_path = fs::canonicalize(&path)
        .map_err(|error| format!("downloaded update installer is unavailable: {error}"))?;
    if !canonical_path.starts_with(&canonical_dir) {
        return Err("downloaded update installer is outside the update directory".to_string());
    }

    Ok(canonical_path)
}

fn silent_update_install_command(installer_path: &Path) -> SilentUpdateInstallCommand {
    let extension = installer_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if extension == "msi" {
        return SilentUpdateInstallCommand {
            program: PathBuf::from("msiexec.exe"),
            args: vec![
                "/i".to_string(),
                installer_path.display().to_string(),
                "/quiet".to_string(),
                "/norestart".to_string(),
            ],
        };
    }

    SilentUpdateInstallCommand {
        program: installer_path.to_path_buf(),
        args: vec!["/S".to_string()],
    }
}

fn powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn app_update_runner_script(
    installer_path: &Path,
    app_path: &Path,
    current_process_id: u32,
) -> String {
    let install_command = silent_update_install_command(installer_path);
    let install_args = install_command
        .args
        .iter()
        .map(|argument| powershell_single_quoted(argument))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        concat!(
            "$ErrorActionPreference = 'Stop'\n",
            "$parentPid = {current_process_id}\n",
            "try {{ Wait-Process -Id $parentPid -Timeout 30 -ErrorAction SilentlyContinue }} catch {{ }}\n",
            "$process = Start-Process -FilePath {installer_program} -ArgumentList @({install_args}) -Wait -PassThru -WindowStyle Hidden\n",
            "if ($process.ExitCode -eq 0) {{\n",
            "  Start-Process -FilePath {app_path}\n",
            "}}\n"
        ),
        current_process_id = current_process_id,
        installer_program = powershell_single_quoted(&install_command.program.display().to_string()),
        install_args = install_args,
        app_path = powershell_single_quoted(&app_path.display().to_string()),
    )
}

#[cfg(windows)]
fn spawn_silent_update_runner(installer_path: &Path) -> Result<(), String> {
    let app_path =
        std::env::current_exe().map_err(|error| format!("failed to locate DS Agent: {error}"))?;
    let update_dir = app_update_dir();
    fs::create_dir_all(&update_dir)
        .map_err(|error| format!("failed to prepare update directory: {error}"))?;
    let runner_path = update_dir.join("install-and-restart-ds-agent.ps1");
    fs::write(
        &runner_path,
        app_update_runner_script(installer_path, &app_path, std::process::id()),
    )
    .map_err(|error| format!("failed to prepare silent update runner: {error}"))?;

    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(&runner_path)
        .creation_flags(WINDOWS_CREATE_NO_WINDOW);
    command
        .spawn()
        .map_err(|error| format!("failed to start silent update runner: {error}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn spawn_silent_update_runner(_installer_path: &Path) -> Result<(), String> {
    Err("silent app updates are only supported on Windows".to_string())
}

fn pending_memory_candidates_for_work_package(
    records: Vec<MemoryCandidateRecord>,
) -> Vec<MemoryCandidate> {
    records
        .into_iter()
        .filter(|record| record.effective_status == MemoryCandidateStatus::Pending)
        .map(|record| record.candidate)
        .collect()
}

fn link_existing_memory_records(
    store: &EventStore,
    source_memory_id: Uuid,
    target_memory_id: Uuid,
    relation: MemoryRelationKind,
    note: String,
) -> EventStoreResult<Vec<MemoryRecord>> {
    let link = MemoryRecordLink::new(source_memory_id, target_memory_id, None, relation, note)
        .map_err(EventStoreError::InvalidState)?;
    store.append_memory_record_link(&link)?;
    store.list_memory_records()
}

fn current_local_directory_readiness(
    app: &AppHandle,
) -> Result<LocalDirectoryReadinessStatus, String> {
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    let directory_state = load_local_directory_state(app_data_dir).map_err(event_store_error)?;
    Ok(local_directory_readiness_from_state(&directory_state))
}

fn computer_tool_strategy_for_command(
    large_model_provider: LargeModelProvider,
    network_search_source_model: Option<NetworkSearchSourceModel>,
) -> ModelDrivenToolStrategy {
    model_driven_tool_strategy_for_current_platform(
        large_model_provider,
        network_search_source_model,
    )
}

fn computer_screenshot_evidence_base_dir(
    app_data_dir: &Path,
    directory_state: &LocalDirectoryState,
) -> PathBuf {
    directory_state
        .settings
        .as_ref()
        .map(|settings| PathBuf::from(&settings.evidence_dir))
        .unwrap_or_else(|| app_data_dir.to_path_buf())
}

fn operations_briefing_report_export_dir(
    app_data_dir: &Path,
    directory_state: &LocalDirectoryState,
) -> PathBuf {
    directory_state
        .settings
        .as_ref()
        .map(|settings| PathBuf::from(&settings.export_dir))
        .unwrap_or_else(|| app_data_dir.to_path_buf())
}

fn operations_briefing_template_seed_dir(
    app_data_dir: &Path,
    directory_state: &LocalDirectoryState,
) -> PathBuf {
    directory_state
        .settings
        .as_ref()
        .map(|settings| PathBuf::from(&settings.evidence_dir))
        .unwrap_or_else(|| app_data_dir.join("operations-briefing-evidence"))
}

fn file_write_workspace_base_dir(
    app_data_dir: &Path,
    directory_state: &LocalDirectoryState,
) -> PathBuf {
    directory_state
        .settings
        .as_ref()
        .map(|settings| PathBuf::from(&settings.workspace_dir))
        .unwrap_or_else(|| app_data_dir.join("workspace"))
}

fn agent_file_write_client(
    directory_state: &LocalDirectoryState,
    desktop_dir: Option<PathBuf>,
) -> Result<AgentFileWriteClient, String> {
    let Some(settings) = directory_state.settings.as_ref() else {
        return Ok(AgentFileWriteClient::Unavailable(
            "workspace is not configured; choose a DS Agent work root before creating files"
                .to_string(),
        ));
    };

    if directory_state.needs_setup {
        return Ok(AgentFileWriteClient::Unavailable(
            "workspace setup is incomplete; choose a DS Agent work root before creating files"
                .to_string(),
        ));
    }

    let workspace_dir = PathBuf::from(&settings.workspace_dir);
    std::fs::create_dir_all(&workspace_dir).map_err(event_store_error)?;
    Ok(AgentFileWriteClient::Local {
        file_client: LocalWorkspaceFileWriteClient::new(workspace_dir.clone(), 512 * 1024),
        office_client: LocalOfficeArtifactClient::new_with_desktop_dir(
            workspace_dir,
            8 * 1024 * 1024,
            desktop_dir,
        ),
    })
}

fn operations_briefing_deepseek_api_key_for_provider(
    read_env: impl Fn(&str) -> Option<String>,
    large_model_provider: LargeModelProvider,
) -> Option<String> {
    if large_model_provider != LargeModelProvider::DeepSeek {
        return None;
    }

    read_env(DEEPSEEK_API_KEY_ENV)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn deepseek_telemetry_with_pricing(
    mut telemetry: Vec<DeepSeekChatTelemetry>,
    pricing_settings: Option<&DeepSeekPricingSettings>,
) -> Vec<DeepSeekChatTelemetry> {
    let Some(pricing_settings) = pricing_settings else {
        return telemetry;
    };

    for entry in &mut telemetry {
        entry.estimated_cost_micro_usd =
            estimate_deepseek_chat_cost_micro_usd(entry, pricing_settings);
    }

    telemetry
}

fn operations_briefing_model_route_context(
    provider: LargeModelProvider,
    model_route: ModelRoute,
) -> String {
    format!(
        "{} / {}",
        large_model_provider_context_label(provider),
        model_route_context_label(model_route)
    )
}

fn large_model_provider_context_label(provider: LargeModelProvider) -> &'static str {
    match provider {
        LargeModelProvider::DeepSeek => "deepseek",
        LargeModelProvider::ChatGpt => "chatgpt",
        LargeModelProvider::Codex => "codex",
        LargeModelProvider::Custom => "custom",
    }
}

fn model_route_context_label(model_route: ModelRoute) -> &'static str {
    match model_route {
        ModelRoute::Auto => "auto",
        ModelRoute::Flash => "flash",
        ModelRoute::Pro => "pro",
    }
}

fn thinking_level_context_label(thinking_level: ThinkingLevel) -> &'static str {
    match thinking_level {
        ThinkingLevel::Auto => "auto",
        ThinkingLevel::Fast => "fast",
        ThinkingLevel::Standard => "standard",
        ThinkingLevel::Deep => "deep",
    }
}

fn operations_briefing_token_cache_context(telemetry: &[DeepSeekChatTelemetry]) -> String {
    if telemetry.is_empty() {
        return "no DeepSeek request recorded".to_string();
    }

    telemetry
        .iter()
        .map(|entry| {
            let tokens = entry
                .total_tokens
                .map(|tokens| format!("{tokens} total tokens"))
                .unwrap_or_else(|| "token usage unavailable".to_string());
            format!(
                "{} cache {}, {}, {} ms",
                entry.model,
                deepseek_cache_status_context_label(entry.cache_status),
                tokens,
                entry.elapsed_ms
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn deepseek_cache_status_context_label(status: DeepSeekChatCacheStatus) -> &'static str {
    match status {
        DeepSeekChatCacheStatus::Disabled => "disabled",
        DeepSeekChatCacheStatus::Hit => "hit",
        DeepSeekChatCacheStatus::Miss => "miss",
    }
}

fn agent_chat_api_key_from_env(read_env: impl Fn(&str) -> Option<String>) -> Option<String> {
    read_env(DEEPSEEK_API_KEY_ENV).and_then(|value| normalize_agent_chat_api_key(Some(value)))
}

fn normalize_agent_chat_api_key(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub fn agent_chat_api_key_from_sources(
    session_api_key: Option<String>,
    read_env: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    normalize_agent_chat_api_key(session_api_key).or_else(|| agent_chat_api_key_from_env(read_env))
}

fn push_unique_agent_chat_api_key(candidates: &mut Vec<String>, value: Option<String>) {
    if let Some(api_key) = normalize_agent_chat_api_key(value) {
        if !candidates.iter().any(|candidate| candidate == &api_key) {
            candidates.push(api_key);
        }
    }
}

pub fn agent_chat_api_key_candidates_from_sources(
    primary_api_key: Option<String>,
    fallback_api_key: Option<String>,
    read_env: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_agent_chat_api_key(&mut candidates, primary_api_key);
    push_unique_agent_chat_api_key(&mut candidates, fallback_api_key);
    push_unique_agent_chat_api_key(&mut candidates, read_env(DEEPSEEK_API_KEY_ENV));
    candidates
}

fn agent_chat_response_from_telemetry(
    content: String,
    telemetry: &DeepSeekChatTelemetry,
    access_mode: AccessMode,
) -> AgentChatResponse {
    let parsed_envelope = parse_agent_model_envelope(&content, access_mode);
    let display_content = parsed_envelope
        .as_ref()
        .map(|envelope| envelope.reply_to_user.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(content.trim())
        .to_string();
    let protocol_version = parsed_envelope
        .as_ref()
        .and_then(|envelope| envelope.protocol_version.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("plain-text")
        .to_string();
    let mut memory_candidate_proposals = parsed_envelope
        .as_ref()
        .map(|envelope| envelope.memory_candidates.clone())
        .unwrap_or_default();
    let proposed_actions = parsed_envelope
        .as_ref()
        .map(|envelope| {
            envelope
                .agent_actions
                .iter()
                .filter_map(|action| {
                    if action.action_type == "memory_candidate" {
                        memory_candidate_proposals
                            .push(agent_memory_candidate_proposal_from_action(action));
                        None
                    } else {
                        Some(action.clone())
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let missing_prerequisites = parsed_envelope
        .as_ref()
        .map(|envelope| envelope.missing_prerequisites.clone())
        .unwrap_or_default();
    let memory_candidates = parsed_envelope
        .as_ref()
        .map(|_| agent_memory_candidates_from_model(&memory_candidate_proposals))
        .unwrap_or_default();

    AgentChatResponse {
        id: Uuid::new_v4(),
        role: "assistant".to_string(),
        content: display_content,
        protocol_version,
        proposed_actions,
        missing_prerequisites,
        memory_candidates,
        model: telemetry.model.clone(),
        cache_status: telemetry.cache_status,
        elapsed_ms: telemetry.elapsed_ms,
        prompt_tokens: telemetry.prompt_tokens,
        completion_tokens: telemetry.completion_tokens,
        total_tokens: telemetry.total_tokens,
        estimated_cost_micro_usd: telemetry.estimated_cost_micro_usd,
        created_at: Utc::now(),
    }
}

fn agent_memory_candidate_proposal_from_action(
    action: &AgentChatActionProposal,
) -> AgentChatMemoryCandidateProposal {
    AgentChatMemoryCandidateProposal {
        title: action
            .title
            .as_deref()
            .or(action.target.as_deref())
            .unwrap_or("DS Agent chat memory")
            .to_string(),
        body: action
            .content
            .as_deref()
            .or(action.target.as_deref())
            .unwrap_or_default()
            .to_string(),
        rationale: action
            .reason
            .as_deref()
            .unwrap_or("DeepSeek proposed this memory candidate as an agent action.")
            .to_string(),
        memory_type: None,
        scope: None,
        sensitivity: None,
        lifecycle: None,
        expires_at: None,
    }
}

fn agent_memory_candidates_from_model(
    proposals: &[AgentChatMemoryCandidateProposal],
) -> Vec<MemoryCandidate> {
    proposals
        .iter()
        .filter_map(|proposal| {
            let title = proposal.title.trim();
            let body = proposal.body.trim();
            if title.is_empty() || body.is_empty() {
                return None;
            }

            let lifecycle = agent_memory_lifecycle_from_model(proposal.lifecycle.as_deref());
            let expires_at = proposal
                .expires_at
                .filter(|_| lifecycle == MemoryLifecycle::Expires);
            let lifecycle = if lifecycle == MemoryLifecycle::Expires && expires_at.is_none() {
                MemoryLifecycle::Active
            } else {
                lifecycle
            };
            let rationale = proposal
                .rationale
                .trim()
                .to_string()
                .if_empty_then("DeepSeek proposed this as a reviewable chat memory candidate.");

            MemoryCandidate::new_with_metadata_and_expiration(
                title.to_string(),
                body.to_string(),
                MemoryCandidateSource::WorkflowReflection,
                None,
                rationale,
                agent_memory_type_from_model(proposal.memory_type.as_deref()),
                agent_memory_scope_from_model(proposal.scope.as_deref()),
                agent_memory_sensitivity_from_model(proposal.sensitivity.as_deref()),
                lifecycle,
                expires_at,
            )
            .ok()
        })
        .collect()
}

trait EmptyStringFallback {
    fn if_empty_then(self, fallback: &str) -> String;
}

impl EmptyStringFallback for String {
    fn if_empty_then(self, fallback: &str) -> String {
        if self.trim().is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

fn agent_memory_type_from_model(value: Option<&str>) -> MemoryType {
    match normalized_agent_memory_field(value).as_deref() {
        Some("project_context") => MemoryType::ProjectContext,
        Some("workflow_rule") => MemoryType::WorkflowRule,
        Some("artifact") => MemoryType::Artifact,
        Some("failure_pattern") => MemoryType::FailurePattern,
        _ => MemoryType::Preference,
    }
}

fn agent_memory_scope_from_model(value: Option<&str>) -> MemoryScope {
    match normalized_agent_memory_field(value).as_deref() {
        Some("project") => MemoryScope::Project,
        Some("organization") => MemoryScope::Organization,
        Some("user") => MemoryScope::User,
        _ => MemoryScope::Workspace,
    }
}

fn agent_memory_sensitivity_from_model(value: Option<&str>) -> MemorySensitivity {
    match normalized_agent_memory_field(value).as_deref() {
        Some("sensitive") => MemorySensitivity::Sensitive,
        _ => MemorySensitivity::Normal,
    }
}

fn agent_memory_lifecycle_from_model(value: Option<&str>) -> MemoryLifecycle {
    match normalized_agent_memory_field(value).as_deref() {
        Some("archived") => MemoryLifecycle::Archived,
        Some("expires") => MemoryLifecycle::Expires,
        _ => MemoryLifecycle::Active,
    }
}

fn normalized_agent_memory_field(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase().replace(['-', ' '], "_"))
}

fn parse_agent_model_envelope(
    content: &str,
    access_mode: AccessMode,
) -> Option<AgentModelEnvelope> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    parse_agent_model_envelope_json(trimmed, access_mode).or_else(|| {
        extract_markdown_fenced_json(trimmed)
            .and_then(|json_text| parse_agent_model_envelope_json(json_text, access_mode))
    })
}

fn parse_agent_model_envelope_json(
    json_text: &str,
    access_mode: AccessMode,
) -> Option<AgentModelEnvelope> {
    serde_json::from_str::<AgentModelEnvelope>(json_text)
        .ok()
        .map(|mut envelope| {
            let agent_actions = std::mem::take(&mut envelope.agent_actions);
            let workflow_calls = std::mem::take(&mut envelope.workflow_calls);
            let artifact_targets = std::mem::take(&mut envelope.artifact_targets);
            envelope.agent_actions = agent_actions
                .into_iter()
                .chain(workflow_calls)
                .chain(
                    artifact_targets
                        .into_iter()
                        .map(agent_action_from_artifact_target),
                )
                .map(|action| normalize_agent_action_proposal(action, access_mode))
                .collect();
            envelope
        })
        .filter(|envelope| !envelope.reply_to_user.trim().is_empty())
}

fn agent_action_from_artifact_target(
    artifact: AgentChatArtifactTargetProposal,
) -> AgentChatActionProposal {
    let target = artifact
        .target
        .trim()
        .to_string()
        .if_empty_then(&default_agent_artifact_target(&artifact));
    let content = artifact.content.trim().to_string();
    let title = artifact
        .title
        .trim()
        .to_string()
        .if_empty_then("Create artifact");
    let reason = artifact.rationale.trim().to_string().if_empty_then(
        "DeepSeek proposed this artifact target for DS Agent to validate and create.",
    );
    let action_type = if normalized_agent_memory_field(artifact.artifact_type.as_deref())
        .is_some_and(|artifact_type| artifact_type.contains("report"))
        || target.starts_with("reports/")
        || target.starts_with("reports\\")
    {
        "create_report"
    } else {
        "file_write"
    };

    AgentChatActionProposal {
        action_type: action_type.to_string(),
        title: Some(title),
        reason: Some(reason),
        risk: artifact.risk,
        requires_confirmation: artifact.requires_confirmation,
        target: Some(target),
        target_location: None,
        destination: None,
        preferred_browser: None,
        content: Some(content),
        capability: None,
        policy_decision: None,
        execution_state: default_agent_action_execution_state(),
        dispatch_note: None,
        permission_request_id: None,
        capability_invocation_id: None,
        workflow_run_id: None,
        blocked_reason: None,
    }
}

fn default_agent_artifact_target(artifact: &AgentChatArtifactTargetProposal) -> String {
    let slug = artifact
        .title
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_lowercase())
            } else if character.is_ascii_whitespace() || matches!(character, '-' | '_') {
                Some('-')
            } else {
                None
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
        .if_empty_then("ds-agent-artifact");
    format!("reports/{slug}-{}.md", Uuid::new_v4().simple())
}

fn extract_markdown_fenced_json(content: &str) -> Option<&str> {
    let fence_start = content.find("```")?;
    let after_opening_fence = &content[fence_start + 3..];
    let first_newline = after_opening_fence.find('\n')?;
    let fence_label = after_opening_fence[..first_newline].trim();
    if !fence_label.is_empty() && !fence_label.eq_ignore_ascii_case("json") {
        return None;
    }

    let after_label = &after_opening_fence[first_newline + 1..];
    let fence_end = after_label.rfind("```")?;
    let json_text = after_label[..fence_end].trim();
    if json_text.starts_with('{') && json_text.ends_with('}') {
        Some(json_text)
    } else {
        None
    }
}

fn default_agent_action_execution_state() -> String {
    "proposed".to_string()
}

fn normalize_agent_action_proposal(
    mut action: AgentChatActionProposal,
    access_mode: AccessMode,
) -> AgentChatActionProposal {
    action.action_type = normalize_agent_action_type(&action.action_type);
    normalize_agent_action_alias(&mut action);
    action.preferred_browser =
        normalize_agent_action_browser_preference(action.preferred_browser.as_deref())
            .or_else(|| infer_agent_action_browser_preference(&action));
    action.target_location =
        normalize_agent_action_target_location(action.target_location.as_deref());
    action.capability = agent_action_capability(&action.action_type);
    action.policy_decision = action.capability.map(|capability| {
        agent_action_policy_decision(access_mode, &action.action_type, capability)
    });
    let risk_requires_confirmation = action_risk_requires_confirmation(action.risk.as_deref())
        && !is_agent_filesystem_mutation_action(&action.action_type);

    action.execution_state = if !is_supported_agent_action_type(&action.action_type) {
        action.blocked_reason = Some(unsupported_agent_action_reason(&action.action_type));
        "blocked".to_string()
    } else if matches!(action.policy_decision, Some(PolicyDecision::Ask))
        || action.requires_confirmation
        || risk_requires_confirmation
    {
        action.dispatch_note =
            Some("local capability policy requires confirmation before dispatch".to_string());
        "needs_confirmation".to_string()
    } else {
        action.dispatch_note =
            Some("local capability policy allows this proposal to enter dispatch".to_string());
        "proposed".to_string()
    };
    action
}

fn normalize_agent_action_type(action_type: &str) -> String {
    action_type
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| match character {
            '-' | ' ' => '_',
            other => other,
        })
        .collect()
}

fn normalize_agent_action_alias(action: &mut AgentChatActionProposal) {
    if normalize_agent_filesystem_mutation_alias(action) {
        return;
    }

    if is_office_create_action_type(&action.action_type) {
        action.action_type = "office_create".to_string();
        action.risk = Some("low".to_string());
        action.requires_confirmation = false;
        return;
    }

    if is_office_open_action_type(&action.action_type) {
        action.action_type = "office_open".to_string();
        action.risk.get_or_insert_with(|| "low".to_string());
        action.requires_confirmation = false;
        return;
    }

    if is_office_update_action_type(&action.action_type) {
        action.action_type = "office_update".to_string();
        action.risk.get_or_insert_with(|| "medium".to_string());
        return;
    }

    if action.action_type == "browser_browse" && is_plain_browser_open_action(action) {
        if let Some(url) = infer_agent_action_browser_url(action) {
            action.action_type = "browser_open".to_string();
            action.target = Some(url);
            action.risk = Some("low".to_string());
            action.requires_confirmation = false;
        }
        return;
    }

    if matches!(
        action.action_type.as_str(),
        "browser_open" | "open_browser" | "open_url" | "open_website"
    ) {
        if let Some(url) = infer_agent_action_browser_url(action) {
            action.action_type = "browser_open".to_string();
            action.target = Some(url);
            action.risk = Some("low".to_string());
            action.requires_confirmation = false;
        }
        return;
    }

    if matches!(
        action.action_type.as_str(),
        "browse_url" | "visit_url" | "web_browse"
    ) {
        if let Some(url) = infer_agent_action_browser_url(action) {
            action.action_type = "browser_browse".to_string();
            action.target = Some(url);
        }
        return;
    }

    if action.action_type == "run_shell" {
        if let Some(url) = infer_agent_action_browser_url(action) {
            action.action_type = "browser_open".to_string();
            action.target = Some(url);
            action.risk = Some("low".to_string());
            action.requires_confirmation = false;
        }
    }
}

fn normalize_agent_filesystem_mutation_alias(action: &mut AgentChatActionProposal) -> bool {
    let normalized = match action.action_type.as_str() {
        "file_create" | "create_file" | "new_file" | "write_file" => "file_create",
        "file_update" | "update_file" | "modify_file" | "edit_file" | "overwrite_file" => {
            "file_update"
        }
        "file_delete" | "delete_file" | "remove_file" => "file_delete",
        "file_rename" | "rename_file" | "move_file" | "file_move" => "file_rename",
        "directory_create" | "create_directory" | "create_folder" | "new_directory"
        | "new_folder" | "mkdir" | "make_directory" => "directory_create",
        "directory_rename" | "rename_directory" | "rename_folder" | "move_directory"
        | "move_folder" | "directory_move" | "folder_move" => "directory_rename",
        "directory_delete" | "delete_directory" | "delete_folder" | "remove_directory"
        | "remove_folder" | "rmdir" => "directory_delete",
        _ => return false,
    };

    action.action_type = normalized.to_string();
    action.requires_confirmation = false;
    action.risk.get_or_insert_with(|| {
        if matches!(
            normalized,
            "file_delete" | "file_rename" | "directory_delete" | "directory_rename"
        ) {
            "medium"
        } else {
            "low"
        }
        .to_string()
    });
    true
}

fn agent_action_policy_decision(
    access_mode: AccessMode,
    action_type: &str,
    capability: CapabilityKind,
) -> PolicyDecision {
    if action_type == "office_create" {
        return match access_mode {
            AccessMode::AskEveryStep => PolicyDecision::Ask,
            AccessMode::AskOnRisk | AccessMode::LimitedAuto | AccessMode::FullAccess => {
                PolicyDecision::Allow
            }
        };
    }

    decide_capability_policy(access_mode, capability)
}

fn normalize_agent_action_target_location(value: Option<&str>) -> Option<String> {
    let normalized = value?.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match normalized.as_str() {
        "desktop" | "user_desktop" | "windows_desktop" | "桌面" => Some("desktop".to_string()),
        "workspace" | "workdir" | "work_dir" | "work_root" | "local_workspace" | "工作区" => {
            Some("workspace".to_string())
        }
        _ => None,
    }
}

fn apply_agent_action_target_location(target: &str, target_location: Option<&str>) -> String {
    let normalized_target = target.trim().replace('\\', "/");
    if normalized_target.starts_with("desktop/") || normalized_target.starts_with("workspace/") {
        return normalized_target;
    }
    match normalize_agent_action_target_location(target_location).as_deref() {
        Some("desktop") => format!("desktop/{normalized_target}"),
        Some("workspace") | None => normalized_target,
        Some(_) => normalized_target,
    }
}

fn is_office_open_action_type(action_type: &str) -> bool {
    matches!(
        action_type,
        "office_open"
            | "open_office"
            | "word_open"
            | "open_word"
            | "open_word_document"
            | "excel_open"
            | "open_excel"
            | "open_excel_workbook"
            | "powerpoint_open"
            | "open_powerpoint"
            | "open_powerpoint_deck"
            | "ppt_open"
            | "open_ppt"
    )
}

fn is_office_update_action_type(action_type: &str) -> bool {
    matches!(
        action_type,
        "office_update"
            | "update_office"
            | "office_edit"
            | "edit_office"
            | "word_update"
            | "update_word"
            | "edit_word"
            | "append_word"
            | "excel_update"
            | "update_excel"
            | "edit_excel"
            | "append_excel"
            | "powerpoint_update"
            | "update_powerpoint"
            | "edit_powerpoint"
            | "append_powerpoint"
            | "ppt_update"
            | "update_ppt"
            | "edit_ppt"
            | "append_ppt"
    )
}

fn is_office_create_action_type(action_type: &str) -> bool {
    matches!(
        action_type,
        "office_create"
            | "create_office"
            | "word_create"
            | "create_word"
            | "create_word_document"
            | "excel_create"
            | "create_excel"
            | "create_excel_workbook"
            | "powerpoint_create"
            | "create_powerpoint"
            | "create_powerpoint_deck"
            | "ppt_create"
            | "create_ppt"
    )
}

fn is_plain_browser_open_action(action: &AgentChatActionProposal) -> bool {
    let text = [
        action.title.as_deref(),
        action.reason.as_deref(),
        action.content.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();

    let has_open_intent = [
        "open",
        "launch",
        "chrome",
        "browser",
        "打开",
        "启动",
        "浏览器",
        "首页",
    ]
    .iter()
    .any(|marker| text.contains(marker));
    let has_read_intent = [
        "read",
        "inspect",
        "extract",
        "summarize",
        "evidence",
        "content",
        "source",
        "fetch",
        "读取",
        "检查",
        "提取",
        "总结",
        "证据",
        "内容",
        "来源",
        "检索",
    ]
    .iter()
    .any(|marker| text.contains(marker));

    has_open_intent && !has_read_intent
}

fn normalize_agent_action_browser_preference(value: Option<&str>) -> Option<String> {
    let value = value?.trim().to_ascii_lowercase();
    if value.contains("chrome") || value.contains("google chrome") || value.contains("谷歌") {
        Some("chrome".to_string())
    } else {
        None
    }
}

fn infer_agent_action_browser_preference(action: &AgentChatActionProposal) -> Option<String> {
    if action.action_type != "browser_open" {
        return None;
    }

    let text = [
        action.title.as_deref(),
        action.reason.as_deref(),
        action.target.as_deref(),
        action.content.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    normalize_agent_action_browser_preference(Some(&text))
}

fn infer_agent_action_browser_url(action: &AgentChatActionProposal) -> Option<String> {
    [
        action.target.as_deref(),
        action.content.as_deref(),
        action.title.as_deref(),
        action.reason.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(extract_safe_browser_url)
}

fn extract_safe_browser_url(text: &str) -> Option<String> {
    text.split(|character: char| {
        character.is_whitespace()
            || matches!(
                character,
                '"' | '\'' | '`' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}'
            )
    })
    .filter_map(normalize_browser_url_candidate)
    .find(|url| is_http_browser_url(url))
}

fn normalize_browser_url_candidate(raw: &str) -> Option<String> {
    let candidate = raw.trim_matches(|character: char| {
        matches!(
            character,
            ',' | ';'
                | ':'
                | '!'
                | '?'
                | '"'
                | '\''
                | '`'
                | '<'
                | '>'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
        )
    });
    let candidate = candidate.trim_end_matches('.').trim();
    if candidate.is_empty() {
        return None;
    }

    let lower = candidate.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(candidate.to_string());
    }

    if looks_like_public_domain(candidate) {
        return Some(format!("https://{candidate}"));
    }

    None
}

fn looks_like_public_domain(candidate: &str) -> bool {
    let without_scheme = candidate
        .trim_start_matches("www.")
        .split('/')
        .next()
        .unwrap_or(candidate);
    without_scheme.contains('.')
        && without_scheme
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '.'))
        && without_scheme
            .split('.')
            .all(|part| !part.is_empty() && part.len() <= 63)
        && without_scheme.rsplit('.').next().is_some_and(|top_level| {
            top_level.len() >= 2
                && top_level
                    .chars()
                    .all(|character| character.is_ascii_alphabetic())
        })
}

fn is_http_browser_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

fn unsupported_agent_action_reason(action_type: &str) -> String {
    if action_type == "run_shell" {
        "unsupported action_type `run_shell` from model response; DS Agent does not execute arbitrary shell commands from chat, and URL-opening shell proposals must include an http:// or https:// target so they can be routed through browser_open".to_string()
    } else {
        format!("unsupported action_type `{action_type}` from model response")
    }
}

fn agent_filesystem_mutation_operation(action_type: &str) -> Option<FileSystemMutationOperation> {
    match action_type {
        "file_create" => Some(FileSystemMutationOperation::CreateFile),
        "file_update" => Some(FileSystemMutationOperation::UpdateFile),
        "file_delete" => Some(FileSystemMutationOperation::DeleteFile),
        "file_rename" => Some(FileSystemMutationOperation::RenameFile),
        "directory_create" => Some(FileSystemMutationOperation::CreateDirectory),
        "directory_rename" => Some(FileSystemMutationOperation::RenameDirectory),
        "directory_delete" => Some(FileSystemMutationOperation::DeleteDirectory),
        _ => None,
    }
}

fn is_agent_filesystem_mutation_action(action_type: &str) -> bool {
    agent_filesystem_mutation_operation(action_type).is_some()
}

fn agent_action_capability(action_type: &str) -> Option<CapabilityKind> {
    match action_type {
        "browser_open" => Some(CapabilityKind::BrowserBrowse),
        "browser_browse" => Some(CapabilityKind::BrowserBrowse),
        "computer_control" => Some(CapabilityKind::ComputerControl),
        "computer_screenshot" => Some(CapabilityKind::ComputerScreenshot),
        "drive_read" => Some(CapabilityKind::DriveRead),
        "drive_write" => Some(CapabilityKind::DriveWrite),
        "email_draft" => Some(CapabilityKind::EmailDraft),
        "email_send" => Some(CapabilityKind::EmailSend),
        "file_read" => Some(CapabilityKind::FileRead),
        "file_write" | "create_report" => Some(CapabilityKind::FileWrite),
        _ if is_agent_filesystem_mutation_action(action_type) => Some(CapabilityKind::FileWrite),
        "network_search" => Some(CapabilityKind::NetworkSearch),
        "office_open" => Some(CapabilityKind::FileRead),
        "office_create" => Some(CapabilityKind::FileWrite),
        "office_update" => Some(CapabilityKind::FileWrite),
        "terminal_read" => Some(CapabilityKind::TerminalRead),
        "terminal_write" => Some(CapabilityKind::TerminalWrite),
        "work_package_export" => Some(CapabilityKind::DriveWrite),
        _ => None,
    }
}

fn is_supported_agent_action_type(action_type: &str) -> bool {
    matches!(
        action_type,
        "browser_open"
            | "browser_browse"
            | "computer_control"
            | "computer_screenshot"
            | "create_report"
            | "deepseek_key_setup"
            | "drive_read"
            | "drive_write"
            | "email_draft"
            | "email_send"
            | "file_read"
            | "file_write"
            | "file_create"
            | "file_update"
            | "file_delete"
            | "file_rename"
            | "directory_create"
            | "directory_rename"
            | "directory_delete"
            | "memory_candidate"
            | "network_search"
            | "office_create"
            | "office_open"
            | "office_update"
            | "operations_briefing"
            | "search_setup"
            | "terminal_read"
            | "terminal_write"
            | "work_package_export"
            | "workspace_setup"
    )
}

fn action_risk_requires_confirmation(risk: Option<&str>) -> bool {
    risk.map(str::trim)
        .map(|value| {
            value.eq_ignore_ascii_case("medium")
                || value.eq_ignore_ascii_case("high")
                || value.eq_ignore_ascii_case("critical")
        })
        .unwrap_or(false)
}

fn build_agent_chat_protocol_user_prompt(
    request: &AgentChatRequest,
    runtime_context: &AgentChatRuntimeContext,
) -> String {
    let source_model = runtime_context
        .network_search_source_model
        .map(|model| serialize_agent_chat_context_value(&model))
        .unwrap_or_else(|| "none".to_string());
    let soul_profile = build_agent_soul_profile_prompt(runtime_context.soul_profile.as_ref());
    let memory_context = build_agent_memory_context_prompt(&runtime_context.memory_context);
    let runtime_memory_sections = [soul_profile, memory_context]
        .into_iter()
        .filter(|section| !section.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let runtime_memory_section = if runtime_memory_sections.is_empty() {
        String::new()
    } else {
        format!("\n\n{runtime_memory_sections}\n")
    };
    format!(
        "DS Agent protocol context:\n\
         - DS Agent is the deterministic local execution layer.\n\
         - DeepSeek is responsible for natural-language understanding, planning, drafting, summarization, classification, and judgment.\n\
         - model_route={model_route}\n\
         - thinking_level={thinking_level}\n\
         - access_mode={access_mode}\n\
         - workspace_ready={workspace_ready}; note={workspace_note}\n\
         - network_search_ready={network_search_ready}; source_model={source_model}; note={network_search_note}\n\
	         - If current web information is needed and network_search_ready is not ready, return missing_prerequisites with kind network_search instead of pretending web evidence exists.\n\
	         - If local files, artifacts, or workflow state are needed and workspace_ready is not ready, return missing_prerequisites with kind workspace before proposing local file actions.\n\
	         - Loop engineering boundary: DS Agent optimizes for the shortest reliable loop that satisfies the user's real goal. Do not keep refining after the done_when conditions are met.\n\
	         - Build an internal goal_contract before answering: user_goal, constraints, done_when, completion_verifier, evidence_needed, and disallowed near-miss results. Use it to avoid returning a similar-looking result that does not complete the user's actual objective.\n\
	         - completion_verifier: a task is complete only when DS Agent can observe local state, tool evidence, source evidence, test output, or another concrete result that satisfies done_when. Model text alone is not completion evidence for local/browser/file/Office/tool tasks.\n\
	         - stop_conditions: stop when the verifier passes; pause for missing_prerequisites or user confirmation; switch strategy after the same failure repeats; stop and report a blocker instead of looping when no new evidence or progress is produced.\n\
	         - near-miss guard: do not substitute advice, explanation, a draft, or a merely related answer when the user asked DS Agent to create, open, edit, inspect, verify, or otherwise complete a concrete outcome.\n\
	         - completion_advice: when a result is complete or partially complete, end with at most one short next-better suggestion grounded in the task. Keep it secondary and do not imply extra work already ran.\n\
	         - Return exactly one structured agent envelope as JSON.\n\
	         - Required JSON fields: protocol_version, reply_to_user, agent_actions, missing_prerequisites.\n\
         - Supported agent_actions action_type values are browser_open, browser_browse, computer_control, computer_screenshot, file_read, file_write, file_create, file_update, file_delete, file_rename, directory_create, directory_rename, directory_delete, create_report, office_create, office_update, office_open, network_search, operations_briefing, terminal_read, terminal_write, workspace_setup, deepseek_key_setup, and search_setup.\n\
         - Do not use run_shell. For opening a website in the user's browser, use action_type browser_open with target set to the exact http:// or https:// URL. For reading or inspecting a web page as evidence, use browser_browse. If the user asked to log in, open the site only and ask the user to enter credentials manually.\n\
         - For local directory listing requests, use exactly one terminal_read action with target set to the exact local folder path. DS Agent will run a bounded non-recursive directory listing without executing arbitrary shell. Do not add a second action to read terminal output.\n\
         - For Windows local filesystem create, update, delete, or rename requests, use file_create, file_update, file_delete, file_rename, directory_create, directory_rename, or directory_delete with exact absolute local paths. Use content for file_create and file_update. Use destination for file_rename and directory_rename. Do not use run_shell for Windows file or directory mutation.\n\
         - browser_open may include preferred_browser=chrome only when the user explicitly asks for Chrome. DS Agent will fall back to the system default browser if Chrome is unavailable.\n\
         - For Word, Excel, and PowerPoint creation requests, prefer the built-in Office control plugin path: use action_type office_create and let DS Agent generate a real .docx, .xlsx, or .pptx before using desktop UI control. If the user asks for the Desktop, set target_location=\"desktop\" and target to the file name or desktop-relative path. If the user asks for the DS Agent workspace, set target_location=\"workspace\" or omit it. Do not infer or hide the location: express the user's location intent in target_location. Set content to either plain body text or JSON with app=word|excel|powerpoint, title, body, rows, slides, and optional target_location. Use office_create for tasks like creating a Word document containing requested text, creating a simple Excel workbook, or creating a PowerPoint deck. For updating an existing Office file, use action_type office_update with target set to the existing file and content as plain body text or JSON with app=word|excel|powerpoint, body, rows, and slides. DS Agent can append Word paragraphs, Excel rows, and PowerPoint slides deterministically. If the user asks to open the created or existing Office file, add a separate office_open action with the same target and target_location; DS Agent will prefer the matching Microsoft Office app and fall back to the system default app if it is unavailable.\n\
         - For desktop UI automation, use computer_screenshot to inspect the current desktop before planning screen-dependent clicks. Use computer_control only for one validated structured input action at a time. Set target to the app or window, set risk=critical, set requires_confirmation=true, and set content to exactly one of: click:x,y[,button], move:x,y, type:text, press:key, hotkey:key+key, or scroll:delta[,axis]. For multi-step desktop tasks such as editing an already open Word document, do not claim completion until DS Agent has actually completed the required local actions.\n\
         - Each agent_actions item may include action_type, title, reason, risk, requires_confirmation, target, destination, target_location, preferred_browser, and content.\n\
         - For file_write or create_report, target must be a relative workspace path and content must be the exact UTF-8 text DS Agent should write after local validation. For office_create, office_update, or office_open, target must be a .docx, .xlsx, or .pptx path when supplied; use target_location=\"desktop\" only when the user explicitly asks for the Desktop.\n\
         - reply_to_user must describe the intended plan, not local completion. Do not say a file was created, opened, edited, or saved until DS Agent returns execution evidence.\n\
         - DS Agent will validate schema, permissions, risk, workspace paths, and confirmations before executing any action.\n\n\
         {runtime_memory_section}\
         Full user message:\n{user_prompt}",
        model_route = serialize_agent_chat_context_value(&request.model_route),
        thinking_level = serialize_agent_chat_context_value(&request.thinking_level),
        access_mode = serialize_agent_chat_context_value(&request.access_mode),
        workspace_ready = runtime_context.workspace_ready.as_str(),
        workspace_note = runtime_context.workspace_note.as_str(),
        network_search_ready = runtime_context.network_search_ready.as_str(),
        network_search_note = runtime_context.network_search_note.as_str(),
        user_prompt = request.prompt
    )
}

fn serialize_agent_chat_context_value<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value)
        .map(|serialized| serialized.trim_matches('"').to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn load_agent_soul_profile_context(
    app_data_dir: &Path,
) -> Result<Option<AgentSoulProfileContext>, String> {
    let soul_path = agent_soul_profile_path(app_data_dir);
    if !soul_path.exists() {
        return Ok(None);
    }
    let body = fs::read_to_string(&soul_path).map_err(event_store_error)?;
    Ok(build_agent_soul_profile_context(&body))
}

fn agent_soul_profile_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir
        .join(LOCAL_MEMORY_DIR_NAME)
        .join(AGENT_SOUL_PROFILE_FILE_NAME)
}

fn agent_soul_profile_state_from_app_data_dir(
    app_data_dir: &Path,
) -> Result<AgentSoulProfileState, String> {
    let soul_path = agent_soul_profile_path(app_data_dir);
    if !soul_path.exists() {
        return Ok(agent_soul_profile_state_from_content(
            false,
            AGENT_SOUL_PROFILE_TEMPLATE.to_string(),
        ));
    }
    let content = fs::read_to_string(&soul_path).map_err(event_store_error)?;
    Ok(agent_soul_profile_state_from_content(true, content))
}

fn save_agent_soul_profile_content(
    app_data_dir: &Path,
    content: &str,
) -> Result<AgentSoulProfileState, String> {
    if content.trim().is_empty() {
        return Err("soul profile content is required".to_string());
    }
    if content.len() > AGENT_SOUL_PROFILE_MAX_BYTES {
        return Err(format!(
            "soul profile must be {} bytes or less",
            AGENT_SOUL_PROFILE_MAX_BYTES
        ));
    }
    let soul_path = agent_soul_profile_path(app_data_dir);
    if let Some(parent) = soul_path.parent() {
        fs::create_dir_all(parent).map_err(event_store_error)?;
    }
    fs::write(&soul_path, content).map_err(event_store_error)?;
    Ok(agent_soul_profile_state_from_content(
        true,
        content.to_string(),
    ))
}

fn agent_soul_profile_state_from_content(exists: bool, content: String) -> AgentSoulProfileState {
    let profile_context = build_agent_soul_profile_context(&content);
    AgentSoulProfileState {
        exists,
        content,
        summary_lines: profile_context
            .as_ref()
            .map(|profile| profile.lines.clone())
            .unwrap_or_default(),
        used_bytes: profile_context
            .as_ref()
            .map(|profile| profile.used_bytes)
            .unwrap_or(0),
        max_bytes: AGENT_SOUL_PROFILE_CONTEXT_MAX_BYTES,
    }
}

fn build_agent_soul_profile_context(body: &str) -> Option<AgentSoulProfileContext> {
    let fields = parse_agent_soul_profile_fields(body);
    let candidate_lines = agent_soul_profile_candidate_lines(&fields);
    if candidate_lines.is_empty() {
        return None;
    }

    let mut lines = Vec::new();
    let mut used_bytes = 0usize;
    for line in candidate_lines {
        let compacted = agent_memory_compact_text(&line, 220);
        let line_bytes = compacted.len();
        if used_bytes + line_bytes > AGENT_SOUL_PROFILE_CONTEXT_MAX_BYTES {
            continue;
        }
        used_bytes += line_bytes;
        lines.push(compacted);
    }

    if lines.is_empty() {
        return None;
    }

    Some(AgentSoulProfileContext {
        lines,
        used_bytes,
        max_bytes: AGENT_SOUL_PROFILE_CONTEXT_MAX_BYTES,
    })
}

fn parse_agent_soul_profile_fields(body: &str) -> std::collections::BTreeMap<String, String> {
    let mut fields = std::collections::BTreeMap::new();
    let mut section = String::new();
    for raw_line in body.lines() {
        let line = raw_line.trim();
        if let Some(heading) = line.strip_prefix("## ") {
            section = heading.trim().to_ascii_lowercase();
            continue;
        }
        if section == "never store" {
            continue;
        }
        if !matches!(section.as_str(), "user" | "ds agent" | "stable preferences") {
            continue;
        }
        let Some(item) = line.strip_prefix("- ") else {
            continue;
        };
        let Some((key, value)) = item.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if value.is_empty()
            || !agent_soul_profile_allowed_key(&key)
            || agent_soul_profile_value_looks_sensitive(value)
        {
            continue;
        }
        fields.insert(key, value.to_string());
    }
    fields
}

fn agent_soul_profile_candidate_lines(
    fields: &std::collections::BTreeMap<String, String>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(address) = fields
        .get("address_as")
        .or_else(|| fields.get("preferred_name"))
    {
        lines.push(format!("user preferred address: {address}"));
    }
    if let Some(preferred_name) = fields.get("preferred_name") {
        lines.push(format!("user preferred name: {preferred_name}"));
    }
    if let Some(name) = fields.get("user_calls_ds_agent") {
        lines.push(format!("user calls this app: {name}"));
    }
    if let Some(name) = fields.get("ds_agent_should_refer_to_itself_as") {
        lines.push(format!("DS Agent self-reference: {name}"));
    }

    let response_defaults = [
        ("language", "language_preferences"),
        ("tone", "default_response_tone"),
        ("length", "default_response_length"),
        ("formatting", "formatting_preferences"),
        ("initiative", "initiative_level"),
    ]
    .into_iter()
    .filter_map(|(label, key)| fields.get(key).map(|value| format!("{label}={value}")))
    .collect::<Vec<_>>();
    if !response_defaults.is_empty() {
        lines.push(format!(
            "response defaults: {}",
            response_defaults.join("; ")
        ));
    }

    let stable_preferences = [
        ("workflow", "workflow_preferences"),
        ("writing", "writing_preferences"),
        ("confirmation", "confirmation_preferences"),
        ("privacy", "privacy_preferences"),
    ]
    .into_iter()
    .filter_map(|(label, key)| fields.get(key).map(|value| format!("{label}={value}")))
    .collect::<Vec<_>>();
    if !stable_preferences.is_empty() {
        lines.push(format!(
            "stable preferences: {}",
            stable_preferences.join("; ")
        ));
    }

    if let Some(boundary) = fields.get("relationship_boundary") {
        lines.push(format!("relationship boundary: {boundary}"));
    }

    lines
}

fn agent_soul_profile_allowed_key(key: &str) -> bool {
    matches!(
        key,
        "preferred_name"
            | "address_as"
            | "language_preferences"
            | "default_response_tone"
            | "default_response_length"
            | "formatting_preferences"
            | "initiative_level"
            | "user_calls_ds_agent"
            | "ds_agent_should_refer_to_itself_as"
            | "relationship_boundary"
            | "workflow_preferences"
            | "writing_preferences"
            | "confirmation_preferences"
            | "privacy_preferences"
    )
}

fn agent_soul_profile_value_looks_sensitive(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();
    ["secret", "password", "api key", "token", "private key"]
        .iter()
        .any(|marker| normalized.contains(marker))
}

fn build_agent_soul_profile_prompt(profile: Option<&AgentSoulProfileContext>) -> String {
    let Some(profile) = profile else {
        return String::new();
    };
    let mut lines = vec!["DS Agent identity profile:".to_string()];
    lines.extend(profile.lines.iter().map(|line| format!("- {line}")));
    lines.push(format!(
        "Profile limits: soul.md compact summary, raw file body omitted. bytes={}/{}",
        profile.used_bytes, profile.max_bytes
    ));
    lines.join("\n")
}

fn agent_soul_profile_receipt_line(profile: &AgentSoulProfileContext) -> String {
    format!(
        "soul_profile=memory/soul.md; reason=identity_profile; bytes={}/{}; lines={}",
        profile.used_bytes,
        profile.max_bytes,
        agent_context_truncate_chars(&profile.lines.join(" | "), 180)
    )
}

struct AgentMemoryCandidateMatch {
    record: MemoryRecord,
    score: i32,
    match_reason: String,
}

fn load_agent_memory_runtime_context(
    store: &EventStore,
    prompt: &str,
) -> Result<AgentMemoryRuntimeContext, String> {
    let memories = store.list_memory_records().map_err(event_store_error)?;
    Ok(select_agent_memory_runtime_context(prompt, &memories))
}

fn select_agent_memory_runtime_context(
    prompt: &str,
    memories: &[MemoryRecord],
) -> AgentMemoryRuntimeContext {
    let query_terms = agent_memory_query_terms(prompt);
    let mut context = AgentMemoryRuntimeContext::default();
    context.query_terms_count = query_terms.len();
    context.considered_records = memories.len();
    let mut sensitive_omitted = 0usize;
    let mut archived_omitted = 0usize;
    let mut candidates = Vec::new();

    if query_terms.is_empty() {
        return context;
    }

    for memory in memories {
        if memory.sensitivity == MemorySensitivity::Sensitive {
            sensitive_omitted += 1;
            continue;
        }
        if memory.lifecycle == MemoryLifecycle::Archived {
            archived_omitted += 1;
            continue;
        }
        if let Some(candidate) = agent_memory_candidate_match(memory, &query_terms) {
            candidates.push(candidate);
        }
    }

    context.filtered_sensitive = sensitive_omitted;
    context.filtered_archived = archived_omitted;
    context.candidate_count = candidates.len();

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.record.pinned.cmp(&left.record.pinned))
            .then_with(|| right.record.updated_at.cmp(&left.record.updated_at))
    });

    let mut budget_omitted = 0usize;
    for candidate in candidates {
        if context.selected.len() >= context.max_records {
            budget_omitted += 1;
            continue;
        }
        let selected = AgentSelectedMemory {
            id: candidate.record.id,
            title: agent_memory_compact_text(&candidate.record.title, 120),
            memory_type: candidate.record.memory_type,
            scope: candidate.record.scope,
            match_reason: candidate.match_reason,
            snippet: agent_memory_compact_text(
                &candidate.record.body,
                AGENT_MEMORY_CONTEXT_SNIPPET_CHARS,
            ),
            rank: context.selected.len() + 1,
            score: candidate.score,
            inclusion_mode: "compact_snippet".to_string(),
        };
        let block_bytes = agent_selected_memory_prompt_block(&selected).len();
        if context.used_bytes + block_bytes > context.max_bytes {
            budget_omitted += 1;
            continue;
        }
        context.used_bytes += block_bytes;
        context.selected.push(selected);
    }

    if sensitive_omitted > 0 {
        context.omissions.push(format!(
            "{sensitive_omitted} sensitive memories omitted from prompt context"
        ));
    }
    if archived_omitted > 0 {
        context.omissions.push(format!(
            "{archived_omitted} archived memories omitted from prompt context"
        ));
    }
    if budget_omitted > 0 {
        context.omissions.push(format!(
            "{budget_omitted} lower-ranked memories omitted by context budget"
        ));
    }
    context.omitted_by_budget = budget_omitted;

    context
}

fn agent_memory_candidate_match(
    memory: &MemoryRecord,
    query_terms: &[String],
) -> Option<AgentMemoryCandidateMatch> {
    let title = memory.title.to_lowercase();
    let body = memory.body.to_lowercase();
    let linked_titles = memory
        .linked_memories
        .iter()
        .map(|linked| linked.title.to_lowercase())
        .collect::<Vec<_>>()
        .join("\n");
    let mut score = 0i32;
    let mut title_terms = Vec::new();
    let mut body_terms = Vec::new();
    let mut linked_terms = Vec::new();

    for term in query_terms {
        if title.contains(term) {
            score += 6;
            title_terms.push(term.clone());
        }
        if body.contains(term) {
            score += 3;
            body_terms.push(term.clone());
        }
        if !linked_titles.is_empty() && linked_titles.contains(term) {
            score += 1;
            linked_terms.push(term.clone());
        }
    }

    if score <= 0 {
        return None;
    }
    if memory.pinned {
        score += 2;
    }

    Some(AgentMemoryCandidateMatch {
        record: memory.clone(),
        score,
        match_reason: agent_memory_match_reason(&title_terms, &body_terms, &linked_terms),
    })
}

fn agent_memory_match_reason(
    title_terms: &[String],
    body_terms: &[String],
    linked_terms: &[String],
) -> String {
    if !title_terms.is_empty() {
        return format!("title_terms={}", agent_memory_join_terms(title_terms, 4));
    }
    if !body_terms.is_empty() {
        return format!("body_terms={}", agent_memory_join_terms(body_terms, 4));
    }
    format!(
        "linked_memory_terms={}",
        agent_memory_join_terms(linked_terms, 4)
    )
}

fn agent_memory_join_terms(terms: &[String], max_terms: usize) -> String {
    terms
        .iter()
        .take(max_terms)
        .cloned()
        .collect::<Vec<_>>()
        .join(",")
}

fn agent_memory_query_terms(prompt: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut ascii = String::new();
    let mut cjk = String::new();

    for character in prompt.chars() {
        if character.is_ascii_alphanumeric() {
            agent_memory_flush_cjk_terms(&mut terms, &mut cjk);
            ascii.push(character.to_ascii_lowercase());
        } else if agent_memory_is_cjk(character) {
            agent_memory_flush_ascii_term(&mut terms, &mut ascii);
            cjk.push(character);
        } else {
            agent_memory_flush_ascii_term(&mut terms, &mut ascii);
            agent_memory_flush_cjk_terms(&mut terms, &mut cjk);
        }
    }
    agent_memory_flush_ascii_term(&mut terms, &mut ascii);
    agent_memory_flush_cjk_terms(&mut terms, &mut cjk);
    terms.truncate(32);
    terms
}

fn agent_memory_flush_ascii_term(terms: &mut Vec<String>, ascii: &mut String) {
    if ascii.len() >= 2 {
        agent_memory_push_unique_term(terms, ascii.clone());
    }
    ascii.clear();
}

fn agent_memory_flush_cjk_terms(terms: &mut Vec<String>, cjk: &mut String) {
    let characters = cjk.chars().collect::<Vec<_>>();
    if characters.len() >= 2 {
        for window in characters.windows(2) {
            agent_memory_push_unique_term(terms, window.iter().copied().collect());
        }
    }
    cjk.clear();
}

fn agent_memory_push_unique_term(terms: &mut Vec<String>, term: String) {
    if !terms.iter().any(|existing| existing == &term) {
        terms.push(term);
    }
}

fn agent_memory_is_cjk(character: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&character)
        || ('\u{3400}'..='\u{4dbf}').contains(&character)
        || ('\u{f900}'..='\u{faff}').contains(&character)
}

fn agent_memory_compact_text(value: &str, max_chars: usize) -> String {
    let compacted = value.split_whitespace().collect::<Vec<_>>().join(" ");
    agent_context_truncate_chars(&compacted, max_chars)
}

fn build_agent_memory_context_prompt(memory_context: &AgentMemoryRuntimeContext) -> String {
    if memory_context.selected.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "Selected reviewed DS Agent memories for this run (bounded, read-only):".to_string(),
        format!(
            "- selection_policy=max_records:{} max_bytes:{} used_bytes:{}; use only when relevant; current user message wins; do not write memories silently",
            memory_context.max_records, memory_context.max_bytes, memory_context.used_bytes
        ),
        format!(
            "- retrieval_receipt=memory_runtime/v1; query_terms_count={}; considered_records={}; candidate_count={}; selected_count={}; filtered_sensitive={}; filtered_archived={}; omitted_by_budget={}",
            memory_context.query_terms_count,
            memory_context.considered_records,
            memory_context.candidate_count,
            memory_context.selected.len(),
            memory_context.filtered_sensitive,
            memory_context.filtered_archived,
            memory_context.omitted_by_budget
        ),
    ];
    lines.extend(
        memory_context
            .selected
            .iter()
            .map(agent_selected_memory_prompt_block),
    );
    if !memory_context.omissions.is_empty() {
        lines.push(format!(
            "- omissions={}",
            memory_context.omissions.join("; ")
        ));
    }
    lines.join("\n")
}

fn agent_selected_memory_prompt_block(memory: &AgentSelectedMemory) -> String {
    format!(
        "- memory_id={}; rank={}; type={}; scope={}; score={}; match_reason={}; inclusion_mode={}\n  title: {}\n  snippet: {}",
        memory.id,
        memory.rank,
        serialize_agent_chat_context_value(&memory.memory_type),
        serialize_agent_chat_context_value(&memory.scope),
        memory.score,
        memory.match_reason,
        memory.inclusion_mode,
        memory.title,
        memory.snippet
    )
}

fn agent_memory_retrieval_receipt_line(memory_context: &AgentMemoryRuntimeContext) -> String {
    format!(
        "memory_retrieval=memory_runtime/v1; query_terms_count={}; considered_records={}; candidate_count={}; selected_count={}; max_records={}; max_bytes={}; used_bytes={}; filtered_sensitive={}; filtered_archived={}; omitted_by_budget={}",
        memory_context.query_terms_count,
        memory_context.considered_records,
        memory_context.candidate_count,
        memory_context.selected.len(),
        memory_context.max_records,
        memory_context.max_bytes,
        memory_context.used_bytes,
        memory_context.filtered_sensitive,
        memory_context.filtered_archived,
        memory_context.omitted_by_budget
    )
}

fn agent_memory_context_has_retrieval_receipt(memory_context: &AgentMemoryRuntimeContext) -> bool {
    memory_context.query_terms_count > 0
        || memory_context.considered_records > 0
        || !memory_context.selected.is_empty()
        || !memory_context.omissions.is_empty()
}

fn agent_selected_memory_receipt_line(memory: &AgentSelectedMemory) -> String {
    format!(
        "memory_id={}; rank={}; title={}; type={}; scope={}; score={}; match_reason={}; inclusion_mode={}; snippet={}",
        memory.id,
        memory.rank,
        memory.title,
        serialize_agent_chat_context_value(&memory.memory_type),
        serialize_agent_chat_context_value(&memory.scope),
        memory.score,
        memory.match_reason,
        memory.inclusion_mode,
        agent_context_truncate_chars(&memory.snippet, 160)
    )
}

pub fn agent_chat_with_transport(
    transport: &impl DeepSeekChatCompletionTransport,
    cache: &DeepSeekMemoryChatCompletionCache,
    api_key: &str,
    request: AgentChatRequest,
    pricing_settings: Option<&DeepSeekPricingSettings>,
) -> Result<(AgentChatResponse, DeepSeekChatTelemetry), String> {
    agent_chat_with_transport_and_runtime_context(
        transport,
        cache,
        api_key,
        request,
        AgentChatRuntimeContext::default(),
        pricing_settings,
    )
}

fn agent_chat_with_transport_and_runtime_context(
    transport: &impl DeepSeekChatCompletionTransport,
    cache: &DeepSeekMemoryChatCompletionCache,
    api_key: &str,
    request: AgentChatRequest,
    runtime_context: AgentChatRuntimeContext,
    pricing_settings: Option<&DeepSeekPricingSettings>,
) -> Result<(AgentChatResponse, DeepSeekChatTelemetry), String> {
    let prompt = request.prompt.trim();
    if prompt.is_empty() {
        return Err("agent chat message is required".to_string());
    }

    let protocol_user_prompt = build_agent_chat_protocol_user_prompt(&request, &runtime_context);
    let deepseek_request = build_deepseek_chat_completion_request(
        request.model_route,
        request.thinking_level,
        AGENT_CHAT_SYSTEM_PROMPT,
        &protocol_user_prompt,
    )?;
    let execution =
        execute_deepseek_chat_completion_with_cache(transport, cache, api_key, &deepseek_request)?;
    let content = execution
        .response
        .first_text()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "deepseek response did not include assistant content".to_string())?
        .to_string();
    let mut priced_telemetry =
        deepseek_telemetry_with_pricing(vec![execution.telemetry], pricing_settings);
    let telemetry = priced_telemetry
        .pop()
        .ok_or_else(|| "deepseek telemetry was not recorded".to_string())?;
    let response = agent_chat_response_from_telemetry(content, &telemetry, request.access_mode);

    Ok((response, telemetry))
}

fn agent_chat_with_dispatch_and_tool_followup(
    transport: &impl DeepSeekChatCompletionTransport,
    cache: &DeepSeekMemoryChatCompletionCache,
    api_key: &str,
    request: AgentChatRequest,
    runtime_context: AgentChatRuntimeContext,
    pricing_settings: Option<&DeepSeekPricingSettings>,
    store: &EventStore,
    file_client: &impl FileContentClient,
    file_write_client: &impl AgentWritableArtifactClient,
    search_client: &impl NetworkSearchClient,
    browser_client: &impl BrowserPageClient,
) -> Result<(AgentChatResponse, Vec<DeepSeekChatTelemetry>), String> {
    let original_user_prompt = request.prompt.clone();
    let (mut response, first_telemetry) = agent_chat_with_transport_and_runtime_context(
        transport,
        cache,
        api_key,
        request.clone(),
        runtime_context.clone(),
        pricing_settings,
    )?;
    mark_agent_workspace_actions_waiting_if_needed(&runtime_context, &mut response);
    dispatch_agent_action_proposals_with_desktop_dir(
        store,
        request.access_mode,
        &mut response,
        file_client,
        file_write_client,
        search_client,
        browser_client,
        runtime_context.desktop_dir.as_deref(),
    )?;

    let mut telemetry = vec![first_telemetry];
    let Some(followup_prompt) =
        build_agent_tool_evidence_followup_prompt(&original_user_prompt, &response)
    else {
        apply_agent_local_completion_summary(&mut response);
        return Ok((response, telemetry));
    };

    let followup_result = agent_chat_with_transport_and_runtime_context(
        transport,
        cache,
        api_key,
        AgentChatRequest {
            prompt: followup_prompt,
            ..request
        },
        runtime_context,
        pricing_settings,
    );
    match followup_result {
        Ok((followup_response, followup_telemetry)) => {
            telemetry.push(followup_telemetry);
            response = merge_agent_chat_followup_response(response, followup_response);
        }
        Err(error) => {
            response.content = agent_chat_completed_actions_followup_failed_message(&error);
        }
    }

    Ok((response, telemetry))
}

fn agent_chat_with_transport_and_runtime_context_with_api_key_fallback(
    transport: &impl DeepSeekChatCompletionTransport,
    cache: &DeepSeekMemoryChatCompletionCache,
    api_keys: &[String],
    request: AgentChatRequest,
    runtime_context: AgentChatRuntimeContext,
    pricing_settings: Option<&DeepSeekPricingSettings>,
) -> Result<(AgentChatResponse, DeepSeekChatTelemetry, String), String> {
    let mut errors = Vec::new();
    for api_key in api_keys {
        match agent_chat_with_transport_and_runtime_context(
            transport,
            cache,
            api_key,
            request.clone(),
            runtime_context.clone(),
            pricing_settings,
        ) {
            Ok((response, telemetry)) => return Ok((response, telemetry, api_key.clone())),
            Err(error) => errors.push(error),
        }
    }

    Err(errors
        .last()
        .cloned()
        .unwrap_or_else(|| "DeepSeek Chat is not configured. Provide a DeepSeek API key for this session or set DEEPSEEK_API_KEY in the local desktop process.".to_string()))
}

fn run_agent_chat_with_clients(
    store: &Mutex<EventStore>,
    transport: &impl DeepSeekChatCompletionTransport,
    cache: &DeepSeekMemoryChatCompletionCache,
    api_key: &str,
    request: AgentChatRequest,
    runtime_context: AgentChatRuntimeContext,
    pricing_settings: Option<&DeepSeekPricingSettings>,
    file_client: &impl FileContentClient,
    file_write_client: &impl AgentWritableArtifactClient,
    search_client: &impl NetworkSearchClient,
    browser_client: &impl BrowserPageClient,
) -> Result<AgentChatResponse, String> {
    run_agent_chat_with_clients_and_api_keys(
        store,
        transport,
        cache,
        &[api_key.to_string()],
        request,
        runtime_context,
        pricing_settings,
        file_client,
        file_write_client,
        search_client,
        browser_client,
    )
}

fn run_agent_chat_with_clients_and_api_keys(
    store: &Mutex<EventStore>,
    transport: &impl DeepSeekChatCompletionTransport,
    cache: &DeepSeekMemoryChatCompletionCache,
    api_keys: &[String],
    request: AgentChatRequest,
    runtime_context: AgentChatRuntimeContext,
    pricing_settings: Option<&DeepSeekPricingSettings>,
    file_client: &impl FileContentClient,
    file_write_client: &impl AgentWritableArtifactClient,
    search_client: &impl NetworkSearchClient,
    browser_client: &impl BrowserPageClient,
) -> Result<AgentChatResponse, String> {
    let original_user_prompt = request.prompt.clone();
    let memory_context = {
        let store = store.lock().map_err(|_| lock_error())?;
        load_agent_memory_runtime_context(&store, &original_user_prompt)?
    };
    let mut runtime_context = runtime_context;
    runtime_context.memory_context = memory_context;
    let (mut response, first_telemetry, followup_api_key) =
        agent_chat_with_transport_and_runtime_context_with_api_key_fallback(
            transport,
            cache,
            api_keys,
            request.clone(),
            runtime_context.clone(),
            pricing_settings,
        )?;
    mark_agent_workspace_actions_waiting_if_needed(&runtime_context, &mut response);

    {
        dispatch_agent_action_proposals_with_store_mutex(
            store,
            request.access_mode,
            &mut response,
            file_client,
            file_write_client,
            search_client,
            browser_client,
            runtime_context.desktop_dir.as_deref(),
        )?;
    }

    let mut telemetry = vec![first_telemetry];
    let model_route_context = serialize_agent_chat_context_value(&request.model_route);
    let thinking_level_context = serialize_agent_chat_context_value(&request.thinking_level);
    let access_mode_context = serialize_agent_chat_context_value(&request.access_mode);
    if let Some(followup_prompt) =
        build_agent_tool_evidence_followup_prompt(&original_user_prompt, &response)
    {
        let followup_result = agent_chat_with_transport_and_runtime_context(
            transport,
            cache,
            &followup_api_key,
            AgentChatRequest {
                prompt: followup_prompt,
                ..request
            },
            runtime_context.clone(),
            pricing_settings,
        );
        match followup_result {
            Ok((followup_response, followup_telemetry)) => {
                telemetry.push(followup_telemetry);
                response = merge_agent_chat_followup_response(response, followup_response);
            }
            Err(error) => {
                response.content = agent_chat_completed_actions_followup_failed_message(&error);
            }
        }
    } else {
        apply_agent_local_completion_summary(&mut response);
    }

    {
        let store = store.lock().map_err(|_| lock_error())?;
        let memory_candidate_gate = apply_agent_memory_candidate_gate(&store, &mut response)?;
        record_agent_memory_candidates(&store, &response)?;
        record_agent_context_receipts(
            &store,
            &response,
            &model_route_context,
            &thinking_level_context,
            &access_mode_context,
            runtime_context.soul_profile.as_ref(),
            &runtime_context.memory_context,
            &memory_candidate_gate,
            &telemetry,
        )?;
        for entry in telemetry {
            store
                .append_deepseek_chat_telemetry(&entry)
                .map_err(event_store_error)?;
        }
    }

    Ok(response)
}

fn build_agent_tool_evidence_followup_prompt(
    original_user_prompt: &str,
    response: &AgentChatResponse,
) -> Option<String> {
    let needs_model_followup = response
        .proposed_actions
        .iter()
        .any(agent_action_needs_model_evidence_followup);
    let completed_actions = completed_agent_action_dispatch_summaries_for_model(response);

    if !needs_model_followup || completed_actions.is_empty() {
        return None;
    }

    Some(format!(
        "DS Agent completed local actions and collected tool evidence.\n\
         Original user message:\n{original_user_prompt}\n\n\
         Tool evidence from completed DS Agent actions:\n{}\n\n\
         Use only this completed evidence and the original user message to produce the final user-facing answer. Do not claim any unexecuted action succeeded. Do not quote internal loop labels such as loop_mode, validators, stop_conditions, or matched_stop_conditions in the user-facing answer.",
        completed_actions.join("\n")
    ))
}

fn record_agent_context_receipts(
    store: &EventStore,
    response: &AgentChatResponse,
    model_route: &str,
    thinking_level: &str,
    access_mode: &str,
    soul_profile: Option<&AgentSoulProfileContext>,
    memory_context: &AgentMemoryRuntimeContext,
    memory_candidate_gate: &AgentMemoryCandidateGateReceipt,
    telemetry: &[DeepSeekChatTelemetry],
) -> Result<(), String> {
    let token_cache_state = operations_briefing_token_cache_context(telemetry);
    for action in response
        .proposed_actions
        .iter()
        .filter(|action| agent_action_needs_context_receipt(action))
    {
        let receipt = agent_context_receipt_for_action(
            action,
            model_route,
            thinking_level,
            access_mode,
            &token_cache_state,
            soul_profile,
            memory_context,
            memory_candidate_gate,
        );
        store
            .append_agent_context_receipt(&receipt)
            .map_err(event_store_error)?;
    }
    Ok(())
}

fn agent_action_needs_context_receipt(action: &AgentChatActionProposal) -> bool {
    action.capability_invocation_id.is_some() && agent_action_needs_model_evidence_followup(action)
}

fn agent_context_receipt_for_action(
    action: &AgentChatActionProposal,
    model_route: &str,
    thinking_level: &str,
    access_mode: &str,
    token_cache_state: &str,
    soul_profile: Option<&AgentSoulProfileContext>,
    memory_context: &AgentMemoryRuntimeContext,
    memory_candidate_gate: &AgentMemoryCandidateGateReceipt,
) -> AgentContextReceipt {
    let mut receipt = AgentContextReceipt::new(
        action.action_type.clone(),
        action.execution_state.clone(),
        model_route.to_string(),
        thinking_level.to_string(),
        token_cache_state.to_string(),
    );
    receipt.capability = action
        .capability
        .as_ref()
        .map(serialize_agent_chat_context_value);
    receipt.policy_decision = action
        .policy_decision
        .as_ref()
        .map(serialize_agent_chat_context_value);
    receipt.capability_invocation_id = action.capability_invocation_id;
    receipt.workflow_run_id = action.workflow_run_id;
    let loop_mode = classify_agent_action_loop_mode(
        &action.action_type,
        &action.execution_state,
        action.requires_confirmation,
        action.workflow_run_id.is_some(),
    );
    receipt.loop_mode = loop_mode.as_str().to_string();
    let loop_mode_descriptor = agent_loop_mode_descriptor(loop_mode);
    receipt.allowed_tools = loop_mode_descriptor
        .allowed_tools
        .iter()
        .map(|value| (*value).to_string())
        .collect();
    receipt.validators = loop_mode_descriptor
        .validators
        .iter()
        .map(|value| (*value).to_string())
        .collect();
    receipt.stop_conditions = loop_mode_descriptor
        .stop_conditions
        .iter()
        .map(|value| (*value).to_string())
        .collect();
    receipt.matched_stop_conditions = agent_context_matched_stop_conditions(action, loop_mode);
    receipt.confirmation_rule = loop_mode_descriptor.confirmation_rule.to_string();
    receipt.policy_constraints = agent_context_policy_constraints(action, access_mode);
    receipt.selected_evidence = agent_context_selected_evidence(action);
    let mut selected_memories = soul_profile
        .map(agent_soul_profile_receipt_line)
        .into_iter()
        .collect::<Vec<_>>();
    if agent_memory_context_has_retrieval_receipt(memory_context) {
        selected_memories.push(agent_memory_retrieval_receipt_line(memory_context));
    }
    selected_memories.extend(
        memory_context
            .selected
            .iter()
            .map(agent_selected_memory_receipt_line),
    );
    receipt.selected_memories = selected_memories;
    receipt.memory_candidate_gate =
        agent_memory_candidate_gate_receipt_lines(memory_candidate_gate);
    receipt.validation_results = agent_context_validation_results(action);
    if memory_candidate_gate.proposed > 0 {
        receipt
            .validation_results
            .push("memory candidate gate reviewed".to_string());
    }
    receipt
        .validation_results
        .push(format!("loop_mode={}", loop_mode.as_str()));
    receipt.validation_results.extend(
        receipt
            .matched_stop_conditions
            .iter()
            .map(|condition| format!("stop_condition_met={condition}")),
    );
    receipt.intentional_omissions = vec![
        "Raw user prompt is not stored in the context receipt.".to_string(),
        "Raw tool result bodies are omitted; use evidence refs and excerpts instead.".to_string(),
        "API keys and local secrets are omitted.".to_string(),
        "Full memory bodies are omitted; selected memories use bounded reviewed snippets."
            .to_string(),
    ];
    receipt
        .intentional_omissions
        .extend(memory_context.omissions.iter().cloned());
    if memory_candidate_gate.proposed > 0 {
        receipt.intentional_omissions.push(
            "Rejected memory candidate bodies are omitted; only gate counts and safe kept labels are stored."
                .to_string(),
        );
    }
    receipt
}

fn agent_context_matched_stop_conditions(
    action: &AgentChatActionProposal,
    loop_mode: AgentLoopMode,
) -> Vec<String> {
    if action.execution_state == "blocked" || action.execution_state == "failed" {
        return vec!["blocked_or_failed".to_string()];
    }
    if action.execution_state == "needs_confirmation" || action.requires_confirmation {
        return vec!["user_confirmation_required".to_string()];
    }
    if action.execution_state == "waiting_prerequisite" {
        return vec!["missing_prerequisite".to_string()];
    }
    if action.execution_state != "succeeded" {
        return Vec::new();
    }

    match loop_mode {
        AgentLoopMode::DirectAnswer => vec!["answer_ready".to_string()],
        AgentLoopMode::EvidenceGathering => vec!["evidence_observed".to_string()],
        AgentLoopMode::PermissionedAction => vec!["action_completed".to_string()],
        AgentLoopMode::WorkflowRun => vec!["workflow_draft_ready".to_string()],
        AgentLoopMode::CodingRepair => vec!["tests_passed".to_string()],
        AgentLoopMode::Review => vec!["review_item_queued".to_string()],
        AgentLoopMode::Verification => vec!["verification_passed".to_string()],
        AgentLoopMode::Resume => vec!["resume_ready".to_string()],
    }
}

fn agent_context_policy_constraints(
    action: &AgentChatActionProposal,
    access_mode: &str,
) -> Vec<String> {
    let mut constraints = vec![
        format!("access_mode={access_mode}"),
        format!("requires_confirmation={}", action.requires_confirmation),
    ];
    if let Some(capability) = action.capability.as_ref() {
        constraints.push(format!(
            "capability={}",
            serialize_agent_chat_context_value(capability)
        ));
    }
    if let Some(decision) = action.policy_decision.as_ref() {
        constraints.push(format!(
            "policy_decision={}",
            serialize_agent_chat_context_value(decision)
        ));
    }
    constraints.push(match action.permission_request_id {
        Some(permission_request_id) => format!("permission_request={permission_request_id}"),
        None => "permission_request=none".to_string(),
    });
    if let Some(capability_invocation_id) = action.capability_invocation_id {
        constraints.push(format!("capability_invocation={capability_invocation_id}"));
    }
    if let Some(workflow_run_id) = action.workflow_run_id {
        constraints.push(format!("workflow_run={workflow_run_id}"));
    }
    constraints
}

fn agent_context_selected_evidence(action: &AgentChatActionProposal) -> Vec<String> {
    let mut evidence = Vec::new();
    if let Some(invocation_id) = action.capability_invocation_id {
        evidence.push(format!("capability_invocation:{invocation_id}"));
    }
    if let Some(workflow_run_id) = action.workflow_run_id {
        evidence.push(format!("workflow_run:{workflow_run_id}"));
    }
    if let Some(target) = action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        evidence.push(format!(
            "target:{}",
            agent_context_truncate_chars(target, 240)
        ));
    }
    if let Some(note) = action
        .dispatch_note
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        evidence.push(format!(
            "dispatch_note:{}",
            agent_context_truncate_chars(note, 480)
        ));
    }
    evidence
}

fn agent_context_validation_results(action: &AgentChatActionProposal) -> Vec<String> {
    let mut results = vec![
        "model proposal parsed and normalized".to_string(),
        format!("execution_state={}", action.execution_state),
    ];
    if action.capability_invocation_id.is_some() {
        results.push("capability invocation recorded".to_string());
    }
    if let Some(decision) = action.policy_decision {
        results.push(format!(
            "policy_decision={}",
            serialize_agent_chat_context_value(&decision)
        ));
    }
    if let Some(blocked_reason) = action
        .blocked_reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        results.push(format!(
            "blocked_reason={}",
            agent_context_truncate_chars(blocked_reason, 240)
        ));
    }
    results
}

fn agent_context_truncate_chars(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if index >= max_chars {
            output.push_str("[truncated]");
            return output;
        }
        output.push(character);
    }
    output
}

fn apply_agent_local_completion_summary(response: &mut AgentChatResponse) {
    let completed_actions = completed_agent_action_dispatch_summaries(response);
    if completed_actions.is_empty() {
        return;
    }

    response.content = format!(
        "DS Agent 已完成并验证本地动作：\n{}\n\n验证：本地执行器返回成功状态和结果记录。\n建议：{}",
        completed_actions.join("\n"),
        agent_local_completion_advice(response)
    );
}

fn agent_local_completion_advice(response: &AgentChatResponse) -> &'static str {
    if response.proposed_actions.iter().any(|action| {
        action.execution_state == "succeeded"
            && matches!(
                action.action_type.as_str(),
                "office_create" | "office_update" | "office_open"
            )
    }) {
        return "如果这份 Office 结果要发给他人，下一步可以让我帮你检查标题、格式和敏感信息。";
    }

    if response.proposed_actions.iter().any(|action| {
        action.execution_state == "succeeded"
            && matches!(
                action.action_type.as_str(),
                "file_create" | "file_update" | "file_write" | "create_report"
            )
    }) {
        return "如果这个文件要用于正式场景，下一步可以让我帮你补一次格式和敏感信息检查。";
    }

    "如果要把这个结果用于正式场景，下一步可以让我帮你补一次复核。"
}

fn completed_agent_action_dispatch_summaries(response: &AgentChatResponse) -> Vec<String> {
    response
        .proposed_actions
        .iter()
        .filter(|action| action.execution_state == "succeeded")
        .filter_map(agent_action_dispatch_summary)
        .collect()
}

fn agent_action_dispatch_summary(action: &AgentChatActionProposal) -> Option<String> {
    let note = action
        .dispatch_note
        .as_deref()
        .map(str::trim)
        .filter(|note| !note.is_empty())?;
    Some(format!(
        "- {} ({}) target={}: {}",
        action
            .title
            .as_deref()
            .unwrap_or(action.action_type.as_str()),
        action.action_type,
        action.target.as_deref().unwrap_or("not specified"),
        note
    ))
}

fn completed_agent_action_dispatch_summaries_for_model(
    response: &AgentChatResponse,
) -> Vec<String> {
    response
        .proposed_actions
        .iter()
        .filter(|action| action.execution_state == "succeeded")
        .filter_map(agent_action_dispatch_summary_for_model)
        .collect()
}

fn agent_action_dispatch_summary_for_model(action: &AgentChatActionProposal) -> Option<String> {
    let summary = agent_action_dispatch_summary(action)?;
    let loop_mode = classify_agent_action_loop_mode(
        &action.action_type,
        &action.execution_state,
        action.requires_confirmation,
        action.workflow_run_id.is_some(),
    );
    let loop_mode_descriptor = agent_loop_mode_descriptor(loop_mode);
    let matched_stop_conditions = agent_context_matched_stop_conditions(action, loop_mode);
    Some(format!(
        "{summary}\n  Loop context: loop_mode={} matched_stop_conditions={} validators={} stop_conditions={}",
        loop_mode.as_str(),
        matched_stop_conditions.join(","),
        loop_mode_descriptor.validators.join(","),
        loop_mode_descriptor.stop_conditions.join(",")
    ))
}

fn agent_action_needs_model_evidence_followup(action: &AgentChatActionProposal) -> bool {
    if action.execution_state != "succeeded" {
        return false;
    }

    matches!(
        action.action_type.as_str(),
        "browser_browse"
            | "computer_screenshot"
            | "create_report"
            | "file_read"
            | "file_write"
            | "network_search"
            | "operations_briefing"
            | "terminal_read"
    )
}

fn merge_agent_chat_followup_response(
    initial_response: AgentChatResponse,
    mut followup_response: AgentChatResponse,
) -> AgentChatResponse {
    followup_response.proposed_actions = initial_response.proposed_actions;
    followup_response.missing_prerequisites = initial_response.missing_prerequisites;
    followup_response
        .memory_candidates
        .splice(0..0, initial_response.memory_candidates);
    followup_response
}

fn agent_chat_completed_actions_followup_failed_message(error: &str) -> String {
    let reason = if error.contains("could not be read")
        || error.contains("decoding response body")
        || error.contains("timed out")
        || error.contains("timeout")
    {
        "DeepSeek 响应读取失败或超时"
    } else {
        "DeepSeek 最终说明生成失败"
    };

    format!(
        "DS Agent 已完成本地动作，但 DeepSeek 最终说明读取失败。请查看右侧运行步骤确认已完成动作；如需重新生成说明，可以稍后重试。（原因：{reason}。）"
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentMemoryCandidateGateDropReason {
    Sensitive,
    Transient,
    Archived,
    Invalid,
    OverLimit,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct AgentMemoryCandidateGateReceipt {
    proposed: usize,
    kept: usize,
    dropped_sensitive: usize,
    dropped_transient: usize,
    dropped_archived: usize,
    dropped_invalid: usize,
    dropped_over_limit: usize,
    kept_summaries: Vec<String>,
}

impl AgentMemoryCandidateGateReceipt {
    fn dropped(&self) -> usize {
        self.dropped_sensitive
            + self.dropped_transient
            + self.dropped_archived
            + self.dropped_invalid
            + self.dropped_over_limit
    }

    fn record_drop(&mut self, reason: AgentMemoryCandidateGateDropReason) {
        match reason {
            AgentMemoryCandidateGateDropReason::Sensitive => self.dropped_sensitive += 1,
            AgentMemoryCandidateGateDropReason::Transient => self.dropped_transient += 1,
            AgentMemoryCandidateGateDropReason::Archived => self.dropped_archived += 1,
            AgentMemoryCandidateGateDropReason::Invalid => self.dropped_invalid += 1,
            AgentMemoryCandidateGateDropReason::OverLimit => self.dropped_over_limit += 1,
        }
    }

    fn record_kept(&mut self, candidate: &MemoryCandidate) {
        self.kept += 1;
        self.kept_summaries.push(format!(
            "kept title={}; suggested_action={}; privacy_review={}",
            agent_memory_candidate_receipt_value(&candidate.title, 96),
            agent_memory_candidate_suggested_action_label(candidate.suggested_action),
            candidate.privacy_review
        ));
    }
}

fn apply_agent_memory_candidate_gate(
    store: &EventStore,
    response: &mut AgentChatResponse,
) -> Result<AgentMemoryCandidateGateReceipt, String> {
    let memories = store.list_memory_records().map_err(event_store_error)?;
    let (candidates, receipt) =
        gate_agent_memory_candidates_for_review(&response.memory_candidates, &memories);
    response.memory_candidates = candidates;
    Ok(receipt)
}

fn gate_agent_memory_candidates_for_review(
    candidates: &[MemoryCandidate],
    memories: &[MemoryRecord],
) -> (Vec<MemoryCandidate>, AgentMemoryCandidateGateReceipt) {
    let mut receipt = AgentMemoryCandidateGateReceipt {
        proposed: candidates.len(),
        ..AgentMemoryCandidateGateReceipt::default()
    };
    let mut gated_candidates = Vec::new();

    for candidate in candidates {
        if let Some(reason) = agent_memory_candidate_gate_drop_reason(candidate) {
            receipt.record_drop(reason);
            continue;
        }
        if gated_candidates.len() >= AGENT_MEMORY_CANDIDATE_GATE_MAX_RECORDS {
            receipt.record_drop(AgentMemoryCandidateGateDropReason::OverLimit);
            continue;
        }
        if let Some(gated_candidate) = gate_agent_memory_candidate_for_review(candidate, memories) {
            receipt.record_kept(&gated_candidate);
            gated_candidates.push(gated_candidate);
        } else {
            receipt.record_drop(AgentMemoryCandidateGateDropReason::Invalid);
        }
    }

    (gated_candidates, receipt)
}

fn gate_agent_memory_candidate_for_review(
    candidate: &MemoryCandidate,
    memories: &[MemoryRecord],
) -> Option<MemoryCandidate> {
    if !agent_memory_candidate_is_reviewable(candidate) {
        return None;
    }

    let mut candidate = candidate.clone();
    let conflict_count = agent_memory_candidate_gate_conflict_count(&candidate, memories);
    candidate.evidence_excerpt =
        agent_context_truncate_chars(&candidate.body, AGENT_MEMORY_CANDIDATE_EVIDENCE_CHARS);
    candidate.privacy_review = "normal".to_string();
    candidate.suggested_action = if conflict_count > 0 {
        MemoryCandidateSuggestedAction::Merge
    } else {
        MemoryCandidateSuggestedAction::New
    };
    let original_rationale =
        agent_context_truncate_chars(&candidate.rationale, AGENT_MEMORY_CANDIDATE_REASON_CHARS);
    candidate.rationale = format!(
        "Memory Candidate Gate: why_remember={}; privacy_review={}; suggested_action={}; evidence_excerpt={}; conflict_count={}; original_rationale={}",
        agent_memory_candidate_gate_why(&candidate),
        candidate.privacy_review,
        agent_memory_candidate_suggested_action_label(candidate.suggested_action),
        candidate.evidence_excerpt,
        conflict_count,
        original_rationale
    );
    Some(candidate)
}

fn agent_memory_candidate_is_reviewable(candidate: &MemoryCandidate) -> bool {
    agent_memory_candidate_gate_drop_reason(candidate).is_none()
}

fn agent_memory_candidate_gate_drop_reason(
    candidate: &MemoryCandidate,
) -> Option<AgentMemoryCandidateGateDropReason> {
    if candidate.sensitivity == MemorySensitivity::Sensitive
        || agent_memory_candidate_contains_sensitive_text(candidate)
    {
        return Some(AgentMemoryCandidateGateDropReason::Sensitive);
    }
    if agent_memory_candidate_is_transient(candidate) {
        return Some(AgentMemoryCandidateGateDropReason::Transient);
    }
    if candidate.lifecycle == MemoryLifecycle::Archived {
        return Some(AgentMemoryCandidateGateDropReason::Archived);
    }
    if candidate.title.trim().is_empty() || candidate.body.trim().is_empty() {
        return Some(AgentMemoryCandidateGateDropReason::Invalid);
    }
    None
}

fn agent_memory_candidate_gate_receipt_lines(
    receipt: &AgentMemoryCandidateGateReceipt,
) -> Vec<String> {
    if receipt.proposed == 0 {
        return Vec::new();
    }

    let mut lines = vec![format!(
        "proposed={}; kept={}; dropped={}; reasons=sensitive={},transient={},archived={},invalid={},over_limit={}",
        receipt.proposed,
        receipt.kept,
        receipt.dropped(),
        receipt.dropped_sensitive,
        receipt.dropped_transient,
        receipt.dropped_archived,
        receipt.dropped_invalid,
        receipt.dropped_over_limit
    )];
    lines.extend(
        receipt
            .kept_summaries
            .iter()
            .take(AGENT_MEMORY_CANDIDATE_GATE_MAX_RECORDS)
            .cloned(),
    );
    lines
}

fn agent_memory_candidate_receipt_value(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    agent_context_truncate_chars(&normalized, max_chars)
}

fn agent_memory_candidate_contains_sensitive_text(candidate: &MemoryCandidate) -> bool {
    let haystack = agent_memory_candidate_gate_haystack(candidate);
    [
        "password",
        "passcode",
        "api key",
        "secret",
        "token",
        "hunter2",
        "sk-",
        "密码",
        "密钥",
        "令牌",
        "身份证",
        "手机号",
        "银行卡",
    ]
    .iter()
    .any(|marker| haystack.contains(marker))
}

fn agent_memory_candidate_is_transient(candidate: &MemoryCandidate) -> bool {
    let haystack = agent_memory_candidate_gate_haystack(candidate);
    [
        "only for today",
        "today's",
        "one-off",
        "temporary",
        "for this task",
        "current task",
        "本次",
        "这次",
        "临时",
        "一次性",
        "今天",
        "只在",
    ]
    .iter()
    .any(|marker| haystack.contains(marker))
}

fn agent_memory_candidate_gate_conflict_count(
    candidate: &MemoryCandidate,
    memories: &[MemoryRecord],
) -> usize {
    memories
        .iter()
        .filter(|memory| {
            memory.sensitivity != MemorySensitivity::Sensitive
                && memory.lifecycle != MemoryLifecycle::Archived
                && agent_memory_candidate_gate_conflicts_with_memory(candidate, memory)
        })
        .count()
}

fn agent_memory_candidate_gate_conflicts_with_memory(
    candidate: &MemoryCandidate,
    memory: &MemoryRecord,
) -> bool {
    let candidate_title = candidate.title.trim().to_lowercase();
    let memory_title = memory.title.trim().to_lowercase();
    if !candidate_title.is_empty() && candidate_title == memory_title {
        return true;
    }

    let candidate_body = candidate.body.trim().to_lowercase();
    let memory_body = memory.body.trim().to_lowercase();
    !candidate_body.is_empty()
        && !memory_body.is_empty()
        && (candidate_body.contains(&memory_body) || memory_body.contains(&candidate_body))
}

fn agent_memory_candidate_gate_haystack(candidate: &MemoryCandidate) -> String {
    format!(
        "{}\n{}\n{}",
        candidate.title, candidate.body, candidate.rationale
    )
    .to_lowercase()
}

fn agent_memory_candidate_gate_why(candidate: &MemoryCandidate) -> &'static str {
    match candidate.memory_type {
        MemoryType::Preference => "stable_user_preference",
        MemoryType::ProjectContext => "project_context",
        MemoryType::WorkflowRule => "workflow_rule",
        MemoryType::Artifact => "artifact_reference",
        MemoryType::FailurePattern => "failure_pattern",
    }
}

fn agent_memory_candidate_suggested_action_label(
    action: MemoryCandidateSuggestedAction,
) -> &'static str {
    match action {
        MemoryCandidateSuggestedAction::New => "new",
        MemoryCandidateSuggestedAction::Merge => "merge",
        MemoryCandidateSuggestedAction::Replace => "replace",
        MemoryCandidateSuggestedAction::Link => "link",
        MemoryCandidateSuggestedAction::RejectHint => "reject_hint",
    }
}

fn record_agent_memory_candidates(
    store: &EventStore,
    response: &AgentChatResponse,
) -> Result<(), String> {
    for candidate in &response.memory_candidates {
        store
            .append_memory_candidate(candidate)
            .map_err(event_store_error)?;
    }
    Ok(())
}

fn record_agent_action_permission_requests(
    store: &EventStore,
    access_mode: AccessMode,
    response: &mut AgentChatResponse,
) -> Result<(), String> {
    for action in &mut response.proposed_actions {
        let Some(capability) = action.capability else {
            continue;
        };
        if action.policy_decision != Some(PolicyDecision::Ask) {
            continue;
        }
        if action.permission_request_id.is_some() {
            continue;
        }

        let request = build_capability_access_request(access_mode, capability)?;
        let entry = PermissionAuditEntry::evaluate(access_mode, capability);
        store
            .append_capability_access_request(&request)
            .map_err(event_store_error)?;
        store
            .append_permission_audit_entry(&entry)
            .map_err(event_store_error)?;

        action.permission_request_id = Some(request.id);
        action.dispatch_note =
            Some("waiting for local permission approval before dispatch".to_string());
    }

    Ok(())
}

fn dispatch_agent_action_proposals(
    store: &EventStore,
    access_mode: AccessMode,
    response: &mut AgentChatResponse,
    file_client: &impl FileContentClient,
    file_write_client: &impl AgentWritableArtifactClient,
    search_client: &impl NetworkSearchClient,
    browser_client: &impl BrowserPageClient,
) -> Result<(), String> {
    dispatch_agent_action_proposals_with_desktop_dir(
        store,
        access_mode,
        response,
        file_client,
        file_write_client,
        search_client,
        browser_client,
        None,
    )
}

fn dispatch_agent_action_proposals_with_desktop_dir(
    store: &EventStore,
    access_mode: AccessMode,
    response: &mut AgentChatResponse,
    file_client: &impl FileContentClient,
    file_write_client: &impl AgentWritableArtifactClient,
    search_client: &impl NetworkSearchClient,
    browser_client: &impl BrowserPageClient,
    desktop_dir: Option<&Path>,
) -> Result<(), String> {
    if !response.missing_prerequisites.is_empty() {
        mark_agent_actions_waiting_for_prerequisites(response);
        return Ok(());
    }
    mark_office_open_actions_waiting_for_pending_create(response);

    for action in &mut response.proposed_actions {
        if action.execution_state == "blocked" {
            continue;
        }

        if matches!(
            action.execution_state.as_str(),
            "waiting_prerequisite" | "succeeded" | "failed"
        ) {
            continue;
        }

        let approval_state = agent_action_approval_state(store, action)?;

        if action.action_type == "operations_briefing" {
            match approval_state {
                AgentActionApprovalState::Approved(approval_request_id) => {
                    dispatch_agent_operations_briefing_action(
                        store,
                        access_mode,
                        action,
                        true,
                        Some(approval_request_id),
                    )?;
                }
                AgentActionApprovalState::Rejected => {
                    action.execution_state = "blocked".to_string();
                    action.blocked_reason =
                        Some("local permission request was rejected before dispatch".to_string());
                    action.dispatch_note =
                        Some("local permission was rejected; action was not executed".to_string());
                }
                AgentActionApprovalState::Unavailable => {
                    action.execution_state = "blocked".to_string();
                    action.blocked_reason =
                        Some("approved local permission is no longer available".to_string());
                    action.dispatch_note = Some(
                        "permission grant is unavailable; action was not executed".to_string(),
                    );
                }
                _ if action.execution_state == "proposed" => {
                    dispatch_agent_operations_briefing_action(
                        store,
                        access_mode,
                        action,
                        false,
                        None,
                    )?;
                }
                _ => {}
            }
            continue;
        }

        match (action.capability, action.policy_decision, approval_state) {
            (Some(_), Some(PolicyDecision::Ask), AgentActionApprovalState::Rejected) => {
                action.execution_state = "blocked".to_string();
                action.blocked_reason =
                    Some("local permission request was rejected before dispatch".to_string());
                action.dispatch_note =
                    Some("local permission was rejected; action was not executed".to_string());
            }
            (Some(_), Some(PolicyDecision::Ask), AgentActionApprovalState::Unavailable) => {
                action.execution_state = "blocked".to_string();
                action.blocked_reason =
                    Some("approved local permission is no longer available".to_string());
                action.dispatch_note =
                    Some("permission grant is unavailable; action was not executed".to_string());
            }
            (
                Some(CapabilityKind::FileRead),
                Some(PolicyDecision::Ask),
                AgentActionApprovalState::Approved(approval_request_id),
            ) => {
                if action.action_type == "office_open" {
                    dispatch_agent_office_open_action(
                        store,
                        access_mode,
                        action,
                        file_write_client,
                        true,
                        Some(approval_request_id),
                    )?;
                } else {
                    dispatch_agent_file_read_action(
                        store,
                        access_mode,
                        action,
                        file_client,
                        true,
                        Some(approval_request_id),
                    )?;
                }
            }
            (
                Some(CapabilityKind::FileWrite),
                Some(PolicyDecision::Ask),
                AgentActionApprovalState::Approved(approval_request_id),
            ) => {
                if action.action_type == "office_create" {
                    dispatch_agent_office_create_action(
                        store,
                        access_mode,
                        action,
                        file_write_client,
                        true,
                        Some(approval_request_id),
                    )?;
                } else if action.action_type == "office_update" {
                    dispatch_agent_office_update_action(
                        store,
                        access_mode,
                        action,
                        file_write_client,
                        true,
                        Some(approval_request_id),
                    )?;
                } else if is_agent_filesystem_mutation_action(&action.action_type) {
                    dispatch_agent_filesystem_mutation_action(
                        store,
                        access_mode,
                        action,
                        true,
                        Some(approval_request_id),
                    )?;
                } else {
                    dispatch_agent_file_write_action(
                        store,
                        access_mode,
                        action,
                        file_write_client,
                        true,
                        Some(approval_request_id),
                    )?;
                }
            }
            (
                Some(CapabilityKind::NetworkSearch),
                Some(PolicyDecision::Ask),
                AgentActionApprovalState::Approved(approval_request_id),
            ) => {
                dispatch_agent_network_search_action(
                    store,
                    access_mode,
                    action,
                    search_client,
                    true,
                    Some(approval_request_id),
                )?;
            }
            (
                Some(CapabilityKind::TerminalRead),
                Some(PolicyDecision::Ask),
                AgentActionApprovalState::Approved(approval_request_id),
            ) => {
                dispatch_agent_terminal_read_action(
                    store,
                    access_mode,
                    action,
                    true,
                    Some(approval_request_id),
                    desktop_dir,
                )?;
            }
            (
                Some(CapabilityKind::BrowserBrowse),
                Some(PolicyDecision::Ask),
                AgentActionApprovalState::Approved(approval_request_id),
            ) => {
                dispatch_agent_browser_action(
                    store,
                    access_mode,
                    action,
                    browser_client,
                    true,
                    Some(approval_request_id),
                )?;
            }
            (Some(_), Some(PolicyDecision::Ask), AgentActionApprovalState::Approved(_)) => {
                action.dispatch_note =
                    Some("local permission is approved; executor wiring is pending".to_string());
            }
            (Some(_), Some(PolicyDecision::Ask), _) => {
                record_agent_action_permission_request(store, access_mode, action)?;
            }
            (Some(CapabilityKind::FileRead), Some(PolicyDecision::Allow), _) => {
                if action.action_type == "office_open" {
                    dispatch_agent_office_open_action(
                        store,
                        access_mode,
                        action,
                        file_write_client,
                        false,
                        None,
                    )?;
                } else {
                    dispatch_agent_file_read_action(
                        store,
                        access_mode,
                        action,
                        file_client,
                        false,
                        None,
                    )?;
                }
            }
            (Some(CapabilityKind::FileWrite), Some(PolicyDecision::Allow), _) => {
                if action.action_type == "office_create" {
                    dispatch_agent_office_create_action(
                        store,
                        access_mode,
                        action,
                        file_write_client,
                        false,
                        None,
                    )?;
                } else if action.action_type == "office_update" {
                    dispatch_agent_office_update_action(
                        store,
                        access_mode,
                        action,
                        file_write_client,
                        false,
                        None,
                    )?;
                } else if is_agent_filesystem_mutation_action(&action.action_type) {
                    dispatch_agent_filesystem_mutation_action(
                        store,
                        access_mode,
                        action,
                        false,
                        None,
                    )?;
                } else {
                    dispatch_agent_file_write_action(
                        store,
                        access_mode,
                        action,
                        file_write_client,
                        false,
                        None,
                    )?;
                }
            }
            (Some(CapabilityKind::NetworkSearch), Some(PolicyDecision::Allow), _) => {
                dispatch_agent_network_search_action(
                    store,
                    access_mode,
                    action,
                    search_client,
                    false,
                    None,
                )?;
            }
            (Some(CapabilityKind::TerminalRead), Some(PolicyDecision::Allow), _) => {
                dispatch_agent_terminal_read_action(
                    store,
                    access_mode,
                    action,
                    false,
                    None,
                    desktop_dir,
                )?;
            }
            (Some(CapabilityKind::BrowserBrowse), Some(PolicyDecision::Allow), _) => {
                dispatch_agent_browser_action(
                    store,
                    access_mode,
                    action,
                    browser_client,
                    false,
                    None,
                )?;
            }
            (Some(_), Some(PolicyDecision::Allow), _) => {
                action.dispatch_note = Some(
                    "local capability policy allowed this proposal; executor wiring is pending"
                        .to_string(),
                );
            }
            _ => {}
        }
    }

    Ok(())
}

fn dispatch_agent_action_proposals_with_store_mutex(
    store_mutex: &Mutex<EventStore>,
    access_mode: AccessMode,
    response: &mut AgentChatResponse,
    file_client: &impl FileContentClient,
    file_write_client: &impl AgentWritableArtifactClient,
    search_client: &impl NetworkSearchClient,
    browser_client: &impl BrowserPageClient,
    desktop_dir: Option<&Path>,
) -> Result<(), String> {
    if !response.missing_prerequisites.is_empty() {
        mark_agent_actions_waiting_for_prerequisites(response);
        return Ok(());
    }
    mark_office_open_actions_waiting_for_pending_create(response);

    for action in &mut response.proposed_actions {
        if action.execution_state == "blocked" {
            continue;
        }

        if matches!(
            action.execution_state.as_str(),
            "waiting_prerequisite" | "succeeded" | "failed"
        ) {
            continue;
        }

        let approval_state = {
            let store = store_mutex.lock().map_err(|_| lock_error())?;
            agent_action_approval_state(&store, action)?
        };

        match (action.capability, action.policy_decision, approval_state) {
            (Some(CapabilityKind::FileRead), Some(PolicyDecision::Allow), _) => {
                if action.action_type == "office_open" {
                    let store = store_mutex.lock().map_err(|_| lock_error())?;
                    dispatch_agent_office_open_action(
                        &store,
                        access_mode,
                        action,
                        file_write_client,
                        false,
                        None,
                    )?;
                } else {
                    dispatch_agent_file_read_action_with_store_mutex(
                        store_mutex,
                        access_mode,
                        action,
                        file_client,
                        false,
                        None,
                    )?;
                }
            }
            (
                Some(CapabilityKind::FileRead),
                Some(PolicyDecision::Ask),
                AgentActionApprovalState::Approved(approval_request_id),
            ) => {
                if action.action_type == "office_open" {
                    let store = store_mutex.lock().map_err(|_| lock_error())?;
                    dispatch_agent_office_open_action(
                        &store,
                        access_mode,
                        action,
                        file_write_client,
                        true,
                        Some(approval_request_id),
                    )?;
                } else {
                    dispatch_agent_file_read_action_with_store_mutex(
                        store_mutex,
                        access_mode,
                        action,
                        file_client,
                        true,
                        Some(approval_request_id),
                    )?;
                }
            }
            (Some(CapabilityKind::TerminalRead), Some(PolicyDecision::Allow), _) => {
                dispatch_agent_terminal_read_action_with_store_mutex(
                    store_mutex,
                    access_mode,
                    action,
                    false,
                    None,
                    desktop_dir,
                )?;
            }
            (
                Some(CapabilityKind::TerminalRead),
                Some(PolicyDecision::Ask),
                AgentActionApprovalState::Approved(approval_request_id),
            ) => {
                dispatch_agent_terminal_read_action_with_store_mutex(
                    store_mutex,
                    access_mode,
                    action,
                    true,
                    Some(approval_request_id),
                    desktop_dir,
                )?;
            }
            _ => {
                let mut single_response = AgentChatResponse {
                    id: response.id,
                    role: response.role.clone(),
                    content: response.content.clone(),
                    protocol_version: response.protocol_version.clone(),
                    proposed_actions: vec![action.clone()],
                    missing_prerequisites: Vec::new(),
                    memory_candidates: Vec::new(),
                    model: response.model.clone(),
                    cache_status: response.cache_status,
                    elapsed_ms: response.elapsed_ms,
                    prompt_tokens: response.prompt_tokens,
                    completion_tokens: response.completion_tokens,
                    total_tokens: response.total_tokens,
                    estimated_cost_micro_usd: response.estimated_cost_micro_usd,
                    created_at: response.created_at,
                };
                {
                    let store = store_mutex.lock().map_err(|_| lock_error())?;
                    dispatch_agent_action_proposals_with_desktop_dir(
                        &store,
                        access_mode,
                        &mut single_response,
                        file_client,
                        file_write_client,
                        search_client,
                        browser_client,
                        desktop_dir,
                    )?;
                }
                if let Some(updated_action) = single_response.proposed_actions.into_iter().next() {
                    *action = updated_action;
                }
            }
        }
    }

    Ok(())
}

fn resume_agent_chat_action_with_clients(
    store: &EventStore,
    access_mode: AccessMode,
    action: AgentChatActionProposal,
    file_client: &impl FileContentClient,
    file_write_client: &impl AgentWritableArtifactClient,
    search_client: &impl NetworkSearchClient,
    browser_client: &impl BrowserPageClient,
    desktop_dir: Option<&Path>,
) -> Result<AgentChatActionProposal, String> {
    let mut response = AgentChatResponse {
        id: Uuid::new_v4(),
        role: "assistant".to_string(),
        content: String::new(),
        protocol_version: "ds-agent-action-resume-v1".to_string(),
        proposed_actions: vec![action],
        missing_prerequisites: Vec::new(),
        memory_candidates: Vec::new(),
        model: "local-ds-agent-dispatch".to_string(),
        cache_status: DeepSeekChatCacheStatus::Miss,
        elapsed_ms: 0,
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        estimated_cost_micro_usd: None,
        created_at: Utc::now(),
    };
    dispatch_agent_action_proposals_with_desktop_dir(
        store,
        access_mode,
        &mut response,
        file_client,
        file_write_client,
        search_client,
        browser_client,
        desktop_dir,
    )?;
    response
        .proposed_actions
        .into_iter()
        .next()
        .ok_or_else(|| "agent action resume returned no action".to_string())
}

fn agent_action_approval_state(
    store: &EventStore,
    action: &AgentChatActionProposal,
) -> Result<AgentActionApprovalState, String> {
    let Some(permission_request_id) = action.permission_request_id else {
        return Ok(AgentActionApprovalState::NoRequest);
    };
    let Some(record) = store
        .list_capability_access_records()
        .map_err(event_store_error)?
        .into_iter()
        .find(|record| record.request.id == permission_request_id)
    else {
        return Ok(AgentActionApprovalState::Unavailable);
    };
    if action
        .capability
        .is_some_and(|capability| capability != record.request.capability)
    {
        return Ok(AgentActionApprovalState::Unavailable);
    }

    match record.effective_status {
        crate::kernel::policy::CapabilityAccessStatus::Approved
            if matches!(
                record.grant_state,
                CapabilityGrantState::Reusable | CapabilityGrantState::OneShotAvailable
            ) =>
        {
            Ok(AgentActionApprovalState::Approved(permission_request_id))
        }
        crate::kernel::policy::CapabilityAccessStatus::Rejected
        | crate::kernel::policy::CapabilityAccessStatus::Denied => {
            Ok(AgentActionApprovalState::Rejected)
        }
        crate::kernel::policy::CapabilityAccessStatus::PendingApproval => {
            Ok(AgentActionApprovalState::Pending)
        }
        _ => Ok(AgentActionApprovalState::Unavailable),
    }
}

fn mark_agent_actions_waiting_for_prerequisites(response: &mut AgentChatResponse) {
    let prerequisite_kinds = response
        .missing_prerequisites
        .iter()
        .map(|prerequisite| prerequisite.kind.trim())
        .filter(|kind| !kind.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let dispatch_note = if prerequisite_kinds.is_empty() {
        "waiting for missing prerequisites before dispatch".to_string()
    } else {
        format!("waiting for missing prerequisites before dispatch: {prerequisite_kinds}")
    };

    for action in &mut response.proposed_actions {
        if action.execution_state == "blocked" {
            continue;
        }
        action.execution_state = "waiting_prerequisite".to_string();
        action.dispatch_note = Some(dispatch_note.clone());
    }
}

fn mark_office_open_actions_waiting_for_pending_create(response: &mut AgentChatResponse) {
    let pending_create_targets = response
        .proposed_actions
        .iter()
        .filter(|action| action.action_type == "office_create")
        .filter(|action| {
            matches!(
                action.execution_state.as_str(),
                "needs_confirmation" | "waiting_prerequisite" | "blocked" | "failed"
            )
        })
        .filter_map(|action| normalized_agent_action_target(action))
        .collect::<Vec<_>>();

    if pending_create_targets.is_empty() {
        return;
    }

    for action in &mut response.proposed_actions {
        if action.action_type != "office_open" {
            continue;
        }
        if !matches!(
            action.execution_state.as_str(),
            "proposed" | "needs_confirmation"
        ) {
            continue;
        }
        let Some(target) = normalized_agent_action_target(action) else {
            continue;
        };
        if !pending_create_targets
            .iter()
            .any(|pending| pending == &target)
        {
            continue;
        }

        action.execution_state = "waiting_prerequisite".to_string();
        action.dispatch_note =
            Some("waiting for Office file creation before opening it".to_string());
    }
}

fn normalized_agent_action_target(action: &AgentChatActionProposal) -> Option<String> {
    action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|target| !target.is_empty())
        .map(|target| {
            apply_agent_action_target_location(target, action.target_location.as_deref())
                .to_ascii_lowercase()
        })
}

fn agent_action_requires_workspace(action: &AgentChatActionProposal) -> bool {
    matches!(
        action.action_type.as_str(),
        "create_report"
            | "file_write"
            | "office_create"
            | "office_open"
            | "office_update"
            | "operations_briefing"
            | "work_package_export"
    )
}

fn mark_agent_workspace_actions_waiting_if_needed(
    runtime_context: &AgentChatRuntimeContext,
    response: &mut AgentChatResponse,
) {
    if runtime_context.workspace_ready != AgentChatReadiness::Missing {
        return;
    }

    let mut found_workspace_action = false;
    for action in &mut response.proposed_actions {
        if !agent_action_requires_workspace(action) {
            continue;
        }
        if matches!(
            action.execution_state.as_str(),
            "blocked" | "succeeded" | "failed"
        ) {
            continue;
        }

        found_workspace_action = true;
        action.execution_state = "waiting_prerequisite".to_string();
        action.dispatch_note =
            Some("waiting for workspace setup before local artifact dispatch".to_string());
    }

    if found_workspace_action
        && !response
            .missing_prerequisites
            .iter()
            .any(|prerequisite| prerequisite.kind.trim() == "workspace")
    {
        response
            .missing_prerequisites
            .push(AgentChatMissingPrerequisite {
                kind: "workspace".to_string(),
                message: "请先选择一个 DS Agent 工作目录。DS Agent 会在这个根目录下自动创建报告、证据、导出、运行记录和工作包等子目录。"
                    .to_string(),
            });
    }
}

fn record_agent_action_permission_request(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
) -> Result<(), String> {
    let Some(capability) = action.capability else {
        return Ok(());
    };
    if action.permission_request_id.is_some() {
        return Ok(());
    }

    let request = build_capability_access_request(access_mode, capability)?;
    let entry = PermissionAuditEntry::evaluate(access_mode, capability);
    store
        .append_capability_access_request(&request)
        .map_err(event_store_error)?;
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;

    action.permission_request_id = Some(request.id);
    action.execution_state = "needs_confirmation".to_string();
    action.dispatch_note =
        Some("waiting for local permission approval before dispatch".to_string());
    Ok(())
}

fn dispatch_agent_file_read_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    file_client: &impl FileContentClient,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let Some(target) = action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason = Some("file_read target is required before dispatch".to_string());
        return Ok(());
    };

    let outcome = run_file_read(
        FileReadRequest {
            access_mode,
            path: target.to_string(),
            approval_granted,
        },
        file_client,
    )?;
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileRead);
    if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.dispatch_note = agent_action_dispatch_note(&invocation);
    Ok(())
}

fn dispatch_agent_file_read_action_with_store_mutex(
    store_mutex: &Mutex<EventStore>,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    file_client: &impl FileContentClient,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let Some(target) = action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason = Some("file_read target is required before dispatch".to_string());
        return Ok(());
    };

    let outcome = run_file_read(
        FileReadRequest {
            access_mode,
            path: target.to_string(),
            approval_granted,
        },
        file_client,
    )?;
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileRead);

    {
        let store = store_mutex.lock().map_err(|_| lock_error())?;
        if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
            store
                .append_capability_access_request(&outcome.access_request)
                .map_err(event_store_error)?;
        }
        store
            .append_permission_audit_entry(&entry)
            .map_err(event_store_error)?;
        store
            .append_capability_invocation(&invocation)
            .map_err(event_store_error)?;
    }

    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.dispatch_note = agent_action_dispatch_note(&invocation);
    Ok(())
}

fn agent_terminal_read_client() -> Result<LocalTerminalReadClient, String> {
    let working_dir = std::env::current_dir().map_err(event_store_error)?;
    Ok(LocalTerminalReadClient::new(working_dir, 4_000))
}

fn agent_terminal_read_command_from_target(
    target: &str,
    desktop_dir: Option<&Path>,
) -> Result<String, String> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Err("terminal_read target or command is required before dispatch".to_string());
    }

    let normalized = trimmed.replace('\\', "/");
    let normalized_lower = normalized.trim_end_matches('/').to_ascii_lowercase();
    if matches!(
        normalized_lower.as_str(),
        "desktop" | "user_desktop" | "windows_desktop" | "桌面"
    ) {
        let desktop_dir = desktop_dir
            .ok_or_else(|| "desktop directory is unavailable on this machine".to_string())?;
        return Ok(format!("ds-agent:list-directory {}", desktop_dir.display()));
    }

    for prefix in ["desktop/", "user_desktop/", "windows_desktop/", "桌面/"] {
        if normalized_lower.starts_with(prefix) {
            let desktop_dir = desktop_dir
                .ok_or_else(|| "desktop directory is unavailable on this machine".to_string())?;
            let relative = normalized[prefix.len()..].trim_matches('/');
            if relative.is_empty() {
                return Ok(format!("ds-agent:list-directory {}", desktop_dir.display()));
            }
            return Ok(format!(
                "ds-agent:list-directory {}",
                desktop_dir.join(relative).display()
            ));
        }
    }

    Ok(trimmed.to_string())
}

fn dispatch_agent_terminal_read_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
    desktop_dir: Option<&Path>,
) -> Result<(), String> {
    let Some(command) = action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason =
            Some("terminal_read target or command is required before dispatch".to_string());
        return Ok(());
    };

    let command = match agent_terminal_read_command_from_target(command, desktop_dir) {
        Ok(command) => command,
        Err(error) => {
            action.execution_state = "blocked".to_string();
            action.blocked_reason = Some(error.clone());
            action.dispatch_note = Some(error);
            return Ok(());
        }
    };

    let client = agent_terminal_read_client()?;
    let outcome = match run_terminal_read_capability(
        TerminalReadRequest {
            access_mode,
            command,
            approval_granted,
        },
        &client,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            action.execution_state = "failed".to_string();
            action.blocked_reason = Some(error.clone());
            action.dispatch_note = Some(error);
            return Ok(());
        }
    };
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::TerminalRead);
    if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.dispatch_note = agent_action_dispatch_note(&invocation);
    Ok(())
}

fn dispatch_agent_terminal_read_action_with_store_mutex(
    store_mutex: &Mutex<EventStore>,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
    desktop_dir: Option<&Path>,
) -> Result<(), String> {
    let Some(command) = action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason =
            Some("terminal_read target or command is required before dispatch".to_string());
        return Ok(());
    };

    let command = match agent_terminal_read_command_from_target(command, desktop_dir) {
        Ok(command) => command,
        Err(error) => {
            action.execution_state = "blocked".to_string();
            action.blocked_reason = Some(error.clone());
            action.dispatch_note = Some(error);
            return Ok(());
        }
    };

    let client = agent_terminal_read_client()?;
    let outcome = match run_terminal_read_capability(
        TerminalReadRequest {
            access_mode,
            command,
            approval_granted,
        },
        &client,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            action.execution_state = "failed".to_string();
            action.blocked_reason = Some(error.clone());
            action.dispatch_note = Some(error);
            return Ok(());
        }
    };
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::TerminalRead);

    {
        let store = store_mutex.lock().map_err(|_| lock_error())?;
        if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
            store
                .append_capability_access_request(&outcome.access_request)
                .map_err(event_store_error)?;
        }
        store
            .append_permission_audit_entry(&entry)
            .map_err(event_store_error)?;
        store
            .append_capability_invocation(&invocation)
            .map_err(event_store_error)?;
    }

    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.dispatch_note = agent_action_dispatch_note(&invocation);
    Ok(())
}

fn dispatch_agent_operations_briefing_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let Some(evidence_folder_path) = action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason = Some(
            "operations_briefing target evidence folder is required before dispatch".to_string(),
        );
        return Ok(());
    };

    let client = LocalEvidenceFolderClient::new(20, 512 * 1024);
    let outcome = build_operations_briefing_run(
        OperationsBriefingRequest {
            access_mode,
            evidence_folder_path: evidence_folder_path.to_string(),
            approval_granted,
        },
        &client,
    )?;
    let mut evidence_invocation = outcome.evidence_invocation;
    evidence_invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileRead);
    if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&evidence_invocation)
        .map_err(event_store_error)?;
    store
        .append_operations_briefing_run(&outcome.run)
        .map_err(event_store_error)?;

    if outcome.access_request.decision == PolicyDecision::Ask {
        action.permission_request_id = Some(outcome.access_request.id);
    }
    action.capability_invocation_id = Some(evidence_invocation.id);
    action.workflow_run_id = Some(outcome.run.id);
    action.execution_state = agent_action_state_from_operations_briefing_run(&outcome.run);
    action.dispatch_note = Some(agent_operations_briefing_dispatch_note(&outcome.run));
    Ok(())
}

fn dispatch_agent_office_create_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    office_client: &impl OfficeArtifactClient,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let spec = match office_create_spec_from_action(
        &action.action_type,
        action.target.as_deref(),
        action.target_location.as_deref(),
        action.title.as_deref(),
        action.reason.as_deref(),
        action.content.as_deref(),
    ) {
        Ok(spec) => spec,
        Err(error) => {
            action.execution_state = "blocked".to_string();
            action.blocked_reason = Some(error);
            action.dispatch_note = Some("office file was not created".to_string());
            return Ok(());
        }
    };

    let outcome = run_office_create_boundary(
        OfficeCreateRequest {
            access_mode,
            spec: spec.clone(),
            approval_granted,
        },
        office_client,
    )?;
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let mut entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileWrite);
    entry.decision = outcome.access_request.decision;
    entry.reason = outcome.access_request.reason.clone();
    if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    if action.permission_request_id.is_none()
        && outcome.access_request.decision == PolicyDecision::Ask
    {
        action.permission_request_id = Some(outcome.access_request.id);
    }
    action.target = Some(spec.path.clone());
    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.blocked_reason = if invocation.status == CapabilityInvocationStatus::Failed {
        invocation
            .warnings
            .first()
            .cloned()
            .or_else(|| invocation.excerpt.clone())
    } else {
        None
    };
    action.dispatch_note = Some(agent_office_create_dispatch_note(
        &invocation,
        outcome.result,
        &spec,
    ));
    Ok(())
}

fn dispatch_agent_office_open_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    office_client: &impl OfficeOpenClient,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let Some(target) = action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason = Some("office_open target is required before dispatch".to_string());
        return Ok(());
    };
    let target = apply_agent_action_target_location(target, action.target_location.as_deref());
    let Some(preferred_app) =
        infer_agent_action_office_app(action).or_else(|| OfficeApp::from_path(&target))
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason =
            Some("office_open target must be a .docx, .xlsx, or .pptx file".to_string());
        return Ok(());
    };

    let outcome = run_office_open_boundary(
        OfficeOpenRequest {
            access_mode,
            path: target.clone(),
            preferred_app: Some(preferred_app),
            approval_granted,
        },
        office_client,
    )?;
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileRead);
    if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    if action.permission_request_id.is_none()
        && outcome.access_request.decision == PolicyDecision::Ask
    {
        action.permission_request_id = Some(outcome.access_request.id);
    }
    action.target = Some(target.replace('\\', "/"));
    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.blocked_reason = if invocation.status == CapabilityInvocationStatus::Failed {
        invocation
            .warnings
            .first()
            .cloned()
            .or_else(|| invocation.excerpt.clone())
    } else {
        None
    };
    action.dispatch_note = Some(agent_office_open_dispatch_note(&invocation, outcome.result));
    Ok(())
}

fn dispatch_agent_office_update_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    office_client: &impl OfficeUpdateClient,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let spec = match office_update_spec_from_action(
        &action.action_type,
        action.target.as_deref(),
        action.target_location.as_deref(),
        action.title.as_deref(),
        action.reason.as_deref(),
        action.content.as_deref(),
    ) {
        Ok(spec) => spec,
        Err(error) => {
            action.execution_state = "blocked".to_string();
            action.blocked_reason = Some(error);
            action.dispatch_note = Some("office file was not updated".to_string());
            return Ok(());
        }
    };

    let outcome = run_office_update_boundary(
        OfficeUpdateRequest {
            access_mode,
            spec: spec.clone(),
            approval_granted,
        },
        office_client,
    )?;
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileWrite);
    if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    if action.permission_request_id.is_none()
        && outcome.access_request.decision == PolicyDecision::Ask
    {
        action.permission_request_id = Some(outcome.access_request.id);
    }
    action.target = Some(spec.path);
    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.blocked_reason = if invocation.status == CapabilityInvocationStatus::Failed {
        invocation
            .warnings
            .first()
            .cloned()
            .or_else(|| invocation.excerpt.clone())
    } else {
        None
    };
    action.dispatch_note = Some(agent_office_update_dispatch_note(
        &invocation,
        outcome.result,
    ));
    Ok(())
}

fn infer_agent_action_office_app(action: &AgentChatActionProposal) -> Option<OfficeApp> {
    action
        .target
        .as_deref()
        .and_then(OfficeApp::from_path)
        .or_else(|| action.title.as_deref().and_then(OfficeApp::from_label))
        .or_else(|| action.reason.as_deref().and_then(OfficeApp::from_label))
        .or_else(|| action.content.as_deref().and_then(OfficeApp::from_label))
}

fn agent_office_update_dispatch_note(
    invocation: &CapabilityInvocation,
    result: Option<OfficeUpdateResult>,
) -> String {
    match invocation.status {
        CapabilityInvocationStatus::Succeeded => result
            .map(|result| {
                format!(
                    "{} updated: {} ({})",
                    result.app.user_facing_name(),
                    result.path,
                    result.summary
                )
            })
            .unwrap_or_else(|| "office file updated".to_string()),
        CapabilityInvocationStatus::PendingApproval => {
            "office file update is waiting for approval".to_string()
        }
        CapabilityInvocationStatus::Failed => invocation
            .warnings
            .first()
            .map(|warning| format!("office file update failed: {warning}"))
            .unwrap_or_else(|| "office file update failed".to_string()),
    }
}

fn agent_office_open_dispatch_note(
    invocation: &CapabilityInvocation,
    result: Option<OfficeOpenResult>,
) -> String {
    match invocation.status {
        CapabilityInvocationStatus::Succeeded => result
            .map(|result| {
                let fallback = result
                    .fallback_note
                    .map(|note| format!(" ({note})"))
                    .unwrap_or_default();
                format!(
                    "{} opened: {}{}",
                    result.app.user_facing_name(),
                    result.path,
                    fallback
                )
            })
            .unwrap_or_else(|| "office file opened".to_string()),
        CapabilityInvocationStatus::PendingApproval => {
            "office file open is waiting for approval".to_string()
        }
        CapabilityInvocationStatus::Failed => invocation
            .warnings
            .first()
            .map(|warning| format!("office file open failed: {warning}"))
            .unwrap_or_else(|| "office file open failed".to_string()),
    }
}

fn agent_office_create_dispatch_note(
    invocation: &CapabilityInvocation,
    result: Option<OfficeCreateResult>,
    spec: &OfficeCreateSpec,
) -> String {
    match invocation.status {
        CapabilityInvocationStatus::Succeeded => result
            .map(|result| {
                let mut note =
                    format!("{} created: {}", result.app.user_facing_name(), result.path);
                if let Some(evidence) = agent_office_create_content_evidence(spec) {
                    note.push_str("\nCreated content:\n");
                    note.push_str(&evidence);
                }
                note
            })
            .unwrap_or_else(|| "office file created".to_string()),
        CapabilityInvocationStatus::PendingApproval => {
            "office file creation is waiting for approval".to_string()
        }
        CapabilityInvocationStatus::Failed => invocation
            .warnings
            .first()
            .map(|warning| format!("office file creation failed: {warning}"))
            .unwrap_or_else(|| "office file creation failed".to_string()),
    }
}

fn agent_office_create_content_evidence(spec: &OfficeCreateSpec) -> Option<String> {
    let evidence = match spec.app {
        OfficeApp::Word => spec.body.trim().to_string(),
        OfficeApp::Excel => spec
            .rows
            .iter()
            .take(20)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.trim())
                    .collect::<Vec<_>>()
                    .join(" | ")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        OfficeApp::PowerPoint => spec
            .slides
            .iter()
            .take(20)
            .enumerate()
            .map(|(index, slide)| {
                let title = slide.title.trim();
                let body = slide.body.trim();
                match (title.is_empty(), body.is_empty()) {
                    (true, true) => format!("Slide {}", index + 1),
                    (false, true) => format!("Slide {}: {}", index + 1, title),
                    (true, false) => format!("Slide {}: {}", index + 1, body),
                    (false, false) => format!("Slide {}: {}\n{}", index + 1, title, body),
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    compact_agent_tool_evidence_text(&evidence, AGENT_OFFICE_CREATE_EVIDENCE_TEXT_LIMIT)
}

fn compact_agent_tool_evidence_text(text: &str, limit: usize) -> Option<String> {
    let compact = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if compact.is_empty() {
        return None;
    }

    let mut truncated = String::new();
    let mut chars = compact.chars();
    for character in chars.by_ref().take(limit) {
        truncated.push(character);
    }
    if chars.next().is_some() {
        truncated.push_str("\n...");
    }
    Some(truncated)
}

fn dispatch_agent_file_write_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    file_write_client: &impl FileWriteClient,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let Some(target) = action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason = Some("file_write target is required before dispatch".to_string());
        return Ok(());
    };
    let Some(content) = action
        .content
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason = Some("file_write content is required before dispatch".to_string());
        return Ok(());
    };
    let summary = action
        .reason
        .as_deref()
        .or(action.title.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Write file proposed by DeepSeek");

    let outcome = run_file_write_boundary(
        FileWriteRequest {
            access_mode,
            path: target.to_string(),
            summary: summary.to_string(),
            content: content.to_string(),
            approval_granted,
        },
        file_write_client,
    )?;
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileWrite);
    if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.dispatch_note = agent_action_dispatch_note(&invocation);
    Ok(())
}

fn agent_action_destination(action: &AgentChatActionProposal) -> Option<String> {
    let direct = action.destination.as_deref();
    let content_fallback = action
        .content
        .as_deref()
        .filter(|value| Path::new(value.trim()).is_absolute());

    direct
        .or(content_fallback)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn dispatch_agent_filesystem_mutation_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let Some(operation) = agent_filesystem_mutation_operation(&action.action_type) else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason =
            Some("filesystem mutation action_type is required before dispatch".to_string());
        return Ok(());
    };
    let Some(target) = action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason =
            Some("filesystem mutation target is required before dispatch".to_string());
        return Ok(());
    };
    let destination = if matches!(
        operation,
        FileSystemMutationOperation::RenameFile | FileSystemMutationOperation::RenameDirectory
    ) {
        let Some(destination) = agent_action_destination(action) else {
            action.execution_state = "blocked".to_string();
            action.blocked_reason =
                Some("filesystem rename destination is required before dispatch".to_string());
            return Ok(());
        };
        Some(destination)
    } else {
        None
    };
    let content = if matches!(
        operation,
        FileSystemMutationOperation::CreateFile | FileSystemMutationOperation::UpdateFile
    ) {
        let Some(content) = action.content.clone() else {
            action.execution_state = "blocked".to_string();
            action.blocked_reason =
                Some("filesystem file content is required before dispatch".to_string());
            return Ok(());
        };
        Some(content)
    } else {
        None
    };

    let client = LocalFileSystemMutationClient;
    let outcome = match run_filesystem_mutation_boundary(
        FileSystemMutationRequest {
            access_mode,
            operation,
            path: target,
            destination,
            content,
            approval_granted,
        },
        &client,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            action.execution_state = "blocked".to_string();
            action.blocked_reason = Some(error.clone());
            action.dispatch_note = Some(error);
            return Ok(());
        }
    };
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileWrite);
    if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.dispatch_note = agent_action_dispatch_note(&invocation);
    Ok(())
}

fn dispatch_agent_network_search_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    search_client: &impl NetworkSearchClient,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let Some(query) = action
        .target
        .as_deref()
        .or(action.reason.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason =
            Some("network_search target or reason is required before dispatch".to_string());
        return Ok(());
    };

    let outcome = run_network_search_boundary(
        NetworkSearchRequest {
            access_mode,
            query: query.to_string(),
            scope: "public web".to_string(),
            approval_granted,
        },
        search_client,
    )?;
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::NetworkSearch);
    if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.dispatch_note = agent_action_dispatch_note(&invocation);
    Ok(())
}

fn agent_computer_control_target(action: &AgentChatActionProposal) -> Option<String> {
    action
        .target
        .as_deref()
        .or(action.title.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn agent_computer_control_action_contract(action: &AgentChatActionProposal) -> Option<String> {
    [
        action.content.as_deref(),
        action.reason.as_deref(),
        action.target.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .find(|value| {
        let lower = value.to_ascii_lowercase();
        matches!(
            lower.split_once(':').map(|(verb, _)| verb.trim()),
            Some("click" | "move" | "type" | "press" | "hotkey" | "scroll")
        )
    })
    .map(ToString::to_string)
}

fn dispatch_agent_computer_control_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    computer_control_client: &impl ComputerControlClient,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let Some(target) = agent_computer_control_target(action) else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason =
            Some("computer_control target window or app is required before dispatch".to_string());
        action.dispatch_note = Some("computer control action was not executed".to_string());
        return Ok(());
    };
    let Some(control_action) = agent_computer_control_action_contract(action) else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason = Some(
            "computer_control content must be one structured action: click:x,y[,button], move:x,y, type:text, press:key, hotkey:key+key, or scroll:delta[,axis]"
                .to_string(),
        );
        action.dispatch_note = Some("computer control action was not executed".to_string());
        return Ok(());
    };

    let outcome = match run_computer_control_boundary(
        ComputerControlRequest {
            access_mode,
            target,
            action: control_action,
            approval_granted,
        },
        computer_control_client,
    ) {
        Ok(outcome) => outcome,
        Err(error) => {
            action.execution_state = "blocked".to_string();
            action.blocked_reason = Some(error);
            action.dispatch_note = Some("computer control action was not executed".to_string());
            return Ok(());
        }
    };

    let mut invocation = outcome.invocation;
    let should_record_access_request =
        !approval_granted || outcome.access_request.decision == PolicyDecision::Allow;
    invocation.approval_request_id = approval_request_id
        .or_else(|| should_record_access_request.then_some(outcome.access_request.id));
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::ComputerControl);

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    if action.permission_request_id.is_none()
        && outcome.access_request.decision == PolicyDecision::Ask
    {
        action.permission_request_id = Some(outcome.access_request.id);
    }
    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.blocked_reason = if invocation.status == CapabilityInvocationStatus::Failed {
        invocation
            .warnings
            .first()
            .cloned()
            .or_else(|| invocation.excerpt.clone())
    } else {
        None
    };
    action.dispatch_note = agent_action_dispatch_note(&invocation);
    Ok(())
}

fn dispatch_agent_browser_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    browser_client: &impl BrowserPageClient,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    if action.action_type == "browser_open" {
        dispatch_agent_browser_open_action(
            store,
            access_mode,
            action,
            approval_granted,
            approval_request_id,
        )
    } else {
        dispatch_agent_browser_browse_action(
            store,
            access_mode,
            action,
            browser_client,
            approval_granted,
            approval_request_id,
        )
    }
}

fn dispatch_agent_browser_open_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let opener = SystemBrowserUrlOpener;
    dispatch_agent_browser_open_action_with_opener(
        store,
        access_mode,
        action,
        approval_granted,
        approval_request_id,
        &opener,
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BrowserUrlOpenOutcome {
    browser_label: String,
    fallback_note: Option<String>,
}

trait BrowserUrlOpener {
    fn open_url(
        &self,
        url: &str,
        preferred_browser: Option<&str>,
    ) -> Result<BrowserUrlOpenOutcome, String>;
}

struct SystemBrowserUrlOpener;

impl BrowserUrlOpener for SystemBrowserUrlOpener {
    fn open_url(
        &self,
        url: &str,
        preferred_browser: Option<&str>,
    ) -> Result<BrowserUrlOpenOutcome, String> {
        if normalize_agent_action_browser_preference(preferred_browser).as_deref() == Some("chrome")
        {
            return open_url_in_chrome_or_default_browser(url);
        }

        open_url_in_default_browser(url).map(|()| BrowserUrlOpenOutcome {
            browser_label: "default browser".to_string(),
            fallback_note: None,
        })
    }
}

fn dispatch_agent_browser_open_action_with_opener(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
    opener: &impl BrowserUrlOpener,
) -> Result<(), String> {
    let Some(url) = action
        .target
        .as_deref()
        .and_then(extract_safe_browser_url)
        .filter(|value| !value.is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason =
            Some("browser_open target URL is required before dispatch".to_string());
        return Ok(());
    };

    let started_at = Instant::now();
    let access_request =
        build_capability_access_request(access_mode, CapabilityKind::BrowserBrowse)?;
    let preferred_browser =
        normalize_agent_action_browser_preference(action.preferred_browser.as_deref());
    let mut invocation = if access_request.decision == PolicyDecision::Ask && !approval_granted {
        CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::BrowserBrowse,
            status: CapabilityInvocationStatus::PendingApproval,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(url.clone()),
            evidence_ref: Some(url.clone()),
            requested_url: Some(url.clone()),
            evidence_url: Some(url.clone()),
            title: Some("Browser open request".to_string()),
            excerpt: None,
            warnings: vec!["browser open requires approval before launching the URL".to_string()],
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        }
    } else {
        match opener.open_url(&url, preferred_browser.as_deref()) {
            Ok(outcome) => {
                let warnings = outcome.fallback_note.into_iter().collect::<Vec<_>>();
                CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::BrowserBrowse,
                    status: CapabilityInvocationStatus::Succeeded,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(url.clone()),
                    evidence_ref: Some(url.clone()),
                    requested_url: Some(url.clone()),
                    evidence_url: Some(url.clone()),
                    title: Some(format!("Browser opened with {}", outcome.browser_label)),
                    excerpt: Some(url.clone()),
                    warnings,
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                }
            }
            Err(error) => CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::BrowserBrowse,
                status: CapabilityInvocationStatus::Failed,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(url.clone()),
                evidence_ref: Some(url.clone()),
                requested_url: Some(url.clone()),
                evidence_url: Some(url),
                title: Some("Browser open failed".to_string()),
                excerpt: None,
                warnings: vec![error],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
        }
    };
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::BrowserBrowse);
    if !approval_granted || access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.dispatch_note = agent_action_dispatch_note(&invocation);
    Ok(())
}

fn open_url_in_chrome_or_default_browser(url: &str) -> Result<BrowserUrlOpenOutcome, String> {
    match open_url_in_chrome(url) {
        Ok(()) => Ok(BrowserUrlOpenOutcome {
            browser_label: "Chrome".to_string(),
            fallback_note: None,
        }),
        Err(chrome_error) => {
            let fallback_note = if chrome_error.contains("could not be found") {
                format!("未检测到 Chrome，已使用默认浏览器打开 {url}")
            } else {
                format!("Chrome 启动失败，已使用默认浏览器打开 {url}")
            };
            open_url_in_default_browser(url)
                .map(|()| BrowserUrlOpenOutcome {
                    browser_label: "default browser".to_string(),
                    fallback_note: Some(fallback_note),
                })
                .map_err(|default_error| {
                    format!("{chrome_error}; default browser fallback also failed: {default_error}")
                })
        }
    }
}

fn open_url_in_chrome(url: &str) -> Result<(), String> {
    let Some(chrome_path) = find_chrome_executable() else {
        return Err("Chrome browser could not be found".to_string());
    };

    Command::new(&chrome_path)
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Chrome could not be launched: {error}"))
}

fn find_chrome_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    if cfg!(target_os = "windows") {
        for variable in [
            "ProgramFiles",
            "PROGRAMFILES",
            "ProgramFiles(x86)",
            "PROGRAMFILES(X86)",
            "LocalAppData",
            "LOCALAPPDATA",
        ] {
            if let Some(base) = std::env::var_os(variable) {
                candidates.push(
                    PathBuf::from(base)
                        .join("Google")
                        .join("Chrome")
                        .join("Application")
                        .join("chrome.exe"),
                );
            }
        }
        candidates.extend(find_executable_on_path(&["chrome.exe", "chrome"]));
    } else if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from(
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        ));
        candidates.extend(find_executable_on_path(&["google-chrome", "chrome"]));
    } else {
        candidates.extend(find_executable_on_path(&[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "chrome",
        ]));
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn find_executable_on_path(names: &[&str]) -> Vec<PathBuf> {
    let Some(path_value) = std::env::var_os("PATH") else {
        return Vec::new();
    };

    std::env::split_paths(&path_value)
        .flat_map(|directory| {
            names
                .iter()
                .map(move |name| directory.join(name))
                .collect::<Vec<_>>()
        })
        .filter(|candidate| candidate.is_file())
        .collect()
}

fn open_url_in_default_browser(url: &str) -> Result<(), String> {
    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler").arg(url);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("default browser URL could not be opened: {error}"))
}

fn dispatch_agent_browser_browse_action(
    store: &EventStore,
    access_mode: AccessMode,
    action: &mut AgentChatActionProposal,
    browser_client: &impl BrowserPageClient,
    approval_granted: bool,
    approval_request_id: Option<Uuid>,
) -> Result<(), String> {
    let Some(url) = action
        .target
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        action.execution_state = "blocked".to_string();
        action.blocked_reason =
            Some("browser_browse target URL is required before dispatch".to_string());
        return Ok(());
    };

    let outcome = run_browser_browse(
        BrowserBrowseRequest {
            access_mode,
            url: url.to_string(),
            approval_granted,
        },
        browser_client,
    )?;
    let mut invocation = outcome.invocation;
    invocation.approval_request_id = approval_request_id;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::BrowserBrowse);
    if !approval_granted || outcome.access_request.decision == PolicyDecision::Allow {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&invocation)
        .map_err(event_store_error)?;

    action.capability_invocation_id = Some(invocation.id);
    action.execution_state = agent_action_state_from_invocation(invocation.status);
    action.dispatch_note = agent_action_dispatch_note(&invocation);
    Ok(())
}

fn agent_action_state_from_invocation(status: CapabilityInvocationStatus) -> String {
    match status {
        CapabilityInvocationStatus::Succeeded => "succeeded",
        CapabilityInvocationStatus::PendingApproval => "needs_confirmation",
        CapabilityInvocationStatus::Failed => "failed",
    }
    .to_string()
}

fn agent_action_state_from_operations_briefing_run(run: &OperationsBriefingRun) -> String {
    match run.status {
        crate::kernel::workflow::OperationsBriefingRunStatus::DraftReady => "succeeded",
        crate::kernel::workflow::OperationsBriefingRunStatus::PendingApproval => {
            "needs_confirmation"
        }
        crate::kernel::workflow::OperationsBriefingRunStatus::Failed => "failed",
    }
    .to_string()
}

fn agent_operations_briefing_dispatch_note(run: &OperationsBriefingRun) -> String {
    let summary = run.summary.trim();
    match run.status {
        crate::kernel::workflow::OperationsBriefingRunStatus::DraftReady => {
            if summary.is_empty() {
                "operations briefing draft ready".to_string()
            } else {
                format!("operations briefing draft ready: {summary}")
            }
        }
        crate::kernel::workflow::OperationsBriefingRunStatus::PendingApproval => {
            "operations briefing waiting for FileRead approval".to_string()
        }
        crate::kernel::workflow::OperationsBriefingRunStatus::Failed => {
            if summary.is_empty() {
                "operations briefing failed".to_string()
            } else {
                format!("operations briefing failed: {summary}")
            }
        }
    }
}

fn agent_action_dispatch_note(invocation: &CapabilityInvocation) -> Option<String> {
    if invocation.capability == CapabilityKind::BrowserBrowse {
        return match invocation.status {
            CapabilityInvocationStatus::Succeeded => {
                if invocation
                    .title
                    .as_deref()
                    .is_some_and(|title| title.starts_with("Browser opened with "))
                {
                    return Some(agent_browser_open_dispatch_note(invocation));
                }

                let target = invocation
                    .title
                    .as_deref()
                    .or(invocation.evidence_url.as_deref())
                    .or(invocation.requested_url.as_deref())
                    .unwrap_or("browser action");
                let url = invocation
                    .evidence_url
                    .as_deref()
                    .or(invocation.requested_url.as_deref());
                Some(match url {
                    Some(url) if target != url => {
                        format!("browser action completed: {target} ({url})")
                    }
                    Some(url) => format!("browser action completed: {url}"),
                    None => format!("browser action completed: {target}"),
                })
            }
            CapabilityInvocationStatus::PendingApproval => {
                Some("browser action waiting for approval".to_string())
            }
            CapabilityInvocationStatus::Failed => invocation
                .warnings
                .first()
                .map(|warning| format!("browser action failed: {warning}"))
                .or_else(|| Some("browser action failed".to_string())),
        };
    }

    if invocation.capability == CapabilityKind::ComputerControl {
        return match invocation.status {
            CapabilityInvocationStatus::Succeeded => invocation
                .excerpt
                .as_deref()
                .map(str::trim)
                .filter(|excerpt| !excerpt.is_empty())
                .map(|excerpt| format!("computer control completed: {excerpt}"))
                .or_else(|| Some("computer control completed".to_string())),
            CapabilityInvocationStatus::PendingApproval => {
                Some("computer control waiting for approval".to_string())
            }
            CapabilityInvocationStatus::Failed => invocation
                .warnings
                .first()
                .map(|warning| format!("computer control failed: {warning}"))
                .or_else(|| Some("computer control failed".to_string())),
        };
    }

    invocation.excerpt.as_deref().map(|excerpt| {
        let action_label = match invocation.capability {
            CapabilityKind::FileRead => "file read",
            CapabilityKind::FileWrite => "file write",
            CapabilityKind::NetworkSearch => "network search",
            _ => "capability action",
        };
        if excerpt.trim().is_empty() {
            format!("{action_label} completed")
        } else {
            format!("{action_label} completed: {}", excerpt.trim())
        }
    })
}

fn agent_browser_open_dispatch_note(invocation: &CapabilityInvocation) -> String {
    if let Some(fallback_note) = invocation
        .warnings
        .iter()
        .find(|warning| warning.contains("默认浏览器"))
    {
        return fallback_note.clone();
    }

    let url = invocation
        .evidence_url
        .as_deref()
        .or(invocation.requested_url.as_deref())
        .unwrap_or("requested URL");
    let title = invocation.title.as_deref().unwrap_or_default();
    if title.contains("Chrome") {
        format!("已使用 Chrome 打开 {url}")
    } else {
        format!("已使用默认浏览器打开 {url}")
    }
}

fn agent_chat_runtime_context(
    app: &AppHandle,
    large_model_provider: LargeModelProvider,
    network_search_source_model: Option<NetworkSearchSourceModel>,
    deepseek_chat_ready: bool,
) -> AgentChatRuntimeContext {
    let app_data_dir = app.path().app_data_dir().ok();
    let soul_profile = app_data_dir
        .as_deref()
        .and_then(|app_data_dir| load_agent_soul_profile_context(app_data_dir).ok())
        .flatten();
    let (workspace_ready, workspace_note) = app_data_dir
        .as_deref()
        .and_then(|app_data_dir| load_local_directory_state(app_data_dir).ok())
        .map(|state| local_directory_readiness_from_state(&state))
        .map(|readiness| {
            (
                if readiness.needs_setup {
                    AgentChatReadiness::Missing
                } else {
                    AgentChatReadiness::Ready
                },
                readiness.note,
            )
        })
        .unwrap_or_else(|| {
            (
                AgentChatReadiness::Unknown,
                "workspace readiness unavailable".to_string(),
            )
        });

    let tool_strategy = model_driven_tool_strategy_for_current_platform(
        large_model_provider,
        network_search_source_model,
    );
    let network_status =
        network_search_route_status_for_strategy(&tool_strategy, deepseek_chat_ready);
    let network_search_ready = if network_status.network_requests_enabled {
        AgentChatReadiness::Ready
    } else {
        AgentChatReadiness::Missing
    };

    AgentChatRuntimeContext {
        workspace_ready,
        workspace_note,
        network_search_ready,
        network_search_note: network_status.note,
        network_search_source_model: tool_strategy.network_search_source_model,
        soul_profile,
        memory_context: AgentMemoryRuntimeContext::default(),
        desktop_dir: app.path().desktop_dir().ok(),
    }
}

#[tauri::command]
pub fn get_foundation_state() -> FoundationState {
    FoundationState::default()
}

#[tauri::command]
pub fn check_app_update() -> Result<AppUpdateStatus, String> {
    fetch_github_releases()
        .map(|releases| update_status_from_releases(releases, app_update_current_version()))
}

#[tauri::command]
pub fn download_app_update() -> Result<AppUpdateDownloadResult, String> {
    let releases = fetch_github_releases()?;
    let release = latest_installable_update_release(&releases, app_update_current_version())
        .ok_or_else(|| "DS Agent is already up to date".to_string())?;
    let latest_version = normalize_release_version(&release.tag_name);
    let asset = release_installable_asset(&release)
        .ok_or_else(|| "latest release has no Windows installer asset".to_string())?;
    let installer_path = download_release_asset(asset)?;

    Ok(AppUpdateDownloadResult {
        latest_version,
        asset_name: asset.name.clone(),
        installer_path: installer_path.display().to_string(),
    })
}

#[tauri::command]
pub fn install_app_update(
    app: AppHandle,
    installer_path: String,
) -> Result<AppUpdateInstallResult, String> {
    let installer_path = validate_downloaded_update_installer_path(&installer_path)?;
    spawn_silent_update_runner(&installer_path)?;
    let result = AppUpdateInstallResult {
        installer_path: installer_path.display().to_string(),
        restart_scheduled: true,
    };
    app.exit(0);
    Ok(result)
}

#[tauri::command]
pub fn get_deepseek_credential_status() -> DeepSeekCredentialStatus {
    current_deepseek_credential_status()
}

#[tauri::command]
pub fn stage_agent_attachments(
    paths: Vec<String>,
    existing_count: usize,
    existing_total_bytes: u64,
) -> Vec<AgentAttachment> {
    stage_agent_attachment_paths(paths, existing_count, existing_total_bytes)
}

#[tauri::command]
pub fn run_agent_chat(
    app: AppHandle,
    prompt: String,
    large_model_provider: LargeModelProvider,
    model_route: ModelRoute,
    thinking_level: ThinkingLevel,
    access_mode: AccessMode,
    network_search_source_model: Option<NetworkSearchSourceModel>,
    api_key_override: Option<String>,
    fallback_api_key_override: Option<String>,
    state: State<'_, AppState>,
) -> Result<AgentChatResponse, String> {
    let api_keys = agent_chat_api_key_candidates_from_sources(
        api_key_override,
        fallback_api_key_override,
        |name| std::env::var(name).ok(),
    );
    if api_keys.is_empty() {
        return Err("DeepSeek Chat is not configured. Provide a DeepSeek API key for this session or set DEEPSEEK_API_KEY in the local desktop process."
            .to_string());
    }
    let pricing_settings = app
        .path()
        .app_data_dir()
        .ok()
        .and_then(|app_data_dir| load_deepseek_pricing_state(app_data_dir).ok())
        .map(|pricing_state| pricing_state.settings);
    let runtime_context = agent_chat_runtime_context(
        &app,
        large_model_provider,
        network_search_source_model,
        true,
    );
    let transport = HttpDeepSeekChatCompletionTransport::new()?;
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    let directory_state = load_local_directory_state(&app_data_dir).map_err(event_store_error)?;
    let desktop_dir = runtime_context.desktop_dir.clone();
    let file_client = LocalFileContentClient::new(512 * 1024);
    let file_write_client = agent_file_write_client(&directory_state, desktop_dir)?;
    let browser_client = HttpBrowserPageClient::new()?;
    let search_client =
        agent_network_search_client(large_model_provider, network_search_source_model);
    run_agent_chat_with_clients_and_api_keys(
        &state.event_store,
        &transport,
        &state.deepseek_chat_cache,
        &api_keys,
        AgentChatRequest {
            prompt,
            model_route,
            thinking_level,
            access_mode,
        },
        runtime_context,
        pricing_settings.as_ref(),
        &file_client,
        &file_write_client,
        &search_client,
        &browser_client,
    )
}

#[tauri::command]
pub fn get_deepseek_user_balance(
    api_key_override: Option<String>,
    fallback_api_key_override: Option<String>,
) -> Result<DeepSeekUserBalanceResponse, String> {
    let api_keys = agent_chat_api_key_candidates_from_sources(
        api_key_override,
        fallback_api_key_override,
        |name| std::env::var(name).ok(),
    );
    if api_keys.is_empty() {
        return Err("DeepSeek API key is required to query user balance.".to_string());
    }

    let transport = HttpDeepSeekChatCompletionTransport::new()?;
    let mut errors = Vec::new();
    for api_key in api_keys {
        match execute_deepseek_user_balance(&transport, &api_key) {
            Ok(balance) => return Ok(balance),
            Err(error) => errors.push(error),
        }
    }

    Err(errors
        .last()
        .cloned()
        .unwrap_or_else(|| "DeepSeek balance query failed.".to_string()))
}

#[tauri::command]
pub fn resume_agent_chat_action(
    app: AppHandle,
    access_mode: AccessMode,
    large_model_provider: LargeModelProvider,
    network_search_source_model: Option<NetworkSearchSourceModel>,
    action: AgentChatActionProposal,
    state: State<'_, AppState>,
) -> Result<AgentChatActionProposal, String> {
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    let directory_state = load_local_directory_state(&app_data_dir).map_err(event_store_error)?;
    let desktop_dir = app.path().desktop_dir().ok();
    let normalized_action = normalize_agent_action_proposal(action, access_mode);
    if normalized_action.action_type == "computer_control" {
        let mut computer_action = normalized_action;
        let approval_state = {
            let store = state.event_store.lock().map_err(|_| lock_error())?;
            agent_action_approval_state(&store, &computer_action)?
        };

        match approval_state {
            AgentActionApprovalState::Approved(approval_request_id) => {
                let unlock_state = state
                    .computer_control_unlock
                    .lock()
                    .map_err(|_| computer_control_unlock_lock_error())?;
                if !unlock_state.is_unlocked(Utc::now()) {
                    computer_action.execution_state = "needs_confirmation".to_string();
                    computer_action.blocked_reason =
                        Some("computer control requires local unlock before execution".to_string());
                    computer_action.dispatch_note = Some(
                        "approve the Computer control unlock challenge, then continue this action"
                            .to_string(),
                    );
                    return Ok(computer_action);
                }
                drop(unlock_state);

                let strategy = computer_tool_strategy_for_command(
                    large_model_provider,
                    network_search_source_model,
                );
                match strategy.computer_control_backend {
                    ComputerControlBackend::LocalWindowsInputControl
                    | ComputerControlBackend::LocalMacosInputControl => {
                        let client = LocalComputerControlClient::new();
                        let store = state.event_store.lock().map_err(|_| lock_error())?;
                        dispatch_agent_computer_control_action(
                            &store,
                            access_mode,
                            &mut computer_action,
                            &client,
                            true,
                            Some(approval_request_id),
                        )?;
                    }
                    ComputerControlBackend::CodexBridgeInputControl
                    | ComputerControlBackend::CodexStyleInputControl => {
                        let client = CodexBridgeComputerControlClient::from_env();
                        let store = state.event_store.lock().map_err(|_| lock_error())?;
                        dispatch_agent_computer_control_action(
                            &store,
                            access_mode,
                            &mut computer_action,
                            &client,
                            true,
                            Some(approval_request_id),
                        )?;
                    }
                }
            }
            AgentActionApprovalState::Rejected => {
                computer_action.execution_state = "blocked".to_string();
                computer_action.blocked_reason =
                    Some("local permission request was rejected before dispatch".to_string());
                computer_action.dispatch_note =
                    Some("local permission was rejected; action was not executed".to_string());
            }
            AgentActionApprovalState::Unavailable => {
                computer_action.execution_state = "blocked".to_string();
                computer_action.blocked_reason =
                    Some("approved local permission is no longer available".to_string());
                computer_action.dispatch_note =
                    Some("permission grant is unavailable; action was not executed".to_string());
            }
            _ => {
                let store = state.event_store.lock().map_err(|_| lock_error())?;
                record_agent_action_permission_request(&store, access_mode, &mut computer_action)?;
            }
        }

        return Ok(computer_action);
    }
    let file_client = LocalFileContentClient::new(512 * 1024);
    let file_write_client = agent_file_write_client(&directory_state, desktop_dir.clone())?;
    let browser_client = HttpBrowserPageClient::new()?;
    let search_client =
        agent_network_search_client(large_model_provider, network_search_source_model);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    resume_agent_chat_action_with_clients(
        &store,
        access_mode,
        normalized_action,
        &file_client,
        &file_write_client,
        &search_client,
        &browser_client,
        desktop_dir.as_deref(),
    )
}

#[tauri::command]
pub fn list_deepseek_chat_telemetry(
    state: State<'_, AppState>,
) -> Result<Vec<DeepSeekChatTelemetry>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .list_deepseek_chat_telemetry()
        .map_err(event_store_error)
}

#[tauri::command]
pub fn list_agent_context_receipts(
    state: State<'_, AppState>,
) -> Result<Vec<AgentContextReceipt>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .list_agent_context_receipts()
        .map_err(event_store_error)
}

#[tauri::command]
pub fn get_deepseek_chat_cache_state(state: State<'_, AppState>) -> DeepSeekChatCacheState {
    state.deepseek_chat_cache.state()
}

#[tauri::command]
pub fn clear_deepseek_chat_cache(state: State<'_, AppState>) -> usize {
    state.deepseek_chat_cache.clear()
}

#[tauri::command]
pub fn get_deepseek_pricing_state(app: AppHandle) -> Result<DeepSeekPricingState, String> {
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    load_deepseek_pricing_state(app_data_dir).map_err(event_store_error)
}

#[tauri::command]
pub fn save_deepseek_pricing_settings(
    app: AppHandle,
    enabled: bool,
    flash_prompt_usd_per_million_tokens: String,
    flash_completion_usd_per_million_tokens: String,
    pro_prompt_usd_per_million_tokens: String,
    pro_completion_usd_per_million_tokens: String,
) -> Result<DeepSeekPricingState, String> {
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    persist_deepseek_pricing_settings(
        app_data_dir,
        DeepSeekPricingSettings {
            enabled,
            flash_prompt_usd_per_million_tokens,
            flash_completion_usd_per_million_tokens,
            pro_prompt_usd_per_million_tokens,
            pro_completion_usd_per_million_tokens,
        },
    )
    .map_err(event_store_error)
}

#[tauri::command]
pub fn get_network_search_route_status() -> NetworkSearchRouteStatus {
    let deepseek_status = current_deepseek_credential_status();
    let foundation_state = FoundationState::default();
    let tool_strategy = model_driven_tool_strategy_for_current_platform(
        foundation_state.large_model_provider,
        foundation_state.network_search_source_model,
    );
    network_search_route_status_for_strategy(&tool_strategy, deepseek_status.chat_completion_ready)
}

#[tauri::command]
pub fn get_computer_use_backend_status() -> ComputerUseBackendStatus {
    let foundation_state = FoundationState::default();
    let tool_strategy = model_driven_tool_strategy_for_current_platform(
        foundation_state.large_model_provider,
        foundation_state.network_search_source_model,
    );
    computer_use_backend_status_for_strategy(&tool_strategy)
}

#[tauri::command]
pub fn get_computer_control_unlock_status(
    state: State<'_, AppState>,
) -> Result<ComputerControlUnlockStatus, String> {
    let unlock_state = state
        .computer_control_unlock
        .lock()
        .map_err(|_| computer_control_unlock_lock_error())?;
    Ok(unlock_state.status(Utc::now()))
}

#[tauri::command]
pub fn unlock_computer_control(
    token: String,
    state: State<'_, AppState>,
) -> Result<ComputerControlUnlockStatus, String> {
    let mut unlock_state = state
        .computer_control_unlock
        .lock()
        .map_err(|_| computer_control_unlock_lock_error())?;
    unlock_state.unlock(&token, Utc::now())
}

#[tauri::command]
pub fn get_network_search_route_status_for_model(
    large_model_provider: LargeModelProvider,
    network_search_source_model: Option<NetworkSearchSourceModel>,
) -> NetworkSearchRouteStatus {
    let deepseek_status = current_deepseek_credential_status();
    let tool_strategy = model_driven_tool_strategy_for_current_platform(
        large_model_provider,
        network_search_source_model,
    );
    network_search_route_status_for_strategy(&tool_strategy, deepseek_status.chat_completion_ready)
}

#[tauri::command]
pub fn get_computer_use_backend_status_for_model(
    large_model_provider: LargeModelProvider,
    network_search_source_model: Option<NetworkSearchSourceModel>,
) -> ComputerUseBackendStatus {
    let tool_strategy = model_driven_tool_strategy_for_current_platform(
        large_model_provider,
        network_search_source_model,
    );
    computer_use_backend_status_for_strategy(&tool_strategy)
}

#[tauri::command]
pub fn get_model_driven_tool_strategy(
    large_model_provider: LargeModelProvider,
    network_search_source_model: Option<NetworkSearchSourceModel>,
) -> ModelDrivenToolStrategy {
    model_driven_tool_strategy_for_current_platform(
        large_model_provider,
        network_search_source_model,
    )
}

#[tauri::command]
pub fn get_local_directory_state(app: AppHandle) -> Result<LocalDirectoryState, String> {
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    load_local_directory_state(app_data_dir).map_err(event_store_error)
}

#[tauri::command]
pub fn get_agent_soul_profile(app: AppHandle) -> Result<AgentSoulProfileState, String> {
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    agent_soul_profile_state_from_app_data_dir(&app_data_dir)
}

#[tauri::command]
pub fn save_agent_soul_profile(
    app: AppHandle,
    content: String,
) -> Result<AgentSoulProfileState, String> {
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    save_agent_soul_profile_content(&app_data_dir, &content)
}

#[tauri::command]
pub fn save_local_directory_settings(
    app: AppHandle,
    workspace_dir: String,
    workspace_name: String,
) -> Result<LocalDirectoryState, String> {
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    let settings =
        LocalDirectorySettings::from_workspace_dir_and_name(workspace_dir, workspace_name)
            .map_err(event_store_error)?;
    persist_local_directory_settings(app_data_dir, settings).map_err(event_store_error)
}

#[tauri::command]
pub fn list_task_records(state: State<'_, AppState>) -> Result<Vec<TaskRecord>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store.list_task_records().map_err(event_store_error)
}

#[tauri::command]
pub fn list_memory_records(state: State<'_, AppState>) -> Result<Vec<MemoryRecord>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store.list_memory_records().map_err(event_store_error)
}

#[tauri::command]
pub fn list_memory_candidate_records(
    state: State<'_, AppState>,
) -> Result<Vec<MemoryCandidateRecord>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .list_memory_candidate_records()
        .map_err(event_store_error)
}

#[tauri::command]
pub fn search_memory_records(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<MemoryRecord>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .search_memory_records(&query)
        .map_err(event_store_error)
}

#[tauri::command]
pub fn propose_memory_candidate(
    title: String,
    body: String,
    memory_type: MemoryType,
    scope: MemoryScope,
    sensitivity: MemorySensitivity,
    lifecycle: MemoryLifecycle,
    expires_at: Option<DateTime<Utc>>,
    state: State<'_, AppState>,
) -> Result<MemoryCandidateRecord, String> {
    let candidate = MemoryCandidate::new_with_metadata_and_expiration(
        title,
        body,
        MemoryCandidateSource::Manual,
        None,
        "User proposed this memory in Memory Studio.".to_string(),
        memory_type,
        scope,
        sensitivity,
        lifecycle,
        expires_at,
    )?;
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .append_memory_candidate(&candidate)
        .map_err(event_store_error)?;

    store
        .list_memory_candidate_records()
        .map_err(event_store_error)?
        .into_iter()
        .find(|record| record.candidate.id == candidate.id)
        .ok_or_else(|| "memory candidate was not found after append".to_string())
}

#[tauri::command]
pub fn resolve_memory_candidate(
    candidate_id: Uuid,
    accepted: bool,
    note: String,
    state: State<'_, AppState>,
) -> Result<MemoryCandidateResolution, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .resolve_memory_candidate(candidate_id, accepted, note)
        .map_err(event_store_error)
}

#[tauri::command]
pub fn preview_memory_candidate_merge(
    candidate_id: Uuid,
    source_memory_ids: Vec<Uuid>,
    state: State<'_, AppState>,
) -> Result<MemoryCandidateMergePreview, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .preview_memory_candidate_merge(candidate_id, source_memory_ids)
        .map_err(event_store_error)
}

#[tauri::command]
pub fn preview_memory_candidate_replace(
    candidate_id: Uuid,
    target_memory_ids: Vec<Uuid>,
    state: State<'_, AppState>,
) -> Result<MemoryCandidateReplacePreview, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .preview_memory_candidate_replace(candidate_id, target_memory_ids)
        .map_err(event_store_error)
}

#[tauri::command]
pub fn merge_memory_candidate_with_conflicts(
    candidate_id: Uuid,
    source_memory_ids: Vec<Uuid>,
    note: String,
    state: State<'_, AppState>,
) -> Result<MemoryCandidateResolution, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .merge_memory_candidate_with_conflicts(candidate_id, source_memory_ids, note)
        .map_err(event_store_error)
}

#[tauri::command]
pub fn replace_memory_candidate_conflicts(
    candidate_id: Uuid,
    target_memory_ids: Vec<Uuid>,
    note: String,
    state: State<'_, AppState>,
) -> Result<MemoryCandidateResolution, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .replace_memory_candidate_conflicts(candidate_id, target_memory_ids, note)
        .map_err(event_store_error)
}

#[tauri::command]
pub fn link_memory_candidate_to_conflicts(
    candidate_id: Uuid,
    linked_memory_ids: Vec<Uuid>,
    relation: Option<MemoryRelationKind>,
    note: String,
    state: State<'_, AppState>,
) -> Result<MemoryCandidateResolution, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .link_memory_candidate_to_conflicts_with_relation(
            candidate_id,
            linked_memory_ids,
            relation.unwrap_or(MemoryRelationKind::Extends),
            note,
        )
        .map_err(event_store_error)
}

#[tauri::command]
pub fn link_memory_records(
    source_memory_id: Uuid,
    target_memory_id: Uuid,
    relation: MemoryRelationKind,
    note: String,
    state: State<'_, AppState>,
) -> Result<Vec<MemoryRecord>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    link_existing_memory_records(&store, source_memory_id, target_memory_id, relation, note)
        .map_err(event_store_error)
}

#[tauri::command]
pub fn update_memory_record(
    memory_id: Uuid,
    title: String,
    body: String,
    memory_type: MemoryType,
    scope: MemoryScope,
    sensitivity: MemorySensitivity,
    lifecycle: MemoryLifecycle,
    expires_at: Option<DateTime<Utc>>,
    note: String,
    state: State<'_, AppState>,
) -> Result<MemoryRecordUpdate, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .update_memory_record(
            memory_id,
            title,
            body,
            memory_type,
            scope,
            sensitivity,
            lifecycle,
            expires_at,
            note,
        )
        .map_err(event_store_error)
}

#[tauri::command]
pub fn delete_memory_record(
    memory_id: Uuid,
    note: String,
    state: State<'_, AppState>,
) -> Result<MemoryRecordDeletion, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .delete_memory_record(memory_id, note)
        .map_err(event_store_error)
}

#[tauri::command]
pub fn list_permission_audit_entries(
    state: State<'_, AppState>,
) -> Result<Vec<PermissionAuditEntry>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .list_permission_audit_entries()
        .map_err(event_store_error)
}

#[tauri::command]
pub fn list_capability_catalog() -> Vec<CapabilityDescriptor> {
    builtin_capability_catalog()
}

#[tauri::command]
pub fn list_capability_access_records(
    state: State<'_, AppState>,
) -> Result<Vec<CapabilityAccessRecord>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .list_capability_access_records()
        .map_err(event_store_error)
}

#[tauri::command]
pub fn list_pending_capability_access_records(
    state: State<'_, AppState>,
) -> Result<Vec<CapabilityAccessRecord>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .list_pending_capability_access_records()
        .map_err(event_store_error)
}

#[tauri::command]
pub fn request_capability_access(
    access_mode: AccessMode,
    capability: CapabilityKind,
    state: State<'_, AppState>,
) -> Result<CapabilityAccessRecord, String> {
    let request = build_capability_access_request(access_mode, capability)?;
    let entry = PermissionAuditEntry::evaluate(access_mode, capability);
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .append_capability_access_request(&request)
        .map_err(event_store_error)?;
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;

    Ok(CapabilityAccessRecord {
        effective_status: request.status,
        grant_state: CapabilityGrantState::NotGranted,
        request,
        resolution: None,
    })
}

#[tauri::command]
pub fn resolve_capability_access_request(
    request_id: Uuid,
    approved: bool,
    note: String,
    state: State<'_, AppState>,
) -> Result<PermissionResolution, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .resolve_capability_access_request(request_id, approved, note)
        .map_err(event_store_error)
}

#[tauri::command]
pub fn list_capability_invocations(
    state: State<'_, AppState>,
) -> Result<Vec<CapabilityInvocation>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .list_capability_invocations()
        .map_err(event_store_error)
}

#[tauri::command]
pub fn list_operations_briefing_runs(
    state: State<'_, AppState>,
) -> Result<Vec<OperationsBriefingRun>, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .list_operations_briefing_runs()
        .map_err(event_store_error)
}

#[tauri::command]
pub fn browse_url(
    access_mode: AccessMode,
    url: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::BrowserBrowse)
            .map_err(event_store_error)?
    };
    let client = HttpBrowserPageClient::new()?;
    let outcome = run_browser_browse(
        BrowserBrowseRequest {
            access_mode,
            url,
            approval_granted,
        },
        &client,
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::BrowserBrowse);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn submit_browser_boundary(
    access_mode: AccessMode,
    url: String,
    summary: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::BrowserSubmit)
            .map_err(event_store_error)?
    };
    let outcome = run_browser_submit_boundary(BrowserSubmitRequest {
        access_mode,
        url,
        summary,
        approval_granted,
    })?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::BrowserSubmit);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn search_network_boundary(
    access_mode: AccessMode,
    large_model_provider: LargeModelProvider,
    query: String,
    scope: String,
    network_search_source_model: Option<NetworkSearchSourceModel>,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::NetworkSearch)
            .map_err(event_store_error)?
    };
    let strategy = model_driven_tool_strategy_for_current_platform(
        large_model_provider,
        network_search_source_model,
    );
    let request = NetworkSearchRequest {
        access_mode,
        query,
        scope,
        approval_granted,
    };
    let outcome = match strategy.network_search_backend {
        NetworkSearchBackend::NativeLargeModel => {
            let client = CodexBridgeNetworkSearchClient::from_env(large_model_provider);
            run_network_search_boundary(request, &client)?
        }
        NetworkSearchBackend::SourceBackedModel | NetworkSearchBackend::DeepSeek => {
            let source_model = strategy.network_search_source_model.ok_or_else(|| {
                "network search source model is required for local source-backed execution"
                    .to_string()
            })?;
            let client = HttpNetworkSearchClient::new(source_model)?;
            run_network_search_boundary(request, &client)?
        }
    };
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::NetworkSearch);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn read_local_file(
    access_mode: AccessMode,
    path: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::FileRead)
            .map_err(event_store_error)?
    };
    let client = LocalFileContentClient::new(512 * 1024);
    let outcome = run_file_read(
        FileReadRequest {
            access_mode,
            path,
            approval_granted,
        },
        &client,
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileRead);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn write_file_boundary(
    app: AppHandle,
    access_mode: AccessMode,
    path: String,
    summary: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::FileWrite)
            .map_err(event_store_error)?
    };
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    let directory_state = load_local_directory_state(&app_data_dir).map_err(event_store_error)?;
    let workspace_dir = file_write_workspace_base_dir(&app_data_dir, &directory_state);
    std::fs::create_dir_all(&workspace_dir).map_err(event_store_error)?;
    let client = LocalWorkspaceFileWriteClient::new(workspace_dir, 512 * 1024);
    let outcome = run_file_write_boundary(
        FileWriteRequest {
            access_mode,
            path,
            summary,
            content,
            approval_granted,
        },
        &client,
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileWrite);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn ingest_evidence_folder(
    access_mode: AccessMode,
    folder_path: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::FileRead)
            .map_err(event_store_error)?
    };
    let client = LocalEvidenceFolderClient::new(20, 512 * 1024);
    let outcome = run_evidence_folder_ingest(
        EvidenceFolderRequest {
            access_mode,
            folder_path,
            approval_granted,
        },
        &client,
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileRead);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn run_terminal_read(
    access_mode: AccessMode,
    command: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::TerminalRead)
            .map_err(event_store_error)?
    };
    let working_dir = std::env::current_dir().map_err(event_store_error)?;
    let client = LocalTerminalReadClient::new(working_dir, 4_000);
    let outcome = run_terminal_read_capability(
        TerminalReadRequest {
            access_mode,
            command,
            approval_granted,
        },
        &client,
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::TerminalRead);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn run_terminal_write(
    access_mode: AccessMode,
    command: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::TerminalWrite)
            .map_err(event_store_error)?
    };
    let outcome = run_terminal_write_boundary(TerminalWriteRequest {
        access_mode,
        command,
        approval_granted,
    })?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::TerminalWrite);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn capture_computer_screenshot(
    app: AppHandle,
    access_mode: AccessMode,
    large_model_provider: LargeModelProvider,
    network_search_source_model: Option<NetworkSearchSourceModel>,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::ComputerScreenshot)
            .map_err(event_store_error)?
    };
    let strategy =
        computer_tool_strategy_for_command(large_model_provider, network_search_source_model);
    let request = ComputerScreenshotRequest {
        access_mode,
        approval_granted,
    };
    let outcome = match strategy.computer_screenshot_backend {
        ComputerScreenshotBackend::LocalWindowsScreenCapture
        | ComputerScreenshotBackend::LocalMacosScreenCapture => {
            let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
            let directory_state =
                load_local_directory_state(&app_data_dir).map_err(event_store_error)?;
            let evidence_base_dir =
                computer_screenshot_evidence_base_dir(&app_data_dir, &directory_state);
            let client = LocalComputerScreenshotClient::new(evidence_base_dir);
            run_computer_screenshot(request, &client)?
        }
        ComputerScreenshotBackend::CodexBridgeScreenCapture
        | ComputerScreenshotBackend::CodexStyleScreenCapture => {
            let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
            let directory_state =
                load_local_directory_state(&app_data_dir).map_err(event_store_error)?;
            let evidence_base_dir =
                computer_screenshot_evidence_base_dir(&app_data_dir, &directory_state);
            let client = CodexBridgeComputerScreenshotClient::from_env(evidence_base_dir);
            run_computer_screenshot(request, &client)?
        }
    };
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::ComputerScreenshot);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn control_computer_boundary(
    access_mode: AccessMode,
    large_model_provider: LargeModelProvider,
    network_search_source_model: Option<NetworkSearchSourceModel>,
    target: String,
    action: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_request_id = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .available_capability_grant_request_id(CapabilityKind::ComputerControl)
            .map_err(event_store_error)?
    };
    let approval_granted = approval_request_id.is_some();
    if should_require_computer_control_unlock(approval_granted) {
        let unlock_state = state
            .computer_control_unlock
            .lock()
            .map_err(|_| computer_control_unlock_lock_error())?;
        if !unlock_state.is_unlocked(Utc::now()) {
            return Err("computer control requires local unlock before execution".to_string());
        }
    }
    let strategy =
        computer_tool_strategy_for_command(large_model_provider, network_search_source_model);
    let request = ComputerControlRequest {
        access_mode,
        target,
        action,
        approval_granted,
    };
    let mut outcome = match strategy.computer_control_backend {
        ComputerControlBackend::LocalWindowsInputControl
        | ComputerControlBackend::LocalMacosInputControl => {
            let client = LocalComputerControlClient::new();
            run_computer_control_boundary(request, &client)?
        }
        ComputerControlBackend::CodexBridgeInputControl
        | ComputerControlBackend::CodexStyleInputControl => {
            let client = CodexBridgeComputerControlClient::from_env();
            run_computer_control_boundary(request, &client)?
        }
    };
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::ComputerControl);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    outcome.invocation.approval_request_id = approval_request_id
        .or_else(|| should_record_access_request.then_some(outcome.access_request.id));
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn send_email_boundary(
    access_mode: AccessMode,
    to: String,
    subject: String,
    body: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_request_id = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .available_capability_grant_request_id(CapabilityKind::EmailSend)
            .map_err(event_store_error)?
    };
    let approval_granted = approval_request_id.is_some();
    let mut outcome = run_email_send_boundary(EmailSendRequest {
        access_mode,
        to,
        subject,
        body,
        approval_granted,
    })?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::EmailSend);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    outcome.invocation.approval_request_id = approval_request_id
        .or_else(|| should_record_access_request.then_some(outcome.access_request.id));
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn create_email_draft_boundary(
    access_mode: AccessMode,
    to: String,
    subject: String,
    body: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::EmailDraft)
            .map_err(event_store_error)?
    };
    let outcome = run_email_draft_boundary(EmailDraftRequest {
        access_mode,
        to,
        subject,
        body,
        approval_granted,
    })?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::EmailDraft);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn read_email_boundary(
    access_mode: AccessMode,
    mailbox: String,
    query: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::EmailRead)
            .map_err(event_store_error)?
    };
    let outcome = run_email_read_boundary(EmailReadRequest {
        access_mode,
        mailbox,
        query,
        approval_granted,
    })?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::EmailRead);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn read_drive_boundary(
    access_mode: AccessMode,
    location: String,
    query: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::DriveRead)
            .map_err(event_store_error)?
    };
    let outcome = run_drive_read_boundary(
        DriveReadRequest {
            access_mode,
            location,
            query,
            approval_granted,
        },
        &LocalDriveFolderClient::new(20, 512 * 1024),
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::DriveRead);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn write_drive_boundary(
    app: AppHandle,
    access_mode: AccessMode,
    location: String,
    summary: String,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::DriveWrite)
            .map_err(event_store_error)?
    };
    let package_json = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        let task_records = store.list_task_records().map_err(event_store_error)?;
        let memory_candidates = pending_memory_candidates_for_work_package(
            store
                .list_memory_candidate_records()
                .map_err(event_store_error)?,
        );
        let operations_briefing_runs = store
            .list_operations_briefing_runs()
            .map_err(event_store_error)?;
        let package = build_work_package_with_tool_readiness(
            FoundationState::default(),
            task_records,
            memory_candidates,
            operations_briefing_runs,
            current_work_package_tool_readiness(current_local_directory_readiness(&app)?),
        );
        serde_json::to_string_pretty(&package).map_err(event_store_error)?
    };
    let outcome = run_drive_write_boundary(
        DriveWriteRequest {
            access_mode,
            location,
            summary,
            package_json: Some(package_json),
            export_file: None,
            approval_granted,
        },
        &LocalDriveFolderClient::new(20, 512 * 1024),
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::DriveWrite);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn run_operations_briefing(
    app: AppHandle,
    access_mode: AccessMode,
    evidence_folder_path: String,
    large_model_provider: LargeModelProvider,
    model_route: ModelRoute,
    thinking_level: ThinkingLevel,
    state: State<'_, AppState>,
) -> Result<OperationsBriefingRun, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::FileRead)
            .map_err(event_store_error)?
    };
    let client = LocalEvidenceFolderClient::new(20, 512 * 1024);
    let request = OperationsBriefingRequest {
        access_mode,
        evidence_folder_path,
        approval_granted,
    };
    let mut deepseek_telemetry = Vec::new();
    let mut outcome = if let Some(api_key) = operations_briefing_deepseek_api_key_for_provider(
        |name| std::env::var(name).ok(),
        large_model_provider,
    ) {
        let transport = HttpDeepSeekChatCompletionTransport::new()?;
        let synthesizer = DeepSeekOperationsBriefingSynthesizer::new_with_cache(
            &transport,
            &state.deepseek_chat_cache,
            api_key,
            model_route,
            thinking_level,
        );
        let outcome =
            build_operations_briefing_run_with_synthesizer(request, &client, &synthesizer)?;
        deepseek_telemetry = synthesizer.take_telemetry();
        outcome
    } else {
        build_operations_briefing_run(request, &client)?
    };
    let pricing_settings = app
        .path()
        .app_data_dir()
        .ok()
        .and_then(|app_data_dir| load_deepseek_pricing_state(app_data_dir).ok())
        .map(|pricing_state| pricing_state.settings);
    let deepseek_telemetry =
        deepseek_telemetry_with_pricing(deepseek_telemetry, pricing_settings.as_ref());
    outcome.run.context_receipt.model_route =
        operations_briefing_model_route_context(large_model_provider, model_route);
    outcome.run.context_receipt.thinking_level =
        thinking_level_context_label(thinking_level).to_string();
    outcome.run.context_receipt.token_cache_state =
        operations_briefing_token_cache_context(&deepseek_telemetry);
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileRead);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.evidence_invocation)
        .map_err(event_store_error)?;
    store
        .append_operations_briefing_run(&outcome.run)
        .map_err(event_store_error)?;
    for telemetry in deepseek_telemetry {
        store
            .append_deepseek_chat_telemetry(&telemetry)
            .map_err(event_store_error)?;
    }

    Ok(outcome.run)
}

#[tauri::command]
pub fn export_operations_briefing_report(
    app: AppHandle,
    access_mode: AccessMode,
    run_id: Uuid,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::DriveWrite)
            .map_err(event_store_error)?
    };
    let run = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .list_operations_briefing_runs()
            .map_err(event_store_error)?
            .into_iter()
            .find(|run| run.id == run_id)
            .ok_or_else(|| "operations briefing run was not found".to_string())?
    };
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    let directory_state = load_local_directory_state(&app_data_dir).map_err(event_store_error)?;
    let export_dir = operations_briefing_report_export_dir(&app_data_dir, &directory_state);
    let report_markdown = render_operations_briefing_report(&run);
    let outcome = run_drive_write_boundary(
        DriveWriteRequest {
            access_mode,
            location: export_dir.to_string_lossy().to_string(),
            summary: format!("Export Operations Briefing report {}", run.id),
            package_json: None,
            export_file: Some(DriveWriteExportFile {
                file_name: operations_briefing_report_file_name(&run),
                content: report_markdown,
                content_base64: None,
            }),
            approval_granted,
        },
        &LocalDriveFolderClient::new(20, 512 * 1024),
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::DriveWrite);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn export_operations_briefing_html_report(
    app: AppHandle,
    access_mode: AccessMode,
    run_id: Uuid,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::DriveWrite)
            .map_err(event_store_error)?
    };
    let run = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .list_operations_briefing_runs()
            .map_err(event_store_error)?
            .into_iter()
            .find(|run| run.id == run_id)
            .ok_or_else(|| "operations briefing run was not found".to_string())?
    };
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    let directory_state = load_local_directory_state(&app_data_dir).map_err(event_store_error)?;
    let export_dir = operations_briefing_report_export_dir(&app_data_dir, &directory_state);
    let report_html = render_operations_briefing_html_report(&run);
    let outcome = run_drive_write_boundary(
        DriveWriteRequest {
            access_mode,
            location: export_dir.to_string_lossy().to_string(),
            summary: format!("Export Operations Briefing HTML report {}", run.id),
            package_json: None,
            export_file: Some(DriveWriteExportFile {
                file_name: operations_briefing_html_report_file_name(&run),
                content: report_html,
                content_base64: None,
            }),
            approval_granted,
        },
        &LocalDriveFolderClient::new(20, 512 * 1024),
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::DriveWrite);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn export_operations_briefing_pdf_report(
    app: AppHandle,
    access_mode: AccessMode,
    run_id: Uuid,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::DriveWrite)
            .map_err(event_store_error)?
    };
    let run = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .list_operations_briefing_runs()
            .map_err(event_store_error)?
            .into_iter()
            .find(|run| run.id == run_id)
            .ok_or_else(|| "operations briefing run was not found".to_string())?
    };
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    let directory_state = load_local_directory_state(&app_data_dir).map_err(event_store_error)?;
    let export_dir = operations_briefing_report_export_dir(&app_data_dir, &directory_state);
    let report_pdf = render_operations_briefing_pdf_report(&run);
    let outcome = run_drive_write_boundary(
        DriveWriteRequest {
            access_mode,
            location: export_dir.to_string_lossy().to_string(),
            summary: format!("Export Operations Briefing PDF report {}", run.id),
            package_json: None,
            export_file: Some(DriveWriteExportFile {
                file_name: operations_briefing_pdf_report_file_name(&run),
                content: String::new(),
                content_base64: Some(general_purpose::STANDARD.encode(report_pdf)),
            }),
            approval_granted,
        },
        &LocalDriveFolderClient::new(20, 512 * 1024),
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::DriveWrite);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn seed_operations_briefing_evidence_templates(
    app: AppHandle,
    access_mode: AccessMode,
    state: State<'_, AppState>,
) -> Result<CapabilityInvocation, String> {
    let approval_granted = {
        let store = state.event_store.lock().map_err(|_| lock_error())?;
        store
            .has_user_approved_capability(CapabilityKind::FileWrite)
            .map_err(event_store_error)?
    };
    let app_data_dir = app.path().app_data_dir().map_err(event_store_error)?;
    let directory_state = load_local_directory_state(&app_data_dir).map_err(event_store_error)?;
    let seed_dir = operations_briefing_template_seed_dir(&app_data_dir, &directory_state);
    let outcome = build_operations_briefing_template_seed(
        OperationsBriefingTemplateSeedRequest {
            access_mode,
            evidence_folder_path: seed_dir.to_string_lossy().to_string(),
            approval_granted,
        },
        &LocalOperationsBriefingTemplateSeeder,
    )?;
    let should_record_access_request = !approval_granted
        || outcome.access_request.decision == crate::kernel::policy::PolicyDecision::Allow;
    let entry = PermissionAuditEntry::evaluate(access_mode, CapabilityKind::FileWrite);
    let store = state.event_store.lock().map_err(|_| lock_error())?;

    if should_record_access_request {
        store
            .append_capability_access_request(&outcome.access_request)
            .map_err(event_store_error)?;
    }
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    store
        .append_capability_invocation(&outcome.invocation)
        .map_err(event_store_error)?;

    Ok(outcome.invocation)
}

#[tauri::command]
pub fn record_permission_audit(
    access_mode: AccessMode,
    capability: CapabilityKind,
    state: State<'_, AppState>,
) -> Result<PermissionAuditEntry, String> {
    let entry = PermissionAuditEntry::evaluate(access_mode, capability);
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .append_permission_audit_entry(&entry)
        .map_err(event_store_error)?;
    Ok(entry)
}

#[tauri::command]
pub fn create_task_record(
    title: String,
    summary: String,
    state: State<'_, AppState>,
) -> Result<TaskRecord, String> {
    let record = TaskRecord::new(title, summary)?;
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .append_task_record(&record)
        .map_err(event_store_error)?;
    let memory = MemoryRecord::from_task_record(&record);
    store
        .append_memory_record(&memory)
        .map_err(event_store_error)?;
    Ok(record)
}

#[tauri::command]
pub fn export_work_package(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<WorkPackage, String> {
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    let task_records = store.list_task_records().map_err(event_store_error)?;
    let memory_candidates = pending_memory_candidates_for_work_package(
        store
            .list_memory_candidate_records()
            .map_err(event_store_error)?,
    );
    let operations_briefing_runs = store
        .list_operations_briefing_runs()
        .map_err(event_store_error)?;
    Ok(build_work_package_with_tool_readiness(
        FoundationState::default(),
        task_records,
        memory_candidates,
        operations_briefing_runs,
        current_work_package_tool_readiness(current_local_directory_readiness(&app)?),
    ))
}

#[tauri::command]
pub fn import_work_package(
    package_json: String,
    state: State<'_, AppState>,
) -> Result<WorkPackageImportSummary, String> {
    let package = parse_work_package_json(&package_json).map_err(event_store_error)?;
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    let mut summary = store
        .import_task_records(&package.task_records)
        .map_err(event_store_error)?;
    summary.memory_candidates = store
        .import_memory_candidates(&package.memory_candidates)
        .map_err(event_store_error)?;
    summary.operations_briefing_runs = store
        .import_operations_briefing_runs(&package.operations_briefing_runs)
        .map_err(event_store_error)?;
    summary.workflow_templates = store
        .import_workflow_template_packages(&package.workflow_templates)
        .map_err(event_store_error)?;
    Ok(summary)
}

#[tauri::command]
pub fn preview_work_package_import(
    package_json: String,
    state: State<'_, AppState>,
) -> Result<WorkPackageImportPreview, String> {
    let package = parse_work_package_json(&package_json).map_err(event_store_error)?;
    let store = state.event_store.lock().map_err(|_| lock_error())?;
    store
        .preview_work_package_import(&package)
        .map_err(event_store_error)
}

#[cfg(test)]
mod tests {
    use super::{
        agent_terminal_read_command_from_target, is_newer_version, is_windows_installer_asset,
        release_installable_asset, silent_update_install_command, update_status_from_release,
        update_status_from_releases, GithubRelease, GithubReleaseAsset,
    };
    use crate::commands::{
        agent_chat_api_key_candidates_from_sources, agent_chat_api_key_from_sources,
        agent_chat_with_dispatch_and_tool_followup, agent_chat_with_transport,
        computer_screenshot_evidence_base_dir, computer_tool_strategy_for_command,
        deepseek_telemetry_with_pricing, dispatch_agent_action_proposals,
        dispatch_agent_browser_open_action_with_opener, dispatch_agent_computer_control_action,
        dispatch_agent_office_create_action, dispatch_agent_office_open_action,
        dispatch_agent_office_update_action, link_existing_memory_records,
        operations_briefing_deepseek_api_key_for_provider, operations_briefing_model_route_context,
        operations_briefing_report_export_dir, operations_briefing_template_seed_dir,
        operations_briefing_token_cache_context, record_agent_action_permission_requests,
        resume_agent_chat_action_with_clients, run_agent_chat_with_clients,
        should_require_computer_control_unlock, thinking_level_context_label,
        AgentChatActionProposal, AgentChatReadiness, AgentChatRequest, AgentChatRuntimeContext,
        BrowserUrlOpenOutcome, BrowserUrlOpener, ComputerControlUnlockState,
        COMPUTER_CONTROL_UNLOCK_TTL_MINUTES,
    };
    use crate::kernel::capability::{
        BrowserPage, BrowserPageClient, CapabilityInvocationStatus, ComputerControlAction,
        ComputerControlClient, ComputerControlExecution, FileContentClient, FileWriteClient,
        FileWriteResult, LocalFileContentClient, NetworkSearchClient, NetworkSearchResult,
        NetworkSearchResultItem,
    };
    use crate::kernel::deepseek::{
        DeepSeekChatCacheStatus, DeepSeekChatCompletionRequest, DeepSeekChatCompletionResponse,
        DeepSeekChatCompletionTransport, DeepSeekChatTelemetry, DeepSeekMemoryChatCompletionCache,
        HttpDeepSeekChatCompletionTransport, DEEPSEEK_API_KEY_ENV, DEEPSEEK_FLASH_MODEL,
    };
    use crate::kernel::deepseek_pricing::DeepSeekPricingSettings;
    use crate::kernel::event_store::EventStore;
    use crate::kernel::local_directory::{
        LocalDirectorySettings, LocalDirectoryState, LOCAL_DIRECTORY_SETTINGS_FILE,
    };
    use crate::kernel::models::{
        AccessMode, ComputerControlBackend, ComputerScreenshotBackend, LargeModelProvider,
        MemoryCandidate, MemoryCandidateRecord, MemoryCandidateResolution, MemoryCandidateSource,
        MemoryCandidateStatus, MemoryLifecycle, MemoryRecord, MemoryRecordSource,
        MemoryRelationKind, MemoryScope, MemorySearchMatch, MemorySensitivity, MemoryType,
        ModelRoute, NetworkSearchSourceModel, TaskRecord, ThinkingLevel,
    };
    use crate::kernel::office::{
        build_office_artifact, OfficeApp, OfficeArtifactClient, OfficeCreateResult,
        OfficeCreateSpec, OfficeOpenClient, OfficeOpenResult, OfficeUpdateClient,
        OfficeUpdateResult, OfficeUpdateSpec,
    };
    use crate::kernel::policy::{CapabilityKind, PolicyDecision};
    use chrono::{Duration, TimeZone, Utc};
    use std::sync::Mutex;
    use uuid::Uuid;

    #[test]
    fn app_update_version_compare_accepts_newer_release_tags() {
        assert!(is_newer_version("v0.1.1", "0.1.0"));
        assert!(is_newer_version("0.2.0", "0.1.9"));
        assert!(is_newer_version("v0.1.0-rc.3", "v0.1.0-rc.1"));
        assert!(is_newer_version("v0.1.0", "v0.1.0-rc.3"));
        assert!(!is_newer_version("v0.1.0-rc.3", "v0.1.0"));
        assert!(!is_newer_version("v0.1.0", "0.1.0"));
        assert!(!is_newer_version("v0.0.9", "0.1.0"));
    }

    #[test]
    fn agent_terminal_read_desktop_target_uses_runtime_desktop_directory() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let desktop_dir = temp_dir.path().join("Desktop");
        let command = agent_terminal_read_command_from_target("desktop", Some(&desktop_dir))
            .expect("desktop target should resolve to a safe directory listing command");

        assert_eq!(
            command,
            format!("ds-agent:list-directory {}", desktop_dir.display())
        );
    }

    #[test]
    fn app_update_status_selects_newer_prerelease_installer_from_release_list() {
        let releases = vec![
            GithubRelease {
                tag_name: "v0.1.0-rc.3".to_string(),
                html_url: "https://github.com/Lee-take/deepseek-agent-os/releases/tag/v0.1.0-rc.3"
                    .to_string(),
                assets: vec![GithubReleaseAsset {
                    name: "DS Agent_0.1.0_x64-setup.exe".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/deepseek-agent-os/releases/download/v0.1.0-rc.3/DS.Agent_0.1.0_x64-setup.exe"
                            .to_string(),
                }],
            },
            GithubRelease {
                tag_name: "v0.1.0-rc.1".to_string(),
                html_url: "https://github.com/Lee-take/deepseek-agent-os/releases/tag/v0.1.0-rc.1"
                    .to_string(),
                assets: vec![GithubReleaseAsset {
                    name: "DS Agent_0.1.0_x64-setup.exe".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/deepseek-agent-os/releases/download/v0.1.0-rc.1/DS.Agent_0.1.0_x64-setup.exe"
                            .to_string(),
                }],
            },
        ];

        let status = update_status_from_releases(releases, "v0.1.0-rc.1");

        assert!(status.update_available);
        assert_eq!(status.current_version, "v0.1.0-rc.1");
        assert_eq!(status.latest_version.as_deref(), Some("0.1.0-rc.3"));
        assert_eq!(
            status.asset_name.as_deref(),
            Some("DS Agent_0.1.0_x64-setup.exe")
        );
    }

    #[test]
    fn app_update_status_keeps_current_prerelease_quiet_from_release_list() {
        let releases = vec![GithubRelease {
            tag_name: "v0.1.0-rc.3".to_string(),
            html_url: "https://github.com/Lee-take/deepseek-agent-os/releases/tag/v0.1.0-rc.3"
                .to_string(),
            assets: vec![GithubReleaseAsset {
                name: "DS Agent_0.1.0_x64-setup.exe".to_string(),
                browser_download_url:
                    "https://github.com/Lee-take/deepseek-agent-os/releases/download/v0.1.0-rc.3/DS.Agent_0.1.0_x64-setup.exe"
                        .to_string(),
            }],
        }];

        let status = update_status_from_releases(releases, "v0.1.0-rc.3");

        assert!(!status.update_available);
        assert_eq!(status.current_version, "v0.1.0-rc.3");
        assert_eq!(status.latest_version.as_deref(), Some("0.1.0-rc.3"));
        assert!(status.asset_name.is_none());
    }

    #[test]
    fn app_update_asset_filter_accepts_windows_installers_only() {
        assert!(is_windows_installer_asset("DS Agent_0.1.1_x64-setup.exe"));
        assert!(is_windows_installer_asset("DS-Agent-0.1.1.msi"));
        assert!(!is_windows_installer_asset("Source code.zip"));
        assert!(!is_windows_installer_asset("DS-Agent-0.1.1-debug.exe"));
        assert!(!is_windows_installer_asset("DS-Agent-0.1.1-symbols.exe"));
    }

    #[test]
    fn app_update_status_hides_source_only_newer_release() {
        let release = GithubRelease {
            tag_name: "v9.9.9".to_string(),
            html_url: "https://github.com/Lee-take/deepseek-agent-os/releases/tag/v9.9.9"
                .to_string(),
            assets: vec![GithubReleaseAsset {
                name: "source.zip".to_string(),
                browser_download_url:
                    "https://github.com/Lee-take/deepseek-agent-os/releases/download/v9.9.9/source.zip"
                        .to_string(),
            }],
        };

        let status = update_status_from_release(release);
        assert!(!status.update_available);
        assert_eq!(
            status.message.as_deref(),
            Some("latest release has no Windows installer asset")
        );
    }

    #[test]
    fn app_update_selects_trusted_windows_installer_asset() {
        let release = GithubRelease {
            tag_name: "v9.9.9".to_string(),
            html_url: "https://github.com/Lee-take/deepseek-agent-os/releases/tag/v9.9.9"
                .to_string(),
            assets: vec![
                GithubReleaseAsset {
                    name: "source.zip".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/deepseek-agent-os/releases/download/v9.9.9/source.zip"
                            .to_string(),
                },
                GithubReleaseAsset {
                    name: "DS Agent_9.9.9_x64-setup.exe".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/deepseek-agent-os/releases/download/v9.9.9/DS.Agent.exe"
                            .to_string(),
                },
            ],
        };

        let asset = release_installable_asset(&release).expect("installer asset");
        assert_eq!(asset.name, "DS Agent_9.9.9_x64-setup.exe");
    }

    #[test]
    fn app_update_silent_installer_command_uses_nsis_s_arg() {
        let command = silent_update_install_command(std::path::Path::new(
            r"C:\Users\tester\AppData\Local\Temp\ds-agent-updates\DS.Agent_0.1.0_rc7_x64-setup.exe",
        ));

        assert_eq!(
            command.program,
            std::path::PathBuf::from(
                r"C:\Users\tester\AppData\Local\Temp\ds-agent-updates\DS.Agent_0.1.0_rc7_x64-setup.exe",
            )
        );
        assert_eq!(command.args, vec!["/S"]);
    }

    struct RecordingDeepSeekTransport {
        response_text: String,
        requests: Mutex<Vec<DeepSeekChatCompletionRequest>>,
    }

    struct RecordingNetworkSearchClient {
        calls: Mutex<Vec<(String, String)>>,
    }

    struct RecordingBrowserPageClient {
        calls: Mutex<Vec<String>>,
    }

    struct RecordingBrowserUrlOpener {
        outcome: BrowserUrlOpenOutcome,
        calls: Mutex<Vec<(String, Option<String>)>>,
    }

    struct RecordingComputerControlClient {
        calls: Mutex<Vec<(String, ComputerControlAction)>>,
    }

    struct RecordingFileWriteClient {
        calls: Mutex<Vec<(String, String)>>,
    }

    struct SequencedDeepSeekTransport {
        responses: Mutex<Vec<Result<String, String>>>,
        requests: Mutex<Vec<DeepSeekChatCompletionRequest>>,
    }

    struct StoreLockCheckingDeepSeekTransport<'a> {
        store: &'a Mutex<EventStore>,
        response_text: String,
        lock_available_at_call: Mutex<Vec<bool>>,
    }

    struct StoreLockCheckingFileContentClient<'a> {
        store: &'a Mutex<EventStore>,
        lock_available_at_read: Mutex<Vec<bool>>,
    }

    impl RecordingDeepSeekTransport {
        fn new(response_text: impl Into<String>) -> Self {
            Self {
                response_text: response_text.into(),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn recorded_requests(&self) -> Vec<DeepSeekChatCompletionRequest> {
            self.requests
                .lock()
                .map(|requests| requests.clone())
                .unwrap_or_default()
        }
    }

    impl DeepSeekChatCompletionTransport for RecordingDeepSeekTransport {
        fn post_chat_completion(
            &self,
            _endpoint: &str,
            _api_key: &str,
            request: &DeepSeekChatCompletionRequest,
        ) -> Result<DeepSeekChatCompletionResponse, String> {
            self.requests
                .lock()
                .expect("record requests")
                .push(request.clone());
            Ok(DeepSeekChatCompletionResponse::from_text(
                request.model.clone(),
                self.response_text.clone(),
            ))
        }
    }

    impl RecordingNetworkSearchClient {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn recorded_calls(&self) -> Vec<(String, String)> {
            self.calls
                .lock()
                .map(|calls| calls.clone())
                .unwrap_or_default()
        }
    }

    impl NetworkSearchClient for RecordingNetworkSearchClient {
        fn search(&self, query: &str, scope: &str) -> Result<NetworkSearchResult, String> {
            self.calls
                .lock()
                .expect("record search calls")
                .push((query.to_string(), scope.to_string()));
            Ok(NetworkSearchResult {
                provider: "fake source search".to_string(),
                query: query.to_string(),
                scope: scope.to_string(),
                search_url: format!("https://search.example/?q={}", query.replace(' ', "+")),
                items: vec![NetworkSearchResultItem {
                    title: "DS Agent result".to_string(),
                    url: "https://example.com/ds-agent".to_string(),
                    snippet: "durable URL evidence".to_string(),
                }],
            })
        }
    }

    impl RecordingBrowserPageClient {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn recorded_calls(&self) -> Vec<String> {
            self.calls
                .lock()
                .map(|calls| calls.clone())
                .unwrap_or_default()
        }
    }

    impl BrowserPageClient for RecordingBrowserPageClient {
        fn fetch_page(&self, url: &str) -> Result<BrowserPage, String> {
            self.calls
                .lock()
                .expect("record browse calls")
                .push(url.to_string());
            Ok(BrowserPage {
                final_url: url.to_string(),
                title: "Browser Evidence".to_string(),
                text: "Browser page evidence collected for DeepSeek synthesis.".to_string(),
            })
        }
    }

    impl RecordingComputerControlClient {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn recorded_calls(&self) -> Vec<(String, ComputerControlAction)> {
            self.calls
                .lock()
                .map(|calls| calls.clone())
                .unwrap_or_default()
        }
    }

    impl ComputerControlClient for RecordingComputerControlClient {
        fn execute_control(
            &self,
            target: &str,
            action: &ComputerControlAction,
        ) -> Result<ComputerControlExecution, String> {
            self.calls
                .lock()
                .expect("record computer control calls")
                .push((target.to_string(), action.clone()));
            Ok(ComputerControlExecution {
                summary: "fake input action executed".to_string(),
            })
        }
    }

    impl RecordingBrowserUrlOpener {
        fn new(outcome: BrowserUrlOpenOutcome) -> Self {
            Self {
                outcome,
                calls: Mutex::new(Vec::new()),
            }
        }

        fn recorded_calls(&self) -> Vec<(String, Option<String>)> {
            self.calls
                .lock()
                .map(|calls| calls.clone())
                .unwrap_or_default()
        }
    }

    impl BrowserUrlOpener for RecordingBrowserUrlOpener {
        fn open_url(
            &self,
            url: &str,
            preferred_browser: Option<&str>,
        ) -> Result<BrowserUrlOpenOutcome, String> {
            self.calls
                .lock()
                .expect("record browser open calls")
                .push((url.to_string(), preferred_browser.map(str::to_string)));
            Ok(self.outcome.clone())
        }
    }

    impl RecordingFileWriteClient {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn recorded_calls(&self) -> Vec<(String, String)> {
            self.calls
                .lock()
                .map(|calls| calls.clone())
                .unwrap_or_default()
        }
    }

    impl FileWriteClient for RecordingFileWriteClient {
        fn write_file(&self, path: &str, content: &str) -> Result<FileWriteResult, String> {
            self.calls
                .lock()
                .expect("record file write calls")
                .push((path.to_string(), content.to_string()));
            Ok(FileWriteResult {
                path: path.to_string(),
                bytes: content.len() as u64,
                encoding: "utf-8".to_string(),
            })
        }
    }

    impl OfficeArtifactClient for RecordingFileWriteClient {
        fn write_office_artifact(
            &self,
            spec: &OfficeCreateSpec,
        ) -> Result<OfficeCreateResult, String> {
            let bytes = build_office_artifact(spec)?;
            self.calls
                .lock()
                .expect("record office write calls")
                .push((spec.path.clone(), format!("office:{:?}", spec.app)));
            Ok(OfficeCreateResult {
                path: spec.path.clone(),
                bytes: bytes.len() as u64,
                app: spec.app,
                artifact_kind: "test_office_artifact".to_string(),
            })
        }
    }

    impl OfficeOpenClient for RecordingFileWriteClient {
        fn open_office_artifact(
            &self,
            path: &str,
            preferred_app: Option<OfficeApp>,
        ) -> Result<OfficeOpenResult, String> {
            self.calls
                .lock()
                .expect("record office open calls")
                .push((path.to_string(), format!("office-open:{preferred_app:?}")));
            Ok(OfficeOpenResult {
                path: path.to_string(),
                app: preferred_app.unwrap_or(OfficeApp::Excel),
                opener_label: "default app".to_string(),
                fallback_note: Some(format!(
                    "未检测到 {}，已使用默认应用打开 {}",
                    preferred_app.unwrap_or(OfficeApp::Excel).user_facing_name(),
                    path
                )),
            })
        }
    }

    impl OfficeUpdateClient for RecordingFileWriteClient {
        fn update_office_artifact(
            &self,
            spec: &OfficeUpdateSpec,
        ) -> Result<OfficeUpdateResult, String> {
            self.calls
                .lock()
                .expect("record office update calls")
                .push((spec.path.clone(), format!("office-update:{:?}", spec.app)));
            Ok(OfficeUpdateResult {
                path: spec.path.clone(),
                bytes: 2048,
                app: spec.app,
                artifact_kind: "test_office_artifact".to_string(),
                summary: "updated existing office artifact".to_string(),
            })
        }
    }

    impl SequencedDeepSeekTransport {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().map(Ok).collect()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn new_results(responses: Vec<Result<String, String>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn recorded_requests(&self) -> Vec<DeepSeekChatCompletionRequest> {
            self.requests
                .lock()
                .map(|requests| requests.clone())
                .unwrap_or_default()
        }
    }

    impl DeepSeekChatCompletionTransport for SequencedDeepSeekTransport {
        fn post_chat_completion(
            &self,
            _endpoint: &str,
            _api_key: &str,
            request: &DeepSeekChatCompletionRequest,
        ) -> Result<DeepSeekChatCompletionResponse, String> {
            self.requests
                .lock()
                .expect("record requests")
                .push(request.clone());
            let response_text = {
                let mut responses = self.responses.lock().expect("read response sequence");
                if responses.is_empty() {
                    return Err("no fake DeepSeek response left".to_string());
                }
                responses.remove(0)
            };
            match response_text {
                Ok(response_text) => Ok(DeepSeekChatCompletionResponse::from_text(
                    request.model.clone(),
                    response_text,
                )),
                Err(error) => Err(error),
            }
        }
    }

    impl<'a> StoreLockCheckingDeepSeekTransport<'a> {
        fn new(store: &'a Mutex<EventStore>, response_text: impl Into<String>) -> Self {
            Self {
                store,
                response_text: response_text.into(),
                lock_available_at_call: Mutex::new(Vec::new()),
            }
        }

        fn lock_checks(&self) -> Vec<bool> {
            self.lock_available_at_call
                .lock()
                .map(|checks| checks.clone())
                .unwrap_or_default()
        }
    }

    impl DeepSeekChatCompletionTransport for StoreLockCheckingDeepSeekTransport<'_> {
        fn post_chat_completion(
            &self,
            _endpoint: &str,
            _api_key: &str,
            request: &DeepSeekChatCompletionRequest,
        ) -> Result<DeepSeekChatCompletionResponse, String> {
            let lock_available = self.store.try_lock().is_ok();
            self.lock_available_at_call
                .lock()
                .expect("record lock check")
                .push(lock_available);
            if !lock_available {
                return Err("event store lock was held during DeepSeek call".to_string());
            }

            Ok(DeepSeekChatCompletionResponse::from_text(
                request.model.clone(),
                self.response_text.clone(),
            ))
        }
    }

    impl<'a> StoreLockCheckingFileContentClient<'a> {
        fn new(store: &'a Mutex<EventStore>) -> Self {
            Self {
                store,
                lock_available_at_read: Mutex::new(Vec::new()),
            }
        }

        fn lock_checks(&self) -> Vec<bool> {
            self.lock_available_at_read
                .lock()
                .map(|checks| checks.clone())
                .unwrap_or_default()
        }
    }

    impl FileContentClient for StoreLockCheckingFileContentClient<'_> {
        fn read_file(&self, path: &str) -> Result<crate::kernel::capability::FileContent, String> {
            let lock_available = self.store.try_lock().is_ok();
            self.lock_available_at_read
                .lock()
                .expect("record file read lock check")
                .push(lock_available);
            if !lock_available {
                return Err("event store lock was held during FileRead client call".to_string());
            }

            Ok(crate::kernel::capability::FileContent {
                path: path.to_string(),
                title: "readme.md".to_string(),
                text: "local file evidence".to_string(),
                bytes: 19,
                encoding: "utf-8".to_string(),
            })
        }
    }

    #[test]
    fn run_agent_chat_does_not_hold_store_lock_while_calling_deepseek() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let transport = StoreLockCheckingDeepSeekTransport::new(
            &store,
            r#"{
                "protocol_version": "ds-agent-envelope/v1",
                "reply_to_user": "我会先理解你的指令，再由 DS Agent 校验和执行本地动作。",
                "agent_actions": [],
                "missing_prerequisites": []
            }"#,
        );
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "帮我看一下这段材料。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat succeeds without holding the store lock during DeepSeek");

        assert_eq!(
            response.content,
            "我会先理解你的指令，再由 DS Agent 校验和执行本地动作。"
        );
        assert_eq!(transport.lock_checks(), vec![true]);
        assert_eq!(
            store
                .lock()
                .expect("store lock")
                .list_deepseek_chat_telemetry()
                .expect("telemetry list")
                .len(),
            1
        );
    }

    #[test]
    fn run_agent_chat_does_not_hold_store_lock_while_running_file_read_client() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let transport = SequencedDeepSeekTransport::new(vec![
            r#"{
                "protocol_version": "ds-agent-envelope/v1",
                "reply_to_user": "我会先读取本地材料。",
                "agent_actions": [
                    {
                        "action_type": "file_read",
                        "title": "读取材料",
                        "target": "readme.md"
                    }
                ],
                "missing_prerequisites": []
            }"#
            .to_string(),
            r#"{
                "protocol_version": "ds-agent-envelope/v1",
                "reply_to_user": "已根据读取到的本地材料完成回答。",
                "agent_actions": [],
                "missing_prerequisites": []
            }"#
            .to_string(),
        ]);
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = StoreLockCheckingFileContentClient::new(&store);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();
        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "读取 readme.md 后回答。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat succeeds without holding the store lock during FileRead");

        assert_eq!(file_client.lock_checks(), vec![true]);
        assert_eq!(response.content, "已根据读取到的本地材料完成回答。");
        assert_eq!(response.proposed_actions[0].execution_state, "succeeded");
        assert_eq!(
            store
                .lock()
                .expect("store lock")
                .list_capability_invocations()
                .expect("invocations")
                .len(),
            1
        );
    }

    #[test]
    fn agent_chat_records_context_receipt_for_completed_evidence_action() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let memory = MemoryRecord {
            id: Uuid::new_v4(),
            title: "项目记忆运行规则".to_string(),
            body: "用户要求 DS Agent 记忆系统要对标 Codex 和 Claude Code，避免用户说过就忘。"
                .to_string(),
            memory_type: MemoryType::WorkflowRule,
            scope: MemoryScope::Project,
            sensitivity: MemorySensitivity::Normal,
            lifecycle: MemoryLifecycle::Active,
            source: MemoryRecordSource::MemoryCandidate,
            source_id: None,
            pinned: false,
            expires_at: None,
            linked_memory_ids: Vec::new(),
            linked_memories: Vec::new(),
            search_match: MemorySearchMatch::direct(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        {
            let store = store.lock().expect("store locks");
            store.append_memory_record(&memory).expect("memory appends");
        }
        let transport = SequencedDeepSeekTransport::new(vec![
            r#"{
                "protocol_version": "ds-agent-envelope/v1",
                "reply_to_user": "I will inspect the file.",
                "agent_actions": [
                    {
                        "action_type": "file_read",
                        "title": "Read evidence",
                        "reason": "Inspect the requested evidence file",
                        "risk": "low",
                        "requires_confirmation": false,
                        "target": "reports/source.md"
                    }
                ],
                "missing_prerequisites": []
            }"#
            .to_string(),
            r#"{
                "protocol_version": "ds-agent-envelope/v1",
                "reply_to_user": "The evidence file was inspected.",
                "agent_actions": [],
                "missing_prerequisites": []
            }"#
            .to_string(),
        ]);
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = StoreLockCheckingFileContentClient::new(&store);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();
        let runtime_context = AgentChatRuntimeContext {
            soul_profile: Some(super::AgentSoulProfileContext {
                lines: vec!["user preferred address: 李总".to_string()],
                used_bytes: 35,
                max_bytes: super::AGENT_SOUL_PROFILE_CONTEXT_MAX_BYTES,
            }),
            ..AgentChatRuntimeContext::default()
        };

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-api-key",
            AgentChatRequest {
                prompt:
                    "Read reports/source.md，并遵循记忆系统对标规则。SECRET_CONTEXT_RECEIPT_TEST"
                        .to_string(),
                model_route: ModelRoute::Auto,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::FullAccess,
            },
            runtime_context,
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat succeeds");

        assert_eq!(response.proposed_actions[0].execution_state, "succeeded");
        let receipt_events = store
            .lock()
            .expect("store locks")
            .list_recent(50)
            .expect("events load")
            .into_iter()
            .filter(|event| event.event_type == "agent_context_receipt_recorded")
            .collect::<Vec<_>>();

        assert_eq!(receipt_events.len(), 1);
        let payload = &receipt_events[0].payload_json;
        assert!(payload.contains("\"loop_mode\":\"evidence_gathering\""));
        assert!(payload.contains("\"action_type\":\"file_read\""));
        assert!(payload.contains("\"model_route\":\"auto\""));
        assert!(payload.contains("\"thinking_level\":\"fast\""));
        assert!(payload.contains("\"allowed_tools\""));
        assert!(payload.contains("\"file_read\""));
        assert!(payload.contains("\"validators\""));
        assert!(payload.contains("\"capability_policy_checked\""));
        assert!(payload.contains("\"stop_conditions\""));
        assert!(payload.contains("\"evidence_observed\""));
        assert!(payload.contains("\"matched_stop_conditions\""));
        assert!(payload.contains("\"stop_condition_met=evidence_observed\""));
        assert!(payload
            .contains("\"confirmation_rule\":\"follow capability policy before tool dispatch\""));
        assert!(payload.contains("\"policy_constraints\""));
        assert!(payload.contains("\"access_mode=full_access\""));
        assert!(payload.contains("\"requires_confirmation=false\""));
        assert!(payload.contains("\"policy_decision=allow\""));
        assert!(payload.contains("permission_request="));
        assert!(payload.contains("reports/source.md"));
        assert!(payload.contains("\"selected_memories\""));
        assert!(payload.contains("memory_retrieval=memory_runtime/v1"));
        assert!(payload.contains("candidate_count=1"));
        assert!(payload.contains("selected_count=1"));
        assert!(payload.contains("query_terms_count="));
        assert!(payload.contains("inclusion_mode=compact_snippet"));
        assert!(payload.contains("rank=1"));
        assert!(payload.contains("score="));
        assert!(payload.contains("项目记忆运行规则"));
        assert!(payload.contains("soul_profile=memory/soul.md"));
        assert!(payload.contains("match_reason="));
        assert!(!payload.contains("SECRET_CONTEXT_RECEIPT_TEST"));
        assert!(!payload.contains("test-api-key"));

        let recorded_requests = transport.recorded_requests();
        assert_eq!(recorded_requests.len(), 2);
        let followup_prompt = recorded_requests[1]
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(followup_prompt.contains("loop_mode=evidence_gathering"));
        assert!(followup_prompt.contains("matched_stop_conditions=evidence_observed"));
        assert!(followup_prompt.contains(
            "validators=schema_normalized,capability_policy_checked,evidence_reference_recorded"
        ));
        assert!(followup_prompt.contains(
            "stop_conditions=evidence_observed,blocked_or_failed,user_confirmation_required"
        ));
        assert!(followup_prompt.contains(
            "Do not quote internal loop labels such as loop_mode, validators, stop_conditions, or matched_stop_conditions in the user-facing answer."
        ));
    }

    #[test]
    fn agent_chat_records_memory_candidate_gate_receipt_without_sensitive_text() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let transport = SequencedDeepSeekTransport::new(vec![
            r#"{
                "protocol_version": "ds-agent-envelope/v1",
                "reply_to_user": "I will inspect the file and propose bounded memory candidates.",
                "agent_actions": [
                    {
                        "action_type": "file_read",
                        "title": "Read evidence",
                        "reason": "Inspect the requested evidence file",
                        "risk": "low",
                        "requires_confirmation": false,
                        "target": "reports/source.md"
                    }
                ],
                "missing_prerequisites": [],
                "memory_candidates": [
                    {
                        "title": "Default response tone",
                        "body": "Use concise, warm, direct Chinese unless the user asks otherwise.",
                        "rationale": "The user wants personalized response tone memory.",
                        "memory_type": "preference",
                        "scope": "user",
                        "sensitivity": "normal",
                        "lifecycle": "active"
                    },
                    {
                        "title": "Private password",
                        "body": "The user's password is hunter2.",
                        "rationale": "The user pasted a password.",
                        "memory_type": "preference",
                        "scope": "user",
                        "sensitivity": "sensitive",
                        "lifecycle": "active"
                    },
                    {
                        "title": "Temporary draft preference",
                        "body": "Only use this wording for today's one-off draft.",
                        "rationale": "The user asked for it in this task.",
                        "memory_type": "preference",
                        "scope": "user",
                        "sensitivity": "normal",
                        "lifecycle": "active"
                    }
                ]
            }"#
            .to_string(),
            r#"{
                "protocol_version": "ds-agent-envelope/v1",
                "reply_to_user": "The evidence file was inspected.",
                "agent_actions": [],
                "missing_prerequisites": []
            }"#
            .to_string(),
        ]);
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = StoreLockCheckingFileContentClient::new(&store);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-api-key",
            AgentChatRequest {
                prompt: "Read reports/source.md and keep memory candidates auditable.".to_string(),
                model_route: ModelRoute::Auto,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::FullAccess,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat succeeds");

        assert_eq!(response.memory_candidates.len(), 1);
        assert_eq!(response.memory_candidates[0].title, "Default response tone");

        let receipt_events = store
            .lock()
            .expect("store locks")
            .list_recent(50)
            .expect("events load")
            .into_iter()
            .filter(|event| event.event_type == "agent_context_receipt_recorded")
            .collect::<Vec<_>>();
        assert_eq!(receipt_events.len(), 1);

        let payload = &receipt_events[0].payload_json;
        assert!(payload.contains("\"memory_candidate_gate\""));
        assert!(payload.contains("proposed=3"));
        assert!(payload.contains("kept=1"));
        assert!(payload.contains("dropped=2"));
        assert!(payload.contains("sensitive=1"));
        assert!(payload.contains("transient=1"));
        assert!(payload.contains("kept title=Default response tone"));
        assert!(!payload.contains("hunter2"));
        assert!(!payload.contains("The user's password"));
        assert!(!payload.contains("Only use this wording"));
    }

    #[test]
    fn agent_chat_with_transport_returns_assistant_reply_and_telemetry() {
        let transport = RecordingDeepSeekTransport::new("你好，我是 DeepSeek 驱动的 DS Agent。");
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, telemetry) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "你好，你是什么大模型？".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should return a reply");

        assert_eq!(reply.role, "assistant");
        assert_eq!(reply.content, "你好，我是 DeepSeek 驱动的 DS Agent。");
        assert_eq!(reply.model, DEEPSEEK_FLASH_MODEL);
        assert_eq!(reply.cache_status, DeepSeekChatCacheStatus::Miss);
        assert_eq!(telemetry.model, DEEPSEEK_FLASH_MODEL);
        assert_eq!(telemetry.cache_status, DeepSeekChatCacheStatus::Miss);
        let recorded = transport.recorded_requests();
        assert_eq!(recorded.len(), 1);
        assert!(recorded[0]
            .messages
            .iter()
            .any(|message| message.content.contains("你好，你是什么大模型？")));
        let user_message = recorded[0]
            .messages
            .iter()
            .find(|message| {
                matches!(
                    message.role,
                    crate::kernel::deepseek::DeepSeekChatRole::User
                )
            })
            .expect("user message should be recorded");
        assert!(user_message.content.contains("model_route=flash"));
        assert!(user_message.content.contains("thinking_level=fast"));
        assert!(user_message.content.contains("access_mode=ask_on_risk"));
        assert!(user_message.content.contains("workspace_ready=unknown"));
        assert!(user_message
            .content
            .contains("network_search_ready=unknown"));
    }

    #[test]
    #[ignore = "requires a live DEEPSEEK_API_KEY and calls the DeepSeek Chat Completions API"]
    fn agent_chat_live_deepseek_smoke_uses_ds_agent_protocol() {
        let api_key = std::env::var(DEEPSEEK_API_KEY_ENV)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .expect("DEEPSEEK_API_KEY must be configured for live agent chat smoke");
        let transport = HttpDeepSeekChatCompletionTransport::new().expect("http transport");
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, telemetry) = agent_chat_with_transport(
            &transport,
            &cache,
            &api_key,
            AgentChatRequest {
                prompt: "请用一句中文回答：DS Agent 当前是否已经连接到 DeepSeek？".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("live agent chat should return a DeepSeek-backed reply");

        assert_eq!(reply.role, "assistant");
        assert!(!reply.content.trim().is_empty());
        assert_eq!(reply.cache_status, DeepSeekChatCacheStatus::Miss);
        assert_eq!(telemetry.cache_status, DeepSeekChatCacheStatus::Miss);
        assert!(
            telemetry.total_tokens.unwrap_or_default() > 0,
            "live agent chat should record token usage"
        );
    }

    #[test]
    #[ignore = "requires a live DEEPSEEK_API_KEY and calls the DeepSeek Chat Completions API"]
    fn agent_chat_live_deepseek_smoke_can_propose_file_write_action() {
        let api_key = std::env::var(DEEPSEEK_API_KEY_ENV)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .expect("DEEPSEEK_API_KEY must be configured for live agent chat smoke");
        let transport = HttpDeepSeekChatCompletionTransport::new().expect("http transport");
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, telemetry) = agent_chat_with_transport(
            &transport,
            &cache,
            &api_key,
            AgentChatRequest {
                prompt: "请创建一份很短的 Markdown 报告，保存到 reports/live-smoke.md，内容只写一行：DS Agent live action smoke。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("live agent chat should return a DeepSeek-backed action proposal");

        let action_types = reply
            .proposed_actions
            .iter()
            .map(|action| action.action_type.as_str())
            .collect::<Vec<_>>();
        assert!(
            action_types
                .iter()
                .any(|action_type| matches!(*action_type, "file_write" | "create_report")),
            "live agent chat should propose a file-write style action, got {action_types:?}; reply={}",
            reply.content
        );
        assert!(
            reply
                .proposed_actions
                .iter()
                .any(|action| action.target.as_deref() == Some("reports/live-smoke.md")),
            "live agent chat should preserve the requested relative workspace target"
        );
        assert_eq!(telemetry.cache_status, DeepSeekChatCacheStatus::Miss);
        assert!(
            telemetry.total_tokens.unwrap_or_default() > 0,
            "live agent chat should record token usage"
        );
    }

    #[test]
    fn agent_chat_parses_structured_model_envelope_into_reply_and_actions() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会先读取工作目录里的材料，然后生成一份报告。",
            "agent_actions": [
                {
                    "action_type": "file_read",
                    "title": "读取工作目录材料",
                    "reason": "用户要求基于材料生成报告。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "sources/"
                },
                {
                    "action_type": "create_report",
                    "title": "生成报告",
                    "reason": "用户需要一份可回看的结果文件。",
                    "risk": "medium",
                    "requires_confirmation": true,
                    "target": "reports/summary.md"
                }
            ],
            "missing_prerequisites": [
                {
                    "kind": "workspace",
                    "message": "需要先设置工作目录。"
                }
            ]
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "请读取我的材料并生成报告。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should parse structured model envelope");

        assert_eq!(
            reply.content,
            "我会先读取工作目录里的材料，然后生成一份报告。"
        );
        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["action_type"],
            "file_read"
        );
        assert_eq!(
            reply_json["proposed_actions"][1]["requires_confirmation"],
            true
        );
        assert_eq!(reply_json["missing_prerequisites"][0]["kind"], "workspace");
    }

    #[test]
    fn agent_chat_parses_object_reply_and_descriptive_prerequisite_fields() {
        let model_envelope = serde_json::json!({
            "protocol_version": "0.3.0",
            "reply_to_user": {
                "role": "assistant",
                "content": "准备生成报告。由于工作区就绪状态未知，我将先执行工作区设置，之后再写入报告文件。",
                "type": "text"
            },
            "agent_actions": [
                {
                    "action_type": "workspace_setup",
                    "title": "初始化工作区",
                    "reason": "当前 workspace_ready 状态为 unknown，需要先确认或创建工作区目录结构，才能安全写入 reports/live-smoke.md。",
                    "risk": "low",
                    "requires_confirmation": false
                },
                {
                    "action_type": "file_write",
                    "title": "写入单行 Markdown 报告",
                    "reason": "用户要求创建 reports/live-smoke.md，内容仅一行。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "reports/live-smoke.md",
                    "content": "DS Agent live action smoke\n"
                }
            ],
            "missing_prerequisites": [
                {
                    "kind": "workspace",
                    "description": "需要工作区可用才能执行 file_write 到 reports/ 路径",
                    "resolution_hint": "DS Agent 先执行 workspace_setup 确保 reports/ 目录存在"
                }
            ],
            "required_confirmations": [],
            "artifact_targets": [],
            "memory_candidates": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "创建报告。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should parse object reply envelope");

        assert_eq!(
            reply.content,
            "准备生成报告。由于工作区就绪状态未知，我将先执行工作区设置，之后再写入报告文件。"
        );
        assert!(
            reply
                .proposed_actions
                .iter()
                .any(|action| action.action_type == "file_write"
                    && action.target.as_deref() == Some("reports/live-smoke.md")),
            "file_write action should survive envelope parsing"
        );
        assert_eq!(reply.missing_prerequisites[0].kind, "workspace");
        assert_eq!(
            reply.missing_prerequisites[0].message,
            "需要工作区可用才能执行 file_write 到 reports/ 路径"
        );
    }

    #[test]
    fn agent_chat_parses_markdown_fenced_structured_model_envelope() {
        let model_envelope = r#"```json
{
  "protocol_version": "ds-agent-envelope-v1",
  "reply_to_user": "我会创建这份报告。",
  "agent_actions": [
    {
      "action_type": "create_report",
      "target": "reports/live-smoke.md",
      "content": "DS Agent live action smoke"
    }
  ],
  "missing_prerequisites": []
}
```"#;
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "创建报告。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should parse fenced JSON envelope");

        assert_eq!(reply.content, "我会创建这份报告。");
        assert_eq!(reply.proposed_actions.len(), 1);
        assert_eq!(reply.proposed_actions[0].action_type, "create_report");
        assert_eq!(
            reply.proposed_actions[0].target.as_deref(),
            Some("reports/live-smoke.md")
        );
    }

    #[test]
    fn agent_chat_parses_artifact_targets_into_report_actions() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope/v1",
            "reply_to_user": "我会把结果保存成报告产物。",
            "artifact_targets": [
                {
                    "type": "report",
                    "title": "经营分析报告",
                    "description": "用户要求生成一份可回看的报告。",
                    "path": "reports/from-artifact.md",
                    "content": "# Artifact\n"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "生成一份经营分析报告。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should normalize artifact target into an action");

        assert_eq!(reply.proposed_actions.len(), 1);
        let action = &reply.proposed_actions[0];
        assert_eq!(action.action_type, "create_report");
        assert_eq!(
            action.capability,
            Some(crate::kernel::policy::CapabilityKind::FileWrite)
        );
        assert_eq!(action.target.as_deref(), Some("reports/from-artifact.md"));
        assert_eq!(action.content.as_deref(), Some("# Artifact"));
        assert_eq!(action.execution_state, "needs_confirmation");
    }

    #[test]
    fn agent_chat_generates_safe_report_target_for_artifact_without_path() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope/v1",
            "reply_to_user": "我会创建季度小结。",
            "artifact_targets": [
                {
                    "type": "report",
                    "title": "Quarterly Summary",
                    "description": "用户要求保留季度小结。",
                    "content": "Quarterly summary body"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "生成季度小结。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should generate a safe workspace-relative artifact target");

        assert_eq!(reply.proposed_actions.len(), 1);
        let target = reply.proposed_actions[0]
            .target
            .as_deref()
            .expect("artifact target should be generated");
        assert!(target.starts_with("reports/quarterly-summary-"));
        assert!(target.ends_with(".md"));
        assert!(!target.contains(".."));
    }

    #[test]
    fn agent_chat_parses_boundary_version_and_workflow_calls() {
        let model_envelope = serde_json::json!({
            "version": "ds-agent-envelope/v1",
            "user_reply": "我会把经营简报作为工作流能力执行。",
            "workflow_calls": [
                {
                    "action_type": "operations_briefing",
                    "title": "运行经营简报工作流",
                    "reason": "用户要求基于证据目录生成经营简报。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "evidence/operations"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "根据证据目录生成经营简报。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should parse boundary envelope workflow calls");

        assert_eq!(reply.protocol_version, "ds-agent-envelope/v1");
        assert_eq!(reply.proposed_actions.len(), 1);
        assert_eq!(reply.proposed_actions[0].action_type, "operations_briefing");
        assert_eq!(reply.proposed_actions[0].execution_state, "proposed");
    }

    #[test]
    fn agent_chat_parses_numeric_protocol_version_without_showing_raw_json() {
        let model_envelope = serde_json::json!({
            "protocol_version": 1,
            "reply_to_user": "已创建并打开文件。",
            "agent_actions": [],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "创建并打开一个 PowerPoint。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should parse numeric protocol version envelope");

        assert_eq!(reply.content, "已创建并打开文件。");
        assert_eq!(reply.protocol_version, "1");
    }

    #[test]
    fn agent_chat_sends_full_user_message_with_agent_protocol_context() {
        let transport = RecordingDeepSeekTransport::new("普通回复");
        let cache = DeepSeekMemoryChatCompletionCache::default();

        agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "帮我看一下这个工作包应该怎么处理。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should call model");

        let recorded = transport.recorded_requests();
        let user_message = recorded[0]
            .messages
            .iter()
            .find(|message| {
                matches!(
                    message.role,
                    crate::kernel::deepseek::DeepSeekChatRole::User
                )
            })
            .expect("user message is sent");
        assert!(user_message
            .content
            .contains("帮我看一下这个工作包应该怎么处理。"));
        assert!(user_message.content.contains("DS Agent protocol context"));
        assert!(user_message.content.contains("structured agent envelope"));
        assert!(user_message.content.contains("reply_to_user"));
        assert!(user_message.content.contains("agent_actions"));
        assert!(user_message.content.contains("goal_contract"));
        assert!(user_message.content.contains("done_when"));
        assert!(user_message.content.contains("completion_verifier"));
        assert!(user_message.content.contains("stop_conditions"));
        assert!(user_message.content.contains("near-miss"));
        assert!(user_message.content.contains("completion_advice"));
    }

    #[test]
    fn agent_chat_selects_relevant_reviewed_memory_for_protocol_prompt() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let now = Utc::now();
        let reviewed_memory = MemoryRecord {
            id: Uuid::new_v4(),
            title: "用户默认语气偏好".to_string(),
            body: "用户希望 DS Agent 默认用简洁、温暖、直接的中文语气回复。".to_string(),
            memory_type: MemoryType::Preference,
            scope: MemoryScope::User,
            sensitivity: MemorySensitivity::Normal,
            lifecycle: MemoryLifecycle::Active,
            source: MemoryRecordSource::MemoryCandidate,
            source_id: None,
            pinned: false,
            expires_at: None,
            linked_memory_ids: Vec::new(),
            linked_memories: Vec::new(),
            search_match: MemorySearchMatch::direct(),
            created_at: now,
            updated_at: now,
        };
        let sensitive_memory = MemoryRecord {
            id: Uuid::new_v4(),
            title: "敏感 API 信息".to_string(),
            body: "SECRET_MEMORY_SHOULD_NOT_APPEAR".to_string(),
            sensitivity: MemorySensitivity::Sensitive,
            ..reviewed_memory.clone()
        };
        let archived_memory = MemoryRecord {
            id: Uuid::new_v4(),
            title: "旧称呼偏好".to_string(),
            body: "ARCHIVED_MEMORY_SHOULD_NOT_APPEAR".to_string(),
            lifecycle: MemoryLifecycle::Archived,
            ..reviewed_memory.clone()
        };
        {
            let store = store.lock().expect("store locks");
            store
                .append_memory_record(&reviewed_memory)
                .expect("reviewed memory appends");
            store
                .append_memory_record(&sensitive_memory)
                .expect("sensitive memory appends");
            store
                .append_memory_record(&archived_memory)
                .expect("archived memory appends");
        }

        let transport = RecordingDeepSeekTransport::new("普通回复");
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = StoreLockCheckingFileContentClient::new(&store);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-api-key",
            AgentChatRequest {
                prompt: "请按我喜欢的默认语气总结这个项目。".to_string(),
                model_route: ModelRoute::Auto,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat succeeds");

        let recorded = transport.recorded_requests();
        assert_eq!(recorded.len(), 1);
        let user_message = recorded[0]
            .messages
            .iter()
            .find(|message| {
                matches!(
                    message.role,
                    crate::kernel::deepseek::DeepSeekChatRole::User
                )
            })
            .expect("user message is sent");
        assert!(user_message
            .content
            .contains("Selected reviewed DS Agent memories"));
        assert!(user_message
            .content
            .contains("retrieval_receipt=memory_runtime/v1"));
        assert!(user_message.content.contains("candidate_count=1"));
        assert!(user_message.content.contains("selected_count=1"));
        assert!(user_message.content.contains("query_terms_count="));
        assert!(user_message.content.contains("filtered_sensitive=1"));
        assert!(user_message.content.contains("filtered_archived=1"));
        assert!(user_message
            .content
            .contains("inclusion_mode=compact_snippet"));
        assert!(user_message.content.contains("rank=1"));
        assert!(user_message.content.contains("score="));
        assert!(user_message.content.contains("用户默认语气偏好"));
        assert!(user_message.content.contains("match_reason="));
        assert!(user_message
            .content
            .contains("用户希望 DS Agent 默认用简洁"));
        assert!(!user_message
            .content
            .contains("SECRET_MEMORY_SHOULD_NOT_APPEAR"));
        assert!(!user_message
            .content
            .contains("ARCHIVED_MEMORY_SHOULD_NOT_APPEAR"));
    }

    #[test]
    fn agent_memory_runtime_context_caps_records_and_reports_budget_omissions() {
        let now = Utc::now();
        let memories = (0..4)
            .map(|index| MemoryRecord {
                id: Uuid::new_v4(),
                title: format!("默认语气偏好 {index}"),
                body: format!("默认语气稳定偏好 {index}，只允许作为紧凑片段进入上下文。"),
                memory_type: MemoryType::Preference,
                scope: MemoryScope::User,
                sensitivity: MemorySensitivity::Normal,
                lifecycle: MemoryLifecycle::Active,
                source: MemoryRecordSource::MemoryCandidate,
                source_id: None,
                pinned: false,
                expires_at: None,
                linked_memory_ids: Vec::new(),
                linked_memories: Vec::new(),
                search_match: MemorySearchMatch::direct(),
                created_at: now,
                updated_at: now + Duration::seconds(index),
            })
            .collect::<Vec<_>>();

        let context = super::select_agent_memory_runtime_context("请按默认语气回复", &memories);

        assert_eq!(context.considered_records, 4);
        assert_eq!(context.candidate_count, 4);
        assert_eq!(
            context.selected.len(),
            super::AGENT_MEMORY_CONTEXT_MAX_RECORDS
        );
        assert_eq!(context.omitted_by_budget, 1);
        assert!(context.query_terms_count > 0);
        assert!(context
            .omissions
            .iter()
            .any(|line| line == "1 lower-ranked memories omitted by context budget"));
        assert_eq!(
            context
                .selected
                .iter()
                .map(|memory| memory.rank)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(context
            .selected
            .iter()
            .all(|memory| memory.inclusion_mode == "compact_snippet"));

        let prompt = super::build_agent_memory_context_prompt(&context);
        assert!(prompt.contains("retrieval_receipt=memory_runtime/v1"));
        assert!(prompt.contains("candidate_count=4"));
        assert!(prompt.contains("selected_count=3"));
        assert!(prompt.contains("omitted_by_budget=1"));

        let receipt = super::agent_memory_retrieval_receipt_line(&context);
        assert!(receipt.contains("memory_retrieval=memory_runtime/v1"));
        assert!(receipt.contains("max_records=3"));
        assert!(receipt.contains("omitted_by_budget=1"));
    }

    #[test]
    fn soul_profile_loader_builds_compact_identity_packet_without_never_store() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let memory_dir = temp_dir
            .path()
            .join(crate::kernel::local_directory::LOCAL_MEMORY_DIR_NAME);
        std::fs::create_dir_all(&memory_dir).expect("memory dir");
        std::fs::write(
            memory_dir.join(super::AGENT_SOUL_PROFILE_FILE_NAME),
            r#"# DS Agent Soul

schema_version: 1

## User

- preferred_name: 李总
- address_as: 李总
- default_response_tone: 简洁、温暖、直接
- default_response_length: concise

## DS Agent

- user_calls_ds_agent: 小 D
- ds_agent_should_refer_to_itself_as: DS Agent

## Never Store

- SECRET_SOUL_SHOULD_NOT_APPEAR
- passwords
"#,
        )
        .expect("write soul profile");

        let profile = super::load_agent_soul_profile_context(temp_dir.path())
            .expect("soul profile loads")
            .expect("soul profile exists");
        let packet = profile.lines.join("\n");

        assert!(packet.contains("user preferred address: 李总"));
        assert!(packet.contains("user calls this app: 小 D"));
        assert!(packet.contains("response defaults:"));
        assert!(profile.used_bytes <= super::AGENT_SOUL_PROFILE_CONTEXT_MAX_BYTES);
        assert!(!packet.contains("SECRET_SOUL_SHOULD_NOT_APPEAR"));
        assert!(!packet.contains("passwords"));
    }

    #[test]
    fn soul_profile_state_returns_template_without_writing_missing_file() {
        let temp_dir = tempfile::tempdir().expect("tempdir");

        let state = super::agent_soul_profile_state_from_app_data_dir(temp_dir.path())
            .expect("soul profile state loads");

        assert!(!state.exists);
        assert!(state.content.contains("# DS Agent Soul"));
        assert!(state.summary_lines.is_empty());
        assert!(!temp_dir
            .path()
            .join(crate::kernel::local_directory::LOCAL_MEMORY_DIR_NAME)
            .join(super::AGENT_SOUL_PROFILE_FILE_NAME)
            .exists());
    }

    #[test]
    fn saving_soul_profile_is_explicit_and_returns_safe_summary() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let content = r#"# DS Agent Soul

## User

- preferred_name: 李总
- default_response_tone: 简洁、温暖、直接

## DS Agent

- user_calls_ds_agent: 小 D

## Never Store

- SECRET_PROFILE_SHOULD_NOT_APPEAR
"#;

        let state = super::save_agent_soul_profile_content(temp_dir.path(), content)
            .expect("soul profile saves");

        assert!(state.exists);
        assert_eq!(state.content, content);
        assert!(state.summary_lines.join("\n").contains("李总"));
        assert!(state.summary_lines.join("\n").contains("小 D"));
        assert!(!state
            .summary_lines
            .join("\n")
            .contains("SECRET_PROFILE_SHOULD_NOT_APPEAR"));
        assert_eq!(
            std::fs::read_to_string(
                temp_dir
                    .path()
                    .join(crate::kernel::local_directory::LOCAL_MEMORY_DIR_NAME)
                    .join(super::AGENT_SOUL_PROFILE_FILE_NAME),
            )
            .expect("saved profile reads"),
            content
        );
    }

    #[test]
    fn agent_chat_protocol_prompt_includes_soul_profile_context() {
        let mut runtime_context = AgentChatRuntimeContext::default();
        runtime_context.soul_profile = Some(super::AgentSoulProfileContext {
            lines: vec![
                "user preferred address: 李总".to_string(),
                "user calls this app: 小 D".to_string(),
                "response defaults: 简洁、温暖、直接".to_string(),
            ],
            used_bytes: 112,
            max_bytes: super::AGENT_SOUL_PROFILE_CONTEXT_MAX_BYTES,
        });

        let prompt = super::build_agent_chat_protocol_user_prompt(
            &AgentChatRequest {
                prompt: "帮我写一段项目总结。".to_string(),
                model_route: ModelRoute::Auto,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            &runtime_context,
        );

        assert!(prompt.contains("DS Agent identity profile"));
        assert!(prompt.contains("user preferred address: 李总"));
        assert!(prompt.contains("user calls this app: 小 D"));
        assert!(prompt.contains("Profile limits: soul.md compact summary, raw file body omitted."));
    }

    #[test]
    fn agent_chat_blocks_unknown_model_action_proposals() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "这个动作需要先由 DS Agent 校验。",
            "agent_actions": [
                {
                    "action_type": "delete_workspace_without_confirmation",
                    "title": "删除工作目录",
                    "reason": "模型提出了不允许的动作。",
                    "risk": "critical",
                    "requires_confirmation": false,
                    "target": "workspace"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "把工作目录删掉。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should return blocked action proposal");

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["execution_state"],
            "blocked"
        );
        assert!(reply_json["proposed_actions"][0]["blocked_reason"]
            .as_str()
            .unwrap_or_default()
            .contains("unsupported action_type"));
    }

    #[test]
    fn agent_chat_normalizes_safe_run_shell_url_to_browser_open() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会先打开 GitHub 首页，登录凭证请你手动输入。",
            "agent_actions": [
                {
                    "action_type": "run_shell",
                    "title": "在浏览器中打开 GitHub",
                    "reason": "用户要求打开 GitHub 首页。",
                    "risk": "medium",
                    "requires_confirmation": true,
                    "command": "chrome https://github.com/"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "用 Chrome 打开 GitHub 首页。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::LimitedAuto,
            },
            None,
        )
        .expect("agent chat should normalize safe URL-opening shell proposal");

        let action = reply
            .proposed_actions
            .first()
            .expect("normalized action exists");
        assert_eq!(action.action_type, "browser_open");
        assert_eq!(action.capability, Some(CapabilityKind::BrowserBrowse));
        assert_eq!(action.target.as_deref(), Some("https://github.com/"));
        assert_eq!(action.risk.as_deref(), Some("low"));
        assert!(!action.requires_confirmation);
        assert_eq!(action.execution_state, "proposed");
        assert_ne!(action.execution_state, "blocked");
        assert!(action.blocked_reason.is_none());
    }

    #[test]
    fn agent_chat_corrects_open_site_browser_browse_to_browser_open() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会在 Chrome 中打开 GitHub 首页。",
            "agent_actions": [
                {
                    "action_type": "browser_browse",
                    "title": "在 Chrome 中打开 GitHub 首页",
                    "reason": "用户要求在 Chrome 中打开 GitHub 首页。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "https://github.com"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "帮我用 chrome 打开 github 首页。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should normalize open-site browser browse");

        let action = reply.proposed_actions.first().expect("action exists");
        assert_eq!(action.action_type, "browser_open");
        assert_eq!(action.target.as_deref(), Some("https://github.com"));
        assert!(!action.requires_confirmation);
        assert_eq!(action.execution_state, "proposed");
    }

    #[test]
    fn agent_chat_preserves_chrome_preference_for_browser_open_actions() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会优先用 Chrome 打开 GitHub 首页；如果本机没有 Chrome，DS Agent 会用默认浏览器打开。",
            "agent_actions": [
                {
                    "action_type": "browser_open",
                    "title": "用 Chrome 打开 GitHub 首页",
                    "reason": "用户明确指定 Chrome。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "https://github.com",
                    "preferred_browser": "chrome"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "帮我用 chrome 打开 github 首页。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should preserve browser preference");

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["preferred_browser"],
            "chrome"
        );
    }

    #[test]
    fn agent_chat_keeps_browser_browse_for_page_reading_actions() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会读取页面内容作为证据。",
            "agent_actions": [
                {
                    "action_type": "browser_browse",
                    "title": "读取 GitHub 首页内容",
                    "reason": "用户要求检查网页内容并提取证据。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "https://github.com"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "读取 github 首页内容。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should preserve page-reading browser browse");

        let action = reply.proposed_actions.first().expect("action exists");
        assert_eq!(action.action_type, "browser_browse");
        assert_eq!(action.target.as_deref(), Some("https://github.com"));
    }

    #[test]
    fn agent_chat_keeps_arbitrary_run_shell_blocked() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "这个动作需要先由 DS Agent 校验。",
            "agent_actions": [
                {
                    "action_type": "run_shell",
                    "title": "运行本地命令",
                    "reason": "模型提出了任意 shell 命令。",
                    "risk": "high",
                    "requires_confirmation": false,
                    "command": "Remove-Item -Recurse C:\\\\Users\\\\prosb\\\\Documents"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "执行这个命令。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::FullAccess,
            },
            None,
        )
        .expect("agent chat should return blocked shell proposal");

        let action = reply
            .proposed_actions
            .first()
            .expect("blocked action exists");
        assert_eq!(action.action_type, "run_shell");
        assert_eq!(action.execution_state, "blocked");
        assert!(action
            .blocked_reason
            .as_deref()
            .unwrap_or_default()
            .contains("does not execute arbitrary shell commands"));
        assert!(action
            .blocked_reason
            .as_deref()
            .unwrap_or_default()
            .contains("browser_open"));
    }

    #[test]
    fn agent_chat_uses_local_capability_policy_over_model_claimed_risk() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我可以生成文件。",
            "agent_actions": [
                {
                    "action_type": "file_write",
                    "title": "写入报告",
                    "reason": "用户要求生成报告文件。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "reports/summary.md"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "生成报告并保存。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should normalize model action through local policy");

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["capability"],
            "file_write"
        );
        assert_eq!(reply_json["proposed_actions"][0]["policy_decision"], "ask");
        assert_eq!(
            reply_json["proposed_actions"][0]["execution_state"],
            "needs_confirmation"
        );
        assert!(reply_json["proposed_actions"][0]["dispatch_note"]
            .as_str()
            .unwrap_or_default()
            .contains("local capability policy"));
    }

    #[test]
    fn agent_chat_normalizes_word_create_to_office_create_action() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会创建 Word 文档。",
            "agent_actions": [
                {
                    "action_type": "word_create",
                    "title": "创建 Word 文档",
                    "reason": "用户要求创建一个 Word 文档。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "office/test.docx",
                    "content": "我在测试"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "在桌面创建一个 Word 文档，写入我在测试。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should normalize Word creation");

        let action = reply.proposed_actions.first().expect("action exists");
        assert_eq!(action.action_type, "office_create");
        assert_eq!(action.capability, Some(CapabilityKind::FileWrite));
        assert_eq!(action.policy_decision, Some(PolicyDecision::Allow));
        assert_eq!(action.execution_state, "proposed");
        assert_eq!(action.target.as_deref(), Some("office/test.docx"));
    }

    #[test]
    fn agent_chat_parses_office_create_with_object_content_without_showing_raw_json() {
        let model_envelope = serde_json::json!({
            "protocol_version": "1.0",
            "reply_to_user": "好的，我将在 DS Agent 的工作区中创建一个 Word 文档，内容为“我在测试”。",
            "agent_actions": [
                {
                    "action_type": "office_create",
                    "title": "创建 Word 文档",
                    "reason": "用户请求创建 Word 文档并写入文本。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "测试文档.docx",
                    "content": {
                        "app": "word",
                        "body": "我在测试"
                    }
                }
            ],
            "missing_prerequisites": [],
            "required_confirmations": [],
            "artifact_targets": [],
            "memory_candidates": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "现在测试在桌面创建一个word文档，文档内写“我在测试”".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should parse object action content");

        assert_eq!(
            reply.content,
            "好的，我将在 DS Agent 的工作区中创建一个 Word 文档，内容为“我在测试”。"
        );
        assert!(
            !reply.content.contains("agent_actions"),
            "raw model JSON must not be shown as the chat reply"
        );

        let action = reply.proposed_actions.first().expect("action exists");
        assert_eq!(action.action_type, "office_create");
        assert_eq!(action.target.as_deref(), Some("测试文档.docx"));
        assert_eq!(
            action.content.as_deref(),
            Some(r#"{"app":"word","body":"我在测试"}"#)
        );
        assert_eq!(action.capability, Some(CapabilityKind::FileWrite));
        assert_eq!(action.policy_decision, Some(PolicyDecision::Allow));
        assert_eq!(action.execution_state, "proposed");
    }

    #[test]
    fn agent_chat_office_create_dispatches_approved_word_document() {
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let client = RecordingFileWriteClient::new();
        let approval_request_id = Uuid::new_v4();
        let mut action = AgentChatActionProposal {
            action_type: "office_create".to_string(),
            title: Some("测试文档".to_string()),
            reason: Some("用户要求创建 Word 文档。".to_string()),
            risk: Some("medium".to_string()),
            requires_confirmation: true,
            target: Some("office/test.docx".to_string()),
            target_location: None,
            destination: None,
            preferred_browser: None,
            content: Some("我在测试".to_string()),
            capability: Some(CapabilityKind::FileWrite),
            policy_decision: Some(PolicyDecision::Ask),
            execution_state: "needs_confirmation".to_string(),
            dispatch_note: None,
            permission_request_id: Some(approval_request_id),
            capability_invocation_id: None,
            workflow_run_id: None,
            blocked_reason: None,
        };

        dispatch_agent_office_create_action(
            &store,
            AccessMode::AskOnRisk,
            &mut action,
            &client,
            true,
            Some(approval_request_id),
        )
        .expect("office create action dispatches");

        assert_eq!(action.execution_state, "succeeded");
        assert!(action
            .dispatch_note
            .as_deref()
            .unwrap_or_default()
            .contains("Word document created"));
        assert!(action
            .dispatch_note
            .as_deref()
            .unwrap_or_default()
            .contains("我在测试"));
        assert_eq!(
            client.recorded_calls(),
            vec![("office/test.docx".to_string(), "office:Word".to_string())]
        );
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].capability, CapabilityKind::FileWrite);
        assert_eq!(
            invocations[0].approval_request_id,
            Some(approval_request_id)
        );
    }

    #[test]
    fn agent_chat_normalizes_office_open_alias_to_office_open_action() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会打开 Excel 工作簿。",
            "agent_actions": [
                {
                    "action_type": "open_excel",
                    "title": "打开 Excel 工作簿",
                    "reason": "用户要求打开刚创建的 Excel 文件。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "office/test.xlsx"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "打开刚才创建的 Excel 表。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should normalize Office open");

        let action = reply.proposed_actions.first().expect("action exists");
        assert_eq!(action.action_type, "office_open");
        assert_eq!(action.capability, Some(CapabilityKind::FileRead));
        assert_eq!(action.policy_decision, Some(PolicyDecision::Allow));
        assert_eq!(action.execution_state, "proposed");
        assert_eq!(action.target.as_deref(), Some("office/test.xlsx"));
    }

    #[test]
    fn agent_chat_office_open_dispatches_allowed_workbook_with_fallback_note() {
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let client = RecordingFileWriteClient::new();
        let mut action = AgentChatActionProposal {
            action_type: "office_open".to_string(),
            title: Some("打开 Excel 工作簿".to_string()),
            reason: Some("用户要求打开 Excel 工作簿。".to_string()),
            risk: Some("low".to_string()),
            requires_confirmation: false,
            target: Some("office/test.xlsx".to_string()),
            target_location: None,
            destination: None,
            preferred_browser: None,
            content: None,
            capability: Some(CapabilityKind::FileRead),
            policy_decision: Some(PolicyDecision::Allow),
            execution_state: "proposed".to_string(),
            dispatch_note: None,
            permission_request_id: None,
            capability_invocation_id: None,
            workflow_run_id: None,
            blocked_reason: None,
        };

        dispatch_agent_office_open_action(
            &store,
            AccessMode::AskOnRisk,
            &mut action,
            &client,
            false,
            None,
        )
        .expect("office open action dispatches");

        assert_eq!(action.execution_state, "succeeded");
        assert!(action
            .dispatch_note
            .as_deref()
            .unwrap_or_default()
            .contains("Excel workbook opened"));
        assert!(action
            .dispatch_note
            .as_deref()
            .unwrap_or_default()
            .contains("默认应用"));
        assert_eq!(
            client.recorded_calls(),
            vec![(
                "office/test.xlsx".to_string(),
                "office-open:Some(Excel)".to_string()
            )]
        );
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].capability, CapabilityKind::FileRead);
    }

    #[test]
    fn agent_chat_creates_then_opens_office_file_without_extra_confirmation() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会创建并打开 Excel 工作簿。",
            "agent_actions": [
                {
                    "action_type": "office_create",
                    "title": "创建 Excel 工作簿",
                    "reason": "用户要求创建 Excel 文件。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "office/test.xlsx",
                    "content": "{\"app\":\"excel\",\"rows\":[[\"项目\",\"数值\"],[\"测试\",1]]}"
                },
                {
                    "action_type": "office_open",
                    "title": "打开 Excel 工作簿",
                    "reason": "创建后打开 Excel 文件。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "office/test.xlsx"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "创建一个 Excel 表并打开。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should return office create and open proposals");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::AskOnRisk,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("dispatch should create and open the office file");

        assert_eq!(reply.proposed_actions[0].execution_state, "succeeded");
        assert_eq!(reply.proposed_actions[1].execution_state, "succeeded");
        assert_eq!(
            file_write_client.recorded_calls(),
            vec![
                ("office/test.xlsx".to_string(), "office:Excel".to_string()),
                (
                    "office/test.xlsx".to_string(),
                    "office-open:Some(Excel)".to_string()
                )
            ]
        );
    }

    #[test]
    fn agent_chat_creates_desktop_word_and_opens_without_confirmation() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会创建并打开 Word 文档，完成后由 DS Agent 返回执行结果。",
            "agent_actions": [
                {
                    "action_type": "office_create",
                    "title": "创建Word文档",
                    "reason": "用户要求在桌面创建 Word 文档。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "测试文档.docx",
                    "target_location": "desktop",
                    "content": {"app":"word","body":"我在测试"}
                },
                {
                    "action_type": "office_open",
                    "title": "打开刚创建的Word文档",
                    "reason": "创建后打开 Word 文档供用户检查。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "测试文档.docx",
                    "target_location": "desktop"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "现在测试在桌面创建一个word文档，文档内写“我在测试”，并打开。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should return structured desktop office actions");

        assert_eq!(reply.proposed_actions.len(), 2);
        assert!(reply
            .proposed_actions
            .iter()
            .all(|action| action.execution_state == "proposed"));
        assert!(reply
            .proposed_actions
            .iter()
            .all(|action| action.policy_decision == Some(PolicyDecision::Allow)));
        assert!(reply
            .proposed_actions
            .iter()
            .all(|action| action.permission_request_id.is_none()));

        dispatch_agent_action_proposals(
            &store,
            AccessMode::AskOnRisk,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("desktop office actions should dispatch");

        assert!(reply
            .proposed_actions
            .iter()
            .all(|action| action.execution_state == "succeeded"));
        assert!(reply
            .proposed_actions
            .iter()
            .all(|action| action.permission_request_id.is_none()));
        assert_eq!(
            reply.proposed_actions[0].target.as_deref(),
            Some("desktop/测试文档.docx")
        );
        assert_eq!(
            file_write_client.recorded_calls(),
            vec![
                (
                    "desktop/测试文档.docx".to_string(),
                    "office:Word".to_string()
                ),
                (
                    "desktop/测试文档.docx".to_string(),
                    "office-open:Some(Word)".to_string()
                )
            ]
        );
    }

    #[test]
    fn agent_chat_normalizes_excel_update_to_office_update_action() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会更新 Excel 工作簿。",
            "agent_actions": [
                {
                    "action_type": "excel_update",
                    "title": "更新 Excel 工作簿",
                    "reason": "用户要求向 Excel 增加一行。",
                    "risk": "medium",
                    "requires_confirmation": true,
                    "target": "office/test.xlsx",
                    "content": "{\"app\":\"excel\",\"rows\":[[\"合计\",\"=SUM(B2:B2)\"]]}"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "把 Excel 表增加一行合计。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should normalize Office update");

        let action = reply.proposed_actions.first().expect("action exists");
        assert_eq!(action.action_type, "office_update");
        assert_eq!(action.capability, Some(CapabilityKind::FileWrite));
        assert_eq!(action.policy_decision, Some(PolicyDecision::Ask));
        assert_eq!(action.execution_state, "needs_confirmation");
        assert_eq!(action.target.as_deref(), Some("office/test.xlsx"));
    }

    #[test]
    fn agent_chat_office_update_dispatches_approved_word_document() {
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let client = RecordingFileWriteClient::new();
        let approval_request_id = Uuid::new_v4();
        let mut action = AgentChatActionProposal {
            action_type: "office_update".to_string(),
            title: Some("更新 Word 文档".to_string()),
            reason: Some("用户要求追加 Word 文档内容。".to_string()),
            risk: Some("medium".to_string()),
            requires_confirmation: true,
            target: Some("office/test.docx".to_string()),
            target_location: None,
            destination: None,
            preferred_browser: None,
            content: Some("追加内容".to_string()),
            capability: Some(CapabilityKind::FileWrite),
            policy_decision: Some(PolicyDecision::Ask),
            execution_state: "needs_confirmation".to_string(),
            dispatch_note: None,
            permission_request_id: Some(approval_request_id),
            capability_invocation_id: None,
            workflow_run_id: None,
            blocked_reason: None,
        };

        dispatch_agent_office_update_action(
            &store,
            AccessMode::AskOnRisk,
            &mut action,
            &client,
            true,
            Some(approval_request_id),
        )
        .expect("office update action dispatches");

        assert_eq!(action.execution_state, "succeeded");
        assert!(action
            .dispatch_note
            .as_deref()
            .unwrap_or_default()
            .contains("Word document updated"));
        assert_eq!(
            client.recorded_calls(),
            vec![(
                "office/test.docx".to_string(),
                "office-update:Word".to_string()
            )]
        );
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].capability, CapabilityKind::FileWrite);
        assert_eq!(
            invocations[0].approval_request_id,
            Some(approval_request_id)
        );
    }

    #[test]
    fn agent_chat_computer_control_dispatches_approved_structured_action() {
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let client = RecordingComputerControlClient::new();
        let approval_request_id = Uuid::new_v4();
        let mut action = AgentChatActionProposal {
            action_type: "computer_control".to_string(),
            title: Some("Type test text".to_string()),
            reason: Some("User asked DS Agent to write text in Word.".to_string()),
            risk: Some("critical".to_string()),
            requires_confirmation: true,
            target: Some("Microsoft Word".to_string()),
            target_location: None,
            destination: None,
            preferred_browser: None,
            content: Some("type:我在测试".to_string()),
            capability: Some(CapabilityKind::ComputerControl),
            policy_decision: Some(PolicyDecision::Ask),
            execution_state: "needs_confirmation".to_string(),
            dispatch_note: None,
            permission_request_id: Some(approval_request_id),
            capability_invocation_id: None,
            workflow_run_id: None,
            blocked_reason: None,
        };

        dispatch_agent_computer_control_action(
            &store,
            AccessMode::AskOnRisk,
            &mut action,
            &client,
            true,
            Some(approval_request_id),
        )
        .expect("computer control action dispatches");

        assert_eq!(action.execution_state, "succeeded");
        assert!(action
            .dispatch_note
            .as_deref()
            .unwrap_or_default()
            .contains("computer control completed"));
        let calls = client.recorded_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "Microsoft Word");
        assert_eq!(
            calls[0].1,
            ComputerControlAction::TypeText {
                text: "我在测试".to_string()
            }
        );
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].capability, CapabilityKind::ComputerControl);
        assert_eq!(
            invocations[0].approval_request_id,
            Some(approval_request_id)
        );
    }

    #[test]
    fn agent_chat_computer_control_blocks_unstructured_natural_language_action() {
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let client = RecordingComputerControlClient::new();
        let mut action = AgentChatActionProposal {
            action_type: "computer_control".to_string(),
            title: Some("Create Word document".to_string()),
            reason: Some("Create a Word document on the desktop and write test text.".to_string()),
            risk: Some("critical".to_string()),
            requires_confirmation: true,
            target: Some("Microsoft Word".to_string()),
            target_location: None,
            destination: None,
            preferred_browser: None,
            content: Some("create a Word document and type 我在测试".to_string()),
            capability: Some(CapabilityKind::ComputerControl),
            policy_decision: Some(PolicyDecision::Ask),
            execution_state: "needs_confirmation".to_string(),
            dispatch_note: None,
            permission_request_id: Some(Uuid::new_v4()),
            capability_invocation_id: None,
            workflow_run_id: None,
            blocked_reason: None,
        };
        let approval_request_id = action.permission_request_id;

        dispatch_agent_computer_control_action(
            &store,
            AccessMode::AskOnRisk,
            &mut action,
            &client,
            true,
            approval_request_id,
        )
        .expect("unstructured action is handled on the action");

        assert_eq!(action.execution_state, "blocked");
        assert!(action
            .blocked_reason
            .as_deref()
            .unwrap_or_default()
            .contains("structured action"));
        assert!(client.recorded_calls().is_empty());
        assert!(store
            .list_capability_invocations()
            .expect("invocations load")
            .is_empty());
    }

    #[test]
    fn agent_chat_action_policy_uses_request_access_mode() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我需要读取一个文件。",
            "agent_actions": [
                {
                    "action_type": "file_read",
                    "title": "读取材料",
                    "reason": "用户要求分析本地材料。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "sources/input.md"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let (reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "读取 sources/input.md。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskEveryStep,
            },
            None,
        )
        .expect("agent chat should apply request access mode");

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(reply_json["proposed_actions"][0]["capability"], "file_read");
        assert_eq!(reply_json["proposed_actions"][0]["policy_decision"], "ask");
        assert_eq!(
            reply_json["proposed_actions"][0]["execution_state"],
            "needs_confirmation"
        );
    }

    #[test]
    fn agent_chat_records_permission_request_for_actions_that_need_confirmation() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会生成报告文件，但需要你确认写入权限。",
            "agent_actions": [
                {
                    "action_type": "file_write",
                    "title": "写入报告",
                    "reason": "用户要求保存报告。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "reports/summary.md"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "生成报告并保存。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should return action proposal");
        record_agent_action_permission_requests(&store, AccessMode::AskOnRisk, &mut reply)
            .expect("permission request records");

        let pending = store
            .list_pending_capability_access_records()
            .expect("pending approvals load");
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].request.capability,
            crate::kernel::policy::CapabilityKind::FileWrite
        );

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["permission_request_id"],
            pending[0].request.id.to_string()
        );
        assert!(reply_json["proposed_actions"][0]["dispatch_note"]
            .as_str()
            .unwrap_or_default()
            .contains("waiting for local permission approval"));
    }

    #[test]
    fn agent_chat_dispatches_action_after_specific_permission_request_is_approved() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_path = temp_dir.path().join("source.md");
        std::fs::write(&source_path, "Approved evidence").expect("write source");
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "读取文件前需要你批准。",
            "agent_actions": [
                {
                    "action_type": "file_read",
                    "title": "读取材料",
                    "reason": "用户要求读取本地材料。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": source_path.to_string_lossy()
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "读取这个文件。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskEveryStep,
            },
            None,
        )
        .expect("agent chat should return file read proposal");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::AskEveryStep,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("first dispatch should request permission");

        let permission_request_id = reply.proposed_actions[0]
            .permission_request_id
            .expect("permission request id should be attached");
        assert_eq!(
            reply.proposed_actions[0].execution_state,
            "needs_confirmation"
        );
        assert!(store
            .list_capability_invocations()
            .expect("invocations load")
            .is_empty());
        store
            .resolve_capability_access_request(
                permission_request_id,
                true,
                "approved for this chat action".to_string(),
            )
            .expect("permission request resolves");

        dispatch_agent_action_proposals(
            &store,
            AccessMode::AskEveryStep,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("approved action should dispatch");

        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(reply.proposed_actions[0].execution_state, "succeeded");
        assert_eq!(
            reply.proposed_actions[0].capability_invocation_id,
            Some(invocations[0].id)
        );
        assert!(reply.proposed_actions[0]
            .dispatch_note
            .as_deref()
            .unwrap_or_default()
            .contains("Approved evidence"));
    }

    #[test]
    fn agent_chat_resume_helper_returns_updated_action_after_permission_approval() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_path = temp_dir.path().join("source.md");
        std::fs::write(&source_path, "Resumed evidence").expect("write source");
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "读取文件前需要你批准。",
            "agent_actions": [
                {
                    "action_type": "file_read",
                    "title": "读取材料",
                    "reason": "用户要求读取本地材料。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": source_path.to_string_lossy()
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "读取这个文件。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskEveryStep,
            },
            None,
        )
        .expect("agent chat should return file read proposal");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::AskEveryStep,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("first dispatch should request permission");
        let permission_request_id = reply.proposed_actions[0]
            .permission_request_id
            .expect("permission request id should be attached");
        store
            .resolve_capability_access_request(
                permission_request_id,
                true,
                "approved for resume helper".to_string(),
            )
            .expect("permission request resolves");

        let resumed_action = resume_agent_chat_action_with_clients(
            &store,
            AccessMode::AskEveryStep,
            reply.proposed_actions[0].clone(),
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
            None,
        )
        .expect("resume helper should dispatch approved action");

        assert_eq!(resumed_action.execution_state, "succeeded");
        assert!(resumed_action.capability_invocation_id.is_some());
        assert!(resumed_action
            .dispatch_note
            .as_deref()
            .unwrap_or_default()
            .contains("Resumed evidence"));
    }

    #[test]
    fn agent_chat_dispatches_allowed_file_read_action_and_records_invocation() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_path = temp_dir.path().join("source.md");
        std::fs::write(&source_path, "Alpha evidence\nBeta evidence").expect("write source");
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会先读取这个文件。",
            "agent_actions": [
                {
                    "action_type": "file_read",
                    "title": "读取材料",
                    "reason": "用户要求分析本地材料。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": source_path.to_string_lossy()
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "读取这个文件。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should return action proposal");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::AskOnRisk,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("allowed file read dispatches");

        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(
            invocations[0].capability,
            crate::kernel::policy::CapabilityKind::FileRead
        );
        assert_eq!(
            invocations[0].status,
            crate::kernel::capability::CapabilityInvocationStatus::Succeeded
        );

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["execution_state"],
            "succeeded"
        );
        assert_eq!(
            reply_json["proposed_actions"][0]["capability_invocation_id"],
            invocations[0].id.to_string()
        );
        assert!(reply_json["proposed_actions"][0]["dispatch_note"]
            .as_str()
            .unwrap_or_default()
            .contains("Alpha evidence"));
    }

    #[test]
    fn agent_chat_dispatches_directory_terminal_read_action_and_follows_up() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp_dir.path().join("alpha.txt"), "Alpha evidence")
            .expect("write source file");
        std::fs::create_dir(temp_dir.path().join("nested")).expect("create nested folder");
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我将列出目录内容。正在执行命令...",
            "agent_actions": [
                {
                    "action_type": "terminal_read",
                    "title": "List directory contents",
                    "reason": "用户提供了本地目录路径，要求查看目录里有什么。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": temp_dir.path().to_string_lossy()
                }
            ],
            "missing_prerequisites": []
        });
        let transport = SequencedDeepSeekTransport::new(vec![
            model_envelope.to_string(),
            "目录中包含 alpha.txt 和 nested。".to_string(),
        ]);
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "看一下这个目录里面有什么。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::FullAccess,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat should dispatch local directory listing");

        assert_eq!(response.content, "目录中包含 alpha.txt 和 nested。");
        assert_eq!(response.proposed_actions.len(), 1);
        assert_eq!(response.proposed_actions[0].execution_state, "succeeded");
        assert!(response.proposed_actions[0]
            .capability_invocation_id
            .is_some());
        let invocations = store
            .lock()
            .expect("store lock")
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].capability, CapabilityKind::TerminalRead);
        assert_eq!(invocations[0].status, CapabilityInvocationStatus::Succeeded);
        assert!(invocations[0]
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("alpha.txt"));
        let recorded_requests = transport.recorded_requests();
        assert_eq!(recorded_requests.len(), 2);
        let followup_user_message = recorded_requests[1]
            .messages
            .iter()
            .find(|message| {
                matches!(
                    message.role,
                    crate::kernel::deepseek::DeepSeekChatRole::User
                )
            })
            .expect("follow-up user message should exist");
        assert!(followup_user_message.content.contains("alpha.txt"));
        assert!(followup_user_message.content.contains("nested"));
    }

    #[test]
    fn agent_chat_resolves_desktop_terminal_read_target_to_safe_directory_listing() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let desktop_dir = temp_dir.path().join("Desktop");
        std::fs::create_dir(&desktop_dir).expect("create fake desktop");
        std::fs::write(
            desktop_dir.join("关于开展安保人员及安全管理人员普查工作的通知（无附件新版）.docx"),
            "word placeholder",
        )
        .expect("write desktop file");
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我先列出桌面文件确认目标文档。",
            "agent_actions": [
                {
                    "action_type": "terminal_read",
                    "title": "列出桌面文件，确认目标文档",
                    "reason": "用户要求处理刚生成在桌面的 Word 文档。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "desktop"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = SequencedDeepSeekTransport::new(vec![
            model_envelope.to_string(),
            "桌面中包含目标 Word 文档。".to_string(),
        ]);
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "列出桌面文件，确认目标文档。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::FullAccess,
            },
            AgentChatRuntimeContext {
                desktop_dir: Some(desktop_dir.clone()),
                ..AgentChatRuntimeContext::default()
            },
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat should resolve desktop target to directory listing");

        assert_eq!(response.content, "桌面中包含目标 Word 文档。");
        assert_eq!(response.proposed_actions.len(), 1);
        assert_eq!(response.proposed_actions[0].execution_state, "succeeded");
        let invocations = store
            .lock()
            .expect("store lock")
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(invocations[0].capability, CapabilityKind::TerminalRead);
        assert_eq!(invocations[0].status, CapabilityInvocationStatus::Succeeded);
        assert!(!invocations[0]
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("terminal command is not in the TerminalRead allowlist"));
        assert!(invocations[0]
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("关于开展安保人员及安全管理人员普查工作的通知"));
        let recorded_requests = transport.recorded_requests();
        assert_eq!(recorded_requests.len(), 2);
        let followup_user_message = recorded_requests[1]
            .messages
            .iter()
            .find(|message| {
                matches!(
                    message.role,
                    crate::kernel::deepseek::DeepSeekChatRole::User
                )
            })
            .expect("follow-up user message should exist");
        assert!(followup_user_message
            .content
            .contains("关于开展安保人员及安全管理人员普查工作的通知"));
    }

    #[test]
    fn agent_chat_dispatches_windows_file_mutation_actions() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_path = temp_dir.path().join("draft.txt");
        let renamed_path = temp_dir.path().join("final.txt");
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会创建、修改、重命名并删除这个本地文件。",
            "agent_actions": [
                {
                    "action_type": "file_create",
                    "title": "Create file",
                    "target": source_path.to_string_lossy(),
                    "content": "first draft"
                },
                {
                    "action_type": "file_update",
                    "title": "Update file",
                    "target": source_path.to_string_lossy(),
                    "content": "second draft"
                },
                {
                    "action_type": "file_rename",
                    "title": "Rename file",
                    "target": source_path.to_string_lossy(),
                    "destination": renamed_path.to_string_lossy()
                },
                {
                    "action_type": "file_delete",
                    "title": "Delete file",
                    "target": renamed_path.to_string_lossy()
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "在 Windows 本地路径上测试文件创建、修改、重命名和删除。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::FullAccess,
            },
            None,
        )
        .expect("agent chat should return file mutation proposals");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::FullAccess,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("file mutation actions dispatch");

        assert_eq!(reply.proposed_actions.len(), 4);
        assert!(reply
            .proposed_actions
            .iter()
            .all(|action| action.execution_state == "succeeded"));
        assert!(!source_path.exists());
        assert!(!renamed_path.exists());
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 4);
        assert!(invocations.iter().all(|invocation| {
            invocation.capability == CapabilityKind::FileWrite
                && invocation.status == CapabilityInvocationStatus::Succeeded
        }));
    }

    #[test]
    fn agent_chat_dispatches_windows_directory_mutation_actions() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_dir = temp_dir.path().join("incoming");
        let renamed_dir = temp_dir.path().join("processed");
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会创建、重命名并删除这个本地目录。",
            "agent_actions": [
                {
                    "action_type": "directory_create",
                    "title": "Create directory",
                    "target": source_dir.to_string_lossy()
                },
                {
                    "action_type": "directory_rename",
                    "title": "Rename directory",
                    "target": source_dir.to_string_lossy(),
                    "destination": renamed_dir.to_string_lossy()
                },
                {
                    "action_type": "directory_delete",
                    "title": "Delete directory",
                    "target": renamed_dir.to_string_lossy()
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "在 Windows 本地路径上测试目录创建、重命名和删除。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::FullAccess,
            },
            None,
        )
        .expect("agent chat should return directory mutation proposals");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::FullAccess,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("directory mutation actions dispatch");

        assert_eq!(reply.proposed_actions.len(), 3);
        assert!(reply
            .proposed_actions
            .iter()
            .all(|action| action.execution_state == "succeeded"));
        assert!(!source_dir.exists());
        assert!(!renamed_dir.exists());
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 3);
        assert!(invocations.iter().all(|invocation| {
            invocation.capability == CapabilityKind::FileWrite
                && invocation.status == CapabilityInvocationStatus::Succeeded
        }));
    }

    #[test]
    fn agent_chat_dispatches_allowed_network_search_action_and_records_invocation() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会先检索来源链接，再基于证据回答。",
            "agent_actions": [
                {
                    "action_type": "network_search",
                    "title": "检索公开网页",
                    "reason": "用户要求当前网页信息。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "DeepSeek Agent OS latest"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "查一下 DeepSeek Agent OS 最新信息。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should return network search proposal");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::AskOnRisk,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("allowed network search dispatches");

        assert_eq!(
            search_client.recorded_calls(),
            vec![(
                "DeepSeek Agent OS latest".to_string(),
                "public web".to_string()
            )]
        );
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(
            invocations[0].capability,
            crate::kernel::policy::CapabilityKind::NetworkSearch
        );
        assert_eq!(
            invocations[0].status,
            crate::kernel::capability::CapabilityInvocationStatus::Succeeded
        );

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["execution_state"],
            "succeeded"
        );
        assert_eq!(
            reply_json["proposed_actions"][0]["capability_invocation_id"],
            invocations[0].id.to_string()
        );
        assert!(reply_json["proposed_actions"][0]["dispatch_note"]
            .as_str()
            .unwrap_or_default()
            .contains("durable URL evidence"));
    }

    #[test]
    fn agent_chat_dispatches_allowed_browser_browse_action_and_records_invocation() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会先打开网页读取页面内容。",
            "agent_actions": [
                {
                    "action_type": "browser_browse",
                    "title": "读取网页",
                    "reason": "用户提供了一个需要打开查看的网页。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "https://example.com/ds-agent"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "打开 https://example.com/ds-agent 看一下。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::LimitedAuto,
            },
            None,
        )
        .expect("agent chat should return browser browse proposal");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::LimitedAuto,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("allowed browser browse dispatches");

        assert_eq!(
            browser_client.recorded_calls(),
            vec!["https://example.com/ds-agent".to_string()]
        );
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(
            invocations[0].capability,
            crate::kernel::policy::CapabilityKind::BrowserBrowse
        );
        assert_eq!(
            invocations[0].status,
            crate::kernel::capability::CapabilityInvocationStatus::Succeeded
        );

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["execution_state"],
            "succeeded"
        );
        assert_eq!(
            reply_json["proposed_actions"][0]["capability_invocation_id"],
            invocations[0].id.to_string()
        );
        assert!(reply_json["proposed_actions"][0]["dispatch_note"]
            .as_str()
            .unwrap_or_default()
            .contains("Browser Evidence"));
    }

    #[test]
    fn agent_chat_browser_open_falls_back_to_default_browser_when_chrome_is_unavailable() {
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let opener = RecordingBrowserUrlOpener::new(BrowserUrlOpenOutcome {
            browser_label: "default browser".to_string(),
            fallback_note: Some(
                "未检测到 Chrome，已使用默认浏览器打开 https://github.com".to_string(),
            ),
        });
        let mut action = AgentChatActionProposal {
            action_type: "browser_open".to_string(),
            title: Some("用 Chrome 打开 GitHub 首页".to_string()),
            reason: Some("用户明确指定 Chrome。".to_string()),
            risk: Some("low".to_string()),
            requires_confirmation: false,
            target: Some("https://github.com".to_string()),
            target_location: None,
            destination: None,
            preferred_browser: Some("chrome".to_string()),
            content: None,
            capability: Some(CapabilityKind::BrowserBrowse),
            policy_decision: Some(PolicyDecision::Allow),
            execution_state: "proposed".to_string(),
            dispatch_note: None,
            permission_request_id: None,
            capability_invocation_id: None,
            workflow_run_id: None,
            blocked_reason: None,
        };

        dispatch_agent_browser_open_action_with_opener(
            &store,
            AccessMode::AskOnRisk,
            &mut action,
            false,
            None,
            &opener,
        )
        .expect("browser_open fallback should dispatch");

        assert_eq!(
            opener.recorded_calls(),
            vec![("https://github.com".to_string(), Some("chrome".to_string()))]
        );
        assert_eq!(action.execution_state, "succeeded");
        assert!(action
            .dispatch_note
            .as_deref()
            .unwrap_or_default()
            .contains("未检测到 Chrome"));
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(
            invocations[0].status,
            crate::kernel::capability::CapabilityInvocationStatus::Succeeded
        );
        assert!(invocations[0]
            .warnings
            .iter()
            .any(|warning| warning.contains("默认浏览器")));
    }

    #[test]
    fn agent_chat_dispatches_allowed_file_write_action_and_records_invocation() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会把结果写入工作目录。",
            "agent_actions": [
                {
                    "action_type": "file_write",
                    "title": "写入报告",
                    "reason": "保存 DeepSeek 生成的摘要报告。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "reports/summary.md",
                    "content": "# Summary\nDS Agent writes only after local validation.\n"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "生成摘要并保存到 reports/summary.md。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::FullAccess,
            },
            None,
        )
        .expect("agent chat should return file write proposal");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::FullAccess,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("allowed file write dispatches");

        assert_eq!(
            file_write_client.recorded_calls(),
            vec![(
                "reports/summary.md".to_string(),
                "# Summary\nDS Agent writes only after local validation.\n".to_string()
            )]
        );
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(
            invocations[0].capability,
            crate::kernel::policy::CapabilityKind::FileWrite
        );
        assert_eq!(
            invocations[0].status,
            crate::kernel::capability::CapabilityInvocationStatus::Succeeded
        );

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["execution_state"],
            "succeeded"
        );
        assert_eq!(
            reply_json["proposed_actions"][0]["capability_invocation_id"],
            invocations[0].id.to_string()
        );
        assert!(reply_json["proposed_actions"][0]["dispatch_note"]
            .as_str()
            .unwrap_or_default()
            .contains("bytes written"));
    }

    #[test]
    fn agent_chat_dispatches_allowed_operations_briefing_action_and_records_run() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp_dir.path().join("revenue.md"),
            "Room revenue improved by 6 percent.",
        )
        .expect("write revenue evidence");
        std::fs::write(
            temp_dir.path().join("complaints.txt"),
            "Guest complaints increased in the west wing.",
        )
        .expect("write complaints evidence");
        let evidence_folder_path = temp_dir.path().to_string_lossy().to_string();
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会把经营简报工作流作为一个本地工作包执行。",
            "agent_actions": [
                {
                    "action_type": "operations_briefing",
                    "title": "运行经营简报工作流",
                    "reason": "用户要求根据本地证据生成经营简报。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": evidence_folder_path
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "根据这个证据目录生成经营简报。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::FullAccess,
            },
            None,
        )
        .expect("agent chat should return operations briefing proposal");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::FullAccess,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("allowed operations briefing dispatches");

        let runs = store
            .list_operations_briefing_runs()
            .expect("operations briefing runs load");
        assert_eq!(runs.len(), 1);
        assert_eq!(
            runs[0].status,
            crate::kernel::workflow::OperationsBriefingRunStatus::DraftReady
        );
        assert!(runs[0]
            .summary
            .contains("Draft ready from evidence folder manifest"));

        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert_eq!(invocations.len(), 1);
        assert_eq!(
            invocations[0].capability,
            crate::kernel::policy::CapabilityKind::FileRead
        );

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["execution_state"],
            "succeeded"
        );
        assert_eq!(
            reply_json["proposed_actions"][0]["workflow_run_id"],
            runs[0].id.to_string()
        );
        assert!(reply_json["proposed_actions"][0]["dispatch_note"]
            .as_str()
            .unwrap_or_default()
            .contains("operations briefing draft ready"));
    }

    #[test]
    fn agent_chat_dispatches_operations_briefing_after_file_read_permission_is_approved() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp_dir.path().join("revenue.md"),
            "Room revenue improved by 6 percent.",
        )
        .expect("write revenue evidence");
        let evidence_folder_path = temp_dir.path().to_string_lossy().to_string();
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "经营简报工作流需要先获得读取证据目录的许可。",
            "workflow_calls": [
                {
                    "action_type": "operations_briefing",
                    "title": "运行经营简报工作流",
                    "reason": "用户要求根据本地证据生成经营简报。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": evidence_folder_path
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "根据证据目录生成经营简报。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskEveryStep,
            },
            None,
        )
        .expect("agent chat should return operations briefing proposal");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::AskEveryStep,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("first dispatch should request FileRead permission");

        let permission_request_id = reply.proposed_actions[0]
            .permission_request_id
            .expect("permission request id should be attached");
        assert_eq!(
            reply.proposed_actions[0].execution_state,
            "needs_confirmation"
        );
        store
            .resolve_capability_access_request(
                permission_request_id,
                true,
                "approved workflow evidence read".to_string(),
            )
            .expect("permission request resolves");

        dispatch_agent_action_proposals(
            &store,
            AccessMode::AskEveryStep,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("approved workflow action should dispatch");

        let runs = store
            .list_operations_briefing_runs()
            .expect("operations briefing runs load");
        assert_eq!(runs.len(), 2);
        assert_eq!(reply.proposed_actions[0].execution_state, "succeeded");
        let workflow_run_id = reply.proposed_actions[0]
            .workflow_run_id
            .expect("workflow run id should point at the executed run");
        let resumed_run = runs
            .iter()
            .find(|run| run.id == workflow_run_id)
            .expect("executed workflow run should be stored");
        assert_eq!(
            resumed_run.status,
            crate::kernel::workflow::OperationsBriefingRunStatus::DraftReady
        );
    }

    #[test]
    fn agent_chat_follows_up_with_deepseek_after_successful_tool_evidence() {
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会先检索来源链接，再基于证据回答。",
            "agent_actions": [
                {
                    "action_type": "network_search",
                    "title": "检索公开网页",
                    "reason": "用户要求当前网页信息。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "DeepSeek Agent OS latest"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = SequencedDeepSeekTransport::new(vec![
            model_envelope.to_string(),
            "根据检索证据，DS Agent 有来源链接可回看。".to_string(),
        ]);
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (reply, telemetry) = agent_chat_with_dispatch_and_tool_followup(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "查一下 DeepSeek Agent OS 最新信息。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            super::AgentChatRuntimeContext::default(),
            None,
            &store,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat should dispatch search and ask DeepSeek to synthesize");

        assert_eq!(telemetry.len(), 2);
        assert_eq!(reply.content, "根据检索证据，DS Agent 有来源链接可回看。");
        assert_eq!(reply.proposed_actions.len(), 1);
        assert_eq!(reply.proposed_actions[0].execution_state, "succeeded");
        assert!(reply.proposed_actions[0].capability_invocation_id.is_some());

        let recorded = transport.recorded_requests();
        assert_eq!(recorded.len(), 2);
        let followup_user_message = recorded[1]
            .messages
            .iter()
            .find(|message| {
                matches!(
                    message.role,
                    crate::kernel::deepseek::DeepSeekChatRole::User
                )
            })
            .expect("follow-up user message should exist");
        assert!(followup_user_message
            .content
            .contains("DS Agent completed local actions"));
        assert!(followup_user_message
            .content
            .contains("查一下 DeepSeek Agent OS 最新信息。"));
        assert!(followup_user_message
            .content
            .contains("https://example.com/ds-agent"));
        assert!(followup_user_message
            .content
            .contains("durable URL evidence"));
        assert!(followup_user_message.content.contains("done_when"));
        assert!(followup_user_message
            .content
            .contains("completion_verifier"));
        assert!(followup_user_message.content.contains("completion_advice"));
    }

    #[test]
    fn agent_chat_preserves_completed_actions_when_tool_followup_deepseek_read_fails() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会先检索来源链接，再基于证据回答。",
            "agent_actions": [
                {
                    "action_type": "network_search",
                    "title": "检索公开网页",
                    "reason": "用户要求当前网页信息。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "DeepSeek Agent OS latest"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = SequencedDeepSeekTransport::new_results(vec![
            Ok(model_envelope.to_string()),
            Err("deepseek chat response could not be read: operation timed out".to_string()),
        ]);
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "查一下 DeepSeek Agent OS 最新信息。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("completed local actions should be preserved when follow-up DeepSeek read fails");

        assert_eq!(response.proposed_actions.len(), 1);
        assert_eq!(response.proposed_actions[0].execution_state, "succeeded");
        assert!(response.content.contains("本地动作"));
        assert!(response.content.contains("DeepSeek"));
        assert_eq!(
            search_client.recorded_calls(),
            vec![(
                "DeepSeek Agent OS latest".to_string(),
                "public web".to_string()
            )]
        );
        let recorded_requests = transport.recorded_requests();
        assert_eq!(recorded_requests.len(), 2);
        let followup_user_message = recorded_requests[1]
            .messages
            .iter()
            .find(|message| {
                matches!(
                    message.role,
                    crate::kernel::deepseek::DeepSeekChatRole::User
                )
            })
            .expect("follow-up user message should exist");
        assert!(followup_user_message
            .content
            .contains("https://example.com/ds-agent"));
        assert!(followup_user_message
            .content
            .contains("durable URL evidence"));
        assert_eq!(
            store
                .lock()
                .expect("store lock")
                .list_deepseek_chat_telemetry()
                .expect("telemetry list")
                .len(),
            1
        );
    }

    #[test]
    fn agent_chat_skips_followup_for_deterministic_office_actions() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我会创建这个 Word 文档。",
            "agent_actions": [
                {
                    "action_type": "office_create",
                    "title": "创建 Word 文档",
                    "target": "desktop/deterministic.docx",
                    "target_location": "desktop",
                    "content": {
                        "app": "word",
                        "body": "确定性 Office 动作不需要二次模型总结。"
                    }
                }
            ],
            "missing_prerequisites": []
        });
        let transport = SequencedDeepSeekTransport::new(vec![
            model_envelope.to_string(),
            "不应调用第二次 DeepSeek。".to_string(),
        ]);
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "在桌面创建一个 Word 文档。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("deterministic office action should complete locally");

        assert_eq!(response.proposed_actions.len(), 1);
        assert_eq!(response.proposed_actions[0].execution_state, "succeeded");
        assert!(response.content.contains("DS Agent 已完成并验证本地动作"));
        assert!(response.content.contains("Word document created"));
        assert!(response
            .content
            .contains("验证：本地执行器返回成功状态和结果记录。"));
        assert!(response.content.contains("建议："));
        assert!(!response.content.contains("Loop context"));
        assert!(!response.content.contains("loop_mode="));
        assert!(!response.content.contains("matched_stop_conditions="));
        assert_eq!(transport.recorded_requests().len(), 1);
        assert_eq!(
            store
                .lock()
                .expect("store lock")
                .list_deepseek_chat_telemetry()
                .expect("telemetry list")
                .len(),
            1
        );
    }

    #[test]
    fn agent_chat_waits_for_missing_prerequisites_before_dispatching_actions() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let source_path = temp_dir.path().join("source.md");
        std::fs::write(&source_path, "Should not be read yet").expect("write source");
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope-v1",
            "reply_to_user": "我需要先知道工作目录，然后再读取文件。",
            "agent_actions": [
                {
                    "action_type": "file_read",
                    "title": "读取材料",
                    "reason": "用户要求分析本地材料。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": source_path.to_string_lossy()
                }
            ],
            "missing_prerequisites": [
                {
                    "kind": "workspace",
                    "message": "请先选择一个工作目录。"
                }
            ]
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let (mut reply, _) = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "读取这个文件。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect("agent chat should return prerequisite and proposal");
        dispatch_agent_action_proposals(
            &store,
            AccessMode::AskOnRisk,
            &mut reply,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("missing prerequisite should defer dispatch without error");

        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");
        assert!(
            invocations.is_empty(),
            "actions must not execute until missing prerequisites are resolved"
        );

        let reply_json = serde_json::to_value(&reply).expect("reply serializes");
        assert_eq!(
            reply_json["proposed_actions"][0]["execution_state"],
            "waiting_prerequisite"
        );
        assert!(reply_json["proposed_actions"][0]["dispatch_note"]
            .as_str()
            .unwrap_or_default()
            .contains("missing prerequisites"));
    }

    #[test]
    fn agent_chat_waits_for_workspace_before_dispatching_artifact_actions() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope/v1",
            "reply_to_user": "我会生成报告，但需要先确认工作目录。",
            "agent_actions": [
                {
                    "action_type": "create_report",
                    "title": "生成报告",
                    "reason": "用户要求保存一份报告。",
                    "risk": "low",
                    "requires_confirmation": false,
                    "target": "reports/summary.md",
                    "content": "# Summary\n"
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "生成一份报告。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::FullAccess,
            },
            AgentChatRuntimeContext {
                workspace_ready: AgentChatReadiness::Missing,
                workspace_note: "local workspace needs setup".to_string(),
                network_search_ready: AgentChatReadiness::Ready,
                network_search_note: "network search ready".to_string(),
                network_search_source_model: None,
                soul_profile: None,
                memory_context: super::AgentMemoryRuntimeContext::default(),
                desktop_dir: None,
            },
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat should return workspace prerequisite without writing");

        assert!(
            file_write_client.recorded_calls().is_empty(),
            "artifact actions must not write before a user workspace is configured"
        );
        assert_eq!(response.missing_prerequisites.len(), 1);
        assert_eq!(response.missing_prerequisites[0].kind, "workspace");
        assert_eq!(
            response.proposed_actions[0].execution_state,
            "waiting_prerequisite"
        );
        assert!(response.proposed_actions[0]
            .dispatch_note
            .as_deref()
            .unwrap_or_default()
            .contains("workspace"));
    }

    #[test]
    fn agent_chat_records_model_memory_candidates_for_review_only() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope/v1",
            "reply_to_user": "我记录了一个候选记忆，等待你复核。",
            "agent_actions": [],
            "missing_prerequisites": [],
            "memory_candidates": [
                {
                    "title": "DS Agent boundary",
                    "body": "DS Agent should treat DeepSeek memory suggestions as reviewable candidates.",
                    "rationale": "The user explicitly defined the model-agent boundary.",
                    "memory_type": "project_context",
                    "scope": "workspace",
                    "sensitivity": "normal",
                    "lifecycle": "active"
                }
            ]
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "请记住这个项目边界。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat should queue memory candidates");

        assert_eq!(response.memory_candidates.len(), 1);
        assert_eq!(response.memory_candidates[0].title, "DS Agent boundary");
        assert_eq!(
            response.memory_candidates[0].source,
            MemoryCandidateSource::WorkflowReflection
        );

        let store = store.lock().expect("store lock");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        assert_eq!(candidate_records.len(), 1);
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert!(
            store
                .list_memory_records()
                .expect("memory records load")
                .is_empty(),
            "model memory candidates must not become long-term memory without review"
        );
    }

    #[test]
    fn agent_chat_gates_model_memory_candidates_to_three_review_items() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope/v1",
            "reply_to_user": "我会提出候选记忆，等待你复核。",
            "agent_actions": [],
            "missing_prerequisites": [],
            "memory_candidates": [
                {
                    "title": "Preferred brief tone",
                    "body": "Use concise operating language with owners and evidence.",
                    "rationale": "The user asked for this style repeatedly.",
                    "memory_type": "preference",
                    "scope": "user",
                    "sensitivity": "normal",
                    "lifecycle": "active"
                },
                {
                    "title": "DS Agent memory boundary",
                    "body": "DS Agent should propose memory candidates for review, not silently write them.",
                    "rationale": "The user made this a project boundary.",
                    "memory_type": "workflow_rule",
                    "scope": "project",
                    "sensitivity": "normal",
                    "lifecycle": "active"
                },
                {
                    "title": "Memory candidate review actions",
                    "body": "Memory candidates should support accept, merge, replace, link, and reject.",
                    "rationale": "The user asked for a Codex-like memory review loop.",
                    "memory_type": "workflow_rule",
                    "scope": "project",
                    "sensitivity": "normal",
                    "lifecycle": "active"
                },
                {
                    "title": "Memory runtime speed boundary",
                    "body": "Memory retrieval and candidate generation should stay bounded and avoid slow endless optimization.",
                    "rationale": "The user explicitly warned against memory work slowing the agent.",
                    "memory_type": "workflow_rule",
                    "scope": "project",
                    "sensitivity": "normal",
                    "lifecycle": "active"
                },
                {
                    "title": "Memory receipt preference",
                    "body": "Show which memories were used and why in a reviewable receipt.",
                    "rationale": "The user wants auditability for memory use.",
                    "memory_type": "preference",
                    "scope": "user",
                    "sensitivity": "normal",
                    "lifecycle": "active"
                }
            ]
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "按我的偏好优化记忆系统。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat should gate memory candidates");

        assert_eq!(
            response.memory_candidates.len(),
            3,
            "candidate gate should keep at most three review items"
        );
        assert!(response
            .memory_candidates
            .iter()
            .all(|candidate| candidate.rationale.contains("Memory Candidate Gate")));

        let store = store.lock().expect("store lock");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        assert_eq!(candidate_records.len(), 3);
    }

    #[test]
    fn agent_chat_memory_candidate_gate_filters_sensitive_and_transient_items() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope/v1",
            "reply_to_user": "我会只保留适合复核的候选记忆。",
            "agent_actions": [],
            "missing_prerequisites": [],
            "memory_candidates": [
                {
                    "title": "Temporary draft preference",
                    "body": "Only use this wording for today's one-off draft.",
                    "rationale": "The user asked for it in this task.",
                    "memory_type": "preference",
                    "scope": "user",
                    "sensitivity": "normal",
                    "lifecycle": "active"
                },
                {
                    "title": "Private password",
                    "body": "The user's password is hunter2.",
                    "rationale": "The user pasted a password.",
                    "memory_type": "preference",
                    "scope": "user",
                    "sensitivity": "sensitive",
                    "lifecycle": "active"
                },
                {
                    "title": "Default response tone",
                    "body": "Use concise, warm, direct Chinese unless the user asks otherwise.",
                    "rationale": "The user wants personalized response tone memory.",
                    "memory_type": "preference",
                    "scope": "user",
                    "sensitivity": "normal",
                    "lifecycle": "active"
                }
            ]
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "记住我的默认回复语气，但不要保存临时或敏感内容。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat should filter unsafe memory candidates");

        assert_eq!(response.memory_candidates.len(), 1);
        assert_eq!(response.memory_candidates[0].title, "Default response tone");
        assert!(response.memory_candidates[0]
            .rationale
            .contains("privacy_review=normal"));

        let store = store.lock().expect("store lock");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        assert_eq!(candidate_records.len(), 1);
        assert!(store
            .list_memory_records()
            .expect("memory records load")
            .is_empty());
    }

    #[test]
    fn agent_chat_treats_memory_candidate_actions_as_review_candidates() {
        let store = Mutex::new(EventStore::open_memory().expect("memory store opens"));
        let model_envelope = serde_json::json!({
            "protocol_version": "ds-agent-envelope/v1",
            "reply_to_user": "我把这条信息放入候选记忆复核队列。",
            "agent_actions": [
                {
                    "action_type": "memory_candidate",
                    "title": "Agent memory boundary",
                    "reason": "用户要求 DS Agent 不要自动写长期记忆。",
                    "content": "DeepSeek may propose memory, but DS Agent must queue it for review."
                }
            ],
            "missing_prerequisites": []
        });
        let transport = RecordingDeepSeekTransport::new(model_envelope.to_string());
        let cache = DeepSeekMemoryChatCompletionCache::default();
        let file_client = LocalFileContentClient::new(512 * 1024);
        let file_write_client = RecordingFileWriteClient::new();
        let search_client = RecordingNetworkSearchClient::new();
        let browser_client = RecordingBrowserPageClient::new();

        let response = run_agent_chat_with_clients(
            &store,
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "这条边界以后要记住。".to_string(),
                model_route: ModelRoute::Flash,
                thinking_level: ThinkingLevel::Fast,
                access_mode: AccessMode::AskOnRisk,
            },
            AgentChatRuntimeContext::default(),
            None,
            &file_client,
            &file_write_client,
            &search_client,
            &browser_client,
        )
        .expect("agent chat should convert memory_candidate actions into review candidates");

        assert!(
            response.proposed_actions.is_empty(),
            "memory_candidate should not remain as an executable action"
        );
        assert_eq!(response.memory_candidates.len(), 1);
        assert_eq!(
            response.memory_candidates[0].body,
            "DeepSeek may propose memory, but DS Agent must queue it for review."
        );

        let store = store.lock().expect("store lock");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        assert_eq!(candidate_records.len(), 1);
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert!(
            store
                .list_memory_records()
                .expect("memory records load")
                .is_empty(),
            "memory_candidate actions must not bypass review"
        );
    }

    #[test]
    fn agent_chat_rejects_blank_prompt_before_calling_transport() {
        let transport = RecordingDeepSeekTransport::new("unused");
        let cache = DeepSeekMemoryChatCompletionCache::default();

        let error = agent_chat_with_transport(
            &transport,
            &cache,
            "test-secret",
            AgentChatRequest {
                prompt: "   ".to_string(),
                model_route: ModelRoute::Auto,
                thinking_level: ThinkingLevel::Auto,
                access_mode: AccessMode::AskOnRisk,
            },
            None,
        )
        .expect_err("blank prompt should be rejected");

        assert!(error.contains("message is required"));
        assert!(transport.recorded_requests().is_empty());
    }

    #[test]
    fn agent_chat_api_key_prefers_session_override_without_persisting_secret() {
        let api_key = agent_chat_api_key_from_sources(Some("  session-key  ".to_string()), |_| {
            Some("env-key".to_string())
        })
        .expect("session key should be accepted");

        assert_eq!(api_key, "session-key");
    }

    #[test]
    fn agent_chat_api_key_falls_back_to_environment_when_override_blank() {
        let api_key = agent_chat_api_key_from_sources(Some("  ".to_string()), |_| {
            Some(" env-key ".to_string())
        })
        .expect("env key should be accepted");

        assert_eq!(api_key, "env-key");
    }

    #[test]
    fn agent_chat_api_key_candidates_keep_primary_fallback_then_environment() {
        let api_keys = agent_chat_api_key_candidates_from_sources(
            Some(" primary-key ".to_string()),
            Some(" fallback-key ".to_string()),
            |name| (name == DEEPSEEK_API_KEY_ENV).then_some(" env-key ".to_string()),
        );

        assert_eq!(api_keys, vec!["primary-key", "fallback-key", "env-key"]);
    }

    #[test]
    fn agent_chat_api_key_candidates_drop_blanks_and_duplicates() {
        let api_keys = agent_chat_api_key_candidates_from_sources(
            Some(" same-key ".to_string()),
            Some("same-key".to_string()),
            |name| (name == DEEPSEEK_API_KEY_ENV).then_some("  ".to_string()),
        );

        assert_eq!(api_keys, vec!["same-key"]);
    }

    #[test]
    fn computer_screenshot_evidence_base_prefers_user_evidence_dir() {
        let app_data_dir = std::path::PathBuf::from("fixtures/app-data");
        let state = LocalDirectoryState {
            app_data_dir: app_data_dir.to_string_lossy().to_string(),
            settings_file: app_data_dir
                .join(LOCAL_DIRECTORY_SETTINGS_FILE)
                .to_string_lossy()
                .to_string(),
            settings: Some(
                LocalDirectorySettings::new(
                    "fixtures/workspace".to_string(),
                    "fixtures/evidence".to_string(),
                    "fixtures/exports".to_string(),
                )
                .expect("settings validate"),
            ),
            needs_setup: false,
        };

        assert_eq!(
            computer_screenshot_evidence_base_dir(&app_data_dir, &state),
            std::path::PathBuf::from("fixtures/evidence")
        );
    }

    #[test]
    fn computer_screenshot_evidence_base_falls_back_to_app_data_before_setup() {
        let app_data_dir = std::path::PathBuf::from("fixtures/app-data");
        let state = LocalDirectoryState {
            app_data_dir: app_data_dir.to_string_lossy().to_string(),
            settings_file: app_data_dir
                .join(LOCAL_DIRECTORY_SETTINGS_FILE)
                .to_string_lossy()
                .to_string(),
            settings: None,
            needs_setup: true,
        };

        assert_eq!(
            computer_screenshot_evidence_base_dir(&app_data_dir, &state),
            app_data_dir
        );
    }

    #[test]
    fn operations_briefing_report_export_dir_prefers_user_export_dir() {
        let app_data_dir = std::path::PathBuf::from("fixtures/app-data");
        let state = LocalDirectoryState {
            app_data_dir: app_data_dir.to_string_lossy().to_string(),
            settings_file: app_data_dir
                .join(LOCAL_DIRECTORY_SETTINGS_FILE)
                .to_string_lossy()
                .to_string(),
            settings: Some(
                LocalDirectorySettings::new(
                    "fixtures/workspace".to_string(),
                    "fixtures/evidence".to_string(),
                    "fixtures/exports".to_string(),
                )
                .expect("settings validate"),
            ),
            needs_setup: false,
        };

        assert_eq!(
            operations_briefing_report_export_dir(&app_data_dir, &state),
            std::path::PathBuf::from("fixtures/exports")
        );
    }

    #[test]
    fn operations_briefing_report_export_dir_falls_back_to_app_data_before_setup() {
        let app_data_dir = std::path::PathBuf::from("fixtures/app-data");
        let state = LocalDirectoryState {
            app_data_dir: app_data_dir.to_string_lossy().to_string(),
            settings_file: app_data_dir
                .join(LOCAL_DIRECTORY_SETTINGS_FILE)
                .to_string_lossy()
                .to_string(),
            settings: None,
            needs_setup: true,
        };

        assert_eq!(
            operations_briefing_report_export_dir(&app_data_dir, &state),
            app_data_dir
        );
    }

    #[test]
    fn operations_briefing_template_seed_dir_prefers_user_evidence_dir() {
        let app_data_dir = std::path::PathBuf::from("fixtures/app-data");
        let state = LocalDirectoryState {
            app_data_dir: app_data_dir.to_string_lossy().to_string(),
            settings_file: app_data_dir
                .join(LOCAL_DIRECTORY_SETTINGS_FILE)
                .to_string_lossy()
                .to_string(),
            settings: Some(
                LocalDirectorySettings::new(
                    "fixtures/workspace".to_string(),
                    "fixtures/evidence".to_string(),
                    "fixtures/exports".to_string(),
                )
                .expect("settings validate"),
            ),
            needs_setup: false,
        };

        assert_eq!(
            operations_briefing_template_seed_dir(&app_data_dir, &state),
            std::path::PathBuf::from("fixtures/evidence")
        );
    }

    #[test]
    fn operations_briefing_template_seed_dir_falls_back_to_app_data_subdir_before_setup() {
        let app_data_dir = std::path::PathBuf::from("fixtures/app-data");
        let state = LocalDirectoryState {
            app_data_dir: app_data_dir.to_string_lossy().to_string(),
            settings_file: app_data_dir
                .join(LOCAL_DIRECTORY_SETTINGS_FILE)
                .to_string_lossy()
                .to_string(),
            settings: None,
            needs_setup: true,
        };

        assert_eq!(
            operations_briefing_template_seed_dir(&app_data_dir, &state),
            app_data_dir.join("operations-briefing-evidence")
        );
    }

    #[test]
    fn computer_control_unlock_state_starts_locked_with_local_challenge() {
        let now = Utc.with_ymd_and_hms(2026, 6, 29, 12, 0, 0).unwrap();
        let state = ComputerControlUnlockState::new("ABC123".to_string());

        let status = state.status(now);

        assert_eq!(status.challenge, "ABC123");
        assert!(!status.unlocked);
        assert_eq!(status.unlocked_until, None);
    }

    #[test]
    fn computer_control_unlock_rejects_wrong_token_without_unlocking() {
        let now = Utc.with_ymd_and_hms(2026, 6, 29, 12, 0, 0).unwrap();
        let mut state = ComputerControlUnlockState::new("ABC123".to_string());

        let error = state
            .unlock("WRONG", now)
            .expect_err("wrong token should not unlock control");

        assert!(error.contains("invalid computer control unlock token"));
        assert!(!state.status(now).unlocked);
    }

    #[test]
    fn computer_control_unlock_accepts_token_for_limited_window_and_expires() {
        let now = Utc.with_ymd_and_hms(2026, 6, 29, 12, 0, 0).unwrap();
        let mut state = ComputerControlUnlockState::new("ABC123".to_string());

        let status = state
            .unlock(" abc123 ", now)
            .expect("matching token unlocks control");

        assert!(status.unlocked);
        assert_eq!(
            status.unlocked_until,
            Some(now + Duration::minutes(COMPUTER_CONTROL_UNLOCK_TTL_MINUTES))
        );
        assert!(state.is_unlocked(now + Duration::minutes(COMPUTER_CONTROL_UNLOCK_TTL_MINUTES - 1)));
        assert!(
            !state.is_unlocked(now + Duration::minutes(COMPUTER_CONTROL_UNLOCK_TTL_MINUTES + 1))
        );
    }

    #[test]
    fn computer_control_execution_requires_unlock_only_after_approval() {
        assert!(!should_require_computer_control_unlock(false));
        assert!(should_require_computer_control_unlock(true));
    }

    #[test]
    fn link_existing_memory_records_returns_projected_relation_notes() {
        let store =
            crate::kernel::event_store::EventStore::open_memory().expect("memory store opens");
        let briefing_task = TaskRecord::new(
            "Briefing tone rule".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("briefing task is valid");
        let source_trace_task = TaskRecord::new(
            "Source trace rule".to_string(),
            "Keep evidence links visible.".to_string(),
        )
        .expect("source trace task is valid");
        let briefing_memory = MemoryRecord::from_task_record(&briefing_task);
        let source_trace_memory = MemoryRecord::from_task_record(&source_trace_task);
        store
            .append_memory_record(&briefing_memory)
            .expect("briefing memory appends");
        store
            .append_memory_record(&source_trace_memory)
            .expect("source trace memory appends");

        let memories = link_existing_memory_records(
            &store,
            briefing_memory.id,
            source_trace_memory.id,
            MemoryRelationKind::Derives,
            "Briefing tone derives from source traceability.".to_string(),
        )
        .expect("existing memories link");

        let projected_briefing = memories
            .iter()
            .find(|memory| memory.id == briefing_memory.id)
            .expect("briefing memory remains visible");
        let projected_source_trace = memories
            .iter()
            .find(|memory| memory.id == source_trace_memory.id)
            .expect("source trace memory remains visible");

        assert_eq!(projected_briefing.linked_memories.len(), 1);
        assert_eq!(
            projected_briefing.linked_memories[0].id,
            source_trace_memory.id
        );
        assert_eq!(
            projected_briefing.linked_memories[0].relation,
            MemoryRelationKind::Derives
        );
        assert_eq!(
            projected_briefing.linked_memories[0].note,
            "Briefing tone derives from source traceability."
        );
        assert_eq!(projected_source_trace.linked_memories.len(), 1);
        assert_eq!(
            projected_source_trace.linked_memories[0].id,
            briefing_memory.id
        );
    }

    #[test]
    fn work_package_exports_only_pending_memory_candidates() {
        let pending_candidate = MemoryCandidate::new(
            "Imported briefing preference".to_string(),
            "Review this candidate before writing it as local memory.".to_string(),
            MemoryCandidateSource::Import,
            None,
            "Imported package candidate.".to_string(),
        )
        .expect("pending candidate is valid");
        let accepted_candidate = MemoryCandidate::new(
            "Already accepted preference".to_string(),
            "This candidate has already been resolved locally.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "Local reviewer accepted this candidate.".to_string(),
        )
        .expect("accepted candidate is valid");
        let rejected_candidate = MemoryCandidate::new(
            "Rejected preference".to_string(),
            "This candidate has already been rejected locally.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "Local reviewer rejected this candidate.".to_string(),
        )
        .expect("rejected candidate is valid");

        let candidates = super::pending_memory_candidates_for_work_package(vec![
            MemoryCandidateRecord {
                candidate: pending_candidate.clone(),
                resolution: None,
                effective_status: MemoryCandidateStatus::Pending,
                conflicting_memory_ids: Vec::new(),
                conflicting_memories: Vec::new(),
            },
            MemoryCandidateRecord {
                candidate: accepted_candidate.clone(),
                resolution: Some(MemoryCandidateResolution::new(
                    accepted_candidate.id,
                    true,
                    "Accepted locally.".to_string(),
                )),
                effective_status: MemoryCandidateStatus::Accepted,
                conflicting_memory_ids: Vec::new(),
                conflicting_memories: Vec::new(),
            },
            MemoryCandidateRecord {
                candidate: rejected_candidate.clone(),
                resolution: Some(MemoryCandidateResolution::new(
                    rejected_candidate.id,
                    false,
                    "Rejected locally.".to_string(),
                )),
                effective_status: MemoryCandidateStatus::Rejected,
                conflicting_memory_ids: Vec::new(),
                conflicting_memories: Vec::new(),
            },
        ]);

        assert_eq!(candidates, vec![pending_candidate]);
    }

    #[test]
    fn computer_tool_strategy_for_command_routes_chatgpt_to_codex_bridge() {
        let strategy = computer_tool_strategy_for_command(LargeModelProvider::ChatGpt, None);

        assert_eq!(
            strategy.computer_screenshot_backend,
            ComputerScreenshotBackend::CodexBridgeScreenCapture
        );
        assert_eq!(
            strategy.computer_control_backend,
            ComputerControlBackend::CodexBridgeInputControl
        );
    }

    #[test]
    fn computer_tool_strategy_for_command_routes_deepseek_to_local_backend() {
        let strategy = computer_tool_strategy_for_command(
            LargeModelProvider::DeepSeek,
            Some(NetworkSearchSourceModel::FreeWebSource),
        );

        if cfg!(target_os = "macos") {
            assert_eq!(
                strategy.computer_screenshot_backend,
                ComputerScreenshotBackend::LocalMacosScreenCapture
            );
            assert_eq!(
                strategy.computer_control_backend,
                ComputerControlBackend::LocalMacosInputControl
            );
        } else {
            assert_eq!(
                strategy.computer_screenshot_backend,
                ComputerScreenshotBackend::LocalWindowsScreenCapture
            );
            assert_eq!(
                strategy.computer_control_backend,
                ComputerControlBackend::LocalWindowsInputControl
            );
        }
    }

    #[test]
    fn operations_briefing_uses_deepseek_key_only_for_deepseek_provider() {
        let key = operations_briefing_deepseek_api_key_for_provider(
            |name| (name == "DEEPSEEK_API_KEY").then_some("test-secret-token".to_string()),
            LargeModelProvider::DeepSeek,
        );
        let chatgpt_key = operations_briefing_deepseek_api_key_for_provider(
            |name| (name == "DEEPSEEK_API_KEY").then_some("test-secret-token".to_string()),
            LargeModelProvider::ChatGpt,
        );

        assert_eq!(key.as_deref(), Some("test-secret-token"));
        assert_eq!(chatgpt_key, None);
    }

    #[test]
    fn operations_briefing_ignores_blank_deepseek_key() {
        let key = operations_briefing_deepseek_api_key_for_provider(
            |name| (name == "DEEPSEEK_API_KEY").then_some("  ".to_string()),
            LargeModelProvider::DeepSeek,
        );

        assert_eq!(key, None);
    }

    #[test]
    fn deepseek_telemetry_with_pricing_sets_cost_estimate() {
        let telemetry = DeepSeekChatTelemetry {
            id: Uuid::new_v4(),
            request_hash: "abc123".to_string(),
            model: DEEPSEEK_FLASH_MODEL.to_string(),
            cache_status: DeepSeekChatCacheStatus::Miss,
            elapsed_ms: 25,
            prompt_tokens: Some(1_000_000),
            completion_tokens: Some(500_000),
            total_tokens: Some(1_500_000),
            estimated_cost_micro_usd: None,
            created_at: Utc::now(),
        };
        let settings = DeepSeekPricingSettings {
            enabled: true,
            flash_prompt_usd_per_million_tokens: "0.14".to_string(),
            flash_completion_usd_per_million_tokens: "0.28".to_string(),
            ..DeepSeekPricingSettings::default()
        };

        let entries = deepseek_telemetry_with_pricing(vec![telemetry], Some(&settings));

        assert_eq!(entries[0].estimated_cost_micro_usd, Some(280_000));
    }

    #[test]
    fn operations_briefing_context_helpers_record_route_thinking_and_cache() {
        let telemetry = DeepSeekChatTelemetry {
            id: Uuid::new_v4(),
            request_hash: "abc123".to_string(),
            model: DEEPSEEK_FLASH_MODEL.to_string(),
            cache_status: DeepSeekChatCacheStatus::Hit,
            elapsed_ms: 25,
            prompt_tokens: Some(10),
            completion_tokens: Some(5),
            total_tokens: Some(15),
            estimated_cost_micro_usd: None,
            created_at: Utc::now(),
        };

        assert_eq!(
            operations_briefing_model_route_context(LargeModelProvider::DeepSeek, ModelRoute::Pro),
            "deepseek / pro"
        );
        assert_eq!(thinking_level_context_label(ThinkingLevel::Deep), "deep");
        assert_eq!(
            operations_briefing_token_cache_context(&[telemetry]),
            format!("{DEEPSEEK_FLASH_MODEL} cache hit, 15 total tokens, 25 ms")
        );
        assert_eq!(
            operations_briefing_token_cache_context(&[]),
            "no DeepSeek request recorded"
        );
    }
}
