#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRoute {
    Auto,
    Flash,
    Pro,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LargeModelProvider {
    #[serde(rename = "deepseek")]
    DeepSeek,
    #[serde(rename = "chatgpt")]
    ChatGpt,
    Codex,
    Custom,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevel {
    Auto,
    Fast,
    Standard,
    Deep,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    AskEveryStep,
    AskOnRisk,
    LimitedAuto,
    FullAccess,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceScope {
    Workspace,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskRecordStatus {
    Active,
    Done,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskRecord {
    pub id: Uuid,
    pub title: String,
    pub summary: String,
    pub status: TaskRecordStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TaskRecord {
    pub fn new(title: String, summary: String) -> Result<Self, String> {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err("task title is required".to_string());
        }

        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            title,
            summary: summary.trim().to_string(),
            status: TaskRecordStatus::Active,
            created_at: now,
            updated_at: now,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRecordSource {
    TaskRecord,
    MemoryCandidate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateSource {
    Manual,
    TaskRecord,
    Import,
    WorkflowReflection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateStatus {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySelectedFeedbackKind {
    Useful,
    Irrelevant,
    Stale,
    Conflicting,
    ShouldUpdate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCandidateSuggestedAction {
    New,
    Update,
    Merge,
    Replace,
    Archive,
    Link,
    RejectHint,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    Preference,
    ProjectContext,
    WorkflowRule,
    Artifact,
    FailurePattern,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    Workspace,
    Project,
    Organization,
    User,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySensitivity {
    Normal,
    Sensitive,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLifecycle {
    Active,
    Archived,
    Expires,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRelationKind {
    Related,
    Updates,
    Extends,
    Derives,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySearchMatchSource {
    Direct,
    LinkedMemoryTitle,
    LinkedMemoryBody,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::enum_variant_names,
    reason = "the Free-prefixed names are the public Rust and snake_case Tauri and TypeScript settings contract; remove only with a versioned cross-language compatibility migration"
)]
pub enum NetworkSearchSourceModel {
    FreeWebSource,
    FreeLocalBrowser,
    FreeSourceAggregator,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkSearchBackend {
    #[serde(rename = "deepseek")]
    DeepSeek,
    NativeLargeModel,
    SourceBackedModel,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailBackend {
    ArchitectureOnly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveBackend {
    LocalFolderExportPackage,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::enum_variant_names,
    reason = "the ScreenCapture-suffixed names are the public Rust and snake_case Tauri and TypeScript settings contract; remove only with a versioned cross-language compatibility migration"
)]
pub enum ComputerScreenshotBackend {
    CodexStyleScreenCapture,
    CodexBridgeScreenCapture,
    LocalWindowsScreenCapture,
    LocalMacosScreenCapture,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::enum_variant_names,
    reason = "the InputControl-suffixed names are the public Rust and snake_case Tauri and TypeScript settings contract; remove only with a versioned cross-language compatibility migration"
)]
pub enum ComputerControlBackend {
    CodexStyleInputControl,
    CodexBridgeInputControl,
    LocalWindowsInputControl,
    LocalMacosInputControl,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolBackendSettings {
    pub network_search: NetworkSearchBackend,
    pub email: EmailBackend,
    pub drive: DriveBackend,
    pub computer_screenshot: ComputerScreenshotBackend,
    pub computer_control: ComputerControlBackend,
}

impl Default for ToolBackendSettings {
    fn default() -> Self {
        let computer_screenshot = if cfg!(target_os = "macos") {
            ComputerScreenshotBackend::LocalMacosScreenCapture
        } else {
            ComputerScreenshotBackend::LocalWindowsScreenCapture
        };
        let computer_control = if cfg!(target_os = "macos") {
            ComputerControlBackend::LocalMacosInputControl
        } else {
            ComputerControlBackend::LocalWindowsInputControl
        };

        Self {
            network_search: NetworkSearchBackend::SourceBackedModel,
            email: EmailBackend::ArchitectureOnly,
            drive: DriveBackend::LocalFolderExportPackage,
            computer_screenshot,
            computer_control,
        }
    }
}

fn default_memory_candidate_type() -> MemoryType {
    MemoryType::Preference
}

fn default_memory_record_type() -> MemoryType {
    MemoryType::ProjectContext
}

fn default_memory_scope() -> MemoryScope {
    MemoryScope::Workspace
}

fn default_memory_sensitivity() -> MemorySensitivity {
    MemorySensitivity::Normal
}

fn default_memory_lifecycle() -> MemoryLifecycle {
    MemoryLifecycle::Active
}

fn default_memory_candidate_suggested_action() -> MemoryCandidateSuggestedAction {
    MemoryCandidateSuggestedAction::New
}

fn default_memory_relation_kind() -> MemoryRelationKind {
    MemoryRelationKind::Related
}

fn default_memory_search_match() -> MemorySearchMatch {
    MemorySearchMatch::direct()
}

fn default_large_model_provider() -> LargeModelProvider {
    LargeModelProvider::DeepSeek
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryCandidate {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    #[serde(default = "default_memory_candidate_type")]
    pub memory_type: MemoryType,
    #[serde(default = "default_memory_scope")]
    pub scope: MemoryScope,
    #[serde(default = "default_memory_sensitivity")]
    pub sensitivity: MemorySensitivity,
    #[serde(default = "default_memory_lifecycle")]
    pub lifecycle: MemoryLifecycle,
    pub source: MemoryCandidateSource,
    pub source_id: Option<Uuid>,
    pub rationale: String,
    #[serde(default)]
    pub evidence_excerpt: String,
    #[serde(default)]
    pub privacy_review: String,
    #[serde(default = "default_memory_candidate_suggested_action")]
    pub suggested_action: MemoryCandidateSuggestedAction,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl MemoryCandidate {
    pub fn new(
        title: String,
        body: String,
        source: MemoryCandidateSource,
        source_id: Option<Uuid>,
        rationale: String,
    ) -> Result<Self, String> {
        Self::new_with_metadata(
            title,
            body,
            source,
            source_id,
            rationale,
            default_memory_candidate_type(),
            default_memory_scope(),
            default_memory_sensitivity(),
            default_memory_lifecycle(),
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "This public constructor preserves existing package and EventStore callers plus review-safe defaults; remove only when every caller migrates together without changing candidate serialization or validation."
    )]
    pub fn new_with_metadata(
        title: String,
        body: String,
        source: MemoryCandidateSource,
        source_id: Option<Uuid>,
        rationale: String,
        memory_type: MemoryType,
        scope: MemoryScope,
        sensitivity: MemorySensitivity,
        lifecycle: MemoryLifecycle,
    ) -> Result<Self, String> {
        Self::new_with_metadata_and_expiration(
            title,
            body,
            source,
            source_id,
            rationale,
            memory_type,
            scope,
            sensitivity,
            lifecycle,
            None,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "This public constructor is shared by Tauri-backed memory flows and compatibility tests; remove only through a coordinated caller migration preserving validation order, defaults, expiration filtering, and serialized fields."
    )]
    pub fn new_with_metadata_and_expiration(
        title: String,
        body: String,
        source: MemoryCandidateSource,
        source_id: Option<Uuid>,
        rationale: String,
        memory_type: MemoryType,
        scope: MemoryScope,
        sensitivity: MemorySensitivity,
        lifecycle: MemoryLifecycle,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<Self, String> {
        let title = title.trim().to_string();
        let body = body.trim().to_string();
        if title.is_empty() {
            return Err("memory candidate title is required".to_string());
        }
        if body.is_empty() {
            return Err("memory candidate body is required".to_string());
        }
        if lifecycle == MemoryLifecycle::Expires && expires_at.is_none() {
            return Err("memory candidate expiration date is required".to_string());
        }

        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            title,
            body,
            memory_type,
            scope,
            sensitivity,
            lifecycle,
            source,
            source_id,
            rationale: rationale.trim().to_string(),
            evidence_excerpt: String::new(),
            privacy_review: String::new(),
            suggested_action: default_memory_candidate_suggested_action(),
            expires_at: expires_at.filter(|_| lifecycle == MemoryLifecycle::Expires),
            created_at: now,
            updated_at: now,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryCandidateResolution {
    pub id: Uuid,
    pub candidate_id: Uuid,
    pub accepted: bool,
    pub note: String,
    pub created_at: DateTime<Utc>,
}

impl MemoryCandidateResolution {
    pub fn new(candidate_id: Uuid, accepted: bool, note: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            candidate_id,
            accepted,
            note: note.trim().to_string(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryRecord {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    #[serde(default = "default_memory_record_type")]
    pub memory_type: MemoryType,
    #[serde(default = "default_memory_scope")]
    pub scope: MemoryScope,
    #[serde(default = "default_memory_sensitivity")]
    pub sensitivity: MemorySensitivity,
    #[serde(default = "default_memory_lifecycle")]
    pub lifecycle: MemoryLifecycle,
    pub source: MemoryRecordSource,
    pub source_id: Option<Uuid>,
    pub pinned: bool,
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub linked_memory_ids: Vec<Uuid>,
    #[serde(default)]
    pub linked_memories: Vec<MemoryRecordLinkSummary>,
    #[serde(
        default = "default_memory_search_match",
        skip_serializing_if = "MemorySearchMatch::is_direct"
    )]
    pub search_match: MemorySearchMatch,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl MemoryRecord {
    pub fn from_task_record(record: &TaskRecord) -> Self {
        let now = Utc::now();
        let body = if record.summary.is_empty() {
            record.title.clone()
        } else {
            record.summary.clone()
        };

        Self {
            id: Uuid::new_v4(),
            title: record.title.clone(),
            body,
            memory_type: MemoryType::ProjectContext,
            scope: default_memory_scope(),
            sensitivity: default_memory_sensitivity(),
            lifecycle: default_memory_lifecycle(),
            source: MemoryRecordSource::TaskRecord,
            source_id: Some(record.id),
            pinned: false,
            expires_at: None,
            linked_memory_ids: Vec::new(),
            linked_memories: Vec::new(),
            search_match: MemorySearchMatch::direct(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn from_memory_candidate(candidate: &MemoryCandidate) -> Self {
        let now = Utc::now();

        Self {
            id: Uuid::new_v4(),
            title: candidate.title.clone(),
            body: candidate.body.clone(),
            memory_type: candidate.memory_type,
            scope: candidate.scope,
            sensitivity: candidate.sensitivity,
            lifecycle: candidate.lifecycle,
            source: MemoryRecordSource::MemoryCandidate,
            source_id: Some(candidate.id),
            pinned: false,
            expires_at: candidate.expires_at,
            linked_memory_ids: Vec::new(),
            linked_memories: Vec::new(),
            search_match: MemorySearchMatch::direct(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn is_expired_at(&self, now: DateTime<Utc>) -> bool {
        self.lifecycle == MemoryLifecycle::Expires
            && self
                .expires_at
                .map(|expires_at| expires_at <= now)
                .unwrap_or(false)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemorySearchMatch {
    pub source: MemorySearchMatchSource,
    pub linked_memory_id: Option<Uuid>,
    pub relation: Option<MemoryRelationKind>,
}

impl MemorySearchMatch {
    pub fn direct() -> Self {
        Self {
            source: MemorySearchMatchSource::Direct,
            linked_memory_id: None,
            relation: None,
        }
    }

    pub fn linked(
        source: MemorySearchMatchSource,
        linked_memory_id: Uuid,
        relation: MemoryRelationKind,
    ) -> Self {
        Self {
            source,
            linked_memory_id: Some(linked_memory_id),
            relation: Some(relation),
        }
    }

    pub fn is_direct(&self) -> bool {
        self.source == MemorySearchMatchSource::Direct
            && self.linked_memory_id.is_none()
            && self.relation.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryRecordLinkSummary {
    pub id: Uuid,
    pub title: String,
    pub memory_type: MemoryType,
    pub scope: MemoryScope,
    #[serde(default = "default_memory_relation_kind")]
    pub relation: MemoryRelationKind,
    #[serde(default)]
    pub note: String,
    pub updated_at: DateTime<Utc>,
}

impl From<&MemoryRecord> for MemoryRecordLinkSummary {
    fn from(record: &MemoryRecord) -> Self {
        Self {
            id: record.id,
            title: record.title.clone(),
            memory_type: record.memory_type,
            scope: record.scope,
            relation: default_memory_relation_kind(),
            note: String::new(),
            updated_at: record.updated_at,
        }
    }
}

impl MemoryRecordLinkSummary {
    pub fn with_relation(mut self, relation: MemoryRelationKind) -> Self {
        self.relation = relation;
        self
    }

    pub fn with_link_context(mut self, relation: MemoryRelationKind, note: &str) -> Self {
        self.relation = relation;
        self.note = note.trim().to_string();
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryRecordLink {
    pub id: Uuid,
    pub source_memory_id: Uuid,
    pub target_memory_id: Uuid,
    pub candidate_id: Option<Uuid>,
    #[serde(default = "default_memory_relation_kind")]
    pub relation: MemoryRelationKind,
    pub note: String,
    pub created_at: DateTime<Utc>,
}

impl MemoryRecordLink {
    pub fn new(
        source_memory_id: Uuid,
        target_memory_id: Uuid,
        candidate_id: Option<Uuid>,
        relation: MemoryRelationKind,
        note: String,
    ) -> Result<Self, String> {
        if source_memory_id == target_memory_id {
            return Err("memory link requires two different memories".to_string());
        }

        Ok(Self {
            id: Uuid::new_v4(),
            source_memory_id,
            target_memory_id,
            candidate_id,
            relation,
            note: note.trim().to_string(),
            created_at: Utc::now(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryConflictSummary {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    pub memory_type: MemoryType,
    pub scope: MemoryScope,
    pub sensitivity: MemorySensitivity,
    pub lifecycle: MemoryLifecycle,
    pub source: MemoryRecordSource,
    pub source_id: Option<Uuid>,
    pub expires_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

impl From<&MemoryRecord> for MemoryConflictSummary {
    fn from(record: &MemoryRecord) -> Self {
        Self {
            id: record.id,
            title: record.title.clone(),
            body: record.body.clone(),
            memory_type: record.memory_type,
            scope: record.scope,
            sensitivity: record.sensitivity,
            lifecycle: record.lifecycle,
            source: record.source,
            source_id: record.source_id,
            expires_at: record.expires_at,
            updated_at: record.updated_at,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryCandidateRecord {
    pub candidate: MemoryCandidate,
    pub resolution: Option<MemoryCandidateResolution>,
    pub effective_status: MemoryCandidateStatus,
    #[serde(default)]
    pub conflicting_memory_ids: Vec<Uuid>,
    #[serde(default)]
    pub conflicting_memories: Vec<MemoryConflictSummary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryCandidateMergePreview {
    pub candidate_id: Uuid,
    pub source_memory_ids: Vec<Uuid>,
    pub title: String,
    pub body: String,
    pub memory_type: MemoryType,
    pub scope: MemoryScope,
    pub sensitivity: MemorySensitivity,
    pub lifecycle: MemoryLifecycle,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryCandidateReplacePreview {
    pub candidate_id: Uuid,
    pub target_memory_ids: Vec<Uuid>,
    pub replacement_title: String,
    pub replacement_body: String,
    pub memory_type: MemoryType,
    pub scope: MemoryScope,
    pub sensitivity: MemorySensitivity,
    pub lifecycle: MemoryLifecycle,
    pub expires_at: Option<DateTime<Utc>>,
    pub target_memories: Vec<MemoryConflictSummary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryRecordUpdate {
    pub id: Uuid,
    pub memory_id: Uuid,
    pub title: String,
    pub body: String,
    pub memory_type: MemoryType,
    pub scope: MemoryScope,
    pub sensitivity: MemorySensitivity,
    pub lifecycle: MemoryLifecycle,
    pub pinned: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub note: String,
    pub updated_at: DateTime<Utc>,
}

pub(crate) struct MemoryRecordUpdateInput {
    pub(crate) memory_id: Uuid,
    pub(crate) title: String,
    pub(crate) body: String,
    pub(crate) memory_type: MemoryType,
    pub(crate) scope: MemoryScope,
    pub(crate) sensitivity: MemorySensitivity,
    pub(crate) lifecycle: MemoryLifecycle,
    pub(crate) pinned: bool,
    pub(crate) expires_at: Option<DateTime<Utc>>,
    pub(crate) note: String,
}

impl MemoryRecordUpdate {
    pub(crate) fn new(input: MemoryRecordUpdateInput) -> Result<Self, String> {
        let MemoryRecordUpdateInput {
            memory_id,
            title,
            body,
            memory_type,
            scope,
            sensitivity,
            lifecycle,
            pinned,
            expires_at,
            note,
        } = input;
        let title = title.trim().to_string();
        let body = body.trim().to_string();
        if title.is_empty() {
            return Err("memory title is required".to_string());
        }
        if body.is_empty() {
            return Err("memory body is required".to_string());
        }
        if lifecycle == MemoryLifecycle::Expires && expires_at.is_none() {
            return Err("memory expiration date is required".to_string());
        }

        Ok(Self {
            id: Uuid::new_v4(),
            memory_id,
            title,
            body,
            memory_type,
            scope,
            sensitivity,
            lifecycle,
            pinned,
            expires_at: expires_at.filter(|_| lifecycle == MemoryLifecycle::Expires),
            note: note.trim().to_string(),
            updated_at: Utc::now(),
        })
    }

    pub fn apply_to(&self, record: &MemoryRecord) -> MemoryRecord {
        MemoryRecord {
            id: record.id,
            title: self.title.clone(),
            body: self.body.clone(),
            memory_type: self.memory_type,
            scope: self.scope,
            sensitivity: self.sensitivity,
            lifecycle: self.lifecycle,
            source: record.source,
            source_id: record.source_id,
            pinned: self.pinned,
            expires_at: self.expires_at,
            linked_memory_ids: record.linked_memory_ids.clone(),
            linked_memories: record.linked_memories.clone(),
            search_match: record.search_match.clone(),
            created_at: record.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryRecordDeletion {
    pub id: Uuid,
    pub memory_id: Uuid,
    pub note: String,
    pub deleted_at: DateTime<Utc>,
}

impl MemoryRecordDeletion {
    pub fn new(memory_id: Uuid, note: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            memory_id,
            note: note.trim().to_string(),
            deleted_at: Utc::now(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemorySelectedFeedback {
    pub id: Uuid,
    pub memory_id: Uuid,
    pub context_receipt_id: Option<Uuid>,
    pub feedback: MemorySelectedFeedbackKind,
    pub note: String,
    pub created_at: DateTime<Utc>,
}

impl MemorySelectedFeedback {
    pub fn new(
        memory_id: Uuid,
        context_receipt_id: Option<Uuid>,
        feedback: MemorySelectedFeedbackKind,
        note: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            memory_id,
            context_receipt_id,
            feedback,
            note: compact_feedback_note(note),
            created_at: Utc::now(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMaintenanceReviewKind {
    Retrieval,
    UpdateArchive,
    Conflict,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMaintenanceActionKind {
    MarkReviewed,
    Snooze,
    RetrievalReviewed,
    UpdateCandidateCreated,
    Archived,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryMaintenanceFeedbackCounts {
    pub useful: usize,
    pub irrelevant: usize,
    pub stale: usize,
    pub conflicting: usize,
    pub should_update: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryMaintenanceReviewAction {
    pub id: Uuid,
    pub memory_id: Uuid,
    pub action: MemoryMaintenanceActionKind,
    pub note: String,
    pub snoozed_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl MemoryMaintenanceReviewAction {
    pub fn new(
        memory_id: Uuid,
        action: MemoryMaintenanceActionKind,
        snoozed_until: Option<DateTime<Utc>>,
        note: String,
    ) -> Result<Self, String> {
        if action != MemoryMaintenanceActionKind::Snooze && snoozed_until.is_some() {
            return Err("only snooze maintenance actions can set snoozed_until".to_string());
        }

        Ok(Self {
            id: Uuid::new_v4(),
            memory_id,
            action,
            note: compact_feedback_note(note),
            snoozed_until,
            created_at: Utc::now(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MemoryMaintenanceReviewItem {
    pub memory: MemoryRecord,
    pub feedback_counts: MemoryMaintenanceFeedbackCounts,
    pub feedback_count: usize,
    pub quality_score: i32,
    pub quality_signals: Vec<String>,
    pub latest_feedback: Option<MemorySelectedFeedback>,
    pub review_kinds: Vec<MemoryMaintenanceReviewKind>,
    pub recommended_actions: Vec<MemoryMaintenanceActionKind>,
    pub review_needed: bool,
    pub snoozed_until: Option<DateTime<Utc>>,
    pub last_action: Option<MemoryMaintenanceReviewAction>,
}

fn compact_feedback_note(note: String) -> String {
    note.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(280)
        .collect()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FoundationState {
    pub app_name: String,
    #[serde(default = "default_large_model_provider")]
    pub large_model_provider: LargeModelProvider,
    pub model_route: ModelRoute,
    pub thinking_level: ThinkingLevel,
    pub access_mode: AccessMode,
    pub workspace_scope: WorkspaceScope,
    #[serde(default)]
    pub network_search_source_model: Option<NetworkSearchSourceModel>,
    pub tool_backends: ToolBackendSettings,
}

impl Default for FoundationState {
    fn default() -> Self {
        Self {
            app_name: "DS Agent".to_string(),
            large_model_provider: default_large_model_provider(),
            model_route: ModelRoute::Auto,
            thinking_level: ThinkingLevel::Auto,
            access_mode: AccessMode::FullAccess,
            workspace_scope: WorkspaceScope::Workspace,
            network_search_source_model: None,
            tool_backends: ToolBackendSettings::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KernelEvent {
    pub id: Uuid,
    pub event_type: String,
    pub payload_json: String,
    pub created_at: DateTime<Utc>,
}

impl KernelEvent {
    pub fn new<T>(event_type: impl Into<String>, payload: T) -> serde_json::Result<Self>
    where
        T: Serialize,
    {
        Ok(Self {
            id: Uuid::new_v4(),
            event_type: event_type.into(),
            payload_json: serde_json::to_string(&payload)?,
            created_at: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AccessMode, ComputerControlBackend, ComputerScreenshotBackend, DriveBackend, EmailBackend,
        FoundationState, KernelEvent, LargeModelProvider, MemoryCandidate, MemoryCandidateSource,
        MemoryLifecycle, MemoryRecord, MemoryRecordSource, MemoryScope, MemorySensitivity,
        MemoryType, ModelRoute, NetworkSearchBackend, TaskRecord, TaskRecordStatus, ThinkingLevel,
        ToolBackendSettings, WorkspaceScope,
    };

    #[test]
    fn foundation_state_defaults_to_deepseek_agent_os() {
        let state = FoundationState::default();

        assert_eq!(state.app_name, "DS Agent");
        assert_eq!(state.large_model_provider, LargeModelProvider::DeepSeek);
        assert_eq!(state.model_route, ModelRoute::Auto);
        assert_eq!(state.thinking_level, ThinkingLevel::Auto);
        assert_eq!(state.access_mode, AccessMode::FullAccess);
        assert_eq!(state.workspace_scope, WorkspaceScope::Workspace);
        assert_eq!(state.network_search_source_model, None);
    }

    #[test]
    fn foundation_state_defaults_to_confirmed_tool_backends() {
        let state = FoundationState::default();

        assert_eq!(
            state.tool_backends.network_search,
            NetworkSearchBackend::SourceBackedModel
        );
        assert_eq!(state.tool_backends.email, EmailBackend::ArchitectureOnly);
        assert_eq!(
            state.tool_backends.drive,
            DriveBackend::LocalFolderExportPackage
        );
        if cfg!(target_os = "macos") {
            assert_eq!(
                state.tool_backends.computer_screenshot,
                ComputerScreenshotBackend::LocalMacosScreenCapture
            );
            assert_eq!(
                state.tool_backends.computer_control,
                ComputerControlBackend::LocalMacosInputControl
            );
        } else {
            assert_eq!(
                state.tool_backends.computer_screenshot,
                ComputerScreenshotBackend::LocalWindowsScreenCapture
            );
            assert_eq!(
                state.tool_backends.computer_control,
                ComputerControlBackend::LocalWindowsInputControl
            );
        }
    }

    #[test]
    fn tool_backend_settings_serialize_phase_two_choices() {
        let value =
            serde_json::to_value(ToolBackendSettings::default()).expect("settings serialize");

        assert_eq!(value["network_search"], "source_backed_model");
        assert_eq!(value["email"], "architecture_only");
        assert_eq!(value["drive"], "local_folder_export_package");
        if cfg!(target_os = "macos") {
            assert_eq!(value["computer_screenshot"], "local_macos_screen_capture");
            assert_eq!(value["computer_control"], "local_macos_input_control");
        } else {
            assert_eq!(value["computer_screenshot"], "local_windows_screen_capture");
            assert_eq!(value["computer_control"], "local_windows_input_control");
        }
    }

    #[test]
    fn kernel_event_serializes_payload_json() {
        let event = KernelEvent::new(
            "foundation.ready",
            serde_json::json!({
                "ready": true
            }),
        )
        .expect("payload serializes");

        assert_ne!(event.id, uuid::Uuid::nil());
        assert_eq!(event.event_type, "foundation.ready");
        assert_eq!(event.payload_json, r#"{"ready":true}"#);
        assert!(event.created_at <= chrono::Utc::now());
    }

    #[test]
    fn task_record_trims_title_and_defaults_to_active() {
        let record = TaskRecord::new(
            "  Draft weekly operations brief  ".to_string(),
            "  Pull mail, drive, and browser findings. ".to_string(),
        )
        .expect("record is valid");

        assert_eq!(record.title, "Draft weekly operations brief");
        assert_eq!(record.summary, "Pull mail, drive, and browser findings.");
        assert_eq!(record.status, TaskRecordStatus::Active);
        assert!(record.created_at <= chrono::Utc::now());
        assert_eq!(record.created_at, record.updated_at);
    }

    #[test]
    fn task_record_rejects_blank_title() {
        let error = TaskRecord::new("   ".to_string(), "summary".to_string())
            .expect_err("blank title should fail");

        assert_eq!(error, "task title is required");
    }

    #[test]
    fn memory_record_from_task_record_preserves_source_and_content() {
        let task = TaskRecord::new(
            "Prepare investor briefing".to_string(),
            "Remember that the briefing depends on inbox and drive evidence.".to_string(),
        )
        .expect("task is valid");

        let memory = MemoryRecord::from_task_record(&task);

        assert_eq!(memory.title, "Prepare investor briefing");
        assert_eq!(
            memory.body,
            "Remember that the briefing depends on inbox and drive evidence."
        );
        assert_eq!(memory.source, MemoryRecordSource::TaskRecord);
        assert_eq!(memory.source_id, Some(task.id));
        assert!(!memory.pinned);
        assert!(memory.created_at <= chrono::Utc::now());
        assert_eq!(memory.created_at, memory.updated_at);
    }

    #[test]
    fn memory_record_from_memory_candidate_preserves_review_source() {
        let candidate = MemoryCandidate::new(
            "Preferred report tone".to_string(),
            "Use concise operating language with clear owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User proposed this as reusable guidance.".to_string(),
        )
        .expect("candidate is valid");

        let memory = MemoryRecord::from_memory_candidate(&candidate);

        assert_eq!(memory.title, "Preferred report tone");
        assert_eq!(
            memory.body,
            "Use concise operating language with clear owners and evidence."
        );
        assert_eq!(memory.source, MemoryRecordSource::MemoryCandidate);
        assert_eq!(memory.source_id, Some(candidate.id));
        assert!(!memory.pinned);
    }

    #[test]
    fn memory_metadata_candidate_defaults_are_review_safe() {
        let candidate = MemoryCandidate::new(
            "Preferred report tone".to_string(),
            "Use concise operating language with clear owners.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User proposed this as reusable guidance.".to_string(),
        )
        .expect("candidate is valid");

        assert_eq!(candidate.memory_type, MemoryType::Preference);
        assert_eq!(candidate.scope, MemoryScope::Workspace);
        assert_eq!(candidate.sensitivity, MemorySensitivity::Normal);
        assert_eq!(candidate.lifecycle, MemoryLifecycle::Active);
    }

    #[test]
    fn memory_metadata_legacy_candidate_json_defaults_review_tags() {
        let candidate: MemoryCandidate = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "title": "Legacy candidate",
            "body": "Loaded from an older event payload.",
            "source": "manual",
            "source_id": null,
            "rationale": "Older versions did not store metadata.",
            "created_at": chrono::Utc::now(),
            "updated_at": chrono::Utc::now()
        }))
        .expect("legacy candidate deserializes");

        assert_eq!(candidate.memory_type, MemoryType::Preference);
        assert_eq!(candidate.scope, MemoryScope::Workspace);
        assert_eq!(candidate.sensitivity, MemorySensitivity::Normal);
        assert_eq!(candidate.lifecycle, MemoryLifecycle::Active);
    }

    #[test]
    fn memory_metadata_legacy_record_json_defaults_review_tags() {
        let record: MemoryRecord = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "title": "Legacy memory",
            "body": "Loaded from an older memory event payload.",
            "source": "task_record",
            "source_id": uuid::Uuid::new_v4(),
            "pinned": false,
            "created_at": chrono::Utc::now(),
            "updated_at": chrono::Utc::now()
        }))
        .expect("legacy memory record deserializes");

        assert_eq!(record.memory_type, MemoryType::ProjectContext);
        assert_eq!(record.scope, MemoryScope::Workspace);
        assert_eq!(record.sensitivity, MemorySensitivity::Normal);
        assert_eq!(record.lifecycle, MemoryLifecycle::Active);
    }
}
