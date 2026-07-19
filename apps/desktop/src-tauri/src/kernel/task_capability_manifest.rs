use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::kernel::goal_lifecycle::{GoalLifecycleProjection, GoalTargetBindingKind};
use crate::kernel::policy::{
    builtin_capability_catalog, capability_risk, CapabilityDescriptor, CapabilityKind, RiskLevel,
};
use crate::kernel::tool_runtime::{builtin_tool_catalog, ToolContract};

pub const TASK_CAPABILITY_MANIFEST_VERSION: &str = "ds-agent.task-capability-manifest/v1";
pub const TASK_CAPABILITY_PROPOSAL_VERSION: &str = "ds-agent.task-capability-proposal/v1";
pub const TASK_CAPABILITY_MANIFEST_SCHEMA_REVISION: u32 = 1;
pub const TASK_AUTHORIZATION_PREVIEW_VERSION: &str = "ds-agent.task-authorization-preview/v1";
pub const TASK_AUTHORIZATION_PREVIEW_SCHEMA_REVISION: u32 = 1;
pub const TASK_AUTHORIZATION_PREVIEW_RENDERER_REVISION: u32 = 1;

const MAX_JSON_BYTES: usize = 64 * 1024;
const MAX_CAPABILITIES: usize = 32;
const MAX_ITEMS_PER_FIELD: usize = 64;
const MAX_ID_BYTES: usize = 128;
const MAX_DISPLAY_BYTES: usize = 512;
const MANIFEST_REVISION_DOMAIN: &[u8] = b"ds-agent.task-capability-manifest-revision.v1\0";
const MANIFEST_FINGERPRINT_DOMAIN: &[u8] = b"ds-agent.task-capability-manifest-fingerprint.v1\0";
const PREVIEW_HASH_DOMAIN: &[u8] = b"ds-agent.task-authorization-preview-hash.v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskCapabilityManifestError {
    JsonTooLarge,
    InvalidJson,
    UnsupportedVersion,
    InvalidIdentifier,
    InvalidBindingHash,
    InvalidDisplay,
    SecretLikeContent,
    CollectionOutOfBounds,
    DuplicateValue,
    NonCanonicalOrder,
    GoalNotFrozen,
    TaskGoalIdentityMismatch,
    GoalRevisionMismatch,
    GoalFingerprintMismatch,
    UnknownCapability,
    UnknownTool,
    ToolCapabilityMismatch,
    CatalogRiskMismatch,
    GoalCapabilityMismatch,
    ScopeBindingMismatch,
    UnknownApplication,
    UnknownVerifier,
    IncompleteGoalBinding,
    ManifestIntegrityMismatch,
    PreviewIntegrityMismatch,
}

impl fmt::Display for TaskCapabilityManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::JsonTooLarge => "task capability manifest json is too large",
            Self::InvalidJson => "task capability manifest json is invalid",
            Self::UnsupportedVersion => "task capability manifest version is unsupported",
            Self::InvalidIdentifier => "task capability manifest contains an invalid identifier",
            Self::InvalidBindingHash => "task capability manifest contains an invalid binding hash",
            Self::InvalidDisplay => "task capability manifest contains invalid display text",
            Self::SecretLikeContent => "task capability manifest contains secret-like content",
            Self::CollectionOutOfBounds => {
                "task capability manifest collection is outside its bounds"
            }
            Self::DuplicateValue => "task capability manifest contains a duplicate value",
            Self::NonCanonicalOrder => "task capability manifest is not canonically ordered",
            Self::GoalNotFrozen => "task capability manifest requires a frozen goal",
            Self::TaskGoalIdentityMismatch => {
                "task capability manifest task or goal identity is stale"
            }
            Self::GoalRevisionMismatch => "task capability manifest goal revision is stale",
            Self::GoalFingerprintMismatch => "task capability manifest goal fingerprint is stale",
            Self::UnknownCapability => "task capability manifest capability is unknown",
            Self::UnknownTool => "task capability manifest tool is unknown",
            Self::ToolCapabilityMismatch => {
                "task capability manifest tool and capability do not match"
            }
            Self::CatalogRiskMismatch => "task capability manifest catalog risk is inconsistent",
            Self::GoalCapabilityMismatch => {
                "task capability manifest exceeds or omits the frozen goal capability scope"
            }
            Self::ScopeBindingMismatch => "task capability manifest scope binding is stale",
            Self::UnknownApplication => "task capability manifest application is not locally bound",
            Self::UnknownVerifier => "task capability manifest verifier is not frozen in the goal",
            Self::IncompleteGoalBinding => {
                "task capability manifest does not bind the complete frozen goal"
            }
            Self::ManifestIntegrityMismatch => "task capability manifest integrity check failed",
            Self::PreviewIntegrityMismatch => "task authorization preview integrity check failed",
        })
    }
}

impl std::error::Error for TaskCapabilityManifestError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCapabilityNeedProposal {
    pub capability: String,
    pub tool_ids: Vec<String>,
    pub application_ids: Vec<String>,
    pub path_target_ids: Vec<String>,
    pub account_target_ids: Vec<String>,
    pub recipient_target_ids: Vec<String>,
    pub time_window_target_ids: Vec<String>,
    pub external_target_ids: Vec<String>,
    pub verifier_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCapabilityDescriptionProposal {
    pub capability: String,
    pub application_ids: Vec<String>,
    pub path_target_ids: Vec<String>,
    pub account_target_ids: Vec<String>,
    pub recipient_target_ids: Vec<String>,
    pub time_window_target_ids: Vec<String>,
    pub external_target_ids: Vec<String>,
    pub verifier_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskCapabilityProposal {
    pub version: String,
    pub expires_at: DateTime<Utc>,
    pub capabilities: Vec<TaskCapabilityDescriptionProposal>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskCapabilityProposalWire {
    version: String,
    expires_at: DateTime<Utc>,
    capabilities: Vec<TaskCapabilityDescriptionProposal>,
}

impl From<TaskCapabilityProposalWire> for TaskCapabilityProposal {
    fn from(wire: TaskCapabilityProposalWire) -> Self {
        Self {
            version: wire.version,
            expires_at: wire.expires_at,
            capabilities: wire.capabilities,
        }
    }
}

impl<'de> Deserialize<'de> for TaskCapabilityProposal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let proposal = Self::from(TaskCapabilityProposalWire::deserialize(deserializer)?);
        proposal.validate().map_err(serde::de::Error::custom)?;
        Ok(proposal)
    }
}

impl TaskCapabilityProposal {
    pub fn parse_json(json: &str) -> Result<Self, TaskCapabilityManifestError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(TaskCapabilityManifestError::JsonTooLarge);
        }
        let wire: TaskCapabilityProposalWire =
            serde_json::from_str(json).map_err(|_| TaskCapabilityManifestError::InvalidJson)?;
        let proposal = Self::from(wire);
        proposal.validate()?;
        Ok(proposal)
    }

    pub fn parse_value(value: Value) -> Result<Self, TaskCapabilityManifestError> {
        let encoded =
            serde_json::to_vec(&value).map_err(|_| TaskCapabilityManifestError::InvalidJson)?;
        if encoded.len() > MAX_JSON_BYTES {
            return Err(TaskCapabilityManifestError::JsonTooLarge);
        }
        let wire: TaskCapabilityProposalWire =
            serde_json::from_value(value).map_err(|_| TaskCapabilityManifestError::InvalidJson)?;
        let proposal = Self::from(wire);
        proposal.validate()?;
        Ok(proposal)
    }

    pub fn to_json(&self) -> Result<String, TaskCapabilityManifestError> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| TaskCapabilityManifestError::InvalidJson)
    }

    pub fn validate(&self) -> Result<(), TaskCapabilityManifestError> {
        if self.version != TASK_CAPABILITY_PROPOSAL_VERSION {
            return Err(TaskCapabilityManifestError::UnsupportedVersion);
        }
        if self.capabilities.is_empty() || self.capabilities.len() > MAX_CAPABILITIES {
            return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
        }
        validate_strict_order(
            self.capabilities
                .iter()
                .map(|entry| entry.capability.as_str()),
        )?;
        let capability_catalog = builtin_capability_catalog();
        for entry in &self.capabilities {
            validate_id(&entry.capability)?;
            if capability_from_name(&capability_catalog, &entry.capability).is_none() {
                return Err(TaskCapabilityManifestError::UnknownCapability);
            }
            validate_id_list(&entry.application_ids)?;
            validate_id_list(&entry.path_target_ids)?;
            validate_id_list(&entry.account_target_ids)?;
            validate_id_list(&entry.recipient_target_ids)?;
            validate_id_list(&entry.time_window_target_ids)?;
            validate_id_list(&entry.external_target_ids)?;
            validate_nonempty_id_list(&entry.verifier_ids)?;
            for target_id in entry
                .path_target_ids
                .iter()
                .chain(&entry.account_target_ids)
                .chain(&entry.recipient_target_ids)
                .chain(&entry.time_window_target_ids)
            {
                if entry.external_target_ids.binary_search(target_id).is_err() {
                    return Err(TaskCapabilityManifestError::IncompleteGoalBinding);
                }
            }
        }
        let encoded =
            serde_json::to_vec(self).map_err(|_| TaskCapabilityManifestError::InvalidJson)?;
        if encoded.len() > MAX_JSON_BYTES {
            return Err(TaskCapabilityManifestError::JsonTooLarge);
        }
        Ok(())
    }

    pub fn bind_to_frozen_goal(
        &self,
        canonical_task_id: Uuid,
        goal: &GoalLifecycleProjection,
    ) -> Result<TaskCapabilityManifestProposal, TaskCapabilityManifestError> {
        self.validate()?;
        let frozen = goal
            .frozen()
            .ok_or(TaskCapabilityManifestError::GoalNotFrozen)?;
        if canonical_task_id != goal.goal_id {
            return Err(TaskCapabilityManifestError::TaskGoalIdentityMismatch);
        }

        let mut capabilities = Vec::with_capacity(self.capabilities.len());
        for entry in &self.capabilities {
            let mut tool_ids = frozen
                .envelope
                .validated_capabilities
                .iter()
                .filter(|binding| binding.capability == entry.capability)
                .map(|binding| binding.tool_id.clone())
                .collect::<Vec<_>>();
            tool_ids.sort();
            if tool_ids.is_empty() {
                return Err(TaskCapabilityManifestError::GoalCapabilityMismatch);
            }
            capabilities.push(TaskCapabilityNeedProposal {
                capability: entry.capability.clone(),
                tool_ids,
                application_ids: entry.application_ids.clone(),
                path_target_ids: entry.path_target_ids.clone(),
                account_target_ids: entry.account_target_ids.clone(),
                recipient_target_ids: entry.recipient_target_ids.clone(),
                time_window_target_ids: entry.time_window_target_ids.clone(),
                external_target_ids: entry.external_target_ids.clone(),
                verifier_ids: entry.verifier_ids.clone(),
            });
        }

        let proposal = TaskCapabilityManifestProposal {
            version: TASK_CAPABILITY_MANIFEST_VERSION.to_string(),
            task_id: canonical_task_id,
            goal_id: goal.goal_id,
            goal_revision: frozen.revision.clone(),
            goal_fingerprint: frozen.fingerprint.clone(),
            expires_at: self.expires_at,
            capabilities,
        };
        proposal.validate()?;
        Ok(proposal)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskCapabilityManifestProposal {
    pub version: String,
    pub task_id: Uuid,
    pub goal_id: Uuid,
    pub goal_revision: String,
    pub goal_fingerprint: String,
    pub expires_at: DateTime<Utc>,
    pub capabilities: Vec<TaskCapabilityNeedProposal>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskCapabilityManifestProposalWire {
    version: String,
    task_id: Uuid,
    goal_id: Uuid,
    goal_revision: String,
    goal_fingerprint: String,
    expires_at: DateTime<Utc>,
    capabilities: Vec<TaskCapabilityNeedProposal>,
}

impl From<TaskCapabilityManifestProposalWire> for TaskCapabilityManifestProposal {
    fn from(wire: TaskCapabilityManifestProposalWire) -> Self {
        Self {
            version: wire.version,
            task_id: wire.task_id,
            goal_id: wire.goal_id,
            goal_revision: wire.goal_revision,
            goal_fingerprint: wire.goal_fingerprint,
            expires_at: wire.expires_at,
            capabilities: wire.capabilities,
        }
    }
}

impl<'de> Deserialize<'de> for TaskCapabilityManifestProposal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let proposal = Self::from(TaskCapabilityManifestProposalWire::deserialize(
            deserializer,
        )?);
        proposal.validate().map_err(serde::de::Error::custom)?;
        Ok(proposal)
    }
}

impl TaskCapabilityManifestProposal {
    pub fn parse_json(json: &str) -> Result<Self, TaskCapabilityManifestError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(TaskCapabilityManifestError::JsonTooLarge);
        }
        let wire: TaskCapabilityManifestProposalWire =
            serde_json::from_str(json).map_err(|_| TaskCapabilityManifestError::InvalidJson)?;
        let proposal = Self::from(wire);
        proposal.validate()?;
        Ok(proposal)
    }

    pub fn parse_value(value: Value) -> Result<Self, TaskCapabilityManifestError> {
        let encoded =
            serde_json::to_vec(&value).map_err(|_| TaskCapabilityManifestError::InvalidJson)?;
        if encoded.len() > MAX_JSON_BYTES {
            return Err(TaskCapabilityManifestError::JsonTooLarge);
        }
        let wire: TaskCapabilityManifestProposalWire =
            serde_json::from_value(value).map_err(|_| TaskCapabilityManifestError::InvalidJson)?;
        let proposal = Self::from(wire);
        proposal.validate()?;
        Ok(proposal)
    }

    pub fn to_json(&self) -> Result<String, TaskCapabilityManifestError> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| TaskCapabilityManifestError::InvalidJson)
    }

    pub fn validate(&self) -> Result<(), TaskCapabilityManifestError> {
        if self.version != TASK_CAPABILITY_MANIFEST_VERSION {
            return Err(TaskCapabilityManifestError::UnsupportedVersion);
        }
        if self.task_id != self.goal_id {
            return Err(TaskCapabilityManifestError::TaskGoalIdentityMismatch);
        }
        validate_hash(&self.goal_revision)?;
        validate_hash(&self.goal_fingerprint)?;
        if self.capabilities.is_empty() || self.capabilities.len() > MAX_CAPABILITIES {
            return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
        }

        validate_strict_order(
            self.capabilities
                .iter()
                .map(|entry| entry.capability.as_str()),
        )?;
        let capability_catalog = builtin_capability_catalog();
        for entry in &self.capabilities {
            validate_id(&entry.capability)?;
            if capability_from_name(&capability_catalog, &entry.capability).is_none() {
                return Err(TaskCapabilityManifestError::UnknownCapability);
            }
            validate_nonempty_id_list(&entry.tool_ids)?;
            validate_id_list(&entry.application_ids)?;
            validate_id_list(&entry.path_target_ids)?;
            validate_id_list(&entry.account_target_ids)?;
            validate_id_list(&entry.recipient_target_ids)?;
            validate_id_list(&entry.time_window_target_ids)?;
            validate_id_list(&entry.external_target_ids)?;
            validate_nonempty_id_list(&entry.verifier_ids)?;
            for target_id in entry
                .path_target_ids
                .iter()
                .chain(&entry.account_target_ids)
                .chain(&entry.recipient_target_ids)
                .chain(&entry.time_window_target_ids)
            {
                if entry.external_target_ids.binary_search(target_id).is_err() {
                    return Err(TaskCapabilityManifestError::IncompleteGoalBinding);
                }
            }
        }
        let encoded =
            serde_json::to_vec(self).map_err(|_| TaskCapabilityManifestError::InvalidJson)?;
        if encoded.len() > MAX_JSON_BYTES {
            return Err(TaskCapabilityManifestError::JsonTooLarge);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct TaskCapabilityManifestContext {
    application_labels: BTreeMap<String, String>,
    target_labels: BTreeMap<String, String>,
}

impl TaskCapabilityManifestContext {
    pub fn with_application(
        mut self,
        application_id: impl Into<String>,
        display_label: impl Into<String>,
    ) -> Self {
        self.application_labels
            .insert(application_id.into(), display_label.into());
        self
    }

    pub fn with_target_display(
        mut self,
        target_id: impl Into<String>,
        display_label: impl Into<String>,
    ) -> Self {
        self.target_labels
            .insert(target_id.into(), display_label.into());
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskToolBinding {
    pub tool_id: String,
    pub tool_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskApplicationBinding {
    pub application_id: String,
    pub display_label: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskScopeBinding {
    pub target_id: String,
    pub binding_kind: GoalTargetBindingKind,
    pub display_label: String,
    pub authority_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskVerifierSummary {
    pub verifier_id: String,
    pub evidence_kind: String,
    pub summary: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCapabilityManifestEntry {
    pub capability: CapabilityKind,
    pub risk_level: RiskLevel,
    pub tools: Vec<TaskToolBinding>,
    pub applications: Vec<TaskApplicationBinding>,
    pub path_scopes: Vec<TaskScopeBinding>,
    pub account_scopes: Vec<TaskScopeBinding>,
    pub recipient_scopes: Vec<TaskScopeBinding>,
    pub time_windows: Vec<TaskScopeBinding>,
    pub external_targets: Vec<TaskScopeBinding>,
    pub verifiers: Vec<TaskVerifierSummary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCapabilityManifest {
    pub version: String,
    pub schema_revision: u32,
    pub task_id: Uuid,
    pub goal_id: Uuid,
    pub goal_revision: String,
    pub goal_fingerprint: String,
    pub expires_at: DateTime<Utc>,
    pub aggregate_risk: RiskLevel,
    pub capabilities: Vec<TaskCapabilityManifestEntry>,
    pub revision: String,
    pub fingerprint: String,
}

impl TaskCapabilityManifest {
    pub fn parse_json(json: &str) -> Result<Self, TaskCapabilityManifestError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(TaskCapabilityManifestError::JsonTooLarge);
        }
        let manifest: Self =
            serde_json::from_str(json).map_err(|_| TaskCapabilityManifestError::InvalidJson)?;
        manifest.validate_integrity()?;
        Ok(manifest)
    }

    pub fn canonical_json(&self) -> Result<String, TaskCapabilityManifestError> {
        self.validate_integrity()?;
        serde_json::to_string(self).map_err(|_| TaskCapabilityManifestError::InvalidJson)
    }

    pub fn validate_integrity(&self) -> Result<(), TaskCapabilityManifestError> {
        if self.version != TASK_CAPABILITY_MANIFEST_VERSION
            || self.schema_revision != TASK_CAPABILITY_MANIFEST_SCHEMA_REVISION
        {
            return Err(TaskCapabilityManifestError::UnsupportedVersion);
        }
        if self.task_id != self.goal_id {
            return Err(TaskCapabilityManifestError::TaskGoalIdentityMismatch);
        }
        validate_hash(&self.goal_revision)?;
        validate_hash(&self.goal_fingerprint)?;
        validate_hash(&self.revision)?;
        validate_hash(&self.fingerprint)?;
        if self.capabilities.is_empty() || self.capabilities.len() > MAX_CAPABILITIES {
            return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
        }
        validate_strict_order(
            self.capabilities
                .iter()
                .map(|entry| entry.capability.as_str()),
        )?;
        let tool_catalog = builtin_tool_catalog()
            .into_iter()
            .map(|tool| (tool.id.clone(), tool))
            .collect::<BTreeMap<_, _>>();
        let capability_catalog = builtin_capability_catalog();
        let mut aggregate = RiskLevel::Low;
        let mut seen_tools = BTreeSet::new();
        for entry in &self.capabilities {
            let descriptor = capability_catalog
                .iter()
                .find(|descriptor| descriptor.capability == entry.capability)
                .ok_or(TaskCapabilityManifestError::UnknownCapability)?;
            if entry.risk_level != capability_risk(entry.capability)
                || entry.risk_level != descriptor.risk_level
            {
                return Err(TaskCapabilityManifestError::CatalogRiskMismatch);
            }
            if risk_rank(entry.risk_level) > risk_rank(aggregate) {
                aggregate = entry.risk_level;
            }
            validate_tool_bindings(&entry.tools)?;
            for tool in &entry.tools {
                let contract = tool_catalog
                    .get(&tool.tool_id)
                    .ok_or(TaskCapabilityManifestError::UnknownTool)?;
                if contract.version != tool.tool_version
                    || contract.capability != entry.capability
                    || contract.risk_level != entry.risk_level
                {
                    return Err(TaskCapabilityManifestError::ToolCapabilityMismatch);
                }
                if !seen_tools.insert(tool.tool_id.as_str()) {
                    return Err(TaskCapabilityManifestError::DuplicateValue);
                }
            }
            validate_application_bindings(&entry.applications)?;
            validate_scope_bindings(&entry.path_scopes, scope_is_path)?;
            validate_scope_bindings(&entry.account_scopes, |kind| {
                kind == GoalTargetBindingKind::Account
            })?;
            validate_scope_bindings(&entry.recipient_scopes, |kind| {
                kind == GoalTargetBindingKind::Recipient
            })?;
            validate_scope_bindings(&entry.time_windows, |kind| {
                kind == GoalTargetBindingKind::TimeWindow
            })?;
            validate_scope_bindings(&entry.external_targets, |_| true)?;
            validate_verifier_summaries(&entry.verifiers)?;
            for scope in entry
                .path_scopes
                .iter()
                .chain(&entry.account_scopes)
                .chain(&entry.recipient_scopes)
                .chain(&entry.time_windows)
            {
                if !entry.external_targets.contains(scope) {
                    return Err(TaskCapabilityManifestError::IncompleteGoalBinding);
                }
            }
        }
        if aggregate != self.aggregate_risk {
            return Err(TaskCapabilityManifestError::CatalogRiskMismatch);
        }
        if self.revision != manifest_revision_for(self)
            || self.fingerprint != manifest_fingerprint_for(self)
        {
            return Err(TaskCapabilityManifestError::ManifestIntegrityMismatch);
        }
        Ok(())
    }

    pub fn validate_for_goal(
        &self,
        goal: &GoalLifecycleProjection,
    ) -> Result<(), TaskCapabilityManifestError> {
        validate_manifest_against_goal(
            self,
            goal,
            &builtin_tool_catalog(),
            &builtin_capability_catalog(),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskPreviewLabel {
    pub id: String,
    pub display_label: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCapabilityPreviewEntry {
    pub capability: CapabilityKind,
    pub risk_level: RiskLevel,
    pub applications: Vec<TaskPreviewLabel>,
    pub path_scopes: Vec<TaskPreviewLabel>,
    pub account_scopes: Vec<TaskPreviewLabel>,
    pub recipient_scopes: Vec<TaskPreviewLabel>,
    pub time_windows: Vec<TaskPreviewLabel>,
    pub external_targets: Vec<TaskPreviewLabel>,
    pub verifiers: Vec<TaskVerifierSummary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskAuthorizationPreview {
    pub version: String,
    pub schema_revision: u32,
    pub renderer_revision: u32,
    pub task_id: Uuid,
    pub goal_id: Uuid,
    pub goal_revision: String,
    pub goal_fingerprint: String,
    pub manifest_revision: String,
    pub manifest_fingerprint: String,
    pub expires_at: DateTime<Utc>,
    pub aggregate_risk: RiskLevel,
    pub capabilities: Vec<TaskCapabilityPreviewEntry>,
    pub preview_hash: String,
}

impl TaskAuthorizationPreview {
    pub fn parse_json(json: &str) -> Result<Self, TaskCapabilityManifestError> {
        if json.len() > MAX_JSON_BYTES {
            return Err(TaskCapabilityManifestError::JsonTooLarge);
        }
        let preview: Self =
            serde_json::from_str(json).map_err(|_| TaskCapabilityManifestError::InvalidJson)?;
        preview.validate_integrity()?;
        Ok(preview)
    }

    pub fn canonical_json(&self) -> Result<String, TaskCapabilityManifestError> {
        self.validate_integrity()?;
        serde_json::to_string(self).map_err(|_| TaskCapabilityManifestError::InvalidJson)
    }

    pub fn validate_integrity(&self) -> Result<(), TaskCapabilityManifestError> {
        if self.version != TASK_AUTHORIZATION_PREVIEW_VERSION
            || self.schema_revision != TASK_AUTHORIZATION_PREVIEW_SCHEMA_REVISION
            || self.renderer_revision != TASK_AUTHORIZATION_PREVIEW_RENDERER_REVISION
        {
            return Err(TaskCapabilityManifestError::UnsupportedVersion);
        }
        if self.task_id != self.goal_id {
            return Err(TaskCapabilityManifestError::TaskGoalIdentityMismatch);
        }
        for hash in [
            &self.goal_revision,
            &self.goal_fingerprint,
            &self.manifest_revision,
            &self.manifest_fingerprint,
            &self.preview_hash,
        ] {
            validate_hash(hash)?;
        }
        if self.capabilities.is_empty() || self.capabilities.len() > MAX_CAPABILITIES {
            return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
        }
        validate_strict_order(
            self.capabilities
                .iter()
                .map(|entry| entry.capability.as_str()),
        )?;
        let mut aggregate = RiskLevel::Low;
        for entry in &self.capabilities {
            if entry.risk_level != capability_risk(entry.capability) {
                return Err(TaskCapabilityManifestError::CatalogRiskMismatch);
            }
            if risk_rank(entry.risk_level) > risk_rank(aggregate) {
                aggregate = entry.risk_level;
            }
            validate_preview_labels(&entry.applications)?;
            validate_preview_labels(&entry.path_scopes)?;
            validate_preview_labels(&entry.account_scopes)?;
            validate_preview_labels(&entry.recipient_scopes)?;
            validate_preview_labels(&entry.time_windows)?;
            validate_preview_labels(&entry.external_targets)?;
            validate_verifier_summaries(&entry.verifiers)?;
        }
        if self.aggregate_risk != aggregate {
            return Err(TaskCapabilityManifestError::CatalogRiskMismatch);
        }
        if self.preview_hash != preview_hash_for(self) {
            return Err(TaskCapabilityManifestError::PreviewIntegrityMismatch);
        }
        Ok(())
    }

    pub fn validate_for_manifest(
        &self,
        manifest: &TaskCapabilityManifest,
    ) -> Result<(), TaskCapabilityManifestError> {
        let expected = task_authorization_preview(manifest)?;
        if *self != expected {
            return Err(TaskCapabilityManifestError::PreviewIntegrityMismatch);
        }
        Ok(())
    }
}

pub fn compile_task_capability_manifest(
    canonical_task_id: Uuid,
    goal: &GoalLifecycleProjection,
    proposal: &TaskCapabilityManifestProposal,
    context: &TaskCapabilityManifestContext,
) -> Result<TaskCapabilityManifest, TaskCapabilityManifestError> {
    let tool_catalog = builtin_tool_catalog();
    let capability_catalog = builtin_capability_catalog();
    compile_with_catalogs(
        canonical_task_id,
        goal,
        proposal,
        context,
        &tool_catalog,
        &capability_catalog,
    )
}

pub fn task_authorization_preview(
    manifest: &TaskCapabilityManifest,
) -> Result<TaskAuthorizationPreview, TaskCapabilityManifestError> {
    manifest.validate_integrity()?;
    let capabilities = manifest
        .capabilities
        .iter()
        .map(|entry| TaskCapabilityPreviewEntry {
            capability: entry.capability,
            risk_level: entry.risk_level,
            applications: entry
                .applications
                .iter()
                .map(|binding| TaskPreviewLabel {
                    id: binding.application_id.clone(),
                    display_label: binding.display_label.clone(),
                })
                .collect(),
            path_scopes: preview_scope_labels(&entry.path_scopes),
            account_scopes: preview_scope_labels(&entry.account_scopes),
            recipient_scopes: preview_scope_labels(&entry.recipient_scopes),
            time_windows: preview_scope_labels(&entry.time_windows),
            external_targets: preview_scope_labels(&entry.external_targets),
            verifiers: entry.verifiers.clone(),
        })
        .collect();
    let mut preview = TaskAuthorizationPreview {
        version: TASK_AUTHORIZATION_PREVIEW_VERSION.to_string(),
        schema_revision: TASK_AUTHORIZATION_PREVIEW_SCHEMA_REVISION,
        renderer_revision: TASK_AUTHORIZATION_PREVIEW_RENDERER_REVISION,
        task_id: manifest.task_id,
        goal_id: manifest.goal_id,
        goal_revision: manifest.goal_revision.clone(),
        goal_fingerprint: manifest.goal_fingerprint.clone(),
        manifest_revision: manifest.revision.clone(),
        manifest_fingerprint: manifest.fingerprint.clone(),
        expires_at: manifest.expires_at,
        aggregate_risk: manifest.aggregate_risk,
        capabilities,
        preview_hash: String::new(),
    };
    preview.preview_hash = preview_hash_for(&preview);
    Ok(preview)
}

fn compile_with_catalogs(
    canonical_task_id: Uuid,
    goal: &GoalLifecycleProjection,
    proposal: &TaskCapabilityManifestProposal,
    context: &TaskCapabilityManifestContext,
    tool_catalog: &[ToolContract],
    capability_catalog: &[CapabilityDescriptor],
) -> Result<TaskCapabilityManifest, TaskCapabilityManifestError> {
    proposal.validate()?;
    let frozen = goal
        .frozen()
        .ok_or(TaskCapabilityManifestError::GoalNotFrozen)?;
    if canonical_task_id != goal.goal_id
        || proposal.task_id != canonical_task_id
        || proposal.goal_id != goal.goal_id
    {
        return Err(TaskCapabilityManifestError::TaskGoalIdentityMismatch);
    }
    if proposal.goal_revision != frozen.revision {
        return Err(TaskCapabilityManifestError::GoalRevisionMismatch);
    }
    if proposal.goal_fingerprint != frozen.fingerprint {
        return Err(TaskCapabilityManifestError::GoalFingerprintMismatch);
    }

    let goal_capabilities = frozen
        .envelope
        .validated_capabilities
        .iter()
        .map(|capability| (capability.tool_id.as_str(), capability))
        .collect::<BTreeMap<_, _>>();
    let goal_targets = frozen
        .envelope
        .bound_targets
        .iter()
        .map(|target| (target.target_id.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let goal_verifiers = frozen
        .envelope
        .verifiers
        .iter()
        .map(|verifier| (verifier.verifier_id.as_str(), verifier))
        .collect::<BTreeMap<_, _>>();
    let tools = tool_catalog
        .iter()
        .map(|tool| (tool.id.as_str(), tool))
        .collect::<BTreeMap<_, _>>();
    let mut covered_tools = BTreeSet::new();
    let mut covered_targets = BTreeSet::new();
    let mut covered_verifiers = BTreeSet::new();
    let mut aggregate_risk = RiskLevel::Low;
    let mut capabilities = Vec::with_capacity(proposal.capabilities.len());

    for entry in &proposal.capabilities {
        let capability = capability_from_name(capability_catalog, &entry.capability)
            .ok_or(TaskCapabilityManifestError::UnknownCapability)?;
        let descriptor = capability_catalog
            .iter()
            .find(|descriptor| descriptor.capability == capability)
            .ok_or(TaskCapabilityManifestError::UnknownCapability)?;
        let risk_level = capability_risk(capability);
        if descriptor.risk_level != risk_level {
            return Err(TaskCapabilityManifestError::CatalogRiskMismatch);
        }
        if risk_rank(risk_level) > risk_rank(aggregate_risk) {
            aggregate_risk = risk_level;
        }

        let mut tool_bindings = Vec::with_capacity(entry.tool_ids.len());
        for tool_id in &entry.tool_ids {
            let tool = tools
                .get(tool_id.as_str())
                .ok_or(TaskCapabilityManifestError::UnknownTool)?;
            if tool.capability != capability {
                return Err(TaskCapabilityManifestError::ToolCapabilityMismatch);
            }
            if tool.risk_level != risk_level {
                return Err(TaskCapabilityManifestError::CatalogRiskMismatch);
            }
            let goal_capability = goal_capabilities
                .get(tool_id.as_str())
                .ok_or(TaskCapabilityManifestError::GoalCapabilityMismatch)?;
            if goal_capability.capability != capability.as_str()
                || goal_capability.risk_level != risk_level
            {
                return Err(TaskCapabilityManifestError::GoalCapabilityMismatch);
            }
            if !covered_tools.insert(tool_id.as_str()) {
                return Err(TaskCapabilityManifestError::DuplicateValue);
            }
            tool_bindings.push(TaskToolBinding {
                tool_id: tool.id.clone(),
                tool_version: tool.version.clone(),
            });
        }

        let applications = entry
            .application_ids
            .iter()
            .map(|application_id| {
                let display_label = context
                    .application_labels
                    .get(application_id)
                    .ok_or(TaskCapabilityManifestError::UnknownApplication)?;
                validate_display(display_label)?;
                Ok(TaskApplicationBinding {
                    application_id: application_id.clone(),
                    display_label: display_label.clone(),
                })
            })
            .collect::<Result<Vec<_>, TaskCapabilityManifestError>>()?;
        let path_scopes = compile_scopes(
            &entry.path_target_ids,
            &goal_targets,
            context,
            scope_is_path,
        )?;
        let account_scopes =
            compile_scopes(&entry.account_target_ids, &goal_targets, context, |kind| {
                kind == GoalTargetBindingKind::Account
            })?;
        let recipient_scopes = compile_scopes(
            &entry.recipient_target_ids,
            &goal_targets,
            context,
            |kind| kind == GoalTargetBindingKind::Recipient,
        )?;
        let time_windows = compile_scopes(
            &entry.time_window_target_ids,
            &goal_targets,
            context,
            |kind| kind == GoalTargetBindingKind::TimeWindow,
        )?;
        let external_targets =
            compile_scopes(&entry.external_target_ids, &goal_targets, context, |_| true)?;
        covered_targets.extend(entry.external_target_ids.iter().map(String::as_str));

        let verifiers = entry
            .verifier_ids
            .iter()
            .map(|verifier_id| {
                let verifier = goal_verifiers
                    .get(verifier_id.as_str())
                    .ok_or(TaskCapabilityManifestError::UnknownVerifier)?;
                covered_verifiers.insert(verifier_id.as_str());
                validate_display(&verifier.description)?;
                Ok(TaskVerifierSummary {
                    verifier_id: verifier.verifier_id.clone(),
                    evidence_kind: verifier.evidence_kind.clone(),
                    summary: verifier.description.clone(),
                })
            })
            .collect::<Result<Vec<_>, TaskCapabilityManifestError>>()?;

        capabilities.push(TaskCapabilityManifestEntry {
            capability,
            risk_level,
            tools: tool_bindings,
            applications,
            path_scopes,
            account_scopes,
            recipient_scopes,
            time_windows,
            external_targets,
            verifiers,
        });
    }

    if covered_tools != goal_capabilities.keys().copied().collect()
        || covered_targets != goal_targets.keys().copied().collect()
        || covered_verifiers != goal_verifiers.keys().copied().collect()
    {
        return Err(TaskCapabilityManifestError::IncompleteGoalBinding);
    }

    let mut manifest = TaskCapabilityManifest {
        version: TASK_CAPABILITY_MANIFEST_VERSION.to_string(),
        schema_revision: TASK_CAPABILITY_MANIFEST_SCHEMA_REVISION,
        task_id: canonical_task_id,
        goal_id: goal.goal_id,
        goal_revision: frozen.revision.clone(),
        goal_fingerprint: frozen.fingerprint.clone(),
        expires_at: proposal.expires_at,
        aggregate_risk,
        capabilities,
        revision: String::new(),
        fingerprint: String::new(),
    };
    manifest.revision = manifest_revision_for(&manifest);
    manifest.fingerprint = manifest_fingerprint_for(&manifest);
    manifest.validate_integrity()?;
    validate_manifest_against_goal(&manifest, goal, tool_catalog, capability_catalog)?;
    Ok(manifest)
}

fn validate_manifest_against_goal(
    manifest: &TaskCapabilityManifest,
    goal: &GoalLifecycleProjection,
    tool_catalog: &[ToolContract],
    capability_catalog: &[CapabilityDescriptor],
) -> Result<(), TaskCapabilityManifestError> {
    manifest.validate_integrity()?;
    let frozen = goal
        .frozen()
        .ok_or(TaskCapabilityManifestError::GoalNotFrozen)?;
    if manifest.task_id != goal.goal_id || manifest.goal_id != goal.goal_id {
        return Err(TaskCapabilityManifestError::TaskGoalIdentityMismatch);
    }
    if manifest.goal_revision != frozen.revision {
        return Err(TaskCapabilityManifestError::GoalRevisionMismatch);
    }
    if manifest.goal_fingerprint != frozen.fingerprint {
        return Err(TaskCapabilityManifestError::GoalFingerprintMismatch);
    }
    let expected_tools = frozen
        .envelope
        .validated_capabilities
        .iter()
        .map(|capability| capability.tool_id.as_str())
        .collect::<BTreeSet<_>>();
    let manifest_tools = manifest
        .capabilities
        .iter()
        .flat_map(|entry| entry.tools.iter().map(|tool| tool.tool_id.as_str()))
        .collect::<BTreeSet<_>>();
    if manifest_tools != expected_tools {
        return Err(TaskCapabilityManifestError::GoalCapabilityMismatch);
    }
    let expected_targets = frozen
        .envelope
        .bound_targets
        .iter()
        .map(|target| (target.target_id.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let expected_verifiers = frozen
        .envelope
        .verifiers
        .iter()
        .map(|verifier| (verifier.verifier_id.as_str(), verifier))
        .collect::<BTreeMap<_, _>>();
    let mut manifest_targets = BTreeSet::new();
    let mut manifest_verifiers = BTreeSet::new();
    for entry in &manifest.capabilities {
        let descriptor = capability_catalog
            .iter()
            .find(|descriptor| descriptor.capability == entry.capability)
            .ok_or(TaskCapabilityManifestError::UnknownCapability)?;
        if descriptor.risk_level != entry.risk_level
            || capability_risk(entry.capability) != entry.risk_level
        {
            return Err(TaskCapabilityManifestError::CatalogRiskMismatch);
        }
        for tool in &entry.tools {
            let contract = tool_catalog
                .iter()
                .find(|contract| contract.id == tool.tool_id)
                .ok_or(TaskCapabilityManifestError::UnknownTool)?;
            if contract.version != tool.tool_version
                || contract.capability != entry.capability
                || contract.risk_level != entry.risk_level
            {
                return Err(TaskCapabilityManifestError::ToolCapabilityMismatch);
            }
            let goal_capability = frozen
                .envelope
                .validated_capabilities
                .iter()
                .find(|capability| capability.tool_id == tool.tool_id)
                .ok_or(TaskCapabilityManifestError::GoalCapabilityMismatch)?;
            if goal_capability.capability != entry.capability.as_str()
                || goal_capability.risk_level != entry.risk_level
            {
                return Err(TaskCapabilityManifestError::GoalCapabilityMismatch);
            }
        }
        for scope in entry
            .path_scopes
            .iter()
            .chain(&entry.account_scopes)
            .chain(&entry.recipient_scopes)
            .chain(&entry.time_windows)
            .chain(&entry.external_targets)
        {
            let target = expected_targets
                .get(scope.target_id.as_str())
                .ok_or(TaskCapabilityManifestError::ScopeBindingMismatch)?;
            if target.binding_kind != scope.binding_kind
                || target.authority_fingerprint != scope.authority_fingerprint
            {
                return Err(TaskCapabilityManifestError::ScopeBindingMismatch);
            }
        }
        manifest_targets.extend(
            entry
                .external_targets
                .iter()
                .map(|target| target.target_id.as_str()),
        );
        for verifier in &entry.verifiers {
            let expected = expected_verifiers
                .get(verifier.verifier_id.as_str())
                .ok_or(TaskCapabilityManifestError::UnknownVerifier)?;
            if expected.evidence_kind != verifier.evidence_kind
                || expected.description != verifier.summary
            {
                return Err(TaskCapabilityManifestError::UnknownVerifier);
            }
            manifest_verifiers.insert(verifier.verifier_id.as_str());
        }
    }
    if manifest_targets != expected_targets.keys().copied().collect()
        || manifest_verifiers != expected_verifiers.keys().copied().collect()
    {
        return Err(TaskCapabilityManifestError::IncompleteGoalBinding);
    }
    Ok(())
}

fn compile_scopes(
    target_ids: &[String],
    goal_targets: &BTreeMap<&str, &crate::kernel::goal_lifecycle::BoundGoalTarget>,
    context: &TaskCapabilityManifestContext,
    kind_allowed: impl Fn(GoalTargetBindingKind) -> bool,
) -> Result<Vec<TaskScopeBinding>, TaskCapabilityManifestError> {
    target_ids
        .iter()
        .map(|target_id| {
            let target = goal_targets
                .get(target_id.as_str())
                .ok_or(TaskCapabilityManifestError::ScopeBindingMismatch)?;
            if !kind_allowed(target.binding_kind) {
                return Err(TaskCapabilityManifestError::ScopeBindingMismatch);
            }
            validate_hash(&target.authority_fingerprint)?;
            let display_label = context
                .target_labels
                .get(target_id)
                .ok_or(TaskCapabilityManifestError::ScopeBindingMismatch)?;
            validate_display(display_label)?;
            Ok(TaskScopeBinding {
                target_id: target_id.clone(),
                binding_kind: target.binding_kind,
                display_label: display_label.clone(),
                authority_fingerprint: target.authority_fingerprint.clone(),
            })
        })
        .collect()
}

fn capability_from_name(catalog: &[CapabilityDescriptor], name: &str) -> Option<CapabilityKind> {
    catalog
        .iter()
        .find(|descriptor| descriptor.capability.as_str() == name)
        .map(|descriptor| descriptor.capability)
}

fn preview_scope_labels(bindings: &[TaskScopeBinding]) -> Vec<TaskPreviewLabel> {
    bindings
        .iter()
        .map(|binding| TaskPreviewLabel {
            id: binding.target_id.clone(),
            display_label: binding.display_label.clone(),
        })
        .collect()
}

#[derive(Serialize)]
struct ManifestRevisionCanonical<'a> {
    version: &'a str,
    schema_revision: u32,
    task_id: Uuid,
    goal_id: Uuid,
    goal_revision: &'a str,
    goal_fingerprint: &'a str,
    expires_at: DateTime<Utc>,
    aggregate_risk: RiskLevel,
    capabilities: &'a [TaskCapabilityManifestEntry],
}

#[derive(Serialize)]
struct ManifestFingerprintCanonical<'a> {
    #[serde(flatten)]
    manifest: ManifestRevisionCanonical<'a>,
    revision: &'a str,
}

#[derive(Serialize)]
struct PreviewCanonical<'a> {
    version: &'a str,
    schema_revision: u32,
    renderer_revision: u32,
    task_id: Uuid,
    goal_id: Uuid,
    goal_revision: &'a str,
    goal_fingerprint: &'a str,
    manifest_revision: &'a str,
    manifest_fingerprint: &'a str,
    expires_at: DateTime<Utc>,
    aggregate_risk: RiskLevel,
    capabilities: &'a [TaskCapabilityPreviewEntry],
}

fn manifest_revision_canonical(manifest: &TaskCapabilityManifest) -> ManifestRevisionCanonical<'_> {
    ManifestRevisionCanonical {
        version: &manifest.version,
        schema_revision: manifest.schema_revision,
        task_id: manifest.task_id,
        goal_id: manifest.goal_id,
        goal_revision: &manifest.goal_revision,
        goal_fingerprint: &manifest.goal_fingerprint,
        expires_at: manifest.expires_at,
        aggregate_risk: manifest.aggregate_risk,
        capabilities: &manifest.capabilities,
    }
}

fn manifest_revision_for(manifest: &TaskCapabilityManifest) -> String {
    domain_hash(
        MANIFEST_REVISION_DOMAIN,
        &serde_json::to_vec(&manifest_revision_canonical(manifest)).unwrap_or_default(),
    )
}

fn manifest_fingerprint_for(manifest: &TaskCapabilityManifest) -> String {
    domain_hash(
        MANIFEST_FINGERPRINT_DOMAIN,
        &serde_json::to_vec(&ManifestFingerprintCanonical {
            manifest: manifest_revision_canonical(manifest),
            revision: &manifest.revision,
        })
        .unwrap_or_default(),
    )
}

fn preview_hash_for(preview: &TaskAuthorizationPreview) -> String {
    domain_hash(
        PREVIEW_HASH_DOMAIN,
        &serde_json::to_vec(&PreviewCanonical {
            version: &preview.version,
            schema_revision: preview.schema_revision,
            renderer_revision: preview.renderer_revision,
            task_id: preview.task_id,
            goal_id: preview.goal_id,
            goal_revision: &preview.goal_revision,
            goal_fingerprint: &preview.goal_fingerprint,
            manifest_revision: &preview.manifest_revision,
            manifest_fingerprint: &preview.manifest_fingerprint,
            expires_at: preview.expires_at,
            aggregate_risk: preview.aggregate_risk,
            capabilities: &preview.capabilities,
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

fn validate_hash(value: &str) -> Result<(), TaskCapabilityManifestError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(TaskCapabilityManifestError::InvalidBindingHash);
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), TaskCapabilityManifestError> {
    if value.is_empty()
        || value != value.trim()
        || value.len() > MAX_ID_BYTES
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(TaskCapabilityManifestError::InvalidIdentifier);
    }
    if contains_secret_like_content(value) || contains_private_internal_reference(value) {
        return Err(TaskCapabilityManifestError::SecretLikeContent);
    }
    Ok(())
}

fn validate_display(value: &str) -> Result<(), TaskCapabilityManifestError> {
    if value.is_empty()
        || value != value.trim()
        || value.len() > MAX_DISPLAY_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(TaskCapabilityManifestError::InvalidDisplay);
    }
    if contains_secret_like_content(value) || contains_private_internal_reference(value) {
        return Err(TaskCapabilityManifestError::SecretLikeContent);
    }
    Ok(())
}

fn validate_id_list(values: &[String]) -> Result<(), TaskCapabilityManifestError> {
    if values.len() > MAX_ITEMS_PER_FIELD {
        return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
    }
    for value in values {
        validate_id(value)?;
    }
    validate_strict_order(values.iter().map(String::as_str))
}

fn validate_nonempty_id_list(values: &[String]) -> Result<(), TaskCapabilityManifestError> {
    if values.is_empty() {
        return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
    }
    validate_id_list(values)
}

fn validate_strict_order<'a>(
    values: impl IntoIterator<Item = &'a str>,
) -> Result<(), TaskCapabilityManifestError> {
    let mut previous: Option<&str> = None;
    for value in values {
        if let Some(previous) = previous {
            if previous == value {
                return Err(TaskCapabilityManifestError::DuplicateValue);
            }
            if previous > value {
                return Err(TaskCapabilityManifestError::NonCanonicalOrder);
            }
        }
        previous = Some(value);
    }
    Ok(())
}

fn validate_tool_bindings(values: &[TaskToolBinding]) -> Result<(), TaskCapabilityManifestError> {
    if values.is_empty() || values.len() > MAX_ITEMS_PER_FIELD {
        return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
    }
    validate_strict_order(values.iter().map(|value| value.tool_id.as_str()))?;
    for value in values {
        validate_id(&value.tool_id)?;
        validate_id(&value.tool_version)?;
    }
    Ok(())
}

fn validate_application_bindings(
    values: &[TaskApplicationBinding],
) -> Result<(), TaskCapabilityManifestError> {
    if values.len() > MAX_ITEMS_PER_FIELD {
        return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
    }
    validate_strict_order(values.iter().map(|value| value.application_id.as_str()))?;
    for value in values {
        validate_id(&value.application_id)?;
        validate_display(&value.display_label)?;
    }
    Ok(())
}

fn validate_scope_bindings(
    values: &[TaskScopeBinding],
    kind_allowed: impl Fn(GoalTargetBindingKind) -> bool,
) -> Result<(), TaskCapabilityManifestError> {
    if values.len() > MAX_ITEMS_PER_FIELD {
        return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
    }
    validate_strict_order(values.iter().map(|value| value.target_id.as_str()))?;
    for value in values {
        validate_id(&value.target_id)?;
        validate_display(&value.display_label)?;
        validate_hash(&value.authority_fingerprint)?;
        if !kind_allowed(value.binding_kind) {
            return Err(TaskCapabilityManifestError::ScopeBindingMismatch);
        }
    }
    Ok(())
}

fn validate_verifier_summaries(
    values: &[TaskVerifierSummary],
) -> Result<(), TaskCapabilityManifestError> {
    if values.is_empty() || values.len() > MAX_ITEMS_PER_FIELD {
        return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
    }
    validate_strict_order(values.iter().map(|value| value.verifier_id.as_str()))?;
    for value in values {
        validate_id(&value.verifier_id)?;
        validate_id(&value.evidence_kind)?;
        validate_display(&value.summary)?;
    }
    Ok(())
}

fn validate_preview_labels(values: &[TaskPreviewLabel]) -> Result<(), TaskCapabilityManifestError> {
    if values.len() > MAX_ITEMS_PER_FIELD {
        return Err(TaskCapabilityManifestError::CollectionOutOfBounds);
    }
    validate_strict_order(values.iter().map(|value| value.id.as_str()))?;
    for value in values {
        validate_id(&value.id)?;
        validate_display(&value.display_label)?;
    }
    Ok(())
}

fn scope_is_path(kind: GoalTargetBindingKind) -> bool {
    matches!(
        kind,
        GoalTargetBindingKind::Workspace | GoalTargetBindingKind::Path
    )
}

fn risk_rank(risk: RiskLevel) -> u8 {
    match risk {
        RiskLevel::Low => 0,
        RiskLevel::Medium => 1,
        RiskLevel::High => 2,
        RiskLevel::Critical => 3,
    }
}

fn contains_private_internal_reference(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let windows_absolute = bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'\\' | b'/')
    });
    windows_absolute
        || lower.starts_with("/")
        || lower.starts_with("\\\\")
        || lower.contains("\\appdata\\")
        || lower.contains("/appdata/")
        || lower.contains("/users/")
        || lower.contains("/home/")
        || lower.contains("vault://")
        || lower.contains("provider_ref")
        || lower.contains("provider-ref")
        || lower.contains("provider://")
        || lower.contains("remote_ref")
        || lower.contains("remote-ref")
        || lower.contains("credential_handle")
        || lower.contains("credential-handle")
        || lower.contains("credential handle")
        || lower.contains("claim_token")
        || lower.contains("claim-token")
}

fn contains_secret_like_content(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    contains_token_after(&lower, "bearer ", 12)
        || contains_token_after(&lower, "api_key=", 12)
        || contains_token_after(&lower, "api_key:", 12)
        || contains_token_after(&lower, "api-key=", 12)
        || contains_token_after(&lower, "api-key:", 12)
        || contains_token_after(&lower, "password=", 12)
        || contains_token_after(&lower, "password:", 12)
        || contains_token_after(&lower, "secret=", 12)
        || contains_token_after(&lower, "secret:", 12)
        || contains_token_after(&lower, "token=", 12)
        || contains_token_after(&lower, "token:", 12)
        || lower.match_indices("sk-").any(|(index, _)| {
            lower[index + 3..]
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-'))
                .count()
                >= 12
        })
}

fn contains_token_after(value: &str, marker: &str, minimum_length: usize) -> bool {
    value.match_indices(marker).any(|(index, _)| {
        value[index + marker.len()..]
            .trim_start_matches([' ', '\'', '"'])
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-' | b'.'))
            .count()
            >= minimum_length
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::event_store::EventStore;
    use crate::kernel::goal_envelope::{
        GoalDoneWhenProposal, GoalEnvelopeProposal, GoalExternalTargetProposal,
        GoalVerifierProposal, GOAL_ENVELOPE_PROPOSAL_VERSION,
    };
    use crate::kernel::goal_lifecycle::{GoalTargetBindingKind, GoalValidationContext};
    use crate::kernel::local_directory::WorkspaceReadinessCode;
    use crate::kernel::models::AccessMode;
    use crate::kernel::policy::{
        exact_tool_preview_hash, CapabilityAccessRequest, PermissionResolution,
        TOOL_APPROVAL_PREVIEW_REVISION,
    };
    use crate::kernel::tool_runtime::{CONNECTOR_MUTATE_TOOL_ID, FILE_WRITE_TOOL_ID};
    use tempfile::tempdir;

    const TASK_ID: Uuid = Uuid::from_u128(0x31a);

    fn frozen_goal_for(store: &EventStore, task_id: Uuid) -> GoalLifecycleProjection {
        let proposal = GoalEnvelopeProposal {
            version: GOAL_ENVELOPE_PROPOSAL_VERSION.to_string(),
            user_goal: "Create a verified report and send one approved external update."
                .to_string(),
            assumptions: Vec::new(),
            constraints: vec!["Use only locally bound task scopes.".to_string()],
            done_when: vec![
                GoalDoneWhenProposal {
                    done_when_id: "external-state-verified".to_string(),
                    description: "The external state is independently verified.".to_string(),
                },
                GoalDoneWhenProposal {
                    done_when_id: "report-verified".to_string(),
                    description: "The local report is independently verified.".to_string(),
                },
            ],
            required_artifacts: Vec::new(),
            verifiers: vec![
                GoalVerifierProposal {
                    verifier_id: "external-verifier-v1".to_string(),
                    done_when_id: "external-state-verified".to_string(),
                    description: "Verify the remote object state after the operation.".to_string(),
                    evidence_kind: "connector_remote_state".to_string(),
                },
                GoalVerifierProposal {
                    verifier_id: "report-verifier-v1".to_string(),
                    done_when_id: "report-verified".to_string(),
                    description: "Verify the report hash and rendered output.".to_string(),
                    evidence_kind: "artifact_hash".to_string(),
                },
            ],
            proposed_capabilities: vec![
                CONNECTOR_MUTATE_TOOL_ID.to_string(),
                FILE_WRITE_TOOL_ID.to_string(),
            ],
            external_targets: vec![
                GoalExternalTargetProposal {
                    target_id: "finance-account".to_string(),
                    description: "A locally bound external account.".to_string(),
                },
                GoalExternalTargetProposal {
                    target_id: "finance-recipient".to_string(),
                    description: "A locally bound recipient.".to_string(),
                },
                GoalExternalTargetProposal {
                    target_id: "report-folder".to_string(),
                    description: "A locally bound workspace path scope.".to_string(),
                },
                GoalExternalTargetProposal {
                    target_id: "weekday-window".to_string(),
                    description: "A locally bound schedule window.".to_string(),
                },
            ],
            stop_conditions: vec!["Stop when any exact scope changes.".to_string()],
        };
        let context =
            GoalValidationContext::new(AccessMode::FullAccess, WorkspaceReadinessCode::Ready)
                .with_max_risk(RiskLevel::Critical)
                .with_enabled_tool(CONNECTOR_MUTATE_TOOL_ID, true)
                .with_enabled_tool(FILE_WRITE_TOOL_ID, true)
                .with_approval_route(CONNECTOR_MUTATE_TOOL_ID)
                .with_approval_route(FILE_WRITE_TOOL_ID)
                .with_verifier_kind("artifact_hash")
                .with_verifier_kind("connector_remote_state")
                .with_target_binding(
                    "finance-account",
                    GoalTargetBindingKind::Account,
                    b"account-authority-v1",
                )
                .with_target_binding(
                    "finance-recipient",
                    GoalTargetBindingKind::Recipient,
                    b"recipient-authority-v1",
                )
                .with_target_binding(
                    "report-folder",
                    GoalTargetBindingKind::Path,
                    b"path-authority-v1",
                )
                .with_target_binding(
                    "weekday-window",
                    GoalTargetBindingKind::TimeWindow,
                    b"time-authority-v1",
                )
                .allowing_local_effects()
                .allowing_external_effects();
        let validated = store
            .submit_goal_proposal(task_id, &proposal, &context)
            .expect("goal validates");
        store
            .freeze_goal_envelope(task_id, validated.revision().expect("goal revision"))
            .expect("goal freezes")
    }

    fn frozen_goal(store: &EventStore) -> GoalLifecycleProjection {
        frozen_goal_for(store, TASK_ID)
    }

    fn manifest_context() -> TaskCapabilityManifestContext {
        TaskCapabilityManifestContext::default()
            .with_application("excel", "Microsoft Excel")
            .with_application("outlook", "Microsoft Outlook")
            .with_target_display("finance-account", "Work mailbox account")
            .with_target_display("finance-recipient", "finance-test@example.com")
            .with_target_display("report-folder", "Workspace / reports")
            .with_target_display("weekday-window", "Weekdays 09:00-17:00 Asia/Shanghai")
    }

    fn manifest_proposal(goal: &GoalLifecycleProjection) -> TaskCapabilityManifestProposal {
        let frozen = goal.frozen().expect("frozen goal");
        TaskCapabilityManifestProposal {
            version: TASK_CAPABILITY_MANIFEST_VERSION.to_string(),
            task_id: TASK_ID,
            goal_id: TASK_ID,
            goal_revision: frozen.revision.clone(),
            goal_fingerprint: frozen.fingerprint.clone(),
            expires_at: "2030-01-02T03:04:05Z".parse().unwrap(),
            capabilities: vec![
                TaskCapabilityNeedProposal {
                    capability: "connector_write".to_string(),
                    tool_ids: vec![CONNECTOR_MUTATE_TOOL_ID.to_string()],
                    application_ids: vec!["outlook".to_string()],
                    path_target_ids: Vec::new(),
                    account_target_ids: vec!["finance-account".to_string()],
                    recipient_target_ids: vec!["finance-recipient".to_string()],
                    time_window_target_ids: vec!["weekday-window".to_string()],
                    external_target_ids: vec![
                        "finance-account".to_string(),
                        "finance-recipient".to_string(),
                        "weekday-window".to_string(),
                    ],
                    verifier_ids: vec!["external-verifier-v1".to_string()],
                },
                TaskCapabilityNeedProposal {
                    capability: "file_write".to_string(),
                    tool_ids: vec![FILE_WRITE_TOOL_ID.to_string()],
                    application_ids: vec!["excel".to_string()],
                    path_target_ids: vec!["report-folder".to_string()],
                    account_target_ids: Vec::new(),
                    recipient_target_ids: Vec::new(),
                    time_window_target_ids: Vec::new(),
                    external_target_ids: vec!["report-folder".to_string()],
                    verifier_ids: vec!["report-verifier-v1".to_string()],
                },
            ],
        }
    }

    fn descriptive_proposal() -> TaskCapabilityProposal {
        TaskCapabilityProposal {
            version: TASK_CAPABILITY_PROPOSAL_VERSION.to_string(),
            expires_at: "2030-01-02T03:04:05Z".parse().unwrap(),
            capabilities: vec![
                TaskCapabilityDescriptionProposal {
                    capability: "connector_write".to_string(),
                    application_ids: vec!["outlook".to_string()],
                    path_target_ids: Vec::new(),
                    account_target_ids: vec!["finance-account".to_string()],
                    recipient_target_ids: vec!["finance-recipient".to_string()],
                    time_window_target_ids: vec!["weekday-window".to_string()],
                    external_target_ids: vec![
                        "finance-account".to_string(),
                        "finance-recipient".to_string(),
                        "weekday-window".to_string(),
                    ],
                    verifier_ids: vec!["external-verifier-v1".to_string()],
                },
                TaskCapabilityDescriptionProposal {
                    capability: "file_write".to_string(),
                    application_ids: vec!["excel".to_string()],
                    path_target_ids: vec!["report-folder".to_string()],
                    account_target_ids: Vec::new(),
                    recipient_target_ids: Vec::new(),
                    time_window_target_ids: Vec::new(),
                    external_target_ids: vec!["report-folder".to_string()],
                    verifier_ids: vec!["report-verifier-v1".to_string()],
                },
            ],
        }
    }

    fn compiled_fixture() -> (
        GoalLifecycleProjection,
        TaskCapabilityManifestProposal,
        TaskCapabilityManifestContext,
        TaskCapabilityManifest,
        TaskAuthorizationPreview,
    ) {
        let store = EventStore::open_memory().unwrap();
        let goal = frozen_goal(&store);
        let proposal = manifest_proposal(&goal);
        let context = manifest_context();
        let manifest = compile_task_capability_manifest(TASK_ID, &goal, &proposal, &context)
            .expect("manifest compiles");
        let preview = task_authorization_preview(&manifest).expect("preview renders");
        (goal, proposal, context, manifest, preview)
    }

    fn rehash_manifest(manifest: &mut TaskCapabilityManifest) {
        manifest.revision = manifest_revision_for(manifest);
        manifest.fingerprint = manifest_fingerprint_for(manifest);
    }

    #[test]
    fn c3d_descriptive_proposal_is_strict_bounded_and_contains_no_kernel_authority() {
        let proposal = descriptive_proposal();
        let serialized = proposal.to_json().unwrap();
        for forbidden in [
            "task_id",
            "goal_id",
            "goal_revision",
            "goal_fingerprint",
            "tool_ids",
            "risk",
            "grant",
            "actor",
            "approval",
            "preview",
            "claim",
            "token",
        ] {
            assert!(!serialized.contains(forbidden), "{forbidden}");
        }

        let mut unsupported = serde_json::to_value(&proposal).unwrap();
        unsupported["version"] = Value::String("ds-agent.task-capability-proposal/v2".to_string());
        assert_eq!(
            TaskCapabilityProposal::parse_value(unsupported),
            Err(TaskCapabilityManifestError::UnsupportedVersion)
        );

        let mut missing = serde_json::to_value(&proposal).unwrap();
        missing.as_object_mut().unwrap().remove("expires_at");
        assert_eq!(
            TaskCapabilityProposal::parse_value(missing),
            Err(TaskCapabilityManifestError::InvalidJson)
        );

        for field in [
            "risk",
            "grant",
            "authority",
            "actor",
            "approval",
            "resolution",
            "claim",
            "token",
            "permission_state",
            "manifest_revision",
            "manifest_fingerprint",
            "preview",
            "preview_hash",
            "renderer_revision",
        ] {
            let mut top = serde_json::to_value(&proposal).unwrap();
            top[field] = Value::String("forged".to_string());
            assert_eq!(
                TaskCapabilityProposal::parse_value(top),
                Err(TaskCapabilityManifestError::InvalidJson),
                "top-level {field}"
            );

            let mut nested = serde_json::to_value(&proposal).unwrap();
            nested["capabilities"][0][field] = Value::String("forged".to_string());
            assert_eq!(
                TaskCapabilityProposal::parse_value(nested),
                Err(TaskCapabilityManifestError::InvalidJson),
                "nested {field}"
            );
        }

        let mut duplicate = proposal.clone();
        duplicate.capabilities[1] = duplicate.capabilities[0].clone();
        assert_eq!(
            duplicate.validate(),
            Err(TaskCapabilityManifestError::DuplicateValue)
        );
        let mut noncanonical = proposal.clone();
        noncanonical.capabilities.reverse();
        assert_eq!(
            noncanonical.validate(),
            Err(TaskCapabilityManifestError::NonCanonicalOrder)
        );
        let mut private_reference = proposal.clone();
        private_reference.capabilities[0].application_ids = vec!["provider_ref".to_string()];
        assert_eq!(
            private_reference.validate(),
            Err(TaskCapabilityManifestError::SecretLikeContent)
        );
        let mut secret = proposal.clone();
        secret.capabilities[0].application_ids =
            vec![format!("{}{}", "sk", "-abcdefghijklmnopqrstuvwxyz")];
        assert_eq!(
            secret.validate(),
            Err(TaskCapabilityManifestError::SecretLikeContent)
        );
        let mut absolute_path = proposal.clone();
        absolute_path.capabilities[1].path_target_ids = vec!["c:\\private".to_string()];
        assert!(absolute_path.validate().is_err());
        assert_eq!(
            TaskCapabilityProposal::parse_json(&format!(
                "{{\"padding\":\"{}\"}}",
                "x".repeat(MAX_JSON_BYTES)
            )),
            Err(TaskCapabilityManifestError::JsonTooLarge)
        );
    }

    #[test]
    fn c3d_kernel_binds_descriptive_needs_to_the_exact_frozen_goal_and_catalog_tools() {
        let store = EventStore::open_memory().unwrap();
        let goal = frozen_goal(&store);
        let proposal = descriptive_proposal();
        let bound = proposal.bind_to_frozen_goal(TASK_ID, &goal).unwrap();
        let frozen = goal.frozen().unwrap();

        assert_eq!(bound.task_id, TASK_ID);
        assert_eq!(bound.goal_id, TASK_ID);
        assert_eq!(bound.goal_revision, frozen.revision);
        assert_eq!(bound.goal_fingerprint, frozen.fingerprint);
        assert_eq!(
            bound.capabilities[0].tool_ids,
            vec![CONNECTOR_MUTATE_TOOL_ID]
        );
        assert_eq!(bound.capabilities[1].tool_ids, vec![FILE_WRITE_TOOL_ID]);
        assert_eq!(
            proposal.bind_to_frozen_goal(Uuid::from_u128(0xdead), &goal),
            Err(TaskCapabilityManifestError::TaskGoalIdentityMismatch)
        );
    }

    #[test]
    fn strict_proposal_rejects_versions_missing_unknown_and_oversized_inputs() {
        let (_, proposal, _, _, _) = compiled_fixture();
        let mut value = serde_json::to_value(&proposal).unwrap();
        value["version"] = Value::String("ds-agent.task-capability-manifest/v2".to_string());
        assert_eq!(
            TaskCapabilityManifestProposal::parse_value(value),
            Err(TaskCapabilityManifestError::UnsupportedVersion)
        );

        let mut missing = serde_json::to_value(&proposal).unwrap();
        missing.as_object_mut().unwrap().remove("expires_at");
        assert_eq!(
            TaskCapabilityManifestProposal::parse_value(missing),
            Err(TaskCapabilityManifestError::InvalidJson)
        );

        let mut unknown = serde_json::to_value(&proposal).unwrap();
        unknown["authorization"] = Value::String("approved".to_string());
        assert_eq!(
            TaskCapabilityManifestProposal::parse_value(unknown),
            Err(TaskCapabilityManifestError::InvalidJson)
        );

        assert_eq!(
            TaskCapabilityManifestProposal::parse_json(&format!(
                "{{\"padding\":\"{}\"}}",
                "x".repeat(MAX_JSON_BYTES)
            )),
            Err(TaskCapabilityManifestError::JsonTooLarge)
        );
        assert_eq!(
            TaskCapabilityManifestProposal::parse_json(&proposal.to_json().unwrap()).unwrap(),
            proposal
        );
    }

    #[test]
    fn model_and_frontend_authority_or_risk_fields_are_never_accepted() {
        let (_, proposal, _, _, _) = compiled_fixture();
        for field in ["risk", "risk_level", "approved", "grant", "decision"] {
            let mut top = serde_json::to_value(&proposal).unwrap();
            top[field] = Value::String("critical".to_string());
            assert_eq!(
                TaskCapabilityManifestProposal::parse_value(top),
                Err(TaskCapabilityManifestError::InvalidJson),
                "top-level {field}"
            );

            let mut nested = serde_json::to_value(&proposal).unwrap();
            nested["capabilities"][0][field] = Value::String("low".to_string());
            assert_eq!(
                TaskCapabilityManifestProposal::parse_value(nested),
                Err(TaskCapabilityManifestError::InvalidJson),
                "nested {field}"
            );
        }
    }

    #[test]
    fn unknown_duplicate_and_noncanonical_capabilities_fail_closed() {
        let (_, proposal, _, _, _) = compiled_fixture();
        let mut unknown = proposal.clone();
        unknown.capabilities[1].capability = "future_unknown".to_string();
        assert_eq!(
            unknown.validate(),
            Err(TaskCapabilityManifestError::UnknownCapability)
        );

        let mut duplicate = proposal.clone();
        duplicate.capabilities[1].capability = "connector_write".to_string();
        assert_eq!(
            duplicate.validate(),
            Err(TaskCapabilityManifestError::DuplicateValue)
        );

        let mut unordered = proposal.clone();
        unordered.capabilities.reverse();
        assert_eq!(
            unordered.validate(),
            Err(TaskCapabilityManifestError::NonCanonicalOrder)
        );

        let mut unordered_tools = proposal.clone();
        unordered_tools.capabilities[0].tool_ids = vec!["z.tool".to_string(), "a.tool".to_string()];
        assert_eq!(
            unordered_tools.validate(),
            Err(TaskCapabilityManifestError::NonCanonicalOrder)
        );
    }

    #[test]
    fn exact_task_goal_revision_and_fingerprint_are_mandatory() {
        let (goal, proposal, context, _, _) = compiled_fixture();
        assert_eq!(
            compile_task_capability_manifest(Uuid::from_u128(99), &goal, &proposal, &context),
            Err(TaskCapabilityManifestError::TaskGoalIdentityMismatch)
        );

        let mut wrong_task = proposal.clone();
        wrong_task.task_id = Uuid::from_u128(99);
        wrong_task.goal_id = wrong_task.task_id;
        assert_eq!(
            compile_task_capability_manifest(wrong_task.task_id, &goal, &wrong_task, &context),
            Err(TaskCapabilityManifestError::TaskGoalIdentityMismatch)
        );

        let mut wrong_revision = proposal.clone();
        wrong_revision.goal_revision = "1".repeat(64);
        assert_eq!(
            compile_task_capability_manifest(TASK_ID, &goal, &wrong_revision, &context),
            Err(TaskCapabilityManifestError::GoalRevisionMismatch)
        );

        let mut wrong_fingerprint = proposal.clone();
        wrong_fingerprint.goal_fingerprint = "2".repeat(64);
        assert_eq!(
            compile_task_capability_manifest(TASK_ID, &goal, &wrong_fingerprint, &context),
            Err(TaskCapabilityManifestError::GoalFingerprintMismatch)
        );

        let (_, _, _, mut tampered_scope, _) = compiled_fixture();
        let entry = &mut tampered_scope.capabilities[0];
        for binding in entry
            .account_scopes
            .iter_mut()
            .chain(entry.external_targets.iter_mut())
            .filter(|binding| binding.target_id == "finance-account")
        {
            binding.authority_fingerprint = "3".repeat(64);
        }
        rehash_manifest(&mut tampered_scope);
        assert!(tampered_scope.validate_integrity().is_ok());
        assert_eq!(
            tampered_scope.validate_for_goal(&goal),
            Err(TaskCapabilityManifestError::ScopeBindingMismatch)
        );

        let (_, _, _, mut tampered_verifier, _) = compiled_fixture();
        tampered_verifier.capabilities[0].verifiers[0].summary =
            "A different verifier summary.".to_string();
        rehash_manifest(&mut tampered_verifier);
        assert!(tampered_verifier.validate_integrity().is_ok());
        assert_eq!(
            tampered_verifier.validate_for_goal(&goal),
            Err(TaskCapabilityManifestError::UnknownVerifier)
        );
    }

    #[test]
    fn kernel_catalog_alone_derives_capability_and_aggregate_risk() {
        let (goal, proposal, context, manifest, preview) = compiled_fixture();
        assert_eq!(manifest.aggregate_risk, RiskLevel::Critical);
        assert_eq!(manifest.capabilities[0].risk_level, RiskLevel::Critical);
        assert_eq!(manifest.capabilities[1].risk_level, RiskLevel::High);
        assert_eq!(preview.aggregate_risk, RiskLevel::Critical);
        assert!(manifest.validate_integrity().is_ok());
        assert!(preview.validate_for_manifest(&manifest).is_ok());

        let mut tool_catalog = builtin_tool_catalog();
        tool_catalog
            .iter_mut()
            .find(|tool| tool.id == FILE_WRITE_TOOL_ID)
            .unwrap()
            .risk_level = RiskLevel::Low;
        assert_eq!(
            compile_with_catalogs(
                TASK_ID,
                &goal,
                &proposal,
                &context,
                &tool_catalog,
                &builtin_capability_catalog(),
            ),
            Err(TaskCapabilityManifestError::CatalogRiskMismatch)
        );
    }

    #[test]
    fn capability_tool_and_complete_goal_coverage_cannot_drift() {
        let (goal, proposal, context, _, _) = compiled_fixture();
        let mut mismatch = proposal.clone();
        mismatch.capabilities[0].tool_ids = vec![FILE_WRITE_TOOL_ID.to_string()];
        mismatch.capabilities[1].tool_ids = vec![CONNECTOR_MUTATE_TOOL_ID.to_string()];
        assert_eq!(
            compile_task_capability_manifest(TASK_ID, &goal, &mismatch, &context),
            Err(TaskCapabilityManifestError::ToolCapabilityMismatch)
        );

        let mut omitted = proposal.clone();
        omitted.capabilities.pop();
        assert_eq!(
            compile_task_capability_manifest(TASK_ID, &goal, &omitted, &context),
            Err(TaskCapabilityManifestError::IncompleteGoalBinding)
        );
    }

    #[test]
    fn path_account_recipient_schedule_expiry_application_and_verifier_drift_change_binding() {
        let (goal, proposal, context, manifest, preview) = compiled_fixture();
        let variants = [
            context
                .clone()
                .with_target_display("report-folder", "Workspace / revised-reports"),
            context
                .clone()
                .with_target_display("finance-account", "Revised work mailbox account"),
            context
                .clone()
                .with_target_display("finance-recipient", "other-test@example.com"),
            context
                .clone()
                .with_target_display("weekday-window", "Weekdays 10:00-18:00 Asia/Shanghai"),
        ];
        for changed_context in variants {
            let changed =
                compile_task_capability_manifest(TASK_ID, &goal, &proposal, &changed_context)
                    .unwrap();
            assert_ne!(changed.fingerprint, manifest.fingerprint);
            assert_eq!(
                preview.validate_for_manifest(&changed),
                Err(TaskCapabilityManifestError::PreviewIntegrityMismatch)
            );
        }

        let mut changed_expiry = proposal.clone();
        changed_expiry.expires_at = "2031-01-02T03:04:05Z".parse().unwrap();
        let changed =
            compile_task_capability_manifest(TASK_ID, &goal, &changed_expiry, &context).unwrap();
        assert_ne!(changed.fingerprint, manifest.fingerprint);

        let mut changed_application = proposal.clone();
        changed_application.capabilities[1].application_ids = vec!["outlook".to_string()];
        let changed =
            compile_task_capability_manifest(TASK_ID, &goal, &changed_application, &context)
                .unwrap();
        assert_ne!(changed.fingerprint, manifest.fingerprint);

        let mut changed_verifier = proposal.clone();
        changed_verifier.capabilities[0].verifier_ids = vec!["report-verifier-v1".to_string()];
        changed_verifier.capabilities[1].verifier_ids = vec!["external-verifier-v1".to_string()];
        let changed =
            compile_task_capability_manifest(TASK_ID, &goal, &changed_verifier, &context).unwrap();
        assert_ne!(changed.fingerprint, manifest.fingerprint);
    }

    #[test]
    fn preview_revision_hash_tamper_replay_and_cross_domain_reuse_fail() {
        let (_goal, proposal, context, manifest, preview) = compiled_fixture();
        assert_ne!(preview.preview_hash, manifest.revision);
        assert_ne!(preview.preview_hash, manifest.fingerprint);

        let mut wrong_revision = preview.clone();
        wrong_revision.renderer_revision += 1;
        assert_eq!(
            wrong_revision.validate_integrity(),
            Err(TaskCapabilityManifestError::UnsupportedVersion)
        );

        let mut tampered = preview.clone();
        tampered.capabilities[0].account_scopes[0].display_label = "Tampered account".to_string();
        assert_eq!(
            tampered.validate_integrity(),
            Err(TaskCapabilityManifestError::PreviewIntegrityMismatch)
        );

        let mut wrong_hash = preview.clone();
        wrong_hash.preview_hash = manifest.fingerprint.clone();
        assert_eq!(
            wrong_hash.validate_integrity(),
            Err(TaskCapabilityManifestError::PreviewIntegrityMismatch)
        );

        let other_store = EventStore::open_memory().unwrap();
        let other_task_id = Uuid::from_u128(0x31b);
        let other_goal = frozen_goal_for(&other_store, other_task_id);
        let mut replay_proposal = proposal.clone();
        replay_proposal.task_id = other_task_id;
        replay_proposal.goal_id = other_task_id;
        replay_proposal.goal_revision = other_goal.frozen().unwrap().revision.clone();
        replay_proposal.goal_fingerprint = other_goal.frozen().unwrap().fingerprint.clone();
        let other_manifest = compile_task_capability_manifest(
            other_task_id,
            &other_goal,
            &replay_proposal,
            &context,
        )
        .unwrap();
        assert_ne!(other_manifest.fingerprint, manifest.fingerprint);
        assert_eq!(
            preview.validate_for_manifest(&other_manifest),
            Err(TaskCapabilityManifestError::PreviewIntegrityMismatch)
        );
    }

    #[test]
    fn canonical_manifest_and_preview_survive_restart_deterministically() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("kernel.sqlite3");
        let first_manifest;
        let first_preview;
        let proposal;
        {
            let store = EventStore::open(&database).unwrap();
            let goal = frozen_goal(&store);
            proposal = manifest_proposal(&goal);
            first_manifest =
                compile_task_capability_manifest(TASK_ID, &goal, &proposal, &manifest_context())
                    .unwrap();
            first_preview = task_authorization_preview(&first_manifest).unwrap();
        }

        let reopened = EventStore::open(&database).unwrap();
        let goal = reopened
            .goal_envelope_projection(TASK_ID)
            .unwrap()
            .expect("legacy GoalEnvelope projection reopens");
        let next_manifest =
            compile_task_capability_manifest(TASK_ID, &goal, &proposal, &manifest_context())
                .unwrap();
        let next_preview = task_authorization_preview(&next_manifest).unwrap();
        assert!(next_manifest.validate_for_goal(&goal).is_ok());
        assert_eq!(next_manifest, first_manifest);
        assert_eq!(next_preview, first_preview);
        assert_eq!(
            TaskCapabilityManifest::parse_json(&first_manifest.canonical_json().unwrap()).unwrap(),
            first_manifest
        );
        assert_eq!(
            TaskAuthorizationPreview::parse_json(&first_preview.canonical_json().unwrap()).unwrap(),
            first_preview
        );
    }

    #[test]
    fn secret_provider_credential_claim_and_private_path_labels_are_rejected() {
        let (goal, proposal, context, manifest, preview) = compiled_fixture();
        for forbidden in [
            format!("sk-{}", "a".repeat(20)),
            "provider_ref=message-123".to_string(),
            "credential_handle=vault-item".to_string(),
            "claim_token=opaque-claim".to_string(),
            r"C:\Users\owner\AppData\Local\private".to_string(),
        ] {
            let changed = context
                .clone()
                .with_target_display("report-folder", forbidden);
            assert_eq!(
                compile_task_capability_manifest(TASK_ID, &goal, &proposal, &changed),
                Err(TaskCapabilityManifestError::SecretLikeContent)
            );
        }

        let public_preview = serde_json::to_string(&preview).unwrap();
        for forbidden in [
            "authority_fingerprint",
            "tool_id",
            "provider_ref",
            "credential_handle",
            "claim_token",
            "AppData",
        ] {
            assert!(!public_preview.contains(forbidden));
        }
        let manifest_json = serde_json::to_string(&manifest).unwrap();
        for forbidden in [
            "provider_ref",
            "credential_handle",
            "claim_token",
            "AppData",
        ] {
            assert!(!manifest_json.contains(forbidden));
        }
    }

    #[test]
    fn malformed_proposal_creates_no_permission_execution_event_or_completion_state() {
        let store = EventStore::open_memory().unwrap();
        let malformed = serde_json::json!({
            "version": TASK_CAPABILITY_MANIFEST_VERSION,
            "task_id": TASK_ID,
            "goal_id": TASK_ID,
            "goal_revision": "1".repeat(64),
            "goal_fingerprint": "2".repeat(64),
            "expires_at": "2030-01-02T03:04:05Z",
            "capabilities": [],
            "approved": true
        });
        assert!(TaskCapabilityManifestProposal::parse_value(malformed).is_err());
        assert!(store.list_recent(10).unwrap().is_empty());
        assert!(store.list_capability_access_records().unwrap().is_empty());
        assert!(store.goal_envelope_projection(TASK_ID).unwrap().is_none());
        assert!(store.goal_completion_projection(TASK_ID).unwrap().is_none());
    }

    #[test]
    fn existing_exact_tool_approval_contract_remains_compatible() {
        let mut request = CapabilityAccessRequest {
            id: Uuid::from_u128(400),
            access_mode: AccessMode::AskEveryStep,
            family: crate::kernel::policy::CapabilityFamily::File,
            capability: CapabilityKind::FileWrite,
            title: "Write local files".to_string(),
            summary: "Write one exact file.".to_string(),
            risk_level: RiskLevel::High,
            decision: crate::kernel::policy::PolicyDecision::Ask,
            status: crate::kernel::policy::CapabilityAccessStatus::PendingApproval,
            reason: "exact approval required".to_string(),
            exact_tool: None,
            created_at: Utc::now(),
        };
        request
            .bind_exact_tool(FILE_WRITE_TOOL_ID, "a".repeat(64), "Write reports/brief.md")
            .unwrap();
        let scope = request.exact_tool.as_ref().unwrap();
        assert_eq!(scope.preview_revision, TOOL_APPROVAL_PREVIEW_REVISION);
        assert_eq!(
            scope.preview_hash,
            exact_tool_preview_hash(scope.preview_revision, &scope.preview)
        );
        assert!(PermissionResolution::new_exact(
            request.id,
            true,
            "Approved exact tool".to_string(),
            0,
            scope,
        )
        .is_ok());
    }

    #[test]
    fn manifest_or_preview_deserialization_rejects_unknown_and_tampered_fields() {
        let (_, _, _, manifest, preview) = compiled_fixture();
        let mut manifest_value = serde_json::to_value(&manifest).unwrap();
        manifest_value["capabilities"][0]["approved"] = Value::Bool(true);
        assert_eq!(
            TaskCapabilityManifest::parse_json(&manifest_value.to_string()),
            Err(TaskCapabilityManifestError::InvalidJson)
        );

        let mut tampered_manifest = manifest.clone();
        tampered_manifest.capabilities[0].risk_level = RiskLevel::Low;
        assert_eq!(
            tampered_manifest.validate_integrity(),
            Err(TaskCapabilityManifestError::CatalogRiskMismatch)
        );

        let mut unknown_tool = manifest.clone();
        unknown_tool.capabilities[0].tools[0].tool_id = "future.unknown".to_string();
        rehash_manifest(&mut unknown_tool);
        assert_eq!(
            unknown_tool.validate_integrity(),
            Err(TaskCapabilityManifestError::UnknownTool)
        );

        let mut preview_value = serde_json::to_value(&preview).unwrap();
        preview_value["grant"] = Value::String("reusable".to_string());
        assert_eq!(
            TaskAuthorizationPreview::parse_json(&preview_value.to_string()),
            Err(TaskCapabilityManifestError::InvalidJson)
        );

        let mut downgraded_preview = preview.clone();
        downgraded_preview.aggregate_risk = RiskLevel::Low;
        downgraded_preview.preview_hash = preview_hash_for(&downgraded_preview);
        assert_eq!(
            downgraded_preview.validate_integrity(),
            Err(TaskCapabilityManifestError::CatalogRiskMismatch)
        );
    }
}
