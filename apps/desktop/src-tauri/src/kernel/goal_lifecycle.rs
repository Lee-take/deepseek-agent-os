use std::collections::{BTreeMap, BTreeSet};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::kernel::goal_envelope::{
    GoalDoneWhenProposal, GoalEnvelopeProposal, GoalEnvelopeProposalError,
    GoalRequiredArtifactProposal, GoalVerifierProposal,
};
use crate::kernel::local_directory::WorkspaceReadinessCode;
use crate::kernel::models::{AccessMode, KernelEvent};
use crate::kernel::policy::{capability_risk, decide, CapabilityKind, PolicyDecision, RiskLevel};
use crate::kernel::tool_runtime::{
    builtin_tool_catalog, ToolContract, ToolExecutionStatus, ToolInvocationRecord, ToolPathScope,
    CONNECTOR_MUTATE_TOOL_ID,
};

pub const GOAL_LIFECYCLE_SCHEMA_VERSION: &str = "ds-agent.goal-envelope-lifecycle/v1";
pub const GOAL_FROZEN_ENVELOPE_VERSION: &str = "ds-agent.goal-envelope-frozen/v1";
pub const GOAL_COMPLETION_SCHEMA_VERSION: &str = "ds-agent.goal-envelope-completion/v1";
pub const GOAL_UI_PROJECTION_VERSION: &str = "ds-agent.goal-envelope-ui/v1";

const PROPOSAL_FINGERPRINT_DOMAIN: &[u8] = b"ds-agent.goal-envelope-proposal-fingerprint.v1\0";
const CONTEXT_FINGERPRINT_DOMAIN: &[u8] = b"ds-agent.goal-envelope-context-fingerprint.v1\0";
const REVISION_DOMAIN: &[u8] = b"ds-agent.goal-envelope-revision.v1\0";
const FROZEN_FINGERPRINT_DOMAIN: &[u8] = b"ds-agent.goal-envelope-frozen-fingerprint.v1\0";
const EVENT_ID_DOMAIN: &[u8] = b"ds-agent.goal-envelope-event-id.v1\0";
const COMPLETION_EVIDENCE_DOMAIN: &[u8] = b"ds-agent.goal-envelope-completion-evidence.v1\0";
const COMPLETION_RECEIPT_DOMAIN: &[u8] = b"ds-agent.goal-envelope-completion-receipt.v1\0";
const COMPLETION_EVENT_ID_DOMAIN: &[u8] = b"ds-agent.goal-envelope-completion-event-id.v1\0";
const MAX_PERSISTED_ENVELOPE_BYTES: usize = 64 * 1024;
const MAX_COMPLETION_EVIDENCE: usize = 128;

const PROPOSAL_RECEIVED_EVENT: &str = "goal_envelope.proposal_received";
const VALIDATION_BLOCKED_EVENT: &str = "goal_envelope.validation_blocked";
const VALIDATED_EVENT: &str = "goal_envelope.validated";
const FROZEN_EVENT: &str = "goal_envelope.frozen";
const COMPLETION_BLOCKED_EVENT: &str = "goal_envelope.verification_blocked";
const COMPLETED_EVENT: &str = "goal_envelope.completed";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalLifecycleStatus {
    ProposalReceived,
    ValidationBlocked,
    Validated,
    Frozen,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalCompletionStatus {
    VerificationBlocked,
    Complete,
}

impl GoalCompletionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerificationBlocked => "verification_blocked",
            Self::Complete => "complete",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalCompletionCode {
    ArtifactIdentityMismatch,
    DuplicateEvidence,
    EvidenceFailed,
    EvidenceGoalMismatch,
    EvidenceRevisionMismatch,
    EvidenceFingerprintMismatch,
    MissingArtifactEvidence,
    MissingVerifierEvidence,
    UnknownEvidence,
    VerifierBindingMismatch,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalVerifierEvidenceStatus {
    Passed,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalCompletionEvidenceReceipt {
    pub evidence_id: Uuid,
    pub goal_id: Uuid,
    pub revision: String,
    pub frozen_fingerprint: String,
    pub verifier_id: String,
    pub done_when_id: String,
    pub evidence_kind: String,
    pub artifact_ids: Vec<String>,
    pub status: GoalVerifierEvidenceStatus,
    pub source_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalCompletionReceipt {
    pub version: String,
    pub goal_id: Uuid,
    pub revision: String,
    pub frozen_fingerprint: String,
    pub evidence_fingerprints: Vec<String>,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalCompletionProjection {
    pub schema_version: String,
    pub goal_id: Uuid,
    pub revision: String,
    pub frozen_fingerprint: String,
    pub status: GoalCompletionStatus,
    pub failure_codes: Vec<GoalCompletionCode>,
    pub evidence: Vec<GoalCompletionEvidenceReceipt>,
    pub completion_receipt: Option<GoalCompletionReceipt>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalEnvelopeUiStatus {
    Proposed,
    Blocked,
    Validated,
    Frozen,
    VerificationBlocked,
    Complete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GoalEnvelopeUiProjection {
    pub version: String,
    pub goal_id: Uuid,
    pub status: GoalEnvelopeUiStatus,
    pub reason_codes: Vec<String>,
    pub revision: Option<String>,
    pub fingerprint: String,
    pub completion_fingerprint: Option<String>,
    pub user_goal_summary: Option<String>,
    pub done_when_count: usize,
    pub required_artifact_count: usize,
    pub verifier_count: usize,
}

impl GoalLifecycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProposalReceived => "proposal_received",
            Self::ValidationBlocked => "validation_blocked",
            Self::Validated => "validated",
            Self::Frozen => "frozen",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalValidationCode {
    AuthorizationUnavailable,
    CapabilityDisabled,
    CapabilityNotReady,
    CapabilityRiskNotAllowed,
    ExternalEffectNotAllowed,
    LocalEffectNotAllowed,
    SensitiveLocalReference,
    TargetBindingInvalid,
    TargetUnbound,
    UnknownCapability,
    VerifierUnavailable,
    WorkspaceNotReady,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalValidationDisposition {
    Blocked,
    Validated,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalTargetBindingKind {
    Workspace,
    Path,
    Account,
    Recipient,
    TimeWindow,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalEffectClass {
    ReadOnly,
    LocalMutation,
    ExternalMutation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalAuthorizationRequirement {
    None,
    FutureApprovalRequired,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GoalTargetAuthorityBinding {
    target_id: String,
    binding_kind: GoalTargetBindingKind,
    authority_fingerprint: String,
    authority_bound: bool,
}

#[derive(Clone, Debug)]
pub struct GoalValidationContext {
    access_mode: AccessMode,
    workspace_readiness: WorkspaceReadinessCode,
    max_risk: RiskLevel,
    enabled_tools: BTreeSet<String>,
    ready_tools: BTreeSet<String>,
    approval_routes: BTreeSet<String>,
    verifier_kinds: BTreeSet<String>,
    target_bindings: BTreeMap<String, GoalTargetAuthorityBinding>,
    allow_local_effects: bool,
    allow_external_effects: bool,
}

impl GoalValidationContext {
    pub fn new(access_mode: AccessMode, workspace_readiness: WorkspaceReadinessCode) -> Self {
        Self {
            access_mode,
            workspace_readiness,
            max_risk: RiskLevel::Low,
            enabled_tools: BTreeSet::new(),
            ready_tools: BTreeSet::new(),
            approval_routes: BTreeSet::new(),
            verifier_kinds: BTreeSet::new(),
            target_bindings: BTreeMap::new(),
            allow_local_effects: false,
            allow_external_effects: false,
        }
    }

    pub fn with_max_risk(mut self, max_risk: RiskLevel) -> Self {
        self.max_risk = max_risk;
        self
    }

    pub fn with_enabled_tool(mut self, tool_id: impl Into<String>, ready: bool) -> Self {
        let tool_id = tool_id.into();
        self.enabled_tools.insert(tool_id.clone());
        if ready {
            self.ready_tools.insert(tool_id);
        } else {
            self.ready_tools.remove(&tool_id);
        }
        self
    }

    pub fn with_approval_route(mut self, tool_id: impl Into<String>) -> Self {
        self.approval_routes.insert(tool_id.into());
        self
    }

    pub fn with_verifier_kind(mut self, evidence_kind: impl Into<String>) -> Self {
        self.verifier_kinds.insert(evidence_kind.into());
        self
    }

    pub fn with_target_binding(
        mut self,
        target_id: impl Into<String>,
        binding_kind: GoalTargetBindingKind,
        local_authority_material: impl AsRef<[u8]>,
    ) -> Self {
        let target_id = target_id.into();
        let local_authority_material = local_authority_material.as_ref();
        let authority_fingerprint = domain_hash(
            b"ds-agent.goal-envelope-target-authority.v1\0",
            local_authority_material,
        );
        self.target_bindings.insert(
            target_id.clone(),
            GoalTargetAuthorityBinding {
                target_id,
                binding_kind,
                authority_fingerprint,
                authority_bound: !local_authority_material.is_empty(),
            },
        );
        self
    }

    pub fn allowing_local_effects(mut self) -> Self {
        self.allow_local_effects = true;
        self
    }

    #[cfg(test)]
    pub fn allowing_external_effects(mut self) -> Self {
        self.allow_external_effects = true;
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalCapabilityValidation {
    pub tool_id: String,
    pub capability: String,
    pub risk_level: RiskLevel,
    pub effect_class: GoalEffectClass,
    pub authorization: GoalAuthorizationRequirement,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundGoalTarget {
    pub target_id: String,
    pub binding_kind: GoalTargetBindingKind,
    pub authority_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalValidationReceipt {
    pub version: String,
    pub disposition: GoalValidationDisposition,
    pub proposal_fingerprint: String,
    pub context_fingerprint: String,
    pub failure_codes: Vec<GoalValidationCode>,
    pub capabilities: Vec<GoalCapabilityValidation>,
    pub target_bindings: Vec<BoundGoalTarget>,
    pub verifier_kinds: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatedGoalEnvelope {
    pub version: String,
    pub user_goal: String,
    pub assumptions: Vec<String>,
    pub constraints: Vec<String>,
    pub done_when: Vec<GoalDoneWhenProposal>,
    pub required_artifacts: Vec<GoalRequiredArtifactProposal>,
    pub verifiers: Vec<GoalVerifierProposal>,
    pub validated_capabilities: Vec<GoalCapabilityValidation>,
    pub bound_targets: Vec<BoundGoalTarget>,
    pub stop_conditions: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalFrozenEnvelope {
    pub version: String,
    pub proposal_fingerprint: String,
    pub revision: String,
    pub envelope: ValidatedGoalEnvelope,
    pub validation_receipt: GoalValidationReceipt,
    pub fingerprint: String,
}

impl GoalFrozenEnvelope {
    pub fn canonical_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&FrozenCanonical {
            version: &self.version,
            proposal_fingerprint: &self.proposal_fingerprint,
            revision: &self.revision,
            envelope: &self.envelope,
            validation_receipt: &self.validation_receipt,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum GoalLifecycleState {
    ProposalReceived {
        proposal_fingerprint: String,
    },
    ValidationBlocked {
        proposal_fingerprint: String,
        validation_receipt: GoalValidationReceipt,
    },
    Validated {
        proposal_fingerprint: String,
        revision: String,
        envelope: ValidatedGoalEnvelope,
        validation_receipt: GoalValidationReceipt,
    },
    Frozen {
        frozen: GoalFrozenEnvelope,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalLifecycleProjection {
    pub schema_version: String,
    pub goal_id: Uuid,
    pub state: GoalLifecycleState,
}

impl GoalLifecycleProjection {
    pub fn status(&self) -> GoalLifecycleStatus {
        match self.state {
            GoalLifecycleState::ProposalReceived { .. } => GoalLifecycleStatus::ProposalReceived,
            GoalLifecycleState::ValidationBlocked { .. } => GoalLifecycleStatus::ValidationBlocked,
            GoalLifecycleState::Validated { .. } => GoalLifecycleStatus::Validated,
            GoalLifecycleState::Frozen { .. } => GoalLifecycleStatus::Frozen,
        }
    }

    pub fn proposal_fingerprint(&self) -> &str {
        match &self.state {
            GoalLifecycleState::ProposalReceived {
                proposal_fingerprint,
            }
            | GoalLifecycleState::ValidationBlocked {
                proposal_fingerprint,
                ..
            }
            | GoalLifecycleState::Validated {
                proposal_fingerprint,
                ..
            } => proposal_fingerprint,
            GoalLifecycleState::Frozen { frozen } => &frozen.proposal_fingerprint,
        }
    }

    pub fn revision(&self) -> Option<&str> {
        match &self.state {
            GoalLifecycleState::Validated { revision, .. } => Some(revision),
            GoalLifecycleState::Frozen { frozen } => Some(&frozen.revision),
            _ => None,
        }
    }

    pub fn validation_receipt(&self) -> Option<&GoalValidationReceipt> {
        match &self.state {
            GoalLifecycleState::ValidationBlocked {
                validation_receipt, ..
            }
            | GoalLifecycleState::Validated {
                validation_receipt, ..
            } => Some(validation_receipt),
            GoalLifecycleState::Frozen { frozen } => Some(&frozen.validation_receipt),
            GoalLifecycleState::ProposalReceived { .. } => None,
        }
    }

    pub fn frozen(&self) -> Option<&GoalFrozenEnvelope> {
        match &self.state {
            GoalLifecycleState::Frozen { frozen } => Some(frozen),
            _ => None,
        }
    }

    pub(super) fn validate_persisted(&self) -> Result<(), &'static str> {
        if self.schema_version != GOAL_LIFECYCLE_SCHEMA_VERSION
            || !valid_hash(self.proposal_fingerprint())
        {
            return Err("goal_projection_invalid");
        }
        match &self.state {
            GoalLifecycleState::ProposalReceived { .. } => Ok(()),
            GoalLifecycleState::ValidationBlocked {
                proposal_fingerprint,
                validation_receipt,
            } => {
                if validation_receipt.disposition != GoalValidationDisposition::Blocked
                    || validation_receipt.failure_codes.is_empty()
                    || validation_receipt.proposal_fingerprint != *proposal_fingerprint
                    || !receipt_is_secret_free(validation_receipt)
                {
                    return Err("goal_projection_invalid");
                }
                Ok(())
            }
            GoalLifecycleState::Validated {
                proposal_fingerprint,
                revision,
                envelope,
                validation_receipt,
            } => {
                if validation_receipt.disposition != GoalValidationDisposition::Validated
                    || !validation_receipt.failure_codes.is_empty()
                    || validation_receipt.proposal_fingerprint != *proposal_fingerprint
                    || revision != &revision_for(proposal_fingerprint, envelope, validation_receipt)
                    || !receipt_is_secret_free(validation_receipt)
                    || !validated_envelope_is_secret_free(envelope, validation_receipt)
                {
                    return Err("goal_projection_invalid");
                }
                Ok(())
            }
            GoalLifecycleState::Frozen { frozen } => {
                if frozen.version != GOAL_FROZEN_ENVELOPE_VERSION
                    || frozen.validation_receipt.disposition != GoalValidationDisposition::Validated
                    || !frozen.validation_receipt.failure_codes.is_empty()
                    || frozen.validation_receipt.proposal_fingerprint != frozen.proposal_fingerprint
                    || frozen.revision
                        != revision_for(
                            &frozen.proposal_fingerprint,
                            &frozen.envelope,
                            &frozen.validation_receipt,
                        )
                    || frozen.fingerprint != frozen_fingerprint(frozen)
                    || !receipt_is_secret_free(&frozen.validation_receipt)
                    || !validated_envelope_is_secret_free(
                        &frozen.envelope,
                        &frozen.validation_receipt,
                    )
                {
                    return Err("goal_projection_invalid");
                }
                Ok(())
            }
        }
    }
}

#[derive(Serialize)]
struct FrozenCanonical<'a> {
    version: &'a str,
    proposal_fingerprint: &'a str,
    revision: &'a str,
    envelope: &'a ValidatedGoalEnvelope,
    validation_receipt: &'a GoalValidationReceipt,
}

#[derive(Serialize)]
struct RevisionCanonical<'a> {
    version: &'static str,
    proposal_fingerprint: &'a str,
    envelope: &'a ValidatedGoalEnvelope,
    validation_receipt: &'a GoalValidationReceipt,
}

#[derive(Serialize)]
struct ContextCanonical<'a> {
    access_mode: AccessMode,
    workspace_readiness: WorkspaceReadinessCode,
    max_risk: RiskLevel,
    enabled_tools: &'a BTreeSet<String>,
    ready_tools: &'a BTreeSet<String>,
    approval_routes: &'a BTreeSet<String>,
    verifier_kinds: &'a BTreeSet<String>,
    target_bindings: Vec<BoundGoalTarget>,
    allow_local_effects: bool,
    allow_external_effects: bool,
}

#[derive(Serialize)]
struct GoalLifecycleEventPayload<'a> {
    schema_version: &'static str,
    goal_id: Uuid,
    status: GoalLifecycleStatus,
    proposal_fingerprint: &'a str,
    revision: Option<&'a str>,
    frozen_fingerprint: Option<&'a str>,
    failure_codes: &'a [GoalValidationCode],
}

pub(super) fn proposal_received_projection(
    goal_id: Uuid,
    proposal: &GoalEnvelopeProposal,
) -> Result<GoalLifecycleProjection, GoalEnvelopeProposalError> {
    proposal.validate()?;
    Ok(GoalLifecycleProjection {
        schema_version: GOAL_LIFECYCLE_SCHEMA_VERSION.to_string(),
        goal_id,
        state: GoalLifecycleState::ProposalReceived {
            proposal_fingerprint: proposal_fingerprint(proposal),
        },
    })
}

pub(super) fn validated_projection(
    goal_id: Uuid,
    proposal: &GoalEnvelopeProposal,
    context: &GoalValidationContext,
) -> Result<GoalLifecycleProjection, GoalEnvelopeProposalError> {
    proposal.validate()?;
    let proposal_fingerprint = proposal_fingerprint(proposal);
    let context_fingerprint = context_fingerprint(context);
    let mut failures = BTreeSet::new();
    if proposal_contains_sensitive_local_reference(proposal) {
        failures.insert(GoalValidationCode::SensitiveLocalReference);
    }

    let catalog = builtin_tool_catalog();
    let mut capabilities = Vec::new();
    for tool_id in &proposal.proposed_capabilities {
        let Some(contract) = catalog.iter().find(|contract| contract.id == *tool_id) else {
            failures.insert(GoalValidationCode::UnknownCapability);
            continue;
        };
        validate_capability(contract, context, &mut failures);
        capabilities.push(capability_validation(contract, context));
    }
    capabilities.sort_by(|left, right| left.tool_id.cmp(&right.tool_id));

    let mut target_bindings = Vec::new();
    for target in &proposal.external_targets {
        let Some(binding) = context.target_bindings.get(&target.target_id) else {
            failures.insert(GoalValidationCode::TargetUnbound);
            continue;
        };
        if binding.target_id != target.target_id
            || !binding.authority_bound
            || !valid_hash(&binding.authority_fingerprint)
            || contains_sensitive_local_reference(&binding.target_id)
        {
            failures.insert(GoalValidationCode::TargetBindingInvalid);
            continue;
        }
        if matches!(
            binding.binding_kind,
            GoalTargetBindingKind::Workspace | GoalTargetBindingKind::Path
        ) && context.workspace_readiness != WorkspaceReadinessCode::Ready
        {
            failures.insert(GoalValidationCode::WorkspaceNotReady);
        }
        target_bindings.push(BoundGoalTarget {
            target_id: target.target_id.clone(),
            binding_kind: binding.binding_kind,
            authority_fingerprint: binding.authority_fingerprint.clone(),
        });
    }
    target_bindings.sort_by(|left, right| left.target_id.cmp(&right.target_id));

    let verifier_kinds = proposal
        .verifiers
        .iter()
        .map(|verifier| verifier.evidence_kind.clone())
        .collect::<BTreeSet<_>>();
    if verifier_kinds
        .iter()
        .any(|kind| !context.verifier_kinds.contains(kind))
    {
        failures.insert(GoalValidationCode::VerifierUnavailable);
    }

    let disposition = if failures.is_empty() {
        GoalValidationDisposition::Validated
    } else {
        GoalValidationDisposition::Blocked
    };
    let receipt = GoalValidationReceipt {
        version: GOAL_LIFECYCLE_SCHEMA_VERSION.to_string(),
        disposition,
        proposal_fingerprint: proposal_fingerprint.clone(),
        context_fingerprint,
        failure_codes: failures.into_iter().collect(),
        capabilities,
        target_bindings: target_bindings.clone(),
        verifier_kinds: verifier_kinds
            .into_iter()
            .filter(|kind| !contains_sensitive_local_reference(kind))
            .collect(),
    };

    if disposition == GoalValidationDisposition::Blocked {
        return Ok(GoalLifecycleProjection {
            schema_version: GOAL_LIFECYCLE_SCHEMA_VERSION.to_string(),
            goal_id,
            state: GoalLifecycleState::ValidationBlocked {
                proposal_fingerprint,
                validation_receipt: receipt,
            },
        });
    }

    let envelope = normalized_envelope(proposal, target_bindings, receipt.capabilities.clone());
    let revision = revision_for(&proposal_fingerprint, &envelope, &receipt);
    Ok(GoalLifecycleProjection {
        schema_version: GOAL_LIFECYCLE_SCHEMA_VERSION.to_string(),
        goal_id,
        state: GoalLifecycleState::Validated {
            proposal_fingerprint,
            revision,
            envelope,
            validation_receipt: receipt,
        },
    })
}

pub(super) fn frozen_projection(
    current: &GoalLifecycleProjection,
    expected_revision: &str,
) -> Result<GoalLifecycleProjection, &'static str> {
    match &current.state {
        GoalLifecycleState::Validated {
            proposal_fingerprint,
            revision,
            envelope,
            validation_receipt,
        } => {
            if revision != expected_revision {
                return Err("goal_revision_mismatch");
            }
            let mut frozen = GoalFrozenEnvelope {
                version: GOAL_FROZEN_ENVELOPE_VERSION.to_string(),
                proposal_fingerprint: proposal_fingerprint.clone(),
                revision: revision.clone(),
                envelope: envelope.clone(),
                validation_receipt: validation_receipt.clone(),
                fingerprint: String::new(),
            };
            frozen.fingerprint = frozen_fingerprint(&frozen);
            Ok(GoalLifecycleProjection {
                schema_version: GOAL_LIFECYCLE_SCHEMA_VERSION.to_string(),
                goal_id: current.goal_id,
                state: GoalLifecycleState::Frozen { frozen },
            })
        }
        GoalLifecycleState::Frozen { frozen } if frozen.revision == expected_revision => {
            Ok(current.clone())
        }
        GoalLifecycleState::Frozen { .. } => Err("goal_revision_mismatch"),
        GoalLifecycleState::ProposalReceived { .. }
        | GoalLifecycleState::ValidationBlocked { .. } => Err("goal_not_validated"),
    }
}

pub(super) fn projection_event(
    projection: &GoalLifecycleProjection,
) -> Result<KernelEvent, serde_json::Error> {
    let failure_codes = projection
        .validation_receipt()
        .map(|receipt| receipt.failure_codes.as_slice())
        .unwrap_or(&[]);
    let frozen_fingerprint = projection
        .frozen()
        .map(|frozen| frozen.fingerprint.as_str());
    let payload = GoalLifecycleEventPayload {
        schema_version: GOAL_LIFECYCLE_SCHEMA_VERSION,
        goal_id: projection.goal_id,
        status: projection.status(),
        proposal_fingerprint: projection.proposal_fingerprint(),
        revision: projection.revision(),
        frozen_fingerprint,
        failure_codes,
    };
    let payload_json = serde_json::to_string(&payload)?;
    let event_type = match projection.status() {
        GoalLifecycleStatus::ProposalReceived => PROPOSAL_RECEIVED_EVENT,
        GoalLifecycleStatus::ValidationBlocked => VALIDATION_BLOCKED_EVENT,
        GoalLifecycleStatus::Validated => VALIDATED_EVENT,
        GoalLifecycleStatus::Frozen => FROZEN_EVENT,
    };
    let event_key = format!(
        "{}\0{}\0{}\0{}\0{}\0{}",
        projection.goal_id,
        projection.proposal_fingerprint(),
        projection.status().as_str(),
        projection.revision().unwrap_or_default(),
        projection
            .validation_receipt()
            .map(|receipt| receipt.context_fingerprint.as_str())
            .unwrap_or_default(),
        frozen_fingerprint.unwrap_or_default()
    );
    Ok(KernelEvent {
        id: deterministic_uuid(EVENT_ID_DOMAIN, event_key.as_bytes()),
        event_type: event_type.to_string(),
        payload_json,
        created_at: Utc::now(),
    })
}

#[derive(Serialize)]
struct ToolEvidenceCanonical<'a> {
    invocation_id: Uuid,
    tool_id: &'a str,
    status: ToolExecutionStatus,
    verification_passed: bool,
    evidence_kinds: Vec<&'a str>,
}

#[derive(Serialize)]
struct CompletionReceiptCanonical<'a> {
    version: &'static str,
    goal_id: Uuid,
    revision: &'a str,
    frozen_fingerprint: &'a str,
    evidence_fingerprints: &'a [String],
}

#[derive(Serialize)]
struct GoalCompletionEventPayload<'a> {
    schema_version: &'static str,
    goal_id: Uuid,
    revision: &'a str,
    frozen_fingerprint: &'a str,
    status: GoalCompletionStatus,
    failure_codes: &'a [GoalCompletionCode],
    completion_fingerprint: Option<&'a str>,
}

pub(super) fn completion_evidence_from_tool_invocation(
    lifecycle: &GoalLifecycleProjection,
    invocation: &ToolInvocationRecord,
) -> Result<Vec<GoalCompletionEvidenceReceipt>, &'static str> {
    let frozen = lifecycle.frozen().ok_or("goal_not_frozen")?;
    if invocation.run_id != Some(lifecycle.goal_id) {
        return Err("goal_evidence_run_mismatch");
    }

    let mut evidence_kinds = invocation
        .evidence
        .iter()
        .map(|evidence| evidence.kind.trim())
        .filter(|kind| safe_contract_token(kind))
        .collect::<Vec<_>>();
    evidence_kinds.sort_unstable();
    evidence_kinds.dedup();
    let source_json = serde_json::to_vec(&ToolEvidenceCanonical {
        invocation_id: invocation.id,
        tool_id: invocation.tool_id.as_str(),
        status: invocation.status,
        verification_passed: invocation.verification.passed,
        evidence_kinds: evidence_kinds.clone(),
    })
    .map_err(|_| "goal_evidence_invalid")?;
    let source_fingerprint = domain_hash(COMPLETION_EVIDENCE_DOMAIN, &source_json);
    let artifact_ids = frozen
        .envelope
        .required_artifacts
        .iter()
        .filter(|artifact| invocation_matches_artifact(invocation, &artifact.artifact_id))
        .map(|artifact| artifact.artifact_id.clone())
        .collect::<Vec<_>>();
    let passed =
        invocation.status == ToolExecutionStatus::Succeeded && invocation.verification.passed;
    let mut receipts = frozen
        .envelope
        .verifiers
        .iter()
        .filter(|verifier| evidence_kinds.contains(&verifier.evidence_kind.as_str()))
        .map(|verifier| {
            let evidence_key = format!(
                "{}\0{}\0{}\0{}",
                invocation.id, lifecycle.goal_id, frozen.revision, verifier.verifier_id
            );
            GoalCompletionEvidenceReceipt {
                evidence_id: deterministic_uuid(
                    COMPLETION_EVIDENCE_DOMAIN,
                    evidence_key.as_bytes(),
                ),
                goal_id: lifecycle.goal_id,
                revision: frozen.revision.clone(),
                frozen_fingerprint: frozen.fingerprint.clone(),
                verifier_id: verifier.verifier_id.clone(),
                done_when_id: verifier.done_when_id.clone(),
                evidence_kind: verifier.evidence_kind.clone(),
                artifact_ids: artifact_ids.clone(),
                status: if passed {
                    GoalVerifierEvidenceStatus::Passed
                } else {
                    GoalVerifierEvidenceStatus::Failed
                },
                source_fingerprint: source_fingerprint.clone(),
            }
        })
        .collect::<Vec<_>>();

    if receipts.is_empty() {
        let evidence_key = format!(
            "{}\0{}\0{}\0unknown",
            invocation.id, lifecycle.goal_id, frozen.revision
        );
        receipts.push(GoalCompletionEvidenceReceipt {
            evidence_id: deterministic_uuid(COMPLETION_EVIDENCE_DOMAIN, evidence_key.as_bytes()),
            goal_id: lifecycle.goal_id,
            revision: frozen.revision.clone(),
            frozen_fingerprint: frozen.fingerprint.clone(),
            verifier_id: "unknown".to_string(),
            done_when_id: "unknown".to_string(),
            evidence_kind: "unknown".to_string(),
            artifact_ids,
            status: GoalVerifierEvidenceStatus::Unknown,
            source_fingerprint,
        });
    }
    Ok(receipts)
}

pub(super) fn completion_projection(
    lifecycle: &GoalLifecycleProjection,
    evidence: &[GoalCompletionEvidenceReceipt],
) -> Result<GoalCompletionProjection, &'static str> {
    let frozen = lifecycle.frozen().ok_or("goal_not_frozen")?;
    if evidence.len() > MAX_COMPLETION_EVIDENCE {
        return Err("goal_completion_evidence_limit");
    }
    let mut failures = BTreeSet::new();
    let mut unique = BTreeMap::<Uuid, GoalCompletionEvidenceReceipt>::new();
    for receipt in evidence {
        if let Some(existing) = unique.get(&receipt.evidence_id) {
            if existing != receipt {
                failures.insert(GoalCompletionCode::DuplicateEvidence);
            }
            continue;
        }
        unique.insert(receipt.evidence_id, receipt.clone());
    }
    let mut evidence = unique.into_values().collect::<Vec<_>>();
    evidence.sort_by_key(|receipt| receipt.evidence_id);

    let verifier_by_id = frozen
        .envelope
        .verifiers
        .iter()
        .map(|verifier| (verifier.verifier_id.as_str(), verifier))
        .collect::<BTreeMap<_, _>>();
    let required_artifact_ids = frozen
        .envelope
        .required_artifacts
        .iter()
        .map(|artifact| artifact.artifact_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut verifier_counts = BTreeMap::<&str, usize>::new();
    let mut passed_verifiers = BTreeSet::<&str>::new();
    let mut passed_artifacts = BTreeSet::<&str>::new();

    for receipt in &evidence {
        if receipt.goal_id != lifecycle.goal_id {
            failures.insert(GoalCompletionCode::EvidenceGoalMismatch);
        }
        if receipt.revision != frozen.revision {
            failures.insert(GoalCompletionCode::EvidenceRevisionMismatch);
        }
        if receipt.frozen_fingerprint != frozen.fingerprint {
            failures.insert(GoalCompletionCode::EvidenceFingerprintMismatch);
        }
        if !valid_hash(&receipt.source_fingerprint)
            || !safe_contract_token(&receipt.verifier_id)
            || !safe_contract_token(&receipt.done_when_id)
            || !safe_contract_token(&receipt.evidence_kind)
        {
            failures.insert(GoalCompletionCode::UnknownEvidence);
            continue;
        }
        let Some(verifier) = verifier_by_id.get(receipt.verifier_id.as_str()) else {
            failures.insert(GoalCompletionCode::UnknownEvidence);
            continue;
        };
        if receipt.done_when_id != verifier.done_when_id
            || receipt.evidence_kind != verifier.evidence_kind
        {
            failures.insert(GoalCompletionCode::VerifierBindingMismatch);
            continue;
        }
        *verifier_counts
            .entry(receipt.verifier_id.as_str())
            .or_default() += 1;
        if verifier_counts[receipt.verifier_id.as_str()] > 1 {
            failures.insert(GoalCompletionCode::DuplicateEvidence);
        }
        if receipt.status != GoalVerifierEvidenceStatus::Passed {
            failures.insert(GoalCompletionCode::EvidenceFailed);
            continue;
        }
        passed_verifiers.insert(receipt.verifier_id.as_str());
        for artifact_id in &receipt.artifact_ids {
            if !required_artifact_ids.contains(artifact_id.as_str()) {
                failures.insert(GoalCompletionCode::ArtifactIdentityMismatch);
            } else {
                passed_artifacts.insert(artifact_id.as_str());
            }
        }
    }

    if frozen
        .envelope
        .verifiers
        .iter()
        .any(|verifier| !passed_verifiers.contains(verifier.verifier_id.as_str()))
    {
        failures.insert(GoalCompletionCode::MissingVerifierEvidence);
    }
    if required_artifact_ids
        .iter()
        .any(|artifact_id| !passed_artifacts.contains(artifact_id))
    {
        failures.insert(GoalCompletionCode::MissingArtifactEvidence);
    }

    let failure_codes = failures.into_iter().collect::<Vec<_>>();
    let status = if failure_codes.is_empty() {
        GoalCompletionStatus::Complete
    } else {
        GoalCompletionStatus::VerificationBlocked
    };
    let completion_receipt = if status == GoalCompletionStatus::Complete {
        let mut evidence_fingerprints = evidence
            .iter()
            .map(|receipt| receipt.source_fingerprint.clone())
            .collect::<Vec<_>>();
        evidence_fingerprints.sort();
        evidence_fingerprints.dedup();
        let canonical = CompletionReceiptCanonical {
            version: GOAL_COMPLETION_SCHEMA_VERSION,
            goal_id: lifecycle.goal_id,
            revision: &frozen.revision,
            frozen_fingerprint: &frozen.fingerprint,
            evidence_fingerprints: &evidence_fingerprints,
        };
        let canonical_json =
            serde_json::to_vec(&canonical).map_err(|_| "goal_completion_receipt_invalid")?;
        Some(GoalCompletionReceipt {
            version: GOAL_COMPLETION_SCHEMA_VERSION.to_string(),
            goal_id: lifecycle.goal_id,
            revision: frozen.revision.clone(),
            frozen_fingerprint: frozen.fingerprint.clone(),
            evidence_fingerprints,
            fingerprint: domain_hash(COMPLETION_RECEIPT_DOMAIN, &canonical_json),
        })
    } else {
        None
    };
    Ok(GoalCompletionProjection {
        schema_version: GOAL_COMPLETION_SCHEMA_VERSION.to_string(),
        goal_id: lifecycle.goal_id,
        revision: frozen.revision.clone(),
        frozen_fingerprint: frozen.fingerprint.clone(),
        status,
        failure_codes,
        evidence,
        completion_receipt,
    })
}

impl GoalCompletionProjection {
    pub(super) fn validate_against(
        &self,
        lifecycle: &GoalLifecycleProjection,
    ) -> Result<(), &'static str> {
        if self.schema_version != GOAL_COMPLETION_SCHEMA_VERSION
            || self.goal_id != lifecycle.goal_id
        {
            return Err("goal_completion_projection_invalid");
        }
        let recomputed = completion_projection(lifecycle, &self.evidence)?;
        if recomputed != *self {
            return Err("goal_completion_projection_invalid");
        }
        Ok(())
    }
}

pub(super) fn completion_event(
    projection: &GoalCompletionProjection,
) -> Result<KernelEvent, serde_json::Error> {
    let completion_fingerprint = projection
        .completion_receipt
        .as_ref()
        .map(|receipt| receipt.fingerprint.as_str());
    let payload = GoalCompletionEventPayload {
        schema_version: GOAL_COMPLETION_SCHEMA_VERSION,
        goal_id: projection.goal_id,
        revision: &projection.revision,
        frozen_fingerprint: &projection.frozen_fingerprint,
        status: projection.status,
        failure_codes: &projection.failure_codes,
        completion_fingerprint,
    };
    let payload_json = serde_json::to_string(&payload)?;
    let evidence_key = projection
        .evidence
        .iter()
        .map(|receipt| receipt.evidence_id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let event_key = format!(
        "{}\0{}\0{}\0{}\0{}\0{}",
        projection.goal_id,
        projection.revision,
        projection.frozen_fingerprint,
        projection.status.as_str(),
        evidence_key,
        completion_fingerprint.unwrap_or_default()
    );
    Ok(KernelEvent {
        id: deterministic_uuid(COMPLETION_EVENT_ID_DOMAIN, event_key.as_bytes()),
        event_type: if projection.status == GoalCompletionStatus::Complete {
            COMPLETED_EVENT
        } else {
            COMPLETION_BLOCKED_EVENT
        }
        .to_string(),
        payload_json,
        created_at: Utc::now(),
    })
}

pub fn goal_ui_projection(
    lifecycle: &GoalLifecycleProjection,
    completion: Option<&GoalCompletionProjection>,
) -> Result<GoalEnvelopeUiProjection, &'static str> {
    if let Some(completion) = completion {
        completion.validate_against(lifecycle)?;
    }
    let (validated, frozen) = match &lifecycle.state {
        GoalLifecycleState::Validated { envelope, .. } => (Some(envelope), None),
        GoalLifecycleState::Frozen { frozen } => (Some(&frozen.envelope), Some(frozen)),
        GoalLifecycleState::ProposalReceived { .. }
        | GoalLifecycleState::ValidationBlocked { .. } => (None, None),
    };
    let status = match (
        lifecycle.status(),
        completion.map(|projection| projection.status),
    ) {
        (_, Some(GoalCompletionStatus::Complete)) => GoalEnvelopeUiStatus::Complete,
        (_, Some(GoalCompletionStatus::VerificationBlocked)) => {
            GoalEnvelopeUiStatus::VerificationBlocked
        }
        (GoalLifecycleStatus::ProposalReceived, None) => GoalEnvelopeUiStatus::Proposed,
        (GoalLifecycleStatus::ValidationBlocked, None) => GoalEnvelopeUiStatus::Blocked,
        (GoalLifecycleStatus::Validated, None) => GoalEnvelopeUiStatus::Validated,
        (GoalLifecycleStatus::Frozen, None) => GoalEnvelopeUiStatus::Frozen,
    };
    let reason_codes = if let Some(completion) = completion {
        completion
            .failure_codes
            .iter()
            .filter_map(stable_code)
            .collect()
    } else {
        lifecycle
            .validation_receipt()
            .into_iter()
            .flat_map(|receipt| receipt.failure_codes.iter())
            .filter_map(stable_code)
            .collect()
    };
    Ok(GoalEnvelopeUiProjection {
        version: GOAL_UI_PROJECTION_VERSION.to_string(),
        goal_id: lifecycle.goal_id,
        status,
        reason_codes,
        revision: lifecycle.revision().map(str::to_string),
        fingerprint: frozen
            .map(|frozen| frozen.fingerprint.clone())
            .unwrap_or_else(|| lifecycle.proposal_fingerprint().to_string()),
        completion_fingerprint: completion
            .and_then(|projection| projection.completion_receipt.as_ref())
            .map(|receipt| receipt.fingerprint.clone()),
        user_goal_summary: validated.map(|envelope| bounded_summary(&envelope.user_goal, 320)),
        done_when_count: validated.map_or(0, |envelope| envelope.done_when.len()),
        required_artifact_count: validated.map_or(0, |envelope| envelope.required_artifacts.len()),
        verifier_count: validated.map_or(0, |envelope| envelope.verifiers.len()),
    })
}

fn invocation_matches_artifact(invocation: &ToolInvocationRecord, artifact_id: &str) -> bool {
    invocation
        .evidence
        .iter()
        .any(|evidence| evidence.reference.trim() == artifact_id)
        || invocation.output.as_ref().is_some_and(|output| {
            ["artifact_id", "id"].iter().any(|key| {
                output.get(*key).and_then(serde_json::Value::as_str) == Some(artifact_id)
            })
        })
}

fn safe_contract_token(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && !contains_sensitive_local_reference(value)
}

fn stable_code<T: Serialize>(code: &T) -> Option<String> {
    serde_json::to_value(code)
        .ok()?
        .as_str()
        .map(str::to_string)
}

fn bounded_summary(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut summary = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        summary.push_str("...");
    }
    summary
}

fn validate_capability(
    contract: &ToolContract,
    context: &GoalValidationContext,
    failures: &mut BTreeSet<GoalValidationCode>,
) {
    if !context.enabled_tools.contains(&contract.id) {
        failures.insert(GoalValidationCode::CapabilityDisabled);
    } else if !context.ready_tools.contains(&contract.id) {
        failures.insert(GoalValidationCode::CapabilityNotReady);
    }
    if risk_rank(contract.risk_level) > risk_rank(context.max_risk)
        || contract.risk_level != capability_risk(contract.capability)
    {
        failures.insert(GoalValidationCode::CapabilityRiskNotAllowed);
    }
    if contract.constraints.path_scope == ToolPathScope::Workspace
        && context.workspace_readiness != WorkspaceReadinessCode::Ready
    {
        failures.insert(GoalValidationCode::WorkspaceNotReady);
    }
    let effect = effect_class(contract);
    if effect == GoalEffectClass::LocalMutation && !context.allow_local_effects {
        failures.insert(GoalValidationCode::LocalEffectNotAllowed);
    }
    if effect == GoalEffectClass::ExternalMutation && !context.allow_external_effects {
        failures.insert(GoalValidationCode::ExternalEffectNotAllowed);
    }
    if decide(context.access_mode, contract.capability) == PolicyDecision::Ask
        && !context.approval_routes.contains(&contract.id)
    {
        failures.insert(GoalValidationCode::AuthorizationUnavailable);
    }
}

fn capability_validation(
    contract: &ToolContract,
    context: &GoalValidationContext,
) -> GoalCapabilityValidation {
    GoalCapabilityValidation {
        tool_id: contract.id.clone(),
        capability: capability_contract_name(contract.capability).to_string(),
        risk_level: contract.risk_level,
        effect_class: effect_class(contract),
        authorization: if decide(context.access_mode, contract.capability) == PolicyDecision::Ask {
            GoalAuthorizationRequirement::FutureApprovalRequired
        } else {
            GoalAuthorizationRequirement::None
        },
    }
}

fn capability_contract_name(capability: CapabilityKind) -> &'static str {
    match capability {
        CapabilityKind::FileRead => "file_read",
        CapabilityKind::FileWrite => "file_write",
        CapabilityKind::NetworkSearch => "network_search",
        CapabilityKind::BrowserBrowse => "browser_browse",
        CapabilityKind::BrowserSubmit => "browser_submit",
        CapabilityKind::EmailRead => "email_read",
        CapabilityKind::EmailDraft => "email_draft",
        CapabilityKind::EmailSend => "email_send",
        CapabilityKind::ConnectorAttachmentRead => "connector_attachment_read",
        CapabilityKind::ConnectorWrite => "connector_write",
        CapabilityKind::DriveRead => "drive_read",
        CapabilityKind::DriveWrite => "drive_write",
        CapabilityKind::TerminalRead => "terminal_read",
        CapabilityKind::TerminalWrite => "terminal_write",
        CapabilityKind::ComputerScreenshot => "computer_screenshot",
        CapabilityKind::ComputerControl => "computer_control",
        CapabilityKind::AppUpdateCheck => "app_update_check",
        CapabilityKind::AppUpdateDownload => "app_update_download",
        CapabilityKind::AppUpdateInstall => "app_update_install",
        CapabilityKind::SkillUse => "skill_use",
    }
}

fn effect_class(contract: &ToolContract) -> GoalEffectClass {
    if contract.id == CONNECTOR_MUTATE_TOOL_ID {
        GoalEffectClass::ExternalMutation
    } else if contract.constraints.mutates_machine_state {
        GoalEffectClass::LocalMutation
    } else {
        GoalEffectClass::ReadOnly
    }
}

fn normalized_envelope(
    proposal: &GoalEnvelopeProposal,
    bound_targets: Vec<BoundGoalTarget>,
    validated_capabilities: Vec<GoalCapabilityValidation>,
) -> ValidatedGoalEnvelope {
    let mut assumptions = proposal.assumptions.clone();
    assumptions.sort();
    let mut constraints = proposal.constraints.clone();
    constraints.sort();
    let mut done_when = proposal.done_when.clone();
    done_when.sort_by(|left, right| left.done_when_id.cmp(&right.done_when_id));
    let mut required_artifacts = proposal.required_artifacts.clone();
    required_artifacts.sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    let mut verifiers = proposal.verifiers.clone();
    verifiers.sort_by(|left, right| left.verifier_id.cmp(&right.verifier_id));
    let mut stop_conditions = proposal.stop_conditions.clone();
    stop_conditions.sort();
    ValidatedGoalEnvelope {
        version: GOAL_FROZEN_ENVELOPE_VERSION.to_string(),
        user_goal: proposal.user_goal.clone(),
        assumptions,
        constraints,
        done_when,
        required_artifacts,
        verifiers,
        validated_capabilities,
        bound_targets,
        stop_conditions,
    }
}

fn proposal_fingerprint(proposal: &GoalEnvelopeProposal) -> String {
    let mut canonical = proposal.clone();
    canonical.assumptions.sort();
    canonical.constraints.sort();
    canonical
        .done_when
        .sort_by(|left, right| left.done_when_id.cmp(&right.done_when_id));
    canonical
        .required_artifacts
        .sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    canonical
        .verifiers
        .sort_by(|left, right| left.verifier_id.cmp(&right.verifier_id));
    canonical.proposed_capabilities.sort();
    canonical
        .external_targets
        .sort_by(|left, right| left.target_id.cmp(&right.target_id));
    canonical.stop_conditions.sort();
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    domain_hash(PROPOSAL_FINGERPRINT_DOMAIN, &bytes)
}

fn context_fingerprint(context: &GoalValidationContext) -> String {
    let mut target_bindings = context
        .target_bindings
        .values()
        .map(|binding| BoundGoalTarget {
            target_id: binding.target_id.clone(),
            binding_kind: binding.binding_kind,
            authority_fingerprint: binding.authority_fingerprint.clone(),
        })
        .collect::<Vec<_>>();
    target_bindings.sort_by(|left, right| left.target_id.cmp(&right.target_id));
    let canonical = ContextCanonical {
        access_mode: context.access_mode,
        workspace_readiness: context.workspace_readiness,
        max_risk: context.max_risk,
        enabled_tools: &context.enabled_tools,
        ready_tools: &context.ready_tools,
        approval_routes: &context.approval_routes,
        verifier_kinds: &context.verifier_kinds,
        target_bindings,
        allow_local_effects: context.allow_local_effects,
        allow_external_effects: context.allow_external_effects,
    };
    domain_hash(
        CONTEXT_FINGERPRINT_DOMAIN,
        &serde_json::to_vec(&canonical).unwrap_or_default(),
    )
}

fn revision_for(
    proposal_fingerprint: &str,
    envelope: &ValidatedGoalEnvelope,
    validation_receipt: &GoalValidationReceipt,
) -> String {
    let canonical = RevisionCanonical {
        version: GOAL_FROZEN_ENVELOPE_VERSION,
        proposal_fingerprint,
        envelope,
        validation_receipt,
    };
    domain_hash(
        REVISION_DOMAIN,
        &serde_json::to_vec(&canonical).unwrap_or_default(),
    )
}

fn frozen_fingerprint(frozen: &GoalFrozenEnvelope) -> String {
    domain_hash(
        FROZEN_FINGERPRINT_DOMAIN,
        frozen.canonical_json().unwrap_or_default().as_bytes(),
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

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn risk_rank(risk: RiskLevel) -> u8 {
    match risk {
        RiskLevel::Low => 0,
        RiskLevel::Medium => 1,
        RiskLevel::High => 2,
        RiskLevel::Critical => 3,
    }
}

fn proposal_contains_sensitive_local_reference(proposal: &GoalEnvelopeProposal) -> bool {
    std::iter::once(proposal.user_goal.as_str())
        .chain(proposal.assumptions.iter().map(String::as_str))
        .chain(proposal.constraints.iter().map(String::as_str))
        .chain(
            proposal
                .done_when
                .iter()
                .flat_map(|value| [value.done_when_id.as_str(), value.description.as_str()]),
        )
        .chain(
            proposal
                .required_artifacts
                .iter()
                .flat_map(|value| [value.artifact_id.as_str(), value.description.as_str()]),
        )
        .chain(proposal.verifiers.iter().flat_map(|value| {
            [
                value.verifier_id.as_str(),
                value.done_when_id.as_str(),
                value.description.as_str(),
                value.evidence_kind.as_str(),
            ]
        }))
        .chain(proposal.proposed_capabilities.iter().map(String::as_str))
        .chain(
            proposal
                .external_targets
                .iter()
                .flat_map(|value| [value.target_id.as_str(), value.description.as_str()]),
        )
        .chain(proposal.stop_conditions.iter().map(String::as_str))
        .any(contains_sensitive_local_reference)
}

fn contains_sensitive_local_reference(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let has_windows_absolute = bytes.windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'\\' | b'/')
    });
    has_windows_absolute
        || lower.contains("\\appdata\\")
        || lower.contains("/appdata/")
        || lower.contains("/users/")
        || lower.contains("/home/")
        || lower.contains("vault://")
        || lower.contains("provider_body")
        || lower.contains("provider-body")
        || lower.contains("claim_token")
        || lower.contains("claim-token")
}

fn receipt_is_secret_free(receipt: &GoalValidationReceipt) -> bool {
    receipt.version == GOAL_LIFECYCLE_SCHEMA_VERSION
        && valid_hash(&receipt.proposal_fingerprint)
        && valid_hash(&receipt.context_fingerprint)
        && receipt
            .failure_codes
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        && receipt
            .capabilities
            .windows(2)
            .all(|pair| pair[0].tool_id < pair[1].tool_id)
        && receipt
            .target_bindings
            .windows(2)
            .all(|pair| pair[0].target_id < pair[1].target_id)
        && receipt
            .verifier_kinds
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        && receipt.target_bindings.iter().all(|binding| {
            valid_hash(&binding.authority_fingerprint)
                && stored_value_is_secret_free(&binding.target_id)
        })
        && receipt.capabilities.iter().all(|capability| {
            stored_value_is_secret_free(&capability.tool_id)
                && stored_value_is_secret_free(&capability.capability)
        })
        && receipt
            .verifier_kinds
            .iter()
            .all(|kind| stored_value_is_secret_free(kind))
}

fn validated_envelope_is_secret_free(
    envelope: &ValidatedGoalEnvelope,
    receipt: &GoalValidationReceipt,
) -> bool {
    if envelope.version != GOAL_FROZEN_ENVELOPE_VERSION
        || envelope.validated_capabilities != receipt.capabilities
        || envelope.bound_targets != receipt.target_bindings
        || serde_json::to_vec(envelope)
            .map(|encoded| encoded.len() > MAX_PERSISTED_ENVELOPE_BYTES)
            .unwrap_or(true)
        || !envelope
            .assumptions
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        || !envelope
            .constraints
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        || !envelope
            .done_when
            .windows(2)
            .all(|pair| pair[0].done_when_id < pair[1].done_when_id)
        || !envelope
            .required_artifacts
            .windows(2)
            .all(|pair| pair[0].artifact_id < pair[1].artifact_id)
        || !envelope
            .verifiers
            .windows(2)
            .all(|pair| pair[0].verifier_id < pair[1].verifier_id)
        || !envelope
            .stop_conditions
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    {
        return false;
    }

    let verifier_kinds = envelope
        .verifiers
        .iter()
        .map(|verifier| verifier.evidence_kind.as_str())
        .collect::<BTreeSet<_>>();
    if !verifier_kinds
        .iter()
        .copied()
        .eq(receipt.verifier_kinds.iter().map(String::as_str))
    {
        return false;
    }

    std::iter::once(envelope.user_goal.as_str())
        .chain(envelope.assumptions.iter().map(String::as_str))
        .chain(envelope.constraints.iter().map(String::as_str))
        .chain(
            envelope
                .done_when
                .iter()
                .flat_map(|value| [value.done_when_id.as_str(), value.description.as_str()]),
        )
        .chain(
            envelope
                .required_artifacts
                .iter()
                .flat_map(|value| [value.artifact_id.as_str(), value.description.as_str()]),
        )
        .chain(envelope.verifiers.iter().flat_map(|value| {
            [
                value.verifier_id.as_str(),
                value.done_when_id.as_str(),
                value.description.as_str(),
                value.evidence_kind.as_str(),
            ]
        }))
        .chain(envelope.stop_conditions.iter().map(String::as_str))
        .all(stored_value_is_secret_free)
}

fn stored_value_is_secret_free(value: &str) -> bool {
    !contains_sensitive_local_reference(value) && !contains_secret_like_content(value)
}

fn contains_secret_like_content(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    contains_token_after(&lower, "bearer ", 12)
        || contains_token_after(&lower, "api_key=", 12)
        || contains_token_after(&lower, "api_key:", 12)
        || contains_token_after(&lower, "api-key=", 12)
        || contains_token_after(&lower, "api-key:", 12)
        || contains_token_after(&lower, "apikey=", 12)
        || contains_token_after(&lower, "apikey:", 12)
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
    use std::fs;

    use rusqlite::Connection;
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::kernel::event_store::EventStore;
    use crate::kernel::goal_envelope::GOAL_ENVELOPE_PROPOSAL_VERSION;
    use crate::kernel::tool_runtime::{
        ToolEvidence, ToolVerificationResult, CONNECTOR_MUTATE_TOOL_ID, FILE_READ_TOOL_ID,
    };

    fn valid_proposal() -> GoalEnvelopeProposal {
        GoalEnvelopeProposal::parse_value(json!({
            "version": GOAL_ENVELOPE_PROPOSAL_VERSION,
            "user_goal": "Create a verified monthly operating brief.",
            "assumptions": ["The supplied source set is complete."],
            "constraints": ["Keep proposed outputs inside the selected workspace."],
            "done_when": [{
                "done_when_id": "brief-ready",
                "description": "The proposed operating brief is complete."
            }],
            "required_artifacts": [{
                "artifact_id": "operating-brief",
                "description": "A proposed operating brief."
            }],
            "verifiers": [{
                "verifier_id": "brief-verifier-v1",
                "done_when_id": "brief-ready",
                "description": "Verify the rendered brief has no overflow.",
                "evidence_kind": "rendered-brief"
            }],
            "proposed_capabilities": [FILE_READ_TOOL_ID],
            "external_targets": [{
                "target_id": "selected-workspace",
                "description": "The workspace proposed by the model; bind it locally."
            }],
            "stop_conditions": ["Stop if a required source is missing."]
        }))
        .expect("valid proposal")
    }

    fn valid_context() -> GoalValidationContext {
        GoalValidationContext::new(AccessMode::FullAccess, WorkspaceReadinessCode::Ready)
            .with_enabled_tool(FILE_READ_TOOL_ID, true)
            .with_verifier_kind("rendered-brief")
            .with_target_binding(
                "selected-workspace",
                GoalTargetBindingKind::Workspace,
                br"C:\private\workspace",
            )
    }

    fn failure_codes(projection: &GoalLifecycleProjection) -> &[GoalValidationCode] {
        &projection
            .validation_receipt()
            .expect("validation receipt")
            .failure_codes
    }

    fn frozen_goal(store: &EventStore, goal_id: Uuid) -> GoalLifecycleProjection {
        let validated = store
            .submit_goal_proposal(goal_id, &valid_proposal(), &valid_context())
            .unwrap();
        store
            .freeze_goal_envelope(goal_id, validated.revision().unwrap())
            .unwrap()
    }

    fn verifier_invocation(
        goal_id: Uuid,
        invocation_id: Uuid,
        status: ToolExecutionStatus,
        passed: bool,
        evidence_kind: &str,
        artifact_id: &str,
    ) -> ToolInvocationRecord {
        ToolInvocationRecord {
            id: invocation_id,
            run_id: Some(goal_id),
            tool_id: FILE_READ_TOOL_ID.to_string(),
            tool_version: "1".to_string(),
            capability: CapabilityKind::FileRead,
            status,
            policy_decision: PolicyDecision::Allow,
            approval_request_id: None,
            input_summary: "bounded local verifier input".to_string(),
            request_fingerprint: "1".repeat(64),
            output: Some(json!({"artifact_id": artifact_id})),
            evidence: vec![ToolEvidence {
                kind: evidence_kind.to_string(),
                reference: artifact_id.to_string(),
                summary: "bounded local verifier evidence".to_string(),
            }],
            verification: ToolVerificationResult {
                passed,
                summary: "local verifier result".to_string(),
                checked_at: Utc::now(),
            },
            error: (!passed).then(|| "safe verifier failure".to_string()),
            recovery_hint: "retry local verification".to_string(),
            elapsed_ms: 1,
            created_at: Utc::now(),
            finished_at: Some(Utc::now()),
        }
    }

    fn persist_bound_verifier_invocation(store: &EventStore, invocation: &ToolInvocationRecord) {
        let mut running = invocation.clone();
        running.status = ToolExecutionStatus::Running;
        running.output = None;
        running.evidence.clear();
        running.verification = ToolVerificationResult::failed("verification is in progress");
        running.error = None;
        running.elapsed_ms = 0;
        running.finished_at = None;
        store
            .append_tool_invocation(&running)
            .expect("running invocation captures the frozen goal binding");
        store
            .append_tool_invocation(invocation)
            .expect("terminal invocation becomes canonical");
    }

    #[test]
    fn valid_proposal_is_validated_frozen_and_restart_safe() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("kernel.sqlite3");
        let goal_id = Uuid::from_u128(1);
        let revision;
        let fingerprint;
        {
            let store = EventStore::open(&database).unwrap();
            let validated = store
                .submit_goal_proposal(goal_id, &valid_proposal(), &valid_context())
                .unwrap();
            assert_eq!(validated.status(), GoalLifecycleStatus::Validated);
            revision = validated.revision().unwrap().to_string();
            let frozen = store.freeze_goal_envelope(goal_id, &revision).unwrap();
            assert_eq!(frozen.status(), GoalLifecycleStatus::Frozen);
            fingerprint = frozen.frozen().unwrap().fingerprint.clone();
            assert!(valid_hash(&fingerprint));
            let persisted = serde_json::to_string(&frozen).unwrap();
            assert!(!persisted.contains("proposed by the model"));
            assert!(!persisted.contains(r"C:\private\workspace"));
        }
        let reopened = EventStore::open(&database).unwrap();
        let readback = reopened.goal_envelope_projection(goal_id).unwrap().unwrap();
        assert_eq!(readback.status(), GoalLifecycleStatus::Frozen);
        assert_eq!(readback.revision(), Some(revision.as_str()));
        assert_eq!(readback.frozen().unwrap().fingerprint, fingerprint);
    }

    #[test]
    fn unknown_disabled_and_not_ready_capabilities_fail_closed() {
        let store = EventStore::open_memory().unwrap();
        let mut unknown = valid_proposal();
        unknown.proposed_capabilities = vec!["future.unknown".to_string()];
        let blocked = store
            .submit_goal_proposal(Uuid::from_u128(2), &unknown, &valid_context())
            .unwrap();
        assert!(failure_codes(&blocked).contains(&GoalValidationCode::UnknownCapability));

        let disabled_context =
            GoalValidationContext::new(AccessMode::FullAccess, WorkspaceReadinessCode::Ready)
                .with_verifier_kind("rendered-brief")
                .with_target_binding(
                    "selected-workspace",
                    GoalTargetBindingKind::Workspace,
                    b"workspace-authority",
                );
        let disabled = store
            .submit_goal_proposal(Uuid::from_u128(3), &valid_proposal(), &disabled_context)
            .unwrap();
        assert!(failure_codes(&disabled).contains(&GoalValidationCode::CapabilityDisabled));

        let not_ready = valid_context().with_enabled_tool(FILE_READ_TOOL_ID, false);
        let blocked = store
            .submit_goal_proposal(Uuid::from_u128(4), &valid_proposal(), &not_ready)
            .unwrap();
        assert!(failure_codes(&blocked).contains(&GoalValidationCode::CapabilityNotReady));
    }

    #[test]
    fn risk_authorization_and_external_effect_policy_fail_closed() {
        let store = EventStore::open_memory().unwrap();
        let mut proposal = valid_proposal();
        proposal.proposed_capabilities = vec![CONNECTOR_MUTATE_TOOL_ID.to_string()];
        proposal.external_targets[0].target_id = "connected-account".to_string();
        let context =
            GoalValidationContext::new(AccessMode::AskEveryStep, WorkspaceReadinessCode::Ready)
                .with_enabled_tool(CONNECTOR_MUTATE_TOOL_ID, true)
                .with_verifier_kind("rendered-brief")
                .with_target_binding(
                    "connected-account",
                    GoalTargetBindingKind::Account,
                    b"local-account-authority",
                );
        let blocked = store
            .submit_goal_proposal(Uuid::from_u128(5), &proposal, &context)
            .unwrap();
        let codes = failure_codes(&blocked);
        assert!(codes.contains(&GoalValidationCode::CapabilityRiskNotAllowed));
        assert!(codes.contains(&GoalValidationCode::AuthorizationUnavailable));
        assert!(codes.contains(&GoalValidationCode::ExternalEffectNotAllowed));
    }

    #[test]
    fn explicit_policy_routes_validate_without_granting_execution_authority() {
        let store = EventStore::open_memory().unwrap();
        let mut proposal = valid_proposal();
        proposal.proposed_capabilities = vec![CONNECTOR_MUTATE_TOOL_ID.to_string()];
        proposal.external_targets[0].target_id = "connected-account".to_string();
        let context =
            GoalValidationContext::new(AccessMode::AskEveryStep, WorkspaceReadinessCode::Ready)
                .with_max_risk(RiskLevel::Critical)
                .with_enabled_tool(CONNECTOR_MUTATE_TOOL_ID, true)
                .with_approval_route(CONNECTOR_MUTATE_TOOL_ID)
                .with_verifier_kind("rendered-brief")
                .with_target_binding(
                    "connected-account",
                    GoalTargetBindingKind::Account,
                    b"local-account-authority",
                )
                .allowing_local_effects()
                .allowing_external_effects();
        let validated = store
            .submit_goal_proposal(Uuid::from_u128(12), &proposal, &context)
            .unwrap();
        assert_eq!(validated.status(), GoalLifecycleStatus::Validated);
        let receipt = validated.validation_receipt().unwrap();
        assert_eq!(
            receipt.capabilities[0].authorization,
            GoalAuthorizationRequirement::FutureApprovalRequired
        );
        let serialized = serde_json::to_string(&validated).unwrap();
        for forbidden in ["approval_granted", "execution_authority", "claim_token"] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn target_and_verifier_must_bind_to_local_authority() {
        let store = EventStore::open_memory().unwrap();
        let context =
            GoalValidationContext::new(AccessMode::FullAccess, WorkspaceReadinessCode::Ready)
                .with_enabled_tool(FILE_READ_TOOL_ID, true);
        let blocked = store
            .submit_goal_proposal(Uuid::from_u128(6), &valid_proposal(), &context)
            .unwrap();
        assert!(failure_codes(&blocked).contains(&GoalValidationCode::TargetUnbound));
        assert!(failure_codes(&blocked).contains(&GoalValidationCode::VerifierUnavailable));
        let error = store
            .freeze_goal_envelope(Uuid::from_u128(6), "0".repeat(64).as_str())
            .expect_err("blocked validation cannot freeze");
        assert!(matches!(
            error,
            crate::kernel::event_store::EventStoreError::InvalidState(code)
                if code == "goal_not_validated"
        ));
    }

    #[test]
    fn secret_unknown_authority_and_absolute_paths_never_enter_persistence() {
        let store = EventStore::open_memory().unwrap();
        let unknown = json!({
            "version": GOAL_ENVELOPE_PROPOSAL_VERSION,
            "user_goal": "Write a brief.",
            "assumptions": [], "constraints": [],
            "done_when": [{"done_when_id":"done","description":"Done."}],
            "required_artifacts": [],
            "verifiers": [{"verifier_id":"v","done_when_id":"done","description":"Verify.","evidence_kind":"rendered-brief"}],
            "proposed_capabilities": [], "external_targets": [], "stop_conditions": [],
            "validated": true
        });
        assert!(GoalEnvelopeProposal::parse_value(unknown).is_err());
        assert!(store.list_recent(10).unwrap().is_empty());

        let mut secret = valid_proposal();
        secret.user_goal = format!("Use sk-{}.", "a".repeat(20));
        assert!(store
            .submit_goal_proposal(Uuid::from_u128(7), &secret, &valid_context())
            .is_err());
        assert!(store.list_recent(10).unwrap().is_empty());

        let mut path = valid_proposal();
        path.user_goal = r"Read C:\Users\owner\AppData\Local\provider-body.json.".to_string();
        let blocked = store
            .submit_goal_proposal(Uuid::from_u128(8), &path, &valid_context())
            .unwrap();
        assert!(failure_codes(&blocked).contains(&GoalValidationCode::SensitiveLocalReference));
        let serialized = serde_json::to_string(&blocked).unwrap();
        let events = serde_json::to_string(&store.list_recent(10).unwrap()).unwrap();
        for forbidden in ["AppData", "provider-body.json", "owner"] {
            assert!(!serialized.contains(forbidden));
            assert!(!events.contains(forbidden));
        }
    }

    #[test]
    fn revision_and_fingerprint_are_deterministic_and_mutation_invalidates_freeze() {
        let store = EventStore::open_memory().unwrap();
        let goal_id = Uuid::from_u128(9);
        let first = store
            .submit_goal_proposal(goal_id, &valid_proposal(), &valid_context())
            .unwrap();
        let first_revision = first.revision().unwrap().to_string();
        let first_frozen = store
            .freeze_goal_envelope(goal_id, &first_revision)
            .unwrap();
        let first_fingerprint = first_frozen.frozen().unwrap().fingerprint.clone();

        let repeat = store
            .submit_goal_proposal(goal_id, &valid_proposal(), &valid_context())
            .unwrap();
        assert_eq!(repeat.revision(), Some(first_revision.as_str()));
        assert_eq!(
            store
                .freeze_goal_envelope(goal_id, &first_revision)
                .unwrap()
                .frozen()
                .unwrap()
                .fingerprint,
            first_fingerprint
        );

        let mut changed = valid_proposal();
        changed
            .constraints
            .push("Use only reviewed sources.".to_string());
        let next = store
            .submit_goal_proposal(goal_id, &changed, &valid_context())
            .unwrap();
        let next_revision = next.revision().unwrap().to_string();
        assert_ne!(next_revision, first_revision);
        assert!(store
            .freeze_goal_envelope(goal_id, &first_revision)
            .is_err());
        let next_frozen = store.freeze_goal_envelope(goal_id, &next_revision).unwrap();
        assert_ne!(next_frozen.frozen().unwrap().fingerprint, first_fingerprint);
    }

    #[test]
    fn duplicate_transitions_are_idempotent_and_events_are_bounded() {
        let store = EventStore::open_memory().unwrap();
        let goal_id = Uuid::from_u128(10);
        let first = store
            .submit_goal_proposal(goal_id, &valid_proposal(), &valid_context())
            .unwrap();
        let second = store
            .submit_goal_proposal(goal_id, &valid_proposal(), &valid_context())
            .unwrap();
        assert_eq!(first, second);
        let revision = first.revision().unwrap();
        let frozen_a = store.freeze_goal_envelope(goal_id, revision).unwrap();
        let frozen_b = store.freeze_goal_envelope(goal_id, revision).unwrap();
        assert_eq!(frozen_a, frozen_b);
        let events = store.list_recent(20).unwrap();
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|event| event.payload_json.len() < 1024));
    }

    #[test]
    fn persisted_projection_rejects_rehashed_secret_paths_and_forged_versions() {
        let mut projection =
            validated_projection(Uuid::from_u128(13), &valid_proposal(), &valid_context()).unwrap();
        let GoalLifecycleState::Validated {
            proposal_fingerprint,
            revision,
            envelope,
            validation_receipt,
        } = &mut projection.state
        else {
            panic!("proposal should validate");
        };

        envelope.user_goal = r"Read C:\Users\owner\AppData\Local\provider-body.json.".to_string();
        *revision = revision_for(proposal_fingerprint, envelope, validation_receipt);
        assert_eq!(
            projection.validate_persisted(),
            Err("goal_projection_invalid")
        );

        let mut projection =
            validated_projection(Uuid::from_u128(14), &valid_proposal(), &valid_context()).unwrap();
        let GoalLifecycleState::Validated {
            proposal_fingerprint,
            revision,
            envelope,
            validation_receipt,
        } = &mut projection.state
        else {
            panic!("proposal should validate");
        };
        validation_receipt.version = "ds-agent.goal-envelope-lifecycle/v2".to_string();
        *revision = revision_for(proposal_fingerprint, envelope, validation_receipt);
        assert_eq!(
            projection.validate_persisted(),
            Err("goal_projection_invalid")
        );

        let mut projection =
            validated_projection(Uuid::from_u128(15), &valid_proposal(), &valid_context()).unwrap();
        let GoalLifecycleState::Validated {
            proposal_fingerprint,
            revision,
            envelope,
            validation_receipt,
        } = &mut projection.state
        else {
            panic!("proposal should validate");
        };
        envelope.user_goal = format!("Use sk-{}.", "a".repeat(20));
        *revision = revision_for(proposal_fingerprint, envelope, validation_receipt);
        assert_eq!(
            projection.validate_persisted(),
            Err("goal_projection_invalid")
        );
    }

    #[test]
    fn exact_authoritative_evidence_completes_and_ui_projection_is_secret_free() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("completion.sqlite3");
        let goal_id = Uuid::from_u128(101);
        let invocation = verifier_invocation(
            goal_id,
            Uuid::from_u128(201),
            ToolExecutionStatus::Succeeded,
            true,
            "rendered-brief",
            "operating-brief",
        );
        let completed;
        {
            let store = EventStore::open(&database).unwrap();
            frozen_goal(&store, goal_id);
            persist_bound_verifier_invocation(&store, &invocation);
            completed = store
                .record_goal_completion_for_tool_invocation(&invocation)
                .unwrap()
                .unwrap();
            assert_eq!(completed.status, GoalCompletionStatus::Complete);
            assert!(completed.failure_codes.is_empty());
            assert!(valid_hash(
                &completed.completion_receipt.as_ref().unwrap().fingerprint
            ));
            let ui = store.goal_envelope_ui_projection(goal_id).unwrap().unwrap();
            assert_eq!(ui.status, GoalEnvelopeUiStatus::Complete);
            assert_eq!(ui.required_artifact_count, 1);
            let serialized = serde_json::to_string(&ui).unwrap();
            for forbidden in [
                "authority_fingerprint",
                "context_fingerprint",
                "target_bindings",
                "provider_body",
                "claim_token",
                "AppData",
            ] {
                assert!(!serialized.contains(forbidden));
            }
        }

        let reopened = EventStore::open(&database).unwrap();
        assert_eq!(
            reopened
                .goal_completion_projection(goal_id)
                .unwrap()
                .unwrap(),
            completed
        );
        assert_eq!(
            reopened
                .record_goal_completion_for_tool_invocation(&invocation)
                .unwrap()
                .unwrap(),
            completed
        );
        let completed_events = reopened
            .list_recent(20)
            .unwrap()
            .into_iter()
            .filter(|event| event.event_type == COMPLETED_EVENT)
            .collect::<Vec<_>>();
        assert_eq!(completed_events.len(), 1);
        assert!(completed_events[0].payload_json.len() < 1024);
    }

    #[test]
    fn ui_projection_distinguishes_proposed_blocked_validated_frozen_and_verification_states() {
        let goal_id = Uuid::from_u128(104);
        let proposal = valid_proposal();
        let proposed = proposal_received_projection(goal_id, &proposal).unwrap();
        assert_eq!(
            goal_ui_projection(&proposed, None).unwrap().status,
            GoalEnvelopeUiStatus::Proposed
        );

        let blocked_context = GoalValidationContext::new(
            AccessMode::FullAccess,
            WorkspaceReadinessCode::WorkspaceMissing,
        );
        let blocked = validated_projection(goal_id, &proposal, &blocked_context).unwrap();
        let blocked_ui = goal_ui_projection(&blocked, None).unwrap();
        assert_eq!(blocked_ui.status, GoalEnvelopeUiStatus::Blocked);
        assert!(!blocked_ui.reason_codes.is_empty());

        let validated = validated_projection(goal_id, &proposal, &valid_context()).unwrap();
        assert_eq!(
            goal_ui_projection(&validated, None).unwrap().status,
            GoalEnvelopeUiStatus::Validated
        );
        let frozen = frozen_projection(&validated, validated.revision().unwrap()).unwrap();
        assert_eq!(
            goal_ui_projection(&frozen, None).unwrap().status,
            GoalEnvelopeUiStatus::Frozen
        );

        let failed_invocation = verifier_invocation(
            goal_id,
            Uuid::from_u128(206),
            ToolExecutionStatus::Failed,
            false,
            "rendered-brief",
            "operating-brief",
        );
        let failed_evidence =
            completion_evidence_from_tool_invocation(&frozen, &failed_invocation).unwrap();
        let verification_blocked = completion_projection(&frozen, &failed_evidence).unwrap();
        assert_eq!(
            goal_ui_projection(&frozen, Some(&verification_blocked))
                .unwrap()
                .status,
            GoalEnvelopeUiStatus::VerificationBlocked
        );

        let passed_invocation = verifier_invocation(
            goal_id,
            Uuid::from_u128(207),
            ToolExecutionStatus::Succeeded,
            true,
            "rendered-brief",
            "operating-brief",
        );
        let passed_evidence =
            completion_evidence_from_tool_invocation(&frozen, &passed_invocation).unwrap();
        let complete = completion_projection(&frozen, &passed_evidence).unwrap();
        assert_eq!(
            goal_ui_projection(&frozen, Some(&complete)).unwrap().status,
            GoalEnvelopeUiStatus::Complete
        );
    }

    #[test]
    fn missing_failed_stale_unknown_duplicate_and_artifact_mismatch_fail_closed() {
        let store = EventStore::open_memory().unwrap();
        let goal_id = Uuid::from_u128(102);
        let frozen = frozen_goal(&store, goal_id);

        let missing = completion_projection(&frozen, &[]).unwrap();
        assert_eq!(missing.status, GoalCompletionStatus::VerificationBlocked);
        assert!(missing
            .failure_codes
            .contains(&GoalCompletionCode::MissingVerifierEvidence));
        assert!(missing
            .failure_codes
            .contains(&GoalCompletionCode::MissingArtifactEvidence));

        let failed_invocation = verifier_invocation(
            goal_id,
            Uuid::from_u128(202),
            ToolExecutionStatus::Failed,
            false,
            "rendered-brief",
            "operating-brief",
        );
        let failed_evidence =
            completion_evidence_from_tool_invocation(&frozen, &failed_invocation).unwrap();
        let failed = completion_projection(&frozen, &failed_evidence).unwrap();
        assert!(failed
            .failure_codes
            .contains(&GoalCompletionCode::EvidenceFailed));

        let passed_invocation = verifier_invocation(
            goal_id,
            Uuid::from_u128(203),
            ToolExecutionStatus::Succeeded,
            true,
            "rendered-brief",
            "operating-brief",
        );
        let exact = completion_evidence_from_tool_invocation(&frozen, &passed_invocation)
            .unwrap()
            .remove(0);

        let mut wrong_goal = exact.clone();
        wrong_goal.goal_id = Uuid::from_u128(999);
        assert!(completion_projection(&frozen, &[wrong_goal])
            .unwrap()
            .failure_codes
            .contains(&GoalCompletionCode::EvidenceGoalMismatch));

        let mut wrong_revision = exact.clone();
        wrong_revision.revision = "2".repeat(64);
        assert!(completion_projection(&frozen, &[wrong_revision])
            .unwrap()
            .failure_codes
            .contains(&GoalCompletionCode::EvidenceRevisionMismatch));

        let mut wrong_fingerprint = exact.clone();
        wrong_fingerprint.frozen_fingerprint = "3".repeat(64);
        assert!(completion_projection(&frozen, &[wrong_fingerprint])
            .unwrap()
            .failure_codes
            .contains(&GoalCompletionCode::EvidenceFingerprintMismatch));

        let mut unknown = exact.clone();
        unknown.verifier_id = "unknown-verifier".to_string();
        assert!(completion_projection(&frozen, &[unknown])
            .unwrap()
            .failure_codes
            .contains(&GoalCompletionCode::UnknownEvidence));

        let mut duplicate = exact.clone();
        duplicate.evidence_id = Uuid::from_u128(204);
        assert!(completion_projection(&frozen, &[exact.clone(), duplicate])
            .unwrap()
            .failure_codes
            .contains(&GoalCompletionCode::DuplicateEvidence));

        let mut wrong_artifact = exact;
        wrong_artifact.artifact_ids = vec!["different-artifact".to_string()];
        let artifact_mismatch = completion_projection(&frozen, &[wrong_artifact]).unwrap();
        assert!(artifact_mismatch
            .failure_codes
            .contains(&GoalCompletionCode::ArtifactIdentityMismatch));
        assert!(artifact_mismatch
            .failure_codes
            .contains(&GoalCompletionCode::MissingArtifactEvidence));
    }

    #[test]
    fn frozen_revision_change_invalidates_old_completion_without_claiming_legacy_complete() {
        let store = EventStore::open_memory().unwrap();
        let goal_id = Uuid::from_u128(103);
        frozen_goal(&store, goal_id);
        let invocation = verifier_invocation(
            goal_id,
            Uuid::from_u128(205),
            ToolExecutionStatus::Succeeded,
            true,
            "rendered-brief",
            "operating-brief",
        );
        persist_bound_verifier_invocation(&store, &invocation);
        assert_eq!(
            store
                .record_goal_completion_for_tool_invocation(&invocation)
                .unwrap()
                .unwrap()
                .status,
            GoalCompletionStatus::Complete
        );

        let mut changed = valid_proposal();
        changed
            .constraints
            .push("Use only locally reviewed sources.".to_string());
        let validated = store
            .submit_goal_proposal(goal_id, &changed, &valid_context())
            .unwrap();
        store
            .freeze_goal_envelope(goal_id, validated.revision().unwrap())
            .unwrap();
        assert!(store.goal_completion_projection(goal_id).unwrap().is_none());
        let ui = store.goal_envelope_ui_projection(goal_id).unwrap().unwrap();
        assert_eq!(ui.status, GoalEnvelopeUiStatus::Frozen);
        assert!(ui.completion_fingerprint.is_none());
    }

    #[test]
    fn legacy_database_defaults_to_no_goal_and_cannot_forge_completion_authority() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("legacy.sqlite3");
        {
            let connection = Connection::open(&database).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE kernel_events (id TEXT PRIMARY KEY NOT NULL, event_type TEXT NOT NULL, payload_json TEXT NOT NULL, created_at TEXT NOT NULL);",
                )
                .unwrap();
        }
        let store = EventStore::open(&database).unwrap();
        assert!(store
            .goal_envelope_projection(Uuid::from_u128(11))
            .unwrap()
            .is_none());
        assert!(store
            .goal_completion_projection(Uuid::from_u128(11))
            .unwrap()
            .is_none());
        let forged = json!({
            "schema_version": GOAL_LIFECYCLE_SCHEMA_VERSION,
            "goal_id": Uuid::from_u128(11),
            "state": {"status": "completed", "approved": true}
        });
        assert!(serde_json::from_value::<GoalLifecycleProjection>(forged).is_err());
        drop(store);
        assert!(fs::metadata(database).is_ok());
    }
}
