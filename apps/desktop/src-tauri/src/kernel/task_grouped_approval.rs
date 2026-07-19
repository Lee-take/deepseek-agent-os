use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::kernel::goal_lifecycle::GoalLifecycleProjection;
use crate::kernel::policy::{
    exact_tool_preview_hash, CapabilityKind, RiskLevel, TOOL_APPROVAL_PREVIEW_REVISION,
};
use crate::kernel::task_capability_manifest::{
    TaskAuthorizationPreview, TaskCapabilityManifest, TaskCapabilityManifestError,
};

pub const TASK_GROUPED_APPROVAL_VERSION: &str = "ds-agent.task-grouped-approval/v1";
pub const TASK_GROUPED_APPROVAL_EVENT_VERSION: &str = "ds-agent.task-grouped-approval-event/v1";
pub const TASK_GROUPED_AUTHORIZATION_UI_VERSION: &str = "ds-agent.task-grouped-authorization-ui/v1";

const GROUP_ID_DOMAIN: &[u8] = b"ds-agent.task-grouped-approval-id.v1\0";
const GROUP_INTEGRITY_DOMAIN: &[u8] = b"ds-agent.task-grouped-approval-integrity.v1\0";
const ITEM_ID_DOMAIN: &[u8] = b"ds-agent.task-grouped-approval-item-id.v1\0";
const REQUEST_ID_DOMAIN: &[u8] = b"ds-agent.task-grouped-approval-request-id.v1\0";
const REQUEST_FINGERPRINT_DOMAIN: &[u8] =
    b"ds-agent.task-grouped-approval-request-fingerprint.v1\0";
const RESOLUTION_ID_DOMAIN: &[u8] = b"ds-agent.task-grouped-approval-resolution-id.v1\0";
const EVENT_ID_DOMAIN: &[u8] = b"ds-agent.task-grouped-approval-event-id.v1\0";
const ITEM_EVENT_ID_DOMAIN: &[u8] = b"ds-agent.task-grouped-approval-item-event-id.v1\0";
const LEGACY_CONSUMPTION_ID_DOMAIN: &[u8] =
    b"ds-agent.task-grouped-approval-legacy-consumption-id.v1\0";
const MAX_GROUPED_APPROVAL_JSON_BYTES: usize = 192 * 1024;
const MAX_AUDIT_ITEMS: usize = 2048;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskGroupedApprovalActor {
    User,
    KernelLifecycle,
    DeepSeekModel,
    FrontendPayload,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskGroupedApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Revoked,
    Expired,
    ScopeChanged,
}

impl TaskGroupedApprovalStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Revoked => "revoked",
            Self::Expired => "expired",
            Self::ScopeChanged => "scope_changed",
        }
    }

    pub const fn carries_authority(self) -> bool {
        matches!(self, Self::Approved)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskGroupedCapabilityAuditStatus {
    Pending,
    Approved,
    Rejected,
    Revoked,
    Expired,
    ScopeChanged,
}

impl From<TaskGroupedApprovalStatus> for TaskGroupedCapabilityAuditStatus {
    fn from(value: TaskGroupedApprovalStatus) -> Self {
        match value {
            TaskGroupedApprovalStatus::Pending => Self::Pending,
            TaskGroupedApprovalStatus::Approved => Self::Approved,
            TaskGroupedApprovalStatus::Rejected => Self::Rejected,
            TaskGroupedApprovalStatus::Revoked => Self::Revoked,
            TaskGroupedApprovalStatus::Expired => Self::Expired,
            TaskGroupedApprovalStatus::ScopeChanged => Self::ScopeChanged,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskGroupedApprovalResolutionSource {
    User,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskGroupedCapabilityAudit {
    pub item_id: String,
    pub capability: CapabilityKind,
    pub risk_level: RiskLevel,
    pub tool_id: String,
    pub tool_version: String,
    pub approval_request_id: Uuid,
    pub request_fingerprint: String,
    pub exact_preview: String,
    pub exact_preview_revision: u32,
    pub exact_preview_hash: String,
    pub status: TaskGroupedCapabilityAuditStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskGroupedApprovalResolution {
    pub id: Uuid,
    pub approved: bool,
    pub source: TaskGroupedApprovalResolutionSource,
    pub expected_projection_revision: u64,
    pub task_id: Uuid,
    pub manifest_revision: String,
    pub manifest_fingerprint: String,
    pub preview_schema_revision: u32,
    pub preview_renderer_revision: u32,
    pub preview_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskGroupedApprovalResolutionClaim {
    pub group_id: Uuid,
    pub task_id: Uuid,
    pub expected_projection_revision: u64,
    pub manifest_revision: String,
    pub manifest_fingerprint: String,
    pub preview_schema_revision: u32,
    pub preview_renderer_revision: u32,
    pub preview_hash: String,
    pub actor: TaskGroupedApprovalActor,
}

impl TaskGroupedApprovalResolutionClaim {
    pub fn from_group(group: &TaskGroupedApproval, actor: TaskGroupedApprovalActor) -> Self {
        Self {
            group_id: group.id,
            task_id: group.task_id,
            expected_projection_revision: group.projection_revision,
            manifest_revision: group.manifest.revision.clone(),
            manifest_fingerprint: group.manifest.fingerprint.clone(),
            preview_schema_revision: group.preview.schema_revision,
            preview_renderer_revision: group.preview.renderer_revision,
            preview_hash: group.preview.preview_hash.clone(),
            actor,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskGroupedCapabilityClaim {
    pub group_id: Uuid,
    pub task_id: Uuid,
    pub expected_projection_revision: u64,
    pub manifest_revision: String,
    pub manifest_fingerprint: String,
    pub preview_schema_revision: u32,
    pub preview_renderer_revision: u32,
    pub preview_hash: String,
    pub capability: CapabilityKind,
    pub tool_id: String,
    pub request_fingerprint: String,
}

impl TaskGroupedCapabilityClaim {
    pub fn from_group_item(group: &TaskGroupedApproval, item: &TaskGroupedCapabilityAudit) -> Self {
        Self {
            group_id: group.id,
            task_id: group.task_id,
            expected_projection_revision: group.projection_revision,
            manifest_revision: group.manifest.revision.clone(),
            manifest_fingerprint: group.manifest.fingerprint.clone(),
            preview_schema_revision: group.preview.schema_revision,
            preview_renderer_revision: group.preview.renderer_revision,
            preview_hash: group.preview.preview_hash.clone(),
            capability: item.capability,
            tool_id: item.tool_id.clone(),
            request_fingerprint: item.request_fingerprint.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskGroupedCapabilityGrant {
    pub group_id: Uuid,
    pub task_id: Uuid,
    pub projection_revision: u64,
    pub manifest_revision: String,
    pub manifest_fingerprint: String,
    pub preview_renderer_revision: u32,
    pub preview_hash: String,
    pub capability: CapabilityKind,
    pub tool_id: String,
    pub request_fingerprint: String,
    pub approval_request_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskGroupedApproval {
    pub version: String,
    pub id: Uuid,
    pub task_id: Uuid,
    pub manifest: TaskCapabilityManifest,
    pub preview: TaskAuthorizationPreview,
    pub status: TaskGroupedApprovalStatus,
    pub projection_revision: u64,
    pub capability_audits: Vec<TaskGroupedCapabilityAudit>,
    pub resolution: Option<TaskGroupedApprovalResolution>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub integrity_hash: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskGroupedApprovalEventReceipt {
    pub version: String,
    pub group_id: Uuid,
    pub task_id: Uuid,
    pub status: TaskGroupedApprovalStatus,
    pub projection_revision: u64,
    pub manifest_revision: String,
    pub manifest_fingerprint: String,
    pub preview_renderer_revision: u32,
    pub preview_hash: String,
    pub capability_audit_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskGroupedAuthorizationIntent {
    pub group_id: Uuid,
    pub task_id: Uuid,
    pub expected_projection_revision: u64,
    pub manifest_revision: String,
    pub manifest_fingerprint: String,
    pub preview_schema_revision: u32,
    pub preview_renderer_revision: u32,
    pub preview_hash: String,
}

impl TaskGroupedAuthorizationIntent {
    pub fn resolution_claim(&self) -> TaskGroupedApprovalResolutionClaim {
        TaskGroupedApprovalResolutionClaim {
            group_id: self.group_id,
            task_id: self.task_id,
            expected_projection_revision: self.expected_projection_revision,
            manifest_revision: self.manifest_revision.clone(),
            manifest_fingerprint: self.manifest_fingerprint.clone(),
            preview_schema_revision: self.preview_schema_revision,
            preview_renderer_revision: self.preview_renderer_revision,
            preview_hash: self.preview_hash.clone(),
            actor: TaskGroupedApprovalActor::User,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskGroupedCapabilityAuditView {
    pub capability: CapabilityKind,
    pub risk_level: RiskLevel,
    pub status: TaskGroupedCapabilityAuditStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskGroupedAuthorizationView {
    pub version: String,
    pub intent: TaskGroupedAuthorizationIntent,
    pub status: TaskGroupedApprovalStatus,
    pub goal: String,
    pub applications: Vec<String>,
    pub paths: Vec<String>,
    pub accounts: Vec<String>,
    pub recipients: Vec<String>,
    pub time_windows: Vec<String>,
    pub external_targets: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub risk_level: RiskLevel,
    pub verifiers: Vec<String>,
    pub capability_audits: Vec<TaskGroupedCapabilityAuditView>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskGroupedApprovalError {
    ManifestInvalid,
    PreviewInvalid,
    BindingMismatch,
    IntegrityMismatch,
    InvalidState,
    InvalidActor,
    Expired,
    CollectionOutOfBounds,
    NonCanonicalAudit,
}

impl std::fmt::Display for TaskGroupedApprovalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ManifestInvalid => "task grouped approval manifest is invalid",
            Self::PreviewInvalid => "task grouped approval preview is invalid",
            Self::BindingMismatch => "task grouped approval binding changed",
            Self::IntegrityMismatch => "task grouped approval integrity check failed",
            Self::InvalidState => "task grouped approval state is invalid",
            Self::InvalidActor => "task grouped approval actor has no authority",
            Self::Expired => "task grouped approval expired",
            Self::CollectionOutOfBounds => "task grouped approval audit collection is invalid",
            Self::NonCanonicalAudit => "task grouped approval audit order is invalid",
        })
    }
}

impl std::error::Error for TaskGroupedApprovalError {}

impl From<TaskCapabilityManifestError> for TaskGroupedApprovalError {
    fn from(_: TaskCapabilityManifestError) -> Self {
        Self::ManifestInvalid
    }
}

#[derive(Serialize)]
struct GroupIdCanonical<'a> {
    task_id: Uuid,
    goal_revision: &'a str,
    goal_fingerprint: &'a str,
    manifest_revision: &'a str,
    manifest_fingerprint: &'a str,
    preview_schema_revision: u32,
    preview_renderer_revision: u32,
    preview_hash: &'a str,
    expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct ItemCanonical<'a> {
    group_id: Uuid,
    capability: &'a str,
    tool_id: &'a str,
    tool_version: &'a str,
}

#[derive(Serialize)]
struct RequestFingerprintCanonical<'a> {
    group_id: Uuid,
    task_id: Uuid,
    manifest_revision: &'a str,
    manifest_fingerprint: &'a str,
    preview_schema_revision: u32,
    preview_renderer_revision: u32,
    preview_hash: &'a str,
    capability: &'a str,
    tool_id: &'a str,
    tool_version: &'a str,
    expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct GroupIntegrityCanonical<'a> {
    version: &'a str,
    id: Uuid,
    task_id: Uuid,
    manifest: &'a TaskCapabilityManifest,
    preview: &'a TaskAuthorizationPreview,
    status: TaskGroupedApprovalStatus,
    projection_revision: u64,
    capability_audits: &'a [TaskGroupedCapabilityAudit],
    resolution: &'a Option<TaskGroupedApprovalResolution>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TaskGroupedApproval {
    pub fn new(
        manifest: TaskCapabilityManifest,
        preview: TaskAuthorizationPreview,
        now: DateTime<Utc>,
    ) -> Result<Self, TaskGroupedApprovalError> {
        manifest
            .validate_integrity()
            .map_err(|_| TaskGroupedApprovalError::ManifestInvalid)?;
        preview
            .validate_for_manifest(&manifest)
            .map_err(|_| TaskGroupedApprovalError::PreviewInvalid)?;
        if manifest.expires_at <= now {
            return Err(TaskGroupedApprovalError::Expired);
        }
        let id = group_id_for(&manifest, &preview);
        let mut capability_audits = Vec::new();
        for entry in &manifest.capabilities {
            for tool in &entry.tools {
                let item_id = item_id_for(id, entry.capability, &tool.tool_id, &tool.tool_version);
                let request_fingerprint = request_fingerprint_for(
                    id,
                    &manifest,
                    &preview,
                    entry.capability,
                    &tool.tool_id,
                    &tool.tool_version,
                );
                let exact_preview =
                    compact_exact_preview(&manifest, &preview, entry.capability, &tool.tool_id);
                capability_audits.push(TaskGroupedCapabilityAudit {
                    approval_request_id: approval_request_id_for(id, &item_id),
                    item_id,
                    capability: entry.capability,
                    risk_level: entry.risk_level,
                    tool_id: tool.tool_id.clone(),
                    tool_version: tool.tool_version.clone(),
                    request_fingerprint,
                    exact_preview_hash: exact_tool_preview_hash(
                        TOOL_APPROVAL_PREVIEW_REVISION,
                        &exact_preview,
                    ),
                    exact_preview,
                    exact_preview_revision: TOOL_APPROVAL_PREVIEW_REVISION,
                    status: TaskGroupedCapabilityAuditStatus::Pending,
                });
            }
        }
        capability_audits.sort_by(|left, right| left.item_id.cmp(&right.item_id));
        let mut group = Self {
            version: TASK_GROUPED_APPROVAL_VERSION.to_string(),
            id,
            task_id: manifest.task_id,
            manifest,
            preview,
            status: TaskGroupedApprovalStatus::Pending,
            projection_revision: 0,
            capability_audits,
            resolution: None,
            created_at: now,
            updated_at: now,
            integrity_hash: String::new(),
        };
        group.integrity_hash = integrity_hash_for(&group);
        group.validate_integrity()?;
        Ok(group)
    }

    pub fn parse_json(json: &str) -> Result<Self, TaskGroupedApprovalError> {
        if json.len() > MAX_GROUPED_APPROVAL_JSON_BYTES {
            return Err(TaskGroupedApprovalError::CollectionOutOfBounds);
        }
        let group: Self =
            serde_json::from_str(json).map_err(|_| TaskGroupedApprovalError::IntegrityMismatch)?;
        group.validate_integrity()?;
        Ok(group)
    }

    pub fn canonical_json(&self) -> Result<String, TaskGroupedApprovalError> {
        self.validate_integrity()?;
        let json =
            serde_json::to_string(self).map_err(|_| TaskGroupedApprovalError::IntegrityMismatch)?;
        if json.len() > MAX_GROUPED_APPROVAL_JSON_BYTES {
            return Err(TaskGroupedApprovalError::CollectionOutOfBounds);
        }
        Ok(json)
    }

    pub fn validate_integrity(&self) -> Result<(), TaskGroupedApprovalError> {
        if self.version != TASK_GROUPED_APPROVAL_VERSION {
            return Err(TaskGroupedApprovalError::IntegrityMismatch);
        }
        self.manifest
            .validate_integrity()
            .map_err(|_| TaskGroupedApprovalError::ManifestInvalid)?;
        self.preview
            .validate_for_manifest(&self.manifest)
            .map_err(|_| TaskGroupedApprovalError::PreviewInvalid)?;
        if self.task_id != self.manifest.task_id
            || self.id != group_id_for(&self.manifest, &self.preview)
            || self.created_at >= self.manifest.expires_at
            || self.updated_at < self.created_at
        {
            return Err(TaskGroupedApprovalError::BindingMismatch);
        }
        if self.capability_audits.is_empty() || self.capability_audits.len() > MAX_AUDIT_ITEMS {
            return Err(TaskGroupedApprovalError::CollectionOutOfBounds);
        }
        if self
            .capability_audits
            .windows(2)
            .any(|pair| pair[0].item_id >= pair[1].item_id)
        {
            return Err(TaskGroupedApprovalError::NonCanonicalAudit);
        }
        let mut expected = Vec::new();
        for entry in &self.manifest.capabilities {
            for tool in &entry.tools {
                expected.push((entry.capability, entry.risk_level, tool));
            }
        }
        expected.sort_by_key(|(capability, _, tool)| {
            item_id_for(self.id, *capability, &tool.tool_id, &tool.tool_version)
        });
        if expected.len() != self.capability_audits.len() {
            return Err(TaskGroupedApprovalError::BindingMismatch);
        }
        let expected_status = TaskGroupedCapabilityAuditStatus::from(self.status);
        for (audit, (capability, risk_level, tool)) in
            self.capability_audits.iter().zip(expected.into_iter())
        {
            let item_id = item_id_for(self.id, capability, &tool.tool_id, &tool.tool_version);
            let request_fingerprint = request_fingerprint_for(
                self.id,
                &self.manifest,
                &self.preview,
                capability,
                &tool.tool_id,
                &tool.tool_version,
            );
            let exact_preview =
                compact_exact_preview(&self.manifest, &self.preview, capability, &tool.tool_id);
            if audit.item_id != item_id
                || audit.capability != capability
                || audit.risk_level != risk_level
                || audit.tool_id != tool.tool_id
                || audit.tool_version != tool.tool_version
                || audit.approval_request_id != approval_request_id_for(self.id, &item_id)
                || audit.request_fingerprint != request_fingerprint
                || audit.exact_preview != exact_preview
                || audit.exact_preview_revision != TOOL_APPROVAL_PREVIEW_REVISION
                || audit.exact_preview_hash
                    != exact_tool_preview_hash(TOOL_APPROVAL_PREVIEW_REVISION, &exact_preview)
                || audit.status != expected_status
            {
                return Err(TaskGroupedApprovalError::BindingMismatch);
            }
        }
        match self.status {
            TaskGroupedApprovalStatus::Pending => {
                if self.projection_revision != 0 || self.resolution.is_some() {
                    return Err(TaskGroupedApprovalError::InvalidState);
                }
            }
            TaskGroupedApprovalStatus::Approved | TaskGroupedApprovalStatus::Rejected => {
                let resolution = self
                    .resolution
                    .as_ref()
                    .ok_or(TaskGroupedApprovalError::InvalidState)?;
                if self.projection_revision != 1
                    || resolution.approved
                        != matches!(self.status, TaskGroupedApprovalStatus::Approved)
                {
                    return Err(TaskGroupedApprovalError::InvalidState);
                }
                validate_resolution(self, resolution)?;
            }
            TaskGroupedApprovalStatus::Revoked => {
                let resolution = self
                    .resolution
                    .as_ref()
                    .ok_or(TaskGroupedApprovalError::InvalidState)?;
                if !resolution.approved || self.projection_revision != 2 {
                    return Err(TaskGroupedApprovalError::InvalidState);
                }
                validate_resolution(self, resolution)?;
            }
            TaskGroupedApprovalStatus::Expired | TaskGroupedApprovalStatus::ScopeChanged => {
                match &self.resolution {
                    None if self.projection_revision == 1 => {}
                    Some(resolution) if resolution.approved && self.projection_revision == 2 => {
                        validate_resolution(self, resolution)?;
                    }
                    _ => return Err(TaskGroupedApprovalError::InvalidState),
                }
            }
        }
        if self.integrity_hash != integrity_hash_for(self) {
            return Err(TaskGroupedApprovalError::IntegrityMismatch);
        }
        Ok(())
    }

    pub fn binding_matches_resolution_claim(
        &self,
        claim: &TaskGroupedApprovalResolutionClaim,
    ) -> bool {
        self.id == claim.group_id
            && self.task_id == claim.task_id
            && self.projection_revision == claim.expected_projection_revision
            && self.manifest.revision == claim.manifest_revision
            && self.manifest.fingerprint == claim.manifest_fingerprint
            && self.preview.schema_revision == claim.preview_schema_revision
            && self.preview.renderer_revision == claim.preview_renderer_revision
            && self.preview.preview_hash == claim.preview_hash
    }

    pub fn resolution_replay_matches(
        &self,
        claim: &TaskGroupedApprovalResolutionClaim,
        approved: bool,
    ) -> bool {
        let Some(resolution) = &self.resolution else {
            return false;
        };
        resolution.approved == approved
            && self.id == claim.group_id
            && self.task_id == claim.task_id
            && resolution.expected_projection_revision == claim.expected_projection_revision
            && resolution.manifest_revision == claim.manifest_revision
            && resolution.manifest_fingerprint == claim.manifest_fingerprint
            && resolution.preview_schema_revision == claim.preview_schema_revision
            && resolution.preview_renderer_revision == claim.preview_renderer_revision
            && resolution.preview_hash == claim.preview_hash
    }

    pub fn capability_item(
        &self,
        claim: &TaskGroupedCapabilityClaim,
    ) -> Result<&TaskGroupedCapabilityAudit, TaskGroupedApprovalError> {
        if self.id != claim.group_id
            || self.task_id != claim.task_id
            || self.projection_revision != claim.expected_projection_revision
            || self.manifest.revision != claim.manifest_revision
            || self.manifest.fingerprint != claim.manifest_fingerprint
            || self.preview.schema_revision != claim.preview_schema_revision
            || self.preview.renderer_revision != claim.preview_renderer_revision
            || self.preview.preview_hash != claim.preview_hash
        {
            return Err(TaskGroupedApprovalError::BindingMismatch);
        }
        self.capability_audits
            .iter()
            .find(|item| {
                item.capability == claim.capability
                    && item.tool_id == claim.tool_id
                    && item.request_fingerprint == claim.request_fingerprint
            })
            .ok_or(TaskGroupedApprovalError::BindingMismatch)
    }

    pub fn authorization_view(
        &self,
        goal: &GoalLifecycleProjection,
    ) -> Result<TaskGroupedAuthorizationView, TaskGroupedApprovalError> {
        self.validate_integrity()?;
        let frozen = goal
            .frozen()
            .ok_or(TaskGroupedApprovalError::BindingMismatch)?;
        if goal.goal_id != self.task_id {
            return Err(TaskGroupedApprovalError::BindingMismatch);
        }
        if matches!(
            self.status,
            TaskGroupedApprovalStatus::Pending | TaskGroupedApprovalStatus::Approved
        ) {
            self.manifest
                .validate_for_goal(goal)
                .map_err(|_| TaskGroupedApprovalError::BindingMismatch)?;
        }

        let mut applications = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut accounts = BTreeSet::new();
        let mut recipients = BTreeSet::new();
        let mut time_windows = BTreeSet::new();
        let mut external_targets = BTreeSet::new();
        let mut verifiers = BTreeSet::new();
        for capability in &self.preview.capabilities {
            applications.extend(
                capability
                    .applications
                    .iter()
                    .map(|value| value.display_label.clone()),
            );
            paths.extend(
                capability
                    .path_scopes
                    .iter()
                    .map(|value| value.display_label.clone()),
            );
            accounts.extend(
                capability
                    .account_scopes
                    .iter()
                    .map(|value| value.display_label.clone()),
            );
            recipients.extend(
                capability
                    .recipient_scopes
                    .iter()
                    .map(|value| value.display_label.clone()),
            );
            time_windows.extend(
                capability
                    .time_windows
                    .iter()
                    .map(|value| value.display_label.clone()),
            );
            external_targets.extend(
                capability
                    .external_targets
                    .iter()
                    .map(|value| value.display_label.clone()),
            );
            verifiers.extend(
                capability
                    .verifiers
                    .iter()
                    .map(|value| value.summary.clone()),
            );
        }

        Ok(TaskGroupedAuthorizationView {
            version: TASK_GROUPED_AUTHORIZATION_UI_VERSION.to_string(),
            intent: TaskGroupedAuthorizationIntent {
                group_id: self.id,
                task_id: self.task_id,
                expected_projection_revision: self.projection_revision,
                manifest_revision: self.manifest.revision.clone(),
                manifest_fingerprint: self.manifest.fingerprint.clone(),
                preview_schema_revision: self.preview.schema_revision,
                preview_renderer_revision: self.preview.renderer_revision,
                preview_hash: self.preview.preview_hash.clone(),
            },
            status: self.status,
            goal: frozen.envelope.user_goal.clone(),
            applications: applications.into_iter().collect(),
            paths: paths.into_iter().collect(),
            accounts: accounts.into_iter().collect(),
            recipients: recipients.into_iter().collect(),
            time_windows: time_windows.into_iter().collect(),
            external_targets: external_targets.into_iter().collect(),
            expires_at: self.manifest.expires_at,
            risk_level: self.manifest.aggregate_risk,
            verifiers: verifiers.into_iter().collect(),
            capability_audits: self
                .capability_audits
                .iter()
                .map(|audit| TaskGroupedCapabilityAuditView {
                    capability: audit.capability,
                    risk_level: audit.risk_level,
                    status: audit.status,
                })
                .collect(),
        })
    }

    pub(crate) fn resolve(
        &self,
        claim: &TaskGroupedApprovalResolutionClaim,
        approved: bool,
        now: DateTime<Utc>,
    ) -> Result<Self, TaskGroupedApprovalError> {
        if claim.actor != TaskGroupedApprovalActor::User {
            return Err(TaskGroupedApprovalError::InvalidActor);
        }
        if self.status != TaskGroupedApprovalStatus::Pending
            || !self.binding_matches_resolution_claim(claim)
        {
            return Err(TaskGroupedApprovalError::BindingMismatch);
        }
        if now >= self.manifest.expires_at {
            return Err(TaskGroupedApprovalError::Expired);
        }
        let resolution = TaskGroupedApprovalResolution {
            id: resolution_id_for(
                self.id,
                approved,
                claim.expected_projection_revision,
                &claim.preview_hash,
            ),
            approved,
            source: TaskGroupedApprovalResolutionSource::User,
            expected_projection_revision: claim.expected_projection_revision,
            task_id: claim.task_id,
            manifest_revision: claim.manifest_revision.clone(),
            manifest_fingerprint: claim.manifest_fingerprint.clone(),
            preview_schema_revision: claim.preview_schema_revision,
            preview_renderer_revision: claim.preview_renderer_revision,
            preview_hash: claim.preview_hash.clone(),
            created_at: now,
        };
        self.transition(
            if approved {
                TaskGroupedApprovalStatus::Approved
            } else {
                TaskGroupedApprovalStatus::Rejected
            },
            Some(resolution),
            now,
        )
    }

    pub(crate) fn revoke(
        &self,
        actor: TaskGroupedApprovalActor,
        now: DateTime<Utc>,
    ) -> Result<Self, TaskGroupedApprovalError> {
        if !matches!(
            actor,
            TaskGroupedApprovalActor::User | TaskGroupedApprovalActor::KernelLifecycle
        ) {
            return Err(TaskGroupedApprovalError::InvalidActor);
        }
        if self.status != TaskGroupedApprovalStatus::Approved {
            return Err(TaskGroupedApprovalError::InvalidState);
        }
        self.transition(
            TaskGroupedApprovalStatus::Revoked,
            self.resolution.clone(),
            now,
        )
    }

    pub(crate) fn expire(&self, now: DateTime<Utc>) -> Result<Self, TaskGroupedApprovalError> {
        if now < self.manifest.expires_at
            || !matches!(
                self.status,
                TaskGroupedApprovalStatus::Pending | TaskGroupedApprovalStatus::Approved
            )
        {
            return Err(TaskGroupedApprovalError::InvalidState);
        }
        self.transition(
            TaskGroupedApprovalStatus::Expired,
            self.resolution.clone(),
            now,
        )
    }

    pub(crate) fn scope_changed(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Self, TaskGroupedApprovalError> {
        if !matches!(
            self.status,
            TaskGroupedApprovalStatus::Pending | TaskGroupedApprovalStatus::Approved
        ) {
            return Err(TaskGroupedApprovalError::InvalidState);
        }
        self.transition(
            TaskGroupedApprovalStatus::ScopeChanged,
            self.resolution.clone(),
            now,
        )
    }

    pub fn event_receipt(&self) -> TaskGroupedApprovalEventReceipt {
        TaskGroupedApprovalEventReceipt {
            version: TASK_GROUPED_APPROVAL_EVENT_VERSION.to_string(),
            group_id: self.id,
            task_id: self.task_id,
            status: self.status,
            projection_revision: self.projection_revision,
            manifest_revision: self.manifest.revision.clone(),
            manifest_fingerprint: self.manifest.fingerprint.clone(),
            preview_renderer_revision: self.preview.renderer_revision,
            preview_hash: self.preview.preview_hash.clone(),
            capability_audit_ids: self
                .capability_audits
                .iter()
                .map(|item| item.item_id.clone())
                .collect(),
            created_at: self.updated_at,
        }
    }

    fn transition(
        &self,
        status: TaskGroupedApprovalStatus,
        resolution: Option<TaskGroupedApprovalResolution>,
        now: DateTime<Utc>,
    ) -> Result<Self, TaskGroupedApprovalError> {
        let mut next = self.clone();
        next.status = status;
        next.projection_revision = next
            .projection_revision
            .checked_add(1)
            .ok_or(TaskGroupedApprovalError::InvalidState)?;
        next.resolution = resolution;
        next.updated_at = now;
        for item in &mut next.capability_audits {
            item.status = status.into();
        }
        next.integrity_hash = integrity_hash_for(&next);
        next.validate_integrity()?;
        Ok(next)
    }
}

fn validate_resolution(
    group: &TaskGroupedApproval,
    resolution: &TaskGroupedApprovalResolution,
) -> Result<(), TaskGroupedApprovalError> {
    if resolution.source != TaskGroupedApprovalResolutionSource::User
        || resolution.task_id != group.task_id
        || resolution.manifest_revision != group.manifest.revision
        || resolution.manifest_fingerprint != group.manifest.fingerprint
        || resolution.preview_schema_revision != group.preview.schema_revision
        || resolution.preview_renderer_revision != group.preview.renderer_revision
        || resolution.preview_hash != group.preview.preview_hash
        || resolution.expected_projection_revision != 0
        || resolution.id
            != resolution_id_for(
                group.id,
                resolution.approved,
                resolution.expected_projection_revision,
                &resolution.preview_hash,
            )
        || resolution.created_at < group.created_at
        || resolution.created_at >= group.manifest.expires_at
        || resolution.created_at > group.updated_at
    {
        return Err(TaskGroupedApprovalError::BindingMismatch);
    }
    Ok(())
}

pub(crate) fn group_id_for(
    manifest: &TaskCapabilityManifest,
    preview: &TaskAuthorizationPreview,
) -> Uuid {
    let canonical = GroupIdCanonical {
        task_id: manifest.task_id,
        goal_revision: &manifest.goal_revision,
        goal_fingerprint: &manifest.goal_fingerprint,
        manifest_revision: &manifest.revision,
        manifest_fingerprint: &manifest.fingerprint,
        preview_schema_revision: preview.schema_revision,
        preview_renderer_revision: preview.renderer_revision,
        preview_hash: &preview.preview_hash,
        expires_at: manifest.expires_at,
    };
    deterministic_uuid(
        GROUP_ID_DOMAIN,
        &serde_json::to_vec(&canonical).unwrap_or_default(),
    )
}

pub(crate) fn event_id_for(group: &TaskGroupedApproval) -> Uuid {
    deterministic_uuid(
        EVENT_ID_DOMAIN,
        format!(
            "{}\0{}\0{}",
            group.id,
            group.status.as_str(),
            group.projection_revision
        )
        .as_bytes(),
    )
}

pub(crate) fn item_event_id_for(
    group: &TaskGroupedApproval,
    item: &TaskGroupedCapabilityAudit,
) -> Uuid {
    deterministic_uuid(
        ITEM_EVENT_ID_DOMAIN,
        format!(
            "{}\0{}\0{}\0{}",
            group.id,
            item.item_id,
            group.status.as_str(),
            group.projection_revision
        )
        .as_bytes(),
    )
}

pub(crate) fn capability_request_event_id_for(group_id: Uuid, item_id: &str) -> Uuid {
    deterministic_uuid(
        EVENT_ID_DOMAIN,
        format!("capability-request\0{group_id}\0{item_id}").as_bytes(),
    )
}

pub(crate) fn permission_resolution_event_id_for(resolution_id: Uuid) -> Uuid {
    deterministic_uuid(
        EVENT_ID_DOMAIN,
        format!("permission-resolution\0{resolution_id}").as_bytes(),
    )
}

pub(crate) fn permission_resolution_id_for(group_id: Uuid, item_id: &str, approved: bool) -> Uuid {
    deterministic_uuid(
        RESOLUTION_ID_DOMAIN,
        format!("item\0{group_id}\0{item_id}\0{approved}").as_bytes(),
    )
}

pub(crate) fn legacy_consumption_id_for(group_id: Uuid, item_id: &str) -> Uuid {
    deterministic_uuid(
        LEGACY_CONSUMPTION_ID_DOMAIN,
        format!("{group_id}\0{item_id}").as_bytes(),
    )
}

fn item_id_for(
    group_id: Uuid,
    capability: CapabilityKind,
    tool_id: &str,
    tool_version: &str,
) -> String {
    domain_hash(
        ITEM_ID_DOMAIN,
        &serde_json::to_vec(&ItemCanonical {
            group_id,
            capability: capability.as_str(),
            tool_id,
            tool_version,
        })
        .unwrap_or_default(),
    )
}

fn approval_request_id_for(group_id: Uuid, item_id: &str) -> Uuid {
    deterministic_uuid(
        REQUEST_ID_DOMAIN,
        format!("{group_id}\0{item_id}").as_bytes(),
    )
}

fn request_fingerprint_for(
    group_id: Uuid,
    manifest: &TaskCapabilityManifest,
    preview: &TaskAuthorizationPreview,
    capability: CapabilityKind,
    tool_id: &str,
    tool_version: &str,
) -> String {
    domain_hash(
        REQUEST_FINGERPRINT_DOMAIN,
        &serde_json::to_vec(&RequestFingerprintCanonical {
            group_id,
            task_id: manifest.task_id,
            manifest_revision: &manifest.revision,
            manifest_fingerprint: &manifest.fingerprint,
            preview_schema_revision: preview.schema_revision,
            preview_renderer_revision: preview.renderer_revision,
            preview_hash: &preview.preview_hash,
            capability: capability.as_str(),
            tool_id,
            tool_version,
            expires_at: manifest.expires_at,
        })
        .unwrap_or_default(),
    )
}

fn compact_exact_preview(
    manifest: &TaskCapabilityManifest,
    preview: &TaskAuthorizationPreview,
    capability: CapabilityKind,
    tool_id: &str,
) -> String {
    format!(
        "task={};manifest={}:{};preview={}/{}:{};capability={};tool={};expires={}",
        manifest.task_id,
        manifest.revision,
        manifest.fingerprint,
        preview.schema_revision,
        preview.renderer_revision,
        preview.preview_hash,
        capability.as_str(),
        tool_id,
        manifest.expires_at.to_rfc3339(),
    )
}

fn resolution_id_for(
    group_id: Uuid,
    approved: bool,
    expected_projection_revision: u64,
    preview_hash: &str,
) -> Uuid {
    deterministic_uuid(
        RESOLUTION_ID_DOMAIN,
        format!("group\0{group_id}\0{approved}\0{expected_projection_revision}\0{preview_hash}")
            .as_bytes(),
    )
}

fn integrity_hash_for(group: &TaskGroupedApproval) -> String {
    domain_hash(
        GROUP_INTEGRITY_DOMAIN,
        &serde_json::to_vec(&GroupIntegrityCanonical {
            version: &group.version,
            id: group.id,
            task_id: group.task_id,
            manifest: &group.manifest,
            preview: &group.preview,
            status: group.status,
            projection_revision: group.projection_revision,
            capability_audits: &group.capability_audits,
            resolution: &group.resolution,
            created_at: group.created_at,
            updated_at: group.updated_at,
        })
        .unwrap_or_default(),
    )
}

fn domain_hash(domain: &[u8], value: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
    format!("{:x}", digest.finalize())
}

fn deterministic_uuid(domain: &[u8], value: &[u8]) -> Uuid {
    let digest = Sha256::digest([domain, value].concat());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}
