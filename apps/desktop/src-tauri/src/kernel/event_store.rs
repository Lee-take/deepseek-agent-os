#![allow(dead_code)]

mod artifact;
mod computer_use;
mod read_execution;
mod revocation;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::kernel::agent_context::AgentContextReceipt;
use crate::kernel::agent_run::{
    AgentRunArtifactRecord, AgentRunCancelRequest, AgentRunClaim, AgentRunContinuationQueued,
    AgentRunExecutionContext, AgentRunFinish, AgentRunGuidanceApplied, AgentRunQueuedGuidance,
    AgentRunRecord, AgentRunRecovery, AgentRunRecoverySweep, AgentRunResourceAccess,
    AgentRunResourceClaim, AgentRunResourceRelease, AgentRunRole, AgentRunStart, AgentRunStatus,
    AgentRunStepRecord, AgentRunStepStatus, AgentRunTransition, AGENT_RUN_MAX_PARALLEL_SUBAGENTS,
};
use crate::kernel::automation::{
    automation_run_transition_allowed, next_scheduled_at, trigger_window_key, AutomationCheckpoint,
    AutomationDefinition, AutomationDefinitionStatus, AutomationRun, AutomationRunStatus,
    MissedRunPolicy, ReviewQueueItem,
};
use crate::kernel::capability::{CapabilityInvocation, CapabilityInvocationStatus};
use crate::kernel::connectors::catalog::ConnectorSyncHealthSnapshot;
use crate::kernel::connectors::domain::{CalendarEvent, MailMessage};
use crate::kernel::connectors::landing::{
    connector_attachment_landing_fingerprint, connector_attachment_workspace_binding,
    ConnectorAttachmentCleanupCandidate, ConnectorAttachmentDownloadPermit,
    ConnectorAttachmentLandingReceipt, ConnectorAttachmentMetadata, LandedConnectorAttachment,
    StagedConnectorAttachment,
};
use crate::kernel::connectors::oauth::{
    ConnectorAuthorizationIntent, ConnectorAuthorizationSession, ConnectorAuthorizationStatus,
};
use crate::kernel::connectors::provider::ConnectorProviderFailure;
use crate::kernel::connectors::reconciliation::{
    ConnectorReconcilerRegistry, EmptyConnectorReconcilerRegistry,
};
use crate::kernel::connectors::revocation::ConnectorRevocationPhase;
use crate::kernel::connectors::runtime_registry::ConnectorSyncRegistry;
use crate::kernel::connectors::sync::{
    ConnectorSyncChange, ConnectorSyncContinuation, ConnectorSyncFailure, ConnectorSyncPage,
    ConnectorSyncPlan, ConnectorSyncProjectionSummary, ConnectorSyncReceipt, ConnectorSyncState,
    ConnectorSyncStateReceipt, ConnectorSyncStateRecovery, MAX_SYNC_PROJECTION_BYTES_PER_ACCOUNT,
    MAX_SYNC_PROJECTION_ITEMS_PER_ACCOUNT, MAX_SYNC_PROJECTION_ITEMS_PER_STREAM,
    MAX_SYNC_PROJECTION_ITEM_BYTES, MAX_SYNC_STREAMS_PER_ACCOUNT, MAX_SYNC_STREAM_IDLE_DAYS,
};
use crate::kernel::connectors::{
    bind_connector_invocation_to_tool_record, bind_running_connector_invocation_to_tool_record,
    connector_invocation_transition_allowed, ConnectorAccount, ConnectorCapability,
    ConnectorCredentialDeleteOutcome, ConnectorCredentialHandle, ConnectorDisconnectPhase,
    ConnectorDisconnectReceipt, ConnectorDisconnectSource, ConnectorDisconnectTicket,
    ConnectorEvidenceRef, ConnectorHealth, ConnectorInvocation, ConnectorInvocationStatus,
    ConnectorMutationReceipt, ConnectorRecoveryAcceptance, ConnectorRecoveryAction,
    ConnectorRecoveryExternalEffectState, ConnectorRecoveryItem, ConnectorRecoveryKind,
    ConnectorRecoveryNextStepCode, ConnectorRecoveryReasonCode, ConnectorRecoveryStatus,
    ConnectorRecoverySyncCapability, ConnectorSecret,
};
use crate::kernel::deepseek::DeepSeekChatTelemetry;
use crate::kernel::expert_team::{
    parent_input_revision, resources_conflict, validate_team_plan, ExpertAttemptResult,
    ExpertMergeReceipt, ExpertTeamPlanItem, EXPERT_TEAM_MAX_TOTAL_ATTEMPTS,
};
use crate::kernel::models::{
    AccessMode, KernelEvent, MemoryCandidate, MemoryCandidateMergePreview, MemoryCandidateRecord,
    MemoryCandidateReplacePreview, MemoryCandidateResolution, MemoryCandidateSource,
    MemoryCandidateStatus, MemoryConflictSummary, MemoryMaintenanceActionKind,
    MemoryMaintenanceReviewAction, MemoryRecord, MemoryRecordDeletion, MemoryRecordLink,
    MemoryRecordLinkSummary, MemoryRecordUpdate, MemoryRelationKind, MemorySearchMatch,
    MemorySearchMatchSource, MemorySelectedFeedback, MemorySelectedFeedbackKind, TaskRecord,
};
use crate::kernel::policy::{
    capability_risk, request_capability_access, CapabilityAccessRecord, CapabilityAccessRequest,
    CapabilityAccessStatus, CapabilityGrantState, CapabilityKind, PermissionAuditEntry,
    PermissionResolution, PolicyDecision, RiskLevel,
};
use crate::kernel::skill::{
    sha256_hex, SkillActivationContext, SkillEnablementChange, SkillEnablementStatus,
    SkillExecutionRecord, SkillInstallationRecord, SkillRecord, SkillTrustLevel, SkillTrustReset,
    SkillUninstallRecord, SkillUpdateCheckRecord, SkillUpdateCheckStatus, SkillUpdateFailureRecord,
    SkillUpdateRecord, SkillUpdateState,
};
use crate::kernel::soul::AgentSoulProfileUpdateAudit;
use crate::kernel::tool_runtime::{
    prepare_tool_execution, tool_approval_preview, tool_request_fingerprint, ToolEvidence,
    ToolExecutionStatus, ToolInvocationRecord, ToolVerificationResult,
    CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID,
};
use crate::kernel::work_package::{
    redact_operations_briefing_run_for_package_export, WorkPackage, WorkPackageImportPreview,
    WorkPackageImportSummary, WorkPackageMemoryCandidateImportPreview,
    WorkPackageMemoryCandidateImportSummary, WorkPackageOperationsBriefingImportPreview,
    WorkPackageOperationsBriefingImportSummary, WorkPackageTaskImportPreview,
    WorkPackageWorkflowTemplateImportPreview, WorkPackageWorkflowTemplateImportSummary,
};
use crate::kernel::workflow::{OperationsBriefingRun, WorkflowTemplatePackage};

pub const CAPABILITY_ACCESS_REQUESTED_EVENT: &str = "capability_access.requested";
pub const CAPABILITY_INVOCATION_RECORDED_EVENT: &str = "capability_invocation.recorded";
pub const AGENT_CONTEXT_RECEIPT_RECORDED_EVENT: &str = "agent_context_receipt_recorded";
pub const AGENT_RUN_CANCEL_REQUESTED_EVENT: &str = "agent_run.cancel_requested";
pub const AGENT_RUN_CLAIMED_EVENT: &str = "agent_run.claimed";
pub const AGENT_RUN_CONTINUATION_QUEUED_EVENT: &str = "agent_run.continuation_queued";
pub const AGENT_RUN_EXECUTION_CONTEXT_RECORDED_EVENT: &str = "agent_run.execution_context_recorded";
pub const AGENT_RUN_RECOVERED_EVENT: &str = "agent_run.recovered";
pub const AGENT_RUN_FINISHED_EVENT: &str = "agent_run.finished";
pub const AGENT_RUN_GUIDANCE_APPLIED_EVENT: &str = "agent_run.guidance_applied";
pub const AGENT_RUN_GUIDANCE_QUEUED_EVENT: &str = "agent_run.guidance_queued";
pub const AGENT_RUN_STARTED_EVENT: &str = "agent_run.started";
pub const AGENT_RUN_STEP_RECORDED_EVENT: &str = "agent_run.step_recorded";
pub const AGENT_RUN_ARTIFACT_RECORDED_EVENT: &str = "agent_run.artifact_recorded";
pub const AGENT_RUN_TRANSITIONED_EVENT: &str = "agent_run.transitioned";
pub const AGENT_RUN_RESOURCE_CLAIMED_EVENT: &str = "agent_run.resource_claimed";
pub const AGENT_RUN_RESOURCE_RELEASED_EVENT: &str = "agent_run.resource_released";
pub const EXPERT_ATTEMPT_RESULT_RECORDED_EVENT: &str = "expert_team.attempt_result_recorded";
pub const EXPERT_MERGE_RECORDED_EVENT: &str = "expert_team.merge_recorded";
pub const DEEPSEEK_CHAT_TELEMETRY_RECORDED_EVENT: &str = "deepseek_chat.telemetry_recorded";
pub const MEMORY_CANDIDATE_PROPOSED_EVENT: &str = "memory_candidate.proposed";
pub const MEMORY_CANDIDATE_RESOLVED_EVENT: &str = "memory_candidate.resolved";
pub const MEMORY_RECORD_CREATED_EVENT: &str = "memory_record.created";
pub const MEMORY_RECORD_UPDATED_EVENT: &str = "memory_record.updated";
pub const MEMORY_RECORD_DELETED_EVENT: &str = "memory_record.deleted";
pub const MEMORY_RECORD_LINKED_EVENT: &str = "memory_record.linked";
pub const MEMORY_SELECTED_FEEDBACK_RECORDED_EVENT: &str = "memory_selected_feedback.recorded";
pub const MEMORY_MAINTENANCE_REVIEW_ACTION_RECORDED_EVENT: &str =
    "memory_maintenance_review.action_recorded";
pub const OPERATIONS_BRIEFING_RUN_RECORDED_EVENT: &str = "operations_briefing.run_recorded";
pub const PERMISSION_AUDIT_RECORDED_EVENT: &str = "permission_audit.recorded";
pub const PERMISSION_RESOLUTION_RECORDED_EVENT: &str = "permission_resolution.recorded";
pub const SKILL_ENABLEMENT_CHANGED_EVENT: &str = "skill.enablement_changed";
pub const SKILL_EXECUTION_RECORDED_EVENT: &str = "skill.execution_recorded";
pub const SKILL_INSTALLED_EVENT: &str = "skill.installed";
pub const SKILL_TRUST_RESET_EVENT: &str = "skill.trust_reset";
pub const SKILL_UNINSTALLED_EVENT: &str = "skill.uninstalled";
pub const SKILL_UPDATED_EVENT: &str = "skill.updated";
pub const SOUL_PROFILE_UPDATED_EVENT: &str = "soul_profile.updated";
pub const SKILL_UPDATE_CHECKED_EVENT: &str = "skill.update_checked";
pub const SKILL_UPDATE_FAILED_EVENT: &str = "skill.update_failed";
pub const TASK_RECORD_CREATED_EVENT: &str = "task_record.created";
pub const TOOL_INVOCATION_RECORDED_EVENT: &str = "tool_invocation.recorded";
pub const CONNECTOR_ATTACHMENT_LANDED_EVENT: &str = "connector.attachment.landed";
pub const CONNECTOR_RECOVERY_RETRY_QUEUED_EVENT: &str = "connector.recovery.retry_queued";
const CONNECTOR_ATTACHMENT_RETENTION_DAYS: i64 = 30;
const CONNECTOR_ATTACHMENT_RECOVERY_LEASE_SECONDS: i64 = 300;
const CONNECTOR_RECONCILIATION_LEASE_SECONDS: i64 = 300;
const CONNECTOR_RECONCILIATION_MAX_BACKOFF_SECONDS: i64 = 3600;
const CONNECTOR_SYNC_RECOVERY_LEASE_SECONDS: i64 = 300;
const MAX_RETAINED_ATTACHMENTS_PER_WORKSPACE: i64 = 32;
const MAX_RETAINED_ATTACHMENT_BYTES_PER_WORKSPACE: i64 = 256 * 1024 * 1024;
pub const WORKFLOW_TEMPLATE_PACKAGE_IMPORTED_EVENT: &str = "workflow_template_package.imported";

#[derive(Debug, Error)]
pub enum EventStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("timestamp parse error: {0}")]
    Timestamp(#[from] chrono::ParseError),

    #[error("uuid parse error: {0}")]
    Uuid(#[from] uuid::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid state: {0}")]
    InvalidState(String),
}

pub type EventStoreResult<T> = Result<T, EventStoreError>;

fn expert_attempt_ready(record: &AgentRunRecord, records: &[AgentRunRecord]) -> bool {
    let Some(contract) = record.expert_contract.as_ref() else {
        return true;
    };
    let Some(parent) = records
        .iter()
        .find(|candidate| candidate.id == contract.parent_run_id)
    else {
        return false;
    };
    if parent_input_revision(&parent.prompt) != contract.parent_input_revision {
        return false;
    }
    let team_records = records
        .iter()
        .filter(|candidate| {
            candidate
                .expert_contract
                .as_ref()
                .is_some_and(|candidate_contract| candidate_contract.team_id == contract.team_id)
        })
        .collect::<Vec<_>>();
    if team_records
        .iter()
        .filter(|candidate| candidate.status == AgentRunStatus::Running)
        .count()
        >= AGENT_RUN_MAX_PARALLEL_SUBAGENTS
    {
        return false;
    }
    for dependency_key in &contract.depends_on {
        let latest = team_records
            .iter()
            .filter(|candidate| {
                candidate
                    .expert_contract
                    .as_ref()
                    .is_some_and(|candidate_contract| {
                        candidate_contract.key.eq_ignore_ascii_case(dependency_key)
                    })
            })
            .max_by_key(|candidate| {
                candidate
                    .expert_contract
                    .as_ref()
                    .map(|candidate_contract| candidate_contract.attempt)
                    .unwrap_or(0)
            });
        if latest.is_none_or(|dependency| {
            dependency.status != AgentRunStatus::Completed
                || dependency
                    .expert_result
                    .as_ref()
                    .is_none_or(|result| !result.passed())
        }) {
            return false;
        }
    }
    !team_records.iter().any(|candidate| {
        candidate.id != record.id
            && candidate.status == AgentRunStatus::Running
            && candidate
                .expert_contract
                .as_ref()
                .is_some_and(|candidate_contract| {
                    resources_conflict(&contract.resources, &candidate_contract.resources)
                })
    })
}

fn ensure_sqlite_column(
    connection: &Connection,
    table: &str,
    column: &str,
    migration: &str,
) -> EventStoreResult<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|candidate| candidate == column) {
        connection.execute(migration, [])?;
    }
    Ok(())
}

fn migrate_connector_authorization_session_rows(connection: &Connection) -> EventStoreResult<()> {
    let mut statement = connection.prepare(
        r#"SELECT id, session_json, expires_at, consumed_at
           FROM connector_authorization_sessions"#,
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for (id, json, expires_at, consumed_at) in rows {
        let value: serde_json::Value = serde_json::from_str(&json)?;
        let object = value.as_object().ok_or_else(|| {
            EventStoreError::InvalidState("legacy OAuth authorization row is invalid".to_string())
        })?;
        let legacy = [
            "status",
            "revision",
            "cleanup_required",
            "cleanup_completed_at",
        ]
        .iter()
        .any(|field| !object.contains_key(*field));
        if !legacy {
            continue;
        }
        let mut session: ConnectorAuthorizationSession = serde_json::from_value(value)?;
        let projected_id = Uuid::parse_str(&id)?;
        let projected_expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        let projected_consumed_at = consumed_at
            .as_deref()
            .map(|value| DateTime::parse_from_rfc3339(value).map(|value| value.with_timezone(&Utc)))
            .transpose()?;
        if session.id != projected_id
            || session.expires_at != projected_expires_at
            || session.consumed_at != projected_consumed_at
        {
            return Err(EventStoreError::InvalidState(
                "legacy OAuth authorization projection is invalid".to_string(),
            ));
        }
        if session.status == ConnectorAuthorizationStatus::Pending && session.consumed_at.is_some()
        {
            session.status = ConnectorAuthorizationStatus::Completed;
        }
        connection.execute(
            r#"UPDATE connector_authorization_sessions
               SET session_json = ?2, status = ?3, revision = ?4,
                   cleanup_required = ?5, cleanup_completed_at = ?6
               WHERE id = ?1"#,
            params![
                id,
                serde_json::to_string(&session)?,
                serde_json::to_string(&session.status)?,
                i64::try_from(session.revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
                if session.cleanup_required { 1i64 } else { 0i64 },
                session
                    .cleanup_completed_at
                    .map(|value| value.to_rfc3339_opts(SecondsFormat::Nanos, true)),
            ],
        )?;
    }
    Ok(())
}

fn durable_connector_attachment_metadata(
    metadata: &ConnectorAttachmentMetadata,
) -> ConnectorAttachmentMetadata {
    let mut durable = metadata.clone();
    durable.parent_remote_ref = "redacted:parent".to_string();
    durable.attachment_remote_ref = "redacted:attachment".to_string();
    durable
}

fn connector_attachment_recovery_fingerprint(
    landing_id: &str,
    failure_kind: Option<&str>,
    workspace_identity: &str,
    storage_identity: &str,
    updated_at: &str,
    recovery_revision: i64,
) -> String {
    sha256_hex(
        format!(
            "ds-agent.connector-recovery.v2\0{landing_id}\0{}\0{workspace_identity}\0{storage_identity}\0{updated_at}\0{recovery_revision}",
            failure_kind.unwrap_or("")
        )
        .as_bytes(),
    )
}

fn connector_sync_recovery_item_id(
    account_id: &str,
    capability: &str,
    stream_fingerprint: &str,
) -> Uuid {
    let digest = Sha256::digest(
        format!(
            "ds-agent.connector-sync-recovery-item.v1\0{account_id}\0{capability}\0{stream_fingerprint}"
        )
        .as_bytes(),
    );
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn connector_sync_recovery_authority_hash(
    account: &ConnectorAccount,
    generation: u64,
    capability: ConnectorCapability,
) -> String {
    sha256_hex(
        format!(
            "ds-agent.connector-sync-recovery-authority.v1\0{}\0{}\0{}\0{}\0{}",
            account.provider_id,
            account.tenant_ref.as_deref().unwrap_or(""),
            serde_json::to_string(&account.credential_handle).unwrap_or_default(),
            generation,
            capability.contract_name(),
        )
        .as_bytes(),
    )
}

fn connector_sync_recovery_state_hash(state_json: &str) -> String {
    sha256_hex(format!("ds-agent.connector-sync-recovery-state.v1\0{state_json}").as_bytes())
}

fn connector_sync_recovery_action_revision(
    state_json: &str,
    account: &ConnectorAccount,
    generation: u64,
    capability: ConnectorCapability,
) -> String {
    sha256_hex(
        format!(
            "ds-agent.connector-sync-recovery-action.v1\0{}\0{}",
            connector_sync_recovery_state_hash(state_json),
            connector_sync_recovery_authority_hash(account, generation, capability),
        )
        .as_bytes(),
    )
}

fn connector_reconciliation_invocation_hash(invocation_json: &str) -> String {
    sha256_hex(
        format!("ds-agent.connector-reconciliation-invocation.v1\0{invocation_json}").as_bytes(),
    )
}

fn connector_authorization_action_token_hash(token: &str) -> String {
    sha256_hex(format!("ds-agent.connector-authorization-action-token.v1\0{token}").as_bytes())
}

fn connector_authorization_session_hash(session_json: &str) -> String {
    sha256_hex(format!("ds-agent.connector-authorization-session.v1\0{session_json}").as_bytes())
}

fn constant_time_text_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

fn connector_recovery_action_was_accepted(
    transaction: &Transaction<'_>,
    action_kind: &str,
    item_id: Uuid,
    action_revision: &str,
) -> EventStoreResult<bool> {
    Ok(transaction
        .query_row(
            r#"SELECT 1 FROM connector_recovery_action_receipts
               WHERE action_kind = ?1 AND item_id = ?2 AND action_revision = ?3"#,
            params![action_kind, item_id.to_string(), action_revision],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn record_connector_recovery_action_acceptance(
    transaction: &Transaction<'_>,
    action_kind: &str,
    item_id: Uuid,
    action_revision: &str,
    accepted_at: DateTime<Utc>,
) -> EventStoreResult<()> {
    let inserted = transaction.execute(
        r#"INSERT INTO connector_recovery_action_receipts
           (action_kind, item_id, action_revision, accepted_at, retain_until)
           VALUES (?1, ?2, ?3, ?4, ?5)"#,
        params![
            action_kind,
            item_id.to_string(),
            action_revision,
            accepted_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            (accepted_at + Duration::days(90)).to_rfc3339_opts(SecondsFormat::Nanos, true),
        ],
    )?;
    if inserted != 1 {
        return Err(EventStoreError::InvalidState(
            "connector recovery action receipt could not be recorded".to_string(),
        ));
    }
    Ok(())
}

fn ensure_connector_sync_account_active(
    transaction: &Transaction<'_>,
    state: &ConnectorSyncState,
) -> EventStoreResult<()> {
    let active = transaction
        .query_row(
            r#"SELECT 1
               FROM connector_accounts AS account
               JOIN connector_account_generations AS generation
                 ON generation.account_id = account.id
               WHERE account.id = ?1 AND account.health = ?2 AND generation.generation = ?3"#,
            params![
                state.account_id().to_string(),
                serde_json::to_string(&ConnectorHealth::Connected)?,
                i64::try_from(state.account_generation()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector account generation is too large".to_string(),
                    )
                })?,
            ],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !active {
        return Err(EventStoreError::InvalidState(
            "connector account changed while sync was in flight".to_string(),
        ));
    }
    Ok(())
}

fn validate_connector_sync_recovery_claim(
    transaction: &Transaction<'_>,
    claim: &ConnectorSyncRecoveryClaim,
    now: DateTime<Utc>,
) -> EventStoreResult<()> {
    if claim.claim_expires_at <= now {
        return Err(EventStoreError::InvalidState(
            "connector sync recovery claim expired".to_string(),
        ));
    }
    let (account_json, generation) = transaction.query_row(
        r#"SELECT account.account_json, generation.generation
           FROM connector_accounts AS account
           JOIN connector_account_generations AS generation ON generation.account_id = account.id
           WHERE account.id = ?1"#,
        params![claim.account.id.to_string()],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )?;
    let account: ConnectorAccount = serde_json::from_str(&account_json)?;
    if account != claim.account
        || account.health != ConnectorHealth::Connected
        || u64::try_from(generation).ok() != Some(claim.state.account_generation())
        || !account
            .granted_capabilities
            .contains(&claim.state.capability())
    {
        return Err(EventStoreError::InvalidState(
            "connector sync recovery authority changed".to_string(),
        ));
    }
    let (state_json, revision, request_json) = transaction.query_row(
        r#"SELECT state_json, revision, request_json FROM connector_sync_streams
           WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3"#,
        params![
            claim.state.account_id().to_string(),
            claim.state.capability().contract_name(),
            claim.state.stream_fingerprint(),
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let state = ConnectorSyncState::from_persistence_json(state_json)
        .map_err(EventStoreError::InvalidState)?;
    let plan =
        ConnectorSyncPlan::from_persistence_json(request_json.as_deref().ok_or_else(|| {
            EventStoreError::InvalidState("connector sync recovery plan is missing".to_string())
        })?)
        .map_err(EventStoreError::InvalidState)?;
    if state != claim.state
        || u64::try_from(revision).ok() != Some(claim.state.revision())
        || plan != claim.plan
    {
        return Err(EventStoreError::InvalidState(
            "connector sync recovery binding changed".to_string(),
        ));
    }
    let live: i64 = transaction.query_row(
        r#"SELECT count(*) FROM connector_sync_recovery_jobs
           WHERE id = ?1 AND status = 'running' AND claim_id = ?2
             AND claim_expires_at = ?3 AND claim_expires_at > ?4
             AND account_id = ?5 AND account_generation = ?6
             AND capability = ?7 AND stream_fingerprint = ?8
             AND expected_state_revision = ?9"#,
        params![
            claim.job_id.to_string(),
            claim.claim_id.to_string(),
            claim
                .claim_expires_at
                .to_rfc3339_opts(SecondsFormat::Nanos, true),
            now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            claim.state.account_id().to_string(),
            i64::try_from(claim.state.account_generation()).map_err(|_| {
                EventStoreError::InvalidState("connector sync generation is too large".to_string())
            })?,
            claim.state.capability().contract_name(),
            claim.state.stream_fingerprint(),
            i64::try_from(claim.state.revision()).map_err(|_| {
                EventStoreError::InvalidState("connector sync revision is too large".to_string())
            })?,
        ],
        |row| row.get(0),
    )?;
    if live != 1 {
        return Err(EventStoreError::InvalidState(
            "connector sync recovery claim was lost".to_string(),
        ));
    }
    Ok(())
}

fn load_connector_reconciliation_binding(
    transaction: &Transaction<'_>,
    invocation: &ConnectorInvocation,
    projected_generation: i64,
) -> EventStoreResult<(ConnectorAccount, ToolInvocationRecord)> {
    let generation = invocation.account_generation.ok_or_else(|| {
        EventStoreError::InvalidState(
            "legacy connector reconciliation has no frozen account generation".to_string(),
        )
    })?;
    if i64::try_from(generation).ok() != Some(projected_generation)
        || invocation.status != ConnectorInvocationStatus::ReconciliationRequired
        || !invocation.capability.external_mutation()
        || invocation.mutation.as_ref().map_or(true, |mutation| {
            mutation.account_generation != Some(generation)
                || mutation.provider_id != invocation.provider_id
                || mutation.account_id != invocation.account_id
                || mutation.capability != invocation.capability
                || mutation.idempotency_key != invocation.idempotency_key
        })
    {
        return Err(EventStoreError::InvalidState(
            "connector reconciliation projection is inconsistent".to_string(),
        ));
    }
    let (account_json, current_generation) = transaction
        .query_row(
            r#"SELECT account.account_json, generation.generation
               FROM connector_accounts AS account
               JOIN connector_account_generations AS generation
                 ON generation.account_id = account.id
               WHERE account.id = ?1 AND account.health = ?2"#,
            params![
                invocation.account_id.to_string(),
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            EventStoreError::InvalidState(
                "connector account is not ready for reconciliation".to_string(),
            )
        })?;
    let account: ConnectorAccount = serde_json::from_str(&account_json)?;
    if current_generation != projected_generation
        || account.provider_id != invocation.provider_id
        || account.health != ConnectorHealth::Connected
        || !account
            .granted_capabilities
            .contains(&invocation.capability)
    {
        return Err(EventStoreError::InvalidState(
            "connector account changed before reconciliation".to_string(),
        ));
    }
    let tool_invocation_id = invocation.tool_invocation_id.ok_or_else(|| {
        EventStoreError::InvalidState(
            "connector reconciliation is missing its exact Tool".to_string(),
        )
    })?;
    let (tool_json, status_json, fingerprint, approval_request_id) = transaction
        .query_row(
            r#"SELECT invocation_json, status, request_fingerprint, approval_request_id
               FROM tool_invocation_state WHERE id = ?1"#,
            params![tool_invocation_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| EventStoreError::NotFound("connector reconciliation Tool".to_string()))?;
    let tool: ToolInvocationRecord = serde_json::from_str(&tool_json)?;
    let approval_request_id = approval_request_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()?
        .ok_or_else(|| {
            EventStoreError::InvalidState(
                "connector reconciliation approval binding is missing".to_string(),
            )
        })?;
    if serde_json::from_str::<ToolExecutionStatus>(&status_json)? != tool.status
        || fingerprint != tool.request_fingerprint
        || tool.approval_request_id != Some(approval_request_id)
    {
        return Err(EventStoreError::InvalidState(
            "connector reconciliation Tool projection is inconsistent".to_string(),
        ));
    }
    bind_running_connector_invocation_to_tool_record(invocation, &tool)
        .map_err(EventStoreError::InvalidState)?;
    let (request_json, effective_status_json) = transaction
        .query_row(
            r#"SELECT request_json, effective_status
               FROM capability_access_state WHERE request_id = ?1"#,
            params![approval_request_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            EventStoreError::NotFound("connector reconciliation approval".to_string())
        })?;
    let approval_request: CapabilityAccessRequest = serde_json::from_str(&request_json)?;
    let effective_status: CapabilityAccessStatus = serde_json::from_str(&effective_status_json)?;
    if approval_request.id != approval_request_id
        || approval_request.capability != CapabilityKind::ConnectorWrite
        || effective_status != CapabilityAccessStatus::Approved
    {
        return Err(EventStoreError::InvalidState(
            "connector reconciliation approval is not valid".to_string(),
        ));
    }
    let consumed = transaction
        .query_row(
            r#"SELECT 1 FROM connector_approval_consumptions
               WHERE request_id = ?1 AND connector_invocation_id = ?2"#,
            params![approval_request_id.to_string(), invocation.id.to_string(),],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !consumed {
        return Err(EventStoreError::InvalidState(
            "connector reconciliation approval was not consumed by this invocation".to_string(),
        ));
    }
    if let Some(automation_run_id) = invocation.automation_run_id {
        let (review_json, status_json) = transaction
            .query_row(
                r#"SELECT item_json, status FROM review_queue_items
                   WHERE automation_run_id = ?1"#,
                params![automation_run_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                EventStoreError::NotFound("connector reconciliation review".to_string())
            })?;
        let review: ReviewQueueItem = serde_json::from_str(&review_json)?;
        if serde_json::to_string(&review.status)? != status_json
            || review.status != crate::kernel::automation::ReviewQueueItemStatus::PendingApproval
            || review.tool_invocation_id != Some(tool_invocation_id)
            || review.preview_fingerprint.as_deref()
                != Some(invocation.request_fingerprint.as_str())
        {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation review binding changed".to_string(),
            ));
        }
    }
    Ok((account, tool))
}

struct ConnectorReconciliationRecoverySnapshot {
    invocation: ConnectorInvocation,
    invocation_json: String,
    account: ConnectorAccount,
    projected_generation: i64,
    next_reconciliation_at: String,
    attempt_count: i64,
    updated_at: String,
    recovery_revision: i64,
}

fn connector_reconciliation_recovery_action_revision(
    snapshot: &ConnectorReconciliationRecoverySnapshot,
) -> EventStoreResult<String> {
    let generation = u64::try_from(snapshot.projected_generation).map_err(|_| {
        EventStoreError::InvalidState(
            "connector reconciliation account generation is invalid".to_string(),
        )
    })?;
    Ok(sha256_hex(
        format!(
            "ds-agent.connector-reconciliation-recovery-action.v2\0{}\0{}\0{}\0{}\0{}\0{}",
            connector_reconciliation_invocation_hash(&snapshot.invocation_json),
            connector_sync_recovery_authority_hash(
                &snapshot.account,
                generation,
                snapshot.invocation.capability,
            ),
            snapshot.next_reconciliation_at,
            snapshot.attempt_count,
            snapshot.updated_at,
            snapshot.recovery_revision,
        )
        .as_bytes(),
    ))
}

fn load_connector_reconciliation_recovery_snapshot(
    transaction: &Transaction<'_>,
    invocation_id: Uuid,
) -> EventStoreResult<ConnectorReconciliationRecoverySnapshot> {
    let row = transaction
        .query_row(
            r#"SELECT account_id, account_generation, idempotency_key,
                      invocation_json, status, next_reconciliation_at,
                      reconciliation_attempt_count, updated_at,
                      reconciliation_claim_id, reconciliation_claim_expires_at,
                      recovery_revision
               FROM connector_invocations
               WHERE id = ?1 AND reconciliation_quarantine_code IS NULL"#,
            params![invocation_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, i64>(10)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            EventStoreError::NotFound("connector reconciliation recovery".to_string())
        })?;
    let (
        projected_account_id,
        projected_generation,
        projected_idempotency_key,
        invocation_json,
        projected_status,
        next_reconciliation_at,
        attempt_count,
        updated_at,
        claim_id,
        claim_expires_at,
        recovery_revision,
    ) = row;
    if claim_id.is_some()
        || claim_expires_at.is_some()
        || attempt_count < 0
        || recovery_revision < 0
    {
        return Err(EventStoreError::InvalidState(
            "connector reconciliation recovery is already claimed or invalid".to_string(),
        ));
    }
    let invocation: ConnectorInvocation = serde_json::from_str(&invocation_json)?;
    if invocation.id != invocation_id
        || invocation.account_id.to_string() != projected_account_id
        || invocation.idempotency_key != projected_idempotency_key
        || invocation
            .updated_at
            .to_rfc3339_opts(SecondsFormat::Nanos, true)
            != updated_at
        || serde_json::to_string(&invocation.status)? != projected_status
        || invocation.status != ConnectorInvocationStatus::ReconciliationRequired
    {
        return Err(EventStoreError::InvalidState(
            "connector reconciliation recovery projection is inconsistent".to_string(),
        ));
    }
    let next_reconciliation_at = next_reconciliation_at.ok_or_else(|| {
        EventStoreError::InvalidState(
            "connector reconciliation recovery has no scheduled verification".to_string(),
        )
    })?;
    DateTime::parse_from_rfc3339(&next_reconciliation_at)?;
    let (account, _) =
        load_connector_reconciliation_binding(transaction, &invocation, projected_generation)?;
    Ok(ConnectorReconciliationRecoverySnapshot {
        invocation,
        invocation_json,
        account,
        projected_generation,
        next_reconciliation_at,
        attempt_count,
        updated_at,
        recovery_revision,
    })
}

fn prune_connector_sync_retention(
    transaction: &Transaction<'_>,
    account_id: Uuid,
    now: DateTime<Utc>,
) -> EventStoreResult<()> {
    let account_id = account_id.to_string();
    let stale_before = (now - Duration::days(MAX_SYNC_STREAM_IDLE_DAYS))
        .to_rfc3339_opts(SecondsFormat::Nanos, true);
    transaction.execute(
        r#"DELETE FROM connector_sync_projection AS projection
           WHERE projection.account_id = ?1 AND EXISTS (
             SELECT 1 FROM connector_sync_streams AS stream
             WHERE stream.account_id = projection.account_id
               AND stream.capability = projection.capability
               AND stream.stream_fingerprint = projection.stream_fingerprint
               AND stream.updated_at < ?2
           )"#,
        params![account_id, stale_before],
    )?;
    transaction.execute(
        "DELETE FROM connector_sync_streams WHERE account_id = ?1 AND updated_at < ?2",
        params![account_id, stale_before],
    )?;
    transaction.execute(
        r#"DELETE FROM connector_sync_projection AS projection
           WHERE projection.account_id = ?1
             AND NOT EXISTS (
               SELECT 1 FROM (
                 SELECT capability, stream_fingerprint
                 FROM connector_sync_streams
                 WHERE account_id = ?1
                 ORDER BY updated_at DESC, rowid DESC
                 LIMIT ?2
               ) AS retained
               WHERE retained.capability = projection.capability
                 AND retained.stream_fingerprint = projection.stream_fingerprint
             )"#,
        params![
            account_id,
            i64::try_from(MAX_SYNC_STREAMS_PER_ACCOUNT).map_err(|_| {
                EventStoreError::InvalidState("connector sync stream budget is invalid".to_string())
            })?,
        ],
    )?;
    transaction.execute(
        r#"DELETE FROM connector_sync_streams
           WHERE account_id = ?1 AND rowid NOT IN (
             SELECT rowid FROM connector_sync_streams
             WHERE account_id = ?1
             ORDER BY updated_at DESC, rowid DESC
             LIMIT ?2
           )"#,
        params![
            account_id,
            i64::try_from(MAX_SYNC_STREAMS_PER_ACCOUNT).map_err(|_| {
                EventStoreError::InvalidState("connector sync stream budget is invalid".to_string())
            })?,
        ],
    )?;
    transaction.execute(
        r#"WITH ranked AS (
             SELECT rowid,
                    ROW_NUMBER() OVER (ORDER BY updated_at DESC, rowid DESC) AS item_number,
                    SUM(
                      LENGTH(CAST(COALESCE(item_json, '') AS BLOB))
                      + LENGTH(CAST(remote_ref AS BLOB))
                    ) OVER (ORDER BY updated_at DESC, rowid DESC) AS running_bytes
             FROM connector_sync_projection
             WHERE account_id = ?1
           )
           DELETE FROM connector_sync_projection
           WHERE rowid IN (
             SELECT rowid FROM ranked
             WHERE item_number > ?2 OR running_bytes > ?3
           )"#,
        params![
            account_id,
            i64::try_from(MAX_SYNC_PROJECTION_ITEMS_PER_ACCOUNT).map_err(|_| {
                EventStoreError::InvalidState(
                    "connector sync account item budget is invalid".to_string(),
                )
            })?,
            i64::try_from(MAX_SYNC_PROJECTION_BYTES_PER_ACCOUNT).map_err(|_| {
                EventStoreError::InvalidState(
                    "connector sync account byte budget is invalid".to_string(),
                )
            })?,
        ],
    )?;
    Ok(())
}

pub struct EventStore {
    conn: Connection,
}

pub(crate) struct ConnectorSyncRecoveryClaim {
    job_id: Uuid,
    claim_id: Uuid,
    claim_expires_at: DateTime<Utc>,
    account: ConnectorAccount,
    state: ConnectorSyncState,
    plan: ConnectorSyncPlan,
    attempt_count: u32,
}

impl ConnectorSyncRecoveryClaim {
    pub(crate) fn job_id(&self) -> Uuid {
        self.job_id
    }

    pub(crate) fn claim_id(&self) -> Uuid {
        self.claim_id
    }

    pub(crate) fn claim_expires_at(&self) -> DateTime<Utc> {
        self.claim_expires_at
    }

    pub(crate) fn account(&self) -> &ConnectorAccount {
        &self.account
    }

    pub(crate) fn state(&self) -> &ConnectorSyncState {
        &self.state
    }

    pub(crate) fn plan(&self) -> &ConnectorSyncPlan {
        &self.plan
    }

    pub(crate) fn attempt_count(&self) -> u32 {
        self.attempt_count
    }
}

pub(crate) struct ConnectorAuthorizationExchangeClaim {
    session: ConnectorAuthorizationSession,
    claim_id: Uuid,
    claim_expires_at: DateTime<Utc>,
    action_authority_handle: Option<ConnectorCredentialHandle>,
}

impl ConnectorAuthorizationExchangeClaim {
    pub(crate) fn session(&self) -> &ConnectorAuthorizationSession {
        &self.session
    }

    pub(crate) fn into_parts(self) -> (ConnectorAuthorizationSession, Uuid, DateTime<Utc>) {
        (self.session, self.claim_id, self.claim_expires_at)
    }

    pub(crate) fn claim_id(&self) -> Uuid {
        self.claim_id
    }

    pub(crate) fn action_authority_handle(&self) -> Option<&ConnectorCredentialHandle> {
        self.action_authority_handle.as_ref()
    }
}

impl std::ops::Deref for ConnectorAuthorizationExchangeClaim {
    type Target = ConnectorAuthorizationSession;

    fn deref(&self) -> &Self::Target {
        &self.session
    }
}

pub(crate) struct ConnectorAuthorizationCleanupClaim {
    session: ConnectorAuthorizationSession,
    claim_id: Uuid,
    claim_expires_at: DateTime<Utc>,
    action_authority_handle: Option<ConnectorCredentialHandle>,
}

pub(crate) enum ConnectorAuthorizationResolution {
    Approved(ConnectorAuthorizationExchangeClaim),
    Cancelled(ConnectorAuthorizationCleanupClaim),
}

pub(crate) struct ConnectorAuthorizationActionProvision {
    review_id: Uuid,
    authorization_id: Uuid,
    authority_handle: ConnectorCredentialHandle,
    authority: ConnectorSecret,
}

impl ConnectorAuthorizationActionProvision {
    pub(crate) fn review_id(&self) -> Uuid {
        self.review_id
    }

    pub(crate) fn authorization_id(&self) -> Uuid {
        self.authorization_id
    }

    pub(crate) fn into_vault_parts(self) -> (ConnectorCredentialHandle, ConnectorSecret) {
        (self.authority_handle, self.authority)
    }
}

pub(crate) struct ConnectorAuthorizationActiveReview {
    review_id: Uuid,
    authorization_id: Uuid,
    authority_handle: ConnectorCredentialHandle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectorAuthorizationReviewIntentState {
    Active,
    Approve,
    Cancel,
}

pub(crate) struct ConnectorAuthorizationReviewSnapshot {
    review_id: Uuid,
    session: ConnectorAuthorizationSession,
    intent_state: ConnectorAuthorizationReviewIntentState,
    authority_handle: Option<ConnectorCredentialHandle>,
    exchange_claim_live: bool,
    account: Option<ConnectorAccount>,
    account_binding_valid: bool,
}

impl ConnectorAuthorizationReviewSnapshot {
    pub(crate) fn review_id(&self) -> Uuid {
        self.review_id
    }

    pub(crate) fn session(&self) -> &ConnectorAuthorizationSession {
        &self.session
    }

    pub(crate) fn intent_state(&self) -> ConnectorAuthorizationReviewIntentState {
        self.intent_state
    }

    pub(crate) fn authority_handle(&self) -> Option<&ConnectorCredentialHandle> {
        self.authority_handle.as_ref()
    }

    pub(crate) fn exchange_claim_live(&self) -> bool {
        self.exchange_claim_live
    }

    pub(crate) fn connected_account(&self) -> Option<&ConnectorAccount> {
        self.account_binding_valid
            .then_some(self.account.as_ref())
            .flatten()
    }

    pub(crate) fn account_binding_valid(&self) -> bool {
        self.account_binding_valid
    }
}

pub(crate) struct ConnectorAuthorizationAuthorityCleanupClaim {
    review_id: Uuid,
    authorization_id: Uuid,
    authority_handle: ConnectorCredentialHandle,
    claim_id: Uuid,
    claim_expires_at: DateTime<Utc>,
}

impl ConnectorAuthorizationAuthorityCleanupClaim {
    pub(crate) fn review_id(&self) -> Uuid {
        self.review_id
    }

    pub(crate) fn authorization_id(&self) -> Uuid {
        self.authorization_id
    }

    pub(crate) fn authority_handle(&self) -> &ConnectorCredentialHandle {
        &self.authority_handle
    }
}

impl ConnectorAuthorizationActiveReview {
    pub(crate) fn review_id(&self) -> Uuid {
        self.review_id
    }

    pub(crate) fn authorization_id(&self) -> Uuid {
        self.authorization_id
    }

    pub(crate) fn authority_handle(&self) -> &ConnectorCredentialHandle {
        &self.authority_handle
    }
}

impl ConnectorAuthorizationCleanupClaim {
    pub(crate) fn session(&self) -> &ConnectorAuthorizationSession {
        &self.session
    }

    pub(crate) fn action_authority_handle(&self) -> Option<&ConnectorCredentialHandle> {
        self.action_authority_handle.as_ref()
    }
}

impl std::ops::Deref for ConnectorAuthorizationCleanupClaim {
    type Target = ConnectorAuthorizationSession;

    fn deref(&self) -> &Self::Target {
        &self.session
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectorAttachmentCleanupClaim {
    Owned(Uuid),
    Busy,
    KeepFile,
}

pub(crate) struct ConnectorAttachmentExecution {
    pub account: ConnectorAccount,
    pub metadata: ConnectorAttachmentMetadata,
    pub workspace_root: std::path::PathBuf,
    pub workspace_identity: String,
}

pub(crate) struct ConnectorReconciliationClaim {
    claim_id: Uuid,
    invocation: ConnectorInvocation,
    account: ConnectorAccount,
    attempt_count: u32,
    claim_expires_at: DateTime<Utc>,
}

impl ConnectorReconciliationClaim {
    pub(crate) fn claim_id(&self) -> Uuid {
        self.claim_id
    }

    pub(crate) fn invocation(&self) -> &ConnectorInvocation {
        &self.invocation
    }

    pub(crate) fn account(&self) -> &ConnectorAccount {
        &self.account
    }

    pub(crate) fn claim_expires_at(&self) -> DateTime<Utc> {
        self.claim_expires_at
    }

    pub(crate) fn attempt_count(&self) -> u32 {
        self.attempt_count
    }
}

impl EventStore {
    pub fn open(path: impl AsRef<Path>) -> EventStoreResult<Self> {
        let store = Self {
            conn: Connection::open(path)?,
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_memory() -> EventStoreResult<Self> {
        let store = Self {
            conn: Connection::open_in_memory()?,
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> EventStoreResult<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS kernel_events (
                id TEXT PRIMARY KEY NOT NULL,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_kernel_events_created_at
                ON kernel_events (created_at);

            CREATE TABLE IF NOT EXISTS capability_access_state (
                request_id TEXT PRIMARY KEY NOT NULL,
                request_json TEXT NOT NULL,
                resolution_json TEXT,
                effective_status TEXT NOT NULL,
                row_revision INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_capability_access_state_status
                ON capability_access_state (effective_status, updated_at);

            CREATE TABLE IF NOT EXISTS tool_invocation_state (
                id TEXT PRIMARY KEY NOT NULL,
                invocation_json TEXT NOT NULL,
                tool_id TEXT NOT NULL,
                capability TEXT NOT NULL,
                status TEXT NOT NULL,
                approval_request_id TEXT,
                request_fingerprint TEXT NOT NULL,
                row_revision INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_tool_invocation_state_status
                ON tool_invocation_state (status, updated_at);

            CREATE INDEX IF NOT EXISTS idx_tool_invocation_state_approval
                ON tool_invocation_state (approval_request_id, status);

            CREATE TABLE IF NOT EXISTS execution_projection_cursor (
                projection_name TEXT PRIMARY KEY NOT NULL,
                last_event_rowid INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS execution_projection_applied_events (
                event_id TEXT PRIMARY KEY NOT NULL,
                event_type TEXT NOT NULL,
                applied_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS capability_approval_consumptions (
                request_id TEXT PRIMARY KEY NOT NULL,
                capability_invocation_id TEXT NOT NULL UNIQUE,
                consumed_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS automation_definitions (
                id TEXT PRIMARY KEY NOT NULL,
                definition_json TEXT NOT NULL,
                status TEXT NOT NULL,
                revision INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS automation_runs (
                id TEXT PRIMARY KEY NOT NULL,
                definition_id TEXT NOT NULL,
                trigger_window_key TEXT NOT NULL,
                scheduled_for TEXT NOT NULL,
                run_json TEXT NOT NULL,
                status TEXT NOT NULL,
                definition_revision INTEGER NOT NULL DEFAULT 0,
                claimed_by TEXT,
                updated_at TEXT NOT NULL,
                UNIQUE (definition_id, trigger_window_key)
            );

            CREATE INDEX IF NOT EXISTS idx_automation_runs_due
                ON automation_runs (status, scheduled_for);

            CREATE TABLE IF NOT EXISTS automation_checkpoints (
                automation_run_id TEXT PRIMARY KEY NOT NULL,
                checkpoint_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS review_queue_items (
                id TEXT PRIMARY KEY NOT NULL,
                automation_run_id TEXT NOT NULL UNIQUE,
                item_json TEXT NOT NULL,
                status TEXT NOT NULL,
                revision INTEGER NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS connector_accounts (
                id TEXT PRIMARY KEY NOT NULL,
                provider_id TEXT NOT NULL,
                account_json TEXT NOT NULL,
                health TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_connector_accounts_provider
                ON connector_accounts (provider_id, health);

            CREATE TABLE IF NOT EXISTS connector_account_generations (
                account_id TEXT PRIMARY KEY NOT NULL,
                generation INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS connector_invocations (
                id TEXT PRIMARY KEY NOT NULL,
                account_id TEXT NOT NULL,
                account_generation INTEGER,
                idempotency_key TEXT NOT NULL,
                invocation_json TEXT NOT NULL,
                status TEXT NOT NULL,
                reconciliation_claim_id TEXT,
                reconciliation_claim_expires_at TEXT,
                next_reconciliation_at TEXT,
                reconciliation_attempt_count INTEGER NOT NULL DEFAULT 0,
                reconciliation_quarantine_code TEXT,
                reconciliation_quarantined_at TEXT,
                recovery_revision INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );

            CREATE UNIQUE INDEX IF NOT EXISTS idx_connector_invocation_idempotency
                ON connector_invocations (account_id, idempotency_key);

            CREATE TABLE IF NOT EXISTS connector_approval_consumptions (
                request_id TEXT PRIMARY KEY NOT NULL,
                connector_invocation_id TEXT NOT NULL UNIQUE,
                consumed_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS connector_attachment_approval_consumptions (
                request_id TEXT PRIMARY KEY NOT NULL,
                landing_id TEXT NOT NULL UNIQUE,
                consumed_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS connector_attachment_sources (
                request_id TEXT PRIMARY KEY NOT NULL,
                tool_invocation_id TEXT NOT NULL UNIQUE,
                request_fingerprint TEXT NOT NULL UNIQUE,
                metadata_json TEXT NOT NULL,
                account_generation INTEGER NOT NULL,
                workspace_root TEXT NOT NULL,
                workspace_identity TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS connector_attachment_active_sources (
                landing_id TEXT PRIMARY KEY NOT NULL,
                metadata_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS connector_attachment_landings (
                id TEXT PRIMARY KEY NOT NULL,
                account_id TEXT NOT NULL,
                account_generation INTEGER NOT NULL,
                metadata_json TEXT NOT NULL,
                size_bytes INTEGER NOT NULL DEFAULT 0,
                tool_invocation_id TEXT NOT NULL UNIQUE,
                approval_request_id TEXT NOT NULL UNIQUE,
                request_fingerprint TEXT NOT NULL,
                landing_fingerprint TEXT NOT NULL,
                workspace_root TEXT,
                workspace_identity TEXT,
                storage_identity TEXT,
                status TEXT NOT NULL,
                receipt_json TEXT,
                failure_kind TEXT,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT,
                expires_at TEXT,
                next_cleanup_at TEXT,
                cleanup_claim_id TEXT,
                cleanup_claim_expires_at TEXT,
                recovery_revision INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_connector_attachment_landings_status
                ON connector_attachment_landings (status, updated_at);

            CREATE TABLE IF NOT EXISTS connector_authorization_sessions (
                id TEXT PRIMARY KEY NOT NULL,
                session_json TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                consumed_at TEXT,
                status TEXT NOT NULL,
                revision INTEGER NOT NULL DEFAULT 0,
                cleanup_required INTEGER NOT NULL DEFAULT 0,
                cleanup_completed_at TEXT,
                exchange_claim_id TEXT,
                exchange_claim_expires_at TEXT,
                cleanup_claim_id TEXT,
                cleanup_claim_expires_at TEXT,
                account_id TEXT,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS connector_authorization_actions (
                authorization_id TEXT PRIMARY KEY NOT NULL,
                token_hash TEXT NOT NULL,
                session_hash TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL,
                review_id TEXT UNIQUE,
                authority_handle_json TEXT,
                action_status TEXT,
                activated_at TEXT,
                resolved_at TEXT,
                resolved_intent TEXT,
                authority_cleanup_required INTEGER NOT NULL DEFAULT 0,
                authority_cleanup_claim_id TEXT,
                authority_cleanup_claim_expires_at TEXT,
                authority_cleanup_completed_at TEXT
            );

            CREATE TABLE IF NOT EXISTS connector_sync_streams (
                account_id TEXT NOT NULL,
                capability TEXT NOT NULL,
                stream_fingerprint TEXT NOT NULL,
                state_json TEXT NOT NULL,
                revision INTEGER NOT NULL,
                request_json TEXT,
                last_successful_at TEXT,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, capability, stream_fingerprint)
            );

            CREATE TABLE IF NOT EXISTS connector_sync_projection (
                account_id TEXT NOT NULL,
                capability TEXT NOT NULL,
                stream_fingerprint TEXT NOT NULL,
                remote_ref TEXT NOT NULL,
                item_json TEXT,
                deleted INTEGER NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, capability, stream_fingerprint, remote_ref)
            );

            CREATE TABLE IF NOT EXISTS connector_sync_recovery_actions (
                item_id TEXT PRIMARY KEY NOT NULL,
                token_hash TEXT NOT NULL,
                account_id TEXT NOT NULL,
                account_generation INTEGER NOT NULL,
                capability TEXT NOT NULL,
                stream_fingerprint TEXT NOT NULL,
                stream_revision INTEGER NOT NULL,
                state_hash TEXT NOT NULL,
                authority_hash TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS connector_sync_recovery_jobs (
                id TEXT PRIMARY KEY NOT NULL,
                recovery_item_id TEXT NOT NULL,
                action_revision TEXT NOT NULL,
                account_id TEXT NOT NULL,
                account_generation INTEGER NOT NULL,
                capability TEXT NOT NULL,
                stream_fingerprint TEXT NOT NULL,
                expected_state_revision INTEGER NOT NULL,
                status TEXT NOT NULL,
                next_attempt_at TEXT NOT NULL,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                claim_id TEXT,
                claim_expires_at TEXT,
                quarantine_code TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(recovery_item_id, action_revision)
            );

            CREATE INDEX IF NOT EXISTS connector_sync_recovery_jobs_due
            ON connector_sync_recovery_jobs
               (status, next_attempt_at, claim_expires_at, attempt_count);

            CREATE TABLE IF NOT EXISTS connector_reconciliation_recovery_actions (
                item_id TEXT PRIMARY KEY NOT NULL,
                action_handle TEXT NOT NULL UNIQUE,
                token_hash TEXT NOT NULL,
                invocation_hash TEXT NOT NULL,
                account_generation INTEGER NOT NULL,
                authority_hash TEXT NOT NULL,
                request_fingerprint_hash TEXT NOT NULL,
                next_reconciliation_at TEXT NOT NULL,
                reconciliation_attempt_count INTEGER NOT NULL,
                invocation_updated_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS connector_recovery_action_receipts (
                action_kind TEXT NOT NULL,
                item_id TEXT NOT NULL,
                action_revision TEXT NOT NULL,
                accepted_at TEXT NOT NULL,
                retain_until TEXT NOT NULL,
                PRIMARY KEY (action_kind, item_id, action_revision)
            );

            "#,
        )?;
        ensure_sqlite_column(
            &self.conn,
            "automation_definitions",
            "revision",
            "ALTER TABLE automation_definitions ADD COLUMN revision INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_sqlite_column(
            &self.conn,
            "automation_runs",
            "definition_revision",
            "ALTER TABLE automation_runs ADD COLUMN definition_revision INTEGER NOT NULL DEFAULT 0",
        )?;
        artifact::migrate(self)?;
        computer_use::migrate(self)?;
        revocation::migrate(self)?;
        read_execution::migrate(self)?;
        ensure_sqlite_column(
            &self.conn,
            "connector_sync_streams",
            "request_json",
            "ALTER TABLE connector_sync_streams ADD COLUMN request_json TEXT",
        )?;
        ensure_sqlite_column(
            &self.conn,
            "connector_sync_streams",
            "last_successful_at",
            "ALTER TABLE connector_sync_streams ADD COLUMN last_successful_at TEXT",
        )?;
        ensure_sqlite_column(
            &self.conn,
            "connector_authorization_sessions",
            "revision",
            "ALTER TABLE connector_authorization_sessions ADD COLUMN revision INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_sqlite_column(
            &self.conn,
            "connector_authorization_sessions",
            "cleanup_required",
            "ALTER TABLE connector_authorization_sessions ADD COLUMN cleanup_required INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_sqlite_column(
            &self.conn,
            "connector_authorization_sessions",
            "cleanup_completed_at",
            "ALTER TABLE connector_authorization_sessions ADD COLUMN cleanup_completed_at TEXT",
        )?;
        ensure_sqlite_column(
            &self.conn,
            "connector_authorization_sessions",
            "account_id",
            "ALTER TABLE connector_authorization_sessions ADD COLUMN account_id TEXT",
        )?;
        for (column, migration) in [
            ("exchange_claim_id", "ALTER TABLE connector_authorization_sessions ADD COLUMN exchange_claim_id TEXT"),
            ("exchange_claim_expires_at", "ALTER TABLE connector_authorization_sessions ADD COLUMN exchange_claim_expires_at TEXT"),
            ("cleanup_claim_id", "ALTER TABLE connector_authorization_sessions ADD COLUMN cleanup_claim_id TEXT"),
            ("cleanup_claim_expires_at", "ALTER TABLE connector_authorization_sessions ADD COLUMN cleanup_claim_expires_at TEXT"),
        ] {
            ensure_sqlite_column(&self.conn, "connector_authorization_sessions", column, migration)?;
        }
        for (column, migration) in [
            (
                "review_id",
                "ALTER TABLE connector_authorization_actions ADD COLUMN review_id TEXT",
            ),
            (
                "authority_handle_json",
                "ALTER TABLE connector_authorization_actions ADD COLUMN authority_handle_json TEXT",
            ),
            (
                "action_status",
                "ALTER TABLE connector_authorization_actions ADD COLUMN action_status TEXT",
            ),
            (
                "activated_at",
                "ALTER TABLE connector_authorization_actions ADD COLUMN activated_at TEXT",
            ),
            (
                "resolved_at",
                "ALTER TABLE connector_authorization_actions ADD COLUMN resolved_at TEXT",
            ),
            (
                "resolved_intent",
                "ALTER TABLE connector_authorization_actions ADD COLUMN resolved_intent TEXT",
            ),
            (
                "authority_cleanup_required",
                "ALTER TABLE connector_authorization_actions ADD COLUMN authority_cleanup_required INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "authority_cleanup_claim_id",
                "ALTER TABLE connector_authorization_actions ADD COLUMN authority_cleanup_claim_id TEXT",
            ),
            (
                "authority_cleanup_claim_expires_at",
                "ALTER TABLE connector_authorization_actions ADD COLUMN authority_cleanup_claim_expires_at TEXT",
            ),
            (
                "authority_cleanup_completed_at",
                "ALTER TABLE connector_authorization_actions ADD COLUMN authority_cleanup_completed_at TEXT",
            ),
        ] {
            ensure_sqlite_column(
                &self.conn,
                "connector_authorization_actions",
                column,
                migration,
            )?;
        }
        self.conn.execute(
            r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_connector_authorization_review
               ON connector_authorization_actions (review_id)
               WHERE review_id IS NOT NULL"#,
            [],
        )?;
        self.conn.execute(
            r#"CREATE UNIQUE INDEX IF NOT EXISTS idx_connector_authorization_account
               ON connector_authorization_sessions (account_id)
               WHERE account_id IS NOT NULL"#,
            [],
        )?;
        self.conn.execute(
            r#"INSERT OR IGNORE INTO connector_account_generations (account_id, generation)
               SELECT id, 0 FROM connector_accounts"#,
            [],
        )?;
        for (table, column, migration) in [
            (
                "connector_authorization_sessions",
                "status",
                "ALTER TABLE connector_authorization_sessions ADD COLUMN status TEXT NOT NULL DEFAULT '\"pending\"'",
            ),
            (
                "review_queue_items",
                "revision",
                "ALTER TABLE review_queue_items ADD COLUMN revision INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "capability_access_state",
                "row_revision",
                "ALTER TABLE capability_access_state ADD COLUMN row_revision INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "capability_access_state",
                "created_at",
                "ALTER TABLE capability_access_state ADD COLUMN created_at TEXT",
            ),
            (
                "tool_invocation_state",
                "row_revision",
                "ALTER TABLE tool_invocation_state ADD COLUMN row_revision INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "tool_invocation_state",
                "created_at",
                "ALTER TABLE tool_invocation_state ADD COLUMN created_at TEXT",
            ),
            (
                "connector_attachment_landings",
                "workspace_root",
                "ALTER TABLE connector_attachment_landings ADD COLUMN workspace_root TEXT",
            ),
            (
                "connector_attachment_landings",
                "size_bytes",
                "ALTER TABLE connector_attachment_landings ADD COLUMN size_bytes INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "connector_attachment_landings",
                "workspace_identity",
                "ALTER TABLE connector_attachment_landings ADD COLUMN workspace_identity TEXT",
            ),
            (
                "connector_attachment_landings",
                "storage_identity",
                "ALTER TABLE connector_attachment_landings ADD COLUMN storage_identity TEXT",
            ),
            (
                "connector_attachment_landings",
                "failure_kind",
                "ALTER TABLE connector_attachment_landings ADD COLUMN failure_kind TEXT",
            ),
            (
                "connector_attachment_landings",
                "attempt_count",
                "ALTER TABLE connector_attachment_landings ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "connector_attachment_landings",
                "created_at",
                "ALTER TABLE connector_attachment_landings ADD COLUMN created_at TEXT",
            ),
            (
                "connector_attachment_landings",
                "expires_at",
                "ALTER TABLE connector_attachment_landings ADD COLUMN expires_at TEXT",
            ),
            (
                "connector_attachment_landings",
                "next_cleanup_at",
                "ALTER TABLE connector_attachment_landings ADD COLUMN next_cleanup_at TEXT",
            ),
            (
                "connector_attachment_landings",
                "cleanup_claim_id",
                "ALTER TABLE connector_attachment_landings ADD COLUMN cleanup_claim_id TEXT",
            ),
            (
                "connector_attachment_landings",
                "cleanup_claim_expires_at",
                "ALTER TABLE connector_attachment_landings ADD COLUMN cleanup_claim_expires_at TEXT",
            ),
            (
                "connector_attachment_landings",
                "recovery_revision",
                "ALTER TABLE connector_attachment_landings ADD COLUMN recovery_revision INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "connector_invocations",
                "account_generation",
                "ALTER TABLE connector_invocations ADD COLUMN account_generation INTEGER",
            ),
            (
                "connector_invocations",
                "reconciliation_claim_id",
                "ALTER TABLE connector_invocations ADD COLUMN reconciliation_claim_id TEXT",
            ),
            (
                "connector_invocations",
                "reconciliation_claim_expires_at",
                "ALTER TABLE connector_invocations ADD COLUMN reconciliation_claim_expires_at TEXT",
            ),
            (
                "connector_invocations",
                "next_reconciliation_at",
                "ALTER TABLE connector_invocations ADD COLUMN next_reconciliation_at TEXT",
            ),
            (
                "connector_invocations",
                "reconciliation_attempt_count",
                "ALTER TABLE connector_invocations ADD COLUMN reconciliation_attempt_count INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "connector_invocations",
                "reconciliation_quarantine_code",
                "ALTER TABLE connector_invocations ADD COLUMN reconciliation_quarantine_code TEXT",
            ),
            (
                "connector_invocations",
                "reconciliation_quarantined_at",
                "ALTER TABLE connector_invocations ADD COLUMN reconciliation_quarantined_at TEXT",
            ),
            (
                "connector_invocations",
                "recovery_revision",
                "ALTER TABLE connector_invocations ADD COLUMN recovery_revision INTEGER NOT NULL DEFAULT 0",
            ),
        ] {
            ensure_sqlite_column(&self.conn, table, column, migration)?;
        }
        ensure_sqlite_column(
            &self.conn,
            "connector_recovery_action_receipts",
            "retain_until",
            "ALTER TABLE connector_recovery_action_receipts ADD COLUMN retain_until TEXT",
        )?;
        self.conn.execute(
            r#"UPDATE connector_recovery_action_receipts
               SET retain_until = strftime('%Y-%m-%dT%H:%M:%fZ', accepted_at, '+90 days')
               WHERE retain_until IS NULL"#,
            [],
        )?;
        self.conn.execute(
            r#"CREATE INDEX IF NOT EXISTS idx_connector_recovery_action_receipts_retention
               ON connector_recovery_action_receipts (retain_until)"#,
            [],
        )?;
        self.conn.execute(
            r#"DELETE FROM connector_recovery_action_receipts
               WHERE rowid IN (
                 SELECT rowid FROM connector_recovery_action_receipts
                 WHERE retain_until <= ?1 ORDER BY retain_until ASC LIMIT 64
               )"#,
            params![Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true)],
        )?;
        migrate_connector_authorization_session_rows(&self.conn)?;
        self.conn.execute(
            r#"CREATE INDEX IF NOT EXISTS idx_connector_attachment_landings_recovery_due
               ON connector_attachment_landings
                  (status, next_cleanup_at, cleanup_claim_expires_at, attempt_count)"#,
            [],
        )?;
        self.conn.execute(
            r#"CREATE INDEX IF NOT EXISTS idx_connector_invocations_reconciliation_due
               ON connector_invocations
                  (status, next_reconciliation_at, reconciliation_claim_expires_at,
                   reconciliation_attempt_count)"#,
            [],
        )?;
        self.conn.execute(
            "UPDATE capability_access_state SET created_at = COALESCE(created_at, updated_at)",
            [],
        )?;
        self.conn.execute(
            "UPDATE tool_invocation_state SET created_at = COALESCE(created_at, updated_at)",
            [],
        )?;
        self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET size_bytes = COALESCE(NULLIF(size_bytes, 0),
                 CAST(json_extract(metadata_json, '$.size_bytes') AS INTEGER), 0)"#,
            [],
        )?;
        self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET created_at = COALESCE(created_at, updated_at),
                   status = CASE
                     WHEN status = 'completed' AND (
                       workspace_root IS NULL OR workspace_identity IS NULL
                       OR storage_identity IS NULL OR receipt_json IS NULL
                     ) THEN 'repair_required'
                     WHEN status IN ('completed', 'failed') THEN status
                     WHEN workspace_root IS NULL OR workspace_identity IS NULL
                       THEN 'repair_required'
                     ELSE status
                   END,
                   failure_kind = CASE
                     WHEN status = 'completed' AND (
                       workspace_root IS NULL OR workspace_identity IS NULL
                       OR storage_identity IS NULL OR receipt_json IS NULL
                     ) THEN 'legacy_completed_unverified'
                     WHEN status NOT IN ('completed', 'failed')
                       AND (workspace_root IS NULL OR workspace_identity IS NULL)
                       THEN 'legacy_workspace_unbound'
                     ELSE failure_kind
                   END"#,
            [],
        )?;
        self.replay_execution_projection_events()?;
        self.fail_legacy_connector_attachment_tools()?;
        Ok(())
    }

    fn replay_execution_projection_events(&self) -> EventStoreResult<()> {
        let last_rowid = self
            .conn
            .query_row(
                r#"SELECT last_event_rowid FROM execution_projection_cursor
                   WHERE projection_name = 'tool_capability_v1'"#,
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0);
        let transaction = self.conn.unchecked_transaction()?;
        let mut statement = transaction.prepare(
            r#"SELECT rowid, id, event_type, payload_json, created_at
               FROM kernel_events
               WHERE rowid > ?1 AND event_type IN (?2, ?3, ?4, ?5)
               ORDER BY rowid ASC"#,
        )?;
        let rows = statement
            .query_map(
                params![
                    last_rowid,
                    CAPABILITY_ACCESS_REQUESTED_EVENT,
                    PERMISSION_RESOLUTION_RECORDED_EVENT,
                    TOOL_INVOCATION_RECORDED_EVENT,
                    CAPABILITY_INVOCATION_RECORDED_EVENT,
                ],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut newest_rowid = last_rowid;
        for (rowid, event_id, event_type, payload_json, created_at) in rows {
            Self::apply_execution_projection(
                &transaction,
                &event_id,
                &event_type,
                &payload_json,
                &created_at,
            )?;
            newest_rowid = rowid;
        }
        transaction.execute(
            r#"INSERT INTO execution_projection_cursor (projection_name, last_event_rowid)
               VALUES ('tool_capability_v1', ?1)
               ON CONFLICT(projection_name) DO UPDATE SET
                 last_event_rowid = excluded.last_event_rowid"#,
            params![newest_rowid],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn apply_execution_projection(
        transaction: &Transaction<'_>,
        event_id: &str,
        event_type: &str,
        payload_json: &str,
        created_at: &str,
    ) -> EventStoreResult<()> {
        if !matches!(
            event_type,
            CAPABILITY_ACCESS_REQUESTED_EVENT
                | PERMISSION_RESOLUTION_RECORDED_EVENT
                | TOOL_INVOCATION_RECORDED_EVENT
                | CAPABILITY_INVOCATION_RECORDED_EVENT
        ) {
            return Ok(());
        }
        let newly_applied = transaction.execute(
            r#"INSERT OR IGNORE INTO execution_projection_applied_events
               (event_id, event_type, applied_at) VALUES (?1, ?2, ?3)"#,
            params![event_id, event_type, created_at],
        )?;
        if newly_applied == 0 {
            return Ok(());
        }
        match event_type {
            CAPABILITY_ACCESS_REQUESTED_EVENT => {
                let request: CapabilityAccessRequest = serde_json::from_str(payload_json)?;
                transaction.execute(
                    r#"INSERT INTO capability_access_state
                       (request_id, request_json, resolution_json, effective_status,
                        row_revision, created_at, updated_at)
                       VALUES (?1, ?2, NULL, ?3, 0, ?4, ?4)
                       ON CONFLICT(request_id) DO UPDATE SET
                         request_json = excluded.request_json,
                         effective_status = CASE
                           WHEN capability_access_state.resolution_json IS NULL
                             THEN excluded.effective_status
                           ELSE capability_access_state.effective_status
                         END,
                         row_revision = capability_access_state.row_revision + 1,
                         updated_at = excluded.updated_at"#,
                    params![
                        request.id.to_string(),
                        payload_json,
                        serde_json::to_string(&request.status)?,
                        created_at,
                    ],
                )?;
            }
            PERMISSION_RESOLUTION_RECORDED_EVENT => {
                let resolution: PermissionResolution = serde_json::from_str(payload_json)?;
                let effective_status = if resolution.approved {
                    CapabilityAccessStatus::Approved
                } else {
                    CapabilityAccessStatus::Rejected
                };
                if let Some(expected) = resolution.expected_request_revision {
                    let current = transaction
                        .query_row(
                            "SELECT row_revision FROM capability_access_state WHERE request_id = ?1",
                            params![resolution.request_id.to_string()],
                            |row| row.get::<_, u64>(0),
                        )
                        .optional()?
                        .ok_or_else(|| {
                            EventStoreError::InvalidState(
                                "permission resolution projection has no request".to_string(),
                            )
                        })?;
                    if current != expected {
                        return Err(EventStoreError::InvalidState(
                            "permission resolution projection revision changed".to_string(),
                        ));
                    }
                }
                let changed = transaction.execute(
                    r#"UPDATE capability_access_state
                       SET resolution_json = ?2, effective_status = ?3,
                           row_revision = row_revision + 1, updated_at = ?4
                       WHERE request_id = ?1"#,
                    params![
                        resolution.request_id.to_string(),
                        payload_json,
                        serde_json::to_string(&effective_status)?,
                        created_at,
                    ],
                )?;
                if changed != 1 {
                    return Err(EventStoreError::InvalidState(
                        "permission resolution projection has no request".to_string(),
                    ));
                }
            }
            TOOL_INVOCATION_RECORDED_EVENT => {
                let invocation: ToolInvocationRecord = serde_json::from_str(payload_json)?;
                transaction.execute(
                    r#"INSERT INTO tool_invocation_state
                       (id, invocation_json, tool_id, capability, status,
                        approval_request_id, request_fingerprint, row_revision,
                        created_at, updated_at)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8)
                       ON CONFLICT(id) DO UPDATE SET
                         invocation_json = excluded.invocation_json,
                         tool_id = excluded.tool_id,
                         capability = excluded.capability,
                         status = excluded.status,
                         approval_request_id = excluded.approval_request_id,
                         request_fingerprint = excluded.request_fingerprint,
                         row_revision = tool_invocation_state.row_revision + 1,
                         updated_at = excluded.updated_at"#,
                    params![
                        invocation.id.to_string(),
                        payload_json,
                        invocation.tool_id,
                        serde_json::to_string(&invocation.capability)?,
                        serde_json::to_string(&invocation.status)?,
                        invocation
                            .approval_request_id
                            .map(|value| value.to_string()),
                        invocation.request_fingerprint,
                        created_at,
                    ],
                )?;
            }
            CAPABILITY_INVOCATION_RECORDED_EVENT => {
                let invocation: CapabilityInvocation = serde_json::from_str(payload_json)?;
                if invocation.status != CapabilityInvocationStatus::PendingApproval {
                    if let Some(request_id) = invocation.approval_request_id {
                        transaction.execute(
                            r#"INSERT OR IGNORE INTO capability_approval_consumptions
                           (request_id, capability_invocation_id, consumed_at)
                           VALUES (?1, ?2, ?3)"#,
                            params![
                                request_id.to_string(),
                                invocation.id.to_string(),
                                created_at
                            ],
                        )?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn insert_kernel_event(
        transaction: &Transaction<'_>,
        event: &KernelEvent,
    ) -> EventStoreResult<()> {
        let created_at = event.created_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
        transaction.execute(
            r#"INSERT INTO kernel_events (id, event_type, payload_json, created_at)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![
                event.id.to_string(),
                event.event_type,
                event.payload_json,
                created_at,
            ],
        )?;
        Self::apply_execution_projection(
            transaction,
            &event.id.to_string(),
            &event.event_type,
            &event.payload_json,
            &created_at,
        )
    }

    fn fail_legacy_connector_attachment_tools(&self) -> EventStoreResult<()> {
        let mut statement = self.conn.prepare(
            r#"SELECT tool_invocation_id FROM connector_attachment_landings
               WHERE status = 'repair_required'
                 AND failure_kind = 'legacy_workspace_unbound'"#,
        )?;
        let tool_ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for tool_id in tool_ids {
            let Ok(tool_id) = Uuid::parse_str(&tool_id) else {
                continue;
            };
            let mut tool = match self.tool_invocation_by_id(tool_id) {
                Ok(tool) => tool,
                Err(EventStoreError::NotFound(_)) => continue,
                Err(error) => return Err(error),
            };
            if !matches!(
                tool.status,
                ToolExecutionStatus::WaitingForConfirmation | ToolExecutionStatus::Running
            ) {
                continue;
            }
            tool.status = ToolExecutionStatus::Failed;
            tool.output = None;
            tool.evidence.clear();
            tool.verification = ToolVerificationResult::failed(
                "legacy attachment landing has no safe workspace binding",
            );
            tool.error =
                Some("connector attachment requires manual workspace inspection".to_string());
            tool.finished_at = Some(Utc::now());
            self.append_tool_invocation(&tool)?;
        }
        Ok(())
    }

    pub fn upsert_automation_definition(
        &self,
        definition: &AutomationDefinition,
    ) -> EventStoreResult<AutomationDefinition> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT definition_json FROM automation_definitions WHERE id = ?1",
                params![definition.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let mut persisted = definition.clone();
        if let Some(existing) = existing {
            let existing: AutomationDefinition = serde_json::from_str(&existing)?;
            if definition.revision != existing.revision {
                return Err(EventStoreError::InvalidState(
                    "automation definition revision changed".to_string(),
                ));
            }
            persisted.revision = existing.revision.checked_add(1).ok_or_else(|| {
                EventStoreError::InvalidState(
                    "automation definition revision is exhausted".to_string(),
                )
            })?;
        } else if definition.revision != 0 {
            return Err(EventStoreError::InvalidState(
                "new automation definition revision is invalid".to_string(),
            ));
        }
        let definition_json = serde_json::to_string(&persisted)?;
        let changed = transaction.execute(
            r#"INSERT INTO automation_definitions (id, definition_json, status, revision, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5)
               ON CONFLICT(id) DO UPDATE SET
                 definition_json = excluded.definition_json,
                 status = excluded.status,
                 revision = excluded.revision,
                 updated_at = excluded.updated_at
               WHERE automation_definitions.revision = excluded.revision - 1"#,
            params![
                definition.id.to_string(),
                definition_json,
                serde_json::to_string(&persisted.status)?,
                i64::try_from(persisted.revision).map_err(|_| EventStoreError::InvalidState(
                    "automation definition revision is too large".to_string()
                ))?,
                persisted
                    .updated_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "automation definition revision changed".to_string(),
            ));
        }
        transaction.commit()?;
        Ok(persisted)
    }

    pub fn set_automation_definition_status(
        &self,
        definition_id: Uuid,
        status: AutomationDefinitionStatus,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<AutomationDefinition> {
        let mut definition = self.automation_definition(definition_id)?;
        definition.status = status;
        definition.updated_at = changed_at;
        self.upsert_automation_definition(&definition)
    }

    pub fn update_automation_goal(
        &self,
        definition_id: Uuid,
        goal: String,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<AutomationDefinition> {
        let mut definition = self.automation_definition(definition_id)?;
        let goal = goal.trim().to_string();
        if goal.is_empty() {
            return Err(EventStoreError::InvalidState(
                "automation goal is required".to_string(),
            ));
        }
        if definition.status == AutomationDefinitionStatus::Deleted {
            return Err(EventStoreError::InvalidState(
                "deleted automation cannot be edited".to_string(),
            ));
        }
        definition.goal = goal;
        definition.updated_at = changed_at;
        self.upsert_automation_definition(&definition)
    }

    pub fn automation_definition(
        &self,
        definition_id: Uuid,
    ) -> EventStoreResult<AutomationDefinition> {
        let json = self.conn.query_row(
            "SELECT definition_json FROM automation_definitions WHERE id = ?1",
            params![definition_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn list_automation_definitions(&self) -> EventStoreResult<Vec<AutomationDefinition>> {
        let mut statement = self.conn.prepare(
            "SELECT definition_json FROM automation_definitions ORDER BY updated_at ASC, rowid ASC",
        )?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .collect()
    }

    pub fn claim_due_automation_run(
        &self,
        definition_id: Uuid,
        now: DateTime<Utc>,
        worker_id: String,
    ) -> EventStoreResult<Option<AutomationRun>> {
        let transaction = self.conn.unchecked_transaction()?;
        let definition_json = transaction.query_row(
            "SELECT definition_json FROM automation_definitions WHERE id = ?1",
            params![definition_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        let definition: AutomationDefinition = serde_json::from_str(&definition_json)?;
        if definition.status != AutomationDefinitionStatus::Enabled {
            transaction.commit()?;
            return Ok(None);
        }
        let scheduled_for = due_automation_window(&transaction, &definition, now)?;
        let Some(scheduled_for) = scheduled_for else {
            transaction.commit()?;
            return Ok(None);
        };
        let key = trigger_window_key(definition.id, scheduled_for);
        let timestamp = now.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let missed = now.signed_duration_since(scheduled_for).num_seconds()
            > i64::try_from(definition.missed_after_seconds).unwrap_or(i64::MAX);
        let skip_missed = missed && definition.missed_run_policy == MissedRunPolicy::Skip;
        let run = AutomationRun {
            id: Uuid::new_v4(),
            definition_id: definition.id,
            definition_revision: definition.revision,
            trigger_window_key: key.clone(),
            scheduled_for,
            status: if skip_missed {
                AutomationRunStatus::Cancelled
            } else {
                AutomationRunStatus::Queued
            },
            attempt: 0,
            agent_run_id: None,
            review_queue_item_id: None,
            last_error: skip_missed.then(|| "missed trigger window skipped by policy".to_string()),
            claimed_by: Some(worker_id.clone()),
            claimed_at: Some(now),
            created_at: now,
            updated_at: now,
        };
        let inserted = transaction.execute(
            r#"INSERT OR IGNORE INTO automation_runs
               (id, definition_id, trigger_window_key, scheduled_for, run_json, status,
                definition_revision, claimed_by, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
            params![
                run.id.to_string(),
                definition.id.to_string(),
                key,
                scheduled_for.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&run)?,
                serde_json::to_string(&run.status)?,
                i64::try_from(run.definition_revision).map_err(|_| {
                    EventStoreError::InvalidState(
                        "automation run definition revision is too large".to_string(),
                    )
                })?,
                worker_id,
                timestamp,
            ],
        )?;
        transaction.commit()?;
        Ok((inserted == 1 && !skip_missed).then_some(run))
    }

    pub fn enqueue_due_automation_agent_run(
        &self,
        definition_id: Uuid,
        now: DateTime<Utc>,
        scheduler_id: String,
        conversation_id: String,
    ) -> EventStoreResult<Option<(AutomationRun, AgentRunStart)>> {
        let transaction = self.conn.unchecked_transaction()?;
        let definition_json = transaction.query_row(
            "SELECT definition_json FROM automation_definitions WHERE id = ?1",
            params![definition_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        let definition: AutomationDefinition = serde_json::from_str(&definition_json)?;
        if definition.status != AutomationDefinitionStatus::Enabled {
            transaction.commit()?;
            return Ok(None);
        }
        let scheduled_for = due_automation_window(&transaction, &definition, now)?;
        let Some(scheduled_for) = scheduled_for else {
            transaction.commit()?;
            return Ok(None);
        };
        let key = trigger_window_key(definition.id, scheduled_for);
        let missed = now.signed_duration_since(scheduled_for).num_seconds()
            > i64::try_from(definition.missed_after_seconds).unwrap_or(i64::MAX);
        let skip_missed = missed && definition.missed_run_policy == MissedRunPolicy::Skip;
        let agent_run = AgentRunStart::queued(conversation_id, definition.goal, 0)
            .map_err(EventStoreError::InvalidState)?;
        let run = AutomationRun {
            id: Uuid::new_v4(),
            definition_id: definition.id,
            definition_revision: definition.revision,
            trigger_window_key: key.clone(),
            scheduled_for,
            status: if skip_missed {
                AutomationRunStatus::Cancelled
            } else {
                AutomationRunStatus::Queued
            },
            attempt: 0,
            agent_run_id: (!skip_missed).then_some(agent_run.id),
            review_queue_item_id: None,
            last_error: skip_missed.then(|| "missed trigger window skipped by policy".to_string()),
            claimed_by: Some(scheduler_id.clone()),
            claimed_at: Some(now),
            created_at: now,
            updated_at: now,
        };
        let timestamp = now.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let inserted = transaction.execute(
            r#"INSERT OR IGNORE INTO automation_runs
               (id, definition_id, trigger_window_key, scheduled_for, run_json, status,
                definition_revision, claimed_by, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
            params![
                run.id.to_string(),
                definition.id.to_string(),
                key,
                scheduled_for.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&run)?,
                serde_json::to_string(&run.status)?,
                i64::try_from(run.definition_revision).map_err(|_| {
                    EventStoreError::InvalidState(
                        "automation run definition revision is too large".to_string(),
                    )
                })?,
                scheduler_id,
                timestamp,
            ],
        )?;
        if inserted == 0 {
            transaction.commit()?;
            return Ok(None);
        }
        if !skip_missed {
            let event = KernelEvent::new(AGENT_RUN_STARTED_EVENT, &agent_run)?;
            transaction.execute(
                r#"INSERT INTO kernel_events (id, event_type, payload_json, created_at)
                   VALUES (?1, ?2, ?3, ?4)"#,
                params![
                    event.id.to_string(),
                    event.event_type,
                    event.payload_json,
                    event.created_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )?;
        }
        transaction.commit()?;
        Ok((!skip_missed).then_some((run, agent_run)))
    }

    pub fn enqueue_manual_automation_agent_run(
        &self,
        definition_id: Uuid,
        manual_invocation_id: Uuid,
        now: DateTime<Utc>,
        conversation_id: String,
    ) -> EventStoreResult<(AutomationRun, AgentRunStart)> {
        let transaction = self.conn.unchecked_transaction()?;
        let definition_json = transaction.query_row(
            "SELECT definition_json FROM automation_definitions WHERE id = ?1",
            params![definition_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        let definition: AutomationDefinition = serde_json::from_str(&definition_json)?;
        if definition.status == AutomationDefinitionStatus::Deleted {
            return Err(EventStoreError::InvalidState(
                "deleted automation cannot run".to_string(),
            ));
        }
        let key = format!("manual:{manual_invocation_id}");
        let agent_run = AgentRunStart::queued(conversation_id, definition.goal, 0)
            .map_err(EventStoreError::InvalidState)?;
        let run = AutomationRun {
            id: Uuid::new_v4(),
            definition_id: definition.id,
            definition_revision: definition.revision,
            trigger_window_key: key.clone(),
            scheduled_for: now,
            status: AutomationRunStatus::Queued,
            attempt: 0,
            agent_run_id: Some(agent_run.id),
            review_queue_item_id: None,
            last_error: None,
            claimed_by: Some("manual".to_string()),
            claimed_at: Some(now),
            created_at: now,
            updated_at: now,
        };
        let inserted = transaction.execute(
            r#"INSERT OR IGNORE INTO automation_runs
               (id, definition_id, trigger_window_key, scheduled_for, run_json, status,
                definition_revision, claimed_by, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
            params![
                run.id.to_string(),
                definition.id.to_string(),
                key,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&run)?,
                serde_json::to_string(&run.status)?,
                i64::try_from(run.definition_revision).map_err(|_| {
                    EventStoreError::InvalidState(
                        "automation run definition revision is too large".to_string(),
                    )
                })?,
                "manual",
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if inserted == 0 {
            return Err(EventStoreError::InvalidState(
                "manual automation invocation already exists".to_string(),
            ));
        }
        let event = KernelEvent::new(AGENT_RUN_STARTED_EVENT, &agent_run)?;
        transaction.execute(
            r#"INSERT INTO kernel_events (id, event_type, payload_json, created_at)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![
                event.id.to_string(),
                event.event_type,
                event.payload_json,
                event.created_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        transaction.commit()?;
        Ok((run, agent_run))
    }

    pub fn list_automation_runs(&self) -> EventStoreResult<Vec<AutomationRun>> {
        let mut statement = self.conn.prepare(
            "SELECT run_json FROM automation_runs ORDER BY scheduled_for ASC, rowid ASC",
        )?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .collect()
    }

    pub fn reconcile_automation_agent_runs(
        &self,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<usize> {
        let agent_runs = self.list_agent_run_records()?;
        let agent_runs = agent_runs
            .into_iter()
            .map(|run| (run.id, run))
            .collect::<std::collections::HashMap<_, _>>();
        let mut reconciled = 0;
        for run in self.list_automation_runs()? {
            let Some(agent_run_id) = run.agent_run_id else {
                continue;
            };
            let Some(agent_run) = agent_runs.get(&agent_run_id) else {
                continue;
            };
            let target = match agent_run.status {
                AgentRunStatus::Queued => AutomationRunStatus::Queued,
                AgentRunStatus::Running => AutomationRunStatus::Running,
                AgentRunStatus::WaitingForPrerequisite => AutomationRunStatus::WaitingReview,
                AgentRunStatus::WaitingForConfirmation => AutomationRunStatus::WaitingApproval,
                AgentRunStatus::Completed => AutomationRunStatus::Completed,
                AgentRunStatus::Failed | AgentRunStatus::Blocked => AutomationRunStatus::Failed,
                AgentRunStatus::Cancelled | AgentRunStatus::CancelRequested => {
                    AutomationRunStatus::Cancelled
                }
            };
            if run.status == target {
                continue;
            }
            self.transition_automation_run(
                run.id,
                target,
                None,
                agent_run.finish_error.clone(),
                changed_at,
            )?;
            reconciled += 1;
        }
        Ok(reconciled)
    }

    pub fn automation_run(&self, run_id: Uuid) -> EventStoreResult<AutomationRun> {
        let json = self.conn.query_row(
            "SELECT run_json FROM automation_runs WHERE id = ?1",
            params![run_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn transition_automation_run(
        &self,
        run_id: Uuid,
        status: AutomationRunStatus,
        agent_run_id: Option<Uuid>,
        last_error: Option<String>,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<AutomationRun> {
        let mut run = self.automation_run(run_id)?;
        if !automation_run_transition_allowed(run.status, status) {
            return Err(EventStoreError::InvalidState(format!(
                "automation run cannot transition from {:?} to {:?}",
                run.status, status
            )));
        }
        if run.status == AutomationRunStatus::Failed && status == AutomationRunStatus::Queued {
            let definition = self.automation_definition(run.definition_id)?;
            if run.attempt >= definition.retry_limit {
                return Err(EventStoreError::InvalidState(
                    "automation retry limit reached".to_string(),
                ));
            }
            run.attempt += 1;
        }
        if let Some(agent_run_id) = agent_run_id {
            run.agent_run_id = Some(agent_run_id);
        }
        run.status = status;
        run.last_error = last_error;
        run.updated_at = changed_at;
        self.persist_automation_run(&run)?;
        Ok(run)
    }

    pub fn link_automation_run_to_agent_run(
        &self,
        automation_run_id: Uuid,
        agent_run_id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<AutomationRun> {
        self.ensure_agent_run_exists(agent_run_id)?;
        let mut run = self.automation_run(automation_run_id)?;
        if let Some(existing_id) = run.agent_run_id {
            if existing_id != agent_run_id {
                return Err(EventStoreError::InvalidState(
                    "automation run is already linked to another agent run".to_string(),
                ));
            }
            return Ok(run);
        }
        run.agent_run_id = Some(agent_run_id);
        run.updated_at = changed_at;
        self.persist_automation_run(&run)?;
        Ok(run)
    }

    pub fn upsert_automation_checkpoint(
        &self,
        checkpoint: &AutomationCheckpoint,
    ) -> EventStoreResult<()> {
        self.conn.execute(
            r#"INSERT INTO automation_checkpoints (automation_run_id, checkpoint_json, updated_at)
               VALUES (?1, ?2, ?3)
               ON CONFLICT(automation_run_id) DO UPDATE SET
                 checkpoint_json = excluded.checkpoint_json,
                 updated_at = excluded.updated_at"#,
            params![
                checkpoint.automation_run_id.to_string(),
                serde_json::to_string(checkpoint)?,
                checkpoint
                    .recorded_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        Ok(())
    }

    pub fn upsert_review_queue_item(&self, item: &ReviewQueueItem) -> EventStoreResult<()> {
        let existing = self
            .conn
            .query_row(
                "SELECT item_json FROM review_queue_items WHERE id = ?1",
                params![item.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|json| serde_json::from_str::<ReviewQueueItem>(&json))
            .transpose()?;
        if existing.as_ref() == Some(item) {
            return Ok(());
        }
        if existing
            .as_ref()
            .is_some_and(|current| item.revision != current.revision.saturating_add(1))
        {
            return Err(EventStoreError::InvalidState(
                "review item revision is stale".to_string(),
            ));
        }
        let transaction = self.conn.unchecked_transaction()?;
        let mut run = self.automation_run(item.automation_run_id)?;
        if let Some(existing_id) = run.review_queue_item_id {
            if existing_id != item.id {
                return Err(EventStoreError::InvalidState(
                    "automation run already has a review queue item".to_string(),
                ));
            }
        }
        run.review_queue_item_id = Some(item.id);
        run.updated_at = item.updated_at;
        let changed = transaction.execute(
            r#"INSERT INTO review_queue_items (id, automation_run_id, item_json, status, revision, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)
               ON CONFLICT(id) DO UPDATE SET
                 item_json = excluded.item_json,
                 status = excluded.status,
                 revision = excluded.revision,
                 updated_at = excluded.updated_at
               WHERE review_queue_items.revision + 1 = excluded.revision"#,
            params![
                item.id.to_string(),
                item.automation_run_id.to_string(),
                serde_json::to_string(item)?,
                serde_json::to_string(&item.status)?,
                item.revision,
                item.updated_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "review item update raced with another writer".to_string(),
            ));
        }
        transaction.execute(
            "UPDATE automation_runs SET run_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![
                run.id.to_string(),
                serde_json::to_string(&run)?,
                run.updated_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_review_queue_items(&self) -> EventStoreResult<Vec<ReviewQueueItem>> {
        let mut statement = self.conn.prepare(
            "SELECT item_json FROM review_queue_items ORDER BY updated_at ASC, rowid ASC",
        )?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .collect()
    }

    pub fn review_queue_item(&self, id: Uuid) -> EventStoreResult<ReviewQueueItem> {
        let json = self.conn.query_row(
            "SELECT item_json FROM review_queue_items WHERE id = ?1",
            params![id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn edit_review_queue_item(
        &self,
        id: Uuid,
        title: String,
        preview_fingerprint: Option<String>,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ReviewQueueItem> {
        let mut item = self.review_queue_item(id)?;
        let previous_revision = item.revision;
        let resolution = self.review_tool_approval_invalidation(&item, "Review preview changed")?;
        item.edit(title, preview_fingerprint, changed_at)
            .map_err(EventStoreError::InvalidState)?;
        self.persist_review_item_transition(&item, previous_revision, resolution)?;
        Ok(item)
    }

    pub fn resolve_review_queue_item(
        &self,
        id: Uuid,
        accepted: bool,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ReviewQueueItem> {
        let mut item = self.review_queue_item(id)?;
        let previous_revision = item.revision;
        let resolution = (!accepted)
            .then(|| self.review_tool_approval_invalidation(&item, "Review item rejected"))
            .transpose()?
            .flatten();
        item.resolve(accepted, changed_at)
            .map_err(EventStoreError::InvalidState)?;
        self.persist_review_item_transition(&item, previous_revision, resolution)?;
        Ok(item)
    }

    fn review_tool_approval_invalidation(
        &self,
        item: &ReviewQueueItem,
        note: &str,
    ) -> EventStoreResult<Option<PermissionResolution>> {
        let Some(tool_invocation_id) = item.tool_invocation_id else {
            return Ok(None);
        };
        let approval_request_id = self
            .list_tool_invocations()?
            .into_iter()
            .find(|record| record.id == tool_invocation_id)
            .and_then(|record| record.approval_request_id);
        Ok(approval_request_id
            .map(|request_id| PermissionResolution::new(request_id, false, note.to_string())))
    }

    fn persist_review_item_transition(
        &self,
        item: &ReviewQueueItem,
        previous_revision: u32,
        resolution: Option<PermissionResolution>,
    ) -> EventStoreResult<()> {
        let mut run = self.automation_run(item.automation_run_id)?;
        if run.review_queue_item_id != Some(item.id) {
            return Err(EventStoreError::InvalidState(
                "automation run is not linked to this review item".to_string(),
            ));
        }
        run.updated_at = item.updated_at;
        let resolution_event = resolution
            .as_ref()
            .map(|value| KernelEvent::new(PERMISSION_RESOLUTION_RECORDED_EVENT, value))
            .transpose()?;
        let transaction = self.conn.unchecked_transaction()?;
        if let Some(event) = resolution_event {
            Self::insert_kernel_event(&transaction, &event)?;
        }
        let changed = transaction.execute(
            r#"UPDATE review_queue_items
               SET item_json = ?2, status = ?3, revision = ?4, updated_at = ?5
               WHERE id = ?1 AND revision = ?6"#,
            params![
                item.id.to_string(),
                serde_json::to_string(item)?,
                serde_json::to_string(&item.status)?,
                item.revision,
                item.updated_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                previous_revision,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "review item transition raced with another writer".to_string(),
            ));
        }
        transaction.execute(
            "UPDATE automation_runs SET run_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![
                run.id.to_string(),
                serde_json::to_string(&run)?,
                run.updated_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn automation_checkpoint(
        &self,
        automation_run_id: Uuid,
    ) -> EventStoreResult<AutomationCheckpoint> {
        let json = self.conn.query_row(
            "SELECT checkpoint_json FROM automation_checkpoints WHERE automation_run_id = ?1",
            params![automation_run_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        Ok(serde_json::from_str(&json)?)
    }

    fn persist_automation_run(&self, run: &AutomationRun) -> EventStoreResult<()> {
        self.conn.execute(
            r#"UPDATE automation_runs
               SET run_json = ?2, status = ?3, definition_revision = ?4,
                   claimed_by = ?5, updated_at = ?6
               WHERE id = ?1"#,
            params![
                run.id.to_string(),
                serde_json::to_string(run)?,
                serde_json::to_string(&run.status)?,
                i64::try_from(run.definition_revision).map_err(|_| {
                    EventStoreError::InvalidState(
                        "automation run definition revision is too large".to_string(),
                    )
                })?,
                run.claimed_by,
                run.updated_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        Ok(())
    }

    pub fn upsert_connector_account(&self, account: &ConnectorAccount) -> EventStoreResult<()> {
        let transaction = self.conn.unchecked_transaction()?;
        let existing = transaction
            .query_row(
                "SELECT account_json FROM connector_accounts WHERE id = ?1",
                params![account.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|json| serde_json::from_str::<ConnectorAccount>(&json))
            .transpose()?;
        if existing.as_ref().is_some_and(|stored| {
            matches!(
                stored.health,
                ConnectorHealth::DisconnectPending | ConnectorHealth::RevocationPending
            )
        }) {
            return Err(EventStoreError::InvalidState(
                "connector pending transition requires its dedicated recovery path".to_string(),
            ));
        }
        transaction.execute(
            r#"INSERT OR IGNORE INTO connector_account_generations (account_id, generation)
               VALUES (?1, 0)"#,
            params![account.id.to_string()],
        )?;
        if existing.as_ref().is_some_and(|stored| {
            stored.provider_id != account.provider_id
                || stored.tenant_ref != account.tenant_ref
                || stored.credential_handle != account.credential_handle
                || stored.granted_capabilities != account.granted_capabilities
        }) {
            transaction.execute(
                "UPDATE connector_account_generations SET generation = generation + 1 WHERE account_id = ?1",
                params![account.id.to_string()],
            )?;
            transaction.execute(
                "DELETE FROM connector_sync_projection WHERE account_id = ?1",
                params![account.id.to_string()],
            )?;
            transaction.execute(
                "DELETE FROM connector_sync_streams WHERE account_id = ?1",
                params![account.id.to_string()],
            )?;
        }
        transaction.execute(
            r#"INSERT INTO connector_accounts (id, provider_id, account_json, health, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5)
               ON CONFLICT(id) DO UPDATE SET
                 provider_id = excluded.provider_id,
                 account_json = excluded.account_json,
                 health = excluded.health,
                 updated_at = excluded.updated_at"#,
            params![
                account.id.to_string(),
                account.provider_id,
                serde_json::to_string(account)?,
                serde_json::to_string(&account.health)?,
                account
                    .updated_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn connector_account_sync_generation(
        &self,
        account: &ConnectorAccount,
    ) -> EventStoreResult<u64> {
        let (account_json, generation) = self.conn.query_row(
            r#"SELECT account.account_json, generation.generation
               FROM connector_accounts AS account
               JOIN connector_account_generations AS generation
                 ON generation.account_id = account.id
               WHERE account.id = ?1 AND account.health = ?2"#,
            params![
                account.id.to_string(),
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let stored: ConnectorAccount = serde_json::from_str(&account_json)?;
        if stored.provider_id != account.provider_id
            || stored.tenant_ref != account.tenant_ref
            || stored.credential_handle != account.credential_handle
            || stored.granted_capabilities != account.granted_capabilities
            || stored.health != ConnectorHealth::Connected
        {
            return Err(EventStoreError::InvalidState(
                "connector account binding changed".to_string(),
            ));
        }
        u64::try_from(generation).map_err(|_| {
            EventStoreError::InvalidState("connector account generation is invalid".to_string())
        })
    }

    pub fn mark_connector_account_needs_repair(
        &self,
        account: &ConnectorAccount,
        expected_generation: u64,
    ) -> EventStoreResult<()> {
        if account.health != ConnectorHealth::NeedsRepair {
            return Err(EventStoreError::InvalidState(
                "connector repair transition requires needs-repair state".to_string(),
            ));
        }
        let changed = self.conn.execute(
            r#"UPDATE connector_accounts
               SET provider_id = ?2, account_json = ?3, health = ?4, updated_at = ?5
               WHERE id = ?1 AND health = ?6 AND EXISTS (
                 SELECT 1 FROM connector_account_generations
                 WHERE account_id = ?1 AND generation = ?7
               )"#,
            params![
                account.id.to_string(),
                account.provider_id,
                serde_json::to_string(account)?,
                serde_json::to_string(&account.health)?,
                account
                    .updated_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorHealth::Connected)?,
                i64::try_from(expected_generation).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector account generation is too large".to_string(),
                    )
                })?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector account changed during repair transition".to_string(),
            ));
        }
        Ok(())
    }

    pub fn begin_connector_disconnect(
        &self,
        account_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorDisconnectTicket> {
        let transaction = self.conn.unchecked_transaction()?;
        let (json, generation) = transaction
            .query_row(
                r#"SELECT account.account_json, generation.generation
                   FROM connector_accounts AS account
                   JOIN connector_account_generations AS generation
                     ON generation.account_id = account.id
                   WHERE account.id = ?1"#,
                params![account_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or_else(|| EventStoreError::NotFound("connector account".to_string()))?;
        let mut account: ConnectorAccount = serde_json::from_str(&json)?;
        let generation = u64::try_from(generation).map_err(|_| {
            EventStoreError::InvalidState("connector account generation is invalid".to_string())
        })?;
        if account.health == ConnectorHealth::DisconnectPending {
            return Ok(ConnectorDisconnectTicket::new(account, generation));
        }
        if account.health == ConnectorHealth::RevocationPending {
            return Err(EventStoreError::InvalidState(
                "connector revocation must be reconciled before local disconnect".to_string(),
            ));
        }
        if account.health == ConnectorHealth::Disconnected {
            return Err(EventStoreError::InvalidState(
                "connector account is already disconnected".to_string(),
            ));
        }
        let previous_health = account.health;
        let next_generation = generation.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState("connector account generation overflowed".to_string())
        })?;
        account.health = ConnectorHealth::DisconnectPending;
        account.updated_at = now;
        let generation_changed = transaction.execute(
            r#"UPDATE connector_account_generations SET generation = ?2
               WHERE account_id = ?1 AND generation = ?3"#,
            params![
                account_id.to_string(),
                i64::try_from(next_generation).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector account generation is too large".to_string(),
                    )
                })?,
                i64::try_from(generation).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector account generation is too large".to_string(),
                    )
                })?,
            ],
        )?;
        if generation_changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector disconnect raced with another account transition".to_string(),
            ));
        }
        transaction.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'cleanup_required', failure_kind = 'account_disconnected', updated_at = ?3
               WHERE account_id = ?1 AND account_generation = ?2
                 AND status IN ('reserved', 'staging', 'ready')"#,
            params![
                account_id.to_string(),
                i64::try_from(generation).map_err(|_| EventStoreError::InvalidState(
                    "connector account generation is too large".to_string()
                ))?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        transaction.execute(
            "DELETE FROM connector_sync_projection WHERE account_id = ?1",
            params![account_id.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM connector_sync_streams WHERE account_id = ?1",
            params![account_id.to_string()],
        )?;
        let changed = transaction.execute(
            r#"UPDATE connector_accounts
               SET account_json = ?2, health = ?3, updated_at = ?4
               WHERE id = ?1 AND health = ?5"#,
            params![
                account_id.to_string(),
                serde_json::to_string(&account)?,
                serde_json::to_string(&account.health)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&previous_health)?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector disconnect could not be started".to_string(),
            ));
        }
        let receipt = ConnectorDisconnectReceipt {
            account_id,
            provider_id: account.provider_id.clone(),
            generation: next_generation,
            phase: ConnectorDisconnectPhase::Started,
            source: ConnectorDisconnectSource::User,
            credential_delete_outcome: None,
            changed_at: now,
        };
        let event = KernelEvent::new("connector.disconnect.started", &receipt)?;
        transaction.execute(
            r#"INSERT INTO kernel_events (id, event_type, payload_json, created_at)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![
                event.id.to_string(),
                event.event_type,
                event.payload_json,
                event.created_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        transaction.commit()?;
        let _ = self.terminalize_connector_attachment_cleanup_tools_for_account_generation(
            account_id, generation, now,
        );
        Ok(ConnectorDisconnectTicket::new(account, next_generation))
    }

    fn terminalize_connector_attachment_cleanup_tools_for_account_generation(
        &self,
        account_id: Uuid,
        generation: u64,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let mut statement = self.conn.prepare(
            r#"SELECT id FROM connector_attachment_landings
               WHERE account_id = ?1 AND account_generation = ?2
                 AND status = 'cleanup_required'"#,
        )?;
        let landing_ids = statement
            .query_map(
                params![
                    account_id.to_string(),
                    i64::try_from(generation).map_err(|_| EventStoreError::InvalidState(
                        "connector account generation is too large".to_string()
                    ))?,
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for landing_id in landing_ids {
            self.terminalize_connector_attachment_cleanup_tool(
                Uuid::parse_str(&landing_id)?,
                changed_at,
            )?;
        }
        Ok(())
    }

    pub fn complete_connector_disconnect(
        &self,
        ticket: &ConnectorDisconnectTicket,
        source: ConnectorDisconnectSource,
        credential_delete_outcome: ConnectorCredentialDeleteOutcome,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAccount> {
        if ticket.account().health != ConnectorHealth::DisconnectPending {
            return Err(EventStoreError::InvalidState(
                "connector disconnect completion requires pending state".to_string(),
            ));
        }
        let transaction = self.conn.unchecked_transaction()?;
        let current_json = transaction
            .query_row(
                r#"SELECT account.account_json
                   FROM connector_accounts AS account
                   JOIN connector_account_generations AS generation
                     ON generation.account_id = account.id
                   WHERE account.id = ?1 AND account.health = ?2 AND generation.generation = ?3"#,
                params![
                    ticket.account().id.to_string(),
                    serde_json::to_string(&ConnectorHealth::DisconnectPending)?,
                    i64::try_from(ticket.generation()).map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector account generation is too large".to_string(),
                        )
                    })?,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector disconnect state changed before completion".to_string(),
                )
            })?;
        let mut current: ConnectorAccount = serde_json::from_str(&current_json)?;
        if current.provider_id != ticket.account().provider_id
            || current.tenant_ref != ticket.account().tenant_ref
            || current.credential_handle != ticket.account().credential_handle
            || current.granted_capabilities != ticket.account().granted_capabilities
        {
            return Err(EventStoreError::InvalidState(
                "connector disconnect account binding changed".to_string(),
            ));
        }
        current.health = ConnectorHealth::Disconnected;
        current.updated_at = now;
        let changed = transaction.execute(
            r#"UPDATE connector_accounts
               SET account_json = ?2, health = ?3, updated_at = ?4
               WHERE id = ?1 AND health = ?5 AND EXISTS (
                 SELECT 1 FROM connector_account_generations
                 WHERE account_id = ?1 AND generation = ?6
               )"#,
            params![
                current.id.to_string(),
                serde_json::to_string(&current)?,
                serde_json::to_string(&current.health)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorHealth::DisconnectPending)?,
                i64::try_from(ticket.generation()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector account generation is too large".to_string(),
                    )
                })?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector disconnect completion raced with another transition".to_string(),
            ));
        }
        let receipt = ConnectorDisconnectReceipt {
            account_id: current.id,
            provider_id: current.provider_id.clone(),
            generation: ticket.generation(),
            phase: ConnectorDisconnectPhase::Completed,
            source,
            credential_delete_outcome: Some(credential_delete_outcome),
            changed_at: now,
        };
        let event = KernelEvent::new("connector.disconnect.completed", &receipt)?;
        transaction.execute(
            r#"INSERT INTO kernel_events (id, event_type, payload_json, created_at)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![
                event.id.to_string(),
                event.event_type,
                event.payload_json,
                event.created_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        transaction.commit()?;
        Ok(current)
    }

    pub fn record_connector_disconnect_failure(
        &self,
        ticket: &ConnectorDisconnectTicket,
        source: ConnectorDisconnectSource,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let transaction = self.conn.unchecked_transaction()?;
        let mut pending = ticket.account().clone();
        pending.updated_at = now;
        let changed = transaction.execute(
            r#"UPDATE connector_accounts SET account_json = ?3, updated_at = ?4
               WHERE id = ?1 AND health = ?2 AND EXISTS (
                 SELECT 1 FROM connector_account_generations
                 WHERE account_id = ?1 AND generation = ?5
               )"#,
            params![
                ticket.account().id.to_string(),
                serde_json::to_string(&ConnectorHealth::DisconnectPending)?,
                serde_json::to_string(&pending)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                i64::try_from(ticket.generation()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector account generation is too large".to_string(),
                    )
                })?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector disconnect failure belongs to a stale transition".to_string(),
            ));
        }
        let receipt = ConnectorDisconnectReceipt {
            account_id: ticket.account().id,
            provider_id: ticket.account().provider_id.clone(),
            generation: ticket.generation(),
            phase: ConnectorDisconnectPhase::CredentialDeleteFailed,
            source,
            credential_delete_outcome: None,
            changed_at: now,
        };
        let event = KernelEvent::new("connector.disconnect.retry_required", &receipt)?;
        transaction.execute(
            r#"INSERT INTO kernel_events (id, event_type, payload_json, created_at)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![
                event.id.to_string(),
                event.event_type,
                event.payload_json,
                event.created_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_pending_connector_disconnects(
        &self,
        limit: usize,
    ) -> EventStoreResult<Vec<ConnectorDisconnectTicket>> {
        if limit == 0 || limit > 100 {
            return Err(EventStoreError::InvalidState(
                "connector disconnect recovery limit is invalid".to_string(),
            ));
        }
        let mut statement = self.conn.prepare(
            r#"SELECT account.account_json, generation.generation
               FROM connector_accounts AS account
               JOIN connector_account_generations AS generation
                 ON generation.account_id = account.id
               WHERE account.health = ?1
               ORDER BY account.updated_at ASC, account.rowid ASC
               LIMIT ?2"#,
        )?;
        let tickets = statement
            .query_map(
                params![
                    serde_json::to_string(&ConnectorHealth::DisconnectPending)?,
                    i64::try_from(limit).map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector disconnect recovery limit is invalid".to_string(),
                        )
                    })?,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )?
            .map(|row| {
                let (json, generation) = row?;
                let account = serde_json::from_str(&json)?;
                let generation = u64::try_from(generation).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector account generation is invalid".to_string(),
                    )
                })?;
                Ok(ConnectorDisconnectTicket::new(account, generation))
            })
            .collect();
        tickets
    }

    pub fn list_connector_accounts(&self) -> EventStoreResult<Vec<ConnectorAccount>> {
        let mut statement = self.conn.prepare(
            "SELECT account_json FROM connector_accounts ORDER BY updated_at ASC, rowid ASC",
        )?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .collect()
    }

    pub fn list_connector_recovery_items(&self) -> EventStoreResult<Vec<ConnectorRecoveryItem>> {
        self.list_connector_recovery_items_with_registries(&EmptyConnectorReconcilerRegistry, None)
    }

    pub(crate) fn list_connector_recovery_items_with_registry(
        &self,
        registry: &dyn ConnectorReconcilerRegistry,
    ) -> EventStoreResult<Vec<ConnectorRecoveryItem>> {
        self.list_connector_recovery_items_with_registries(registry, None)
    }

    pub(crate) fn list_connector_recovery_items_with_runtime_registries(
        &self,
        registry: &dyn ConnectorReconcilerRegistry,
        sync_registry: &dyn ConnectorSyncRegistry,
    ) -> EventStoreResult<Vec<ConnectorRecoveryItem>> {
        self.list_connector_recovery_items_with_registries(registry, Some(sync_registry))
    }

    fn list_connector_recovery_items_with_registries(
        &self,
        registry: &dyn ConnectorReconcilerRegistry,
        sync_registry: Option<&dyn ConnectorSyncRegistry>,
    ) -> EventStoreResult<Vec<ConnectorRecoveryItem>> {
        let mut items = Vec::new();

        let mut attachment_statement = self.conn.prepare(
            r#"SELECT id, metadata_json, failure_kind, workspace_root,
                      workspace_identity, storage_identity, updated_at, recovery_revision
               FROM connector_attachment_landings
               WHERE status = 'repair_required'
               ORDER BY updated_at DESC, id ASC LIMIT 100"#,
        )?;
        let attachment_rows = attachment_statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(attachment_statement);
        for (
            id,
            metadata_json,
            failure_kind,
            workspace_root,
            workspace_identity,
            storage_identity,
            updated_at,
            recovery_revision,
        ) in attachment_rows
        {
            let metadata: ConnectorAttachmentMetadata = serde_json::from_str(&metadata_json)?;
            let reason_code = match failure_kind.as_deref() {
                Some("legacy_workspace_unbound") => {
                    ConnectorRecoveryReasonCode::AttachmentLegacyWorkspaceUnbound
                }
                Some("legacy_completed_unverified") => {
                    ConnectorRecoveryReasonCode::AttachmentLegacyReceiptIncomplete
                }
                Some("retention_identity_conflict") => {
                    ConnectorRecoveryReasonCode::AttachmentRetentionIdentityChanged
                }
                Some("ready_identity_conflict")
                | Some("ready_cleanup_identity_conflict")
                | Some("unsafe_cleanup_boundary") => {
                    ConnectorRecoveryReasonCode::AttachmentStoredIdentityChanged
                }
                Some("recovery_projection_unavailable") => {
                    ConnectorRecoveryReasonCode::AttachmentExecutionRecordIncomplete
                }
                _ => ConnectorRecoveryReasonCode::AttachmentRecoveryRequired,
            };
            let retry_binding = (failure_kind.as_deref()
                != Some("recovery_projection_unavailable"))
            .then(|| {
                workspace_root
                    .as_deref()
                    .zip(workspace_identity.as_deref())
                    .zip(
                        storage_identity
                            .as_deref()
                            .filter(|identity| !identity.trim().is_empty()),
                    )
            })
            .flatten();
            let action = retry_binding.map(
                |((_workspace_root, workspace_identity), storage_identity)| {
                    ConnectorRecoveryAction::RetryAttachmentCleanup {
                        action_revision: connector_attachment_recovery_fingerprint(
                            &id,
                            failure_kind.as_deref(),
                            workspace_identity,
                            storage_identity,
                            &updated_at,
                            recovery_revision,
                        ),
                    }
                },
            );
            items.push(ConnectorRecoveryItem {
                id: Uuid::parse_str(&id)?,
                kind: ConnectorRecoveryKind::Attachment,
                status: ConnectorRecoveryStatus::RepairRequired,
                title: metadata.file_name,
                reason_code,
                external_effect_state: ConnectorRecoveryExternalEffectState::LocalFilePreserved,
                next_step_code: if action.is_some() {
                    ConnectorRecoveryNextStepCode::RetryLocalCleanup
                } else {
                    ConnectorRecoveryNextStepCode::InspectFileManually
                },
                sync_capability: None,
                action,
                updated_at: DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc),
            });
        }

        let accounts = self.list_connector_accounts()?;
        let accounts_by_id = accounts
            .iter()
            .map(|account| (account.id, account.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        for account in accounts {
            if !matches!(
                account.health,
                ConnectorHealth::NeedsRepair
                    | ConnectorHealth::DisconnectPending
                    | ConnectorHealth::RevocationPending
            ) {
                continue;
            }
            let (status, reason_code, external_effect_state, next_step_code) = match account.health
            {
                ConnectorHealth::NeedsRepair => (
                    ConnectorRecoveryStatus::NeedsRepair,
                    ConnectorRecoveryReasonCode::AccountNeedsRepair,
                    ConnectorRecoveryExternalEffectState::NoExternalWrite,
                    ConnectorRecoveryNextStepCode::ReviewAccountConnection,
                ),
                ConnectorHealth::DisconnectPending => (
                    ConnectorRecoveryStatus::DisconnectPending,
                    ConnectorRecoveryReasonCode::AccountDisconnectPending,
                    ConnectorRecoveryExternalEffectState::LocalCredentialRemovalPending,
                    ConnectorRecoveryNextStepCode::WaitForLocalDisconnectRecovery,
                ),
                ConnectorHealth::RevocationPending => {
                    let phase = self.active_connector_revocation_phase(account.id)?;
                    let (external_effect_state, next_step_code) = match phase {
                        ConnectorRevocationPhase::PendingRemote
                        | ConnectorRevocationPhase::RetryScheduled => (
                            ConnectorRecoveryExternalEffectState::NoExternalWrite,
                            ConnectorRecoveryNextStepCode::ReviewAccountConnection,
                        ),
                        ConnectorRevocationPhase::RemoteCallStarted
                        | ConnectorRevocationPhase::ReconciliationRequired => (
                            ConnectorRecoveryExternalEffectState::ExternalResultUncertain,
                            ConnectorRecoveryNextStepCode::VerifyProviderState,
                        ),
                        ConnectorRevocationPhase::RemoteConfirmed => (
                            ConnectorRecoveryExternalEffectState::LocalCredentialRemovalPending,
                            ConnectorRecoveryNextStepCode::WaitForLocalDisconnectRecovery,
                        ),
                        ConnectorRevocationPhase::Completed => {
                            continue;
                        }
                    };
                    (
                        ConnectorRecoveryStatus::RevocationPending,
                        ConnectorRecoveryReasonCode::AccountRevocationPending,
                        external_effect_state,
                        next_step_code,
                    )
                }
                _ => unreachable!(),
            };
            items.push(ConnectorRecoveryItem {
                id: account.id,
                kind: ConnectorRecoveryKind::Account,
                status,
                title: account.display_name,
                reason_code,
                external_effect_state,
                next_step_code,
                sync_capability: None,
                action: None,
                updated_at: account.updated_at,
            });
        }

        let mut sync_statement = self.conn.prepare(
            r#"SELECT account_id, capability, stream_fingerprint, state_json, updated_at
               FROM connector_sync_streams
               ORDER BY updated_at DESC, account_id ASC, capability ASC
               LIMIT 100"#,
        )?;
        let sync_rows = sync_statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(sync_statement);
        for (account_id, capability, stream_fingerprint, state_json, updated_at) in sync_rows {
            let item = (|| -> EventStoreResult<Option<ConnectorRecoveryItem>> {
                let state = ConnectorSyncState::from_persistence_json(state_json.clone())
                    .map_err(EventStoreError::InvalidState)?;
                let parsed_account_id = Uuid::parse_str(&account_id)?;
                if state.account_id() != parsed_account_id
                    || state.capability().contract_name() != capability
                    || state.stream_fingerprint() != stream_fingerprint
                    || state.updated_at()
                        != DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc)
                {
                    return Err(EventStoreError::InvalidState(
                        "connector sync recovery identity is invalid".to_string(),
                    ));
                }
                if !state.stopped() {
                    return Ok(None);
                }
                let sync_capability = match state.capability() {
                    ConnectorCapability::MailSyncInbox => ConnectorRecoverySyncCapability::Mail,
                    ConnectorCapability::CalendarSyncEvents => {
                        ConnectorRecoverySyncCapability::Calendar
                    }
                    _ => {
                        return Err(EventStoreError::InvalidState(
                            "connector sync recovery capability is invalid".to_string(),
                        ));
                    }
                };
                let item_id =
                    connector_sync_recovery_item_id(&account_id, &capability, &stream_fingerprint);
                let account = accounts_by_id.get(&parsed_account_id);
                let action = match account {
                    Some(account)
                        if account.health == ConnectorHealth::Connected
                            && account.granted_capabilities.contains(&state.capability())
                            && sync_registry.is_none_or(|registry| {
                                registry.execution_enabled()
                                    && match state.capability() {
                                        ConnectorCapability::MailSyncInbox => {
                                            registry.mail_provider(&account.provider_id).is_some()
                                        }
                                        ConnectorCapability::CalendarSyncEvents => registry
                                            .calendar_provider(&account.provider_id)
                                            .is_some(),
                                        _ => false,
                                    }
                            }) =>
                    {
                        Some(ConnectorRecoveryAction::ResumeSync {
                            action_revision: connector_sync_recovery_action_revision(
                                &state_json,
                                account,
                                state.account_generation(),
                                state.capability(),
                            ),
                        })
                    }
                    _ => None,
                };
                Ok(Some(ConnectorRecoveryItem {
                    id: item_id,
                    kind: ConnectorRecoveryKind::Sync,
                    status: ConnectorRecoveryStatus::SyncExhausted,
                    title: accounts_by_id
                        .get(&parsed_account_id)
                        .map(|account| account.display_name.clone())
                        .unwrap_or_else(|| "Unavailable account".to_string()),
                    reason_code: ConnectorRecoveryReasonCode::SyncRetryExhausted,
                    external_effect_state: ConnectorRecoveryExternalEffectState::NoExternalWrite,
                    next_step_code: ConnectorRecoveryNextStepCode::ReviewAccountConnection,
                    sync_capability: Some(sync_capability),
                    action,
                    updated_at: state.updated_at(),
                }))
            })();
            if let Ok(Some(item)) = item {
                items.push(item);
            }
        }

        let mut reconciliation_statement = self.conn.prepare(
            r#"SELECT id, updated_at
               FROM connector_invocations
               WHERE status = ?1
               ORDER BY updated_at DESC, id ASC
               LIMIT 100"#,
        )?;
        let reconciliation_rows = reconciliation_statement
            .query_map(
                params![serde_json::to_string(
                    &ConnectorInvocationStatus::ReconciliationRequired
                )?],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        drop(reconciliation_statement);
        let reconciliation_transaction = self.conn.unchecked_transaction()?;
        for (id, updated_at) in reconciliation_rows {
            let Ok(item_id) = Uuid::parse_str(&id) else {
                continue;
            };
            let action = load_connector_reconciliation_recovery_snapshot(
                &reconciliation_transaction,
                item_id,
            )
            .ok()
            .filter(|snapshot| {
                registry.supports(
                    &snapshot.invocation.provider_id,
                    snapshot.invocation.capability,
                ) && DateTime::parse_from_rfc3339(&snapshot.next_reconciliation_at)
                    .map(|scheduled| scheduled.with_timezone(&Utc) > Utc::now())
                    .unwrap_or(false)
            })
            .and_then(|snapshot| {
                connector_reconciliation_recovery_action_revision(&snapshot)
                    .ok()
                    .map(
                        |action_revision| ConnectorRecoveryAction::InspectExternalResult {
                            action_revision,
                        },
                    )
            });
            items.push(ConnectorRecoveryItem {
                id: item_id,
                kind: ConnectorRecoveryKind::Reconciliation,
                status: ConnectorRecoveryStatus::ReconciliationRequired,
                title: "External action".to_string(),
                reason_code: ConnectorRecoveryReasonCode::ReconciliationRequired,
                external_effect_state:
                    ConnectorRecoveryExternalEffectState::ExternalResultUncertain,
                next_step_code: ConnectorRecoveryNextStepCode::VerifyProviderState,
                sync_capability: None,
                action,
                updated_at: DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc),
            });
        }
        reconciliation_transaction.commit()?;

        items.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(items)
    }

    pub fn retry_connector_attachment_recovery(
        &self,
        landing_id: Uuid,
        action_revision: &str,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorRecoveryAcceptance> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        const ACTION_KIND: &str = "attachment_cleanup";
        if connector_recovery_action_was_accepted(
            &transaction,
            ACTION_KIND,
            landing_id,
            action_revision,
        )? {
            return Ok(ConnectorRecoveryAcceptance::AlreadyAccepted);
        }
        let (failure_kind, workspace_identity, storage_identity, updated_at, recovery_revision) =
            transaction
                .query_row(
                    r#"SELECT failure_kind, workspace_identity, storage_identity, updated_at,
                          recovery_revision
                   FROM connector_attachment_landings
                   WHERE id = ?1 AND status = 'repair_required'
                     AND storage_identity IS NOT NULL AND trim(storage_identity) <> ''
                     AND workspace_root IS NOT NULL AND workspace_identity IS NOT NULL"#,
                    params![landing_id.to_string()],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| {
                    EventStoreError::InvalidState(
                        "connector recovery item is not safely retryable".to_string(),
                    )
                })?;
        let current_fingerprint = connector_attachment_recovery_fingerprint(
            &landing_id.to_string(),
            failure_kind.as_deref(),
            &workspace_identity,
            &storage_identity,
            &updated_at,
            recovery_revision,
        );
        if action_revision.trim().is_empty()
            || !constant_time_text_eq(&current_fingerprint, action_revision)
        {
            return Err(EventStoreError::InvalidState(
                "connector recovery card is stale".to_string(),
            ));
        }
        let changed_at_text = changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let changed = transaction.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'cleanup_required', failure_kind = 'manual_retry',
                   attempt_count = 0, next_cleanup_at = ?2,
                   cleanup_claim_id = NULL, cleanup_claim_expires_at = NULL,
                   updated_at = ?2
               WHERE id = ?1 AND status = 'repair_required'
                 AND storage_identity IS NOT NULL AND trim(storage_identity) <> ''
                 AND workspace_root IS NOT NULL AND workspace_identity IS NOT NULL
                 AND updated_at = ?3 AND recovery_revision = ?4"#,
            params![
                landing_id.to_string(),
                changed_at_text,
                updated_at,
                recovery_revision
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector recovery item is not safely retryable".to_string(),
            ));
        }
        let event = KernelEvent::new(
            CONNECTOR_RECOVERY_RETRY_QUEUED_EVENT,
            &serde_json::json!({
                "landing_id": landing_id,
                "kind": "attachment_cleanup",
                "changed_at": changed_at,
            }),
        )?;
        Self::insert_kernel_event(&transaction, &event)?;
        record_connector_recovery_action_acceptance(
            &transaction,
            ACTION_KIND,
            landing_id,
            action_revision,
            changed_at,
        )?;
        transaction.commit()?;
        Ok(ConnectorRecoveryAcceptance::Accepted)
    }

    pub fn resume_connector_read_sync_from_recovery(
        &self,
        item_id: Uuid,
        action_revision: &str,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorRecoveryAcceptance> {
        self.resume_connector_read_sync_from_recovery_with_registry(
            item_id,
            action_revision,
            None,
            changed_at,
        )
    }

    pub(crate) fn resume_connector_read_sync_from_recovery_with_sync_registry(
        &self,
        item_id: Uuid,
        action_revision: &str,
        registry: &dyn ConnectorSyncRegistry,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorRecoveryAcceptance> {
        self.resume_connector_read_sync_from_recovery_with_registry(
            item_id,
            action_revision,
            Some(registry),
            changed_at,
        )
    }

    fn resume_connector_read_sync_from_recovery_with_registry(
        &self,
        item_id: Uuid,
        action_revision: &str,
        sync_registry: Option<&dyn ConnectorSyncRegistry>,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorRecoveryAcceptance> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        const ACTION_KIND: &str = "read_sync";
        if connector_recovery_action_was_accepted(
            &transaction,
            ACTION_KIND,
            item_id,
            action_revision,
        )? {
            return Ok(ConnectorRecoveryAcceptance::AlreadyAccepted);
        }
        let rows = {
            let mut statement = transaction.prepare(
                r#"SELECT account_id, capability, stream_fingerprint, state_json,
                          revision, updated_at, request_json
                   FROM connector_sync_streams"#,
            )?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let (
            account_id_text,
            capability_text,
            stream_fingerprint,
            state_json,
            revision,
            updated_at,
            request_json,
        ) = rows
            .into_iter()
            .find(|(account_id, capability, stream_fingerprint, _, _, _, _)| {
                connector_sync_recovery_item_id(account_id, capability, stream_fingerprint)
                    == item_id
            })
            .ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector sync recovery action is invalid".to_string(),
                )
            })?;
        if action_revision.trim().is_empty() {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery action is invalid".to_string(),
            ));
        }
        let capability = match capability_text.as_str() {
            "mail_sync_inbox" => ConnectorCapability::MailSyncInbox,
            "calendar_sync_events" => ConnectorCapability::CalendarSyncEvents,
            _ => {
                return Err(EventStoreError::InvalidState(
                    "connector sync recovery action is invalid".to_string(),
                ))
            }
        };
        let plan = request_json
            .as_deref()
            .ok_or_else(|| {
                EventStoreError::InvalidState("connector sync recovery plan is missing".to_string())
            })
            .and_then(|value| {
                ConnectorSyncPlan::from_persistence_json(value)
                    .map_err(EventStoreError::InvalidState)
            })?;
        if !matches!(
            (&plan, capability),
            (
                ConnectorSyncPlan::MailInbox { .. },
                ConnectorCapability::MailSyncInbox
            ) | (
                ConnectorSyncPlan::CalendarRange { .. },
                ConnectorCapability::CalendarSyncEvents
            )
        ) {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery plan capability is invalid".to_string(),
            ));
        }
        let state = ConnectorSyncState::from_persistence_json(state_json.clone())
            .map_err(EventStoreError::InvalidState)?;
        let account_id = Uuid::parse_str(&account_id_text)?;
        if state.account_id() != account_id
            || state.capability() != capability
            || state.stream_fingerprint() != stream_fingerprint
            || i64::try_from(state.revision()).ok() != Some(revision)
            || state.updated_at() != DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc)
            || !state.stopped()
        {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery action is stale".to_string(),
            ));
        }
        let (account_json, current_generation) = transaction.query_row(
            r#"SELECT account.account_json, generation.generation
               FROM connector_accounts AS account
               JOIN connector_account_generations AS generation ON generation.account_id = account.id
               WHERE account.id = ?1"#,
            params![account_id_text],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let account: ConnectorAccount = serde_json::from_str(&account_json)?;
        let current_generation = u64::try_from(current_generation).map_err(|_| {
            EventStoreError::InvalidState("connector sync recovery action is invalid".to_string())
        })?;
        if account.id != account_id
            || account.health != ConnectorHealth::Connected
            || current_generation != state.account_generation()
            || !account.granted_capabilities.contains(&capability)
            || !constant_time_text_eq(
                action_revision,
                &connector_sync_recovery_action_revision(
                    &state_json,
                    &account,
                    current_generation,
                    capability,
                ),
            )
        {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery authority changed".to_string(),
            ));
        }
        if !sync_registry.is_none_or(|registry| {
            registry.execution_enabled()
                && match capability {
                    ConnectorCapability::MailSyncInbox => {
                        registry.mail_provider(&account.provider_id).is_some()
                    }
                    ConnectorCapability::CalendarSyncEvents => {
                        registry.calendar_provider(&account.provider_id).is_some()
                    }
                    _ => false,
                }
        }) {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery is unavailable".to_string(),
            ));
        }
        let next = state
            .resume_after_user_recovery(changed_at)
            .map_err(EventStoreError::InvalidState)?;
        let next_json = next
            .persistence_json()
            .map_err(EventStoreError::InvalidState)?;
        let changed = transaction.execute(
            r#"UPDATE connector_sync_streams
               SET state_json = ?4, revision = ?5, updated_at = ?6
               WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                 AND revision = ?7 AND state_json = ?8"#,
            params![
                account_id_text,
                capability_text,
                stream_fingerprint,
                next_json,
                i64::try_from(next.revision()).map_err(|_| EventStoreError::InvalidState(
                    "connector sync revision is too large".to_string()
                ))?,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                revision,
                state_json,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery action raced".to_string(),
            ));
        }
        let job_id = Uuid::new_v4();
        let changed_at_text = changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
        transaction.execute(
            r#"INSERT INTO connector_sync_recovery_jobs
               (id, recovery_item_id, action_revision, account_id, account_generation,
                capability, stream_fingerprint, expected_state_revision, status,
                next_attempt_at, attempt_count, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', ?9, 0, ?9, ?9)"#,
            params![
                job_id.to_string(),
                item_id.to_string(),
                action_revision,
                account_id.to_string(),
                i64::try_from(current_generation).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync account generation is too large".to_string(),
                    )
                })?,
                capability.contract_name(),
                stream_fingerprint,
                i64::try_from(next.revision()).map_err(|_| EventStoreError::InvalidState(
                    "connector sync revision is too large".to_string(),
                ))?,
                changed_at_text,
            ],
        )?;
        let event = KernelEvent::new(
            "connector.sync_recovery.resumed",
            &serde_json::json!({
                "recovery_item_id": item_id,
                "kind": "read_sync",
                "capability": match capability {
                    ConnectorCapability::MailSyncInbox => "mail",
                    ConnectorCapability::CalendarSyncEvents => "calendar",
                    _ => unreachable!(),
                },
                "changed_at": changed_at,
            }),
        )?;
        Self::insert_kernel_event(&transaction, &event)?;
        record_connector_recovery_action_acceptance(
            &transaction,
            ACTION_KIND,
            item_id,
            action_revision,
            changed_at,
        )?;
        transaction.commit()?;
        Ok(ConnectorRecoveryAcceptance::Accepted)
    }

    pub(crate) fn claim_due_connector_sync_recovery_jobs(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<ConnectorSyncRecoveryClaim>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let now_text = now.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let mut claims = Vec::new();
        while claims.len() < limit {
            let rows = {
                let mut statement = transaction.prepare(
                    r#"SELECT id, recovery_item_id, action_revision, account_id,
                          account_generation, capability, stream_fingerprint,
                          expected_state_revision, attempt_count
                   FROM connector_sync_recovery_jobs
                   WHERE (status IN ('queued', 'backoff') AND next_attempt_at <= ?1
                          AND (claim_expires_at IS NULL OR claim_expires_at <= ?1))
                      OR (status = 'running' AND claim_expires_at <= ?1)
                   ORDER BY next_attempt_at ASC, created_at ASC, id ASC
                   LIMIT 64"#,
                )?;
                let rows = statement
                    .query_map(params![now_text], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, String>(6)?,
                            row.get::<_, i64>(7)?,
                            row.get::<_, i64>(8)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            let row_count = rows.len();
            for row in rows {
                if claims.len() >= limit {
                    break;
                }
                let (
                    job_id,
                    item_id,
                    action_revision,
                    account_id,
                    generation,
                    capability,
                    stream,
                    expected_revision,
                    attempt_count,
                ) = row;
                let validated = (|| -> EventStoreResult<(Uuid, ConnectorAccount, ConnectorSyncState, ConnectorSyncPlan, u32)> {
                let job_uuid = Uuid::parse_str(&job_id)?;
                let item_uuid = Uuid::parse_str(&item_id)?;
                let account_uuid = Uuid::parse_str(&account_id)?;
                let generation = u64::try_from(generation).map_err(|_| EventStoreError::InvalidState("connector sync recovery generation is invalid".to_string()))?;
                let expected_revision = u64::try_from(expected_revision).map_err(|_| EventStoreError::InvalidState("connector sync recovery revision is invalid".to_string()))?;
                let attempt_count = u32::try_from(attempt_count).map_err(|_| EventStoreError::InvalidState("connector sync recovery attempt is invalid".to_string()))?;
                let capability = match capability.as_str() {
                    "mail_sync_inbox" => ConnectorCapability::MailSyncInbox,
                    "calendar_sync_events" => ConnectorCapability::CalendarSyncEvents,
                    _ => return Err(EventStoreError::InvalidState("connector sync recovery capability is invalid".to_string())),
                };
                if connector_sync_recovery_item_id(&account_id, capability.contract_name(), &stream) != item_uuid {
                    return Err(EventStoreError::InvalidState("connector sync recovery binding is invalid".to_string()));
                }
                let accepted: i64 = transaction.query_row(
                    r#"SELECT count(*) FROM connector_recovery_action_receipts
                       WHERE action_kind = 'read_sync' AND item_id = ?1 AND action_revision = ?2"#,
                    params![item_id, action_revision],
                    |row| row.get(0),
                )?;
                if accepted != 1 {
                    return Err(EventStoreError::InvalidState("connector sync recovery acceptance is invalid".to_string()));
                }
                let (account_json, current_generation) = transaction.query_row(
                    r#"SELECT account.account_json, generation.generation
                       FROM connector_accounts AS account
                       JOIN connector_account_generations AS generation ON generation.account_id = account.id
                       WHERE account.id = ?1"#,
                    params![account_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )?;
                let account: ConnectorAccount = serde_json::from_str(&account_json)?;
                if account.id != account_uuid
                    || account.health != ConnectorHealth::Connected
                    || u64::try_from(current_generation).ok() != Some(generation)
                    || !account.granted_capabilities.contains(&capability)
                {
                    return Err(EventStoreError::InvalidState("connector sync recovery account authority changed".to_string()));
                }
                let (state_json, revision, request_json) = transaction.query_row(
                    r#"SELECT state_json, revision, request_json FROM connector_sync_streams
                       WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3"#,
                    params![account_id, capability.contract_name(), stream],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<String>>(2)?)),
                )?;
                let state = ConnectorSyncState::from_persistence_json(state_json).map_err(EventStoreError::InvalidState)?;
                if state.account_id() != account_uuid
                    || state.account_generation() != generation
                    || state.capability() != capability
                    || state.stream_fingerprint() != stream
                    || state.revision() != expected_revision
                    || u64::try_from(revision).ok() != Some(expected_revision)
                    || state.stopped()
                {
                    return Err(EventStoreError::InvalidState("connector sync recovery state changed".to_string()));
                }
                let plan = ConnectorSyncPlan::from_persistence_json(request_json.as_deref().ok_or_else(|| EventStoreError::InvalidState("connector sync recovery plan is missing".to_string()))?).map_err(EventStoreError::InvalidState)?;
                if !matches!((&plan, capability), (ConnectorSyncPlan::MailInbox { .. }, ConnectorCapability::MailSyncInbox) | (ConnectorSyncPlan::CalendarRange { .. }, ConnectorCapability::CalendarSyncEvents)) {
                    return Err(EventStoreError::InvalidState("connector sync recovery plan capability is invalid".to_string()));
                }
                Ok((job_uuid, account, state, plan, attempt_count))
            })();
                let Ok((job_uuid, account, state, plan, attempt_count)) = validated else {
                    transaction.execute(
                        r#"UPDATE connector_sync_recovery_jobs
                       SET status = 'repair_required', quarantine_code = 'invalid_binding',
                           claim_id = NULL, claim_expires_at = NULL, updated_at = ?2
                       WHERE id = ?1
                         AND (status IN ('queued', 'backoff')
                              OR (status = 'running' AND claim_expires_at <= ?2))"#,
                        params![job_id, now_text],
                    )?;
                    continue;
                };
                let claim_id = Uuid::new_v4();
                let claim_expires_at =
                    now + Duration::seconds(CONNECTOR_SYNC_RECOVERY_LEASE_SECONDS);
                let changed = transaction.execute(
                    r#"UPDATE connector_sync_recovery_jobs
                   SET status = 'running', claim_id = ?2, claim_expires_at = ?3,
                       attempt_count = attempt_count + 1, updated_at = ?4
                   WHERE id = ?1
                     AND ((status IN ('queued', 'backoff') AND next_attempt_at <= ?4
                           AND (claim_expires_at IS NULL OR claim_expires_at <= ?4))
                          OR (status = 'running' AND claim_expires_at <= ?4))"#,
                    params![
                        job_id,
                        claim_id.to_string(),
                        claim_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                        now_text
                    ],
                )?;
                if changed == 1 {
                    claims.push(ConnectorSyncRecoveryClaim {
                        job_id: job_uuid,
                        claim_id,
                        claim_expires_at,
                        account,
                        state,
                        plan,
                        attempt_count: attempt_count.saturating_add(1),
                    });
                }
            }
            if row_count < 64 {
                break;
            }
        }
        transaction.commit()?;
        Ok(claims)
    }

    pub(crate) fn reset_expired_connector_sync_recovery_claims(
        &self,
        now: DateTime<Utc>,
    ) -> EventStoreResult<usize> {
        let now_text = now.to_rfc3339_opts(SecondsFormat::Nanos, true);
        Ok(self.conn.execute(
            r#"UPDATE connector_sync_recovery_jobs
               SET status = 'queued', next_attempt_at = ?1, claim_id = NULL,
                   claim_expires_at = NULL, updated_at = ?1
               WHERE rowid IN (
                   SELECT rowid FROM connector_sync_recovery_jobs
                   WHERE status = 'running' AND claim_expires_at <= ?1
                   ORDER BY claim_expires_at ASC, rowid ASC LIMIT 64
               )"#,
            params![now_text],
        )?)
    }

    pub(crate) fn renew_connector_sync_recovery_claim(
        &self,
        claim: &mut ConnectorSyncRecoveryClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if claim.claim_expires_at <= now {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery claim expired".to_string(),
            ));
        }
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let (account_json, generation) = transaction.query_row(
            r#"SELECT account.account_json, generation.generation
               FROM connector_accounts AS account
               JOIN connector_account_generations AS generation ON generation.account_id = account.id
               WHERE account.id = ?1"#,
            params![claim.account.id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        let account: ConnectorAccount = serde_json::from_str(&account_json)?;
        let (state_json, request_json) = transaction.query_row(
            r#"SELECT state_json, request_json FROM connector_sync_streams
               WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                 AND revision = ?4"#,
            params![
                claim.state.account_id().to_string(),
                claim.state.capability().contract_name(),
                claim.state.stream_fingerprint(),
                i64::try_from(claim.state.revision()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync revision is too large".to_string(),
                    )
                })?,
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )?;
        let current_state = ConnectorSyncState::from_persistence_json(state_json)
            .map_err(EventStoreError::InvalidState)?;
        let current_plan =
            ConnectorSyncPlan::from_persistence_json(request_json.as_deref().ok_or_else(|| {
                EventStoreError::InvalidState("connector sync recovery plan is missing".to_string())
            })?)
            .map_err(EventStoreError::InvalidState)?;
        if account != claim.account
            || account.health != ConnectorHealth::Connected
            || u64::try_from(generation).ok() != Some(claim.state.account_generation())
            || current_state != claim.state
            || current_plan != claim.plan
        {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery authority changed".to_string(),
            ));
        }
        let next_expires_at = now + Duration::seconds(CONNECTOR_SYNC_RECOVERY_LEASE_SECONDS);
        let changed = transaction.execute(
            r#"UPDATE connector_sync_recovery_jobs
               SET claim_expires_at = ?4, updated_at = ?5
               WHERE id = ?1 AND status = 'running' AND claim_id = ?2
                 AND claim_expires_at = ?3 AND claim_expires_at > ?5
                 AND account_id = ?6 AND account_generation = ?7
                 AND capability = ?8 AND stream_fingerprint = ?9
                 AND expected_state_revision = ?10"#,
            params![
                claim.job_id.to_string(),
                claim.claim_id.to_string(),
                claim
                    .claim_expires_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                next_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                claim.state.account_id().to_string(),
                i64::try_from(claim.state.account_generation()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync generation is too large".to_string(),
                    )
                })?,
                claim.state.capability().contract_name(),
                claim.state.stream_fingerprint(),
                i64::try_from(claim.state.revision()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync revision is too large".to_string(),
                    )
                })?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery claim was lost".to_string(),
            ));
        }
        transaction.commit()?;
        claim.claim_expires_at = next_expires_at;
        Ok(())
    }

    pub(crate) fn relinquish_unavailable_connector_sync_recovery_claim(
        &self,
        claim: &ConnectorSyncRecoveryClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let next_attempt_at = now + Duration::seconds(30);
        let changed = self.conn.execute(
            r#"UPDATE connector_sync_recovery_jobs
               SET status = 'queued', next_attempt_at = ?4,
                   attempt_count = CASE WHEN attempt_count > 0 THEN attempt_count - 1 ELSE 0 END,
                   claim_id = NULL, claim_expires_at = NULL, updated_at = ?5
               WHERE id = ?1 AND status = 'running' AND claim_id = ?2
                 AND claim_expires_at = ?3 AND claim_expires_at > ?5"#,
            params![
                claim.job_id.to_string(),
                claim.claim_id.to_string(),
                claim
                    .claim_expires_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                next_attempt_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery claim was lost".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn complete_claimed_mail_sync_recovery(
        &self,
        claim: &ConnectorSyncRecoveryClaim,
        page: &ConnectorSyncPage<MailMessage>,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorSyncState> {
        claim
            .plan
            .mail_request()
            .map_err(EventStoreError::InvalidState)?;
        if claim.state.capability() != ConnectorCapability::MailSyncInbox {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery claim capability is invalid".to_string(),
            ));
        }
        self.complete_claimed_connector_sync_recovery(
            claim,
            page,
            &|message| message.remote_ref.as_str(),
            now,
        )
    }

    pub(crate) fn complete_claimed_calendar_sync_recovery(
        &self,
        claim: &ConnectorSyncRecoveryClaim,
        page: &ConnectorSyncPage<CalendarEvent>,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorSyncState> {
        claim
            .plan
            .calendar_request()
            .map_err(EventStoreError::InvalidState)?;
        if claim.state.capability() != ConnectorCapability::CalendarSyncEvents {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery claim capability is invalid".to_string(),
            ));
        }
        self.complete_claimed_connector_sync_recovery(
            claim,
            page,
            &|event| event.remote_ref.as_str(),
            now,
        )
    }

    fn complete_claimed_connector_sync_recovery<T, F>(
        &self,
        claim: &ConnectorSyncRecoveryClaim,
        page: &ConnectorSyncPage<T>,
        remote_ref: &F,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorSyncState>
    where
        T: serde::Serialize,
        F: Fn(&T) -> &str,
    {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        validate_connector_sync_recovery_claim(&transaction, claim, now)?;
        let next = self.commit_connector_sync_page_in_transaction(
            &transaction,
            &claim.state,
            page,
            remote_ref,
            now,
        )?;
        let status = match page.continuation() {
            ConnectorSyncContinuation::Next(_) => "queued",
            ConnectorSyncContinuation::Delta(_) => "completed",
        };
        let changed = transaction.execute(
            r#"UPDATE connector_sync_recovery_jobs
               SET expected_state_revision = ?4, status = ?5,
                   next_attempt_at = ?6, claim_id = NULL, claim_expires_at = NULL,
                   quarantine_code = NULL, updated_at = ?6
               WHERE id = ?1 AND status = 'running' AND claim_id = ?2
                 AND claim_expires_at = ?3 AND claim_expires_at > ?6
                 AND account_id = ?7 AND account_generation = ?8
                 AND capability = ?9 AND stream_fingerprint = ?10
                 AND expected_state_revision = ?11"#,
            params![
                claim.job_id.to_string(),
                claim.claim_id.to_string(),
                claim
                    .claim_expires_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                i64::try_from(next.revision()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync revision is too large".to_string(),
                    )
                })?,
                status,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                claim.state.account_id().to_string(),
                i64::try_from(claim.state.account_generation()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync generation is too large".to_string(),
                    )
                })?,
                claim.state.capability().contract_name(),
                claim.state.stream_fingerprint(),
                i64::try_from(claim.state.revision()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync revision is too large".to_string(),
                    )
                })?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery claim was lost".to_string(),
            ));
        }
        transaction.commit()?;
        Ok(next)
    }

    pub(crate) fn finalize_claimed_connector_sync_recovery_failure(
        &self,
        claim: &ConnectorSyncRecoveryClaim,
        failure: ConnectorProviderFailure,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorSyncState> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        validate_connector_sync_recovery_claim(&transaction, claim, now)?;
        let (next, reason, repair_account) = match failure {
            ConnectorProviderFailure::AuthorizationExpired
            | ConnectorProviderFailure::PermissionDenied => (
                claim
                    .state
                    .stop_after_recovery_failure(now)
                    .map_err(EventStoreError::InvalidState)?,
                "account_repair_required",
                true,
            ),
            ConnectorProviderFailure::CursorExpired => (
                claim
                    .state
                    .stop_after_recovery_failure(now)
                    .map_err(EventStoreError::InvalidState)?,
                "cursor_repair_required",
                false,
            ),
            ConnectorProviderFailure::RateLimited {
                retry_after_seconds,
            } => match claim
                .state
                .recovery(
                    ConnectorSyncFailure::RateLimited {
                        retry_after_seconds,
                    },
                    claim.attempt_count,
                    3,
                    now,
                )
                .map_err(EventStoreError::InvalidState)?
            {
                ConnectorSyncStateRecovery::Persist { next, reason } => (next, reason, false),
                ConnectorSyncStateRecovery::RepairAccount => unreachable!(),
            },
            ConnectorProviderFailure::NetworkUnavailable => match claim
                .state
                .recovery(
                    ConnectorSyncFailure::NetworkUnavailable,
                    claim.attempt_count,
                    3,
                    now,
                )
                .map_err(EventStoreError::InvalidState)?
            {
                ConnectorSyncStateRecovery::Persist { next, reason } => (next, reason, false),
                ConnectorSyncStateRecovery::RepairAccount => unreachable!(),
            },
            ConnectorProviderFailure::RemoteNotFound
            | ConnectorProviderFailure::InvalidResponse => match claim
                .state
                .recovery(
                    ConnectorSyncFailure::InvalidResponse,
                    claim.attempt_count,
                    3,
                    now,
                )
                .map_err(EventStoreError::InvalidState)?
            {
                ConnectorSyncStateRecovery::Persist { next, reason } => (next, reason, false),
                ConnectorSyncStateRecovery::RepairAccount => unreachable!(),
            },
        };
        if repair_account {
            let mut next_account = claim.account.clone();
            next_account.health = ConnectorHealth::NeedsRepair;
            next_account.updated_at = now;
            let changed = transaction.execute(
                r#"UPDATE connector_accounts
                   SET account_json = ?3, health = ?4, updated_at = ?5
                   WHERE id = ?1 AND account_json = ?2 AND health = ?6
                     AND EXISTS (
                       SELECT 1 FROM connector_account_generations
                       WHERE account_id = ?1 AND generation = ?7
                     )"#,
                params![
                    claim.account.id.to_string(),
                    serde_json::to_string(&claim.account)?,
                    serde_json::to_string(&next_account)?,
                    serde_json::to_string(&ConnectorHealth::NeedsRepair)?,
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    serde_json::to_string(&ConnectorHealth::Connected)?,
                    i64::try_from(claim.state.account_generation()).map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector sync generation is too large".to_string(),
                        )
                    })?,
                ],
            )?;
            if changed != 1 {
                return Err(EventStoreError::InvalidState(
                    "connector sync recovery account changed".to_string(),
                ));
            }
        }
        let next_json = next
            .persistence_json()
            .map_err(EventStoreError::InvalidState)?;
        let changed = transaction.execute(
            r#"UPDATE connector_sync_streams
               SET state_json = ?4, revision = ?5, updated_at = ?6
               WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                 AND state_json = ?7 AND revision = ?8"#,
            params![
                claim.state.account_id().to_string(),
                claim.state.capability().contract_name(),
                claim.state.stream_fingerprint(),
                next_json,
                i64::try_from(next.revision()).map_err(|_| EventStoreError::InvalidState(
                    "connector sync revision is too large".to_string(),
                ))?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                claim
                    .state
                    .persistence_json()
                    .map_err(EventStoreError::InvalidState)?,
                i64::try_from(claim.state.revision()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync revision is too large".to_string(),
                    )
                })?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery state changed".to_string(),
            ));
        }
        let status = if next.stopped() {
            "repair_required"
        } else {
            "backoff"
        };
        let next_attempt_at = next
            .retry_state()
            .map(|retry| retry.retry_at)
            .unwrap_or(now);
        let changed = transaction.execute(
            r#"UPDATE connector_sync_recovery_jobs
               SET expected_state_revision = ?4, status = ?5, next_attempt_at = ?6,
                   claim_id = NULL, claim_expires_at = NULL,
                   quarantine_code = ?7, updated_at = ?8
               WHERE id = ?1 AND status = 'running' AND claim_id = ?2
                 AND claim_expires_at = ?3 AND claim_expires_at > ?8
                 AND expected_state_revision = ?9"#,
            params![
                claim.job_id.to_string(),
                claim.claim_id.to_string(),
                claim
                    .claim_expires_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                i64::try_from(next.revision()).map_err(|_| EventStoreError::InvalidState(
                    "connector sync revision is too large".to_string(),
                ))?,
                status,
                next_attempt_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                if next.stopped() { Some(reason) } else { None },
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                i64::try_from(claim.state.revision()).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync revision is too large".to_string(),
                    )
                })?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector sync recovery claim was lost".to_string(),
            ));
        }
        let event = KernelEvent::new(
            "connector.sync_recovery.failed",
            &serde_json::json!({
                "kind": "read_sync",
                "capability": match claim.state.capability() {
                    ConnectorCapability::MailSyncInbox => "mail",
                    ConnectorCapability::CalendarSyncEvents => "calendar",
                    _ => "unsupported",
                },
                "outcome": if repair_account {
                    "account_repair_required"
                } else if next.stopped() {
                    "repair_required"
                } else {
                    "deferred"
                },
                "changed_at": now,
            }),
        )?;
        Self::insert_kernel_event(&transaction, &event)?;
        transaction.commit()?;
        Ok(next)
    }

    pub(crate) fn schedule_connector_reconciliation_from_recovery(
        &self,
        item_id: Uuid,
        action_revision: &str,
        registry: &dyn ConnectorReconcilerRegistry,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorRecoveryAcceptance> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        const ACTION_KIND: &str = "read_only_verification";
        if connector_recovery_action_was_accepted(
            &transaction,
            ACTION_KIND,
            item_id,
            action_revision,
        )? {
            return Ok(ConnectorRecoveryAcceptance::AlreadyAccepted);
        }
        let snapshot = load_connector_reconciliation_recovery_snapshot(&transaction, item_id)?;
        if action_revision.trim().is_empty()
            || !constant_time_text_eq(
                action_revision,
                &connector_reconciliation_recovery_action_revision(&snapshot)?,
            )
        {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation recovery action is invalid".to_string(),
            ));
        }
        if !registry.supports(
            &snapshot.invocation.provider_id,
            snapshot.invocation.capability,
        ) {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation recovery is unavailable".to_string(),
            ));
        }
        let changed = transaction.execute(
            r#"UPDATE connector_invocations
               SET next_reconciliation_at = ?2
               WHERE id = ?1 AND status = ?3
                 AND reconciliation_quarantine_code IS NULL
                 AND account_generation = ?4 AND invocation_json = ?5
                 AND next_reconciliation_at = ?6
                 AND reconciliation_attempt_count = ?7 AND updated_at = ?8
                 AND recovery_revision = ?9
                 AND reconciliation_claim_id IS NULL
                 AND reconciliation_claim_expires_at IS NULL"#,
            params![
                item_id.to_string(),
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorInvocationStatus::ReconciliationRequired)?,
                snapshot.projected_generation,
                snapshot.invocation_json,
                snapshot.next_reconciliation_at,
                snapshot.attempt_count,
                snapshot.updated_at,
                snapshot.recovery_revision,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation recovery action raced".to_string(),
            ));
        }
        let event = KernelEvent::new(
            "connector.reconciliation_inspection.scheduled",
            &serde_json::json!({
                "recovery_item_id": item_id,
                "kind": "read_only_verification",
                "changed_at": changed_at,
            }),
        )?;
        Self::insert_kernel_event(&transaction, &event)?;
        record_connector_recovery_action_acceptance(
            &transaction,
            ACTION_KIND,
            item_id,
            action_revision,
            changed_at,
        )?;
        transaction.commit()?;
        Ok(ConnectorRecoveryAcceptance::Accepted)
    }

    pub fn append_connector_invocation(
        &self,
        invocation: &ConnectorInvocation,
    ) -> EventStoreResult<bool> {
        if invocation.capability.external_mutation()
            && invocation.mutation.as_ref().is_some_and(|mutation| {
                mutation.account_generation != invocation.account_generation
            })
        {
            return Err(EventStoreError::InvalidState(
                "connector mutation account generation projection is inconsistent".to_string(),
            ));
        }
        let account_generation = invocation
            .account_generation
            .map(|generation| {
                i64::try_from(generation).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector mutation account generation is too large".to_string(),
                    )
                })
            })
            .transpose()?;
        let inserted = self.conn.execute(
            r#"INSERT OR IGNORE INTO connector_invocations
               (id, account_id, account_generation, idempotency_key, invocation_json, status, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
            params![
                invocation.id.to_string(),
                invocation.account_id.to_string(),
                account_generation,
                invocation.idempotency_key,
                serde_json::to_string(invocation)?,
                serde_json::to_string(&invocation.status)?,
                invocation
                    .updated_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        Ok(inserted == 1)
    }

    pub fn list_connector_invocations(&self) -> EventStoreResult<Vec<ConnectorInvocation>> {
        let mut statement = self.conn.prepare(
            "SELECT invocation_json FROM connector_invocations ORDER BY updated_at ASC, rowid ASC",
        )?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .collect()
    }

    pub fn connector_invocation(&self, id: Uuid) -> EventStoreResult<ConnectorInvocation> {
        let json = self.conn.query_row(
            "SELECT invocation_json FROM connector_invocations WHERE id = ?1",
            params![id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        Ok(serde_json::from_str(&json)?)
    }

    pub(crate) fn prepare_connector_attachment_download_approval(
        &self,
        metadata: &ConnectorAttachmentMetadata,
        workspace_root: &Path,
        run_id: Option<Uuid>,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<(CapabilityAccessRecord, ToolInvocationRecord)> {
        let (workspace_root, workspace_identity) =
            connector_attachment_workspace_binding(workspace_root)
                .map_err(EventStoreError::InvalidState)?;
        let workspace_root_text = workspace_root
            .to_str()
            .ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector attachment workspace path encoding is unsupported".to_string(),
                )
            })?
            .to_string();
        let transaction = self.conn.unchecked_transaction()?;
        let (account_json, generation) = transaction
            .query_row(
                r#"SELECT account.account_json, generation.generation
                   FROM connector_accounts AS account
                   JOIN connector_account_generations AS generation
                     ON generation.account_id = account.id
                   WHERE account.id = ?1 AND account.provider_id = ?2 AND account.health = ?3"#,
                params![
                    metadata.account_id.to_string(),
                    metadata.provider_id,
                    serde_json::to_string(&ConnectorHealth::Connected)?,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector attachment account is unavailable".to_string(),
                )
            })?;
        let account: ConnectorAccount = serde_json::from_str(&account_json)?;
        if !account
            .granted_capabilities
            .contains(&ConnectorCapability::MailReadAttachment)
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment account has no attachment-read grant".to_string(),
            ));
        }
        let generation = u64::try_from(generation).map_err(|_| {
            EventStoreError::InvalidState("connector account generation is invalid".to_string())
        })?;
        let exact_request = metadata
            .tool_request(
                generation,
                &workspace_identity,
                AccessMode::AskEveryStep,
                run_id,
            )
            .map_err(EventStoreError::InvalidState)?;
        let plan = prepare_tool_execution(&exact_request).map_err(EventStoreError::InvalidState)?;
        if plan.policy_decision != PolicyDecision::Ask
            || plan.contract.capability != CapabilityKind::ConnectorAttachmentRead
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment policy did not require exact approval".to_string(),
            ));
        }
        let request_fingerprint = tool_request_fingerprint(&exact_request);
        let existing = transaction
            .query_row(
                r#"SELECT request_id, tool_invocation_id
                   FROM connector_attachment_sources WHERE request_fingerprint = ?1"#,
                params![request_fingerprint],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        if let Some((request_id, tool_invocation_id)) = existing {
            transaction.commit()?;
            return Ok((
                self.capability_access_record_by_id(Uuid::parse_str(&request_id)?)?,
                self.tool_invocation_by_id(Uuid::parse_str(&tool_invocation_id)?)?,
            ));
        }

        let mut request = request_capability_access(
            AccessMode::AskEveryStep,
            CapabilityKind::ConnectorAttachmentRead,
        )
        .map_err(EventStoreError::InvalidState)?;
        request.created_at = changed_at;
        request
            .bind_exact_tool(
                plan.contract.id.clone(),
                request_fingerprint.clone(),
                tool_approval_preview(&exact_request),
            )
            .map_err(EventStoreError::InvalidState)?;
        let audit = PermissionAuditEntry::evaluate(
            AccessMode::AskEveryStep,
            CapabilityKind::ConnectorAttachmentRead,
        );
        let tool = ToolInvocationRecord::waiting_for_confirmation(&plan, request.id);
        let capability_invocation = CapabilityInvocation {
            id: tool.id,
            capability: CapabilityKind::ConnectorAttachmentRead,
            status: CapabilityInvocationStatus::PendingApproval,
            policy_decision: PolicyDecision::Ask,
            approval_request_id: Some(request.id),
            requested_resource: Some(metadata.file_name.trim().to_string()),
            evidence_ref: None,
            requested_url: None,
            evidence_url: None,
            title: Some("Connected attachment awaiting exact approval".to_string()),
            excerpt: None,
            warnings: vec!["Attachment content remains untrusted evidence.".to_string()],
            elapsed_ms: 0,
            created_at: changed_at,
        };
        transaction.execute(
            r#"INSERT INTO connector_attachment_sources
               (request_id, tool_invocation_id, request_fingerprint, metadata_json,
                account_generation, workspace_root, workspace_identity, created_at, expires_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
            params![
                request.id.to_string(),
                tool.id.to_string(),
                request_fingerprint,
                serde_json::to_string(metadata)?,
                i64::try_from(generation).map_err(|_| EventStoreError::InvalidState(
                    "connector account generation is too large".to_string()
                ))?,
                workspace_root_text,
                workspace_identity,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                (changed_at + Duration::minutes(15)).to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        for event in [
            KernelEvent::new(CAPABILITY_ACCESS_REQUESTED_EVENT, &request)?,
            KernelEvent::new(PERMISSION_AUDIT_RECORDED_EVENT, &audit)?,
            KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool)?,
            KernelEvent::new(CAPABILITY_INVOCATION_RECORDED_EVENT, &capability_invocation)?,
        ] {
            Self::insert_kernel_event(&transaction, &event)?;
        }
        transaction.commit()?;
        Ok((self.capability_access_record_by_id(request.id)?, tool))
    }

    #[cfg(test)]
    fn reserve_connector_attachment_download(
        &self,
        metadata: &ConnectorAttachmentMetadata,
        workspace_root: &Path,
        tool_invocation_id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAttachmentDownloadPermit> {
        let (workspace_root, workspace_identity) =
            connector_attachment_workspace_binding(workspace_root)
                .map_err(EventStoreError::InvalidState)?;
        let workspace_root_text = workspace_root
            .to_str()
            .ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector attachment workspace path encoding is unsupported".to_string(),
                )
            })?
            .to_string();
        let account = self
            .list_connector_accounts()?
            .into_iter()
            .find(|account| account.id == metadata.account_id)
            .ok_or_else(|| EventStoreError::NotFound("connector account".to_string()))?;
        let generation = self.connector_account_sync_generation(&account)?;
        if account.health != ConnectorHealth::Connected
            || account.provider_id != metadata.provider_id
            || !account
                .granted_capabilities
                .contains(&crate::kernel::connectors::ConnectorCapability::MailReadAttachment)
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment account is not ready".to_string(),
            ));
        }
        let mut tool_record = self.tool_invocation_by_id(tool_invocation_id)?;
        let approval_request_id = tool_record.approval_request_id.ok_or_else(|| {
            EventStoreError::InvalidState(
                "connector attachment approval request is missing".to_string(),
            )
        })?;
        let approval = self.capability_access_record_by_id(approval_request_id)?;
        if approval.request.capability != CapabilityKind::ConnectorAttachmentRead
            || approval.effective_status != CapabilityAccessStatus::Approved
            || approval.grant_state != CapabilityGrantState::OneShotAvailable
        {
            return Err(EventStoreError::InvalidState(
                "exact connector attachment approval is unavailable".to_string(),
            ));
        }
        let expected_request = metadata
            .tool_request(
                generation,
                &workspace_identity,
                approval.request.access_mode,
                tool_record.run_id,
            )
            .map_err(EventStoreError::InvalidState)?;
        let expected_request_fingerprint = tool_request_fingerprint(&expected_request);
        let exact_scope_matches = approval.request.exact_tool.as_ref().is_some_and(|scope| {
            scope.tool_id == CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID
                && scope.request_fingerprint == expected_request_fingerprint
                && scope.preview == tool_approval_preview(&expected_request)
        });
        if tool_record.tool_id != CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID
            || tool_record.capability != CapabilityKind::ConnectorAttachmentRead
            || tool_record.status != ToolExecutionStatus::WaitingForConfirmation
            || tool_record.request_fingerprint != expected_request_fingerprint
            || !exact_scope_matches
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment tool is not bound to the exact approved request".to_string(),
            ));
        }

        let landing_id = Uuid::new_v4();
        let landing_fingerprint = connector_attachment_landing_fingerprint(metadata, generation)
            .map_err(EventStoreError::InvalidState)?;
        tool_record.status = ToolExecutionStatus::Running;
        tool_record.verification =
            ToolVerificationResult::failed("connector attachment download is in progress");
        tool_record.error = None;
        tool_record.finished_at = None;
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool_record)?;
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute(
            r#"INSERT INTO connector_attachment_approval_consumptions
               (request_id, landing_id, consumed_at) VALUES (?1, ?2, ?3)"#,
            params![
                approval_request_id.to_string(),
                landing_id.to_string(),
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        let inserted = transaction.execute(
            r#"INSERT INTO connector_attachment_landings
               (id, account_id, account_generation, metadata_json,
                 tool_invocation_id, approval_request_id, request_fingerprint,
                 landing_fingerprint, workspace_root, workspace_identity, status,
                 receipt_json, failure_kind, created_at, updated_at, size_bytes)
                SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                       'reserved', NULL, NULL, ?11, ?11, ?14
                WHERE EXISTS (
                  SELECT 1 FROM connector_accounts AS account
                  JOIN connector_account_generations AS generation
                    ON generation.account_id = account.id
                  WHERE account.id = ?2
                    AND account.provider_id = ?12
                    AND account.health = ?13
                    AND generation.generation = ?3
                )
                AND (SELECT COUNT(*) FROM connector_attachment_landings
                     WHERE workspace_identity = ?10
                       AND status IN ('reserved', 'staging', 'ready', 'completed',
                                      'retention_cleanup')) < ?15
                AND COALESCE((SELECT SUM(size_bytes) FROM connector_attachment_landings
                              WHERE workspace_identity = ?10
                                AND status IN ('reserved', 'staging', 'ready', 'completed',
                                               'retention_cleanup')), 0) + ?14 <= ?16"#,
            params![
                landing_id.to_string(),
                metadata.account_id.to_string(),
                i64::try_from(generation).map_err(|_| EventStoreError::InvalidState(
                    "connector account generation is too large".to_string()
                ))?,
                serde_json::to_string(&durable_connector_attachment_metadata(metadata))?,
                tool_invocation_id.to_string(),
                approval_request_id.to_string(),
                expected_request_fingerprint,
                landing_fingerprint,
                workspace_root_text,
                workspace_identity,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                metadata.provider_id,
                serde_json::to_string(&ConnectorHealth::Connected)?,
                i64::try_from(metadata.size_bytes).map_err(|_| EventStoreError::InvalidState(
                    "connector attachment size is too large".to_string()
                ))?,
                MAX_RETAINED_ATTACHMENTS_PER_WORKSPACE,
                MAX_RETAINED_ATTACHMENT_BYTES_PER_WORKSPACE,
            ],
        )?;
        if inserted != 1 {
            return Err(EventStoreError::InvalidState(
                "connector attachment account changed before reservation".to_string(),
            ));
        }
        Self::insert_kernel_event(&transaction, &tool_event)?;
        transaction.commit()?;
        ConnectorAttachmentDownloadPermit::reserved(
            landing_id,
            metadata,
            generation,
            landing_fingerprint,
            workspace_identity,
        )
        .map_err(EventStoreError::InvalidState)
    }

    pub(crate) fn approve_and_reserve_connector_attachment_download(
        &self,
        request_id: Uuid,
        expected_request_revision: u64,
        expected_preview_revision: u32,
        expected_preview_hash: &str,
        note: String,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAttachmentDownloadPermit> {
        let (
            tool_invocation_id,
            request_fingerprint,
            metadata_json,
            generation,
            workspace_root,
            stored_workspace_identity,
            expires_at,
        ) = self
            .conn
            .query_row(
                r#"SELECT tool_invocation_id, request_fingerprint, metadata_json,
                          account_generation, workspace_root, workspace_identity, expires_at
                   FROM connector_attachment_sources WHERE request_id = ?1"#,
                params![request_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| {
                EventStoreError::NotFound(
                    "connector attachment approval source is unavailable".to_string(),
                )
            })?;
        let expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        if expires_at <= changed_at {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval source expired".to_string(),
            ));
        }
        let generation = u64::try_from(generation).map_err(|_| {
            EventStoreError::InvalidState("connector account generation is invalid".to_string())
        })?;
        let metadata: ConnectorAttachmentMetadata = serde_json::from_str(&metadata_json)?;
        let workspace_root = PathBuf::from(workspace_root);
        let (workspace_root, workspace_identity) =
            connector_attachment_workspace_binding(&workspace_root)
                .map_err(EventStoreError::InvalidState)?;
        if workspace_identity != stored_workspace_identity {
            return Err(EventStoreError::InvalidState(
                "connector attachment workspace identity changed".to_string(),
            ));
        }
        let workspace_root_text = workspace_root
            .to_str()
            .ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector attachment workspace path encoding is unsupported".to_string(),
                )
            })?
            .to_string();
        let approval = self.capability_access_record_by_id(request_id)?;
        if approval.request.capability != CapabilityKind::ConnectorAttachmentRead
            || approval.effective_status != CapabilityAccessStatus::PendingApproval
            || approval.resolution.is_some()
            || approval.projection_revision != expected_request_revision
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval is stale or unavailable".to_string(),
            ));
        }
        let scope = approval.request.exact_tool.as_ref().ok_or_else(|| {
            EventStoreError::InvalidState(
                "connector attachment exact preview evidence is missing".to_string(),
            )
        })?;
        if scope.preview_revision != expected_preview_revision
            || scope.preview_hash != expected_preview_hash
            || scope.request_fingerprint != request_fingerprint
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval preview changed".to_string(),
            ));
        }
        let tool_invocation_id = Uuid::parse_str(&tool_invocation_id)?;
        let mut tool_record = self.tool_invocation_by_id(tool_invocation_id)?;
        let expected_request = metadata
            .tool_request(
                generation,
                &workspace_identity,
                approval.request.access_mode,
                tool_record.run_id,
            )
            .map_err(EventStoreError::InvalidState)?;
        let expected_fingerprint = tool_request_fingerprint(&expected_request);
        if expected_fingerprint != request_fingerprint
            || scope.tool_id != CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID
            || scope.preview != tool_approval_preview(&expected_request)
            || tool_record.approval_request_id != Some(request_id)
            || tool_record.tool_id != CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID
            || tool_record.capability != CapabilityKind::ConnectorAttachmentRead
            || tool_record.status != ToolExecutionStatus::WaitingForConfirmation
            || tool_record.request_fingerprint != request_fingerprint
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment tool is not bound to the exact approved request".to_string(),
            ));
        }

        let resolution = PermissionResolution::new_exact(
            request_id,
            true,
            note,
            expected_request_revision,
            scope,
        )
        .map_err(EventStoreError::InvalidState)?;
        let landing_id = Uuid::new_v4();
        let landing_fingerprint = connector_attachment_landing_fingerprint(&metadata, generation)
            .map_err(EventStoreError::InvalidState)?;
        tool_record.status = ToolExecutionStatus::Running;
        tool_record.verification =
            ToolVerificationResult::failed("connector attachment download is in progress");
        tool_record.error = None;
        tool_record.finished_at = None;
        let resolution_event = KernelEvent::new(PERMISSION_RESOLUTION_RECORDED_EVENT, &resolution)?;
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool_record)?;
        let transaction = self.conn.unchecked_transaction()?;
        Self::insert_kernel_event(&transaction, &resolution_event)?;
        transaction.execute(
            r#"INSERT INTO connector_attachment_approval_consumptions
               (request_id, landing_id, consumed_at) VALUES (?1, ?2, ?3)"#,
            params![
                request_id.to_string(),
                landing_id.to_string(),
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        let inserted = transaction.execute(
            r#"INSERT INTO connector_attachment_landings
               (id, account_id, account_generation, metadata_json,
                tool_invocation_id, approval_request_id, request_fingerprint,
                landing_fingerprint, workspace_root, workspace_identity, status,
                receipt_json, failure_kind, created_at, updated_at, size_bytes)
               SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                      'reserved', NULL, NULL, ?11, ?11, ?14
               WHERE EXISTS (
                 SELECT 1 FROM connector_accounts AS account
                 JOIN connector_account_generations AS generation
                   ON generation.account_id = account.id
                 WHERE account.id = ?2 AND account.provider_id = ?12
                   AND account.health = ?13 AND generation.generation = ?3
               )
               AND (SELECT COUNT(*) FROM connector_attachment_landings
                    WHERE workspace_identity = ?10
                      AND status IN ('reserved', 'staging', 'ready', 'completed',
                                     'retention_cleanup')) < ?15
               AND COALESCE((SELECT SUM(size_bytes) FROM connector_attachment_landings
                             WHERE workspace_identity = ?10
                               AND status IN ('reserved', 'staging', 'ready', 'completed',
                                              'retention_cleanup')), 0) + ?14 <= ?16"#,
            params![
                landing_id.to_string(),
                metadata.account_id.to_string(),
                i64::try_from(generation).map_err(|_| EventStoreError::InvalidState(
                    "connector account generation is too large".to_string()
                ))?,
                serde_json::to_string(&durable_connector_attachment_metadata(&metadata))?,
                tool_invocation_id.to_string(),
                request_id.to_string(),
                request_fingerprint,
                landing_fingerprint,
                workspace_root_text,
                workspace_identity,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                metadata.provider_id,
                serde_json::to_string(&ConnectorHealth::Connected)?,
                i64::try_from(metadata.size_bytes).map_err(|_| EventStoreError::InvalidState(
                    "connector attachment size is too large".to_string()
                ))?,
                MAX_RETAINED_ATTACHMENTS_PER_WORKSPACE,
                MAX_RETAINED_ATTACHMENT_BYTES_PER_WORKSPACE,
            ],
        )?;
        if inserted != 1 {
            return Err(EventStoreError::InvalidState(
                "connector attachment account changed before reservation".to_string(),
            ));
        }
        transaction.execute(
            r#"INSERT INTO connector_attachment_active_sources
               (landing_id, metadata_json, created_at, expires_at)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![
                landing_id.to_string(),
                serde_json::to_string(&metadata)?,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        let source_deleted = transaction.execute(
            r#"DELETE FROM connector_attachment_sources
               WHERE request_id = ?1 AND tool_invocation_id = ?2"#,
            params![request_id.to_string(), tool_invocation_id.to_string()],
        )?;
        if source_deleted != 1 {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval source changed".to_string(),
            ));
        }
        Self::insert_kernel_event(&transaction, &tool_event)?;
        transaction.commit()?;
        ConnectorAttachmentDownloadPermit::reserved(
            landing_id,
            &metadata,
            generation,
            landing_fingerprint,
            stored_workspace_identity,
        )
        .map_err(EventStoreError::InvalidState)
    }

    pub(crate) fn reject_connector_attachment_download(
        &self,
        request_id: Uuid,
        expected_request_revision: u64,
        expected_preview_revision: u32,
        expected_preview_hash: &str,
        note: String,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<PermissionResolution> {
        let tool_invocation_id = self
            .conn
            .query_row(
                "SELECT tool_invocation_id FROM connector_attachment_sources WHERE request_id = ?1",
                params![request_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| {
                EventStoreError::NotFound(
                    "connector attachment approval source is unavailable".to_string(),
                )
            })?;
        let approval = self.capability_access_record_by_id(request_id)?;
        if approval.request.capability != CapabilityKind::ConnectorAttachmentRead
            || approval.effective_status != CapabilityAccessStatus::PendingApproval
            || approval.resolution.is_some()
            || approval.projection_revision != expected_request_revision
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval is stale or unavailable".to_string(),
            ));
        }
        let scope = approval.request.exact_tool.as_ref().ok_or_else(|| {
            EventStoreError::InvalidState(
                "connector attachment exact preview evidence is missing".to_string(),
            )
        })?;
        if scope.preview_revision != expected_preview_revision
            || scope.preview_hash != expected_preview_hash
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval preview changed".to_string(),
            ));
        }
        let mut tool = self.tool_invocation_by_id(Uuid::parse_str(&tool_invocation_id)?)?;
        if tool.approval_request_id != Some(request_id)
            || tool.status != ToolExecutionStatus::WaitingForConfirmation
            || tool.tool_id != CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval tool changed".to_string(),
            ));
        }
        let resolution = PermissionResolution::new_exact(
            request_id,
            false,
            note,
            expected_request_revision,
            scope,
        )
        .map_err(EventStoreError::InvalidState)?;
        tool.status = ToolExecutionStatus::Blocked;
        tool.output = None;
        tool.evidence.clear();
        tool.verification =
            ToolVerificationResult::failed("connector attachment download was rejected");
        tool.error = Some("local user rejected this exact attachment download".to_string());
        tool.finished_at = Some(changed_at);
        let capability_invocation = CapabilityInvocation {
            id: tool.id,
            capability: CapabilityKind::ConnectorAttachmentRead,
            status: CapabilityInvocationStatus::Failed,
            policy_decision: PolicyDecision::Ask,
            approval_request_id: Some(request_id),
            requested_resource: None,
            evidence_ref: None,
            requested_url: None,
            evidence_url: None,
            title: Some("Connected attachment rejected".to_string()),
            excerpt: None,
            warnings: vec!["No attachment bytes were downloaded.".to_string()],
            elapsed_ms: 0,
            created_at: changed_at,
        };
        let transaction = self.conn.unchecked_transaction()?;
        for event in [
            KernelEvent::new(PERMISSION_RESOLUTION_RECORDED_EVENT, &resolution)?,
            KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool)?,
            KernelEvent::new(CAPABILITY_INVOCATION_RECORDED_EVENT, &capability_invocation)?,
        ] {
            Self::insert_kernel_event(&transaction, &event)?;
        }
        let deleted = transaction.execute(
            r#"DELETE FROM connector_attachment_sources
               WHERE request_id = ?1 AND tool_invocation_id = ?2"#,
            params![request_id.to_string(), tool_invocation_id],
        )?;
        if deleted != 1 {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval source changed".to_string(),
            ));
        }
        transaction.commit()?;
        Ok(resolution)
    }

    pub(crate) fn complete_connector_attachment_landing(
        &self,
        landed: &LandedConnectorAttachment,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        self.complete_connector_attachment_landing_with_claim(landed, None, changed_at)
    }

    pub(crate) fn complete_recovered_connector_attachment_landing(
        &self,
        landed: &LandedConnectorAttachment,
        claim_id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        self.complete_connector_attachment_landing_with_claim(landed, Some(claim_id), changed_at)
    }

    fn complete_connector_attachment_landing_with_claim(
        &self,
        landed: &LandedConnectorAttachment,
        recovery_claim_id: Option<Uuid>,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let receipt = landed.receipt();
        if !receipt.untrusted_evidence
            || receipt.landing_ref.trim().is_empty()
            || receipt.landing_ref.contains(['/', '\\', ':'])
            || receipt.sha256.len() != 64
            || !receipt.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || receipt.byte_size == 0
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment receipt is invalid".to_string(),
            ));
        }
        let (account_id, generation, metadata_json, tool_invocation_id, status, ready_receipt_json) =
            self.conn.query_row(
                r#"SELECT account_id, account_generation, metadata_json,
                          tool_invocation_id, status, receipt_json
                   FROM connector_attachment_landings WHERE id = ?1"#,
                params![receipt.landing_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )?;
        if status == "completed" && recovery_claim_id.is_none() {
            let stored: String = self.conn.query_row(
                "SELECT receipt_json FROM connector_attachment_landings WHERE id = ?1",
                params![receipt.landing_id.to_string()],
                |row| row.get(0),
            )?;
            let stored: ConnectorAttachmentLandingReceipt = serde_json::from_str(&stored)?;
            return if stored == *receipt {
                Ok(())
            } else {
                Err(EventStoreError::InvalidState(
                    "connector attachment completion replay changed".to_string(),
                ))
            };
        }
        if status != "ready" {
            return Err(EventStoreError::InvalidState(
                "connector attachment landing is not reserved".to_string(),
            ));
        }
        let metadata: ConnectorAttachmentMetadata = serde_json::from_str(&metadata_json)?;
        let ready_receipt = ready_receipt_json
            .ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector attachment ready receipt is missing".to_string(),
                )
            })
            .and_then(|json| {
                serde_json::from_str::<ConnectorAttachmentLandingReceipt>(&json).map_err(Into::into)
            })?;
        if ready_receipt != *receipt {
            return Err(EventStoreError::InvalidState(
                "connector attachment commit does not match the ready receipt".to_string(),
            ));
        }
        let expected_account_id = Uuid::parse_str(&account_id)?;
        let expected_generation = u64::try_from(generation).map_err(|_| {
            EventStoreError::InvalidState("connector attachment generation is invalid".to_string())
        })?;
        if receipt.account_id != expected_account_id
            || receipt.account_generation != expected_generation
            || receipt.provider_id != metadata.provider_id
            || receipt.media_type != metadata.declared_media_type
            || receipt.byte_size != metadata.size_bytes
            || receipt.landing_ref
                != metadata
                    .expected_landing_ref(receipt.landing_id)
                    .map_err(EventStoreError::InvalidState)?
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment receipt does not match reservation".to_string(),
            ));
        }
        let tool_invocation_id = Uuid::parse_str(&tool_invocation_id)?;
        let mut tool_record = self.tool_invocation_by_id(tool_invocation_id)?;
        if tool_record.status != ToolExecutionStatus::Running
            || tool_record.capability != CapabilityKind::ConnectorAttachmentRead
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment tool is not running".to_string(),
            ));
        }
        tool_record.status = ToolExecutionStatus::Succeeded;
        tool_record.output = Some(serde_json::json!({
            "landing_ref": receipt.landing_ref,
            "sha256": receipt.sha256,
            "byte_size": receipt.byte_size,
            "media_type": receipt.media_type,
        }));
        tool_record.evidence = vec![ToolEvidence {
            kind: "connector_attachment".to_string(),
            reference: receipt.landing_ref.clone(),
            summary: "Approved connector attachment landed as untrusted evidence.".to_string(),
        }];
        tool_record.verification =
            ToolVerificationResult::passed("attachment hash, type, size, and generation verified");
        tool_record.finished_at = Some(changed_at);
        tool_record.error = None;
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool_record)?;
        let receipt_event = KernelEvent::new(CONNECTOR_ATTACHMENT_LANDED_EVENT, receipt)?;
        let recovery_claim_id = recovery_claim_id.map(|value| value.to_string());
        let transaction = self.conn.unchecked_transaction()?;
        let changed = transaction.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'completed', receipt_json = ?2, updated_at = ?3,
                   expires_at = ?4, next_cleanup_at = ?4, cleanup_claim_id = NULL,
                   cleanup_claim_expires_at = NULL
               WHERE id = ?1 AND status = 'ready' AND receipt_json = ?2
                  AND ((?6 IS NULL AND cleanup_claim_id IS NULL)
                       OR (cleanup_claim_id = ?6 AND cleanup_claim_expires_at > ?3))
                 AND EXISTS (
                 SELECT 1 FROM connector_accounts AS account
                 JOIN connector_account_generations AS generation
                   ON generation.account_id = account.id
                 WHERE account.id = connector_attachment_landings.account_id
                   AND account.health = ?5
                   AND generation.generation = connector_attachment_landings.account_generation
               )"#,
            params![
                receipt.landing_id.to_string(),
                serde_json::to_string(receipt)?,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                (changed_at + Duration::days(CONNECTOR_ATTACHMENT_RETENTION_DAYS))
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorHealth::Connected)?,
                recovery_claim_id,
            ],
        )?;
        if changed != 1 {
            let replay = transaction
                .query_row(
                    r#"SELECT status, receipt_json FROM connector_attachment_landings
                       WHERE id = ?1"#,
                    params![receipt.landing_id.to_string()],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?;
            if recovery_claim_id.is_none() {
                if let Some((status, Some(stored))) = replay {
                    if status == "completed"
                        && serde_json::from_str::<ConnectorAttachmentLandingReceipt>(&stored)?
                            == *receipt
                    {
                        transaction.commit()?;
                        return Ok(());
                    }
                }
            }
            return Err(EventStoreError::InvalidState(
                "connector attachment account changed before completion".to_string(),
            ));
        }
        for event in [tool_event, receipt_event] {
            Self::insert_kernel_event(&transaction, &event)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn assert_connector_attachment_execution_current(
        &self,
        landing_id: Uuid,
    ) -> EventStoreResult<()> {
        let binding = self
            .conn
            .query_row(
                r#"SELECT landing.workspace_root, landing.workspace_identity
                 FROM connector_attachment_landings AS landing
                 JOIN connector_accounts AS account ON account.id = landing.account_id
                 JOIN connector_account_generations AS generation
                   ON generation.account_id = account.id
                 WHERE landing.id = ?1
                   AND landing.status IN ('reserved', 'staging')
                   AND account.health = ?2
                   AND generation.generation = landing.account_generation
                   AND landing.workspace_root IS NOT NULL
                   AND landing.workspace_identity IS NOT NULL"#,
                params![
                    landing_id.to_string(),
                    serde_json::to_string(&ConnectorHealth::Connected)?,
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((workspace_root, stored_workspace_identity)) = binding else {
            return Err(EventStoreError::InvalidState(
                "connector attachment execution authority changed".to_string(),
            ));
        };
        let (_, workspace_identity) =
            connector_attachment_workspace_binding(Path::new(&workspace_root))
                .map_err(EventStoreError::InvalidState)?;
        if workspace_identity != stored_workspace_identity {
            return Err(EventStoreError::InvalidState(
                "connector attachment workspace identity changed".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn load_connector_attachment_execution(
        &self,
        landing_id: Uuid,
    ) -> EventStoreResult<ConnectorAttachmentExecution> {
        let (account_json, metadata_json, workspace_root, stored_workspace_identity) =
            self.conn.query_row(
                r#"SELECT account.account_json, source.metadata_json,
                          landing.workspace_root, landing.workspace_identity
                   FROM connector_attachment_landings AS landing
                   JOIN connector_attachment_active_sources AS source
                     ON source.landing_id = landing.id
                   JOIN connector_accounts AS account ON account.id = landing.account_id
                   JOIN connector_account_generations AS generation
                     ON generation.account_id = account.id
                   WHERE landing.id = ?1
                     AND landing.status = 'reserved'
                     AND account.health = ?2
                     AND generation.generation = landing.account_generation"#,
                params![
                    landing_id.to_string(),
                    serde_json::to_string(&ConnectorHealth::Connected)?,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )?;
        let workspace_root = std::path::PathBuf::from(workspace_root);
        let (_, workspace_identity) = connector_attachment_workspace_binding(&workspace_root)
            .map_err(EventStoreError::InvalidState)?;
        if workspace_identity != stored_workspace_identity {
            return Err(EventStoreError::InvalidState(
                "connector attachment workspace identity changed".to_string(),
            ));
        }
        Ok(ConnectorAttachmentExecution {
            account: serde_json::from_str(&account_json)?,
            metadata: serde_json::from_str(&metadata_json)?,
            workspace_root,
            workspace_identity,
        })
    }

    pub(crate) fn mark_connector_attachment_staging(
        &self,
        landing_id: Uuid,
        storage_identity: &str,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if storage_identity.trim().is_empty() || storage_identity.len() > 64 {
            return Err(EventStoreError::InvalidState(
                "connector attachment storage identity is invalid".to_string(),
            ));
        }
        let changed = self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'staging', storage_identity = ?2, updated_at = ?3
               WHERE id = ?1 AND status = 'reserved' AND EXISTS (
                 SELECT 1 FROM connector_accounts AS account
                 JOIN connector_account_generations AS generation
                   ON generation.account_id = account.id
                 WHERE account.id = connector_attachment_landings.account_id
                   AND account.health = ?4
                   AND generation.generation = connector_attachment_landings.account_generation
               )"#,
            params![
                landing_id.to_string(),
                storage_identity,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(EventStoreError::InvalidState(
                "connector attachment account changed before staging".to_string(),
            ))
        }
    }

    pub(crate) fn mark_connector_attachment_ready(
        &self,
        staged: &StagedConnectorAttachment,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let receipt = staged.receipt();
        let transaction = self.conn.unchecked_transaction()?;
        let changed = transaction.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'ready', receipt_json = ?2, updated_at = ?3
               WHERE id = ?1 AND status = 'staging' AND storage_identity = ?4 AND EXISTS (
                 SELECT 1 FROM connector_accounts AS account
                 JOIN connector_account_generations AS generation
                   ON generation.account_id = account.id
                 WHERE account.id = connector_attachment_landings.account_id
                   AND account.health = ?5
                   AND generation.generation = connector_attachment_landings.account_generation
               )"#,
            params![
                receipt.landing_id.to_string(),
                serde_json::to_string(receipt)?,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                receipt.storage_identity,
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
        )?;
        if changed == 1 {
            transaction.execute(
                "DELETE FROM connector_attachment_active_sources WHERE landing_id = ?1",
                params![receipt.landing_id.to_string()],
            )?;
            transaction.commit()?;
            Ok(())
        } else {
            Err(EventStoreError::InvalidState(
                "connector attachment account changed before file commit".to_string(),
            ))
        }
    }

    pub(crate) fn claim_startup_connector_attachment_cleanup_candidates(
        &self,
        changed_at: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<ConnectorAttachmentCleanupCandidate>> {
        self.claim_connector_attachment_cleanup_candidates(changed_at, limit, true)
    }

    pub(crate) fn claim_runtime_connector_attachment_cleanup_candidates(
        &self,
        changed_at: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<ConnectorAttachmentCleanupCandidate>> {
        self.claim_connector_attachment_cleanup_candidates(changed_at, limit, false)
    }

    fn claim_connector_attachment_cleanup_candidates(
        &self,
        changed_at: DateTime<Utc>,
        limit: usize,
        include_abandoned_executions: bool,
    ) -> EventStoreResult<Vec<ConnectorAttachmentCleanupCandidate>> {
        let limit = limit.clamp(1, 64);
        let claim_id = Uuid::new_v4().to_string();
        let claim_expires_at = (changed_at
            + Duration::seconds(CONNECTOR_ATTACHMENT_RECOVERY_LEASE_SECONDS))
        .to_rfc3339_opts(SecondsFormat::Nanos, true);
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'cleanup_required', failure_kind = 'startup_recovery',
                   attempt_count = attempt_count + 1, next_cleanup_at = NULL,
                   cleanup_claim_id = ?3, cleanup_claim_expires_at = ?5,
                   updated_at = ?1
               WHERE id IN (
                 SELECT id FROM connector_attachment_landings
                 WHERE ((?4 = 1 AND status IN ('reserved', 'staging')) OR (
                         status = 'cleanup_required'
                         AND (next_cleanup_at IS NULL OR next_cleanup_at <= ?1)
                         AND (cleanup_claim_id IS NULL OR cleanup_claim_expires_at <= ?1)
                       ))
                   AND workspace_root IS NOT NULL AND workspace_identity IS NOT NULL
                 ORDER BY attempt_count ASC, updated_at ASC, id ASC LIMIT ?2
               )"#,
            params![
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                limit as i64,
                claim_id,
                include_abandoned_executions,
                claim_expires_at,
            ],
        )?;
        let mut statement = transaction.prepare(
            r#"SELECT id, cleanup_claim_id, metadata_json, workspace_root, workspace_identity,
                      storage_identity, receipt_json
               FROM connector_attachment_landings
               WHERE status = 'cleanup_required' AND cleanup_claim_id = ?1
               ORDER BY id ASC LIMIT ?2"#,
        )?;
        let rows = statement
            .query_map(params![claim_id, limit as i64,], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        transaction.commit()?;
        let candidates = rows
            .into_iter()
            .map(
                |(
                    id,
                    claim_id,
                    metadata,
                    workspace_root,
                    workspace_identity,
                    storage_identity,
                    receipt_json,
                )| {
                    let receipt = receipt_json
                        .map(|json| {
                            serde_json::from_str::<ConnectorAttachmentLandingReceipt>(&json)
                        })
                        .transpose()?;
                    Ok(ConnectorAttachmentCleanupCandidate {
                        landing_id: Uuid::parse_str(&id)?,
                        claim_id: Uuid::parse_str(&claim_id)?,
                        metadata: serde_json::from_str(&metadata)?,
                        workspace_root: workspace_root.into(),
                        workspace_identity,
                        storage_identity,
                        receipt,
                    })
                },
            )
            .collect::<EventStoreResult<Vec<_>>>()?;
        let mut terminalized = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            if self
                .terminalize_connector_attachment_cleanup_tool(candidate.landing_id, changed_at)
                .is_ok()
            {
                terminalized.push(candidate);
            } else {
                let _ = self.quarantine_connector_attachment_recovery_projection(
                    candidate.landing_id,
                    candidate.claim_id,
                    changed_at,
                );
            }
        }
        Ok(terminalized)
    }

    pub(crate) fn reset_stale_connector_attachment_recovery_claims(
        &self,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET cleanup_claim_id = NULL,
                   cleanup_claim_expires_at = NULL,
                   next_cleanup_at = CASE
                     WHEN status IN ('ready', 'cleanup_required', 'retention_cleanup')
                       THEN COALESCE(next_cleanup_at, ?1)
                     ELSE next_cleanup_at
                   END
               WHERE cleanup_claim_id IS NOT NULL"#,
            params![changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true)],
        )?;
        Ok(())
    }

    pub(crate) fn renew_connector_attachment_recovery_claim(
        &self,
        landing_id: Uuid,
        claim_id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let changed_at_text = changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let claim_expires_at = (changed_at
            + Duration::seconds(CONNECTOR_ATTACHMENT_RECOVERY_LEASE_SECONDS))
        .to_rfc3339_opts(SecondsFormat::Nanos, true);
        let changed = self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET cleanup_claim_expires_at = ?3
               WHERE id = ?1 AND cleanup_claim_id = ?2
                 AND status IN ('ready', 'cleanup_required', 'retention_cleanup')
                 AND cleanup_claim_expires_at > ?4"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                claim_expires_at,
                changed_at_text,
            ],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(EventStoreError::InvalidState(
                "connector attachment recovery lease changed".to_string(),
            ))
        }
    }

    #[cfg(windows)]
    pub(crate) fn claim_startup_ready_connector_attachment_recovery_candidates(
        &self,
        changed_at: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<ConnectorAttachmentCleanupCandidate>> {
        self.claim_ready_connector_attachment_recovery_candidates(changed_at, limit, true)
    }

    #[cfg(windows)]
    pub(crate) fn claim_runtime_ready_connector_attachment_recovery_candidates(
        &self,
        changed_at: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<ConnectorAttachmentCleanupCandidate>> {
        self.claim_ready_connector_attachment_recovery_candidates(changed_at, limit, false)
    }

    #[cfg(windows)]
    fn claim_ready_connector_attachment_recovery_candidates(
        &self,
        changed_at: DateTime<Utc>,
        limit: usize,
        include_abandoned_executions: bool,
    ) -> EventStoreResult<Vec<ConnectorAttachmentCleanupCandidate>> {
        let limit = limit.clamp(1, 64);
        let claim_id = Uuid::new_v4().to_string();
        let claim_expires_at = (changed_at
            + Duration::seconds(CONNECTOR_ATTACHMENT_RECOVERY_LEASE_SECONDS))
        .to_rfc3339_opts(SecondsFormat::Nanos, true);
        let changed_at_text = changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute(
            r#"UPDATE connector_attachment_landings
               SET cleanup_claim_id = ?3, cleanup_claim_expires_at = ?4,
                   next_cleanup_at = NULL, updated_at = ?1
               WHERE id IN (
                 SELECT id FROM connector_attachment_landings
                 WHERE status = 'ready' AND workspace_root IS NOT NULL
                   AND workspace_identity IS NOT NULL AND receipt_json IS NOT NULL
                   AND (cleanup_claim_id IS NULL OR cleanup_claim_expires_at <= ?1)
                   AND ((?5 = 1
                         AND (next_cleanup_at IS NULL OR next_cleanup_at <= ?1))
                        OR (?5 = 0 AND (
                          (cleanup_claim_id IS NULL
                           AND next_cleanup_at IS NOT NULL AND next_cleanup_at <= ?1)
                          OR (cleanup_claim_id IS NOT NULL
                              AND cleanup_claim_expires_at <= ?1)
                        )))
                 ORDER BY attempt_count ASC, COALESCE(next_cleanup_at, updated_at) ASC, id ASC
                 LIMIT ?2
               )"#,
            params![
                changed_at_text,
                limit as i64,
                claim_id,
                claim_expires_at,
                include_abandoned_executions,
            ],
        )?;
        let mut statement = transaction.prepare(
            r#"SELECT id, cleanup_claim_id, metadata_json, workspace_root, workspace_identity,
                      storage_identity, receipt_json
               FROM connector_attachment_landings
               WHERE status = 'ready' AND cleanup_claim_id = ?1
               ORDER BY id ASC
               LIMIT ?2"#,
        )?;
        let rows = statement
            .query_map(params![claim_id, limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        transaction.commit()?;
        rows.into_iter()
            .map(
                |(
                    id,
                    claim_id,
                    metadata,
                    workspace_root,
                    workspace_identity,
                    storage_identity,
                    receipt_json,
                )| {
                    Ok(ConnectorAttachmentCleanupCandidate {
                        landing_id: Uuid::parse_str(&id)?,
                        claim_id: Uuid::parse_str(&claim_id)?,
                        metadata: serde_json::from_str(&metadata)?,
                        workspace_root: workspace_root.into(),
                        workspace_identity,
                        storage_identity,
                        receipt: Some(serde_json::from_str(&receipt_json)?),
                    })
                },
            )
            .collect()
    }

    #[cfg(windows)]
    pub(crate) fn defer_connector_attachment_ready_recovery(
        &self,
        landing_id: Uuid,
        claim_id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let changed_at_text = changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let attempt_count: u32 = self.conn.query_row(
            r#"SELECT attempt_count FROM connector_attachment_landings
               WHERE id = ?1 AND status = 'ready' AND cleanup_claim_id = ?2
                 AND cleanup_claim_expires_at > ?3"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                changed_at_text,
            ],
            |row| row.get(0),
        )?;
        let exponent = attempt_count.min(9);
        let delay_seconds = 5i64.saturating_mul(1i64 << exponent).min(3600);
        let changed = self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET attempt_count = attempt_count + 1, next_cleanup_at = ?3,
                   cleanup_claim_id = NULL, cleanup_claim_expires_at = NULL,
                   updated_at = ?4
               WHERE id = ?1 AND status = 'ready' AND cleanup_claim_id = ?2
                 AND cleanup_claim_expires_at > ?4"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                (changed_at + Duration::seconds(delay_seconds))
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                changed_at_text,
            ],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(EventStoreError::InvalidState(
                "connector attachment ready recovery state changed".to_string(),
            ))
        }
    }

    #[cfg(windows)]
    pub(crate) fn claim_expired_connector_attachment_retention_candidates(
        &self,
        changed_at: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<ConnectorAttachmentCleanupCandidate>> {
        let limit = limit.clamp(1, 64);
        let changed_at_text = changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let claim_id = Uuid::new_v4().to_string();
        let claim_expires_at = (changed_at
            + Duration::seconds(CONNECTOR_ATTACHMENT_RECOVERY_LEASE_SECONDS))
        .to_rfc3339_opts(SecondsFormat::Nanos, true);
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'retention_cleanup', attempt_count = attempt_count + 1,
                   next_cleanup_at = NULL, cleanup_claim_id = ?3,
                   cleanup_claim_expires_at = ?4, updated_at = ?1
               WHERE id IN (
                 SELECT id FROM connector_attachment_landings
                 WHERE ((status = 'completed' AND expires_at IS NOT NULL AND expires_at <= ?1)
                        OR (status = 'retention_cleanup'
                            AND (next_cleanup_at IS NULL OR next_cleanup_at <= ?1)
                            AND (cleanup_claim_id IS NULL OR cleanup_claim_expires_at <= ?1)))
                   AND receipt_json IS NOT NULL AND workspace_root IS NOT NULL
                   AND workspace_identity IS NOT NULL
                 ORDER BY COALESCE(next_cleanup_at, expires_at) ASC, id ASC LIMIT ?2
               )"#,
            params![changed_at_text, limit as i64, claim_id, claim_expires_at],
        )?;
        let mut statement = transaction.prepare(
            r#"SELECT id, cleanup_claim_id, metadata_json, workspace_root, workspace_identity,
                      storage_identity, receipt_json
               FROM connector_attachment_landings
               WHERE status = 'retention_cleanup' AND cleanup_claim_id = ?1
               ORDER BY id ASC LIMIT ?2"#,
        )?;
        let rows = statement
            .query_map(params![claim_id, limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        transaction.commit()?;
        rows.into_iter()
            .map(
                |(
                    id,
                    claim_id,
                    metadata,
                    workspace_root,
                    workspace_identity,
                    storage_identity,
                    receipt_json,
                )| {
                    Ok(ConnectorAttachmentCleanupCandidate {
                        landing_id: Uuid::parse_str(&id)?,
                        claim_id: Uuid::parse_str(&claim_id)?,
                        metadata: serde_json::from_str(&metadata)?,
                        workspace_root: workspace_root.into(),
                        workspace_identity,
                        storage_identity,
                        receipt: Some(serde_json::from_str(&receipt_json)?),
                    })
                },
            )
            .collect()
    }

    pub(crate) fn defer_connector_attachment_cleanup(
        &self,
        landing_id: Uuid,
        claim_id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let changed_at_text = changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let attempt_count: u32 = self.conn.query_row(
            r#"SELECT attempt_count FROM connector_attachment_landings
               WHERE id = ?1 AND cleanup_claim_id = ?2
                 AND status IN ('cleanup_required', 'retention_cleanup')
                 AND cleanup_claim_expires_at > ?3"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                changed_at_text,
            ],
            |row| row.get(0),
        )?;
        let exponent = attempt_count.saturating_sub(1).min(9);
        let delay_seconds = 5i64.saturating_mul(1i64 << exponent).min(3600);
        let changed = self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET next_cleanup_at = ?3, cleanup_claim_id = NULL,
                   cleanup_claim_expires_at = NULL, updated_at = ?4
               WHERE id = ?1 AND cleanup_claim_id = ?2
                 AND status IN ('cleanup_required', 'retention_cleanup')
                 AND cleanup_claim_expires_at > ?4"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                (changed_at + Duration::seconds(delay_seconds))
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                changed_at_text,
            ],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(EventStoreError::InvalidState(
                "connector attachment cleanup state changed".to_string(),
            ))
        }
    }

    pub(crate) fn complete_connector_attachment_retention(
        &self,
        landing_id: Uuid,
        claim_id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let changed = self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'expired', next_cleanup_at = NULL,
                   cleanup_claim_id = NULL, cleanup_claim_expires_at = NULL,
                   updated_at = ?3
               WHERE id = ?1 AND status = 'retention_cleanup' AND cleanup_claim_id = ?2
                 AND cleanup_claim_expires_at > ?3"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(EventStoreError::InvalidState(
                "connector attachment retention completion raced".to_string(),
            ))
        }
    }

    pub(crate) fn mark_connector_attachment_retention_repair_required(
        &self,
        landing_id: Uuid,
        claim_id: Uuid,
        failure_kind: &str,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let changed = self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'repair_required', failure_kind = ?3,
                   next_cleanup_at = NULL, cleanup_claim_id = NULL,
                   cleanup_claim_expires_at = NULL,
                   recovery_revision = recovery_revision + 1, updated_at = ?4
               WHERE id = ?1 AND status = 'retention_cleanup' AND cleanup_claim_id = ?2
                 AND cleanup_claim_expires_at > ?4"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                failure_kind,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(EventStoreError::InvalidState(
                "connector attachment retention repair raced".to_string(),
            ))
        }
    }

    pub(crate) fn claim_connector_attachment_cleanup(
        &self,
        landing_id: Uuid,
        failure_kind: &str,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAttachmentCleanupClaim> {
        let claim_id = Uuid::new_v4();
        let changed_at_text = changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let claim_expires_at = (changed_at
            + Duration::seconds(CONNECTOR_ATTACHMENT_RECOVERY_LEASE_SECONDS))
        .to_rfc3339_opts(SecondsFormat::Nanos, true);
        let changed = self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'cleanup_required', failure_kind = ?2,
                   cleanup_claim_id = ?4, cleanup_claim_expires_at = ?5,
                   next_cleanup_at = NULL, updated_at = ?3
               WHERE id = ?1 AND (
                 (status IN ('reserved', 'staging', 'ready')
                     AND (cleanup_claim_id IS NULL OR cleanup_claim_expires_at <= ?3))
                 OR (status = 'cleanup_required'
                     AND (cleanup_claim_id IS NULL OR cleanup_claim_expires_at <= ?3))
               )"#,
            params![
                landing_id.to_string(),
                failure_kind,
                changed_at_text,
                claim_id.to_string(),
                claim_expires_at,
            ],
        )?;
        if changed == 1 {
            self.terminalize_connector_attachment_cleanup_tool(landing_id, changed_at)?;
            return Ok(ConnectorAttachmentCleanupClaim::Owned(claim_id));
        }
        let status: String = self.conn.query_row(
            "SELECT status FROM connector_attachment_landings WHERE id = ?1",
            params![landing_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(if status == "cleanup_required" {
            ConnectorAttachmentCleanupClaim::Busy
        } else {
            ConnectorAttachmentCleanupClaim::KeepFile
        })
    }

    pub(crate) fn transition_ready_recovery_to_cleanup(
        &self,
        landing_id: Uuid,
        claim_id: Uuid,
        failure_kind: &str,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let changed = self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'cleanup_required', failure_kind = ?3,
                   next_cleanup_at = NULL, updated_at = ?4
               WHERE id = ?1 AND status = 'ready' AND cleanup_claim_id = ?2
                 AND cleanup_claim_expires_at > ?4"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                failure_kind,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector attachment ready recovery claim changed".to_string(),
            ));
        }
        self.terminalize_connector_attachment_cleanup_tool(landing_id, changed_at)
    }

    fn terminalize_connector_attachment_cleanup_tool(
        &self,
        landing_id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let tool_invocation_id: String = self.conn.query_row(
            r#"SELECT tool_invocation_id FROM connector_attachment_landings
               WHERE id = ?1 AND status IN ('cleanup_required', 'repair_required')"#,
            params![landing_id.to_string()],
            |row| row.get(0),
        )?;
        let mut tool = self.tool_invocation_by_id(Uuid::parse_str(&tool_invocation_id)?)?;
        if matches!(
            tool.status,
            ToolExecutionStatus::Succeeded
                | ToolExecutionStatus::Failed
                | ToolExecutionStatus::Blocked
        ) {
            return Ok(());
        }
        tool.status = ToolExecutionStatus::Failed;
        tool.output = None;
        tool.evidence.clear();
        tool.verification = ToolVerificationResult::failed(
            "connector attachment execution ended before durable file completion",
        );
        tool.error = Some("connector attachment download did not complete".to_string());
        tool.finished_at = Some(changed_at);
        self.append_tool_invocation(&tool)
    }

    fn quarantine_connector_attachment_recovery_projection(
        &self,
        landing_id: Uuid,
        claim_id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let changed = self.conn.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'repair_required', failure_kind = 'recovery_projection_unavailable',
                   next_cleanup_at = NULL, cleanup_claim_id = NULL,
                   cleanup_claim_expires_at = NULL,
                   recovery_revision = recovery_revision + 1, updated_at = ?3
               WHERE id = ?1 AND status = 'cleanup_required' AND cleanup_claim_id = ?2
                 AND cleanup_claim_expires_at > ?3"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(EventStoreError::InvalidState(
                "connector attachment recovery quarantine raced".to_string(),
            ))
        }
    }

    pub(crate) fn connector_attachment_cleanup_candidate(
        &self,
        landing_id: Uuid,
        claim_id: Uuid,
    ) -> EventStoreResult<ConnectorAttachmentCleanupCandidate> {
        let (metadata, workspace_root, workspace_identity, storage_identity, receipt_json) =
            self.conn.query_row(
                r#"SELECT metadata_json, workspace_root, workspace_identity,
                      storage_identity, receipt_json
               FROM connector_attachment_landings
               WHERE id = ?1 AND status = 'cleanup_required' AND cleanup_claim_id = ?2"#,
                params![landing_id.to_string(), claim_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )?;
        Ok(ConnectorAttachmentCleanupCandidate {
            landing_id,
            claim_id,
            metadata: serde_json::from_str(&metadata)?,
            workspace_root: workspace_root.into(),
            workspace_identity,
            storage_identity,
            receipt: receipt_json
                .map(|json| serde_json::from_str::<ConnectorAttachmentLandingReceipt>(&json))
                .transpose()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn connector_attachment_status(&self, landing_id: Uuid) -> EventStoreResult<String> {
        Ok(self.conn.query_row(
            "SELECT status FROM connector_attachment_landings WHERE id = ?1",
            params![landing_id.to_string()],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn mark_connector_attachment_repair_required(
        &self,
        landing_id: Uuid,
        claim_id: Uuid,
        failure_kind: &str,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let tool_invocation_id: String = self.conn.query_row(
            r#"SELECT tool_invocation_id FROM connector_attachment_landings
               WHERE id = ?1 AND status = 'cleanup_required' AND cleanup_claim_id = ?2"#,
            params![landing_id.to_string(), claim_id.to_string()],
            |row| row.get(0),
        )?;
        let mut tool = self.tool_invocation_by_id(Uuid::parse_str(&tool_invocation_id)?)?;
        tool.status = ToolExecutionStatus::Failed;
        tool.output = None;
        tool.evidence.clear();
        tool.verification =
            ToolVerificationResult::failed("attachment landing requires manual workspace repair");
        tool.error = Some("connector attachment workspace identity changed".to_string());
        tool.finished_at = Some(changed_at);
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool)?;
        let transaction = self.conn.unchecked_transaction()?;
        let changed = transaction.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'repair_required', failure_kind = ?3,
                   cleanup_claim_id = NULL, cleanup_claim_expires_at = NULL,
                   recovery_revision = recovery_revision + 1, updated_at = ?4
               WHERE id = ?1 AND status = 'cleanup_required' AND cleanup_claim_id = ?2
                 AND cleanup_claim_expires_at > ?4"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                failure_kind,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector attachment repair transition raced".to_string(),
            ));
        }
        transaction.execute(
            "DELETE FROM connector_attachment_active_sources WHERE landing_id = ?1",
            params![landing_id.to_string()],
        )?;
        Self::insert_kernel_event(&transaction, &tool_event)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn fail_connector_attachment_after_cleanup(
        &self,
        landing_id: Uuid,
        claim_id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let tool_invocation_id: String = self.conn.query_row(
            r#"SELECT tool_invocation_id FROM connector_attachment_landings
               WHERE id = ?1 AND status = 'cleanup_required' AND cleanup_claim_id = ?2"#,
            params![landing_id.to_string(), claim_id.to_string()],
            |row| row.get(0),
        )?;
        let tool_invocation_id = Uuid::parse_str(&tool_invocation_id)?;
        let mut tool = self.tool_invocation_by_id(tool_invocation_id)?;
        tool.status = ToolExecutionStatus::Failed;
        tool.output = None;
        tool.evidence.clear();
        tool.verification =
            ToolVerificationResult::failed("incomplete attachment landing was removed");
        tool.error = Some("connector attachment landing was interrupted".to_string());
        tool.finished_at = Some(changed_at);
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool)?;
        let transaction = self.conn.unchecked_transaction()?;
        let changed = transaction.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'failed', receipt_json = NULL,
                   cleanup_claim_id = NULL, cleanup_claim_expires_at = NULL,
                   updated_at = ?3
               WHERE id = ?1 AND status = 'cleanup_required' AND cleanup_claim_id = ?2
                 AND cleanup_claim_expires_at > ?3"#,
            params![
                landing_id.to_string(),
                claim_id.to_string(),
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector attachment cleanup completion raced".to_string(),
            ));
        }
        transaction.execute(
            "DELETE FROM connector_attachment_active_sources WHERE landing_id = ?1",
            params![landing_id.to_string()],
        )?;
        Self::insert_kernel_event(&transaction, &tool_event)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn start_approved_connector_invocation(
        &self,
        id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorInvocation> {
        let mut invocation = self.connector_invocation(id)?;
        if invocation.status != ConnectorInvocationStatus::PendingApproval {
            return Err(EventStoreError::InvalidState(
                "connector mutation is not waiting for approval".to_string(),
            ));
        }
        let account = self
            .list_connector_accounts()?
            .into_iter()
            .find(|account| account.id == invocation.account_id)
            .ok_or_else(|| EventStoreError::NotFound("connector account".to_string()))?;
        if account.health != ConnectorHealth::Connected
            || account.provider_id != invocation.provider_id
            || !account
                .granted_capabilities
                .contains(&invocation.capability)
        {
            return Err(EventStoreError::InvalidState(
                "connector account is not ready for this mutation".to_string(),
            ));
        }
        let account_generation = invocation.account_generation.ok_or_else(|| {
            EventStoreError::InvalidState(
                "legacy connector mutation has no frozen account generation".to_string(),
            )
        })?;
        if self.connector_account_sync_generation(&account)? != account_generation {
            return Err(EventStoreError::InvalidState(
                "connector account changed after the exact mutation preview".to_string(),
            ));
        }
        let account_generation = i64::try_from(account_generation).map_err(|_| {
            EventStoreError::InvalidState(
                "connector mutation account generation is too large".to_string(),
            )
        })?;
        let tool_invocation_id = invocation.tool_invocation_id.ok_or_else(|| {
            EventStoreError::InvalidState(
                "connector mutation is missing its exact tool invocation".to_string(),
            )
        })?;
        let mut tool_record = self
            .list_tool_invocations()?
            .into_iter()
            .find(|record| record.id == tool_invocation_id)
            .ok_or_else(|| EventStoreError::NotFound("connector tool invocation".to_string()))?;
        bind_connector_invocation_to_tool_record(&invocation, &tool_record)
            .map_err(EventStoreError::InvalidState)?;
        let approval_request_id = tool_record.approval_request_id.ok_or_else(|| {
            EventStoreError::InvalidState("connector approval request is missing".to_string())
        })?;
        let approved = self
            .list_capability_access_records()?
            .into_iter()
            .any(|record| {
                record.request.id == approval_request_id
                    && record.request.capability == CapabilityKind::ConnectorWrite
                    && record.effective_status == CapabilityAccessStatus::Approved
                    && record.grant_state == CapabilityGrantState::OneShotAvailable
            });
        if !approved {
            return Err(EventStoreError::InvalidState(
                "exact connector approval is not available".to_string(),
            ));
        }
        if let Some(automation_run_id) = invocation.automation_run_id {
            let linked_review = self.list_review_queue_items()?.into_iter().any(|item| {
                item.automation_run_id == automation_run_id
                    && item.tool_invocation_id == Some(tool_invocation_id)
                    && item.status
                        == crate::kernel::automation::ReviewQueueItemStatus::PendingApproval
                    && item.preview_fingerprint.as_deref()
                        == Some(invocation.request_fingerprint.as_str())
            });
            if !linked_review {
                return Err(EventStoreError::InvalidState(
                    "automation connector mutation is not bound to its frozen review".to_string(),
                ));
            }
        }

        tool_record.status = ToolExecutionStatus::Running;
        tool_record.verification =
            ToolVerificationResult::failed("connector mutation is in progress");
        tool_record.error = None;
        tool_record.finished_at = None;
        invocation.status = ConnectorInvocationStatus::Running;
        invocation.updated_at = changed_at;
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool_record)?;
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute(
            r#"INSERT INTO connector_approval_consumptions
               (request_id, connector_invocation_id, consumed_at)
               VALUES (?1, ?2, ?3)"#,
            params![
                approval_request_id.to_string(),
                invocation.id.to_string(),
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        let updated = transaction.execute(
            r#"UPDATE connector_invocations
               SET invocation_json = ?2, status = ?3, updated_at = ?4
               WHERE id = ?1 AND status = ?5 AND account_generation = ?6
                 AND EXISTS (
                   SELECT 1
                   FROM connector_accounts AS account
                   JOIN connector_account_generations AS generation
                     ON generation.account_id = account.id
                   WHERE account.id = connector_invocations.account_id
                     AND account.health = ?7
                     AND account.provider_id = ?8
                     AND generation.generation = ?6
                 )"#,
            params![
                invocation.id.to_string(),
                serde_json::to_string(&invocation)?,
                serde_json::to_string(&invocation.status)?,
                invocation
                    .updated_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorInvocationStatus::PendingApproval)?,
                account_generation,
                serde_json::to_string(&ConnectorHealth::Connected)?,
                invocation.provider_id,
            ],
        )?;
        if updated != 1 {
            return Err(EventStoreError::InvalidState(
                "connector mutation was already started".to_string(),
            ));
        }
        Self::insert_kernel_event(&transaction, &tool_event)?;
        transaction.commit()?;
        Ok(invocation)
    }

    pub(crate) fn mark_connector_invocation_reconciliation_required(
        &self,
        id: Uuid,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorInvocation> {
        let mut invocation = self.connector_invocation(id)?;
        if invocation.status != ConnectorInvocationStatus::Running
            || !invocation.capability.external_mutation()
        {
            return Err(EventStoreError::InvalidState(
                "only a running external mutation can require reconciliation".to_string(),
            ));
        }
        let account_generation = invocation.account_generation.ok_or_else(|| {
            EventStoreError::InvalidState(
                "legacy connector mutation has no frozen account generation".to_string(),
            )
        })?;
        let account_generation = i64::try_from(account_generation).map_err(|_| {
            EventStoreError::InvalidState(
                "connector mutation account generation is too large".to_string(),
            )
        })?;
        let tool_invocation_id = invocation.tool_invocation_id.ok_or_else(|| {
            EventStoreError::InvalidState("connector mutation Tool is missing".to_string())
        })?;
        let mut tool = self.tool_invocation_by_id(tool_invocation_id)?;
        bind_running_connector_invocation_to_tool_record(&invocation, &tool)
            .map_err(EventStoreError::InvalidState)?;
        tool.verification = ToolVerificationResult::failed(
            "external result is uncertain; read-only reconciliation is pending",
        );
        tool.error = None;
        tool.finished_at = None;
        invocation.status = ConnectorInvocationStatus::ReconciliationRequired;
        invocation.updated_at = changed_at;
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool)?;
        let transaction = self.conn.unchecked_transaction()?;
        let updated = transaction.execute(
            r#"UPDATE connector_invocations
               SET invocation_json = ?2, status = ?3, updated_at = ?4,
                   next_reconciliation_at = ?4,
                   reconciliation_claim_id = NULL,
                   reconciliation_claim_expires_at = NULL,
                   reconciliation_attempt_count = 0,
                   recovery_revision = recovery_revision + 1
               WHERE id = ?1 AND status = ?5 AND account_generation = ?6"#,
            params![
                invocation.id.to_string(),
                serde_json::to_string(&invocation)?,
                serde_json::to_string(&invocation.status)?,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorInvocationStatus::Running)?,
                account_generation,
            ],
        )?;
        if updated != 1 {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation transition raced with another writer".to_string(),
            ));
        }
        Self::insert_kernel_event(&transaction, &tool_event)?;
        transaction.commit()?;
        Ok(invocation)
    }

    pub(crate) fn claim_due_connector_reconciliations(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<ConnectorReconciliationClaim>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        const MAX_RECONCILIATION_CANDIDATES_PER_SWEEP: usize = 64;
        let candidate_budget = MAX_RECONCILIATION_CANDIDATES_PER_SWEEP;
        let scan_limit = i64::try_from(candidate_budget).map_err(|_| {
            EventStoreError::InvalidState(
                "connector reconciliation claim limit is too large".to_string(),
            )
        })?;
        let mut remaining_scan_budget = candidate_budget;
        let now_text = now.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let mut claims = Vec::new();
        while remaining_scan_budget > 0 {
            let mut statement = self.conn.prepare(
                r#"SELECT id FROM connector_invocations
                   WHERE status = ?1 AND reconciliation_quarantine_code IS NULL
                     AND account_generation IS NOT NULL
                     AND next_reconciliation_at IS NOT NULL
                     AND next_reconciliation_at <= ?2
                     AND (
                       reconciliation_claim_id IS NULL
                       OR reconciliation_claim_expires_at IS NULL
                       OR reconciliation_claim_expires_at <= ?2
                     )
                   ORDER BY next_reconciliation_at ASC,
                            reconciliation_attempt_count ASC, rowid ASC
                   LIMIT ?3"#,
            )?;
            let candidate_ids = statement
                .query_map(
                    params![
                        serde_json::to_string(&ConnectorInvocationStatus::ReconciliationRequired)?,
                        now_text,
                        scan_limit,
                    ],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            drop(statement);
            if candidate_ids.is_empty() {
                break;
            }
            let mut progressed = false;
            for raw_id in candidate_ids {
                if remaining_scan_budget == 0 {
                    break;
                }
                remaining_scan_budget -= 1;
                let candidate_id = match Uuid::parse_str(&raw_id) {
                    Ok(value) => value,
                    Err(_) => {
                        progressed |= self.quarantine_due_connector_reconciliation(&raw_id, now)?;
                        continue;
                    }
                };
                match self.claim_connector_reconciliation(candidate_id, now) {
                    Ok(Some(claim)) => {
                        progressed = true;
                        claims.push(claim);
                        if claims.len() == limit {
                            return Ok(claims);
                        }
                    }
                    Ok(None) => {}
                    Err(EventStoreError::InvalidState(_))
                    | Err(EventStoreError::NotFound(_))
                    | Err(EventStoreError::Json(_))
                    | Err(EventStoreError::Uuid(_))
                    | Err(EventStoreError::Timestamp(_)) => {
                        progressed |= self.quarantine_due_connector_reconciliation(&raw_id, now)?;
                    }
                    Err(error @ EventStoreError::Sqlite(_)) => return Err(error),
                }
            }
            if !progressed {
                break;
            }
        }
        Ok(claims)
    }

    fn quarantine_due_connector_reconciliation(
        &self,
        id: &str,
        now: DateTime<Utc>,
    ) -> EventStoreResult<bool> {
        let changed = self.conn.execute(
            r#"UPDATE connector_invocations
               SET reconciliation_quarantine_code = 'invalid_projection_binding',
                   reconciliation_quarantined_at = ?3
               WHERE id = ?1 AND status = ?2
                 AND reconciliation_quarantine_code IS NULL
                 AND next_reconciliation_at IS NOT NULL
                 AND next_reconciliation_at <= ?3
                 AND (
                   reconciliation_claim_id IS NULL
                   OR reconciliation_claim_expires_at IS NULL
                   OR reconciliation_claim_expires_at <= ?3
                 )"#,
            params![
                id,
                serde_json::to_string(&ConnectorInvocationStatus::ReconciliationRequired)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        Ok(changed == 1)
    }

    pub(crate) fn reset_abandoned_connector_reconciliation_claims(
        &self,
        now: DateTime<Utc>,
    ) -> EventStoreResult<usize> {
        Ok(self.conn.execute(
            r#"UPDATE connector_invocations
               SET reconciliation_claim_id = NULL,
                   reconciliation_claim_expires_at = NULL,
                   next_reconciliation_at = COALESCE(next_reconciliation_at, ?2)
               WHERE status = ?1 AND reconciliation_claim_id IS NOT NULL"#,
            params![
                serde_json::to_string(&ConnectorInvocationStatus::ReconciliationRequired)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?)
    }

    fn claim_connector_reconciliation(
        &self,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<Option<ConnectorReconciliationClaim>> {
        let transaction = self.conn.unchecked_transaction()?;
        let row = transaction
            .query_row(
                r#"SELECT invocation_json, account_generation,
                          reconciliation_claim_id, reconciliation_claim_expires_at,
                          next_reconciliation_at, reconciliation_attempt_count
                   FROM connector_invocations WHERE id = ?1 AND status = ?2"#,
                params![
                    id.to_string(),
                    serde_json::to_string(&ConnectorInvocationStatus::ReconciliationRequired)?,
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((
            invocation_json,
            projected_generation,
            existing_claim_id,
            existing_claim_expiry,
            next_reconciliation_at,
            attempt_count,
        )) = row
        else {
            return Ok(None);
        };
        let due = next_reconciliation_at
            .as_deref()
            .map(DateTime::parse_from_rfc3339)
            .transpose()?
            .map(|value| value.with_timezone(&Utc) <= now)
            .unwrap_or(false);
        let claim_available = existing_claim_id.is_none()
            || existing_claim_expiry
                .as_deref()
                .map(DateTime::parse_from_rfc3339)
                .transpose()?
                .map(|value| value.with_timezone(&Utc) <= now)
                .unwrap_or(true);
        if !due || !claim_available {
            return Ok(None);
        }
        let invocation: ConnectorInvocation = serde_json::from_str(&invocation_json)?;
        let (account, _) =
            load_connector_reconciliation_binding(&transaction, &invocation, projected_generation)?;
        let claim_id = Uuid::new_v4();
        let claim_expires_at = now + Duration::seconds(CONNECTOR_RECONCILIATION_LEASE_SECONDS);
        let updated = transaction.execute(
            r#"UPDATE connector_invocations
               SET reconciliation_claim_id = ?2,
                   reconciliation_claim_expires_at = ?3
               WHERE id = ?1 AND status = ?4 AND account_generation = ?5
                 AND next_reconciliation_at IS NOT NULL
                 AND next_reconciliation_at <= ?6
                 AND (
                   reconciliation_claim_id IS NULL
                   OR reconciliation_claim_expires_at IS NULL
                   OR reconciliation_claim_expires_at <= ?6
                 )"#,
            params![
                id.to_string(),
                claim_id.to_string(),
                claim_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorInvocationStatus::ReconciliationRequired)?,
                projected_generation,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if updated != 1 {
            return Ok(None);
        }
        transaction.commit()?;
        Ok(Some(ConnectorReconciliationClaim {
            claim_id,
            invocation,
            account,
            attempt_count: u32::try_from(attempt_count).map_err(|_| {
                EventStoreError::InvalidState(
                    "connector reconciliation attempt count is invalid".to_string(),
                )
            })?,
            claim_expires_at,
        }))
    }

    pub(crate) fn renew_connector_reconciliation_claim(
        &self,
        claim: &mut ConnectorReconciliationClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let next_expiry = now + Duration::seconds(CONNECTOR_RECONCILIATION_LEASE_SECONDS);
        let generation = i64::try_from(claim.invocation.account_generation.ok_or_else(|| {
            EventStoreError::InvalidState("legacy reconciliation cannot renew a claim".to_string())
        })?)
        .map_err(|_| {
            EventStoreError::InvalidState(
                "connector reconciliation account generation is too large".to_string(),
            )
        })?;
        let transaction = self.conn.unchecked_transaction()?;
        let (current_json, projected_generation) = transaction
            .query_row(
                r#"SELECT invocation_json, account_generation
                   FROM connector_invocations
                   WHERE id = ?1 AND status = ?2
                     AND reconciliation_claim_id = ?3
                     AND reconciliation_claim_expires_at > ?4"#,
                params![
                    claim.invocation.id.to_string(),
                    serde_json::to_string(&ConnectorInvocationStatus::ReconciliationRequired)?,
                    claim.claim_id.to_string(),
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector reconciliation claim is no longer live".to_string(),
                )
            })?;
        let current: ConnectorInvocation = serde_json::from_str(&current_json)?;
        if current != claim.invocation || projected_generation != generation {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation invocation changed after claim".to_string(),
            ));
        }
        load_connector_reconciliation_binding(&transaction, &current, projected_generation)?;
        let changed = transaction.execute(
            r#"UPDATE connector_invocations
               SET reconciliation_claim_expires_at = ?3
               WHERE id = ?1 AND status = ?4
                 AND reconciliation_claim_id = ?2
                 AND reconciliation_claim_expires_at > ?5
                 AND account_generation = ?6
                 AND EXISTS (
                   SELECT 1 FROM connector_accounts AS account
                   JOIN connector_account_generations AS current
                     ON current.account_id = account.id
                   WHERE account.id = connector_invocations.account_id
                     AND account.health = ?7
                     AND current.generation = ?6
                 )"#,
            params![
                claim.invocation.id.to_string(),
                claim.claim_id.to_string(),
                next_expiry.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorInvocationStatus::ReconciliationRequired)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                generation,
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation claim could not be renewed".to_string(),
            ));
        }
        transaction.commit()?;
        claim.claim_expires_at = next_expiry;
        Ok(())
    }

    pub(crate) fn defer_connector_reconciliation(
        &self,
        claim: &ConnectorReconciliationClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<DateTime<Utc>> {
        let exponent = claim.attempt_count.min(6);
        let backoff_seconds =
            (30_i64 << exponent).min(CONNECTOR_RECONCILIATION_MAX_BACKOFF_SECONDS);
        let next_reconciliation_at = now + Duration::seconds(backoff_seconds);
        let changed = self.conn.execute(
            r#"UPDATE connector_invocations
               SET reconciliation_claim_id = NULL,
                   reconciliation_claim_expires_at = NULL,
                   next_reconciliation_at = ?3,
                   reconciliation_attempt_count = reconciliation_attempt_count + 1
               WHERE id = ?1 AND status = ?4
                 AND reconciliation_claim_id = ?2
                 AND reconciliation_claim_expires_at > ?5"#,
            params![
                claim.invocation.id.to_string(),
                claim.claim_id.to_string(),
                next_reconciliation_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorInvocationStatus::ReconciliationRequired)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation defer lost its claim".to_string(),
            ));
        }
        Ok(next_reconciliation_at)
    }

    pub fn complete_connector_invocation(
        &self,
        id: Uuid,
        receipt: ConnectorMutationReceipt,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorInvocation> {
        self.complete_connector_invocation_internal(id, receipt, changed_at, None)
    }

    pub(crate) fn complete_claimed_connector_reconciliation(
        &self,
        claim: &ConnectorReconciliationClaim,
        receipt: ConnectorMutationReceipt,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorInvocation> {
        if claim.invocation.account_id != receipt.account_id {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation receipt is bound to another account".to_string(),
            ));
        }
        self.complete_connector_invocation_internal(
            claim.invocation.id,
            receipt,
            changed_at,
            Some(claim),
        )
    }

    pub(crate) fn fail_claimed_connector_reconciliation_known_not_applied(
        &self,
        claim: &ConnectorReconciliationClaim,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorInvocation> {
        if claim.claim_expires_at <= changed_at {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation claim expired before completion".to_string(),
            ));
        }
        let mut invocation = self.connector_invocation(claim.invocation.id)?;
        if invocation != claim.invocation
            || invocation.status != ConnectorInvocationStatus::ReconciliationRequired
        {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation changed after the read-only query".to_string(),
            ));
        }
        let account_generation = i64::try_from(invocation.account_generation.ok_or_else(|| {
            EventStoreError::InvalidState(
                "legacy connector mutation has no frozen account generation".to_string(),
            )
        })?)
        .map_err(|_| {
            EventStoreError::InvalidState(
                "connector reconciliation account generation is too large".to_string(),
            )
        })?;
        let tool_invocation_id = invocation.tool_invocation_id.ok_or_else(|| {
            EventStoreError::InvalidState("connector tool invocation is missing".to_string())
        })?;
        let mut tool = self.tool_invocation_by_id(tool_invocation_id)?;
        if tool.status != ToolExecutionStatus::Running
            || tool.request_fingerprint != invocation.request_fingerprint
        {
            return Err(EventStoreError::InvalidState(
                "connector tool audit changed before reconciliation completed".to_string(),
            ));
        }
        tool.status = ToolExecutionStatus::Failed;
        tool.output = Some(serde_json::json!({ "outcome": "known_not_applied" }));
        tool.evidence.clear();
        tool.verification = ToolVerificationResult::passed(
            "provider read-only reconciliation confirmed no external mutation",
        );
        tool.error = None;
        tool.finished_at = Some(changed_at);
        let capability_invocation = CapabilityInvocation {
            id: tool.id,
            capability: CapabilityKind::ConnectorWrite,
            status: CapabilityInvocationStatus::Failed,
            policy_decision: tool.policy_decision,
            approval_request_id: tool.approval_request_id,
            requested_resource: None,
            evidence_ref: None,
            requested_url: None,
            evidence_url: None,
            title: Some("Connected account change was not applied".to_string()),
            excerpt: None,
            warnings: Vec::new(),
            elapsed_ms: tool.elapsed_ms,
            created_at: changed_at,
        };
        let mut review_item = match invocation.automation_run_id {
            Some(automation_run_id) => self.list_review_queue_items()?.into_iter().find(|item| {
                item.automation_run_id == automation_run_id
                    && item.tool_invocation_id == Some(tool_invocation_id)
            }),
            None => None,
        };
        let review_previous_revision = review_item.as_ref().map(|item| item.revision);
        if let Some(item) = review_item.as_mut() {
            item.resolve(false, changed_at)
                .map_err(EventStoreError::InvalidState)?;
        }
        invocation.status = ConnectorInvocationStatus::Failed;
        invocation.evidence.clear();
        invocation.updated_at = changed_at;
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool)?;
        let capability_event =
            KernelEvent::new(CAPABILITY_INVOCATION_RECORDED_EVENT, &capability_invocation)?;
        let transaction = self.conn.unchecked_transaction()?;
        load_connector_reconciliation_binding(&transaction, &claim.invocation, account_generation)?;
        let updated = transaction.execute(
            r#"UPDATE connector_invocations
               SET invocation_json = ?2, status = ?3, updated_at = ?4,
                   reconciliation_claim_id = NULL,
                   reconciliation_claim_expires_at = NULL,
                   next_reconciliation_at = NULL
               WHERE id = ?1 AND status = ?5 AND account_generation = ?6
                 AND reconciliation_claim_id = ?7
                 AND reconciliation_claim_expires_at > ?4"#,
            params![
                invocation.id.to_string(),
                serde_json::to_string(&invocation)?,
                serde_json::to_string(&invocation.status)?,
                changed_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorInvocationStatus::ReconciliationRequired)?,
                account_generation,
                claim.claim_id.to_string(),
            ],
        )?;
        if updated != 1 {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation completion lost its claim".to_string(),
            ));
        }
        for event in [tool_event, capability_event] {
            Self::insert_kernel_event(&transaction, &event)?;
        }
        if let Some(item) = review_item {
            let changed = transaction.execute(
                r#"UPDATE review_queue_items
                   SET item_json = ?2, status = ?3, revision = ?4, updated_at = ?5
                   WHERE id = ?1 AND revision = ?6"#,
                params![
                    item.id.to_string(),
                    serde_json::to_string(&item)?,
                    serde_json::to_string(&item.status)?,
                    item.revision,
                    item.updated_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    review_previous_revision,
                ],
            )?;
            if changed != 1 {
                return Err(EventStoreError::InvalidState(
                    "review reconciliation raced with another writer".to_string(),
                ));
            }
        }
        transaction.commit()?;
        Ok(invocation)
    }

    fn complete_connector_invocation_internal(
        &self,
        id: Uuid,
        receipt: ConnectorMutationReceipt,
        changed_at: DateTime<Utc>,
        claim: Option<&ConnectorReconciliationClaim>,
    ) -> EventStoreResult<ConnectorInvocation> {
        let mut invocation = self.connector_invocation(id)?;
        let mutation = invocation.mutation.as_ref().ok_or_else(|| {
            EventStoreError::InvalidState("connector mutation envelope is missing".to_string())
        })?;
        let receipt_matches = receipt.provider_id == mutation.provider_id
            && receipt.account_id == mutation.account_id
            && mutation.account_generation == invocation.account_generation
            && receipt.capability == mutation.capability
            && receipt.target_ref == mutation.target_ref
            && receipt.request_fingerprint == invocation.request_fingerprint
            && receipt.idempotency_key == mutation.idempotency_key
            && receipt.evidence.provider_id == mutation.provider_id
            && receipt.evidence.account_id == mutation.account_id
            && !receipt.evidence.remote_object_ref.trim().is_empty();
        if !receipt_matches {
            return Err(EventStoreError::InvalidState(
                "connector receipt does not match the frozen mutation".to_string(),
            ));
        }
        if invocation.status == ConnectorInvocationStatus::Succeeded && claim.is_some() {
            return Err(EventStoreError::InvalidState(
                "connector reconciliation claim cannot replay a completed invocation".to_string(),
            ));
        }
        if invocation.status == ConnectorInvocationStatus::Succeeded {
            if invocation.evidence.as_slice() == [receipt.evidence.clone()] {
                return Ok(invocation);
            }
            return Err(EventStoreError::InvalidState(
                "connector completion replay has different evidence".to_string(),
            ));
        }
        let account_generation = invocation.account_generation.ok_or_else(|| {
            EventStoreError::InvalidState(
                "legacy connector mutation has no frozen account generation".to_string(),
            )
        })?;
        let account_generation = i64::try_from(account_generation).map_err(|_| {
            EventStoreError::InvalidState(
                "connector mutation account generation is too large".to_string(),
            )
        })?;
        if !matches!(
            invocation.status,
            ConnectorInvocationStatus::Running | ConnectorInvocationStatus::ReconciliationRequired
        ) {
            return Err(EventStoreError::InvalidState(
                "connector mutation is not ready to complete".to_string(),
            ));
        }
        if invocation.status == ConnectorInvocationStatus::Running && claim.is_some() {
            return Err(EventStoreError::InvalidState(
                "a reconciliation claim cannot complete a running mutation".to_string(),
            ));
        }
        if invocation.status == ConnectorInvocationStatus::ReconciliationRequired && claim.is_none()
        {
            return Err(EventStoreError::InvalidState(
                "uncertain connector mutation requires a fenced reconciliation claim".to_string(),
            ));
        }
        if invocation.status == ConnectorInvocationStatus::ReconciliationRequired
            && !receipt.reconciled
        {
            return Err(EventStoreError::InvalidState(
                "uncertain connector mutation requires reconciled provider evidence".to_string(),
            ));
        }
        let tool_invocation_id = invocation.tool_invocation_id.ok_or_else(|| {
            EventStoreError::InvalidState("connector tool invocation is missing".to_string())
        })?;
        let mut tool_record = self
            .list_tool_invocations()?
            .into_iter()
            .find(|record| record.id == tool_invocation_id)
            .ok_or_else(|| EventStoreError::NotFound("connector tool invocation".to_string()))?;
        if tool_record.status != ToolExecutionStatus::Running
            || tool_record.request_fingerprint != invocation.request_fingerprint
        {
            return Err(EventStoreError::InvalidState(
                "connector tool audit is not running for this exact request".to_string(),
            ));
        }
        let evidence = receipt.evidence.clone();
        tool_record.status = ToolExecutionStatus::Succeeded;
        tool_record.output = Some(serde_json::json!({
            "remote_object_ref": evidence.remote_object_ref,
            "outcome": "applied",
        }));
        tool_record.evidence = vec![ToolEvidence {
            kind: "connector_remote_state".to_string(),
            reference: evidence.remote_object_ref.clone(),
            summary: evidence
                .bounded_summary
                .clone()
                .unwrap_or_else(|| "Provider confirmed the remote mutation.".to_string()),
        }];
        tool_record.verification =
            ToolVerificationResult::passed("provider remote state reconciled");
        tool_record.error = None;
        tool_record.finished_at = Some(changed_at);
        let capability_invocation = CapabilityInvocation {
            id: tool_record.id,
            capability: CapabilityKind::ConnectorWrite,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: tool_record.policy_decision,
            approval_request_id: tool_record.approval_request_id,
            requested_resource: Some(format!(
                "{}:{}:{}",
                invocation.provider_id, invocation.account_id, invocation.idempotency_key
            )),
            evidence_ref: Some(evidence.remote_object_ref.clone()),
            requested_url: None,
            evidence_url: None,
            title: Some("Connected account change".to_string()),
            excerpt: evidence.bounded_summary.clone(),
            warnings: Vec::new(),
            elapsed_ms: tool_record.elapsed_ms,
            created_at: changed_at,
        };
        let mut review_item = match invocation.automation_run_id {
            Some(automation_run_id) => self.list_review_queue_items()?.into_iter().find(|item| {
                item.automation_run_id == automation_run_id
                    && item.tool_invocation_id == Some(tool_invocation_id)
            }),
            None => None,
        };
        let review_previous_revision = review_item.as_ref().map(|item| item.revision);
        if let Some(item) = review_item.as_mut() {
            item.complete_approved_action(evidence.remote_object_ref.clone(), changed_at)
                .map_err(EventStoreError::InvalidState)?;
        }
        let previous_status = invocation.status;
        let reconciliation_binding = claim.map(|_| invocation.clone());
        invocation.status = ConnectorInvocationStatus::Succeeded;
        invocation.evidence = vec![evidence];
        invocation.updated_at = changed_at;
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool_record)?;
        let capability_event =
            KernelEvent::new(CAPABILITY_INVOCATION_RECORDED_EVENT, &capability_invocation)?;
        let transaction = self.conn.unchecked_transaction()?;
        if let Some(binding) = reconciliation_binding.as_ref() {
            load_connector_reconciliation_binding(&transaction, binding, account_generation)?;
        }
        let updated = if let Some(claim) = claim {
            if claim.invocation.id != invocation.id
                || claim.invocation.account_generation != invocation.account_generation
                || claim.claim_expires_at <= changed_at
            {
                return Err(EventStoreError::InvalidState(
                    "connector reconciliation claim is stale or mismatched".to_string(),
                ));
            }
            transaction.execute(
                r#"UPDATE connector_invocations
                   SET invocation_json = ?2, status = ?3, updated_at = ?4,
                       reconciliation_claim_id = NULL,
                       reconciliation_claim_expires_at = NULL,
                       next_reconciliation_at = NULL
                   WHERE id = ?1 AND status = ?5 AND account_generation = ?6
                     AND reconciliation_claim_id = ?9
                     AND reconciliation_claim_expires_at > ?4
                     AND EXISTS (
                       SELECT 1
                       FROM connector_accounts AS account
                       JOIN connector_account_generations AS generation
                         ON generation.account_id = account.id
                       WHERE account.id = connector_invocations.account_id
                         AND account.health = ?7
                         AND account.provider_id = ?8
                         AND generation.generation = ?6
                     )"#,
                params![
                    invocation.id.to_string(),
                    serde_json::to_string(&invocation)?,
                    serde_json::to_string(&invocation.status)?,
                    invocation
                        .updated_at
                        .to_rfc3339_opts(SecondsFormat::Nanos, true),
                    serde_json::to_string(&previous_status)?,
                    account_generation,
                    serde_json::to_string(&ConnectorHealth::Connected)?,
                    invocation.provider_id,
                    claim.claim_id.to_string(),
                ],
            )?
        } else {
            transaction.execute(
                r#"UPDATE connector_invocations
                   SET invocation_json = ?2, status = ?3, updated_at = ?4
                   WHERE id = ?1 AND status = ?5 AND account_generation = ?6
                     AND EXISTS (
                       SELECT 1
                       FROM connector_accounts AS account
                       JOIN connector_account_generations AS generation
                         ON generation.account_id = account.id
                       WHERE account.id = connector_invocations.account_id
                         AND account.health = ?7
                         AND account.provider_id = ?8
                         AND generation.generation = ?6
                     )"#,
                params![
                    invocation.id.to_string(),
                    serde_json::to_string(&invocation)?,
                    serde_json::to_string(&invocation.status)?,
                    invocation
                        .updated_at
                        .to_rfc3339_opts(SecondsFormat::Nanos, true),
                    serde_json::to_string(&previous_status)?,
                    account_generation,
                    serde_json::to_string(&ConnectorHealth::Connected)?,
                    invocation.provider_id,
                ],
            )?
        };
        if updated != 1 {
            return Err(EventStoreError::InvalidState(
                "connector mutation completion raced with another worker".to_string(),
            ));
        }
        for event in [tool_event, capability_event] {
            Self::insert_kernel_event(&transaction, &event)?;
        }
        if let Some(item) = review_item {
            let changed = transaction.execute(
                r#"UPDATE review_queue_items
                   SET item_json = ?2, status = ?3, revision = ?4, updated_at = ?5
                   WHERE id = ?1 AND revision = ?6"#,
                params![
                    item.id.to_string(),
                    serde_json::to_string(&item)?,
                    serde_json::to_string(&item.status)?,
                    item.revision,
                    item.updated_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    review_previous_revision,
                ],
            )?;
            if changed != 1 {
                return Err(EventStoreError::InvalidState(
                    "review completion raced with another writer".to_string(),
                ));
            }
        }
        transaction.commit()?;
        Ok(invocation)
    }

    pub fn transition_connector_invocation(
        &self,
        id: Uuid,
        status: ConnectorInvocationStatus,
        evidence: Vec<ConnectorEvidenceRef>,
        changed_at: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorInvocation> {
        let mut invocation = self.connector_invocation(id)?;
        if invocation.capability.external_mutation()
            && matches!(
                status,
                ConnectorInvocationStatus::Running
                    | ConnectorInvocationStatus::Succeeded
                    | ConnectorInvocationStatus::ReconciliationRequired
            )
        {
            return Err(EventStoreError::InvalidState(
                "external connector mutation must use its approval and evidence boundary"
                    .to_string(),
            ));
        }
        if !connector_invocation_transition_allowed(invocation.status, status) {
            return Err(EventStoreError::InvalidState(format!(
                "connector invocation cannot transition from {:?} to {:?}",
                invocation.status, status
            )));
        }
        if status == ConnectorInvocationStatus::Succeeded && evidence.is_empty() {
            return Err(EventStoreError::InvalidState(
                "successful connector invocation requires evidence".to_string(),
            ));
        }
        invocation.status = status;
        invocation.evidence = evidence;
        invocation.updated_at = changed_at;
        self.conn.execute(
            r#"UPDATE connector_invocations
               SET invocation_json = ?2, status = ?3, updated_at = ?4
               WHERE id = ?1"#,
            params![
                invocation.id.to_string(),
                serde_json::to_string(&invocation)?,
                serde_json::to_string(&invocation.status)?,
                invocation
                    .updated_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        Ok(invocation)
    }

    pub fn upsert_connector_authorization_session(
        &self,
        session: &ConnectorAuthorizationSession,
    ) -> EventStoreResult<()> {
        self.conn.execute(
            r#"INSERT INTO connector_authorization_sessions
               (id, session_json, expires_at, consumed_at, status, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)
               ON CONFLICT(id) DO UPDATE SET
                 session_json = excluded.session_json,
                 expires_at = excluded.expires_at,
                 consumed_at = excluded.consumed_at,
                 status = excluded.status,
                 updated_at = excluded.updated_at
               WHERE connector_authorization_sessions.status = ?7"#,
            params![
                session.id.to_string(),
                serde_json::to_string(session)?,
                session
                    .expires_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                session
                    .consumed_at
                    .map(|value| value.to_rfc3339_opts(SecondsFormat::Nanos, true)),
                serde_json::to_string(&session.status)?,
                Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorAuthorizationStatus::Pending)?,
            ],
        )?;
        Ok(())
    }

    pub(crate) fn insert_preparing_connector_authorization(
        &self,
        session: &ConnectorAuthorizationSession,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if session.status != ConnectorAuthorizationStatus::Preparing
            || session.revision != 0
            || session.consumed_at.is_some()
            || session.cleanup_required
            || session.cleanup_completed_at.is_some()
            || session.expires_at <= now
        {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization preparation is invalid".to_string(),
            ));
        }
        let inserted = self.conn.execute(
            r#"INSERT INTO connector_authorization_sessions
               (id, session_json, expires_at, consumed_at, status, revision,
                cleanup_required, cleanup_completed_at, account_id, updated_at)
               VALUES (?1, ?2, ?3, NULL, ?4, 0, 0, NULL, NULL, ?5)"#,
            params![
                session.id.to_string(),
                serde_json::to_string(session)?,
                session
                    .expires_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&session.status)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if inserted != 1 {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization preparation raced".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn activate_preparing_connector_authorization(
        &self,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationSession> {
        let mut session = self.connector_authorization_session(id)?;
        if session.status != ConnectorAuthorizationStatus::Preparing
            || session.revision != 0
            || session.expires_at <= now
            || session.cleanup_required
        {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization preparation is stale".to_string(),
            ));
        }
        let previous_json = serde_json::to_string(&session)?;
        session.status = ConnectorAuthorizationStatus::Pending;
        session.revision = 1;
        let changed = self.conn.execute(
            r#"UPDATE connector_authorization_sessions
               SET session_json = ?2, status = ?3, revision = 1, updated_at = ?4
               WHERE id = ?1 AND session_json = ?5 AND status = ?6
                 AND revision = 0 AND cleanup_required = 0
                 AND consumed_at IS NULL AND expires_at > ?4"#,
            params![
                id.to_string(),
                serde_json::to_string(&session)?,
                serde_json::to_string(&ConnectorAuthorizationStatus::Pending)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                previous_json,
                serde_json::to_string(&ConnectorAuthorizationStatus::Preparing)?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization preparation raced".to_string(),
            ));
        }
        Ok(session)
    }

    pub fn connector_authorization_session(
        &self,
        id: Uuid,
    ) -> EventStoreResult<ConnectorAuthorizationSession> {
        let (
            json,
            expires_at,
            consumed_at,
            status,
            revision,
            cleanup_required,
            cleanup_completed_at,
        ) = self.conn.query_row(
            r#"SELECT session_json, expires_at, consumed_at, status, revision,
                      cleanup_required, cleanup_completed_at
               FROM connector_authorization_sessions WHERE id = ?1"#,
            params![id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )?;
        let session: ConnectorAuthorizationSession = serde_json::from_str(&json)?;
        let projected_expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        let projected_consumed_at = consumed_at
            .map(|value| {
                DateTime::parse_from_rfc3339(&value).map(|value| value.with_timezone(&Utc))
            })
            .transpose()?;
        let projected_status: ConnectorAuthorizationStatus = serde_json::from_str(&status)?;
        let projected_revision = u64::try_from(revision).map_err(|_| {
            EventStoreError::InvalidState("OAuth authorization revision is invalid".to_string())
        })?;
        let projected_cleanup_completed_at = cleanup_completed_at
            .map(|value| {
                DateTime::parse_from_rfc3339(&value).map(|value| value.with_timezone(&Utc))
            })
            .transpose()?;
        if session.id != id
            || session.expires_at != projected_expires_at
            || session.consumed_at != projected_consumed_at
            || session.status != projected_status
            || session.revision != projected_revision
            || session.cleanup_required != (cleanup_required != 0)
            || session.cleanup_completed_at != projected_cleanup_completed_at
        {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization session projection is invalid".to_string(),
            ));
        }
        Ok(session)
    }

    pub(crate) fn prepare_connector_authorization_review(
        &self,
        authorization_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationActionProvision> {
        let transaction = self.conn.unchecked_transaction()?;
        let (session_json, expires_at, consumed_at, status) = transaction.query_row(
            r#"SELECT session_json, expires_at, consumed_at, status
               FROM connector_authorization_sessions WHERE id = ?1"#,
            params![authorization_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        let session: ConnectorAuthorizationSession = serde_json::from_str(&session_json)?;
        let projected_expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        let projected_status: ConnectorAuthorizationStatus = serde_json::from_str(&status)?;
        if session.id != authorization_id
            || session.status != ConnectorAuthorizationStatus::Pending
            || projected_status != ConnectorAuthorizationStatus::Pending
            || session.consumed_at.is_some()
            || consumed_at.is_some()
            || session.expires_at != projected_expires_at
            || session.expires_at <= now
        {
            return Err(EventStoreError::InvalidState(
                "connector authorization review is unavailable".to_string(),
            ));
        }
        let review_id = Uuid::new_v4();
        let authority_handle = ConnectorCredentialHandle::new();
        let authority = ConnectorSecret::new(Uuid::new_v4().to_string())
            .map_err(EventStoreError::InvalidState)?;
        transaction.execute(
            r#"INSERT INTO connector_authorization_actions
               (authorization_id, token_hash, session_hash, expires_at, created_at,
                review_id, authority_handle_json, action_status, activated_at, resolved_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'preparing', NULL, NULL)"#,
            params![
                authorization_id.to_string(),
                connector_authorization_action_token_hash(authority.expose()),
                connector_authorization_session_hash(&session_json),
                expires_at,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                review_id.to_string(),
                serde_json::to_string(&authority_handle)?,
            ],
        )?;
        transaction.commit()?;
        Ok(ConnectorAuthorizationActionProvision {
            review_id,
            authorization_id,
            authority_handle,
            authority,
        })
    }

    pub(crate) fn activate_connector_authorization_review(
        &self,
        review_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationActiveReview> {
        let transaction = self.conn.unchecked_transaction()?;
        let (authorization_id, authority_handle_json, session_hash, expires_at, action_status) =
            transaction.query_row(
                r#"SELECT authorization_id, authority_handle_json, session_hash, expires_at,
                          action_status
                   FROM connector_authorization_actions WHERE review_id = ?1"#,
                params![review_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )?;
        let authorization_id = Uuid::parse_str(&authorization_id)?;
        let authority_handle: ConnectorCredentialHandle =
            serde_json::from_str(authority_handle_json.as_deref().ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector authorization review authority is unavailable".to_string(),
                )
            })?)?;
        let action_expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        let session_json = transaction.query_row(
            r#"SELECT session_json FROM connector_authorization_sessions
               WHERE id = ?1 AND status = ?2 AND consumed_at IS NULL AND expires_at > ?3"#,
            params![
                authorization_id.to_string(),
                serde_json::to_string(&ConnectorAuthorizationStatus::Pending)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
            |row| row.get::<_, String>(0),
        )?;
        if action_status.as_deref() != Some("preparing")
            || action_expires_at <= now
            || !constant_time_text_eq(
                &session_hash,
                &connector_authorization_session_hash(&session_json),
            )
        {
            return Err(EventStoreError::InvalidState(
                "connector authorization review is stale".to_string(),
            ));
        }
        let changed = transaction.execute(
            r#"UPDATE connector_authorization_actions
               SET action_status = 'active', activated_at = ?2
               WHERE review_id = ?1 AND action_status = 'preparing'
                 AND resolved_at IS NULL AND expires_at > ?2"#,
            params![
                review_id.to_string(),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true)
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector authorization review activation raced".to_string(),
            ));
        }
        transaction.commit()?;
        Ok(ConnectorAuthorizationActiveReview {
            review_id,
            authorization_id,
            authority_handle,
        })
    }

    pub(crate) fn connector_authorization_active_review(
        &self,
        review_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationActiveReview> {
        let (authorization_id, authority_handle_json, session_hash, expires_at) =
            self.conn.query_row(
                r#"SELECT authorization_id, authority_handle_json, session_hash, expires_at
                   FROM connector_authorization_actions
                   WHERE review_id = ?1 AND action_status = 'active'
                     AND activated_at IS NOT NULL AND resolved_at IS NULL"#,
                params![review_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )?;
        let authorization_id = Uuid::parse_str(&authorization_id)?;
        let action_expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        let session = self.connector_authorization_session(authorization_id)?;
        let session_json = serde_json::to_string(&session)?;
        if session.status != ConnectorAuthorizationStatus::Pending
            || session.consumed_at.is_some()
            || session.expires_at <= now
            || action_expires_at != session.expires_at
            || action_expires_at <= now
            || !constant_time_text_eq(
                &session_hash,
                &connector_authorization_session_hash(&session_json),
            )
        {
            return Err(EventStoreError::InvalidState(
                "connector authorization review is stale".to_string(),
            ));
        }
        let authority_handle =
            serde_json::from_str(authority_handle_json.as_deref().ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector authorization review authority is unavailable".to_string(),
                )
            })?)?;
        Ok(ConnectorAuthorizationActiveReview {
            review_id,
            authorization_id,
            authority_handle,
        })
    }

    pub(crate) fn validate_connector_authorization_active_review_authority(
        &self,
        review_id: Uuid,
        authority: &ConnectorSecret,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        self.connector_authorization_active_review(review_id, now)?;
        let token_hash = self.conn.query_row(
            r#"SELECT token_hash FROM connector_authorization_actions
               WHERE review_id = ?1 AND action_status = 'active'"#,
            params![review_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        if !constant_time_text_eq(
            &token_hash,
            &connector_authorization_action_token_hash(authority.expose()),
        ) {
            return Err(EventStoreError::InvalidState(
                "connector authorization review authority is invalid".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn connector_authorization_review_ids(
        &self,
        limit: usize,
    ) -> EventStoreResult<Vec<Uuid>> {
        let limit = i64::try_from(limit.min(32)).map_err(|_| {
            EventStoreError::InvalidState(
                "connector authorization review limit is invalid".to_string(),
            )
        })?;
        let mut statement = self.conn.prepare(
            r#"SELECT review_id FROM connector_authorization_actions
               WHERE review_id IS NOT NULL AND action_status != 'preparing'
               ORDER BY COALESCE(resolved_at, activated_at, created_at) DESC, rowid DESC
               LIMIT ?1"#,
        )?;
        let review_ids = statement
            .query_map(params![limit], |row| row.get::<_, String>(0))?
            .filter_map(|value| value.ok().and_then(|value| Uuid::parse_str(&value).ok()))
            .collect();
        Ok(review_ids)
    }

    pub(crate) fn connector_authorization_review_snapshot(
        &self,
        review_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationReviewSnapshot> {
        let (authorization_id, action_status, resolved_intent, authority_handle_json) =
            self.conn.query_row(
                r#"SELECT authorization_id, action_status, resolved_intent,
                          authority_handle_json
                   FROM connector_authorization_actions WHERE review_id = ?1"#,
                params![review_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )?;
        let intent_state = match (action_status.as_str(), resolved_intent.as_deref()) {
            ("active", None) => ConnectorAuthorizationReviewIntentState::Active,
            ("consumed" | "resolved", Some("approve")) => {
                ConnectorAuthorizationReviewIntentState::Approve
            }
            ("consumed" | "resolved", Some("cancel" | "expired")) => {
                ConnectorAuthorizationReviewIntentState::Cancel
            }
            _ => {
                return Err(EventStoreError::InvalidState(
                    "connector authorization review projection is invalid".to_string(),
                ))
            }
        };
        let authorization_id = Uuid::parse_str(&authorization_id)?;
        let session = self.connector_authorization_session(authorization_id)?;
        let (exchange_claim_id, exchange_claim_expires_at, account_id) = self.conn.query_row(
            r#"SELECT exchange_claim_id, exchange_claim_expires_at, account_id
               FROM connector_authorization_sessions WHERE id = ?1"#,
            params![authorization_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )?;
        let exchange_claim_live = match (exchange_claim_id, exchange_claim_expires_at) {
            (Some(claim_id), Some(expires_at)) => {
                Uuid::parse_str(&claim_id).is_ok()
                    && DateTime::parse_from_rfc3339(&expires_at)
                        .map(|value| value.with_timezone(&Utc) > now)
                        .unwrap_or(false)
            }
            (None, None) => false,
            _ => false,
        };
        let authority_handle = authority_handle_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        let account = account_id
            .as_deref()
            .and_then(|account_id| Uuid::parse_str(account_id).ok())
            .and_then(|account_id| {
                self.list_connector_accounts()
                    .ok()?
                    .into_iter()
                    .find(|account| account.id == account_id)
            });
        let account_generation = account.as_ref().and_then(|account| {
            self.conn
                .query_row(
                    "SELECT generation FROM connector_account_generations WHERE account_id = ?1",
                    params![account.id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .ok()
                .flatten()
        });
        let account_binding_valid = account.as_ref().is_some_and(|account| {
            session.status == ConnectorAuthorizationStatus::Completed
                && session.consumed_at.is_some()
                && !session.cleanup_required
                && account.provider_id == session.provider_id
                && account.credential_handle == session.result_credential_handle
                && account.granted_capabilities == session.requested_capabilities
                && account.health == ConnectorHealth::Connected
                && account_generation == Some(0)
        });
        Ok(ConnectorAuthorizationReviewSnapshot {
            review_id,
            session,
            intent_state,
            authority_handle,
            exchange_claim_live,
            account,
            account_binding_valid,
        })
    }

    pub(crate) fn connector_authorization_authority_cleanup_candidates(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<Uuid>> {
        let limit = i64::try_from(limit.min(64)).map_err(|_| {
            EventStoreError::InvalidState(
                "connector authorization authority cleanup limit is invalid".to_string(),
            )
        })?;
        let mut statement = self.conn.prepare(
            r#"SELECT review_id FROM connector_authorization_actions
               WHERE action_status = 'consumed' AND authority_cleanup_required = 1
                 AND review_id IS NOT NULL AND authority_handle_json IS NOT NULL
                 AND (authority_cleanup_claim_expires_at IS NULL
                      OR authority_cleanup_claim_expires_at <= ?1)
               ORDER BY resolved_at ASC, rowid ASC LIMIT ?2"#,
        )?;
        let candidates = statement
            .query_map(
                params![now.to_rfc3339_opts(SecondsFormat::Nanos, true), limit],
                |row| row.get::<_, String>(0),
            )?
            .map(|value| Ok(Uuid::parse_str(&value?)?))
            .collect();
        candidates
    }

    pub(crate) fn begin_connector_authorization_authority_cleanup(
        &self,
        review_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationAuthorityCleanupClaim> {
        let transaction = self.conn.unchecked_transaction()?;
        let (authorization_id, authority_handle_json, current_claim_expires_at) = transaction
            .query_row(
                r#"SELECT authorization_id, authority_handle_json,
                          authority_cleanup_claim_expires_at
                   FROM connector_authorization_actions
                   WHERE review_id = ?1 AND action_status = 'consumed'
                     AND authority_cleanup_required = 1 AND resolved_at IS NOT NULL"#,
                params![review_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )?;
        let current_claim_expires_at = current_claim_expires_at
            .as_deref()
            .map(|value| DateTime::parse_from_rfc3339(value).map(|value| value.with_timezone(&Utc)))
            .transpose()?;
        if current_claim_expires_at.is_some_and(|expires_at| expires_at > now) {
            return Err(EventStoreError::InvalidState(
                "connector authorization authority cleanup is already claimed".to_string(),
            ));
        }
        let authority_handle: ConnectorCredentialHandle =
            serde_json::from_str(authority_handle_json.as_deref().ok_or_else(|| {
                EventStoreError::InvalidState(
                    "connector authorization authority cleanup handle is unavailable".to_string(),
                )
            })?)?;
        let claim_id = Uuid::new_v4();
        let claim_expires_at = now + Duration::minutes(5);
        let changed = transaction.execute(
            r#"UPDATE connector_authorization_actions
               SET authority_cleanup_claim_id = ?2,
                   authority_cleanup_claim_expires_at = ?3
               WHERE review_id = ?1 AND action_status = 'consumed'
                 AND authority_cleanup_required = 1
                 AND (authority_cleanup_claim_expires_at IS NULL
                      OR authority_cleanup_claim_expires_at <= ?4)"#,
            params![
                review_id.to_string(),
                claim_id.to_string(),
                claim_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector authorization authority cleanup raced".to_string(),
            ));
        }
        transaction.commit()?;
        Ok(ConnectorAuthorizationAuthorityCleanupClaim {
            review_id,
            authorization_id: Uuid::parse_str(&authorization_id)?,
            authority_handle,
            claim_id,
            claim_expires_at,
        })
    }

    pub(crate) fn finish_connector_authorization_authority_cleanup(
        &self,
        claim: &ConnectorAuthorizationAuthorityCleanupClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let changed = self.conn.execute(
            r#"UPDATE connector_authorization_actions
               SET token_hash = 'redacted', session_hash = 'redacted',
                   authority_handle_json = NULL, action_status = 'resolved',
                   authority_cleanup_required = 0,
                   authority_cleanup_claim_id = NULL,
                   authority_cleanup_claim_expires_at = NULL,
                   authority_cleanup_completed_at = ?5
               WHERE review_id = ?1 AND authorization_id = ?2
                 AND action_status = 'consumed' AND authority_cleanup_required = 1
                 AND authority_cleanup_claim_id = ?3
                 AND authority_cleanup_claim_expires_at = ?4
                 AND authority_cleanup_claim_expires_at > ?5"#,
            params![
                claim.review_id.to_string(),
                claim.authorization_id.to_string(),
                claim.claim_id.to_string(),
                claim
                    .claim_expires_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector authorization authority cleanup raced".to_string(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn issue_connector_authorization_action(
        &self,
        authorization_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<String> {
        let transaction = self.conn.unchecked_transaction()?;
        let (session_json, expires_at, consumed_at, status) = transaction.query_row(
            r#"SELECT session_json, expires_at, consumed_at, status
               FROM connector_authorization_sessions WHERE id = ?1"#,
            params![authorization_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        let session: ConnectorAuthorizationSession = serde_json::from_str(&session_json)?;
        let projected_expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        let projected_status: ConnectorAuthorizationStatus = serde_json::from_str(&status)?;
        if session.id != authorization_id
            || session.status != ConnectorAuthorizationStatus::Pending
            || projected_status != ConnectorAuthorizationStatus::Pending
            || session.consumed_at.is_some()
            || consumed_at.is_some()
            || session.expires_at != projected_expires_at
            || session.expires_at <= now
        {
            return Err(EventStoreError::InvalidState(
                "connector authorization action is unavailable".to_string(),
            ));
        }
        let token = Uuid::new_v4().to_string();
        transaction.execute(
            r#"INSERT INTO connector_authorization_actions
               (authorization_id, token_hash, session_hash, expires_at, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5)"#,
            params![
                authorization_id.to_string(),
                connector_authorization_action_token_hash(&token),
                connector_authorization_session_hash(&session_json),
                expires_at,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        transaction.commit()?;
        Ok(token)
    }

    #[cfg(test)]
    pub(crate) fn claim_connector_authorization_action(
        &self,
        authorization_id: Uuid,
        action_token: &str,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationExchangeClaim> {
        match self.resolve_connector_authorization_action(
            authorization_id,
            action_token,
            ConnectorAuthorizationIntent::Approve,
            now,
        )? {
            ConnectorAuthorizationResolution::Approved(claim) => Ok(claim),
            ConnectorAuthorizationResolution::Cancelled(_) => Err(EventStoreError::InvalidState(
                "connector authorization action resolved unexpectedly".to_string(),
            )),
        }
    }

    #[cfg(test)]
    pub(crate) fn resolve_connector_authorization_action(
        &self,
        authorization_id: Uuid,
        action_token: &str,
        intent: ConnectorAuthorizationIntent,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationResolution> {
        self.resolve_connector_authorization_action_inner(
            authorization_id,
            None,
            action_token,
            intent,
            now,
        )
    }

    pub(crate) fn resolve_connector_authorization_review(
        &self,
        review_id: Uuid,
        authority: &ConnectorSecret,
        intent: ConnectorAuthorizationIntent,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationResolution> {
        let authorization_id = self.conn.query_row(
            r#"SELECT authorization_id FROM connector_authorization_actions
               WHERE review_id = ?1"#,
            params![review_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        self.resolve_connector_authorization_action_inner(
            Uuid::parse_str(&authorization_id)?,
            Some(review_id),
            authority.expose(),
            intent,
            now,
        )
    }

    fn resolve_connector_authorization_action_inner(
        &self,
        authorization_id: Uuid,
        expected_review_id: Option<Uuid>,
        action_token: &str,
        intent: ConnectorAuthorizationIntent,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationResolution> {
        let transaction = self.conn.unchecked_transaction()?;
        let (
            token_hash,
            session_hash,
            action_expires_at,
            stored_review_id,
            authority_handle_json,
            action_status,
            resolved_at,
        ) = transaction.query_row(
            r#"SELECT token_hash, session_hash, expires_at, review_id,
                      authority_handle_json, action_status, resolved_at
                   FROM connector_authorization_actions WHERE authorization_id = ?1"#,
            params![authorization_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )?;
        let stored_review_id = stored_review_id
            .as_deref()
            .map(Uuid::parse_str)
            .transpose()?;
        let action_authority_handle = authority_handle_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        let valid_authority_kind = match expected_review_id {
            Some(review_id) => {
                stored_review_id == Some(review_id)
                    && action_status.as_deref() == Some("active")
                    && action_authority_handle.is_some()
                    && resolved_at.is_none()
            }
            None => stored_review_id.is_none(),
        };
        if action_token.trim().is_empty()
            || !valid_authority_kind
            || !constant_time_text_eq(
                &token_hash,
                &connector_authorization_action_token_hash(action_token),
            )
            || DateTime::parse_from_rfc3339(&action_expires_at)?.with_timezone(&Utc) <= now
        {
            return Err(EventStoreError::InvalidState(
                "connector authorization action is invalid".to_string(),
            ));
        }
        let (session_json, expires_at, consumed_at, status) = transaction.query_row(
            r#"SELECT session_json, expires_at, consumed_at, status
               FROM connector_authorization_sessions WHERE id = ?1"#,
            params![authorization_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?;
        let mut session: ConnectorAuthorizationSession = serde_json::from_str(&session_json)?;
        let projected_expires_at = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        let projected_status: ConnectorAuthorizationStatus = serde_json::from_str(&status)?;
        if session.id != authorization_id
            || session.status != ConnectorAuthorizationStatus::Pending
            || projected_status != ConnectorAuthorizationStatus::Pending
            || session.consumed_at.is_some()
            || consumed_at.is_some()
            || session.expires_at != projected_expires_at
            || session.expires_at <= now
            || !constant_time_text_eq(
                &session_hash,
                &connector_authorization_session_hash(&session_json),
            )
        {
            return Err(EventStoreError::InvalidState(
                "connector authorization action is stale".to_string(),
            ));
        }
        let resolved_intent = match intent {
            ConnectorAuthorizationIntent::Approve => "approve",
            ConnectorAuthorizationIntent::Cancel => "cancel",
        };
        if intent == ConnectorAuthorizationIntent::Cancel {
            session.status = ConnectorAuthorizationStatus::Cancelled;
            session.cleanup_required = true;
            session.cleanup_completed_at = None;
            session.revision = session.revision.checked_add(1).ok_or_else(|| {
                EventStoreError::InvalidState("OAuth authorization revision overflowed".to_string())
            })?;
            let claim_id = Uuid::new_v4();
            let claim_expires_at = now + Duration::minutes(5);
            let changed = transaction.execute(
                r#"UPDATE connector_authorization_sessions
                   SET session_json = ?2, status = ?3, revision = ?7, updated_at = ?4,
                       cleanup_required = 1, cleanup_completed_at = NULL,
                       cleanup_claim_id = ?8, cleanup_claim_expires_at = ?9
                   WHERE id = ?1 AND session_json = ?5 AND status = ?6
                     AND consumed_at IS NULL AND expires_at > ?4"#,
                params![
                    authorization_id.to_string(),
                    serde_json::to_string(&session)?,
                    serde_json::to_string(&ConnectorAuthorizationStatus::Cancelled)?,
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    session_json,
                    serde_json::to_string(&ConnectorAuthorizationStatus::Pending)?,
                    i64::try_from(session.revision).map_err(|_| EventStoreError::InvalidState(
                        "OAuth authorization revision is too large".to_string()
                    ))?,
                    claim_id.to_string(),
                    claim_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )?;
            if changed != 1 {
                return Err(EventStoreError::InvalidState(
                    "connector authorization action raced".to_string(),
                ));
            }
            let consumed = if let Some(review_id) = expected_review_id {
                transaction.execute(
                    r#"UPDATE connector_authorization_actions
                       SET action_status = 'consumed', resolved_at = ?3,
                           resolved_intent = ?4, authority_cleanup_required = 1
                       WHERE authorization_id = ?1 AND review_id = ?2
                         AND token_hash = ?5 AND action_status = 'active'
                         AND resolved_at IS NULL"#,
                    params![
                        authorization_id.to_string(),
                        review_id.to_string(),
                        now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                        resolved_intent,
                        token_hash,
                    ],
                )?
            } else {
                transaction.execute(
                    r#"DELETE FROM connector_authorization_actions
                       WHERE authorization_id = ?1 AND token_hash = ?2
                         AND review_id IS NULL"#,
                    params![authorization_id.to_string(), token_hash],
                )?
            };
            if consumed != 1 {
                return Err(EventStoreError::InvalidState(
                    "connector authorization action raced".to_string(),
                ));
            }
            transaction.commit()?;
            return Ok(ConnectorAuthorizationResolution::Cancelled(
                ConnectorAuthorizationCleanupClaim {
                    session,
                    claim_id,
                    claim_expires_at,
                    action_authority_handle,
                },
            ));
        }
        session.status = ConnectorAuthorizationStatus::Exchanging;
        session.revision = session.revision.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState("OAuth authorization revision overflowed".to_string())
        })?;
        let claim_id = Uuid::new_v4();
        let claim_expires_at = now + Duration::minutes(5);
        let next_json = serde_json::to_string(&session)?;
        let changed = transaction.execute(
            r#"UPDATE connector_authorization_sessions
               SET session_json = ?2, status = ?3, revision = ?7, updated_at = ?4,
                   exchange_claim_id = ?8, exchange_claim_expires_at = ?9
               WHERE id = ?1 AND session_json = ?5 AND status = ?6
                 AND consumed_at IS NULL AND expires_at > ?4"#,
            params![
                authorization_id.to_string(),
                next_json,
                serde_json::to_string(&ConnectorAuthorizationStatus::Exchanging)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                session_json,
                serde_json::to_string(&ConnectorAuthorizationStatus::Pending)?,
                i64::try_from(session.revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
                claim_id.to_string(),
                claim_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector authorization action raced".to_string(),
            ));
        }
        let consumed = if let Some(review_id) = expected_review_id {
            transaction.execute(
                r#"UPDATE connector_authorization_actions
                   SET action_status = 'consumed', resolved_at = ?3,
                       resolved_intent = ?4, authority_cleanup_required = 1
                   WHERE authorization_id = ?1 AND review_id = ?2
                     AND token_hash = ?5 AND action_status = 'active'
                     AND resolved_at IS NULL"#,
                params![
                    authorization_id.to_string(),
                    review_id.to_string(),
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    resolved_intent,
                    token_hash,
                ],
            )?
        } else {
            transaction.execute(
                r#"DELETE FROM connector_authorization_actions
                   WHERE authorization_id = ?1 AND token_hash = ?2
                     AND review_id IS NULL"#,
                params![authorization_id.to_string(), token_hash],
            )?
        };
        if consumed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector authorization action raced".to_string(),
            ));
        }
        transaction.commit()?;
        Ok(ConnectorAuthorizationResolution::Approved(
            ConnectorAuthorizationExchangeClaim {
                session,
                claim_id,
                claim_expires_at,
                action_authority_handle,
            },
        ))
    }

    pub(crate) fn validate_connector_authorization_exchange_claim(
        &self,
        claim: &ConnectorAuthorizationExchangeClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let (session_json, status, revision, claim_id, claim_expires_at) = self.conn.query_row(
            r#"SELECT session_json, status, revision, exchange_claim_id,
                      exchange_claim_expires_at
               FROM connector_authorization_sessions WHERE id = ?1"#,
            params![claim.session.id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )?;
        let revision = u64::try_from(revision).map_err(|_| {
            EventStoreError::InvalidState("OAuth authorization revision is invalid".to_string())
        })?;
        let claim_expires_at = claim_expires_at
            .as_deref()
            .map(|value| DateTime::parse_from_rfc3339(value).map(|value| value.with_timezone(&Utc)))
            .transpose()?;
        if session_json != serde_json::to_string(&claim.session)?
            || serde_json::from_str::<ConnectorAuthorizationStatus>(&status)?
                != ConnectorAuthorizationStatus::Exchanging
            || revision != claim.session.revision
            || claim_id != Some(claim.claim_id.to_string())
            || claim_expires_at != Some(claim.claim_expires_at)
            || claim.claim_expires_at <= now
        {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization exchange claim is stale".to_string(),
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn claim_connector_authorization_session(
        &self,
        id: Uuid,
        returned_state: &str,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationExchangeClaim> {
        let mut session = self.connector_authorization_session(id)?;
        if session.status != ConnectorAuthorizationStatus::Pending
            || session.consumed_at.is_some()
            || session.expires_at <= now
            || session.state != returned_state
        {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization session cannot be claimed".to_string(),
            ));
        }
        session.status = ConnectorAuthorizationStatus::Exchanging;
        session.revision = session.revision.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState("OAuth authorization revision overflowed".to_string())
        })?;
        let claim_id = Uuid::new_v4();
        let claim_expires_at = now + Duration::minutes(5);
        let updated = self.conn.execute(
            r#"UPDATE connector_authorization_sessions
               SET session_json = ?2, status = ?3, revision = ?6, updated_at = ?4,
                   exchange_claim_id = ?7, exchange_claim_expires_at = ?8
               WHERE id = ?1 AND status = ?5 AND consumed_at IS NULL AND expires_at > ?4"#,
            params![
                session.id.to_string(),
                serde_json::to_string(&session)?,
                serde_json::to_string(&session.status)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorAuthorizationStatus::Pending)?,
                i64::try_from(session.revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
                claim_id.to_string(),
                claim_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if updated != 1 {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization session was already claimed".to_string(),
            ));
        }
        Ok(ConnectorAuthorizationExchangeClaim {
            session,
            claim_id,
            claim_expires_at,
            action_authority_handle: None,
        })
    }

    #[cfg(test)]
    pub fn finish_connector_authorization_session(
        &self,
        session: &ConnectorAuthorizationSession,
        claim_id: Uuid,
        claim_expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if session.status != ConnectorAuthorizationStatus::Completed
            || session.consumed_at.is_none()
        {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization session is not completed".to_string(),
            ));
        }
        let updated = self.conn.execute(
            r#"UPDATE connector_authorization_sessions
               SET session_json = ?2, consumed_at = ?3, status = ?4,
                   revision = ?7, updated_at = ?5,
                   exchange_claim_id = NULL, exchange_claim_expires_at = NULL
               WHERE id = ?1 AND status = ?6 AND consumed_at IS NULL
                 AND exchange_claim_id = ?8 AND exchange_claim_expires_at = ?9
                 AND exchange_claim_expires_at > ?5"#,
            params![
                session.id.to_string(),
                serde_json::to_string(session)?,
                session
                    .consumed_at
                    .map(|value| value.to_rfc3339_opts(SecondsFormat::Nanos, true)),
                serde_json::to_string(&session.status)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorAuthorizationStatus::Exchanging)?,
                i64::try_from(session.revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
                claim_id.to_string(),
                claim_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if updated != 1 {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization session finalization raced".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn finish_connector_authorization_with_account(
        &self,
        session: &ConnectorAuthorizationSession,
        account: &ConnectorAccount,
        claim_id: Uuid,
        claim_expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if session.status != ConnectorAuthorizationStatus::Completed
            || session.consumed_at.is_none()
            || session.cleanup_required
            || account.provider_id != session.provider_id
            || account.credential_handle != session.result_credential_handle
            || account.granted_capabilities != session.requested_capabilities
            || account.health != ConnectorHealth::Connected
        {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization account binding is invalid".to_string(),
            ));
        }
        let mut claimed = session.clone();
        claimed.status = ConnectorAuthorizationStatus::Exchanging;
        claimed.consumed_at = None;
        claimed.revision = claimed.revision.checked_sub(1).ok_or_else(|| {
            EventStoreError::InvalidState("OAuth authorization revision is invalid".to_string())
        })?;
        let claimed_json = serde_json::to_string(&claimed)?;
        let transaction = self.conn.unchecked_transaction()?;
        let inserted = transaction.execute(
            r#"INSERT INTO connector_accounts (id, provider_id, account_json, health, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5)"#,
            params![
                account.id.to_string(),
                account.provider_id,
                serde_json::to_string(account)?,
                serde_json::to_string(&account.health)?,
                account
                    .updated_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if inserted != 1 {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization account materialization raced".to_string(),
            ));
        }
        transaction.execute(
            r#"INSERT INTO connector_account_generations (account_id, generation)
               VALUES (?1, 0)"#,
            params![account.id.to_string()],
        )?;
        let changed = transaction.execute(
            r#"UPDATE connector_authorization_sessions
               SET session_json = ?2, consumed_at = ?3, status = ?4,
                   revision = ?5, account_id = ?6, updated_at = ?7,
                   exchange_claim_id = NULL, exchange_claim_expires_at = NULL
               WHERE id = ?1 AND session_json = ?8 AND status = ?9
                 AND revision = ?10 AND consumed_at IS NULL
                 AND cleanup_required = 0
                 AND exchange_claim_id = ?11 AND exchange_claim_expires_at = ?12
                 AND exchange_claim_expires_at > ?7"#,
            params![
                session.id.to_string(),
                serde_json::to_string(session)?,
                session
                    .consumed_at
                    .map(|value| value.to_rfc3339_opts(SecondsFormat::Nanos, true)),
                serde_json::to_string(&ConnectorAuthorizationStatus::Completed)?,
                i64::try_from(session.revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
                account.id.to_string(),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                claimed_json,
                serde_json::to_string(&ConnectorAuthorizationStatus::Exchanging)?,
                i64::try_from(claimed.revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
                claim_id.to_string(),
                claim_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization account finalization raced".to_string(),
            ));
        }
        transaction.execute(
            r#"DELETE FROM connector_authorization_actions
               WHERE authorization_id = ?1 AND review_id IS NULL"#,
            params![session.id.to_string()],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn mark_connector_authorization_repair(
        &self,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let mut session = self.connector_authorization_session(id)?;
        if matches!(
            session.status,
            ConnectorAuthorizationStatus::Completed | ConnectorAuthorizationStatus::Exchanging
        ) {
            return Err(EventStoreError::InvalidState(
                "claimed OAuth authorization cannot be generically repaired".to_string(),
            ));
        }
        session.status = ConnectorAuthorizationStatus::RepairRequired;
        session.cleanup_required = true;
        session.cleanup_completed_at = None;
        session.revision = session.revision.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState("OAuth authorization revision overflowed".to_string())
        })?;
        let updated = self.conn.execute(
            r#"UPDATE connector_authorization_sessions
               SET session_json = ?2, status = ?3, revision = ?6,
                   cleanup_required = 1, cleanup_completed_at = NULL, updated_at = ?4
               WHERE id = ?1 AND status != ?5"#,
            params![
                session.id.to_string(),
                serde_json::to_string(&session)?,
                serde_json::to_string(&session.status)?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                serde_json::to_string(&ConnectorAuthorizationStatus::Completed)?,
                i64::try_from(session.revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
            ],
        )?;
        if updated != 1 {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization repair state did not persist".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn mark_connector_authorization_exchange_repair(
        &self,
        id: Uuid,
        claim_id: Uuid,
        claim_expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if claim_expires_at <= now {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization exchange claim expired".to_string(),
            ));
        }
        let mut session = self.connector_authorization_session(id)?;
        if session.status != ConnectorAuthorizationStatus::Exchanging {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization exchange is not active".to_string(),
            ));
        }
        let previous_json = serde_json::to_string(&session)?;
        let previous_revision = session.revision;
        session.status = ConnectorAuthorizationStatus::RepairRequired;
        session.cleanup_required = true;
        session.cleanup_completed_at = None;
        session.revision = session.revision.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState("OAuth authorization revision overflowed".to_string())
        })?;
        let changed = self.conn.execute(
            r#"UPDATE connector_authorization_sessions
               SET session_json = ?2, status = ?3, revision = ?4,
                   cleanup_required = 1, cleanup_completed_at = NULL,
                   exchange_claim_id = NULL, exchange_claim_expires_at = NULL,
                   updated_at = ?5
               WHERE id = ?1 AND session_json = ?6 AND status = ?7 AND revision = ?8
                 AND exchange_claim_id = ?9 AND exchange_claim_expires_at = ?10
                 AND exchange_claim_expires_at > ?5"#,
            params![
                id.to_string(),
                serde_json::to_string(&session)?,
                serde_json::to_string(&ConnectorAuthorizationStatus::RepairRequired)?,
                i64::try_from(session.revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                previous_json,
                serde_json::to_string(&ConnectorAuthorizationStatus::Exchanging)?,
                i64::try_from(previous_revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
                claim_id.to_string(),
                claim_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization exchange repair raced".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn connector_authorization_cleanup_candidates(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<Uuid>> {
        let limit = i64::try_from(limit.min(64)).map_err(|_| {
            EventStoreError::InvalidState("OAuth cleanup limit is invalid".to_string())
        })?;
        let mut statement = self.conn.prepare(
            r#"SELECT id FROM connector_authorization_sessions
               WHERE cleanup_required = 1
                  OR status = ?1
                  OR EXISTS (
                    SELECT 1 FROM connector_authorization_actions action
                    WHERE action.authorization_id = connector_authorization_sessions.id
                      AND action.action_status = 'preparing'
                  )
                  OR (status = ?2 AND
                      (exchange_claim_expires_at IS NULL OR exchange_claim_expires_at <= ?4))
                  OR (status = ?3 AND expires_at <= ?4)
               ORDER BY updated_at ASC LIMIT ?5"#,
        )?;
        let candidates = statement
            .query_map(
                params![
                    serde_json::to_string(&ConnectorAuthorizationStatus::Preparing)?,
                    serde_json::to_string(&ConnectorAuthorizationStatus::Exchanging)?,
                    serde_json::to_string(&ConnectorAuthorizationStatus::Pending)?,
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    limit,
                ],
                |row| row.get::<_, String>(0),
            )?
            .map(|id| Ok(Uuid::parse_str(&id?)?))
            .collect();
        candidates
    }

    pub(crate) fn begin_connector_authorization_cleanup(
        &self,
        id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationCleanupClaim> {
        let transaction = self.conn.unchecked_transaction()?;
        let (
            session_json,
            status,
            revision,
            cleanup_required,
            expires_at,
            current_claim_expiry,
            exchange_claim_expiry,
            action_authority_handle_json,
            action_status,
        ) = transaction.query_row(
            r#"SELECT session_json, status, revision, cleanup_required, expires_at,
                          cleanup_claim_expires_at, exchange_claim_expires_at,
                          (SELECT authority_handle_json FROM connector_authorization_actions
                           WHERE authorization_id = connector_authorization_sessions.id
                             AND action_status = 'preparing'),
                          (SELECT action_status FROM connector_authorization_actions
                           WHERE authorization_id = connector_authorization_sessions.id)
               FROM connector_authorization_sessions WHERE id = ?1"#,
            params![id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )?;
        let mut session: ConnectorAuthorizationSession = serde_json::from_str(&session_json)?;
        let projected_status: ConnectorAuthorizationStatus = serde_json::from_str(&status)?;
        let projected_revision = u64::try_from(revision).map_err(|_| {
            EventStoreError::InvalidState("OAuth authorization revision is invalid".to_string())
        })?;
        let projected_expiry = DateTime::parse_from_rfc3339(&expires_at)?.with_timezone(&Utc);
        let current_claim_expiry = current_claim_expiry
            .as_deref()
            .map(|value| DateTime::parse_from_rfc3339(value).map(|value| value.with_timezone(&Utc)))
            .transpose()?;
        let exchange_claim_expiry = exchange_claim_expiry
            .as_deref()
            .map(|value| DateTime::parse_from_rfc3339(value).map(|value| value.with_timezone(&Utc)))
            .transpose()?;
        let action_authority_handle = action_authority_handle_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        let preparing_action_requires_cleanup =
            matches!(action_status.as_deref(), Some("preparing"));
        let active_action_expired =
            matches!(action_status.as_deref(), Some("active")) && projected_expiry <= now;
        let action_requires_cleanup = preparing_action_requires_cleanup || active_action_expired;
        if session.id != id
            || session.status != projected_status
            || session.revision != projected_revision
            || session.cleanup_required != (cleanup_required != 0)
            || session.expires_at != projected_expiry
            || session.status == ConnectorAuthorizationStatus::Completed
        {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization cleanup projection is invalid".to_string(),
            ));
        }
        if current_claim_expiry.is_some_and(|expiry| expiry > now) {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization cleanup is already claimed".to_string(),
            ));
        }
        if session.status == ConnectorAuthorizationStatus::Exchanging
            && exchange_claim_expiry.is_some_and(|expiry| expiry > now)
        {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization exchange is still active".to_string(),
            ));
        }
        let next_status = match session.status {
            ConnectorAuthorizationStatus::Preparing => ConnectorAuthorizationStatus::Cancelled,
            ConnectorAuthorizationStatus::Pending if preparing_action_requires_cleanup => {
                ConnectorAuthorizationStatus::RepairRequired
            }
            ConnectorAuthorizationStatus::Pending if active_action_expired => {
                ConnectorAuthorizationStatus::Cancelled
            }
            ConnectorAuthorizationStatus::Pending if session.expires_at <= now => {
                ConnectorAuthorizationStatus::Cancelled
            }
            ConnectorAuthorizationStatus::Exchanging
            | ConnectorAuthorizationStatus::RepairRequired => {
                ConnectorAuthorizationStatus::RepairRequired
            }
            ConnectorAuthorizationStatus::Cancelled if session.cleanup_required => {
                ConnectorAuthorizationStatus::Cancelled
            }
            _ if session.cleanup_required || action_requires_cleanup => session.status,
            _ => {
                return Err(EventStoreError::InvalidState(
                    "OAuth authorization cleanup is not due".to_string(),
                ))
            }
        };
        if !session.cleanup_required || session.status != next_status {
            session.status = next_status;
            session.cleanup_required = true;
            session.cleanup_completed_at = None;
            session.revision = session.revision.checked_add(1).ok_or_else(|| {
                EventStoreError::InvalidState("OAuth authorization revision overflowed".to_string())
            })?;
        }
        let claim_id = Uuid::new_v4();
        let claim_expires_at = now + Duration::minutes(5);
        let changed = transaction.execute(
            r#"UPDATE connector_authorization_sessions
                   SET session_json = ?2, status = ?3, revision = ?4,
                       cleanup_required = 1, cleanup_completed_at = NULL, updated_at = ?5,
                       exchange_claim_id = NULL, exchange_claim_expires_at = NULL,
                       cleanup_claim_id = ?9, cleanup_claim_expires_at = ?10
                   WHERE id = ?1 AND session_json = ?6 AND status = ?7
                     AND revision = ?8
                     AND (cleanup_claim_expires_at IS NULL OR cleanup_claim_expires_at <= ?5)"#,
            params![
                id.to_string(),
                serde_json::to_string(&session)?,
                serde_json::to_string(&session.status)?,
                i64::try_from(session.revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                session_json,
                status,
                revision,
                claim_id.to_string(),
                claim_expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization cleanup raced".to_string(),
            ));
        }
        transaction.execute(
            r#"UPDATE connector_authorization_actions
               SET action_status = 'cleanup_required', resolved_at = ?2
               WHERE authorization_id = ?1 AND authority_handle_json IS NOT NULL
                 AND action_status = 'preparing'"#,
            params![
                id.to_string(),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true)
            ],
        )?;
        transaction.execute(
            r#"UPDATE connector_authorization_actions
               SET action_status = 'consumed', resolved_at = ?2,
                   resolved_intent = 'expired', authority_cleanup_required = 1
               WHERE authorization_id = ?1 AND action_status = 'active'
                 AND resolved_at IS NULL AND expires_at <= ?2"#,
            params![
                id.to_string(),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true)
            ],
        )?;
        transaction.execute(
            r#"DELETE FROM connector_authorization_actions
               WHERE authorization_id = ?1 AND authority_handle_json IS NULL"#,
            params![id.to_string()],
        )?;
        transaction.commit()?;
        Ok(ConnectorAuthorizationCleanupClaim {
            session,
            claim_id,
            claim_expires_at,
            action_authority_handle,
        })
    }

    pub(crate) fn finish_connector_authorization_cleanup(
        &self,
        claim: &ConnectorAuthorizationCleanupClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAuthorizationSession> {
        let current = self.connector_authorization_session(claim.session.id)?;
        if current != claim.session
            || !current.cleanup_required
            || !matches!(
                current.status,
                ConnectorAuthorizationStatus::Cancelled
                    | ConnectorAuthorizationStatus::RepairRequired
            )
        {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization cleanup is stale".to_string(),
            ));
        }
        let previous_json = serde_json::to_string(&current)?;
        let mut next = current;
        next.cleanup_required = false;
        next.cleanup_completed_at = Some(now);
        next.revision = next.revision.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState("OAuth authorization revision overflowed".to_string())
        })?;
        let transaction = self.conn.unchecked_transaction()?;
        let changed = transaction.execute(
            r#"UPDATE connector_authorization_sessions
               SET session_json = ?2, revision = ?3, cleanup_required = 0,
                   cleanup_completed_at = ?4, updated_at = ?4,
                   cleanup_claim_id = NULL, cleanup_claim_expires_at = NULL
               WHERE id = ?1 AND session_json = ?5 AND revision = ?6
                 AND cleanup_required = 1 AND cleanup_claim_id = ?7
                 AND cleanup_claim_expires_at = ?8 AND cleanup_claim_expires_at > ?4"#,
            params![
                next.id.to_string(),
                serde_json::to_string(&next)?,
                i64::try_from(next.revision).map_err(|_| EventStoreError::InvalidState(
                    "OAuth authorization revision is too large".to_string()
                ))?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                previous_json,
                i64::try_from(claim.session.revision).map_err(
                    |_| EventStoreError::InvalidState(
                        "OAuth authorization revision is too large".to_string()
                    )
                )?,
                claim.claim_id.to_string(),
                claim
                    .claim_expires_at
                    .to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "OAuth authorization cleanup raced".to_string(),
            ));
        }
        transaction.execute(
            r#"DELETE FROM connector_authorization_actions
               WHERE authorization_id = ?1 AND action_status = 'cleanup_required'"#,
            params![claim.session.id.to_string()],
        )?;
        transaction.commit()?;
        Ok(next)
    }

    pub fn connector_sync_state(
        &self,
        account_id: Uuid,
        capability: crate::kernel::connectors::ConnectorCapability,
        stream_fingerprint: &str,
    ) -> EventStoreResult<Option<ConnectorSyncState>> {
        let json = self
            .conn
            .query_row(
                r#"SELECT state_json FROM connector_sync_streams
                   WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3"#,
                params![
                    account_id.to_string(),
                    capability.contract_name(),
                    stream_fingerprint,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        json.map(|value| {
            let state = ConnectorSyncState::from_persistence_json(value)
                .map_err(EventStoreError::InvalidState)?;
            if state.account_id() != account_id
                || state.capability() != capability
                || state.stream_fingerprint() != stream_fingerprint
            {
                return Err(EventStoreError::InvalidState(
                    "connector sync state identity is invalid".to_string(),
                ));
            }
            Ok(state)
        })
        .transpose()
    }

    pub(crate) fn record_connector_sync_plan(
        &self,
        state: &ConnectorSyncState,
        request_json: &str,
    ) -> EventStoreResult<()> {
        let plan = ConnectorSyncPlan::from_persistence_json(request_json)
            .map_err(EventStoreError::InvalidState)?;
        let capability_matches = matches!(
            (&plan, state.capability()),
            (
                ConnectorSyncPlan::MailInbox { .. },
                ConnectorCapability::MailSyncInbox
            ) | (
                ConnectorSyncPlan::CalendarRange { .. },
                ConnectorCapability::CalendarSyncEvents
            )
        );
        if !capability_matches {
            return Err(EventStoreError::InvalidState(
                "connector sync plan capability is invalid".to_string(),
            ));
        }
        let canonical_json = plan
            .persistence_json()
            .map_err(EventStoreError::InvalidState)?;
        let state_json = state
            .persistence_json()
            .map_err(EventStoreError::InvalidState)?;
        let transaction = self.conn.unchecked_transaction()?;
        ensure_connector_sync_account_active(&transaction, state)?;
        let existing = transaction
            .query_row(
                r#"SELECT request_json FROM connector_sync_streams
                   WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                     AND revision = ?4 AND state_json = ?5"#,
                params![
                    state.account_id().to_string(),
                    state.capability().contract_name(),
                    state.stream_fingerprint(),
                    i64::try_from(state.revision()).map_err(|_| EventStoreError::InvalidState(
                        "connector sync revision is too large".to_string()
                    ))?,
                    state_json,
                ],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?;
        match existing {
            Some(Some(value)) if constant_time_text_eq(&value, &canonical_json) => {}
            Some(None) => {
                let changed = transaction.execute(
                    r#"UPDATE connector_sync_streams SET request_json = ?6
                       WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                         AND revision = ?4 AND state_json = ?5 AND request_json IS NULL"#,
                    params![
                        state.account_id().to_string(),
                        state.capability().contract_name(),
                        state.stream_fingerprint(),
                        i64::try_from(state.revision()).map_err(|_| {
                            EventStoreError::InvalidState(
                                "connector sync revision is too large".to_string(),
                            )
                        })?,
                        state_json,
                        canonical_json,
                    ],
                )?;
                if changed != 1 {
                    return Err(EventStoreError::InvalidState(
                        "connector sync plan raced with another worker".to_string(),
                    ));
                }
            }
            Some(Some(_)) => {
                return Err(EventStoreError::InvalidState(
                    "connector sync plan changed for an existing stream".to_string(),
                ));
            }
            None if state.revision() == 0 => {
                let inserted = transaction.execute(
                    r#"INSERT OR IGNORE INTO connector_sync_streams
                       (account_id, capability, stream_fingerprint, state_json, revision,
                        request_json, updated_at)
                       VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)"#,
                    params![
                        state.account_id().to_string(),
                        state.capability().contract_name(),
                        state.stream_fingerprint(),
                        state_json,
                        canonical_json,
                        state
                            .updated_at()
                            .to_rfc3339_opts(SecondsFormat::Nanos, true),
                    ],
                )?;
                if inserted != 1 {
                    return Err(EventStoreError::InvalidState(
                        "connector sync plan raced with another worker".to_string(),
                    ));
                }
            }
            None => {
                return Err(EventStoreError::InvalidState(
                    "connector sync state changed before its plan was recorded".to_string(),
                ));
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn connector_account_sync_health_snapshot(
        &self,
        account: &ConnectorAccount,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorSyncHealthSnapshot> {
        let generation = self.connector_account_sync_generation(account)?;
        let mut statement = self.conn.prepare(
            r#"SELECT state_json, last_successful_at FROM connector_sync_streams
               WHERE account_id = ?1 ORDER BY updated_at DESC"#,
        )?;
        let rows = statement.query_map(params![account.id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        let mut snapshot = ConnectorSyncHealthSnapshot::default();
        for row in rows {
            let Ok((value, last_successful_at)) = row else {
                continue;
            };
            let Ok(state) = ConnectorSyncState::from_persistence_json(value) else {
                continue;
            };
            if state.account_id() != account.id || state.account_generation() != generation {
                continue;
            }
            snapshot.stream_count = snapshot.stream_count.saturating_add(1);
            snapshot.any_stopped |= state.stopped();
            snapshot.any_delayed |= state
                .retry_state()
                .is_some_and(|retry| retry.retry_at > now);
            if let Some(last_successful_at) = last_successful_at {
                let parsed = DateTime::parse_from_rfc3339(&last_successful_at)?.with_timezone(&Utc);
                snapshot.last_successful_sync_at = Some(
                    snapshot
                        .last_successful_sync_at
                        .map_or(parsed, |current| current.max(parsed)),
                );
            }
        }
        Ok(snapshot)
    }

    pub fn commit_connector_sync_page<T>(
        &self,
        state: &ConnectorSyncState,
        page: &ConnectorSyncPage<T>,
        remote_ref: impl Fn(&T) -> &str,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorSyncState>
    where
        T: serde::Serialize,
    {
        let transaction = self.conn.unchecked_transaction()?;
        let next = self.commit_connector_sync_page_in_transaction(
            &transaction,
            state,
            page,
            &remote_ref,
            now,
        )?;
        transaction.commit()?;
        Ok(next)
    }

    fn commit_connector_sync_page_in_transaction<T, F>(
        &self,
        transaction: &Transaction<'_>,
        state: &ConnectorSyncState,
        page: &ConnectorSyncPage<T>,
        remote_ref: &F,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorSyncState>
    where
        T: serde::Serialize,
        F: Fn(&T) -> &str,
    {
        if page.changes().len() > 100 {
            return Err(EventStoreError::InvalidState(
                "connector sync page exceeded the shared item budget".to_string(),
            ));
        }
        let next = state
            .advance(page, now)
            .map_err(EventStoreError::InvalidState)?;
        let state_json = next
            .persistence_json()
            .map_err(EventStoreError::InvalidState)?;
        let receipt = ConnectorSyncReceipt {
            account_id: next.account_id(),
            account_generation: next.account_generation(),
            capability: next.capability(),
            stream_fingerprint: next.stream_fingerprint().to_string(),
            change_count: page.changes().len(),
            has_resume_page: next.has_resume_page(),
            has_committed_delta: next.has_committed_delta(),
            revision: next.revision(),
            committed_at: now,
        };
        let event = KernelEvent::new(
            "connector.sync_page.committed",
            &serde_json::json!({
                "kind": "read_sync",
                "capability": match next.capability() {
                    ConnectorCapability::MailSyncInbox => "mail",
                    ConnectorCapability::CalendarSyncEvents => "calendar",
                    _ => "unsupported",
                },
                "change_count": receipt.change_count,
                "has_more": receipt.has_resume_page,
                "committed_at": receipt.committed_at,
            }),
        )?;
        ensure_connector_sync_account_active(transaction, &next)?;
        for change in page.changes() {
            let (remote_ref, item_json, deleted) = match change {
                ConnectorSyncChange::Upsert(item) => (
                    remote_ref(item).trim().to_string(),
                    Some(serde_json::to_string(item)?),
                    0,
                ),
                ConnectorSyncChange::Deleted { remote_ref } => {
                    (remote_ref.trim().to_string(), None, 1)
                }
            };
            if remote_ref.is_empty() || remote_ref.chars().count() > 1024 {
                return Err(EventStoreError::InvalidState(
                    "connector sync remote reference is invalid".to_string(),
                ));
            }
            if item_json
                .as_ref()
                .is_some_and(|value| value.len() > MAX_SYNC_PROJECTION_ITEM_BYTES)
            {
                return Err(EventStoreError::InvalidState(
                    "connector sync projection item exceeded the byte budget".to_string(),
                ));
            }
            transaction.execute(
                r#"INSERT INTO connector_sync_projection
                   (account_id, capability, stream_fingerprint, remote_ref, item_json, deleted, updated_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                   ON CONFLICT(account_id, capability, stream_fingerprint, remote_ref)
                   DO UPDATE SET item_json = excluded.item_json,
                                 deleted = excluded.deleted,
                                 updated_at = excluded.updated_at"#,
                params![
                    next.account_id().to_string(),
                    next.capability().contract_name(),
                    next.stream_fingerprint(),
                    remote_ref,
                    item_json,
                    deleted,
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )?;
        }
        transaction.execute(
            r#"DELETE FROM connector_sync_projection
               WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                 AND rowid NOT IN (
                   SELECT rowid FROM connector_sync_projection
                   WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                   ORDER BY updated_at DESC, rowid DESC
                   LIMIT ?4
                 )"#,
            params![
                next.account_id().to_string(),
                next.capability().contract_name(),
                next.stream_fingerprint(),
                i64::try_from(MAX_SYNC_PROJECTION_ITEMS_PER_STREAM).map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync projection budget is invalid".to_string(),
                    )
                })?,
            ],
        )?;
        let changed = if state.revision() == 0 {
            let updated = transaction.execute(
                r#"UPDATE connector_sync_streams
                   SET state_json = ?4, revision = ?5, updated_at = ?6,
                       last_successful_at = ?6
                   WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                     AND revision = 0 AND request_json IS NOT NULL"#,
                params![
                    next.account_id().to_string(),
                    next.capability().contract_name(),
                    next.stream_fingerprint(),
                    state_json,
                    i64::try_from(next.revision()).map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector sync revision is too large".to_string(),
                        )
                    })?,
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )?;
            if updated == 1 {
                updated
            } else {
                transaction.execute(
                    r#"INSERT OR IGNORE INTO connector_sync_streams
                       (account_id, capability, stream_fingerprint, state_json, revision,
                        last_successful_at, updated_at)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)"#,
                    params![
                        next.account_id().to_string(),
                        next.capability().contract_name(),
                        next.stream_fingerprint(),
                        state_json,
                        i64::try_from(next.revision()).map_err(|_| {
                            EventStoreError::InvalidState(
                                "connector sync revision is too large".to_string(),
                            )
                        })?,
                        now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    ],
                )?
            }
        } else {
            transaction.execute(
                r#"UPDATE connector_sync_streams
                   SET state_json = ?4, revision = ?5, updated_at = ?6,
                       last_successful_at = ?6
                   WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                     AND revision = ?7"#,
                params![
                    next.account_id().to_string(),
                    next.capability().contract_name(),
                    next.stream_fingerprint(),
                    state_json,
                    i64::try_from(next.revision()).map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector sync revision is too large".to_string(),
                        )
                    })?,
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    i64::try_from(state.revision()).map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector sync revision is too large".to_string(),
                        )
                    })?,
                ],
            )?
        };
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector sync page raced with another worker".to_string(),
            ));
        }
        prune_connector_sync_retention(transaction, next.account_id(), now)?;
        transaction.execute(
            r#"INSERT INTO kernel_events (id, event_type, payload_json, created_at)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![
                event.id.to_string(),
                event.event_type,
                event.payload_json,
                event.created_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        Ok(next)
    }

    pub fn connector_sync_projection_summaries(
        &self,
        account_id: Uuid,
        capability: crate::kernel::connectors::ConnectorCapability,
        stream_fingerprint: &str,
    ) -> EventStoreResult<Vec<ConnectorSyncProjectionSummary>> {
        let mut statement = self.conn.prepare(
            r#"SELECT remote_ref, deleted, updated_at
               FROM connector_sync_projection
               WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
               ORDER BY remote_ref ASC"#,
        )?;
        let rows = statement.query_map(
            params![
                account_id.to_string(),
                capability.contract_name(),
                stream_fingerprint,
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        let mut summaries = Vec::new();
        for row in rows {
            let (remote_ref, deleted, updated_at) = row?;
            summaries.push(ConnectorSyncProjectionSummary {
                remote_ref,
                deleted: deleted != 0,
                updated_at: DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc),
            });
        }
        Ok(summaries)
    }

    pub fn compare_and_swap_connector_sync_state(
        &self,
        current: &ConnectorSyncState,
        next: &ConnectorSyncState,
        reason: &str,
    ) -> EventStoreResult<()> {
        if current.account_id() != next.account_id()
            || current.account_generation() != next.account_generation()
            || current.capability() != next.capability()
            || current.stream_fingerprint() != next.stream_fingerprint()
            || next.revision() != current.revision().saturating_add(1)
            || !matches!(
                reason,
                "cursor_rebuilt" | "retry_scheduled" | "retry_exhausted"
            )
        {
            return Err(EventStoreError::InvalidState(
                "connector sync state transition is invalid".to_string(),
            ));
        }
        let state_json = next
            .persistence_json()
            .map_err(EventStoreError::InvalidState)?;
        let receipt = ConnectorSyncStateReceipt {
            account_id: next.account_id(),
            account_generation: next.account_generation(),
            capability: next.capability(),
            stream_fingerprint: next.stream_fingerprint().to_string(),
            reason: reason.to_string(),
            has_resume_page: next.has_resume_page(),
            has_committed_delta: next.has_committed_delta(),
            retry_at: next.retry_state().map(|retry| retry.retry_at),
            stopped: next.stopped(),
            rebuild_attempt: next.rebuild_attempt(),
            revision: next.revision(),
            changed_at: next.updated_at(),
        };
        let event = KernelEvent::new(
            "connector.sync_state.changed",
            &serde_json::json!({
                "kind": "read_sync",
                "capability": match next.capability() {
                    ConnectorCapability::MailSyncInbox => "mail",
                    ConnectorCapability::CalendarSyncEvents => "calendar",
                    _ => "unsupported",
                },
                "reason": receipt.reason,
                "stopped": receipt.stopped,
                "changed_at": receipt.changed_at,
            }),
        )?;
        let transaction = self.conn.unchecked_transaction()?;
        ensure_connector_sync_account_active(&transaction, next)?;
        let changed = if current.revision() == 0 {
            let updated = transaction.execute(
                r#"UPDATE connector_sync_streams
                   SET state_json = ?4, revision = ?5, updated_at = ?6
                   WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                     AND revision = 0 AND request_json IS NOT NULL"#,
                params![
                    next.account_id().to_string(),
                    next.capability().contract_name(),
                    next.stream_fingerprint(),
                    state_json,
                    i64::try_from(next.revision()).map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector sync revision is too large".to_string(),
                        )
                    })?,
                    next.updated_at()
                        .to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )?;
            if updated == 1 {
                updated
            } else {
                transaction.execute(
                    r#"INSERT OR IGNORE INTO connector_sync_streams
                       (account_id, capability, stream_fingerprint, state_json, revision, updated_at)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
                    params![
                        next.account_id().to_string(),
                        next.capability().contract_name(),
                        next.stream_fingerprint(),
                        state_json,
                        i64::try_from(next.revision()).map_err(|_| {
                            EventStoreError::InvalidState(
                                "connector sync revision is too large".to_string(),
                            )
                        })?,
                        next.updated_at()
                            .to_rfc3339_opts(SecondsFormat::Nanos, true),
                    ],
                )?
            }
        } else {
            transaction.execute(
                r#"UPDATE connector_sync_streams
                   SET state_json = ?4, revision = ?5, updated_at = ?6
                   WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3
                     AND revision = ?7"#,
                params![
                    next.account_id().to_string(),
                    next.capability().contract_name(),
                    next.stream_fingerprint(),
                    state_json,
                    i64::try_from(next.revision()).map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector sync revision is too large".to_string(),
                        )
                    })?,
                    next.updated_at()
                        .to_rfc3339_opts(SecondsFormat::Nanos, true),
                    i64::try_from(current.revision()).map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector sync revision is too large".to_string(),
                        )
                    })?,
                ],
            )?
        };
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector sync state raced with another worker".to_string(),
            ));
        }
        if reason == "cursor_rebuilt" {
            transaction.execute(
                r#"DELETE FROM connector_sync_projection
                   WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3"#,
                params![
                    next.account_id().to_string(),
                    next.capability().contract_name(),
                    next.stream_fingerprint(),
                ],
            )?;
        }
        prune_connector_sync_retention(&transaction, next.account_id(), next.updated_at())?;
        transaction.execute(
            r#"INSERT INTO kernel_events (id, event_type, payload_json, created_at)
               VALUES (?1, ?2, ?3, ?4)"#,
            params![
                event.id.to_string(),
                event.event_type,
                event.payload_json,
                event.created_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn append(&self, event: &KernelEvent) -> EventStoreResult<()> {
        let transaction = self.conn.unchecked_transaction()?;
        Self::insert_kernel_event(&transaction, event)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_recent(&self, limit: usize) -> EventStoreResult<Vec<KernelEvent>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut statement = self.conn.prepare(
            r#"
            SELECT id, event_type, payload_json, created_at
            FROM kernel_events
            ORDER BY created_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = statement
            .query_map(params![limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut events = Vec::with_capacity(rows.len());
        for (id, event_type, payload_json, created_at) in rows {
            events.push(KernelEvent {
                id: Uuid::parse_str(&id)?,
                event_type,
                payload_json,
                created_at: DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc),
            });
        }

        Ok(events)
    }

    pub fn append_task_record(&self, record: &TaskRecord) -> EventStoreResult<()> {
        let event = KernelEvent::new(TASK_RECORD_CREATED_EVENT, record)?;
        self.append(&event)
    }

    pub fn list_task_records(&self) -> EventStoreResult<Vec<TaskRecord>> {
        let events = self.list_by_type(TASK_RECORD_CREATED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<TaskRecord>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn append_agent_run_start(&self, start: &AgentRunStart) -> EventStoreResult<bool> {
        let starts = self.list_agent_run_starts()?;
        let existing = starts.iter().any(|record| record.id == start.id);
        if existing {
            return Ok(false);
        }

        if start.role == AgentRunRole::Subagent {
            let parent_id = start.parent_run_id.ok_or_else(|| {
                EventStoreError::InvalidState("subagent run requires a parent run".to_string())
            })?;
            let parent = starts
                .iter()
                .find(|record| record.id == parent_id)
                .ok_or_else(|| {
                    EventStoreError::InvalidState("subagent parent run does not exist".to_string())
                })?;
            if parent.role != AgentRunRole::Parent || parent.parent_run_id.is_some() {
                return Err(EventStoreError::InvalidState(
                    "recursive subagent delegation is not supported".to_string(),
                ));
            }
            if parent.conversation_id != start.conversation_id {
                return Err(EventStoreError::InvalidState(
                    "subagent must remain in its parent conversation".to_string(),
                ));
            }
            let sibling_count = starts
                .iter()
                .filter(|record| record.parent_run_id == Some(parent_id))
                .count();
            let sibling_limit = if start.expert_contract.is_some() {
                EXPERT_TEAM_MAX_TOTAL_ATTEMPTS
            } else {
                AGENT_RUN_MAX_PARALLEL_SUBAGENTS
            };
            if sibling_count >= sibling_limit {
                return Err(EventStoreError::InvalidState(format!(
                    "a parent run may create at most {sibling_limit} child attempts"
                )));
            }
            if let Some(contract) = &start.expert_contract {
                if contract.parent_run_id != parent_id
                    || contract.key != start.subtask_key.clone().unwrap_or_default()
                    || contract.parent_input_revision != parent_input_revision(&parent.prompt)
                {
                    return Err(EventStoreError::InvalidState(
                        "expert attempt contract does not match its immutable parent input"
                            .to_string(),
                    ));
                }
            }
        } else if start.parent_run_id.is_some() || start.subtask_key.is_some() {
            return Err(EventStoreError::InvalidState(
                "parent run cannot carry subagent linkage".to_string(),
            ));
        }

        let event = KernelEvent::new(AGENT_RUN_STARTED_EVENT, start)?;
        self.append(&event)?;
        Ok(true)
    }

    pub fn append_subagent_runs(
        &self,
        parent_run_id: Uuid,
        subtasks: Vec<(String, String)>,
    ) -> EventStoreResult<Vec<AgentRunRecord>> {
        if subtasks.is_empty() || subtasks.len() > AGENT_RUN_MAX_PARALLEL_SUBAGENTS {
            return Err(EventStoreError::InvalidState(format!(
                "subagent plan must contain between 1 and {AGENT_RUN_MAX_PARALLEL_SUBAGENTS} subtasks"
            )));
        }
        let records = self.list_agent_run_records()?;
        let parent = records
            .iter()
            .find(|record| record.id == parent_run_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("agent run {parent_run_id}")))?;
        if parent.role != AgentRunRole::Parent || parent.parent_run_id.is_some() {
            return Err(EventStoreError::InvalidState(
                "only a parent run may create subagents".to_string(),
            ));
        }
        if !matches!(
            parent.status,
            AgentRunStatus::Queued | AgentRunStatus::Running
        ) {
            return Err(EventStoreError::InvalidState(format!(
                "parent run cannot create subagents from status {:?}",
                parent.status
            )));
        }
        if records
            .iter()
            .any(|record| record.parent_run_id == Some(parent_run_id))
        {
            return Err(EventStoreError::InvalidState(
                "parent run already has a subagent plan".to_string(),
            ));
        }

        let mut keys = std::collections::HashSet::new();
        let starts = subtasks
            .into_iter()
            .map(|(key, prompt)| {
                let start = AgentRunStart::queued_subagent(
                    parent_run_id,
                    parent.conversation_id.clone(),
                    key,
                    prompt,
                )
                .map_err(EventStoreError::InvalidState)?;
                if !keys.insert(start.subtask_key.clone().unwrap_or_default()) {
                    return Err(EventStoreError::InvalidState(
                        "subagent subtask keys must be unique within a parent run".to_string(),
                    ));
                }
                Ok(start)
            })
            .collect::<EventStoreResult<Vec<_>>>()?;
        for start in &starts {
            self.append_agent_run_start(start)?;
        }
        let child_ids = starts
            .iter()
            .map(|start| start.id)
            .collect::<std::collections::HashSet<_>>();
        Ok(self
            .list_agent_run_records()?
            .into_iter()
            .filter(|record| child_ids.contains(&record.id))
            .collect())
    }

    pub fn append_expert_team_runs(
        &self,
        parent_run_id: Uuid,
        items: Vec<ExpertTeamPlanItem>,
    ) -> EventStoreResult<Vec<AgentRunRecord>> {
        let records = self.list_agent_run_records()?;
        let parent = records
            .iter()
            .find(|record| record.id == parent_run_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("agent run {parent_run_id}")))?;
        if parent.role != AgentRunRole::Parent || parent.parent_run_id.is_some() {
            return Err(EventStoreError::InvalidState(
                "only a parent run may create an expert team".to_string(),
            ));
        }
        if !matches!(
            parent.status,
            AgentRunStatus::Queued | AgentRunStatus::Running
        ) {
            return Err(EventStoreError::InvalidState(format!(
                "parent run cannot create an expert team from status {:?}",
                parent.status
            )));
        }
        if records
            .iter()
            .any(|record| record.parent_run_id == Some(parent_run_id))
        {
            return Err(EventStoreError::InvalidState(
                "parent run already has a child plan".to_string(),
            ));
        }
        let contracts = validate_team_plan(parent_run_id, &parent.prompt, &items)
            .map_err(EventStoreError::InvalidState)?;
        let starts = contracts
            .into_iter()
            .map(|contract| {
                AgentRunStart::queued_expert(
                    parent_run_id,
                    parent.conversation_id.clone(),
                    contract,
                )
                .map_err(EventStoreError::InvalidState)
            })
            .collect::<EventStoreResult<Vec<_>>>()?;
        let events = starts
            .iter()
            .map(|start| KernelEvent::new(AGENT_RUN_STARTED_EVENT, start).map_err(Into::into))
            .collect::<EventStoreResult<Vec<_>>>()?;
        let transaction = self.conn.unchecked_transaction()?;
        for event in &events {
            Self::insert_kernel_event(&transaction, event)?;
        }
        transaction.commit()?;
        let child_ids = starts
            .iter()
            .map(|start| start.id)
            .collect::<std::collections::HashSet<_>>();
        Ok(self
            .list_agent_run_records()?
            .into_iter()
            .filter(|record| child_ids.contains(&record.id))
            .collect())
    }

    pub fn append_expert_retry(
        &self,
        parent_run_id: Uuid,
        key: &str,
    ) -> EventStoreResult<AgentRunRecord> {
        let records = self.list_agent_run_records()?;
        let parent = records
            .iter()
            .find(|record| record.id == parent_run_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("agent run {parent_run_id}")))?;
        let mut attempts = records
            .iter()
            .filter(|record| record.parent_run_id == Some(parent_run_id))
            .filter(|record| {
                record
                    .expert_contract
                    .as_ref()
                    .is_some_and(|contract| contract.key.eq_ignore_ascii_case(key))
            })
            .collect::<Vec<_>>();
        attempts.sort_by_key(|record| {
            record
                .expert_contract
                .as_ref()
                .map(|contract| contract.attempt)
                .unwrap_or(0)
        });
        let latest = attempts.last().copied().ok_or_else(|| {
            EventStoreError::NotFound(format!("expert attempt {parent_run_id}/{key}"))
        })?;
        let mut contract = latest.expert_contract.clone().ok_or_else(|| {
            EventStoreError::InvalidState("expert retry requires a contract".to_string())
        })?;
        if !matches!(
            latest.status,
            AgentRunStatus::Failed | AgentRunStatus::Cancelled
        ) {
            return Err(EventStoreError::InvalidState(
                "only a failed or cancelled latest expert attempt may retry".to_string(),
            ));
        }
        if latest
            .expert_result
            .as_ref()
            .is_some_and(|result| !result.retry_eligible)
            || contract.attempt >= contract.retry_policy.max_attempts
        {
            return Err(EventStoreError::InvalidState(
                "expert retry policy is exhausted or the effect state is unsafe".to_string(),
            ));
        }
        if records
            .iter()
            .filter(|record| record.parent_run_id == Some(parent_run_id))
            .count()
            >= EXPERT_TEAM_MAX_TOTAL_ATTEMPTS
        {
            return Err(EventStoreError::InvalidState(
                "expert team total attempt budget is exhausted".to_string(),
            ));
        }
        contract.attempt = contract.attempt.saturating_add(1);
        contract.previous_attempt_run_id = Some(latest.id);
        if let Some(substitute_role) = contract.retry_policy.substitute_role {
            contract.role = substitute_role;
        }
        contract.prompt = format!(
            "Retry attempt {} for the same bounded expert assignment. Preserve uncertainty and correct the previous failure without expanding capability scope.\n\nOriginal assignment:\n{}\n\nPrevious failure:\n{}",
            contract.attempt,
            contract.prompt,
            latest
                .finish_error
                .as_deref()
                .unwrap_or("The previous attempt did not pass its deterministic quality gate.")
        )
        .chars()
        .take(4_000)
        .collect();
        if contract.parent_input_revision != parent_input_revision(&parent.prompt) {
            return Err(EventStoreError::InvalidState(
                "expert retry parent input revision is stale".to_string(),
            ));
        }
        let start =
            AgentRunStart::queued_expert(parent_run_id, parent.conversation_id.clone(), contract)
                .map_err(EventStoreError::InvalidState)?;
        self.append_agent_run_start(&start)?;
        self.list_agent_run_records()?
            .into_iter()
            .find(|record| record.id == start.id)
            .ok_or_else(|| EventStoreError::NotFound(format!("agent run {}", start.id)))
    }

    pub fn append_due_expert_retries(
        &self,
        parent_run_id: Uuid,
    ) -> EventStoreResult<Vec<AgentRunRecord>> {
        let records = self.list_agent_run_records()?;
        let mut latest_by_key = std::collections::HashMap::<String, &AgentRunRecord>::new();
        for record in records
            .iter()
            .filter(|record| record.parent_run_id == Some(parent_run_id))
            .filter(|record| record.expert_contract.is_some())
        {
            let contract = record.expert_contract.as_ref().expect("checked above");
            latest_by_key
                .entry(contract.key.to_ascii_lowercase())
                .and_modify(|current| {
                    if contract.attempt
                        > current
                            .expert_contract
                            .as_ref()
                            .map(|current| current.attempt)
                            .unwrap_or(0)
                    {
                        *current = record;
                    }
                })
                .or_insert(record);
        }
        let mut keys = latest_by_key
            .into_values()
            .filter(|record| {
                matches!(
                    record.status,
                    AgentRunStatus::Failed | AgentRunStatus::Cancelled
                )
            })
            .filter_map(|record| {
                let contract = record.expert_contract.as_ref()?;
                let eligible = record
                    .expert_result
                    .as_ref()
                    .map(|result| result.retry_eligible)
                    .unwrap_or(contract.attempt < contract.retry_policy.max_attempts);
                eligible.then_some(contract.key.clone())
            })
            .collect::<Vec<_>>();
        keys.sort();
        keys.into_iter()
            .map(|key| self.append_expert_retry(parent_run_id, &key))
            .collect()
    }

    pub fn request_agent_run_tree_cancel(
        &self,
        parent_run_id: Uuid,
        reason: String,
    ) -> EventStoreResult<Vec<AgentRunRecord>> {
        let records = self.list_agent_run_records()?;
        let parent = records
            .iter()
            .find(|record| record.id == parent_run_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("agent run {parent_run_id}")))?;
        if parent.role != AgentRunRole::Parent {
            return Err(EventStoreError::InvalidState(
                "run-tree cancellation must target the parent run".to_string(),
            ));
        }
        let target_ids = records
            .iter()
            .filter(|record| {
                record.id == parent_run_id || record.parent_run_id == Some(parent_run_id)
            })
            .filter(|record| {
                !matches!(
                    record.status,
                    AgentRunStatus::Completed | AgentRunStatus::Failed | AgentRunStatus::Cancelled
                )
            })
            .map(|record| record.id)
            .collect::<Vec<_>>();
        for run_id in &target_ids {
            let cancel = AgentRunCancelRequest::new(*run_id, reason.clone())
                .map_err(EventStoreError::InvalidState)?;
            self.append_agent_run_cancel_request(&cancel)?;
        }
        Ok(self
            .list_agent_run_records()?
            .into_iter()
            .filter(|record| target_ids.contains(&record.id))
            .collect())
    }

    fn list_agent_run_starts(&self) -> EventStoreResult<Vec<AgentRunStart>> {
        let events = self.list_by_type(AGENT_RUN_STARTED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunStart>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn claim_next_agent_run(
        &self,
        worker_id: String,
        lease_seconds: i64,
    ) -> EventStoreResult<Option<AgentRunRecord>> {
        self.recover_expired_agent_runs(Utc::now())?;
        let records = self.list_agent_run_records()?;
        let Some(next_run) = records
            .iter()
            .filter(|record| record.status == AgentRunStatus::Queued)
            .filter(|record| expert_attempt_ready(record, &records))
            .min_by_key(|record| record.started_at)
            .cloned()
        else {
            return Ok(None);
        };
        let claim = AgentRunClaim::new(next_run.id, worker_id, lease_seconds)
            .map_err(EventStoreError::InvalidState)?;
        self.append_agent_run_claim(&claim)?;
        self.list_agent_run_records()
            .map(|records| records.into_iter().find(|record| record.id == next_run.id))
    }

    pub fn claim_agent_run(
        &self,
        run_id: Uuid,
        worker_id: String,
        lease_seconds: i64,
    ) -> EventStoreResult<AgentRunRecord> {
        self.recover_expired_agent_runs(Utc::now())?;
        let records = self.list_agent_run_records()?;
        let target = records
            .iter()
            .find(|record| record.id == run_id)
            .cloned()
            .ok_or_else(|| EventStoreError::InvalidState("agent run does not exist".to_string()))?;
        if target.status != AgentRunStatus::Queued || target.cancel_requested {
            return Err(EventStoreError::InvalidState(format!(
                "agent run {} cannot be claimed from status {:?}",
                target.id, target.status
            )));
        }
        if !expert_attempt_ready(&target, &records) {
            return Err(EventStoreError::InvalidState(format!(
                "expert attempt {} is waiting for dependencies or a resource lease",
                target.id
            )));
        }

        let claim = AgentRunClaim::new(run_id, worker_id, lease_seconds)
            .map_err(EventStoreError::InvalidState)?;
        self.append_agent_run_claim(&claim)?;
        self.list_agent_run_records()?
            .into_iter()
            .find(|record| record.id == run_id)
            .ok_or_else(|| EventStoreError::InvalidState("claimed agent run missing".to_string()))
    }

    pub fn append_agent_run_claim(&self, claim: &AgentRunClaim) -> EventStoreResult<()> {
        self.ensure_agent_run_exists(claim.run_id)?;
        let records = self.list_agent_run_records()?;
        if let Some(record) = records.iter().find(|record| record.id == claim.run_id) {
            if record.status == AgentRunStatus::Queued && !expert_attempt_ready(record, &records) {
                return Err(EventStoreError::InvalidState(
                    "expert attempt is not ready for a claim".to_string(),
                ));
            }
        }
        let event = KernelEvent::new(AGENT_RUN_CLAIMED_EVENT, claim)?;
        self.append(&event)
    }

    pub fn heartbeat_agent_run_lease(
        &self,
        run_id: Uuid,
        worker_id: String,
        lease_seconds: i64,
    ) -> EventStoreResult<AgentRunRecord> {
        let renewed_claim = AgentRunClaim::new(run_id, worker_id, lease_seconds)
            .map_err(EventStoreError::InvalidState)?;
        let record = self
            .list_agent_run_records()?
            .into_iter()
            .find(|record| record.id == run_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("agent run {run_id}")))?;
        if record.worker_id.as_deref() != Some(renewed_claim.worker_id.as_str()) {
            return Err(EventStoreError::InvalidState(format!(
                "agent run {run_id} lease is owned by another worker"
            )));
        }
        if matches!(
            record.status,
            AgentRunStatus::Queued
                | AgentRunStatus::Completed
                | AgentRunStatus::Failed
                | AgentRunStatus::Cancelled
        ) {
            return Err(EventStoreError::InvalidState(format!(
                "agent run {run_id} cannot renew its lease from status {:?}",
                record.status
            )));
        }
        if record
            .lease_expires_at
            .is_none_or(|lease_expires_at| lease_expires_at <= renewed_claim.claimed_at)
        {
            return Err(EventStoreError::InvalidState(format!(
                "agent run {run_id} lease expired before its heartbeat"
            )));
        }

        self.append_agent_run_claim(&renewed_claim)?;
        self.renew_agent_run_resources_for_run(run_id, lease_seconds)?;
        self.list_agent_run_records()?
            .into_iter()
            .find(|record| record.id == run_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("agent run {run_id}")))
    }

    fn list_agent_run_claims(&self) -> EventStoreResult<Vec<AgentRunClaim>> {
        let events = self.list_by_type(AGENT_RUN_CLAIMED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunClaim>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn append_agent_run_recovery(&self, recovery: &AgentRunRecovery) -> EventStoreResult<()> {
        self.ensure_agent_run_exists(recovery.run_id)?;
        let event = KernelEvent::new(AGENT_RUN_RECOVERED_EVENT, recovery)?;
        self.append(&event)
    }

    fn list_agent_run_recoveries(&self) -> EventStoreResult<Vec<AgentRunRecovery>> {
        let events = self.list_by_type(AGENT_RUN_RECOVERED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunRecovery>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn append_agent_run_execution_context(
        &self,
        context: &AgentRunExecutionContext,
    ) -> EventStoreResult<()> {
        self.ensure_agent_run_exists(context.run_id)?;
        let event = KernelEvent::new(AGENT_RUN_EXECUTION_CONTEXT_RECORDED_EVENT, context)?;
        self.append(&event)
    }

    fn list_agent_run_execution_contexts(&self) -> EventStoreResult<Vec<AgentRunExecutionContext>> {
        let events = self.list_by_type(AGENT_RUN_EXECUTION_CONTEXT_RECORDED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunExecutionContext>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_agent_run_continuation_queued(
        &self,
        continuation: &AgentRunContinuationQueued,
    ) -> EventStoreResult<()> {
        self.ensure_agent_run_exists(continuation.run_id)?;
        let invocation_exists = self.list_tool_invocations()?.into_iter().any(|invocation| {
            invocation.id == continuation.tool_invocation_id
                && invocation.run_id == Some(continuation.run_id)
                && invocation.status == ToolExecutionStatus::Succeeded
        });
        if !invocation_exists {
            return Err(EventStoreError::InvalidState(format!(
                "agent run continuation requires succeeded tool invocation {}",
                continuation.tool_invocation_id
            )));
        }
        let already_queued = self
            .list_agent_run_continuations()?
            .into_iter()
            .any(|current| current.tool_invocation_id == continuation.tool_invocation_id);
        if already_queued {
            return Ok(());
        }
        let event = KernelEvent::new(AGENT_RUN_CONTINUATION_QUEUED_EVENT, continuation)?;
        self.append(&event)
    }

    fn list_agent_run_continuations(&self) -> EventStoreResult<Vec<AgentRunContinuationQueued>> {
        let events = self.list_by_type(AGENT_RUN_CONTINUATION_QUEUED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunContinuationQueued>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn recover_expired_agent_runs(
        &self,
        now: DateTime<Utc>,
    ) -> EventStoreResult<AgentRunRecoverySweep> {
        let mut sweep = AgentRunRecoverySweep::default();
        let invocations = self.list_tool_invocations()?;
        for record in self.list_agent_run_records()? {
            let Some(lease_expires_at) = record.lease_expires_at else {
                continue;
            };
            if lease_expires_at > now
                || !matches!(
                    record.status,
                    AgentRunStatus::Running | AgentRunStatus::CancelRequested
                )
            {
                continue;
            }

            if record.cancel_requested || record.status == AgentRunStatus::CancelRequested {
                let finish = AgentRunFinish::new(
                    record.id,
                    AgentRunStatus::Cancelled,
                    Some("Cancelled after the previous worker lease expired.".to_string()),
                    None,
                )
                .map_err(EventStoreError::InvalidState)?;
                self.append_agent_run_finish(&finish)?;
                sweep.cancelled += 1;
                continue;
            }

            let indeterminate = invocations.iter().find(|invocation| {
                invocation.run_id == Some(record.id)
                    && invocation.status == ToolExecutionStatus::Running
            });
            if let Some(invocation) = indeterminate {
                let reason = format!(
                    "Agent run recovery blocked because tool `{}` invocation {} has an indeterminate outcome; DS Agent will not replay a possible side effect automatically.",
                    invocation.tool_id, invocation.id
                );
                let transition = AgentRunTransition::new(
                    record.id,
                    AgentRunStatus::Blocked,
                    reason.clone(),
                    Some(invocation.id),
                )
                .map_err(EventStoreError::InvalidState)?;
                self.append_agent_run_transition(&transition)?;
                let sequence = record
                    .steps
                    .iter()
                    .map(|step| step.sequence)
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                let step = AgentRunStepRecord::new(
                    record.id,
                    sequence,
                    AgentRunStepStatus::Failed,
                    "agent.recovery".to_string(),
                    reason,
                )
                .map_err(EventStoreError::InvalidState)?;
                self.append_agent_run_step(&step)?;
                sweep.blocked += 1;
                continue;
            }

            let previous_worker_id = record.worker_id.clone().ok_or_else(|| {
                EventStoreError::InvalidState(format!(
                    "expired agent run {} is missing its previous worker id",
                    record.id
                ))
            })?;
            let reason = format!(
                "Recovered after worker `{previous_worker_id}` lease expired at {}.",
                lease_expires_at.to_rfc3339()
            );
            let recovery = AgentRunRecovery::new(
                record.id,
                previous_worker_id,
                lease_expires_at,
                reason.clone(),
            )
            .map_err(EventStoreError::InvalidState)?;
            self.append_agent_run_recovery(&recovery)?;
            let sequence = record
                .steps
                .iter()
                .map(|step| step.sequence)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let step = AgentRunStepRecord::new(
                record.id,
                sequence,
                AgentRunStepStatus::Completed,
                "agent.recovery".to_string(),
                reason,
            )
            .map_err(EventStoreError::InvalidState)?;
            self.append_agent_run_step(&step)?;
            sweep.recovered += 1;
        }
        Ok(sweep)
    }

    pub fn append_agent_run_queued_guidance(
        &self,
        guidance: &AgentRunQueuedGuidance,
    ) -> EventStoreResult<()> {
        self.ensure_agent_run_exists(guidance.run_id)?;
        let event = KernelEvent::new(AGENT_RUN_GUIDANCE_QUEUED_EVENT, guidance)?;
        self.append(&event)
    }

    pub fn append_agent_run_guidance_applied(
        &self,
        applied: &AgentRunGuidanceApplied,
    ) -> EventStoreResult<bool> {
        self.ensure_agent_run_exists(applied.run_id)?;
        let guidance_exists = self.list_agent_run_guidance()?.into_iter().any(|guidance| {
            guidance.id == applied.guidance_id && guidance.run_id == applied.run_id
        });
        if !guidance_exists {
            return Err(EventStoreError::NotFound(format!(
                "agent run guidance {}",
                applied.guidance_id
            )));
        }
        if self
            .list_agent_run_guidance_applications()?
            .into_iter()
            .any(|current| current.guidance_id == applied.guidance_id)
        {
            return Ok(false);
        }
        let event = KernelEvent::new(AGENT_RUN_GUIDANCE_APPLIED_EVENT, applied)?;
        self.append(&event)?;
        Ok(true)
    }

    fn list_agent_run_guidance(&self) -> EventStoreResult<Vec<AgentRunQueuedGuidance>> {
        let events = self.list_by_type(AGENT_RUN_GUIDANCE_QUEUED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunQueuedGuidance>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    fn list_agent_run_guidance_applications(
        &self,
    ) -> EventStoreResult<Vec<AgentRunGuidanceApplied>> {
        let events = self.list_by_type(AGENT_RUN_GUIDANCE_APPLIED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunGuidanceApplied>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn list_unapplied_agent_run_guidance(
        &self,
        run_id: Uuid,
    ) -> EventStoreResult<Vec<AgentRunQueuedGuidance>> {
        self.ensure_agent_run_exists(run_id)?;
        let applied_ids = self
            .list_agent_run_guidance_applications()?
            .into_iter()
            .filter(|applied| applied.run_id == run_id)
            .map(|applied| applied.guidance_id)
            .collect::<std::collections::HashSet<_>>();
        let mut guidance = self
            .list_agent_run_guidance()?
            .into_iter()
            .filter(|guidance| guidance.run_id == run_id && !applied_ids.contains(&guidance.id))
            .collect::<Vec<_>>();
        guidance.sort_by_key(|item| item.queued_at);
        Ok(guidance)
    }

    pub fn append_agent_run_cancel_request(
        &self,
        cancel: &AgentRunCancelRequest,
    ) -> EventStoreResult<()> {
        self.ensure_agent_run_exists(cancel.run_id)?;
        let event = KernelEvent::new(AGENT_RUN_CANCEL_REQUESTED_EVENT, cancel)?;
        self.append(&event)
    }

    fn list_agent_run_cancel_requests(&self) -> EventStoreResult<Vec<AgentRunCancelRequest>> {
        let events = self.list_by_type(AGENT_RUN_CANCEL_REQUESTED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunCancelRequest>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_agent_run_transition(
        &self,
        transition: &AgentRunTransition,
    ) -> EventStoreResult<()> {
        let record = self
            .list_agent_run_records()?
            .into_iter()
            .find(|record| record.id == transition.run_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("agent run {}", transition.run_id)))?;
        if record.cancel_requested
            || matches!(
                record.status,
                AgentRunStatus::Completed | AgentRunStatus::Failed | AgentRunStatus::Cancelled
            )
        {
            return Err(EventStoreError::InvalidState(format!(
                "agent run {} cannot transition from status {:?}",
                record.id, record.status
            )));
        }
        let event = KernelEvent::new(AGENT_RUN_TRANSITIONED_EVENT, transition)?;
        self.append(&event)
    }

    fn list_agent_run_transitions(&self) -> EventStoreResult<Vec<AgentRunTransition>> {
        let events = self.list_by_type(AGENT_RUN_TRANSITIONED_EVENT, 1000)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunTransition>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn claim_agent_run_resource(
        &self,
        claim: AgentRunResourceClaim,
    ) -> EventStoreResult<AgentRunResourceClaim> {
        if let Some(run_id) = claim.run_id {
            let record = self
                .list_agent_run_records()?
                .into_iter()
                .find(|record| record.id == run_id)
                .ok_or_else(|| EventStoreError::NotFound(format!("agent run {run_id}")))?;
            if record.cancel_requested
                || matches!(
                    record.status,
                    AgentRunStatus::Completed | AgentRunStatus::Failed | AgentRunStatus::Cancelled
                )
            {
                return Err(EventStoreError::InvalidState(format!(
                    "agent run {} cannot claim resources from status {:?}",
                    record.id, record.status
                )));
            }
        }

        let active_claims = self.list_active_agent_run_resource_claims()?;
        if let Some(existing) = active_claims.iter().find(|existing| {
            existing.tool_invocation_id == claim.tool_invocation_id
                && existing.resource_key == claim.resource_key
                && existing.access == claim.access
        }) {
            return Ok(existing.clone());
        }
        if let Some(conflict) = active_claims.iter().find(|existing| {
            existing.resource_key == claim.resource_key
                && (existing.access == AgentRunResourceAccess::Write
                    || claim.access == AgentRunResourceAccess::Write)
        }) {
            return Err(EventStoreError::InvalidState(format!(
                "resource `{}` is already claimed by {}",
                claim.resource_key,
                conflict
                    .run_id
                    .map(|run_id| format!("agent run {run_id}"))
                    .unwrap_or_else(|| "another tool invocation".to_string())
            )));
        }

        let event = KernelEvent::new(AGENT_RUN_RESOURCE_CLAIMED_EVENT, &claim)?;
        self.append(&event)?;
        Ok(claim)
    }

    fn list_agent_run_resource_claims(&self) -> EventStoreResult<Vec<AgentRunResourceClaim>> {
        let events = self.list_by_type(AGENT_RUN_RESOURCE_CLAIMED_EVENT, 1000)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunResourceClaim>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_agent_run_resource_release(
        &self,
        release: &AgentRunResourceRelease,
    ) -> EventStoreResult<()> {
        let claim = self
            .list_agent_run_resource_claims()?
            .into_iter()
            .find(|claim| claim.id == release.claim_id)
            .ok_or_else(|| {
                EventStoreError::NotFound(format!("agent run resource claim {}", release.claim_id))
            })?;
        if claim.run_id != release.run_id
            || claim.tool_invocation_id != release.tool_invocation_id
            || claim.resource_key != release.resource_key
        {
            return Err(EventStoreError::InvalidState(
                "agent run resource release does not match its claim".to_string(),
            ));
        }
        if self
            .list_agent_run_resource_releases()?
            .into_iter()
            .any(|existing| existing.claim_id == release.claim_id)
        {
            return Ok(());
        }
        let event = KernelEvent::new(AGENT_RUN_RESOURCE_RELEASED_EVENT, release)?;
        self.append(&event)
    }

    fn list_agent_run_resource_releases(&self) -> EventStoreResult<Vec<AgentRunResourceRelease>> {
        let events = self.list_by_type(AGENT_RUN_RESOURCE_RELEASED_EVENT, 1000)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunResourceRelease>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn list_active_agent_run_resource_claims(
        &self,
    ) -> EventStoreResult<Vec<AgentRunResourceClaim>> {
        let released_claim_ids = self
            .list_agent_run_resource_releases()?
            .into_iter()
            .map(|release| release.claim_id)
            .collect::<std::collections::HashSet<_>>();
        let now = Utc::now();
        let mut latest_by_resource =
            std::collections::HashMap::<(Uuid, String, bool), AgentRunResourceClaim>::new();
        for claim in self
            .list_agent_run_resource_claims()?
            .into_iter()
            .filter(|claim| claim.lease_expires_at > now && !released_claim_ids.contains(&claim.id))
        {
            let key = (
                claim.tool_invocation_id,
                claim.resource_key.clone(),
                claim.access == AgentRunResourceAccess::Write,
            );
            latest_by_resource
                .entry(key)
                .and_modify(|current| {
                    if claim.claimed_at > current.claimed_at {
                        *current = claim.clone();
                    }
                })
                .or_insert(claim);
        }
        let mut claims = latest_by_resource.into_values().collect::<Vec<_>>();
        claims.sort_by_key(|claim| claim.claimed_at);
        Ok(claims)
    }

    fn renew_agent_run_resources_for_run(
        &self,
        run_id: Uuid,
        lease_seconds: i64,
    ) -> EventStoreResult<usize> {
        let claims = self
            .list_active_agent_run_resource_claims()?
            .into_iter()
            .filter(|claim| claim.run_id == Some(run_id))
            .collect::<Vec<_>>();
        for claim in &claims {
            let renewed = AgentRunResourceClaim::new(
                claim.run_id,
                claim.tool_invocation_id,
                claim.resource_key.clone(),
                claim.access,
                lease_seconds,
            )
            .map_err(EventStoreError::InvalidState)?;
            let event = KernelEvent::new(AGENT_RUN_RESOURCE_CLAIMED_EVENT, &renewed)?;
            self.append(&event)?;
        }
        Ok(claims.len())
    }

    pub fn release_agent_run_resources_for_invocation(
        &self,
        tool_invocation_id: Uuid,
        outcome: String,
    ) -> EventStoreResult<usize> {
        let released_claim_ids = self
            .list_agent_run_resource_releases()?
            .into_iter()
            .map(|release| release.claim_id)
            .collect::<std::collections::HashSet<_>>();
        let claims = self
            .list_agent_run_resource_claims()?
            .into_iter()
            .filter(|claim| {
                claim.tool_invocation_id == tool_invocation_id
                    && !released_claim_ids.contains(&claim.id)
            })
            .collect::<Vec<_>>();
        for claim in &claims {
            let release = AgentRunResourceRelease::new(claim, outcome.clone())
                .map_err(EventStoreError::InvalidState)?;
            self.append_agent_run_resource_release(&release)?;
        }
        Ok(claims.len())
    }

    pub fn append_agent_run_finish(&self, finish: &AgentRunFinish) -> EventStoreResult<()> {
        self.ensure_agent_run_exists(finish.run_id)?;
        let event = KernelEvent::new(AGENT_RUN_FINISHED_EVENT, finish)?;
        self.append(&event)
    }

    fn list_agent_run_finishes(&self) -> EventStoreResult<Vec<AgentRunFinish>> {
        let events = self.list_by_type(AGENT_RUN_FINISHED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunFinish>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn append_agent_run_step(&self, step: &AgentRunStepRecord) -> EventStoreResult<()> {
        self.ensure_agent_run_exists(step.run_id)?;
        let event = KernelEvent::new(AGENT_RUN_STEP_RECORDED_EVENT, step)?;
        self.append(&event)
    }

    fn list_agent_run_steps(&self) -> EventStoreResult<Vec<AgentRunStepRecord>> {
        let events = self.list_by_type(AGENT_RUN_STEP_RECORDED_EVENT, 1000)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunStepRecord>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn append_agent_run_artifact(
        &self,
        artifact: &AgentRunArtifactRecord,
    ) -> EventStoreResult<()> {
        self.ensure_agent_run_exists(artifact.run_id)?;
        let event = KernelEvent::new(AGENT_RUN_ARTIFACT_RECORDED_EVENT, artifact)?;
        self.append(&event)
    }

    fn list_agent_run_artifacts(&self) -> EventStoreResult<Vec<AgentRunArtifactRecord>> {
        let events = self.list_by_type(AGENT_RUN_ARTIFACT_RECORDED_EVENT, 1000)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentRunArtifactRecord>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_expert_attempt_result(
        &self,
        result: &ExpertAttemptResult,
    ) -> EventStoreResult<()> {
        let records = self.list_agent_run_records()?;
        let record = records
            .iter()
            .find(|record| record.id == result.run_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("agent run {}", result.run_id)))?;
        let contract = record.expert_contract.as_ref().ok_or_else(|| {
            EventStoreError::InvalidState("expert result requires an expert attempt".to_string())
        })?;
        if result.parent_run_id != contract.parent_run_id
            || result.key != contract.key
            || result.role != contract.role
            || result.attempt != contract.attempt
            || result.parent_input_revision != contract.parent_input_revision
        {
            return Err(EventStoreError::InvalidState(
                "expert result does not match its immutable attempt contract".to_string(),
            ));
        }
        if self
            .list_expert_attempt_results()?
            .iter()
            .any(|existing| existing.run_id == result.run_id)
        {
            return Err(EventStoreError::InvalidState(
                "expert attempt result is immutable and already recorded".to_string(),
            ));
        }
        let event = KernelEvent::new(EXPERT_ATTEMPT_RESULT_RECORDED_EVENT, result)?;
        self.append(&event)
    }

    pub fn list_expert_attempt_results(&self) -> EventStoreResult<Vec<ExpertAttemptResult>> {
        self.list_by_type(EXPERT_ATTEMPT_RESULT_RECORDED_EVENT, 1000)?
            .into_iter()
            .map(|event| serde_json::from_str(&event.payload_json).map_err(Into::into))
            .collect()
    }

    pub fn append_expert_merge_receipt(
        &self,
        receipt: &ExpertMergeReceipt,
    ) -> EventStoreResult<()> {
        let records = self.list_agent_run_records()?;
        let parent = records
            .iter()
            .find(|record| record.id == receipt.parent_run_id)
            .ok_or_else(|| {
                EventStoreError::NotFound(format!("agent run {}", receipt.parent_run_id))
            })?;
        if parent.role != AgentRunRole::Parent
            || parent_input_revision(&parent.prompt) != receipt.parent_input_revision
        {
            return Err(EventStoreError::InvalidState(
                "expert merge parent revision is stale".to_string(),
            ));
        }
        let production = records
            .iter()
            .find(|record| record.id == receipt.production_run_id)
            .and_then(|record| record.expert_result.as_ref())
            .ok_or_else(|| {
                EventStoreError::InvalidState(
                    "expert merge production result is missing".to_string(),
                )
            })?;
        let review = records
            .iter()
            .find(|record| record.id == receipt.review_run_id)
            .and_then(|record| record.expert_result.as_ref())
            .ok_or_else(|| {
                EventStoreError::InvalidState("expert merge review result is missing".to_string())
            })?;
        let review_accepted = review.review.as_ref().is_some_and(|verdict| {
            verdict.decision == crate::kernel::expert_team::ExpertReviewDecision::Accept
                && verdict.target_revision == production.output_revision
        });
        if !production.passed()
            || !review.passed()
            || production.output_revision != receipt.production_revision
            || !review_accepted
        {
            return Err(EventStoreError::InvalidState(
                "expert merge requires a passed production revision and an exact accepted review"
                    .to_string(),
            ));
        }
        if self.list_expert_merge_receipts()?.iter().any(|existing| {
            existing.parent_run_id == receipt.parent_run_id
                && existing.parent_input_revision == receipt.parent_input_revision
        }) {
            return Err(EventStoreError::InvalidState(
                "expert parent revision already has a merge receipt".to_string(),
            ));
        }
        let event = KernelEvent::new(EXPERT_MERGE_RECORDED_EVENT, receipt)?;
        self.append(&event)
    }

    pub fn list_expert_merge_receipts(&self) -> EventStoreResult<Vec<ExpertMergeReceipt>> {
        self.list_by_type(EXPERT_MERGE_RECORDED_EVENT, 500)?
            .into_iter()
            .map(|event| serde_json::from_str(&event.payload_json).map_err(Into::into))
            .collect()
    }

    pub fn list_agent_run_records(&self) -> EventStoreResult<Vec<AgentRunRecord>> {
        let mut expert_result_by_run_id = std::collections::HashMap::new();
        for result in self.list_expert_attempt_results()? {
            expert_result_by_run_id.insert(result.run_id, result);
        }
        let mut expert_merge_by_parent_id = std::collections::HashMap::new();
        for receipt in self.list_expert_merge_receipts()? {
            expert_merge_by_parent_id.insert(receipt.parent_run_id, receipt);
        }
        let mut applied_by_guidance_id =
            std::collections::HashMap::<Uuid, AgentRunGuidanceApplied>::new();
        for applied in self.list_agent_run_guidance_applications()? {
            applied_by_guidance_id
                .entry(applied.guidance_id)
                .and_modify(|current| {
                    if applied.applied_at > current.applied_at {
                        *current = applied.clone();
                    }
                })
                .or_insert(applied);
        }
        let mut guidance_by_run_id =
            std::collections::HashMap::<Uuid, Vec<AgentRunQueuedGuidance>>::new();
        for mut guidance in self.list_agent_run_guidance()? {
            guidance.applied_at = applied_by_guidance_id
                .remove(&guidance.id)
                .filter(|applied| applied.run_id == guidance.run_id)
                .map(|applied| applied.applied_at);
            guidance_by_run_id
                .entry(guidance.run_id)
                .or_default()
                .push(guidance);
        }
        for guidance in guidance_by_run_id.values_mut() {
            guidance.sort_by_key(|item| item.queued_at);
        }

        let mut cancel_by_run_id = std::collections::HashMap::<Uuid, AgentRunCancelRequest>::new();
        for cancel in self.list_agent_run_cancel_requests()? {
            cancel_by_run_id
                .entry(cancel.run_id)
                .and_modify(|current| {
                    if cancel.requested_at > current.requested_at {
                        *current = cancel.clone();
                    }
                })
                .or_insert(cancel);
        }

        let mut claim_by_run_id = std::collections::HashMap::<Uuid, AgentRunClaim>::new();
        for claim in self.list_agent_run_claims()? {
            claim_by_run_id
                .entry(claim.run_id)
                .and_modify(|current| {
                    if claim.claimed_at > current.claimed_at {
                        *current = claim.clone();
                    }
                })
                .or_insert(claim);
        }

        let mut recovery_count_by_run_id = std::collections::HashMap::<Uuid, usize>::new();
        let mut recovery_by_run_id = std::collections::HashMap::<Uuid, AgentRunRecovery>::new();
        for recovery in self.list_agent_run_recoveries()? {
            *recovery_count_by_run_id.entry(recovery.run_id).or_default() += 1;
            recovery_by_run_id
                .entry(recovery.run_id)
                .and_modify(|current| {
                    if recovery.recovered_at > current.recovered_at {
                        *current = recovery.clone();
                    }
                })
                .or_insert(recovery);
        }

        let mut execution_context_by_run_id =
            std::collections::HashMap::<Uuid, AgentRunExecutionContext>::new();
        for context in self.list_agent_run_execution_contexts()? {
            execution_context_by_run_id
                .entry(context.run_id)
                .and_modify(|current| {
                    if context.recorded_at > current.recorded_at {
                        *current = context.clone();
                    }
                })
                .or_insert(context);
        }

        let mut continuation_count_by_run_id = std::collections::HashMap::<Uuid, usize>::new();
        let mut continuation_by_run_id =
            std::collections::HashMap::<Uuid, AgentRunContinuationQueued>::new();
        for continuation in self.list_agent_run_continuations()? {
            *continuation_count_by_run_id
                .entry(continuation.run_id)
                .or_default() += 1;
            continuation_by_run_id
                .entry(continuation.run_id)
                .and_modify(|current| {
                    if continuation.queued_at > current.queued_at {
                        *current = continuation.clone();
                    }
                })
                .or_insert(continuation);
        }

        let mut transition_by_run_id = std::collections::HashMap::<Uuid, AgentRunTransition>::new();
        for transition in self.list_agent_run_transitions()? {
            transition_by_run_id
                .entry(transition.run_id)
                .and_modify(|current| {
                    if transition.transitioned_at > current.transitioned_at {
                        *current = transition.clone();
                    }
                })
                .or_insert(transition);
        }

        let mut finish_by_run_id = std::collections::HashMap::<Uuid, AgentRunFinish>::new();
        for finish in self.list_agent_run_finishes()? {
            finish_by_run_id
                .entry(finish.run_id)
                .and_modify(|current| {
                    if finish.finished_at > current.finished_at {
                        *current = finish.clone();
                    }
                })
                .or_insert(finish);
        }

        let mut steps_by_run_id = std::collections::HashMap::<Uuid, Vec<AgentRunStepRecord>>::new();
        for step in self.list_agent_run_steps()? {
            steps_by_run_id.entry(step.run_id).or_default().push(step);
        }
        for steps in steps_by_run_id.values_mut() {
            steps.sort_by_key(|step| (step.sequence, step.recorded_at));
        }

        let mut artifacts_by_run_id =
            std::collections::HashMap::<Uuid, Vec<AgentRunArtifactRecord>>::new();
        for artifact in self.list_agent_run_artifacts()? {
            artifacts_by_run_id
                .entry(artifact.run_id)
                .or_default()
                .push(artifact);
        }
        for artifacts in artifacts_by_run_id.values_mut() {
            artifacts.sort_by_key(|artifact| artifact.created_at);
        }

        self.list_agent_run_starts()?
            .into_iter()
            .map(|start| {
                let queued_guidance = guidance_by_run_id.remove(&start.id).unwrap_or_default();
                let latest_cancel = cancel_by_run_id.remove(&start.id);
                let latest_claim = claim_by_run_id.remove(&start.id);
                let latest_recovery = recovery_by_run_id.remove(&start.id);
                let recovery_count = recovery_count_by_run_id.remove(&start.id).unwrap_or(0);
                let execution_context = execution_context_by_run_id.remove(&start.id);
                let latest_continuation = continuation_by_run_id.remove(&start.id);
                let continuation_count =
                    continuation_count_by_run_id.remove(&start.id).unwrap_or(0);
                let latest_transition = transition_by_run_id.remove(&start.id);
                let latest_finish = finish_by_run_id.remove(&start.id);
                let steps = steps_by_run_id.remove(&start.id).unwrap_or_default();
                let artifacts = artifacts_by_run_id.remove(&start.id).unwrap_or_default();
                let expert_result = expert_result_by_run_id.remove(&start.id);
                let expert_merge_receipt = expert_merge_by_parent_id.remove(&start.id);
                let mut updated_at = start.started_at;
                let latest_claim_at = latest_claim.as_ref().map(|claim| claim.claimed_at);
                let latest_recovery_at = latest_recovery
                    .as_ref()
                    .map(|recovery| recovery.recovered_at);
                let latest_continuation_at = latest_continuation
                    .as_ref()
                    .map(|continuation| continuation.queued_at);
                let latest_queue_at = match (latest_recovery_at, latest_continuation_at) {
                    (Some(recovered_at), Some(continued_at)) => {
                        Some(recovered_at.max(continued_at))
                    }
                    (Some(recovered_at), None) => Some(recovered_at),
                    (None, Some(continued_at)) => Some(continued_at),
                    (None, None) => None,
                };
                let latest_transition_at = latest_transition
                    .as_ref()
                    .map(|transition| transition.transitioned_at);
                let transition_is_latest = latest_transition_at.is_some_and(|transitioned_at| {
                    latest_claim_at.is_none_or(|claimed_at| transitioned_at >= claimed_at)
                        && latest_queue_at.is_none_or(|queued_at| transitioned_at >= queued_at)
                });
                let queue_is_latest = latest_queue_at.is_some_and(|queued_at| {
                    latest_claim_at.is_none_or(|claimed_at| queued_at >= claimed_at)
                        && latest_transition_at
                            .is_none_or(|transitioned_at| queued_at > transitioned_at)
                });
                let mut status = latest_finish
                    .as_ref()
                    .map(|finish| finish.status)
                    .unwrap_or_else(|| {
                        if transition_is_latest {
                            latest_transition
                                .as_ref()
                                .map(|transition| transition.status)
                                .unwrap_or(start.initial_status)
                        } else if queue_is_latest {
                            AgentRunStatus::Queued
                        } else if latest_claim.is_some() {
                            AgentRunStatus::Running
                        } else {
                            start.initial_status
                        }
                    });
                let finished_at = latest_finish.as_ref().map(|finish| finish.finished_at);
                let finish_summary = latest_finish
                    .as_ref()
                    .and_then(|finish| finish.summary.clone());
                let finish_error = latest_finish
                    .as_ref()
                    .and_then(|finish| finish.error.clone());
                let mut status_reason = if latest_finish.is_none() && transition_is_latest {
                    latest_transition
                        .as_ref()
                        .map(|transition| transition.reason.clone())
                } else if latest_finish.is_none() && queue_is_latest {
                    match (&latest_recovery, &latest_continuation) {
                        (Some(recovery), Some(continuation))
                            if recovery.recovered_at >= continuation.queued_at =>
                        {
                            Some(recovery.reason.clone())
                        }
                        (_, Some(continuation)) => Some(continuation.reason.clone()),
                        (Some(recovery), None) => Some(recovery.reason.clone()),
                        (None, None) => None,
                    }
                } else {
                    None
                };
                let mut waiting_tool_invocation_id =
                    if latest_finish.is_none() && transition_is_latest {
                        latest_transition
                            .as_ref()
                            .and_then(|transition| transition.tool_invocation_id)
                    } else {
                        None
                    };

                if let Some(finished_at) = finished_at {
                    updated_at = updated_at.max(finished_at);
                }
                for guidance in &queued_guidance {
                    updated_at = updated_at.max(guidance.queued_at);
                    if let Some(applied_at) = guidance.applied_at {
                        updated_at = updated_at.max(applied_at);
                    }
                }
                for step in &steps {
                    updated_at = updated_at.max(step.recorded_at);
                }
                for artifact in &artifacts {
                    updated_at = updated_at.max(artifact.created_at);
                }
                let active_claim = if queue_is_latest {
                    None
                } else if latest_transition
                    .as_ref()
                    .is_some_and(|transition| transition.status == AgentRunStatus::Queued)
                {
                    None
                } else {
                    latest_claim.as_ref()
                };
                let worker_id = active_claim.map(|claim| claim.worker_id.clone());
                let lease_expires_at = active_claim.map(|claim| claim.lease_expires_at);
                if let Some(claim) = &latest_claim {
                    updated_at = updated_at.max(claim.claimed_at);
                }
                if let Some(recovery) = &latest_recovery {
                    updated_at = updated_at.max(recovery.recovered_at);
                }
                if let Some(context) = &execution_context {
                    updated_at = updated_at.max(context.recorded_at);
                }
                if let Some(continuation) = &latest_continuation {
                    updated_at = updated_at.max(continuation.queued_at);
                }
                if let Some(transition) = &latest_transition {
                    updated_at = updated_at.max(transition.transitioned_at);
                }
                let cancel_requested = latest_cancel.is_some();
                let cancel_reason = latest_cancel.as_ref().map(|cancel| cancel.reason.clone());
                if let Some(cancel) = &latest_cancel {
                    updated_at = updated_at.max(cancel.requested_at);
                    if !matches!(
                        latest_finish.as_ref().map(|finish| finish.status),
                        Some(AgentRunStatus::Cancelled | AgentRunStatus::Failed)
                    ) {
                        status = AgentRunStatus::CancelRequested;
                        status_reason = Some(cancel.reason.clone());
                        waiting_tool_invocation_id = None;
                    }
                }

                Ok(AgentRunRecord {
                    id: start.id,
                    conversation_id: start.conversation_id,
                    prompt: start.prompt,
                    execution_prompt: execution_context
                        .as_ref()
                        .map(|context| context.execution_prompt.clone()),
                    execution_context_recorded_at: execution_context
                        .as_ref()
                        .map(|context| context.recorded_at),
                    attachment_count: start.attachment_count,
                    role: start.role,
                    parent_run_id: start.parent_run_id,
                    subtask_key: start.subtask_key,
                    expert_contract: start.expert_contract,
                    expert_result,
                    expert_merge_receipt,
                    status,
                    worker_id,
                    lease_expires_at,
                    recovery_count,
                    last_recovered_at: latest_recovery
                        .as_ref()
                        .map(|recovery| recovery.recovered_at),
                    recovery_reason: latest_recovery
                        .as_ref()
                        .map(|recovery| recovery.reason.clone()),
                    continuation_count,
                    continuation_queued_at: latest_continuation
                        .as_ref()
                        .map(|continuation| continuation.queued_at),
                    continuation_tool_invocation_id: latest_continuation
                        .as_ref()
                        .map(|continuation| continuation.tool_invocation_id),
                    queued_guidance,
                    steps,
                    artifacts,
                    cancel_requested,
                    cancel_reason,
                    status_reason,
                    waiting_tool_invocation_id,
                    started_at: start.started_at,
                    updated_at,
                    finished_at,
                    finish_summary,
                    finish_error,
                })
            })
            .collect()
    }

    fn ensure_agent_run_exists(&self, run_id: Uuid) -> EventStoreResult<()> {
        let exists = self
            .list_agent_run_starts()?
            .into_iter()
            .any(|record| record.id == run_id);
        if exists {
            Ok(())
        } else {
            Err(EventStoreError::NotFound(format!("agent run {run_id}")))
        }
    }

    pub fn import_task_records(
        &self,
        records: &[TaskRecord],
    ) -> EventStoreResult<WorkPackageImportSummary> {
        let mut existing_ids = self
            .list_task_records()?
            .into_iter()
            .map(|record| record.id)
            .collect::<std::collections::HashSet<_>>();
        let mut summary = WorkPackageImportSummary {
            imported: 0,
            skipped: 0,
            memory_candidates: WorkPackageMemoryCandidateImportSummary {
                imported: 0,
                skipped: 0,
            },
            operations_briefing_runs: WorkPackageOperationsBriefingImportSummary {
                imported: 0,
                skipped: 0,
            },
            workflow_templates: WorkPackageWorkflowTemplateImportSummary {
                imported: 0,
                skipped: 0,
            },
        };

        for record in records {
            if existing_ids.contains(&record.id) {
                summary.skipped += 1;
                continue;
            }

            self.append_task_record(record)?;
            let memory = MemoryRecord::from_task_record(record);
            self.append_memory_record(&memory)?;
            existing_ids.insert(record.id);
            summary.imported += 1;
        }

        Ok(summary)
    }

    pub fn preview_work_package_import(
        &self,
        package: &WorkPackage,
    ) -> EventStoreResult<WorkPackageImportPreview> {
        let existing_ids = self
            .list_task_records()?
            .into_iter()
            .map(|record| record.id)
            .collect::<std::collections::HashSet<_>>();
        let (total, skipped) = preview_import_counts(
            existing_ids,
            package.task_records.iter().map(|record| record.id),
        );
        let existing_candidate_ids = self
            .list_memory_candidates()?
            .into_iter()
            .map(|candidate| candidate.id)
            .collect::<std::collections::HashSet<_>>();
        let (total_candidates, skipped_candidates) = preview_import_counts(
            existing_candidate_ids,
            package
                .memory_candidates
                .iter()
                .map(|candidate| candidate.id),
        );
        let existing_briefing_run_ids = self
            .list_operations_briefing_runs()?
            .into_iter()
            .map(|run| run.id)
            .collect::<std::collections::HashSet<_>>();
        let (total_briefing_runs, skipped_briefing_runs) = preview_import_counts(
            existing_briefing_run_ids,
            package.operations_briefing_runs.iter().map(|run| run.id),
        );
        let existing_template_ids = self
            .list_workflow_template_packages()?
            .into_iter()
            .map(|template| template.id)
            .collect::<std::collections::HashSet<_>>();
        let (total_templates, skipped_templates) = preview_import_counts(
            existing_template_ids,
            package
                .workflow_templates
                .iter()
                .map(|template| template.id.clone()),
        );

        Ok(WorkPackageImportPreview {
            task_records: WorkPackageTaskImportPreview {
                total,
                new: total.saturating_sub(skipped),
                skipped,
            },
            memory_candidates: WorkPackageMemoryCandidateImportPreview {
                total: total_candidates,
                new: total_candidates.saturating_sub(skipped_candidates),
                skipped: skipped_candidates,
                review_supported: true,
            },
            operations_briefing_runs: WorkPackageOperationsBriefingImportPreview {
                total: total_briefing_runs,
                new: total_briefing_runs.saturating_sub(skipped_briefing_runs),
                skipped: skipped_briefing_runs,
                replay_supported: true,
            },
            workflow_templates: WorkPackageWorkflowTemplateImportPreview {
                total: total_templates,
                new: total_templates.saturating_sub(skipped_templates),
                skipped: skipped_templates,
                import_supported: true,
            },
        })
    }

    pub fn append_workflow_template_package(
        &self,
        template: &WorkflowTemplatePackage,
    ) -> EventStoreResult<bool> {
        let existing = self
            .list_workflow_template_packages()?
            .into_iter()
            .any(|local_template| local_template.id == template.id);
        if existing {
            return Ok(false);
        }

        let event = KernelEvent::new(WORKFLOW_TEMPLATE_PACKAGE_IMPORTED_EVENT, template)?;
        self.append(&event)?;
        Ok(true)
    }

    pub fn list_workflow_template_packages(
        &self,
    ) -> EventStoreResult<Vec<WorkflowTemplatePackage>> {
        let events = self.list_by_type(WORKFLOW_TEMPLATE_PACKAGE_IMPORTED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<WorkflowTemplatePackage>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn import_workflow_template_packages(
        &self,
        templates: &[WorkflowTemplatePackage],
    ) -> EventStoreResult<WorkPackageWorkflowTemplateImportSummary> {
        let mut summary = WorkPackageWorkflowTemplateImportSummary {
            imported: 0,
            skipped: 0,
        };

        for template in templates {
            if self.append_workflow_template_package(template)? {
                summary.imported += 1;
            } else {
                summary.skipped += 1;
            }
        }

        Ok(summary)
    }

    pub fn append_memory_record(&self, record: &MemoryRecord) -> EventStoreResult<bool> {
        if let Some(source_id) = record.source_id {
            let existing = self.list_memory_records()?.into_iter().any(|memory| {
                memory.source == record.source && memory.source_id == Some(source_id)
            });
            if existing {
                return Ok(false);
            }
        }

        let mut persisted_record = record.clone();
        persisted_record.linked_memory_ids = Vec::new();
        persisted_record.linked_memories = Vec::new();
        persisted_record.search_match = MemorySearchMatch::direct();
        let event = KernelEvent::new(MEMORY_RECORD_CREATED_EVENT, &persisted_record)?;
        self.append(&event)?;
        Ok(true)
    }

    fn ensure_memory_record_source_not_already_written(
        &self,
        record: &MemoryRecord,
    ) -> EventStoreResult<()> {
        if record.source_id.is_none() {
            return Ok(());
        }

        let exists = self
            .list_memory_records()?
            .into_iter()
            .any(|memory| memory.source == record.source && memory.source_id == record.source_id);
        if exists {
            return Err(EventStoreError::InvalidState(
                "accepted memory candidate was already written".to_string(),
            ));
        }

        Ok(())
    }

    pub fn list_memory_records(&self) -> EventStoreResult<Vec<MemoryRecord>> {
        self.list_memory_records_at(Utc::now())
    }

    pub fn list_memory_records_at(
        &self,
        now: DateTime<Utc>,
    ) -> EventStoreResult<Vec<MemoryRecord>> {
        let deleted_memory_ids = self
            .list_memory_record_deletions()?
            .into_iter()
            .map(|deletion| deletion.memory_id)
            .collect::<std::collections::HashSet<_>>();
        let latest_updates = self.list_memory_record_updates()?.into_iter().fold(
            std::collections::HashMap::new(),
            |mut updates, update| {
                updates.entry(update.memory_id).or_insert(update);
                updates
            },
        );
        let events = self.list_by_type(MEMORY_RECORD_CREATED_EVENT, 500)?;
        let memories = events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<MemoryRecord>(&event.payload_json).map_err(Into::into)
            })
            .map(|record| {
                record.map(|memory| {
                    latest_updates
                        .get(&memory.id)
                        .map(|update| update.apply_to(&memory))
                        .unwrap_or(memory)
                })
            })
            .filter(|record| {
                record
                    .as_ref()
                    .map(|memory| !deleted_memory_ids.contains(&memory.id))
                    .unwrap_or(true)
            })
            .filter(|record| {
                record
                    .as_ref()
                    .map(|memory| !memory.is_expired_at(now))
                    .unwrap_or(true)
            })
            .collect::<EventStoreResult<Vec<_>>>()?;

        self.with_memory_record_links(memories)
    }

    fn with_memory_record_links(
        &self,
        memories: Vec<MemoryRecord>,
    ) -> EventStoreResult<Vec<MemoryRecord>> {
        let visible_memory_ids = memories
            .iter()
            .map(|memory| memory.id)
            .collect::<std::collections::HashSet<_>>();
        let summaries_by_id = memories
            .iter()
            .map(|memory| (memory.id, MemoryRecordLinkSummary::from(memory)))
            .collect::<std::collections::HashMap<_, _>>();
        let mut linked_ids_by_memory_id: std::collections::HashMap<Uuid, Vec<Uuid>> =
            std::collections::HashMap::new();
        let mut linked_summaries_by_memory_id: std::collections::HashMap<
            Uuid,
            Vec<MemoryRecordLinkSummary>,
        > = std::collections::HashMap::new();

        for link in self.list_memory_record_links()? {
            if link.source_memory_id == link.target_memory_id {
                continue;
            }
            if !visible_memory_ids.contains(&link.source_memory_id)
                || !visible_memory_ids.contains(&link.target_memory_id)
            {
                continue;
            }

            push_unique_link(
                &mut linked_ids_by_memory_id,
                link.source_memory_id,
                link.target_memory_id,
            );
            push_unique_link(
                &mut linked_ids_by_memory_id,
                link.target_memory_id,
                link.source_memory_id,
            );
            if let Some(summary) = summaries_by_id.get(&link.target_memory_id) {
                push_unique_link_summary(
                    &mut linked_summaries_by_memory_id,
                    link.source_memory_id,
                    summary.clone().with_link_context(link.relation, &link.note),
                );
            }
            if let Some(summary) = summaries_by_id.get(&link.source_memory_id) {
                push_unique_link_summary(
                    &mut linked_summaries_by_memory_id,
                    link.target_memory_id,
                    summary.clone().with_link_context(link.relation, &link.note),
                );
            }
        }

        Ok(memories
            .into_iter()
            .map(|mut memory| {
                let linked_memory_ids = linked_ids_by_memory_id
                    .remove(&memory.id)
                    .unwrap_or_default();
                let linked_memories = linked_summaries_by_memory_id
                    .remove(&memory.id)
                    .unwrap_or_default();
                memory.linked_memory_ids = linked_memory_ids;
                memory.linked_memories = linked_memories;
                memory
            })
            .collect())
    }

    pub fn list_memory_record_updates(&self) -> EventStoreResult<Vec<MemoryRecordUpdate>> {
        let events = self.list_by_type(MEMORY_RECORD_UPDATED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<MemoryRecordUpdate>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn update_memory_record(
        &self,
        memory_id: Uuid,
        title: String,
        body: String,
        memory_type: crate::kernel::models::MemoryType,
        scope: crate::kernel::models::MemoryScope,
        sensitivity: crate::kernel::models::MemorySensitivity,
        lifecycle: crate::kernel::models::MemoryLifecycle,
        expires_at: Option<DateTime<Utc>>,
        note: String,
    ) -> EventStoreResult<MemoryRecordUpdate> {
        let existing = self
            .list_memory_records()?
            .into_iter()
            .find(|memory| memory.id == memory_id)
            .ok_or_else(|| {
                EventStoreError::NotFound(format!("memory record {memory_id} was not found"))
            })?;
        let update = MemoryRecordUpdate::new(
            memory_id,
            title,
            body,
            memory_type,
            scope,
            sensitivity,
            lifecycle,
            existing.pinned,
            expires_at,
            note,
        )
        .map_err(EventStoreError::InvalidState)?;
        let event = KernelEvent::new(MEMORY_RECORD_UPDATED_EVENT, &update)?;
        self.append(&event)?;
        Ok(update)
    }

    pub fn list_memory_record_deletions(&self) -> EventStoreResult<Vec<MemoryRecordDeletion>> {
        let events = self.list_by_type(MEMORY_RECORD_DELETED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<MemoryRecordDeletion>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn delete_memory_record(
        &self,
        memory_id: Uuid,
        note: String,
    ) -> EventStoreResult<MemoryRecordDeletion> {
        let exists = self
            .list_memory_records()?
            .into_iter()
            .any(|memory| memory.id == memory_id);
        if !exists {
            return Err(EventStoreError::NotFound(format!(
                "memory record {memory_id} was not found"
            )));
        }

        let deletion = MemoryRecordDeletion::new(memory_id, note);
        let event = KernelEvent::new(MEMORY_RECORD_DELETED_EVENT, &deletion)?;
        self.append(&event)?;
        Ok(deletion)
    }

    pub fn record_selected_memory_feedback(
        &self,
        memory_id: Uuid,
        context_receipt_id: Option<Uuid>,
        feedback: MemorySelectedFeedbackKind,
        note: String,
    ) -> EventStoreResult<MemorySelectedFeedback> {
        let exists = self
            .list_memory_records()?
            .into_iter()
            .any(|memory| memory.id == memory_id);
        if !exists {
            return Err(EventStoreError::NotFound(format!(
                "memory record {memory_id} was not found"
            )));
        }

        let feedback = MemorySelectedFeedback::new(memory_id, context_receipt_id, feedback, note);
        let event = KernelEvent::new(MEMORY_SELECTED_FEEDBACK_RECORDED_EVENT, &feedback)?;
        self.append(&event)?;
        Ok(feedback)
    }

    pub fn list_selected_memory_feedback(&self) -> EventStoreResult<Vec<MemorySelectedFeedback>> {
        let events = self.list_by_type(MEMORY_SELECTED_FEEDBACK_RECORDED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<MemorySelectedFeedback>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn record_memory_maintenance_review_action(
        &self,
        memory_id: Uuid,
        action: MemoryMaintenanceActionKind,
        snoozed_until: Option<DateTime<Utc>>,
        note: String,
    ) -> EventStoreResult<MemoryMaintenanceReviewAction> {
        let exists = self
            .list_memory_records()?
            .into_iter()
            .any(|memory| memory.id == memory_id);
        if !exists {
            return Err(EventStoreError::NotFound(format!(
                "memory record {memory_id} was not found"
            )));
        }

        let action = MemoryMaintenanceReviewAction::new(memory_id, action, snoozed_until, note)
            .map_err(EventStoreError::InvalidState)?;
        let event = KernelEvent::new(MEMORY_MAINTENANCE_REVIEW_ACTION_RECORDED_EVENT, &action)?;
        self.append(&event)?;
        Ok(action)
    }

    pub fn list_memory_maintenance_review_actions(
        &self,
    ) -> EventStoreResult<Vec<MemoryMaintenanceReviewAction>> {
        let events = self.list_by_type(MEMORY_MAINTENANCE_REVIEW_ACTION_RECORDED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<MemoryMaintenanceReviewAction>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_memory_record_link(&self, link: &MemoryRecordLink) -> EventStoreResult<()> {
        let visible_memory_ids = self
            .list_memory_records()?
            .into_iter()
            .map(|memory| memory.id)
            .collect::<std::collections::HashSet<_>>();
        if !visible_memory_ids.contains(&link.source_memory_id) {
            return Err(EventStoreError::NotFound(format!(
                "memory record {} was not found",
                link.source_memory_id
            )));
        }
        if !visible_memory_ids.contains(&link.target_memory_id) {
            return Err(EventStoreError::NotFound(format!(
                "memory record {} was not found",
                link.target_memory_id
            )));
        }
        if link.source_memory_id == link.target_memory_id {
            return Err(EventStoreError::InvalidState(
                "memory record link cannot point to itself".to_string(),
            ));
        }

        let duplicate_exists = self
            .list_memory_record_links()?
            .into_iter()
            .any(|existing| {
                existing.relation == link.relation
                    && ((existing.source_memory_id == link.source_memory_id
                        && existing.target_memory_id == link.target_memory_id)
                        || (existing.source_memory_id == link.target_memory_id
                            && existing.target_memory_id == link.source_memory_id))
            });
        if duplicate_exists {
            return Ok(());
        }

        let event = KernelEvent::new(MEMORY_RECORD_LINKED_EVENT, link)?;
        self.append(&event)
    }

    pub fn list_memory_record_links(&self) -> EventStoreResult<Vec<MemoryRecordLink>> {
        let events = self.list_by_type(MEMORY_RECORD_LINKED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<MemoryRecordLink>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn search_memory_records(&self, query: &str) -> EventStoreResult<Vec<MemoryRecord>> {
        self.search_memory_records_at(query, Utc::now())
    }

    pub fn search_memory_records_at(
        &self,
        query: &str,
        now: DateTime<Utc>,
    ) -> EventStoreResult<Vec<MemoryRecord>> {
        let query = query.trim().to_lowercase();
        let memories = self.list_memory_records_at(now)?;
        if query.is_empty() {
            return Ok(memories);
        }

        let memory_bodies_by_id = memories
            .iter()
            .map(|memory| (memory.id, memory.body.to_lowercase()))
            .collect::<std::collections::HashMap<_, _>>();

        Ok(memories
            .into_iter()
            .filter_map(|mut memory| {
                memory_record_search_match(&memory, &query, &memory_bodies_by_id).map(
                    |search_match| {
                        memory.search_match = search_match;
                        memory
                    },
                )
            })
            .collect())
    }

    pub fn append_memory_candidate(&self, candidate: &MemoryCandidate) -> EventStoreResult<()> {
        let event = KernelEvent::new(MEMORY_CANDIDATE_PROPOSED_EVENT, candidate)?;
        self.append(&event)
    }

    pub fn import_memory_candidates(
        &self,
        candidates: &[MemoryCandidate],
    ) -> EventStoreResult<WorkPackageMemoryCandidateImportSummary> {
        let mut existing_ids = self
            .list_memory_candidates()?
            .into_iter()
            .map(|candidate| candidate.id)
            .collect::<std::collections::HashSet<_>>();
        let mut summary = WorkPackageMemoryCandidateImportSummary {
            imported: 0,
            skipped: 0,
        };

        for candidate in candidates {
            if existing_ids.contains(&candidate.id) {
                summary.skipped += 1;
                continue;
            }

            let mut imported_candidate = candidate.clone();
            imported_candidate.source = MemoryCandidateSource::Import;
            imported_candidate.source_id = None;
            self.append_memory_candidate(&imported_candidate)?;
            existing_ids.insert(imported_candidate.id);
            summary.imported += 1;
        }

        Ok(summary)
    }

    pub fn list_memory_candidates(&self) -> EventStoreResult<Vec<MemoryCandidate>> {
        let events = self.list_by_type(MEMORY_CANDIDATE_PROPOSED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<MemoryCandidate>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn append_memory_candidate_resolution(
        &self,
        resolution: &MemoryCandidateResolution,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(MEMORY_CANDIDATE_RESOLVED_EVENT, resolution)?;
        self.append(&event)
    }

    pub fn list_memory_candidate_resolutions(
        &self,
    ) -> EventStoreResult<Vec<MemoryCandidateResolution>> {
        let events = self.list_by_type(MEMORY_CANDIDATE_RESOLVED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<MemoryCandidateResolution>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn list_memory_candidate_records(&self) -> EventStoreResult<Vec<MemoryCandidateRecord>> {
        let mut latest_resolution_by_candidate_id = std::collections::HashMap::new();
        for resolution in self.list_memory_candidate_resolutions()? {
            latest_resolution_by_candidate_id
                .entry(resolution.candidate_id)
                .or_insert(resolution);
        }
        let visible_memories = self.list_memory_records()?;

        self.list_memory_candidates()?
            .into_iter()
            .map(|candidate| {
                let resolution = latest_resolution_by_candidate_id
                    .remove(&candidate.id)
                    .map(|resolution| resolution.to_owned());
                let effective_status = match &resolution {
                    Some(resolution) if resolution.accepted => MemoryCandidateStatus::Accepted,
                    Some(_) => MemoryCandidateStatus::Rejected,
                    None => MemoryCandidateStatus::Pending,
                };
                let conflicting_memories = visible_memories
                    .iter()
                    .filter(|memory| memory_candidate_conflicts_with_record(&candidate, memory))
                    .map(MemoryConflictSummary::from)
                    .collect::<Vec<_>>();
                let conflicting_memory_ids = conflicting_memories
                    .iter()
                    .map(|memory| memory.id)
                    .collect();

                Ok(MemoryCandidateRecord {
                    candidate,
                    resolution,
                    effective_status,
                    conflicting_memory_ids,
                    conflicting_memories,
                })
            })
            .collect()
    }

    pub fn resolve_memory_candidate(
        &self,
        candidate_id: Uuid,
        accepted: bool,
        note: String,
    ) -> EventStoreResult<MemoryCandidateResolution> {
        let record = self
            .list_memory_candidate_records()?
            .into_iter()
            .find(|record| record.candidate.id == candidate_id)
            .ok_or_else(|| {
                EventStoreError::NotFound("memory candidate does not exist".to_string())
            })?;

        if record.resolution.is_some() {
            return Err(EventStoreError::InvalidState(
                "memory candidate is already resolved".to_string(),
            ));
        }

        let memory = if accepted {
            let memory = MemoryRecord::from_memory_candidate(&record.candidate);
            self.ensure_memory_record_source_not_already_written(&memory)?;
            Some(memory)
        } else {
            None
        };

        let resolution = MemoryCandidateResolution::new(candidate_id, accepted, note);
        self.append_memory_candidate_resolution(&resolution)?;
        if let Some(memory) = memory {
            if !self.append_memory_record(&memory)? {
                return Err(EventStoreError::InvalidState(
                    "accepted memory candidate was already written".to_string(),
                ));
            }
        }
        Ok(resolution)
    }

    pub fn preview_memory_candidate_merge(
        &self,
        candidate_id: Uuid,
        source_memory_ids: Vec<Uuid>,
    ) -> EventStoreResult<MemoryCandidateMergePreview> {
        let mut unique_source_memory_ids = Vec::new();
        let mut seen_source_memory_ids = std::collections::HashSet::new();
        for memory_id in source_memory_ids {
            if seen_source_memory_ids.insert(memory_id) {
                unique_source_memory_ids.push(memory_id);
            }
        }
        if unique_source_memory_ids.is_empty() {
            return Err(EventStoreError::InvalidState(
                "memory candidate merge preview requires at least one source memory".to_string(),
            ));
        }

        let record = self
            .list_memory_candidate_records()?
            .into_iter()
            .find(|record| record.candidate.id == candidate_id)
            .ok_or_else(|| {
                EventStoreError::NotFound("memory candidate does not exist".to_string())
            })?;

        if record.resolution.is_some() {
            return Err(EventStoreError::InvalidState(
                "memory candidate is already resolved".to_string(),
            ));
        }

        let visible_memories = self.list_memory_records()?;
        let visible_memories_by_id = visible_memories
            .iter()
            .map(|memory| (memory.id, memory))
            .collect::<std::collections::HashMap<_, _>>();
        let mut source_bodies = Vec::new();
        for memory_id in &unique_source_memory_ids {
            let memory = visible_memories_by_id.get(memory_id).ok_or_else(|| {
                EventStoreError::NotFound(format!("memory record {memory_id} was not found"))
            })?;
            if !record.conflicting_memory_ids.contains(memory_id) {
                return Err(EventStoreError::InvalidState(format!(
                    "memory record {memory_id} is not a current candidate conflict"
                )));
            }
            push_unique_memory_body(&mut source_bodies, &memory.body);
        }
        push_unique_memory_body(&mut source_bodies, &record.candidate.body);

        Ok(MemoryCandidateMergePreview {
            candidate_id,
            source_memory_ids: unique_source_memory_ids,
            title: record.candidate.title,
            body: source_bodies.join("\n\n"),
            memory_type: record.candidate.memory_type,
            scope: record.candidate.scope,
            sensitivity: record.candidate.sensitivity,
            lifecycle: record.candidate.lifecycle,
            expires_at: record.candidate.expires_at,
        })
    }

    pub fn preview_memory_candidate_replace(
        &self,
        candidate_id: Uuid,
        target_memory_ids: Vec<Uuid>,
    ) -> EventStoreResult<MemoryCandidateReplacePreview> {
        let mut unique_target_memory_ids = Vec::new();
        let mut seen_target_memory_ids = std::collections::HashSet::new();
        for memory_id in target_memory_ids {
            if seen_target_memory_ids.insert(memory_id) {
                unique_target_memory_ids.push(memory_id);
            }
        }
        if unique_target_memory_ids.is_empty() {
            return Err(EventStoreError::InvalidState(
                "memory candidate replace preview requires at least one target memory".to_string(),
            ));
        }

        let record = self
            .list_memory_candidate_records()?
            .into_iter()
            .find(|record| record.candidate.id == candidate_id)
            .ok_or_else(|| {
                EventStoreError::NotFound("memory candidate does not exist".to_string())
            })?;

        if record.resolution.is_some() {
            return Err(EventStoreError::InvalidState(
                "memory candidate is already resolved".to_string(),
            ));
        }

        let visible_memories = self.list_memory_records()?;
        let visible_memories_by_id = visible_memories
            .iter()
            .map(|memory| (memory.id, memory))
            .collect::<std::collections::HashMap<_, _>>();
        let mut target_memories = Vec::new();
        for memory_id in &unique_target_memory_ids {
            let memory = visible_memories_by_id.get(memory_id).ok_or_else(|| {
                EventStoreError::NotFound(format!("memory record {memory_id} was not found"))
            })?;
            if !record.conflicting_memory_ids.contains(memory_id) {
                return Err(EventStoreError::InvalidState(format!(
                    "memory record {memory_id} is not a current candidate conflict"
                )));
            }
            target_memories.push(MemoryConflictSummary::from(*memory));
        }

        Ok(MemoryCandidateReplacePreview {
            candidate_id,
            target_memory_ids: unique_target_memory_ids,
            replacement_title: record.candidate.title,
            replacement_body: record.candidate.body,
            memory_type: record.candidate.memory_type,
            scope: record.candidate.scope,
            sensitivity: record.candidate.sensitivity,
            lifecycle: record.candidate.lifecycle,
            expires_at: record.candidate.expires_at,
            target_memories,
        })
    }

    pub fn merge_memory_candidate_with_conflicts(
        &self,
        candidate_id: Uuid,
        source_memory_ids: Vec<Uuid>,
        note: String,
    ) -> EventStoreResult<MemoryCandidateResolution> {
        let preview = self.preview_memory_candidate_merge(candidate_id, source_memory_ids)?;
        let record = self
            .list_memory_candidate_records()?
            .into_iter()
            .find(|record| record.candidate.id == candidate_id)
            .ok_or_else(|| {
                EventStoreError::NotFound("memory candidate does not exist".to_string())
            })?;
        if record.resolution.is_some() {
            return Err(EventStoreError::InvalidState(
                "memory candidate is already resolved".to_string(),
            ));
        }

        let mut merged_candidate = record.candidate.clone();
        merged_candidate.title = preview.title;
        merged_candidate.body = preview.body;
        merged_candidate.memory_type = preview.memory_type;
        merged_candidate.scope = preview.scope;
        merged_candidate.sensitivity = preview.sensitivity;
        merged_candidate.lifecycle = preview.lifecycle;
        merged_candidate.expires_at = preview.expires_at;
        let merged_memory = MemoryRecord::from_memory_candidate(&merged_candidate);
        self.ensure_memory_record_source_not_already_written(&merged_memory)?;

        let resolution = MemoryCandidateResolution::new(candidate_id, true, note.clone());
        self.append_memory_candidate_resolution(&resolution)?;
        if !self.append_memory_record(&merged_memory)? {
            return Err(EventStoreError::InvalidState(
                "accepted memory candidate was already written".to_string(),
            ));
        }

        for memory_id in preview.source_memory_ids {
            let link = MemoryRecordLink::new(
                merged_memory.id,
                memory_id,
                Some(candidate_id),
                MemoryRelationKind::Derives,
                note.clone(),
            )
            .map_err(EventStoreError::InvalidState)?;
            self.append_memory_record_link(&link)?;
            self.delete_memory_record(memory_id, note.clone())?;
        }

        Ok(resolution)
    }

    pub fn replace_memory_candidate_conflicts(
        &self,
        candidate_id: Uuid,
        target_memory_ids: Vec<Uuid>,
        note: String,
    ) -> EventStoreResult<MemoryCandidateResolution> {
        let preview = self.preview_memory_candidate_replace(candidate_id, target_memory_ids)?;
        let record = self
            .list_memory_candidate_records()?
            .into_iter()
            .find(|record| record.candidate.id == candidate_id)
            .ok_or_else(|| {
                EventStoreError::NotFound("memory candidate does not exist".to_string())
            })?;
        if record.resolution.is_some() {
            return Err(EventStoreError::InvalidState(
                "memory candidate is already resolved".to_string(),
            ));
        }

        let replacement_memory = MemoryRecord::from_memory_candidate(&record.candidate);
        self.ensure_memory_record_source_not_already_written(&replacement_memory)?;

        let resolution = MemoryCandidateResolution::new(candidate_id, true, note.clone());
        self.append_memory_candidate_resolution(&resolution)?;
        if !self.append_memory_record(&replacement_memory)? {
            return Err(EventStoreError::InvalidState(
                "accepted memory candidate was already written".to_string(),
            ));
        }

        for memory_id in preview.target_memory_ids {
            let link = MemoryRecordLink::new(
                replacement_memory.id,
                memory_id,
                Some(candidate_id),
                MemoryRelationKind::Updates,
                note.clone(),
            )
            .map_err(EventStoreError::InvalidState)?;
            self.append_memory_record_link(&link)?;
            self.delete_memory_record(memory_id, note.clone())?;
        }

        Ok(resolution)
    }

    pub fn update_memory_candidate_conflict(
        &self,
        candidate_id: Uuid,
        target_memory_id: Uuid,
        note: String,
    ) -> EventStoreResult<MemoryCandidateResolution> {
        let record = self
            .list_memory_candidate_records()?
            .into_iter()
            .find(|record| record.candidate.id == candidate_id)
            .ok_or_else(|| {
                EventStoreError::NotFound("memory candidate does not exist".to_string())
            })?;
        if record.resolution.is_some() {
            return Err(EventStoreError::InvalidState(
                "memory candidate is already resolved".to_string(),
            ));
        }
        if !record.conflicting_memory_ids.contains(&target_memory_id) {
            return Err(EventStoreError::InvalidState(format!(
                "memory record {target_memory_id} is not a current candidate conflict"
            )));
        }

        let resolution = MemoryCandidateResolution::new(candidate_id, true, note.clone());
        self.append_memory_candidate_resolution(&resolution)?;
        self.update_memory_record(
            target_memory_id,
            record.candidate.title,
            record.candidate.body,
            record.candidate.memory_type,
            record.candidate.scope,
            record.candidate.sensitivity,
            record.candidate.lifecycle,
            record.candidate.expires_at,
            note,
        )?;

        Ok(resolution)
    }

    pub fn archive_memory_candidate_conflicts(
        &self,
        candidate_id: Uuid,
        target_memory_ids: Vec<Uuid>,
        note: String,
    ) -> EventStoreResult<MemoryCandidateResolution> {
        let mut unique_target_memory_ids = Vec::new();
        let mut seen_target_memory_ids = std::collections::HashSet::new();
        for memory_id in target_memory_ids {
            if seen_target_memory_ids.insert(memory_id) {
                unique_target_memory_ids.push(memory_id);
            }
        }
        if unique_target_memory_ids.is_empty() {
            return Err(EventStoreError::InvalidState(
                "memory candidate archive requires at least one target memory".to_string(),
            ));
        }

        let record = self
            .list_memory_candidate_records()?
            .into_iter()
            .find(|record| record.candidate.id == candidate_id)
            .ok_or_else(|| {
                EventStoreError::NotFound("memory candidate does not exist".to_string())
            })?;
        if record.resolution.is_some() {
            return Err(EventStoreError::InvalidState(
                "memory candidate is already resolved".to_string(),
            ));
        }
        let conflicting_memory_ids = record
            .conflicting_memory_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        for memory_id in &unique_target_memory_ids {
            if !conflicting_memory_ids.contains(memory_id) {
                return Err(EventStoreError::InvalidState(format!(
                    "memory record {memory_id} is not a current candidate conflict"
                )));
            }
        }

        let resolution = MemoryCandidateResolution::new(candidate_id, true, note.clone());
        self.append_memory_candidate_resolution(&resolution)?;
        for memory_id in unique_target_memory_ids {
            self.delete_memory_record(memory_id, note.clone())?;
        }

        Ok(resolution)
    }

    pub fn link_memory_candidate_to_conflicts(
        &self,
        candidate_id: Uuid,
        linked_memory_ids: Vec<Uuid>,
        note: String,
    ) -> EventStoreResult<MemoryCandidateResolution> {
        self.link_memory_candidate_to_conflicts_with_relation(
            candidate_id,
            linked_memory_ids,
            MemoryRelationKind::Extends,
            note,
        )
    }

    pub fn link_memory_candidate_to_conflicts_with_relation(
        &self,
        candidate_id: Uuid,
        linked_memory_ids: Vec<Uuid>,
        relation: MemoryRelationKind,
        note: String,
    ) -> EventStoreResult<MemoryCandidateResolution> {
        let mut unique_linked_memory_ids = Vec::new();
        let mut seen_linked_memory_ids = std::collections::HashSet::new();
        for memory_id in linked_memory_ids {
            if seen_linked_memory_ids.insert(memory_id) {
                unique_linked_memory_ids.push(memory_id);
            }
        }
        if unique_linked_memory_ids.is_empty() {
            return Err(EventStoreError::InvalidState(
                "memory candidate link requires at least one target memory".to_string(),
            ));
        }

        let record = self
            .list_memory_candidate_records()?
            .into_iter()
            .find(|record| record.candidate.id == candidate_id)
            .ok_or_else(|| {
                EventStoreError::NotFound("memory candidate does not exist".to_string())
            })?;

        if record.resolution.is_some() {
            return Err(EventStoreError::InvalidState(
                "memory candidate is already resolved".to_string(),
            ));
        }

        let visible_memory_ids = self
            .list_memory_records()?
            .into_iter()
            .map(|memory| memory.id)
            .collect::<std::collections::HashSet<_>>();
        for memory_id in &unique_linked_memory_ids {
            if !visible_memory_ids.contains(memory_id) {
                return Err(EventStoreError::NotFound(format!(
                    "memory record {memory_id} was not found"
                )));
            }
        }
        let conflicting_memory_ids = record
            .conflicting_memory_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        for memory_id in &unique_linked_memory_ids {
            if !conflicting_memory_ids.contains(memory_id) {
                return Err(EventStoreError::InvalidState(format!(
                    "memory record {memory_id} is not a current candidate conflict"
                )));
            }
        }

        let memory = MemoryRecord::from_memory_candidate(&record.candidate);
        self.ensure_memory_record_source_not_already_written(&memory)?;

        let resolution = MemoryCandidateResolution::new(candidate_id, true, note.clone());
        self.append_memory_candidate_resolution(&resolution)?;
        if !self.append_memory_record(&memory)? {
            return Err(EventStoreError::InvalidState(
                "accepted memory candidate was already written".to_string(),
            ));
        }

        for linked_memory_id in unique_linked_memory_ids {
            let link = MemoryRecordLink::new(
                memory.id,
                linked_memory_id,
                Some(candidate_id),
                relation,
                note.clone(),
            )
            .map_err(EventStoreError::InvalidState)?;
            self.append_memory_record_link(&link)?;
        }

        Ok(resolution)
    }

    pub fn append_permission_audit_entry(
        &self,
        entry: &PermissionAuditEntry,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(PERMISSION_AUDIT_RECORDED_EVENT, entry)?;
        self.append(&event)
    }

    pub fn append_deepseek_chat_telemetry(
        &self,
        telemetry: &DeepSeekChatTelemetry,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(DEEPSEEK_CHAT_TELEMETRY_RECORDED_EVENT, telemetry)?;
        self.append(&event)
    }

    pub fn append_agent_context_receipt(
        &self,
        receipt: &AgentContextReceipt,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(AGENT_CONTEXT_RECEIPT_RECORDED_EVENT, receipt)?;
        self.append(&event)
    }

    pub fn append_soul_profile_update(
        &self,
        audit: &AgentSoulProfileUpdateAudit,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(SOUL_PROFILE_UPDATED_EVENT, audit)?;
        self.append(&event)
    }

    pub fn list_soul_profile_updates(&self) -> EventStoreResult<Vec<AgentSoulProfileUpdateAudit>> {
        self.list_by_type(SOUL_PROFILE_UPDATED_EVENT, 100)?
            .into_iter()
            .map(|event| serde_json::from_str(&event.payload_json).map_err(Into::into))
            .collect()
    }

    pub fn list_agent_context_receipts(&self) -> EventStoreResult<Vec<AgentContextReceipt>> {
        let events = self.list_by_type(AGENT_CONTEXT_RECEIPT_RECORDED_EVENT, 100)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<AgentContextReceipt>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn list_deepseek_chat_telemetry(&self) -> EventStoreResult<Vec<DeepSeekChatTelemetry>> {
        let events = self.list_by_type(DEEPSEEK_CHAT_TELEMETRY_RECORDED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<DeepSeekChatTelemetry>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn list_permission_audit_entries(&self) -> EventStoreResult<Vec<PermissionAuditEntry>> {
        let events = self.list_by_type(PERMISSION_AUDIT_RECORDED_EVENT, 100)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<PermissionAuditEntry>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_skill_installation(
        &self,
        installation: &SkillInstallationRecord,
    ) -> EventStoreResult<bool> {
        let existing = self.list_skill_records()?.into_iter().any(|record| {
            record.manifest.name == installation.manifest.name
                && record.manifest.version == installation.manifest.version
                && record.manifest.source.url == installation.manifest.source.url
        });
        if existing {
            return Ok(false);
        }

        let event = KernelEvent::new(SKILL_INSTALLED_EVENT, installation)?;
        self.append(&event)?;
        Ok(true)
    }

    pub fn list_skill_installations(&self) -> EventStoreResult<Vec<SkillInstallationRecord>> {
        let events = self.list_by_type(SKILL_INSTALLED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<SkillInstallationRecord>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_skill_enablement_change(
        &self,
        change: &SkillEnablementChange,
    ) -> EventStoreResult<()> {
        let exists = self
            .list_skill_installations()?
            .into_iter()
            .any(|record| record.id == change.skill_id);
        if !exists {
            return Err(EventStoreError::NotFound(format!(
                "skill installation {}",
                change.skill_id
            )));
        }

        let event = KernelEvent::new(SKILL_ENABLEMENT_CHANGED_EVENT, change)?;
        self.append(&event)
    }

    pub fn list_skill_enablement_changes(&self) -> EventStoreResult<Vec<SkillEnablementChange>> {
        let events = self.list_by_type(SKILL_ENABLEMENT_CHANGED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<SkillEnablementChange>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_skill_trust_reset(&self, reset: &SkillTrustReset) -> EventStoreResult<()> {
        self.ensure_skill_installation_exists(reset.skill_id)?;
        let event = KernelEvent::new(SKILL_TRUST_RESET_EVENT, reset)?;
        self.append(&event)
    }

    pub fn list_skill_trust_resets(&self) -> EventStoreResult<Vec<SkillTrustReset>> {
        let events = self.list_by_type(SKILL_TRUST_RESET_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<SkillTrustReset>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn append_skill_uninstall(&self, uninstall: &SkillUninstallRecord) -> EventStoreResult<()> {
        self.ensure_skill_installation_exists(uninstall.skill_id)?;
        let record = self
            .list_skill_records()?
            .into_iter()
            .find(|record| record.id == uninstall.skill_id)
            .ok_or_else(|| {
                EventStoreError::NotFound(format!(
                    "active skill installation {}",
                    uninstall.skill_id
                ))
            })?;
        if record.system_protected {
            return Err(EventStoreError::InvalidState(format!(
                "protected system skill cannot be uninstalled: {}",
                record.manifest.name
            )));
        }
        let event = KernelEvent::new(SKILL_UNINSTALLED_EVENT, uninstall)?;
        self.append(&event)
    }

    pub fn list_skill_uninstalls(&self) -> EventStoreResult<Vec<SkillUninstallRecord>> {
        let events = self.list_by_type(SKILL_UNINSTALLED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<SkillUninstallRecord>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_skill_update(&self, update: &SkillUpdateRecord) -> EventStoreResult<()> {
        self.ensure_skill_installation_exists(update.skill_id)?;
        let current = self
            .list_skill_records()?
            .into_iter()
            .find(|record| record.id == update.skill_id)
            .ok_or_else(|| {
                EventStoreError::NotFound(format!("active skill installation {}", update.skill_id))
            })?;
        if current.manifest.version != update.previous_version {
            return Err(EventStoreError::InvalidState(format!(
                "skill update expected version {}, current version is {}",
                update.previous_version, current.manifest.version
            )));
        }
        if current.manifest.name != update.manifest.name {
            return Err(EventStoreError::InvalidState(
                "skill update cannot change package identity".to_string(),
            ));
        }
        if update.manifest.permissions.iter().any(|permission| {
            !current
                .manifest
                .permissions
                .iter()
                .any(|current_permission| {
                    current_permission.kind == permission.kind
                        && current_permission.scope == permission.scope
                })
        }) {
            return Err(EventStoreError::InvalidState(
                "skill update cannot expand declared permissions automatically".to_string(),
            ));
        }
        if sha256_hex(update.entry_content.as_bytes()) != update.entry_sha256 {
            return Err(EventStoreError::InvalidState(
                "skill update entry integrity mismatch".to_string(),
            ));
        }
        if let (Some(current_source), Some(next_source)) =
            (&current.source_identity, &update.source_identity)
        {
            if current_source.provider != next_source.provider
                || current_source.repository_url != next_source.repository_url
                || current_source.package_path != next_source.package_path
            {
                return Err(EventStoreError::InvalidState(
                    "skill update cannot change canonical source identity".to_string(),
                ));
            }
        }

        let event = KernelEvent::new(SKILL_UPDATED_EVENT, update)?;
        self.append(&event)
    }

    pub fn list_skill_updates(&self) -> EventStoreResult<Vec<SkillUpdateRecord>> {
        let events = self.list_by_type(SKILL_UPDATED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<SkillUpdateRecord>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn append_skill_update_check(
        &self,
        check: &SkillUpdateCheckRecord,
    ) -> EventStoreResult<()> {
        self.ensure_skill_installation_exists(check.skill_id)?;
        let event = KernelEvent::new(SKILL_UPDATE_CHECKED_EVENT, check)?;
        self.append(&event)
    }

    pub fn list_skill_update_checks(&self) -> EventStoreResult<Vec<SkillUpdateCheckRecord>> {
        let events = self.list_by_type(SKILL_UPDATE_CHECKED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<SkillUpdateCheckRecord>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_skill_update_failure(
        &self,
        failure: &SkillUpdateFailureRecord,
    ) -> EventStoreResult<()> {
        self.ensure_skill_installation_exists(failure.skill_id)?;
        let event = KernelEvent::new(SKILL_UPDATE_FAILED_EVENT, failure)?;
        self.append(&event)
    }

    pub fn list_skill_update_failures(&self) -> EventStoreResult<Vec<SkillUpdateFailureRecord>> {
        let events = self.list_by_type(SKILL_UPDATE_FAILED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<SkillUpdateFailureRecord>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn list_skill_records(&self) -> EventStoreResult<Vec<SkillRecord>> {
        let mut latest_change_by_skill_id = std::collections::HashMap::new();
        for change in self.list_skill_enablement_changes()? {
            latest_change_by_skill_id
                .entry(change.skill_id)
                .or_insert(change);
        }

        let mut latest_reset_by_skill_id = std::collections::HashMap::new();
        for reset in self.list_skill_trust_resets()? {
            latest_reset_by_skill_id
                .entry(reset.skill_id)
                .or_insert(reset);
        }

        let mut latest_update_by_skill_id = std::collections::HashMap::new();
        for update in self.list_skill_updates()? {
            latest_update_by_skill_id
                .entry(update.skill_id)
                .or_insert(update);
        }

        let mut latest_check_by_skill_id = std::collections::HashMap::new();
        for check in self.list_skill_update_checks()? {
            latest_check_by_skill_id
                .entry(check.skill_id)
                .or_insert(check);
        }

        let mut latest_failure_by_skill_id = std::collections::HashMap::new();
        for failure in self.list_skill_update_failures()? {
            latest_failure_by_skill_id
                .entry(failure.skill_id)
                .or_insert(failure);
        }

        let uninstalled_skill_ids = self
            .list_skill_uninstalls()?
            .into_iter()
            .map(|record| record.skill_id)
            .collect::<std::collections::HashSet<_>>();

        self.list_skill_installations()?
            .into_iter()
            .filter(|installation| !uninstalled_skill_ids.contains(&installation.id))
            .map(|installation| {
                let latest_change = latest_change_by_skill_id.remove(&installation.id);
                let latest_reset = latest_reset_by_skill_id.remove(&installation.id);
                let latest_update = latest_update_by_skill_id.remove(&installation.id);
                let latest_check = latest_check_by_skill_id.remove(&installation.id);
                let latest_failure = latest_failure_by_skill_id.remove(&installation.id);
                let mut manifest = installation.manifest;
                let mut installed_from = installation.installed_from;
                let mut source_identity = installation.source_identity;
                let mut entry_content = installation.entry_content;
                let mut entry_sha256 = installation.entry_sha256;
                let mut updated_at = installation.installed_at;
                let mut rollback_version = None;
                let mut rollback_revision = None;
                if let Some(update) = latest_update.as_ref() {
                    manifest = update.manifest.clone();
                    installed_from = update.updated_from.clone();
                    source_identity = update.source_identity.clone();
                    entry_content = Some(update.entry_content.clone());
                    entry_sha256 = Some(update.entry_sha256.clone());
                    updated_at = update.applied_at;
                    rollback_version = Some(update.previous_version.clone());
                    rollback_revision = update.previous_revision.clone();
                }
                let entry_available = entry_content
                    .as_ref()
                    .is_some_and(|content| !content.trim().is_empty())
                    && entry_sha256
                        .as_ref()
                        .is_some_and(|hash| !hash.trim().is_empty());
                let mut enablement_status = latest_change
                    .as_ref()
                    .map(|change| change.status)
                    .unwrap_or(SkillEnablementStatus::Enabled);
                let mut last_audit_note = latest_change.as_ref().map(|change| change.note.clone());
                if let Some(change) = latest_change.as_ref() {
                    updated_at = updated_at.max(change.changed_at);
                }
                if let Some(reset) = latest_reset.filter(|reset| {
                    latest_update
                        .as_ref()
                        .is_none_or(|update| reset.reset_at > update.applied_at)
                }) {
                    manifest.trust_level = SkillTrustLevel::Untrusted;
                    enablement_status = SkillEnablementStatus::Disabled;
                    last_audit_note = Some(reset.note);
                    updated_at = updated_at.max(reset.reset_at);
                }

                let mut update_state = latest_check
                    .as_ref()
                    .map(|check| match check.status {
                        SkillUpdateCheckStatus::UpdateAvailable => {
                            SkillUpdateState::UpdateAvailable
                        }
                        SkillUpdateCheckStatus::Failed => SkillUpdateState::Failed,
                        SkillUpdateCheckStatus::Current | SkillUpdateCheckStatus::Updated => {
                            SkillUpdateState::Current
                        }
                    })
                    .unwrap_or_default();
                if latest_failure.as_ref().is_some_and(|failure| {
                    latest_check
                        .as_ref()
                        .is_none_or(|check| failure.failed_at >= check.checked_at)
                        && failure.failed_at >= updated_at
                }) {
                    update_state = SkillUpdateState::Failed;
                }

                Ok(SkillRecord {
                    id: installation.id,
                    manifest,
                    installed_from,
                    installed_at: installation.installed_at,
                    enablement_status,
                    last_audit_note,
                    updated_at,
                    package_kind: installation.package_kind,
                    system_protected: installation.system_protected,
                    source_identity,
                    update_policy: installation.update_policy,
                    update_state,
                    last_update_checked_at: latest_check.map(|check| check.checked_at),
                    last_update_failure: latest_failure.map(|failure| failure.error),
                    rollback_version,
                    rollback_revision,
                    entry_available,
                    entry_sha256,
                })
            })
            .collect()
    }

    pub fn prepare_skill_activation(
        &self,
        skill_id: Uuid,
        input_summary: String,
    ) -> EventStoreResult<SkillActivationContext> {
        let record = self
            .list_skill_records()?
            .into_iter()
            .find(|record| record.id == skill_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("skill installation {skill_id}")))?;
        let mut installation = self
            .list_skill_installations()?
            .into_iter()
            .find(|installation| installation.id == skill_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("skill installation {skill_id}")))?;
        if let Some(update) = self
            .list_skill_updates()?
            .into_iter()
            .find(|update| update.skill_id == skill_id)
        {
            installation.manifest = update.manifest;
            installation.installed_from = update.updated_from;
            installation.source_identity = update.source_identity;
            installation.entry_content = Some(update.entry_content);
            installation.entry_sha256 = Some(update.entry_sha256);
        }
        SkillActivationContext::for_installation(&record, &installation, input_summary)
            .map_err(EventStoreError::InvalidState)
    }

    pub fn prepare_skill_execution(
        &self,
        skill_id: Uuid,
        input_summary: String,
    ) -> EventStoreResult<SkillExecutionRecord> {
        let record = self
            .list_skill_records()?
            .into_iter()
            .find(|record| record.id == skill_id)
            .ok_or_else(|| EventStoreError::NotFound(format!("skill installation {skill_id}")))?;
        let execution = SkillExecutionRecord::for_skill(&record, input_summary)
            .map_err(EventStoreError::InvalidState)?;
        self.append_skill_execution(&execution)?;
        Ok(execution)
    }

    pub fn append_skill_execution(&self, execution: &SkillExecutionRecord) -> EventStoreResult<()> {
        self.ensure_skill_installation_exists(execution.skill_id)?;
        let event = KernelEvent::new(SKILL_EXECUTION_RECORDED_EVENT, execution)?;
        self.append(&event)
    }

    pub fn list_skill_executions(&self) -> EventStoreResult<Vec<SkillExecutionRecord>> {
        let events = self.list_by_type(SKILL_EXECUTION_RECORDED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<SkillExecutionRecord>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    fn ensure_skill_installation_exists(&self, skill_id: Uuid) -> EventStoreResult<()> {
        let exists = self
            .list_skill_installations()?
            .into_iter()
            .any(|record| record.id == skill_id);
        if exists {
            Ok(())
        } else {
            Err(EventStoreError::NotFound(format!(
                "skill installation {skill_id}"
            )))
        }
    }

    pub fn append_capability_access_request(
        &self,
        request: &CapabilityAccessRequest,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(CAPABILITY_ACCESS_REQUESTED_EVENT, request)?;
        self.append(&event)
    }

    pub fn list_capability_access_requests(
        &self,
    ) -> EventStoreResult<Vec<CapabilityAccessRequest>> {
        let mut statement = self
            .conn
            .prepare("SELECT request_json FROM capability_access_state")?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut requests = rows
            .into_iter()
            .map(|json| serde_json::from_str::<CapabilityAccessRequest>(&json).map_err(Into::into))
            .collect::<EventStoreResult<Vec<_>>>()?;
        requests.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(requests)
    }

    pub fn append_permission_resolution(
        &self,
        resolution: &PermissionResolution,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(PERMISSION_RESOLUTION_RECORDED_EVENT, resolution)?;
        self.append(&event)
    }

    pub fn list_permission_resolutions(&self) -> EventStoreResult<Vec<PermissionResolution>> {
        let mut statement = self.conn.prepare(
            r#"SELECT resolution_json FROM capability_access_state
               WHERE resolution_json IS NOT NULL"#,
        )?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut resolutions = rows
            .into_iter()
            .map(|json| serde_json::from_str::<PermissionResolution>(&json).map_err(Into::into))
            .collect::<EventStoreResult<Vec<_>>>()?;
        resolutions.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(resolutions)
    }

    pub fn list_capability_access_records(&self) -> EventStoreResult<Vec<CapabilityAccessRecord>> {
        let mut statement = self.conn.prepare(
            r#"SELECT request_id FROM connector_approval_consumptions
               UNION SELECT request_id FROM connector_attachment_approval_consumptions
               UNION SELECT request_id FROM capability_approval_consumptions"#,
        )?;
        let consumptions = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .filter_map(|value| Uuid::parse_str(&value).ok())
            .collect::<std::collections::HashSet<_>>();
        drop(statement);
        let mut statement = self.conn.prepare(
            r#"SELECT request_json, resolution_json, effective_status, row_revision
               FROM capability_access_state ORDER BY created_at DESC, rowid DESC"#,
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(request_json, resolution_json, effective_status_json, projection_revision)| {
                    let request: CapabilityAccessRequest = serde_json::from_str(&request_json)?;
                    let resolution = resolution_json
                        .map(|json| serde_json::from_str::<PermissionResolution>(&json))
                        .transpose()?;
                    let effective_status: CapabilityAccessStatus =
                        serde_json::from_str(&effective_status_json)?;
                    let grant_state = capability_grant_state(
                        &request,
                        resolution.as_ref(),
                        effective_status,
                        &[],
                        &consumptions,
                    );

                    Ok(CapabilityAccessRecord {
                        request,
                        resolution,
                        effective_status,
                        projection_revision,
                        grant_state,
                    })
                },
            )
            .collect()
    }

    pub fn list_pending_capability_access_records(
        &self,
    ) -> EventStoreResult<Vec<CapabilityAccessRecord>> {
        Ok(self
            .list_capability_access_records()?
            .into_iter()
            .filter(|record| record.effective_status == CapabilityAccessStatus::PendingApproval)
            .collect())
    }

    pub fn has_user_approved_capability(
        &self,
        capability: CapabilityKind,
    ) -> EventStoreResult<bool> {
        Ok(self
            .available_capability_grant_request_id(capability)?
            .is_some())
    }

    pub fn available_capability_grant_request_id(
        &self,
        capability: CapabilityKind,
    ) -> EventStoreResult<Option<Uuid>> {
        Ok(self
            .list_capability_access_records()?
            .into_iter()
            .find(|record| {
                record.request.capability == capability
                    && matches!(
                        record.grant_state,
                        CapabilityGrantState::Reusable | CapabilityGrantState::OneShotAvailable
                    )
            })
            .map(|record| record.request.id))
    }

    pub fn resolve_capability_access_request(
        &self,
        request_id: Uuid,
        approved: bool,
        note: String,
    ) -> EventStoreResult<PermissionResolution> {
        let record = self.capability_access_record_by_id(request_id)?;

        if record.request.capability == CapabilityKind::ConnectorAttachmentRead {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval requires the dedicated exact-preview resolver"
                    .to_string(),
            ));
        }

        if record.resolution.is_some() {
            return Err(EventStoreError::InvalidState(
                "capability access request is already resolved".to_string(),
            ));
        }

        if record.effective_status != CapabilityAccessStatus::PendingApproval {
            return Err(EventStoreError::InvalidState(
                "capability access request does not require approval".to_string(),
            ));
        }

        let mut resolution = PermissionResolution::new(request_id, approved, note);
        resolution.expected_request_revision = Some(record.projection_revision);
        self.append_permission_resolution(&resolution)?;
        Ok(resolution)
    }

    #[cfg(test)]
    fn resolve_connector_attachment_access_request(
        &self,
        request_id: Uuid,
        approved: bool,
        note: String,
        expected_request_revision: u64,
        expected_preview_revision: u32,
        expected_preview_hash: &str,
    ) -> EventStoreResult<PermissionResolution> {
        let record = self.capability_access_record_by_id(request_id)?;
        if record.request.capability != CapabilityKind::ConnectorAttachmentRead
            || record.effective_status != CapabilityAccessStatus::PendingApproval
            || record.resolution.is_some()
            || record.projection_revision != expected_request_revision
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval is stale or unavailable".to_string(),
            ));
        }
        let scope = record.request.exact_tool.as_ref().ok_or_else(|| {
            EventStoreError::InvalidState(
                "connector attachment approval has no exact preview evidence".to_string(),
            )
        })?;
        if scope.preview_revision != expected_preview_revision
            || scope.preview_hash != expected_preview_hash
        {
            return Err(EventStoreError::InvalidState(
                "connector attachment approval preview changed".to_string(),
            ));
        }
        let resolution = PermissionResolution::new_exact(
            request_id,
            approved,
            note,
            expected_request_revision,
            scope,
        )
        .map_err(EventStoreError::InvalidState)?;
        self.append_permission_resolution(&resolution)?;
        Ok(resolution)
    }

    pub fn append_capability_invocation(
        &self,
        invocation: &CapabilityInvocation,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(CAPABILITY_INVOCATION_RECORDED_EVENT, invocation)?;
        self.append(&event)
    }

    pub fn list_capability_invocations(&self) -> EventStoreResult<Vec<CapabilityInvocation>> {
        let events = self.list_by_type(CAPABILITY_INVOCATION_RECORDED_EVENT, 100)?;
        let mut seen = std::collections::HashSet::new();
        let mut invocations = Vec::new();
        for event in events {
            let invocation = serde_json::from_str::<CapabilityInvocation>(&event.payload_json)?;
            if seen.insert(invocation.id) {
                invocations.push(invocation);
            }
        }
        Ok(invocations)
    }

    pub fn append_tool_invocation(
        &self,
        invocation: &ToolInvocationRecord,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, invocation)?;
        self.append(&event)
    }

    pub fn list_tool_invocations(&self) -> EventStoreResult<Vec<ToolInvocationRecord>> {
        let mut statement = self.conn.prepare(
            r#"SELECT invocation_json FROM tool_invocation_state
               ORDER BY updated_at DESC, rowid DESC LIMIT 500"#,
        )?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|json| serde_json::from_str::<ToolInvocationRecord>(&json).map_err(Into::into))
            .collect()
    }

    fn tool_invocation_by_id(&self, invocation_id: Uuid) -> EventStoreResult<ToolInvocationRecord> {
        let json = self
            .conn
            .query_row(
                "SELECT invocation_json FROM tool_invocation_state WHERE id = ?1",
                params![invocation_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| EventStoreError::NotFound("connector attachment tool".to_string()))?;
        Ok(serde_json::from_str(&json)?)
    }

    fn capability_access_record_by_id(
        &self,
        request_id: Uuid,
    ) -> EventStoreResult<CapabilityAccessRecord> {
        let (request_json, resolution_json, effective_status_json, projection_revision) = self
            .conn
            .query_row(
                r#"SELECT request_json, resolution_json, effective_status, row_revision
                   FROM capability_access_state WHERE request_id = ?1"#,
                params![request_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, u64>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| {
                EventStoreError::NotFound("connector attachment approval".to_string())
            })?;
        let request: CapabilityAccessRequest = serde_json::from_str(&request_json)?;
        let resolution = resolution_json
            .map(|json| serde_json::from_str::<PermissionResolution>(&json))
            .transpose()?;
        let effective_status: CapabilityAccessStatus =
            serde_json::from_str(&effective_status_json)?;
        let consumed: i64 = self.conn.query_row(
            r#"SELECT EXISTS (
                 SELECT 1 FROM connector_approval_consumptions WHERE request_id = ?1
                 UNION ALL
                 SELECT 1 FROM connector_attachment_approval_consumptions WHERE request_id = ?1
                 UNION ALL
                 SELECT 1 FROM capability_approval_consumptions WHERE request_id = ?1
               )"#,
            params![request_id.to_string()],
            |row| row.get(0),
        )?;
        let grant_state = if effective_status != CapabilityAccessStatus::Approved {
            CapabilityGrantState::NotGranted
        } else if consumed == 1 {
            CapabilityGrantState::OneShotConsumed
        } else if capability_risk(request.capability) == RiskLevel::Critical {
            CapabilityGrantState::OneShotAvailable
        } else {
            CapabilityGrantState::Reusable
        };
        Ok(CapabilityAccessRecord {
            request,
            resolution,
            effective_status,
            projection_revision,
            grant_state,
        })
    }

    pub fn append_operations_briefing_run(
        &self,
        run: &OperationsBriefingRun,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(OPERATIONS_BRIEFING_RUN_RECORDED_EVENT, run)?;
        self.append(&event)
    }

    pub fn list_operations_briefing_runs(&self) -> EventStoreResult<Vec<OperationsBriefingRun>> {
        let events = self.list_by_type(OPERATIONS_BRIEFING_RUN_RECORDED_EVENT, 100)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<OperationsBriefingRun>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn import_operations_briefing_runs(
        &self,
        runs: &[OperationsBriefingRun],
    ) -> EventStoreResult<WorkPackageOperationsBriefingImportSummary> {
        let mut existing_ids = self
            .list_operations_briefing_runs()?
            .into_iter()
            .map(|run| run.id)
            .collect::<std::collections::HashSet<_>>();
        let mut summary = WorkPackageOperationsBriefingImportSummary {
            imported: 0,
            skipped: 0,
        };

        for run in runs {
            if existing_ids.contains(&run.id) {
                summary.skipped += 1;
                continue;
            }

            let mut archived_run = redact_operations_briefing_run_for_package_export(run.clone());
            archived_run.archived_from_package = true;
            self.append_operations_briefing_run(&archived_run)?;
            existing_ids.insert(archived_run.id);
            summary.imported += 1;
        }

        Ok(summary)
    }

    fn list_by_type(&self, event_type: &str, limit: usize) -> EventStoreResult<Vec<KernelEvent>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut statement = self.conn.prepare(
            r#"
            SELECT id, event_type, payload_json, created_at
            FROM kernel_events
            WHERE event_type = ?1
            ORDER BY created_at DESC
            LIMIT ?2
            "#,
        )?;
        let rows = statement
            .query_map(params![event_type, limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut events = Vec::with_capacity(rows.len());
        for (id, event_type, payload_json, created_at) in rows {
            events.push(KernelEvent {
                id: Uuid::parse_str(&id)?,
                event_type,
                payload_json,
                created_at: DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc),
            });
        }

        Ok(events)
    }
}

fn due_automation_window(
    transaction: &rusqlite::Transaction<'_>,
    definition: &AutomationDefinition,
    now: DateTime<Utc>,
) -> EventStoreResult<Option<DateTime<Utc>>> {
    let last_scheduled = transaction
        .query_row(
            "SELECT scheduled_for FROM automation_runs WHERE definition_id = ?1 ORDER BY scheduled_for DESC LIMIT 1",
            params![definition.id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|value| DateTime::parse_from_rfc3339(&value).map(|time| time.with_timezone(&Utc)))
        .transpose()?;
    let next = match (&definition.schedule, last_scheduled) {
        (crate::kernel::automation::AutomationSchedule::Once { run_at }, None) => Some(*run_at),
        (crate::kernel::automation::AutomationSchedule::Once { .. }, Some(_)) => None,
        (_, last) => {
            let mut candidate = next_scheduled_at(
                definition,
                last.unwrap_or(definition.created_at - chrono::Duration::nanoseconds(1)),
            )
            .map_err(EventStoreError::InvalidState)?;
            for _ in 0..5_000 {
                let Some(current) = candidate else { break };
                let following = next_scheduled_at(definition, current)
                    .map_err(EventStoreError::InvalidState)?;
                if following.is_some_and(|value| value <= now) {
                    candidate = following;
                } else {
                    break;
                }
            }
            candidate
        }
    };
    Ok(next.filter(|scheduled_for| *scheduled_for <= now))
}

fn push_unique_link(
    links_by_memory_id: &mut std::collections::HashMap<Uuid, Vec<Uuid>>,
    memory_id: Uuid,
    linked_memory_id: Uuid,
) {
    let links = links_by_memory_id.entry(memory_id).or_default();
    if !links.contains(&linked_memory_id) {
        links.push(linked_memory_id);
    }
}

fn push_unique_link_summary(
    summaries_by_memory_id: &mut std::collections::HashMap<Uuid, Vec<MemoryRecordLinkSummary>>,
    memory_id: Uuid,
    linked_memory: MemoryRecordLinkSummary,
) {
    let summaries = summaries_by_memory_id.entry(memory_id).or_default();
    if !summaries
        .iter()
        .any(|summary| summary.id == linked_memory.id)
    {
        summaries.push(linked_memory);
    }
}

fn push_unique_memory_body(bodies: &mut Vec<String>, body: &str) {
    let body = body.trim();
    if body.is_empty() || bodies.iter().any(|existing| existing == body) {
        return;
    }

    bodies.push(body.to_string());
}

fn preview_import_counts<Id>(
    mut seen_ids: std::collections::HashSet<Id>,
    incoming_ids: impl IntoIterator<Item = Id>,
) -> (usize, usize)
where
    Id: Eq + std::hash::Hash,
{
    let mut total = 0;
    let mut skipped = 0;

    for id in incoming_ids {
        total += 1;
        if !seen_ids.insert(id) {
            skipped += 1;
        }
    }

    (total, skipped)
}

fn memory_record_search_match(
    memory: &MemoryRecord,
    query: &str,
    memory_bodies_by_id: &std::collections::HashMap<Uuid, String>,
) -> Option<MemorySearchMatch> {
    if memory.title.to_lowercase().contains(query) || memory.body.to_lowercase().contains(query) {
        return Some(MemorySearchMatch::direct());
    }

    for linked_memory in &memory.linked_memories {
        if linked_memory.title.to_lowercase().contains(query) {
            return Some(MemorySearchMatch::linked(
                MemorySearchMatchSource::LinkedMemoryTitle,
                linked_memory.id,
                linked_memory.relation,
            ));
        }
    }

    for linked_memory in &memory.linked_memories {
        if memory_bodies_by_id
            .get(&linked_memory.id)
            .map(|body| body.contains(query))
            .unwrap_or(false)
        {
            return Some(MemorySearchMatch::linked(
                MemorySearchMatchSource::LinkedMemoryBody,
                linked_memory.id,
                linked_memory.relation,
            ));
        }
    }

    None
}

fn normalize_memory_text(value: &str) -> String {
    value.trim().to_lowercase()
}

fn memory_candidate_conflicts_with_record(
    candidate: &MemoryCandidate,
    memory: &MemoryRecord,
) -> bool {
    if memory.source_id == Some(candidate.id) {
        return false;
    }
    if candidate.source_id == Some(memory.id) {
        return true;
    }

    let candidate_title = normalize_memory_text(&candidate.title);
    let memory_title = normalize_memory_text(&memory.title);
    if candidate_title == memory_title {
        return true;
    }

    if candidate.memory_type != memory.memory_type || candidate.scope != memory.scope {
        return false;
    }

    let candidate_body = normalize_memory_text(&candidate.body);
    let memory_body = normalize_memory_text(&memory.body);
    let long_enough_for_containment =
        candidate_body.chars().count() >= 18 && memory_body.chars().count() >= 18;

    candidate_body == memory_body
        || (long_enough_for_containment
            && (candidate_body.contains(&memory_body) || memory_body.contains(&candidate_body)))
}

fn capability_grant_state(
    request: &CapabilityAccessRequest,
    resolution: Option<&PermissionResolution>,
    effective_status: CapabilityAccessStatus,
    invocations: &[CapabilityInvocation],
    connector_consumptions: &std::collections::HashSet<Uuid>,
) -> CapabilityGrantState {
    if effective_status != CapabilityAccessStatus::Approved {
        return CapabilityGrantState::NotGranted;
    }

    let risk = capability_risk(request.capability);
    let one_shot = request.access_mode == crate::kernel::models::AccessMode::AskEveryStep
        || matches!(risk, RiskLevel::High | RiskLevel::Critical);
    if !one_shot {
        return CapabilityGrantState::Reusable;
    }

    let Some(resolution) = resolution else {
        return CapabilityGrantState::NotGranted;
    };

    let consumed = connector_consumptions.contains(&request.id)
        || invocations.iter().any(|invocation| {
            if invocation.capability != request.capability
                || invocation.created_at < resolution.created_at
            {
                return false;
            }

            match invocation.approval_request_id {
                Some(approval_request_id) => approval_request_id == request.id,
                None => false,
            }
        });

    if consumed {
        CapabilityGrantState::OneShotConsumed
    } else {
        CapabilityGrantState::OneShotAvailable
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Duration, SecondsFormat, Utc};
    use rusqlite::{params, Connection};
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        ConnectorAttachmentCleanupClaim, EventStore, EventStoreError,
        CONNECTOR_RECOVERY_RETRY_QUEUED_EVENT, MEMORY_RECORD_LINKED_EVENT,
    };
    use crate::kernel::automation::{
        AutomationCheckpoint, AutomationDefinition, AutomationDefinitionStatus,
        AutomationRunStatus, AutomationSchedule, MissedRunPolicy, ReviewQueueItem,
        ReviewQueueItemStatus,
    };
    use crate::kernel::connectors::landing::ConnectorAttachmentMetadata;

    #[test]
    fn durable_automation_claim_is_deduplicated_across_restart() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("automation.sqlite3");
        let definition = AutomationDefinition::once(
            "Prepare a durable result.".to_string(),
            "Asia/Shanghai".to_string(),
            Utc::now() - Duration::minutes(1),
        )
        .expect("definition is valid");
        {
            let store = EventStore::open(&path).expect("store opens");
            store
                .upsert_automation_definition(&definition)
                .expect("definition persists");
            assert!(store
                .claim_due_automation_run(definition.id, Utc::now(), "worker-1".to_string())
                .expect("claim succeeds")
                .is_some());
        }
        let store = EventStore::open(&path).expect("store reopens");
        assert!(store
            .claim_due_automation_run(definition.id, Utc::now(), "worker-2".to_string())
            .expect("duplicate is safe")
            .is_none());
        assert_eq!(store.list_automation_runs().expect("runs load").len(), 1);
    }

    #[test]
    fn paused_automation_is_not_claimed() {
        let store = EventStore::open_memory().expect("store opens");
        let definition = AutomationDefinition::once(
            "Prepare a reviewable result.".to_string(),
            "Asia/Shanghai".to_string(),
            Utc::now() - Duration::minutes(1),
        )
        .expect("definition is valid");
        store
            .upsert_automation_definition(&definition)
            .expect("definition persists");
        store
            .set_automation_definition_status(
                definition.id,
                AutomationDefinitionStatus::Paused,
                Utc::now(),
            )
            .expect("pause persists");
        assert!(store
            .claim_due_automation_run(definition.id, Utc::now(), "worker".to_string())
            .expect("paused claim is safe")
            .is_none());
    }

    #[test]
    fn waiting_automation_states_are_not_retryable_failures() {
        let store = EventStore::open_memory().expect("store opens");
        let definition = AutomationDefinition::once(
            "Create a result for review.".to_string(),
            "Asia/Shanghai".to_string(),
            Utc::now() - Duration::minutes(1),
        )
        .expect("definition is valid");
        store
            .upsert_automation_definition(&definition)
            .expect("definition persists");
        let run = store
            .claim_due_automation_run(definition.id, Utc::now(), "worker".to_string())
            .expect("claim succeeds")
            .expect("run is due");
        let running = store
            .transition_automation_run(
                run.id,
                AutomationRunStatus::Running,
                Some(Uuid::new_v4()),
                None,
                Utc::now(),
            )
            .expect("run starts");
        let waiting = store
            .transition_automation_run(
                running.id,
                AutomationRunStatus::WaitingApproval,
                None,
                None,
                Utc::now(),
            )
            .expect("run waits");
        assert_eq!(waiting.status, AutomationRunStatus::WaitingApproval);
        assert!(store
            .transition_automation_run(
                waiting.id,
                AutomationRunStatus::Queued,
                None,
                None,
                Utc::now()
            )
            .is_err());
    }

    #[test]
    fn retry_limit_and_checkpoint_survive_restart() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("automation-retry.sqlite3");
        let mut definition = AutomationDefinition::once(
            "Retry a bounded local task.".to_string(),
            "Asia/Shanghai".to_string(),
            Utc::now() - Duration::minutes(1),
        )
        .expect("definition is valid");
        definition.retry_limit = 1;
        let run_id;
        {
            let store = EventStore::open(&path).expect("store opens");
            store
                .upsert_automation_definition(&definition)
                .expect("definition persists");
            let run = store
                .claim_due_automation_run(definition.id, Utc::now(), "worker".to_string())
                .expect("claim succeeds")
                .expect("run exists");
            run_id = run.id;
            store
                .transition_automation_run(
                    run.id,
                    AutomationRunStatus::Running,
                    None,
                    None,
                    Utc::now(),
                )
                .expect("run starts");
            store
                .transition_automation_run(
                    run.id,
                    AutomationRunStatus::Failed,
                    None,
                    Some("temporary failure".to_string()),
                    Utc::now(),
                )
                .expect("run fails");
            store
                .transition_automation_run(
                    run.id,
                    AutomationRunStatus::Queued,
                    None,
                    None,
                    Utc::now(),
                )
                .expect("one retry queues");
            store
                .upsert_automation_checkpoint(&AutomationCheckpoint {
                    automation_run_id: run.id,
                    dedup_key: run.trigger_window_key,
                    tool_invocation_id: None,
                    evidence_ref: Some("evidence://bounded".to_string()),
                    recorded_at: Utc::now(),
                })
                .expect("checkpoint persists");
        }
        let store = EventStore::open(&path).expect("store reopens");
        let run = store.automation_run(run_id).expect("run reloads");
        assert_eq!(run.attempt, 1);
        store
            .transition_automation_run(run.id, AutomationRunStatus::Running, None, None, Utc::now())
            .expect("retry starts");
        store
            .transition_automation_run(
                run.id,
                AutomationRunStatus::Failed,
                None,
                Some("still failing".to_string()),
                Utc::now(),
            )
            .expect("retry fails");
        assert!(store
            .transition_automation_run(run.id, AutomationRunStatus::Queued, None, None, Utc::now())
            .is_err());
    }

    #[test]
    fn missed_skip_policy_records_window_without_queueing_work() {
        let store = EventStore::open_memory().expect("store opens");
        let mut definition = AutomationDefinition::once(
            "Do not replay stale work.".to_string(),
            "Asia/Shanghai".to_string(),
            Utc::now() - Duration::hours(1),
        )
        .expect("definition is valid");
        definition.missed_run_policy = MissedRunPolicy::Skip;
        definition.missed_after_seconds = 60;
        store
            .upsert_automation_definition(&definition)
            .expect("definition persists");
        assert!(store
            .claim_due_automation_run(definition.id, Utc::now(), "worker".to_string())
            .expect("claim checks policy")
            .is_none());
        let runs = store.list_automation_runs().expect("runs load");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, AutomationRunStatus::Cancelled);
        assert!(store
            .claim_due_automation_run(definition.id, Utc::now(), "worker-2".to_string())
            .expect("repeat wake is safe")
            .is_none());
        assert_eq!(store.list_automation_runs().expect("runs reload").len(), 1);
    }

    #[test]
    fn recurring_run_once_claims_only_latest_missed_window() {
        let store = EventStore::open_memory().expect("store opens");
        let now = DateTime::parse_from_rfc3339("2026-07-12T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut definition = AutomationDefinition::once(
            "Daily summary".to_string(),
            "Asia/Shanghai".to_string(),
            now,
        )
        .expect("definition is valid");
        definition.created_at = now - Duration::days(10);
        definition.schedule = AutomationSchedule::Daily { hour: 9, minute: 0 };
        definition.missed_run_policy = MissedRunPolicy::RunOnce;
        store
            .upsert_automation_definition(&definition)
            .expect("definition persists");
        let run = store
            .claim_due_automation_run(definition.id, now, "scheduler".to_string())
            .expect("claim succeeds")
            .expect("latest window queues");
        assert_eq!(
            run.scheduled_for,
            DateTime::parse_from_rfc3339("2026-07-12T01:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
        assert!(store
            .claim_due_automation_run(definition.id, now, "scheduler-2".to_string())
            .expect("same wake is safe")
            .is_none());
        let next_day = now + Duration::days(1);
        let next = store
            .claim_due_automation_run(definition.id, next_day, "scheduler-3".to_string())
            .expect("next day succeeds")
            .expect("next window queues");
        assert_eq!(
            next.scheduled_for,
            DateTime::parse_from_rfc3339("2026-07-13T01:00:00Z")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn review_item_and_checkpoint_links_survive_restart() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("automation-review.sqlite3");
        let run_id;
        let item_id = Uuid::new_v4();
        {
            let store = EventStore::open(&path).expect("store opens");
            let definition = AutomationDefinition::once(
                "Prepare a draft for review.".to_string(),
                "Asia/Shanghai".to_string(),
                Utc::now() - Duration::minutes(1),
            )
            .expect("definition is valid");
            store
                .upsert_automation_definition(&definition)
                .expect("definition persists");
            let run = store
                .claim_due_automation_run(definition.id, Utc::now(), "worker".to_string())
                .expect("claim succeeds")
                .expect("run exists");
            run_id = run.id;
            let now = Utc::now();
            store
                .upsert_review_queue_item(&ReviewQueueItem {
                    id: item_id,
                    automation_run_id: run.id,
                    agent_run_id: None,
                    tool_invocation_id: None,
                    status: ReviewQueueItemStatus::PendingReview,
                    preview_fingerprint: Some("sha256:draft".to_string()),
                    revision: 0,
                    title: "Review generated draft".to_string(),
                    evidence_ref: Some("evidence://draft".to_string()),
                    created_at: now,
                    updated_at: now,
                })
                .expect("review item persists");
            store
                .upsert_automation_checkpoint(&AutomationCheckpoint {
                    automation_run_id: run.id,
                    dedup_key: run.trigger_window_key,
                    tool_invocation_id: None,
                    evidence_ref: Some("evidence://draft".to_string()),
                    recorded_at: now,
                })
                .expect("checkpoint persists");
        }
        let store = EventStore::open(&path).expect("store reopens");
        assert_eq!(
            store
                .automation_run(run_id)
                .expect("run loads")
                .review_queue_item_id,
            Some(item_id)
        );
        assert_eq!(
            store.list_review_queue_items().expect("items load").len(),
            1
        );
        assert_eq!(
            store
                .automation_checkpoint(run_id)
                .expect("checkpoint loads")
                .evidence_ref
                .as_deref(),
            Some("evidence://draft")
        );
    }

    #[test]
    fn editing_review_item_invalidates_old_exact_approval() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("review-edit.sqlite3");
        let item_id = Uuid::new_v4();
        {
            let store = EventStore::open(&path).expect("store opens");
            let definition = AutomationDefinition::once(
                "Review draft".to_string(),
                "Asia/Shanghai".to_string(),
                Utc::now() - Duration::minutes(1),
            )
            .expect("definition valid");
            store
                .upsert_automation_definition(&definition)
                .expect("definition persists");
            let run = store
                .claim_due_automation_run(definition.id, Utc::now(), "worker".to_string())
                .expect("claim succeeds")
                .expect("run exists");
            let now = Utc::now();
            let access_request = request_capability_access(
                crate::kernel::models::AccessMode::FullAccess,
                CapabilityKind::ConnectorWrite,
            )
            .expect("connector write requires an approval request");
            store
                .append_capability_access_request(&access_request)
                .expect("approval request persists");
            let plan = prepare_tool_execution(&ToolExecutionRequest {
                tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
                input: json!({
                    "provider_id": "fake",
                    "account_id": Uuid::new_v4().to_string(),
                    "account_generation": 0,
                    "capability": "mail_send_draft",
                    "target_ref": "draft:1",
                    "preview_hash": "sha256:preview-a",
                    "idempotency_key": "send:1:once",
                    "automation_run_id": run.id.to_string()
                }),
                access_mode: crate::kernel::models::AccessMode::FullAccess,
                run_id: None,
            })
            .expect("tool plan valid");
            let tool_record =
                ToolInvocationRecord::waiting_for_confirmation(&plan, access_request.id);
            store
                .append_tool_invocation(&tool_record)
                .expect("tool approval persists");
            let mut item = ReviewQueueItem {
                id: item_id,
                automation_run_id: run.id,
                agent_run_id: None,
                tool_invocation_id: None,
                status: ReviewQueueItemStatus::PendingReview,
                preview_fingerprint: Some("sha256:preview-a".to_string()),
                revision: 0,
                title: "Original draft".to_string(),
                evidence_ref: None,
                created_at: now,
                updated_at: now,
            };
            item.request_approval(tool_record.id, "sha256:preview-a".to_string(), now)
                .expect("approval requested");
            store
                .upsert_review_queue_item(&item)
                .expect("item persists");
            let edited = store
                .edit_review_queue_item(
                    item.id,
                    "Edited draft".to_string(),
                    Some("sha256:preview-b".to_string()),
                    Utc::now(),
                )
                .expect("item edits");
            assert_eq!(edited.status, ReviewQueueItemStatus::PendingReview);
            assert!(edited.tool_invocation_id.is_none());
            assert_eq!(edited.revision, 2);
            let access = store
                .list_capability_access_records()
                .expect("access records load");
            let record = access
                .iter()
                .find(|record| record.request.id == access_request.id)
                .expect("approval record exists");
            assert_eq!(record.effective_status, CapabilityAccessStatus::Rejected);
            let mut edited = edited;
            assert!(edited
                .request_approval(Uuid::new_v4(), "sha256:preview-a".to_string(), Utc::now())
                .is_err());
            edited
                .request_approval(Uuid::new_v4(), "sha256:preview-b".to_string(), Utc::now())
                .expect("new approval binds");
            store
                .upsert_review_queue_item(&edited)
                .expect("new approval persists");
            store
                .resolve_review_queue_item(item.id, false, Utc::now())
                .expect("review rejects");
            assert!(store.upsert_review_queue_item(&item).is_err());
        }
        let store = EventStore::open(&path).expect("store reopens");
        assert_eq!(
            store
                .review_queue_item(item_id)
                .expect("item reloads")
                .status,
            ReviewQueueItemStatus::Rejected
        );
    }

    #[test]
    fn automation_run_links_once_to_existing_agent_run() {
        let store = EventStore::open_memory().expect("store opens");
        let definition = AutomationDefinition::once(
            "Execute through the existing Agent worker.".to_string(),
            "Asia/Shanghai".to_string(),
            Utc::now() - Duration::minutes(1),
        )
        .expect("definition is valid");
        store
            .upsert_automation_definition(&definition)
            .expect("definition persists");
        let automation_run = store
            .claim_due_automation_run(definition.id, Utc::now(), "scheduler".to_string())
            .expect("claim succeeds")
            .expect("run exists");
        let agent_run =
            AgentRunStart::queued("automation-conversation".to_string(), definition.goal, 0)
                .expect("agent run is valid");
        store
            .append_agent_run_start(&agent_run)
            .expect("agent run persists");
        let linked = store
            .link_automation_run_to_agent_run(automation_run.id, agent_run.id, Utc::now())
            .expect("link persists");
        assert_eq!(linked.agent_run_id, Some(agent_run.id));
        assert_eq!(
            store
                .link_automation_run_to_agent_run(automation_run.id, agent_run.id, Utc::now())
                .expect("same link is idempotent")
                .agent_run_id,
            Some(agent_run.id)
        );

        let other_agent_run = AgentRunStart::queued(
            "automation-conversation".to_string(),
            "Different run".to_string(),
            0,
        )
        .expect("other run is valid");
        store
            .append_agent_run_start(&other_agent_run)
            .expect("other run persists");
        assert!(store
            .link_automation_run_to_agent_run(automation_run.id, other_agent_run.id, Utc::now())
            .is_err());
    }

    #[test]
    fn due_automation_and_agent_run_enqueue_in_one_deduplicated_transaction() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("automation-enqueue.sqlite3");
        let definition = AutomationDefinition::once(
            "Run through the durable Agent worker.".to_string(),
            "Asia/Shanghai".to_string(),
            Utc::now() - Duration::minutes(1),
        )
        .expect("definition is valid");
        let automation_run_id;
        let agent_run_id;
        {
            let store = EventStore::open(&path).expect("store opens");
            store
                .upsert_automation_definition(&definition)
                .expect("definition persists");
            let (automation_run, agent_run) = store
                .enqueue_due_automation_agent_run(
                    definition.id,
                    Utc::now(),
                    "scheduler-1".to_string(),
                    "automation-conversation".to_string(),
                )
                .expect("enqueue succeeds")
                .expect("window is due");
            automation_run_id = automation_run.id;
            agent_run_id = agent_run.id;
            assert_eq!(automation_run.agent_run_id, Some(agent_run.id));
            assert!(store
                .enqueue_due_automation_agent_run(
                    definition.id,
                    Utc::now(),
                    "scheduler-2".to_string(),
                    "automation-conversation".to_string(),
                )
                .expect("duplicate wake is safe")
                .is_none());
        }
        let store = EventStore::open(&path).expect("store reopens");
        assert_eq!(
            store
                .automation_run(automation_run_id)
                .expect("automation run loads")
                .agent_run_id,
            Some(agent_run_id)
        );
        let agent_runs = store.list_agent_run_records().expect("agent runs load");
        assert_eq!(
            agent_runs
                .iter()
                .filter(|run| run.id == agent_run_id)
                .count(),
            1
        );
        assert_eq!(
            store
                .list_automation_runs()
                .expect("automation runs load")
                .len(),
            1
        );
    }

    #[test]
    fn automation_edit_manual_run_and_delete_are_durable() {
        let store = EventStore::open_memory().expect("store opens");
        let definition = AutomationDefinition::once(
            "Original goal".to_string(),
            "Asia/Shanghai".to_string(),
            Utc::now() + Duration::days(1),
        )
        .expect("definition is valid");
        store
            .upsert_automation_definition(&definition)
            .expect("definition persists");
        let edited = store
            .update_automation_goal(definition.id, "Edited goal".to_string(), Utc::now())
            .expect("goal edits");
        assert_eq!(edited.goal, "Edited goal");
        let invocation_id = Uuid::new_v4();
        let (manual, agent) = store
            .enqueue_manual_automation_agent_run(
                definition.id,
                invocation_id,
                Utc::now(),
                format!("automation:{}", definition.id),
            )
            .expect("manual run queues");
        assert_eq!(manual.agent_run_id, Some(agent.id));
        assert!(store
            .enqueue_manual_automation_agent_run(
                definition.id,
                invocation_id,
                Utc::now(),
                format!("automation:{}", definition.id),
            )
            .is_err());
        store
            .set_automation_definition_status(
                definition.id,
                AutomationDefinitionStatus::Deleted,
                Utc::now(),
            )
            .expect("definition deletes");
        assert!(store
            .enqueue_manual_automation_agent_run(
                definition.id,
                Uuid::new_v4(),
                Utc::now(),
                format!("automation:{}", definition.id),
            )
            .is_err());
    }
    use crate::kernel::agent_context::AgentContextReceipt;
    use crate::kernel::agent_run::{
        AgentRunArtifactRecord, AgentRunCancelRequest, AgentRunClaim, AgentRunContinuationQueued,
        AgentRunExecutionContext, AgentRunFinish, AgentRunGuidanceApplied, AgentRunQueuedGuidance,
        AgentRunResourceAccess, AgentRunResourceClaim, AgentRunResourceRelease, AgentRunRole,
        AgentRunStart, AgentRunStatus, AgentRunStepRecord, AgentRunStepStatus, AgentRunTransition,
    };
    use crate::kernel::capability::{CapabilityInvocation, CapabilityInvocationStatus};
    use crate::kernel::connectors::{
        ConnectorAccount, ConnectorCapability, ConnectorCredentialHandle, ConnectorEvidenceRef,
        ConnectorHealth, ConnectorInvocation, ConnectorInvocationStatus,
    };
    use crate::kernel::expert_team::{
        ExpertAttemptResult, ExpertAttemptUsage, ExpertBudget, ExpertCapability,
        ExpertExternalEffectState, ExpertMergeReceipt, ExpertOutputContract, ExpertQualityGate,
        ExpertResourceAccess, ExpertResourceRequirement, ExpertRetryPolicy, ExpertReviewDecision,
        ExpertReviewVerdict, ExpertRole, ExpertTeamPlanItem,
    };

    #[test]
    fn connector_account_and_idempotent_invocation_survive_restart() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("connectors.sqlite3");
        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "fake".to_string(),
            display_name: "Test account".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailSearch],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        let invocation = ConnectorInvocation {
            id: Uuid::new_v4(),
            provider_id: "fake".to_string(),
            account_id: account.id,
            account_generation: None,
            capability: ConnectorCapability::MailSearch,
            automation_run_id: None,
            tool_invocation_id: None,
            request_fingerprint: "sha256:bounded-request".to_string(),
            idempotency_key: "fake:mail-search:window-1".to_string(),
            mutation: None,
            status: ConnectorInvocationStatus::Succeeded,
            evidence: vec![],
            created_at: now,
            updated_at: now,
        };
        {
            let store = EventStore::open(&path).expect("store opens");
            store
                .upsert_connector_account(&account)
                .expect("account persists");
            assert!(store
                .append_connector_invocation(&invocation)
                .expect("invocation persists"));
            let mut duplicate = invocation.clone();
            duplicate.id = Uuid::new_v4();
            assert!(!store
                .append_connector_invocation(&duplicate)
                .expect("duplicate is safe"));
        }
        let store = EventStore::open(&path).expect("store reopens");
        assert_eq!(
            store.list_connector_accounts().expect("accounts load"),
            vec![account]
        );
        assert_eq!(
            store
                .list_connector_invocations()
                .expect("invocations load"),
            vec![invocation]
        );
    }

    #[test]
    fn automation_reconciliation_projects_agent_completion_after_restart() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("automation-reconcile.sqlite3");
        let automation_run_id;
        let agent_run_id;
        {
            let store = EventStore::open(&path).expect("store opens");
            let definition = AutomationDefinition::once(
                "Complete through the Agent worker.".to_string(),
                "Asia/Shanghai".to_string(),
                Utc::now() - Duration::minutes(1),
            )
            .expect("definition is valid");
            store
                .upsert_automation_definition(&definition)
                .expect("definition persists");
            let (automation_run, agent_run) = store
                .enqueue_due_automation_agent_run(
                    definition.id,
                    Utc::now(),
                    "scheduler".to_string(),
                    "automation-conversation".to_string(),
                )
                .expect("enqueue succeeds")
                .expect("window is due");
            automation_run_id = automation_run.id;
            agent_run_id = agent_run.id;
            store
                .append_agent_run_finish(
                    &AgentRunFinish::new(
                        agent_run.id,
                        AgentRunStatus::Completed,
                        Some("Verified result".to_string()),
                        None,
                    )
                    .expect("finish is valid"),
                )
                .expect("finish persists");
        }
        let store = EventStore::open(&path).expect("store reopens");
        assert_eq!(
            store
                .reconcile_automation_agent_runs(Utc::now())
                .expect("reconcile succeeds"),
            1
        );
        let run = store
            .automation_run(automation_run_id)
            .expect("automation run loads");
        assert_eq!(run.agent_run_id, Some(agent_run_id));
        assert_eq!(run.status, AutomationRunStatus::Completed);
        assert_eq!(
            store
                .reconcile_automation_agent_runs(Utc::now())
                .expect("second reconcile is idempotent"),
            0
        );
    }

    #[test]
    fn external_connector_mutation_cannot_bypass_approval_boundary() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("connector-reconcile.sqlite3");
        let now = Utc::now();
        let account_id = Uuid::new_v4();
        let request = ToolExecutionRequest {
            tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            input: json!({
                "provider_id": "fake",
                "account_id": account_id.to_string(),
                "account_generation": 0,
                "capability": "mail_send_draft",
                "target_ref": "draft:only-once",
                "preview_hash": "sha256:frozen-preview",
                "idempotency_key": "send:only-once",
                "automation_run_id": Uuid::new_v4().to_string()
            }),
            access_mode: AccessMode::FullAccess,
            run_id: Some(Uuid::new_v4()),
        };
        let plan = prepare_tool_execution(&request).expect("tool plan is valid");
        let tool = ToolInvocationRecord::waiting_for_confirmation(&plan, Uuid::new_v4());
        let invocation =
            ConnectorInvocation::from_tool_request(&request, &tool).expect("invocation is valid");
        let store = EventStore::open(&path).expect("store opens");
        assert!(store
            .append_connector_invocation(&invocation)
            .expect("invocation persists"));
        assert!(store
            .transition_connector_invocation(
                invocation.id,
                ConnectorInvocationStatus::Running,
                vec![],
                now
            )
            .is_err());
        let evidence = ConnectorEvidenceRef {
            provider_id: "fake".to_string(),
            account_id,
            remote_object_ref: "remote:sent-message-42".to_string(),
            retrieved_at: Utc::now(),
            bounded_summary: Some("Provider confirms one sent message.".to_string()),
        };
        assert!(store
            .transition_connector_invocation(
                invocation.id,
                ConnectorInvocationStatus::Succeeded,
                vec![evidence],
                Utc::now()
            )
            .is_err());
        assert_eq!(
            store
                .connector_invocation(invocation.id)
                .expect("invocation stays pending")
                .status,
            ConnectorInvocationStatus::PendingApproval
        );
    }
    use crate::kernel::deepseek::{DeepSeekChatCacheStatus, DeepSeekChatTelemetry};
    use crate::kernel::models::{AccessMode, FoundationState};
    use crate::kernel::models::{
        KernelEvent, MemoryCandidate, MemoryCandidateSource, MemoryCandidateStatus,
        MemoryCandidateSuggestedAction, MemoryLifecycle, MemoryRecord, MemoryRecordLink,
        MemoryRecordLinkSummary, MemoryRecordSource, MemoryRelationKind, MemoryScope,
        MemorySearchMatch, MemorySearchMatchSource, MemorySelectedFeedbackKind, MemorySensitivity,
        MemoryType, TaskRecord,
    };
    use crate::kernel::policy::{
        request_capability_access, CapabilityAccessStatus, CapabilityGrantState, CapabilityKind,
        PermissionAuditEntry, PolicyDecision,
    };
    use crate::kernel::tool_runtime::{
        prepare_tool_execution, ToolEvidence, ToolExecutionRequest, ToolExecutionStatus,
        ToolInvocationRecord, ToolVerificationResult, APP_UPDATE_CHECK_TOOL_ID,
        CONNECTOR_MUTATE_TOOL_ID,
    };
    use crate::kernel::work_package::export_work_package;
    use crate::kernel::workflow::WorkflowTemplatePackage;
    use crate::kernel::workflow::{
        OperationsBriefingAction, OperationsBriefingAnomaly, OperationsBriefingRun,
        OperationsBriefingRunStatus, OPERATIONS_BRIEFING_WORKFLOW_ID,
    };

    fn sample_operations_briefing_run() -> OperationsBriefingRun {
        OperationsBriefingRun {
            id: uuid::Uuid::new_v4(),
            workflow_id: OPERATIONS_BRIEFING_WORKFLOW_ID.to_string(),
            status: OperationsBriefingRunStatus::DraftReady,
            archived_from_package: false,
            evidence_folder_path: Some("fixtures/evidence".to_string()),
            evidence_invocation_id: Some(uuid::Uuid::new_v4()),
            title: "Operations Briefing Draft".to_string(),
            summary: "Draft ready from evidence folder manifest.".to_string(),
            anomalies: vec![OperationsBriefingAnomaly {
                area: "Evidence review".to_string(),
                signal: "Review accepted text files.".to_string(),
                evidence_ref: Some("fixtures/evidence".to_string()),
            }],
            action_plan: vec![OperationsBriefingAction {
                owner: "Operations owner".to_string(),
                action: "Confirm evidence set.".to_string(),
                due_hint: "Next briefing cycle".to_string(),
            }],
            warnings: Vec::new(),
            context_receipt: Default::default(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn appends_and_lists_recent_kernel_event() {
        let store = EventStore::open_memory().expect("memory store opens");
        let payload = serde_json::json!({
            "source": "foundation"
        });
        let event = KernelEvent::new("foundation.started", payload).expect("payload serializes");

        store.append(&event).expect("event appends");
        let events = store.list_recent(10).expect("recent events load");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, event.id);
        assert_eq!(events[0].event_type, event.event_type);
        assert_eq!(events[0].payload_json, event.payload_json);
    }

    #[test]
    fn appends_and_lists_task_records() {
        let store = EventStore::open_memory().expect("memory store opens");
        let record = TaskRecord::new(
            "Review finance inbox".to_string(),
            "Collect evidence for the operations briefing.".to_string(),
        )
        .expect("record is valid");

        store
            .append_task_record(&record)
            .expect("task record appends");
        let records = store.list_task_records().expect("records load");

        assert_eq!(records, vec![record]);
    }

    #[test]
    fn agent_run_records_queue_guidance_and_preserve_cancel_requested_over_stale_finish() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::new(
            "conversation-1".to_string(),
            "Prepare the operating briefing.".to_string(),
            2,
        )
        .expect("run start is valid");
        store
            .append_agent_run_start(&start)
            .expect("run start appends");
        let guidance = AgentRunQueuedGuidance::new(
            start.id,
            "Use the latest handoff before drafting.".to_string(),
        )
        .expect("guidance is valid");
        store
            .append_agent_run_queued_guidance(&guidance)
            .expect("guidance appends");
        let cancel = AgentRunCancelRequest::new(
            start.id,
            "User changed direction while run was active.".to_string(),
        )
        .expect("cancel request is valid");
        store
            .append_agent_run_cancel_request(&cancel)
            .expect("cancel appends");
        let stale_finish = AgentRunFinish::completed(
            start.id,
            "Worker returned after the cancellation request.".to_string(),
        )
        .expect("finish is valid");
        store
            .append_agent_run_finish(&stale_finish)
            .expect("finish is still audited");

        let records = store.list_agent_run_records().expect("run records load");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, start.id);
        assert_eq!(records[0].status, AgentRunStatus::CancelRequested);
        assert_eq!(records[0].queued_guidance, vec![guidance]);
        assert_eq!(
            records[0].cancel_reason.as_deref(),
            Some("User changed direction while run was active.")
        );
        assert_eq!(
            records[0].finish_summary.as_deref(),
            Some("Worker returned after the cancellation request.")
        );
    }

    #[test]
    fn subagent_runs_preserve_parent_linkage_and_enforce_one_level_three_child_limit() {
        let store = EventStore::open_memory().expect("memory store opens");
        let parent = AgentRunStart::queued(
            "conversation-parallel".to_string(),
            "Compare three independent evidence sources.".to_string(),
            0,
        )
        .expect("parent is valid");
        store
            .append_agent_run_start(&parent)
            .expect("parent appends");

        let mut children = Vec::new();
        for index in 1..=3 {
            let child = AgentRunStart::queued_subagent(
                parent.id,
                parent.conversation_id.clone(),
                format!("source-{index}"),
                format!("Read source {index} and return evidence."),
            )
            .expect("subagent is valid");
            store
                .append_agent_run_start(&child)
                .expect("subagent appends");
            children.push(child);
        }

        let fourth = AgentRunStart::queued_subagent(
            parent.id,
            parent.conversation_id.clone(),
            "source-4".to_string(),
            "Read source 4.".to_string(),
        )
        .expect("fourth subagent shape is valid");
        let limit_error = store.append_agent_run_start(&fourth);
        assert!(matches!(limit_error, Err(EventStoreError::InvalidState(_))));

        let recursive = AgentRunStart::queued_subagent(
            children[0].id,
            parent.conversation_id.clone(),
            "nested".to_string(),
            "Do nested work.".to_string(),
        )
        .expect("nested subagent shape is valid");
        let recursion_error = store.append_agent_run_start(&recursive);
        assert!(matches!(
            recursion_error,
            Err(EventStoreError::InvalidState(_))
        ));

        let records = store.list_agent_run_records().expect("records load");
        let parent_record = records
            .iter()
            .find(|record| record.id == parent.id)
            .unwrap();
        assert_eq!(parent_record.role, AgentRunRole::Parent);
        assert!(parent_record.parent_run_id.is_none());
        for child in children {
            let record = records.iter().find(|record| record.id == child.id).unwrap();
            assert_eq!(record.role, AgentRunRole::Subagent);
            assert_eq!(record.parent_run_id, Some(parent.id));
            assert_eq!(record.subtask_key, child.subtask_key);
            assert_eq!(record.status, AgentRunStatus::Queued);
        }
    }

    #[test]
    fn subagent_plan_is_created_once_and_parent_cancel_propagates_to_open_children() {
        let store = EventStore::open_memory().expect("memory store opens");
        let parent = AgentRunStart::queued(
            "conversation-tree".to_string(),
            "Research independent sources and synthesize them.".to_string(),
            0,
        )
        .expect("parent is valid");
        store
            .append_agent_run_start(&parent)
            .expect("parent appends");

        let children = store
            .append_subagent_runs(
                parent.id,
                vec![
                    ("source-a".to_string(), "Read source A.".to_string()),
                    ("source-b".to_string(), "Read source B.".to_string()),
                ],
            )
            .expect("subagent plan appends");
        assert_eq!(children.len(), 2);
        assert!(children
            .iter()
            .all(|record| record.role == AgentRunRole::Subagent));

        let duplicate_plan = store.append_subagent_runs(
            parent.id,
            vec![("source-c".to_string(), "Read source C.".to_string())],
        );
        assert!(matches!(
            duplicate_plan,
            Err(EventStoreError::InvalidState(_))
        ));

        let cancelled = store
            .request_agent_run_tree_cancel(parent.id, "User cancelled the parent task.".to_string())
            .expect("tree cancellation appends");
        assert_eq!(cancelled.len(), 3);
        assert!(cancelled
            .iter()
            .all(|record| record.status == AgentRunStatus::CancelRequested));
        assert!(cancelled.iter().all(
            |record| record.cancel_reason.as_deref() == Some("User cancelled the parent task.")
        ));
    }

    fn expert_plan_item(
        key: &str,
        role: ExpertRole,
        depends_on: &[&str],
        max_attempts: u8,
    ) -> ExpertTeamPlanItem {
        let production = role == ExpertRole::Production;
        ExpertTeamPlanItem {
            key: key.to_string(),
            role,
            prompt: format!("Complete {key} with explicit evidence."),
            depends_on: depends_on.iter().map(|value| value.to_string()).collect(),
            capabilities: if production {
                vec![
                    ExpertCapability::FileRead,
                    ExpertCapability::ManagedStagingWrite,
                ]
            } else {
                vec![ExpertCapability::FileRead]
            },
            resources: if production {
                vec![ExpertResourceRequirement {
                    key: "draft".to_string(),
                    access: ExpertResourceAccess::Write,
                }]
            } else {
                vec![ExpertResourceRequirement {
                    key: "evidence".to_string(),
                    access: ExpertResourceAccess::Read,
                }]
            },
            budget: ExpertBudget::default(),
            output_contract: ExpertOutputContract::default(),
            retry_policy: ExpertRetryPolicy {
                max_attempts,
                substitute_role: None,
            },
        }
    }

    fn expert_result(
        record: &crate::kernel::agent_run::AgentRunRecord,
        passed: bool,
    ) -> ExpertAttemptResult {
        let contract = record.expert_contract.as_ref().expect("expert contract");
        ExpertAttemptResult {
            id: Uuid::new_v4(),
            run_id: record.id,
            parent_run_id: contract.parent_run_id,
            key: contract.key.clone(),
            role: contract.role,
            attempt: contract.attempt,
            parent_input_revision: contract.parent_input_revision.clone(),
            output_revision: format!("revision-{}-{}", contract.key, contract.attempt),
            summary: format!("{} result", contract.key),
            claims: Vec::new(),
            evidence: Vec::new(),
            unresolved_conflicts: Vec::new(),
            missing_evidence: Vec::new(),
            usage: ExpertAttemptUsage {
                elapsed_ms: 100,
                tool_calls: 1,
                tokens: 200,
                output_bytes: 100,
                staged_bytes: 0,
            },
            quality_gates: vec![ExpertQualityGate {
                code: "fake_executor".to_string(),
                passed,
                detail: "deterministic fake executor gate".to_string(),
            }],
            staging: None,
            review: None,
            external_effect_state: ExpertExternalEffectState::VerifiedReadOnly,
            retry_eligible: !passed && contract.attempt < contract.retry_policy.max_attempts,
            recorded_at: Utc::now(),
        }
    }

    #[test]
    fn expert_attempt_dependencies_results_and_retry_lineage_survive_restart() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("expert-team.sqlite3");
        let parent_id;
        let failed_run_id;
        let retry_run_id;
        {
            let store = EventStore::open(&path).expect("store opens");
            let parent = AgentRunStart::new(
                "expert-conversation".to_string(),
                "Research and analyze a source-backed decision.".to_string(),
                0,
            )
            .expect("parent valid");
            parent_id = parent.id;
            store
                .append_agent_run_start(&parent)
                .expect("parent appends");
            let children = store
                .append_expert_team_runs(
                    parent.id,
                    vec![
                        expert_plan_item("research", ExpertRole::Research, &[], 2),
                        expert_plan_item("analysis", ExpertRole::Analysis, &["research"], 1),
                    ],
                )
                .expect("expert plan appends atomically");
            assert_eq!(children.len(), 2);
            let research = store
                .claim_next_agent_run("expert-worker-1".to_string(), 60)
                .expect("claim works")
                .expect("research is ready");
            assert_eq!(research.expert_contract.as_ref().unwrap().key, "research");
            failed_run_id = research.id;
            let failed = expert_result(&research, false);
            store
                .append_expert_attempt_result(&failed)
                .expect("failed result persists");
            store
                .append_agent_run_finish(
                    &AgentRunFinish::new(
                        research.id,
                        AgentRunStatus::Failed,
                        None,
                        Some("fake gate failed".to_string()),
                    )
                    .expect("finish valid"),
                )
                .expect("failure persists");
            let retries = store
                .append_due_expert_retries(parent.id)
                .expect("retry queues");
            assert_eq!(retries.len(), 1);
            let retry = &retries[0];
            retry_run_id = retry.id;
            let retry_contract = retry.expert_contract.as_ref().unwrap();
            assert_eq!(retry_contract.attempt, 2);
            assert_eq!(retry_contract.previous_attempt_run_id, Some(failed_run_id));

            let claimed_retry = store
                .claim_next_agent_run("expert-worker-2".to_string(), 60)
                .expect("retry claim works")
                .expect("retry is ready before dependent analysis");
            assert_eq!(claimed_retry.id, retry_run_id);
            let passed = expert_result(&claimed_retry, true);
            store
                .append_expert_attempt_result(&passed)
                .expect("retry result persists");
            store
                .append_agent_run_finish(
                    &AgentRunFinish::completed(claimed_retry.id, "research passed".to_string())
                        .expect("finish valid"),
                )
                .expect("retry completion persists");
            let analysis = store
                .claim_next_agent_run("expert-worker-3".to_string(), 60)
                .expect("dependent claim works")
                .expect("analysis becomes ready");
            assert_eq!(analysis.expert_contract.as_ref().unwrap().key, "analysis");
        }
        let store = EventStore::open(&path).expect("store reopens");
        let records = store.list_agent_run_records().expect("records project");
        let children = records
            .iter()
            .filter(|record| record.parent_run_id == Some(parent_id))
            .collect::<Vec<_>>();
        assert_eq!(children.len(), 3);
        assert!(children
            .iter()
            .find(|record| record.id == failed_run_id)
            .unwrap()
            .expert_result
            .is_some());
        assert_eq!(
            children
                .iter()
                .find(|record| record.id == retry_run_id)
                .unwrap()
                .expert_contract
                .as_ref()
                .unwrap()
                .previous_attempt_run_id,
            Some(failed_run_id)
        );
    }

    #[test]
    fn expert_merge_receipt_is_exact_revision_bound_and_idempotent() {
        let store = EventStore::open_memory().expect("store opens");
        let parent = AgentRunStart::new(
            "expert-merge-conversation".to_string(),
            "Produce and review one controlled draft.".to_string(),
            0,
        )
        .expect("parent valid");
        store
            .append_agent_run_start(&parent)
            .expect("parent appends");
        let children = store
            .append_expert_team_runs(
                parent.id,
                vec![
                    expert_plan_item("draft", ExpertRole::Production, &[], 1),
                    expert_plan_item("review", ExpertRole::Review, &["draft"], 1),
                ],
            )
            .expect("team appends");
        let production = children
            .iter()
            .find(|record| record.expert_contract.as_ref().unwrap().role == ExpertRole::Production)
            .unwrap();
        let review = children
            .iter()
            .find(|record| record.expert_contract.as_ref().unwrap().role == ExpertRole::Review)
            .unwrap();
        let mut production_result = expert_result(production, true);
        production_result.output_revision = "production-revision".to_string();
        store
            .append_expert_attempt_result(&production_result)
            .expect("production result persists");
        store
            .append_agent_run_finish(
                &AgentRunFinish::completed(production.id, "draft ready".to_string())
                    .expect("finish valid"),
            )
            .expect("production completes");
        let mut review_result = expert_result(review, true);
        review_result.review = Some(ExpertReviewVerdict {
            target_revision: production_result.output_revision.clone(),
            decision: ExpertReviewDecision::Accept,
            findings: vec!["Exact revision is acceptable.".to_string()],
        });
        store
            .append_expert_attempt_result(&review_result)
            .expect("review result persists");
        store
            .append_agent_run_finish(
                &AgentRunFinish::completed(review.id, "review accepted".to_string())
                    .expect("finish valid"),
            )
            .expect("review completes");

        let mut receipt = ExpertMergeReceipt {
            id: Uuid::new_v4(),
            parent_run_id: parent.id,
            parent_input_revision: crate::kernel::expert_team::parent_input_revision(
                &parent.prompt,
            ),
            production_run_id: production.id,
            production_revision: "stale-revision".to_string(),
            review_run_id: review.id,
            merged_at: Utc::now(),
        };
        assert!(matches!(
            store.append_expert_merge_receipt(&receipt),
            Err(EventStoreError::InvalidState(_))
        ));
        receipt.production_revision = production_result.output_revision;
        store
            .append_expert_merge_receipt(&receipt)
            .expect("exact merge receipt persists");
        assert!(matches!(
            store.append_expert_merge_receipt(&receipt),
            Err(EventStoreError::InvalidState(_))
        ));
        let parent_record = store
            .list_agent_run_records()
            .expect("records project")
            .into_iter()
            .find(|record| record.id == parent.id)
            .unwrap();
        assert_eq!(parent_record.expert_merge_receipt, Some(receipt));
    }

    #[test]
    fn parent_synthesis_transition_requeues_without_reusing_the_planning_worker_lease() {
        let store = EventStore::open_memory().expect("memory store opens");
        let parent = AgentRunStart::queued(
            "conversation-synthesis".to_string(),
            "Synthesize parallel evidence.".to_string(),
            0,
        )
        .expect("parent is valid");
        store
            .append_agent_run_start(&parent)
            .expect("parent appends");
        store
            .claim_agent_run(parent.id, "planning-worker".to_string(), 60)
            .expect("planning worker claims parent");
        let waiting = AgentRunTransition::new(
            parent.id,
            AgentRunStatus::Blocked,
            "Waiting for Subagents.".to_string(),
            None,
        )
        .expect("waiting transition is valid");
        store
            .append_agent_run_transition(&waiting)
            .expect("waiting transition appends");
        let synthesis = AgentRunTransition::new(
            parent.id,
            AgentRunStatus::Queued,
            "Subagents finished; synthesize.".to_string(),
            None,
        )
        .expect("synthesis transition is valid");
        store
            .append_agent_run_transition(&synthesis)
            .expect("synthesis transition appends");

        let queued = store
            .list_agent_run_records()
            .expect("records load")
            .into_iter()
            .find(|record| record.id == parent.id)
            .expect("parent exists");
        assert_eq!(queued.status, AgentRunStatus::Queued);
        assert!(queued.worker_id.is_none());
        assert!(queued.lease_expires_at.is_none());
        assert_eq!(
            queued.status_reason.as_deref(),
            Some("Subagents finished; synthesize.")
        );
    }

    #[test]
    fn agent_run_guidance_application_is_durable_without_deleting_guidance_history() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::queued(
            "conversation-guidance".to_string(),
            "Prepare a verified result.".to_string(),
            0,
        )
        .expect("run start is valid");
        store
            .append_agent_run_start(&start)
            .expect("run start appends");
        let first =
            AgentRunQueuedGuidance::new(start.id, "Use the latest source only.".to_string())
                .expect("first guidance is valid");
        let second = AgentRunQueuedGuidance::new(start.id, "Keep the output concise.".to_string())
            .expect("second guidance is valid");
        store
            .append_agent_run_queued_guidance(&first)
            .expect("first guidance appends");
        store
            .append_agent_run_queued_guidance(&second)
            .expect("second guidance appends");
        let applied = AgentRunGuidanceApplied::new(start.id, first.id)
            .expect("guidance application is valid");
        store
            .append_agent_run_guidance_applied(&applied)
            .expect("guidance application appends");

        let record = store
            .list_agent_run_records()
            .expect("run records load")
            .into_iter()
            .find(|record| record.id == start.id)
            .expect("run record exists");

        assert_eq!(record.queued_guidance.len(), 2);
        assert_eq!(record.queued_guidance[0].id, first.id);
        assert_eq!(
            record.queued_guidance[0].applied_at,
            Some(applied.applied_at)
        );
        assert_eq!(record.queued_guidance[1].id, second.id);
        assert!(record.queued_guidance[1].applied_at.is_none());
        assert_eq!(
            store
                .list_unapplied_agent_run_guidance(start.id)
                .expect("unapplied guidance loads"),
            vec![second]
        );
    }

    #[test]
    fn agent_run_queue_claims_oldest_pending_and_skips_cancel_requested() {
        let store = EventStore::open_memory().expect("memory store opens");
        let first = AgentRunStart::queued(
            "conversation-1".to_string(),
            "First queued run.".to_string(),
            0,
        )
        .expect("first run is valid");
        let second = AgentRunStart::queued(
            "conversation-1".to_string(),
            "Second queued run.".to_string(),
            0,
        )
        .expect("second run is valid");
        store
            .append_agent_run_start(&first)
            .expect("first run appends");
        store
            .append_agent_run_start(&second)
            .expect("second run appends");
        let cancel =
            AgentRunCancelRequest::new(first.id, "User cancelled before claim.".to_string())
                .expect("cancel is valid");
        store
            .append_agent_run_cancel_request(&cancel)
            .expect("cancel appends");

        let claimed = store
            .claim_next_agent_run("worker-a".to_string(), 30)
            .expect("claim succeeds")
            .expect("second run claimed");

        assert_eq!(claimed.id, second.id);
        assert_eq!(claimed.status, AgentRunStatus::Running);
        assert_eq!(claimed.worker_id.as_deref(), Some("worker-a"));
        assert!(claimed.lease_expires_at.is_some());

        let records = store.list_agent_run_records().expect("records load");
        let first_record = records
            .iter()
            .find(|record| record.id == first.id)
            .expect("first record exists");
        assert_eq!(first_record.status, AgentRunStatus::CancelRequested);
    }

    #[test]
    fn agent_run_can_claim_a_specific_queued_run_for_a_worker() {
        let store = EventStore::open_memory().expect("memory store opens");
        let first = AgentRunStart::queued(
            "conversation-1".to_string(),
            "First queued run.".to_string(),
            0,
        )
        .expect("first run is valid");
        let second = AgentRunStart::queued(
            "conversation-1".to_string(),
            "Second queued run.".to_string(),
            0,
        )
        .expect("second run is valid");
        store
            .append_agent_run_start(&first)
            .expect("first run appends");
        store
            .append_agent_run_start(&second)
            .expect("second run appends");

        let claimed = store
            .claim_agent_run(second.id, "worker-b".to_string(), 45)
            .expect("specific run claims");

        assert_eq!(claimed.id, second.id);
        assert_eq!(claimed.status, AgentRunStatus::Running);
        assert_eq!(claimed.worker_id.as_deref(), Some("worker-b"));
        assert!(claimed.lease_expires_at.is_some());

        let stale_claim = store.claim_agent_run(second.id, "worker-c".to_string(), 45);
        assert!(stale_claim.is_err());
    }

    #[test]
    fn agent_run_expired_lease_is_recovered_once_and_claimed_by_a_new_worker() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::queued(
            "conversation-recovery".to_string(),
            "Continue after a desktop restart.".to_string(),
            0,
        )
        .expect("run is valid");
        store.append_agent_run_start(&start).expect("run appends");
        let expired_at = Utc::now() - Duration::seconds(5);
        store
            .append_agent_run_claim(&AgentRunClaim {
                id: Uuid::new_v4(),
                run_id: start.id,
                worker_id: "worker-before-restart".to_string(),
                claimed_at: expired_at - Duration::seconds(30),
                lease_expires_at: expired_at,
            })
            .expect("expired claim appends");

        let claimed = store
            .claim_next_agent_run("worker-after-restart".to_string(), 30)
            .expect("recovery claim succeeds")
            .expect("expired run is reclaimed");

        assert_eq!(claimed.id, start.id);
        assert_eq!(claimed.status, AgentRunStatus::Running);
        assert_eq!(claimed.worker_id.as_deref(), Some("worker-after-restart"));
        assert_eq!(claimed.recovery_count, 1);
        assert!(claimed.last_recovered_at.is_some());
        assert!(claimed
            .recovery_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("expired")));

        let sweep = store
            .recover_expired_agent_runs(Utc::now())
            .expect("second recovery sweep succeeds");
        assert_eq!(sweep.recovered, 0);
        assert_eq!(sweep.blocked, 0);
        assert_eq!(sweep.cancelled, 0);
    }

    #[test]
    fn agent_run_execution_context_is_durable_and_latest_value_is_projected() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::queued(
            "conversation-context".to_string(),
            "Visible user prompt.".to_string(),
            1,
        )
        .expect("run is valid");
        store.append_agent_run_start(&start).expect("run appends");
        let first = AgentRunExecutionContext::new(
            start.id,
            "Conversation context v1 with attachment evidence.".to_string(),
        )
        .expect("first execution context is valid");
        store
            .append_agent_run_execution_context(&first)
            .expect("first execution context appends");
        let second = AgentRunExecutionContext::new(
            start.id,
            "Conversation context v2 after bounded compression.".to_string(),
        )
        .expect("second execution context is valid");
        store
            .append_agent_run_execution_context(&second)
            .expect("second execution context appends");

        let record = store
            .list_agent_run_records()
            .expect("records load")
            .into_iter()
            .find(|record| record.id == start.id)
            .expect("run exists");

        assert_eq!(
            record.execution_prompt.as_deref(),
            Some("Conversation context v2 after bounded compression.")
        );
        assert_eq!(
            record.execution_context_recorded_at,
            Some(second.recorded_at)
        );
    }

    #[test]
    fn agent_run_active_lease_is_not_recovered_or_claimed() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::queued(
            "conversation-active-lease".to_string(),
            "Keep the active worker isolated.".to_string(),
            0,
        )
        .expect("run is valid");
        store.append_agent_run_start(&start).expect("run appends");
        store
            .claim_agent_run(start.id, "active-worker".to_string(), 120)
            .expect("run claims");

        let claimed = store
            .claim_next_agent_run("competing-worker".to_string(), 30)
            .expect("claim scan succeeds");

        assert!(claimed.is_none());
        let record = store
            .list_agent_run_records()
            .expect("records load")
            .into_iter()
            .find(|record| record.id == start.id)
            .expect("run exists");
        assert_eq!(record.worker_id.as_deref(), Some("active-worker"));
        assert_eq!(record.recovery_count, 0);
    }

    #[test]
    fn agent_run_expired_lease_with_cancel_request_finishes_cancelled() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::queued(
            "conversation-cancel-recovery".to_string(),
            "Stop this run even if the worker vanished.".to_string(),
            0,
        )
        .expect("run is valid");
        store.append_agent_run_start(&start).expect("run appends");
        let expired_at = Utc::now() - Duration::seconds(5);
        store
            .append_agent_run_claim(&AgentRunClaim {
                id: Uuid::new_v4(),
                run_id: start.id,
                worker_id: "lost-worker".to_string(),
                claimed_at: expired_at - Duration::seconds(30),
                lease_expires_at: expired_at,
            })
            .expect("expired claim appends");
        store
            .append_agent_run_cancel_request(
                &AgentRunCancelRequest::new(
                    start.id,
                    "User cancelled while the worker was offline.".to_string(),
                )
                .expect("cancel request is valid"),
            )
            .expect("cancel request appends");

        let sweep = store
            .recover_expired_agent_runs(Utc::now())
            .expect("recovery sweep succeeds");

        assert_eq!(sweep.cancelled, 1);
        let record = store
            .list_agent_run_records()
            .expect("records load")
            .into_iter()
            .find(|record| record.id == start.id)
            .expect("run exists");
        assert_eq!(record.status, AgentRunStatus::Cancelled);
        assert_eq!(record.recovery_count, 0);
    }

    #[test]
    fn agent_run_expired_lease_with_indeterminate_tool_is_blocked_without_replay() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::queued(
            "conversation-indeterminate".to_string(),
            "Do not duplicate an interrupted side effect.".to_string(),
            0,
        )
        .expect("run is valid");
        store.append_agent_run_start(&start).expect("run appends");
        let expired_at = Utc::now() - Duration::seconds(5);
        store
            .append_agent_run_claim(&AgentRunClaim {
                id: Uuid::new_v4(),
                run_id: start.id,
                worker_id: "lost-worker".to_string(),
                claimed_at: expired_at - Duration::seconds(30),
                lease_expires_at: expired_at,
            })
            .expect("expired claim appends");
        let plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: APP_UPDATE_CHECK_TOOL_ID.to_string(),
            input: json!({}),
            access_mode: AccessMode::FullAccess,
            run_id: Some(start.id),
        })
        .expect("tool plan prepares");
        store
            .append_tool_invocation(&ToolInvocationRecord::running(&plan, None))
            .expect("running invocation appends");

        let sweep = store
            .recover_expired_agent_runs(Utc::now())
            .expect("recovery sweep succeeds");

        assert_eq!(sweep.blocked, 1);
        assert!(store
            .claim_next_agent_run("replacement-worker".to_string(), 30)
            .expect("claim scan succeeds")
            .is_none());
        let record = store
            .list_agent_run_records()
            .expect("records load")
            .into_iter()
            .find(|record| record.id == start.id)
            .expect("run exists");
        assert_eq!(record.status, AgentRunStatus::Blocked);
        assert!(record
            .status_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("indeterminate")));
    }

    #[test]
    fn agent_run_records_step_stream_and_artifacts() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::queued(
            "conversation-1".to_string(),
            "Create a briefing and attach the report.".to_string(),
            1,
        )
        .expect("run is valid");
        store.append_agent_run_start(&start).expect("run appends");
        store
            .claim_next_agent_run("worker-a".to_string(), 30)
            .expect("claim succeeds")
            .expect("run claimed");
        let step = AgentRunStepRecord::new(
            start.id,
            1,
            AgentRunStepStatus::Completed,
            "Read evidence".to_string(),
            "Evidence manifest loaded.".to_string(),
        )
        .expect("step is valid");
        store.append_agent_run_step(&step).expect("step appends");
        let artifact = AgentRunArtifactRecord::new(
            start.id,
            "report".to_string(),
            "Operations briefing".to_string(),
            "D:/DS Agent/reports/briefing.md".to_string(),
        )
        .expect("artifact is valid");
        store
            .append_agent_run_artifact(&artifact)
            .expect("artifact appends");

        let records = store.list_agent_run_records().expect("records load");
        let record = records
            .iter()
            .find(|record| record.id == start.id)
            .expect("record exists");

        assert_eq!(record.steps, vec![step]);
        assert_eq!(record.artifacts, vec![artifact]);
        assert_eq!(record.status, AgentRunStatus::Running);
    }

    #[test]
    fn agent_run_transition_projects_waiting_state_and_tool_reference() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::queued(
            "conversation-1".to_string(),
            "Download the approved update in the background.".to_string(),
            0,
        )
        .expect("run is valid");
        store.append_agent_run_start(&start).expect("run appends");
        store
            .claim_agent_run(start.id, "worker-a".to_string(), 30)
            .expect("run claims");
        let tool_invocation_id = Uuid::new_v4();
        let transition = AgentRunTransition::new(
            start.id,
            AgentRunStatus::WaitingForConfirmation,
            "Waiting for local approval before downloading the update.".to_string(),
            Some(tool_invocation_id),
        )
        .expect("transition is valid");
        store
            .append_agent_run_transition(&transition)
            .expect("transition appends");

        let record = store
            .list_agent_run_records()
            .expect("records load")
            .into_iter()
            .find(|record| record.id == start.id)
            .expect("record exists");

        assert_eq!(record.status, AgentRunStatus::WaitingForConfirmation);
        assert_eq!(
            record.status_reason.as_deref(),
            Some("Waiting for local approval before downloading the update.")
        );
        assert_eq!(record.waiting_tool_invocation_id, Some(tool_invocation_id));
        assert_eq!(record.updated_at, transition.transitioned_at);
    }

    #[test]
    fn agent_run_approved_tool_continuation_requeues_same_run_and_clears_worker_lease() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::queued(
            "conversation-continuation".to_string(),
            "Continue planning after the approved tool succeeds.".to_string(),
            0,
        )
        .expect("run is valid");
        store.append_agent_run_start(&start).expect("run appends");
        store
            .claim_agent_run(start.id, "worker-before-approval".to_string(), 60)
            .expect("run claims");
        let plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: APP_UPDATE_CHECK_TOOL_ID.to_string(),
            input: json!({}),
            access_mode: AccessMode::FullAccess,
            run_id: Some(start.id),
        })
        .expect("tool plan prepares");
        let invocation = ToolInvocationRecord::succeeded(
            &plan,
            json!({"current_version": "0.1.2", "update_available": false}),
            vec![ToolEvidence {
                kind: "release_status".to_string(),
                reference: "app-update://current".to_string(),
                summary: "Update status checked.".to_string(),
            }],
            ToolVerificationResult::passed("Update status verified."),
            None,
            1,
        )
        .expect("succeeded invocation is valid");
        store
            .append_tool_invocation(&invocation)
            .expect("succeeded invocation appends");
        let invocation_id = invocation.id;
        store
            .append_agent_run_transition(
                &AgentRunTransition::new(
                    start.id,
                    AgentRunStatus::WaitingForConfirmation,
                    "Waiting for exact local approval.".to_string(),
                    Some(invocation_id),
                )
                .expect("waiting transition is valid"),
            )
            .expect("waiting transition appends");
        let continuation = AgentRunContinuationQueued::new(
            start.id,
            invocation_id,
            "Approved tool succeeded; continue DeepSeek planning from verified evidence."
                .to_string(),
        )
        .expect("continuation is valid");
        store
            .append_agent_run_continuation_queued(&continuation)
            .expect("continuation appends");

        let record = store
            .list_agent_run_records()
            .expect("records load")
            .into_iter()
            .find(|record| record.id == start.id)
            .expect("run exists");

        assert_eq!(record.status, AgentRunStatus::Queued);
        assert!(record.worker_id.is_none());
        assert!(record.lease_expires_at.is_none());
        assert!(record.waiting_tool_invocation_id.is_none());
        assert_eq!(record.continuation_count, 1);
        assert_eq!(record.continuation_tool_invocation_id, Some(invocation_id));
        assert_eq!(record.continuation_queued_at, Some(continuation.queued_at));
    }

    #[test]
    fn agent_run_resource_write_claim_blocks_conflict_until_release() {
        let store = EventStore::open_memory().expect("memory store opens");
        let first = AgentRunStart::new(
            "conversation-1".to_string(),
            "Download an update.".to_string(),
            0,
        )
        .expect("first run is valid");
        let second = AgentRunStart::new(
            "conversation-2".to_string(),
            "Install an update.".to_string(),
            0,
        )
        .expect("second run is valid");
        store
            .append_agent_run_start(&first)
            .expect("first run appends");
        store
            .append_agent_run_start(&second)
            .expect("second run appends");

        let first_claim = store
            .claim_agent_run_resource(
                AgentRunResourceClaim::new(
                    first.id,
                    Uuid::new_v4(),
                    "app_update://installer".to_string(),
                    AgentRunResourceAccess::Write,
                    30,
                )
                .expect("first claim is valid"),
            )
            .expect("first claim succeeds");
        let second_claim = AgentRunResourceClaim::new(
            second.id,
            Uuid::new_v4(),
            "app_update://installer".to_string(),
            AgentRunResourceAccess::Write,
            30,
        )
        .expect("second claim is valid");

        let conflict = store.claim_agent_run_resource(second_claim.clone());
        assert!(matches!(conflict, Err(EventStoreError::InvalidState(_))));

        let release = AgentRunResourceRelease::new(
            &first_claim,
            "verified tool execution completed".to_string(),
        )
        .expect("release is valid");
        store
            .append_agent_run_resource_release(&release)
            .expect("release appends");

        let claimed = store
            .claim_agent_run_resource(second_claim.clone())
            .expect("second claim succeeds after release");
        assert_eq!(claimed, second_claim);
        assert_eq!(
            store
                .list_active_agent_run_resource_claims()
                .expect("active claims load"),
            vec![second_claim]
        );
    }

    #[test]
    fn agent_run_heartbeat_renews_worker_and_resource_leases() {
        let store = EventStore::open_memory().expect("memory store opens");
        let start = AgentRunStart::queued(
            "conversation-heartbeat".to_string(),
            "Run a long verified workspace mutation.".to_string(),
            0,
        )
        .expect("run is valid");
        store.append_agent_run_start(&start).expect("run appends");
        let claimed = store
            .claim_agent_run(start.id, "worker-heartbeat".to_string(), 1)
            .expect("run claim succeeds");
        let invocation_id = Uuid::new_v4();
        let resource = store
            .claim_agent_run_resource(
                AgentRunResourceClaim::new(
                    start.id,
                    invocation_id,
                    "workspace://mutation".to_string(),
                    AgentRunResourceAccess::Write,
                    1,
                )
                .expect("resource claim is valid"),
            )
            .expect("resource claim succeeds");

        let wrong_worker =
            store.heartbeat_agent_run_lease(start.id, "worker-other".to_string(), 60);
        assert!(matches!(
            wrong_worker,
            Err(EventStoreError::InvalidState(_))
        ));

        let renewed = store
            .heartbeat_agent_run_lease(start.id, "worker-heartbeat".to_string(), 60)
            .expect("owning worker renews the run");
        let active_resources = store
            .list_active_agent_run_resource_claims()
            .expect("active resources load");

        assert_eq!(renewed.worker_id.as_deref(), Some("worker-heartbeat"));
        assert!(renewed.lease_expires_at > claimed.lease_expires_at);
        assert_eq!(active_resources.len(), 1);
        assert_eq!(active_resources[0].tool_invocation_id, invocation_id);
        assert!(active_resources[0].lease_expires_at > resource.lease_expires_at);

        let sweep = store
            .recover_expired_agent_runs(
                claimed
                    .lease_expires_at
                    .expect("original worker lease exists")
                    + Duration::seconds(1),
            )
            .expect("recovery sweep succeeds");
        assert_eq!(sweep, Default::default());
        assert_eq!(
            store
                .list_agent_run_records()
                .expect("runs load after recovery boundary")
                .into_iter()
                .find(|record| record.id == start.id)
                .expect("run exists")
                .status,
            AgentRunStatus::Running
        );

        store
            .release_agent_run_resources_for_invocation(
                invocation_id,
                "verified long-running tool completed".to_string(),
            )
            .expect("all renewed resource claims release");
        assert!(store
            .list_active_agent_run_resource_claims()
            .expect("active resources load after release")
            .is_empty());
    }

    #[test]
    fn imports_task_records_and_skips_existing_ids() {
        let store = EventStore::open_memory().expect("memory store opens");
        let existing = TaskRecord::new(
            "Review finance inbox".to_string(),
            "Collect evidence for the operations briefing.".to_string(),
        )
        .expect("record is valid");
        let incoming = TaskRecord::new(
            "Prepare weekly work package".to_string(),
            "Export task records for handoff.".to_string(),
        )
        .expect("record is valid");
        store
            .append_task_record(&existing)
            .expect("existing record appends");

        let summary = store
            .import_task_records(&[existing.clone(), incoming.clone()])
            .expect("records import");
        let records = store.list_task_records().expect("records load");

        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(records.len(), 2);
        assert!(records.contains(&existing));
        assert!(records.contains(&incoming));
    }

    #[test]
    fn captures_memory_from_task_record_once() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Prepare executive summary".to_string(),
            "Remember the report needs source links and approval history.".to_string(),
        )
        .expect("task is valid");
        let memory = MemoryRecord::from_task_record(&task);

        store.append_memory_record(&memory).expect("memory appends");
        let duplicate = store
            .append_memory_record(&MemoryRecord::from_task_record(&task))
            .expect("duplicate memory is skipped");
        let memories = store.list_memory_records().expect("memories load");

        assert!(!duplicate);
        assert_eq!(memories, vec![memory]);
    }

    #[test]
    fn importing_task_records_captures_memory_for_new_records() {
        let store = EventStore::open_memory().expect("memory store opens");
        let existing = TaskRecord::new(
            "Review finance inbox".to_string(),
            "Collect evidence for the operations briefing.".to_string(),
        )
        .expect("record is valid");
        let incoming = TaskRecord::new(
            "Prepare weekly work package".to_string(),
            "Export task records and remember the handoff scope.".to_string(),
        )
        .expect("record is valid");
        store
            .append_task_record(&existing)
            .expect("existing record appends");
        store
            .append_memory_record(&MemoryRecord::from_task_record(&existing))
            .expect("existing memory appends");

        let summary = store
            .import_task_records(&[existing.clone(), incoming.clone()])
            .expect("records import");
        let memories = store.list_memory_records().expect("memories load");

        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(
            memories
                .iter()
                .filter(|memory| memory.source_id == Some(existing.id))
                .count(),
            1
        );
        assert!(memories
            .iter()
            .any(|memory| memory.source_id == Some(incoming.id)));
    }

    #[test]
    fn imported_memory_candidate_import_preview_counts_new_skipped_items_without_writing() {
        let store = EventStore::open_memory().expect("memory store opens");
        let existing = TaskRecord::new(
            "Existing task".to_string(),
            "Already present in the local event store.".to_string(),
        )
        .expect("record is valid");
        let incoming = TaskRecord::new(
            "Incoming handoff task".to_string(),
            "New task from a pasted work package.".to_string(),
        )
        .expect("record is valid");
        store
            .append_task_record(&existing)
            .expect("existing task appends");
        let existing_candidate = MemoryCandidate::new(
            "Existing memory candidate".to_string(),
            "This candidate is already in local review.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "Local reviewer already has this candidate.".to_string(),
        )
        .expect("candidate is valid");
        let incoming_candidate = MemoryCandidate::new(
            "Imported memory candidate".to_string(),
            "This candidate should be reviewed before becoming memory.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "Imported from a handoff package.".to_string(),
        )
        .expect("candidate is valid");
        store
            .append_memory_candidate(&existing_candidate)
            .expect("existing candidate appends");

        let package = export_work_package(
            FoundationState::default(),
            vec![existing.clone(), incoming],
            vec![existing_candidate, incoming_candidate],
            vec![sample_operations_briefing_run()],
        );
        let preview = store
            .preview_work_package_import(&package)
            .expect("preview loads");
        let records = store.list_task_records().expect("records load");

        assert_eq!(preview.task_records.total, 2);
        assert_eq!(preview.task_records.new, 1);
        assert_eq!(preview.task_records.skipped, 1);
        assert_eq!(preview.operations_briefing_runs.total, 1);
        assert!(preview.operations_briefing_runs.replay_supported);
        assert_eq!(preview.memory_candidates.total, 2);
        assert_eq!(preview.memory_candidates.new, 1);
        assert_eq!(preview.memory_candidates.skipped, 1);
        assert!(preview.memory_candidates.review_supported);
        assert_eq!(records, vec![existing]);
    }

    #[test]
    fn operations_briefing_import_preview_counts_new_skipped_archives() {
        let store = EventStore::open_memory().expect("memory store opens");
        let existing_run = sample_operations_briefing_run();
        let incoming_run = sample_operations_briefing_run();
        store
            .append_operations_briefing_run(&existing_run)
            .expect("existing briefing run appends");

        let package = export_work_package(
            FoundationState::default(),
            Vec::new(),
            Vec::new(),
            vec![existing_run.clone(), incoming_run],
        );
        let preview = store
            .preview_work_package_import(&package)
            .expect("preview loads");
        let preview_json = serde_json::to_value(&preview).expect("preview serializes");

        assert_eq!(preview.operations_briefing_runs.total, 2);
        assert_eq!(
            preview_json["operations_briefing_runs"]["new"],
            serde_json::json!(1)
        );
        assert_eq!(
            preview_json["operations_briefing_runs"]["skipped"],
            serde_json::json!(1)
        );
        assert!(preview.operations_briefing_runs.replay_supported);
    }

    #[test]
    fn imported_memory_candidate_imports_new_candidates_as_pending_without_writing_memory() {
        let store = EventStore::open_memory().expect("memory store opens");
        let existing = MemoryCandidate::new(
            "Existing imported rule".to_string(),
            "This candidate is already present locally.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "Existing local review candidate.".to_string(),
        )
        .expect("candidate is valid");
        let source_machine_candidate_source_id = Uuid::new_v4();
        let incoming = MemoryCandidate::new_with_metadata(
            "Imported project context".to_string(),
            "Review this package context before saving it as local memory.".to_string(),
            MemoryCandidateSource::Manual,
            Some(source_machine_candidate_source_id),
            "Imported from a handoff package.".to_string(),
            MemoryType::ProjectContext,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");

        store
            .append_memory_candidate(&existing)
            .expect("existing candidate appends");

        let summary = store
            .import_memory_candidates(&[existing.clone(), incoming.clone()])
            .expect("candidates import");
        let records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let imported = records
            .iter()
            .find(|record| record.candidate.id == incoming.id)
            .expect("incoming candidate imports");

        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(imported.effective_status, MemoryCandidateStatus::Pending);
        assert_eq!(imported.candidate.source, MemoryCandidateSource::Import);
        assert_eq!(imported.candidate.source_id, None);
        assert_eq!(imported.candidate.memory_type, MemoryType::ProjectContext);
        assert_eq!(imported.candidate.scope, MemoryScope::Project);
        assert_eq!(imported.candidate.sensitivity, MemorySensitivity::Sensitive);
        assert!(memories.is_empty());
    }

    #[test]
    fn workflow_template_package_import_preview_counts_new_and_skipped_templates() {
        let store = EventStore::open_memory().expect("memory store opens");
        let existing = WorkflowTemplatePackage::new(
            "operations.briefing.templates.v1".to_string(),
            "operations.briefing.v1".to_string(),
            "Operations Briefing Templates".to_string(),
            "Existing local template package.".to_string(),
            Vec::new(),
        )
        .expect("template package is valid");
        let incoming = WorkflowTemplatePackage::new(
            "operations.weekly-review.templates.v1".to_string(),
            "operations.weekly-review.v1".to_string(),
            "Weekly Review Templates".to_string(),
            "Incoming imported template package.".to_string(),
            Vec::new(),
        )
        .expect("template package is valid");
        store
            .append_workflow_template_package(&existing)
            .expect("existing template package appends");

        let mut package = export_work_package(
            FoundationState::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        package.workflow_templates = vec![existing, incoming];
        let preview = store
            .preview_work_package_import(&package)
            .expect("preview loads");

        assert_eq!(preview.workflow_templates.total, 2);
        assert_eq!(preview.workflow_templates.new, 1);
        assert_eq!(preview.workflow_templates.skipped, 1);
        assert!(preview.workflow_templates.import_supported);
    }

    #[test]
    fn work_package_import_preview_counts_duplicate_package_ids_as_skipped() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Duplicate task from package".to_string(),
            "The work package contains this task twice.".to_string(),
        )
        .expect("task is valid");
        let candidate = MemoryCandidate::new(
            "Duplicate candidate from package".to_string(),
            "The work package contains this memory candidate twice.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "Imported from a duplicated package entry.".to_string(),
        )
        .expect("candidate is valid");
        let run = sample_operations_briefing_run();
        let template = WorkflowTemplatePackage::new(
            "operations.duplicate.templates.v1".to_string(),
            "operations.duplicate.v1".to_string(),
            "Duplicate Template Package".to_string(),
            "The work package contains this workflow template twice.".to_string(),
            Vec::new(),
        )
        .expect("template package is valid");
        let mut package = export_work_package(
            FoundationState::default(),
            vec![task.clone(), task],
            vec![candidate.clone(), candidate],
            vec![run.clone(), run],
        );
        package.workflow_templates = vec![template.clone(), template];

        let preview = store
            .preview_work_package_import(&package)
            .expect("preview loads");

        assert_eq!(preview.task_records.total, 2);
        assert_eq!(preview.task_records.new, 1);
        assert_eq!(preview.task_records.skipped, 1);
        assert_eq!(preview.memory_candidates.total, 2);
        assert_eq!(preview.memory_candidates.new, 1);
        assert_eq!(preview.memory_candidates.skipped, 1);
        assert_eq!(preview.operations_briefing_runs.total, 2);
        assert_eq!(preview.operations_briefing_runs.new, 1);
        assert_eq!(preview.operations_briefing_runs.skipped, 1);
        assert_eq!(preview.workflow_templates.total, 2);
        assert_eq!(preview.workflow_templates.new, 1);
        assert_eq!(preview.workflow_templates.skipped, 1);
    }

    #[test]
    fn workflow_template_package_import_adds_new_templates_once() {
        let store = EventStore::open_memory().expect("memory store opens");
        let existing = WorkflowTemplatePackage::new(
            "operations.briefing.templates.v1".to_string(),
            "operations.briefing.v1".to_string(),
            "Operations Briefing Templates".to_string(),
            "Existing local template package.".to_string(),
            Vec::new(),
        )
        .expect("template package is valid");
        let incoming = WorkflowTemplatePackage::new(
            "operations.weekly-review.templates.v1".to_string(),
            "operations.weekly-review.v1".to_string(),
            "Weekly Review Templates".to_string(),
            "Incoming imported template package.".to_string(),
            Vec::new(),
        )
        .expect("template package is valid");
        store
            .append_workflow_template_package(&existing)
            .expect("existing template package appends");

        let summary = store
            .import_workflow_template_packages(&[existing.clone(), incoming.clone()])
            .expect("template packages import");
        let templates = store
            .list_workflow_template_packages()
            .expect("template packages load");

        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(templates.len(), 2);
        assert!(templates.iter().any(|template| template.id == existing.id));
        assert!(templates.iter().any(|template| template.id == incoming.id));
    }

    #[test]
    fn searches_memory_records_by_title_and_body_case_insensitively() {
        let store = EventStore::open_memory().expect("memory store opens");
        let briefing = TaskRecord::new(
            "Prepare executive briefing".to_string(),
            "Include approval history and drive links.".to_string(),
        )
        .expect("record is valid");
        let browser = TaskRecord::new(
            "Review browser research".to_string(),
            "Capture competitor pricing notes.".to_string(),
        )
        .expect("record is valid");
        store
            .append_memory_record(&MemoryRecord::from_task_record(&briefing))
            .expect("briefing memory appends");
        store
            .append_memory_record(&MemoryRecord::from_task_record(&browser))
            .expect("browser memory appends");

        let title_matches = store
            .search_memory_records("BRIEF")
            .expect("title search works");
        let body_matches = store
            .search_memory_records("pricing")
            .expect("body search works");

        assert_eq!(title_matches.len(), 1);
        assert_eq!(title_matches[0].source_id, Some(briefing.id));
        assert_eq!(body_matches.len(), 1);
        assert_eq!(body_matches[0].source_id, Some(browser.id));
    }

    #[test]
    fn accepting_memory_candidate_writes_long_term_memory_once() {
        let store = EventStore::open_memory().expect("memory store opens");
        let candidate = MemoryCandidate::new(
            "Preferred report tone".to_string(),
            "Use concise operating language with clear owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User proposed this as reusable guidance.".to_string(),
        )
        .expect("candidate is valid");

        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let pending = store
            .list_memory_candidate_records()
            .expect("candidates load");

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].effective_status, MemoryCandidateStatus::Pending);

        store
            .resolve_memory_candidate(candidate.id, true, "Looks reusable.".to_string())
            .expect("candidate resolves");
        let resolved = store
            .list_memory_candidate_records()
            .expect("candidates reload");
        let memories = store.list_memory_records().expect("memories load");

        assert_eq!(
            resolved[0].effective_status,
            MemoryCandidateStatus::Accepted
        );
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].title, candidate.title);
        assert_eq!(memories[0].source, MemoryRecordSource::MemoryCandidate);
        assert_eq!(memories[0].source_id, Some(candidate.id));

        let duplicate = store
            .append_memory_record(&MemoryRecord::from_memory_candidate(&candidate))
            .expect("duplicate accepted memory is skipped");
        assert!(!duplicate);
    }

    #[test]
    fn accepting_memory_candidate_rejects_already_written_candidate_without_resolution() {
        let store = EventStore::open_memory().expect("memory store opens");
        let candidate = MemoryCandidate::new(
            "Preferred report tone".to_string(),
            "Use concise operating language with clear owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User proposed this as reusable guidance.".to_string(),
        )
        .expect("candidate is valid");
        let already_written_memory = MemoryRecord::from_memory_candidate(&candidate);

        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        store
            .append_memory_record(&already_written_memory)
            .expect("already-written candidate memory appends");
        let error = store
            .resolve_memory_candidate(candidate.id, true, "Looks reusable.".to_string())
            .expect_err("already-written candidate must not resolve again");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");

        assert!(matches!(error, EventStoreError::InvalidState(_)));
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].id, already_written_memory.id);
    }

    #[test]
    fn linking_memory_candidate_accepts_candidate_and_keeps_related_memories() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let existing_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants to keep the related but richer instruction.".to_string(),
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&existing_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let resolution = store
            .link_memory_candidate_to_conflicts(
                candidate.id,
                vec![existing_memory.id],
                "Keep both memories and mark them related.".to_string(),
            )
            .expect("candidate links");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let links = store.list_memory_record_links().expect("links load");
        let accepted_memory = memories
            .iter()
            .find(|memory| memory.source_id == Some(candidate.id))
            .expect("accepted memory is written");
        let original_memory = memories
            .iter()
            .find(|memory| memory.id == existing_memory.id)
            .expect("original memory is preserved");

        assert!(resolution.accepted);
        assert_eq!(resolution.candidate_id, candidate.id);
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Accepted
        );
        assert_eq!(memories.len(), 2);
        assert_eq!(links.len(), 1);
        assert_eq!(
            serde_json::to_value(&links[0]).expect("link serializes")["relation"],
            "extends"
        );
        assert_eq!(links[0].source_memory_id, accepted_memory.id);
        assert_eq!(links[0].target_memory_id, original_memory.id);
        assert_eq!(links[0].candidate_id, Some(candidate.id));
        assert_eq!(accepted_memory.linked_memory_ids, vec![original_memory.id]);
        assert_eq!(original_memory.linked_memory_ids, vec![accepted_memory.id]);
        assert_eq!(
            serde_json::to_value(&accepted_memory.linked_memories[0]).expect("summary serializes")
                ["relation"],
            "extends"
        );
        assert_eq!(
            serde_json::to_value(&original_memory.linked_memories[0]).expect("summary serializes")
                ["relation"],
            "extends"
        );
        assert_eq!(
            serde_json::to_value(&accepted_memory.linked_memories[0]).expect("summary serializes")
                ["note"],
            "Keep both memories and mark them related."
        );
        assert_eq!(
            serde_json::to_value(&original_memory.linked_memories[0]).expect("summary serializes")
                ["note"],
            "Keep both memories and mark them related."
        );
        assert_eq!(
            accepted_memory.linked_memories[0].title,
            original_memory.title
        );
        assert_eq!(
            original_memory.linked_memories[0].title,
            accepted_memory.title
        );
    }

    #[test]
    fn linking_memory_candidate_rejects_already_written_candidate_without_resolution() {
        let store = EventStore::open_memory().expect("memory store opens");
        let conflict_task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners.".to_string(),
        )
        .expect("task is valid");
        let conflict_memory = MemoryRecord::from_task_record(&conflict_task);
        let candidate = MemoryCandidate::new_with_metadata(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User proposed this as reusable guidance.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Normal,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");
        let already_written_memory = MemoryRecord::from_memory_candidate(&candidate);

        store
            .append_memory_record(&conflict_memory)
            .expect("conflict memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        store
            .append_memory_record(&already_written_memory)
            .expect("duplicate candidate memory appends");
        let error = store
            .link_memory_candidate_to_conflicts_with_relation(
                candidate.id,
                vec![conflict_memory.id],
                MemoryRelationKind::Extends,
                "Link stale unresolved candidate.".to_string(),
            )
            .expect_err("already-written candidate must not resolve again");

        assert!(matches!(error, EventStoreError::InvalidState(_)));
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let links = store.list_memory_record_links().expect("links load");
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert!(links.is_empty());
    }

    #[test]
    fn linking_memory_candidate_accepts_explicit_relation() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let existing_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants to keep both instructions as related context.".to_string(),
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&existing_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        store
            .link_memory_candidate_to_conflicts_with_relation(
                candidate.id,
                vec![existing_memory.id],
                MemoryRelationKind::Related,
                "Keep both memories as related context.".to_string(),
            )
            .expect("candidate links with explicit relation");

        let memories = store.list_memory_records().expect("memories load");
        let links = store.list_memory_record_links().expect("links load");
        let accepted_memory = memories
            .iter()
            .find(|memory| memory.source_id == Some(candidate.id))
            .expect("accepted memory is written");
        let original_memory = memories
            .iter()
            .find(|memory| memory.id == existing_memory.id)
            .expect("original memory is preserved");

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].relation, MemoryRelationKind::Related);
        assert_eq!(
            accepted_memory.linked_memories[0].relation,
            MemoryRelationKind::Related
        );
        assert_eq!(
            original_memory.linked_memories[0].relation,
            MemoryRelationKind::Related
        );
    }

    #[test]
    fn linking_memory_candidate_rejects_non_conflicting_targets() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Source citation rule".to_string(),
            "Always keep source traceability visible.".to_string(),
        )
        .expect("task is valid");
        let unrelated_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants this as reusable guidance.".to_string(),
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&unrelated_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let error = store
            .link_memory_candidate_to_conflicts_with_relation(
                candidate.id,
                vec![unrelated_memory.id],
                MemoryRelationKind::Related,
                "This target is not a current conflict.".to_string(),
            )
            .expect_err("non-conflicting target is rejected");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let links = store.list_memory_record_links().expect("links load");

        assert!(matches!(error, EventStoreError::InvalidState(_)));
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].id, unrelated_memory.id);
        assert!(links.is_empty());
    }

    #[test]
    fn legacy_self_link_events_are_ignored_when_projecting_memory_graph() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Briefing tone rule".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let briefing_memory = MemoryRecord::from_task_record(&task);
        let legacy_self_link = MemoryRecordLink {
            id: Uuid::new_v4(),
            source_memory_id: briefing_memory.id,
            target_memory_id: briefing_memory.id,
            candidate_id: None,
            relation: MemoryRelationKind::Related,
            note: "Legacy self-loop should not be projected.".to_string(),
            created_at: Utc::now(),
        };
        let event = KernelEvent::new(MEMORY_RECORD_LINKED_EVENT, &legacy_self_link)
            .expect("legacy self-link event serializes");

        store
            .append_memory_record(&briefing_memory)
            .expect("briefing memory appends");
        store.append(&event).expect("legacy self-link appends");
        let memories = store.list_memory_records().expect("memories load");

        assert_eq!(
            store.list_memory_record_links().expect("links load").len(),
            1
        );
        assert_eq!(memories.len(), 1);
        assert!(memories[0].linked_memory_ids.is_empty());
        assert!(memories[0].linked_memories.is_empty());
    }

    #[test]
    fn appending_memory_record_link_rejects_self_link() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Briefing tone rule".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let briefing_memory = MemoryRecord::from_task_record(&task);
        let link = MemoryRecordLink {
            id: Uuid::new_v4(),
            source_memory_id: briefing_memory.id,
            target_memory_id: briefing_memory.id,
            candidate_id: None,
            relation: MemoryRelationKind::Related,
            note: "A memory must not link to itself.".to_string(),
            created_at: Utc::now(),
        };

        store
            .append_memory_record(&briefing_memory)
            .expect("briefing memory appends");
        let error = store
            .append_memory_record_link(&link)
            .expect_err("self-link is rejected");

        assert!(matches!(error, EventStoreError::InvalidState(_)));
        assert!(store
            .list_memory_record_links()
            .expect("links load")
            .is_empty());
    }

    #[test]
    fn appending_memory_record_link_rejects_missing_memory_endpoint() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Briefing tone rule".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let briefing_memory = MemoryRecord::from_task_record(&task);
        let missing_memory_id = Uuid::new_v4();
        let link = MemoryRecordLink::new(
            briefing_memory.id,
            missing_memory_id,
            None,
            MemoryRelationKind::Related,
            "Missing endpoints must not be persisted as graph edges.".to_string(),
        )
        .expect("memory link shape is valid");

        store
            .append_memory_record(&briefing_memory)
            .expect("briefing memory appends");
        let error = store
            .append_memory_record_link(&link)
            .expect_err("link with missing target is rejected");

        assert!(matches!(error, EventStoreError::NotFound(_)));
        assert!(store
            .list_memory_record_links()
            .expect("links load")
            .is_empty());
    }

    #[test]
    fn appending_memory_record_link_skips_duplicate_memory_pair() {
        let store = EventStore::open_memory().expect("memory store opens");
        let briefing_task = TaskRecord::new(
            "Briefing tone rule".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let audit_task = TaskRecord::new(
            "Audit trail standard".to_string(),
            "Keep source traceability visible.".to_string(),
        )
        .expect("task is valid");
        let briefing_memory = MemoryRecord::from_task_record(&briefing_task);
        let audit_memory = MemoryRecord::from_task_record(&audit_task);
        let first_link = MemoryRecordLink::new(
            briefing_memory.id,
            audit_memory.id,
            None,
            MemoryRelationKind::Related,
            "Keep these memories related.".to_string(),
        )
        .expect("memory link is valid");
        let reversed_duplicate = MemoryRecordLink::new(
            audit_memory.id,
            briefing_memory.id,
            None,
            MemoryRelationKind::Related,
            "Duplicate relation from the other direction.".to_string(),
        )
        .expect("reversed memory link is valid");

        store
            .append_memory_record(&briefing_memory)
            .expect("briefing memory appends");
        store
            .append_memory_record(&audit_memory)
            .expect("audit memory appends");
        store
            .append_memory_record_link(&first_link)
            .expect("first memory link appends");
        store
            .append_memory_record_link(&reversed_duplicate)
            .expect("duplicate memory link is idempotent");
        let links = store.list_memory_record_links().expect("links load");

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].source_memory_id, briefing_memory.id);
        assert_eq!(links[0].target_memory_id, audit_memory.id);
    }

    #[test]
    fn appending_memory_record_link_allows_more_specific_relation_for_existing_pair() {
        let store = EventStore::open_memory().expect("memory store opens");
        let briefing_task = TaskRecord::new(
            "Briefing tone rule".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let audit_task = TaskRecord::new(
            "Audit trail standard".to_string(),
            "Keep source traceability visible.".to_string(),
        )
        .expect("task is valid");
        let briefing_memory = MemoryRecord::from_task_record(&briefing_task);
        let audit_memory = MemoryRecord::from_task_record(&audit_task);
        let broad_link = MemoryRecordLink::new(
            briefing_memory.id,
            audit_memory.id,
            None,
            MemoryRelationKind::Related,
            "Initial broad relation.".to_string(),
        )
        .expect("broad memory link is valid");
        let specific_link = MemoryRecordLink::new(
            briefing_memory.id,
            audit_memory.id,
            None,
            MemoryRelationKind::Updates,
            "The audit rule supersedes the older briefing note.".to_string(),
        )
        .expect("specific memory link is valid");

        store
            .append_memory_record(&briefing_memory)
            .expect("briefing memory appends");
        store
            .append_memory_record(&audit_memory)
            .expect("audit memory appends");
        store
            .append_memory_record_link(&broad_link)
            .expect("broad memory link appends");
        store
            .append_memory_record_link(&specific_link)
            .expect("specific relation appends");
        let links = store.list_memory_record_links().expect("links load");
        let memories = store.list_memory_records().expect("memories load");
        let projected_briefing = memories
            .iter()
            .find(|memory| memory.id == briefing_memory.id)
            .expect("briefing memory is visible");

        assert_eq!(links.len(), 2);
        assert_eq!(
            projected_briefing.linked_memories[0].relation,
            MemoryRelationKind::Updates
        );
        assert_eq!(
            projected_briefing.linked_memories[0].note,
            "The audit rule supersedes the older briefing note."
        );
    }

    #[test]
    fn search_memory_records_matches_linked_memory_titles() {
        let store = EventStore::open_memory().expect("memory store opens");
        let briefing_task = TaskRecord::new(
            "Briefing tone rule".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let audit_task = TaskRecord::new(
            "Audit trail standard".to_string(),
            "Keep evidence and source traceability visible.".to_string(),
        )
        .expect("task is valid");
        let briefing_memory = MemoryRecord::from_task_record(&briefing_task);
        let audit_memory = MemoryRecord::from_task_record(&audit_task);
        let link = MemoryRecordLink::new(
            briefing_memory.id,
            audit_memory.id,
            None,
            MemoryRelationKind::Related,
            "Search should follow related memory summaries.".to_string(),
        )
        .expect("memory link is valid");

        store
            .append_memory_record(&briefing_memory)
            .expect("briefing memory appends");
        store
            .append_memory_record(&audit_memory)
            .expect("audit memory appends");
        store
            .append_memory_record_link(&link)
            .expect("memory link appends");

        let matches = store
            .search_memory_records("audit trail")
            .expect("linked memory search works");
        let matched_ids = matches.iter().map(|memory| memory.id).collect::<Vec<_>>();

        assert_eq!(matches.len(), 2);
        assert!(matched_ids.contains(&briefing_memory.id));
        assert!(matched_ids.contains(&audit_memory.id));
    }

    #[test]
    fn search_memory_records_matches_linked_memory_bodies() {
        let store = EventStore::open_memory().expect("memory store opens");
        let briefing_task = TaskRecord::new(
            "Briefing tone rule".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let audit_task = TaskRecord::new(
            "Audit trail standard".to_string(),
            "Keep evidence and source traceability visible.".to_string(),
        )
        .expect("task is valid");
        let briefing_memory = MemoryRecord::from_task_record(&briefing_task);
        let audit_memory = MemoryRecord::from_task_record(&audit_task);
        let link = MemoryRecordLink::new(
            briefing_memory.id,
            audit_memory.id,
            None,
            MemoryRelationKind::Related,
            "Search should follow related memory bodies.".to_string(),
        )
        .expect("memory link is valid");

        store
            .append_memory_record(&briefing_memory)
            .expect("briefing memory appends");
        store
            .append_memory_record(&audit_memory)
            .expect("audit memory appends");
        store
            .append_memory_record_link(&link)
            .expect("memory link appends");

        let matches = store
            .search_memory_records("source traceability visible")
            .expect("linked memory body search works");
        let matched_ids = matches.iter().map(|memory| memory.id).collect::<Vec<_>>();

        assert_eq!(matches.len(), 2);
        assert!(matched_ids.contains(&briefing_memory.id));
        assert!(matched_ids.contains(&audit_memory.id));
    }

    #[test]
    fn appending_memory_record_does_not_persist_search_projection() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Projection-free memory".to_string(),
            "Search metadata should stay out of append-only memory events.".to_string(),
        )
        .expect("task is valid");
        let mut memory = MemoryRecord::from_task_record(&task);
        memory.search_match = MemorySearchMatch::linked(
            MemorySearchMatchSource::LinkedMemoryBody,
            Uuid::new_v4(),
            MemoryRelationKind::Related,
        );

        store.append_memory_record(&memory).expect("memory appends");

        let events = store
            .list_by_type(super::MEMORY_RECORD_CREATED_EVENT, 10)
            .expect("memory events load");
        assert_eq!(events.len(), 1);
        assert!(
            !events[0].payload_json.contains("search_match"),
            "search projection leaked into memory event payload: {}",
            events[0].payload_json
        );

        let memories = store.list_memory_records().expect("memories load");
        assert_eq!(memories.len(), 1);
        assert_eq!(
            memories[0].search_match.source,
            MemorySearchMatchSource::Direct
        );
    }

    #[test]
    fn appending_memory_record_does_not_persist_link_projection() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Link projection-free memory".to_string(),
            "Related memory summaries should stay out of append-only memory events.".to_string(),
        )
        .expect("task is valid");
        let mut memory = MemoryRecord::from_task_record(&task);
        let projected_link_id = Uuid::new_v4();
        memory.linked_memory_ids = vec![projected_link_id];
        memory.linked_memories = vec![MemoryRecordLinkSummary {
            id: projected_link_id,
            title: "Projected related memory".to_string(),
            memory_type: MemoryType::WorkflowRule,
            scope: MemoryScope::Project,
            relation: MemoryRelationKind::Related,
            note: String::new(),
            updated_at: Utc::now(),
        }];

        store.append_memory_record(&memory).expect("memory appends");

        let events = store
            .list_by_type(super::MEMORY_RECORD_CREATED_EVENT, 10)
            .expect("memory events load");
        let persisted_memory: MemoryRecord =
            serde_json::from_str(&events[0].payload_json).expect("memory event deserializes");
        assert!(
            persisted_memory.linked_memory_ids.is_empty(),
            "linked memory IDs leaked into memory event payload: {}",
            events[0].payload_json
        );
        assert!(
            persisted_memory.linked_memories.is_empty(),
            "linked memory summaries leaked into memory event payload: {}",
            events[0].payload_json
        );
    }

    #[test]
    fn search_memory_records_reports_linked_body_match_source() {
        let store = EventStore::open_memory().expect("memory store opens");
        let briefing_task = TaskRecord::new(
            "Briefing tone rule".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let audit_task = TaskRecord::new(
            "Audit trail standard".to_string(),
            "Keep evidence and source traceability visible.".to_string(),
        )
        .expect("task is valid");
        let briefing_memory = MemoryRecord::from_task_record(&briefing_task);
        let audit_memory = MemoryRecord::from_task_record(&audit_task);
        let link = MemoryRecordLink::new(
            briefing_memory.id,
            audit_memory.id,
            None,
            MemoryRelationKind::Derives,
            "Search provenance should explain linked body matches.".to_string(),
        )
        .expect("memory link is valid");

        store
            .append_memory_record(&briefing_memory)
            .expect("briefing memory appends");
        store
            .append_memory_record(&audit_memory)
            .expect("audit memory appends");
        store
            .append_memory_record_link(&link)
            .expect("memory link appends");

        let matches = store
            .search_memory_records("source traceability visible")
            .expect("linked memory body search works");
        let briefing_match = matches
            .iter()
            .find(|memory| memory.id == briefing_memory.id)
            .expect("briefing memory is matched through linked body");

        assert_eq!(
            briefing_match.search_match.source,
            MemorySearchMatchSource::LinkedMemoryBody
        );
        assert_eq!(
            briefing_match.search_match.linked_memory_id,
            Some(audit_memory.id)
        );
        assert_eq!(
            briefing_match.search_match.relation,
            Some(MemoryRelationKind::Derives)
        );
    }

    #[test]
    fn previewing_memory_candidate_merge_does_not_write_events() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let existing_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new_with_metadata(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants a richer reusable instruction.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&existing_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let preview = store
            .preview_memory_candidate_merge(candidate.id, vec![existing_memory.id])
            .expect("merge preview builds");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let links = store.list_memory_record_links().expect("links load");

        assert_eq!(preview.candidate_id, candidate.id);
        assert_eq!(preview.source_memory_ids, vec![existing_memory.id]);
        assert_eq!(preview.title, candidate.title);
        assert!(preview.body.contains(&existing_memory.body));
        assert!(preview.body.contains(&candidate.body));
        assert_eq!(preview.memory_type, MemoryType::WorkflowRule);
        assert_eq!(preview.scope, MemoryScope::Project);
        assert_eq!(preview.sensitivity, MemorySensitivity::Sensitive);
        assert_eq!(preview.lifecycle, MemoryLifecycle::Active);
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert_eq!(memories.len(), 1);
        assert!(links.is_empty());
    }

    #[test]
    fn previewing_memory_candidate_replace_does_not_write_events() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let existing_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new_with_metadata(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants the richer instruction to supersede the old one.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&existing_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let preview = store
            .preview_memory_candidate_replace(candidate.id, vec![existing_memory.id])
            .expect("replace preview builds");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let deletions = store
            .list_memory_record_deletions()
            .expect("deletions load");
        let links = store.list_memory_record_links().expect("links load");

        assert_eq!(preview.candidate_id, candidate.id);
        assert_eq!(preview.target_memory_ids, vec![existing_memory.id]);
        assert_eq!(preview.replacement_title, candidate.title);
        assert_eq!(preview.replacement_body, candidate.body);
        assert_eq!(preview.target_memories.len(), 1);
        assert_eq!(preview.target_memories[0].id, existing_memory.id);
        assert_eq!(preview.memory_type, MemoryType::WorkflowRule);
        assert_eq!(preview.scope, MemoryScope::Project);
        assert_eq!(preview.sensitivity, MemorySensitivity::Sensitive);
        assert_eq!(preview.lifecycle, MemoryLifecycle::Active);
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert_eq!(memories.len(), 1);
        assert!(deletions.is_empty());
        assert!(links.is_empty());
    }

    #[test]
    fn merging_memory_candidate_accepts_merged_memory_and_hides_sources() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let existing_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new_with_metadata(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants a richer reusable instruction.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&existing_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let resolution = store
            .merge_memory_candidate_with_conflicts(
                candidate.id,
                vec![existing_memory.id],
                "Merge and accept richer memory.".to_string(),
            )
            .expect("candidate merges");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let deletions = store
            .list_memory_record_deletions()
            .expect("deletions load");
        let links = store.list_memory_record_links().expect("links load");

        assert!(resolution.accepted);
        assert_eq!(resolution.candidate_id, candidate.id);
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Accepted
        );
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].source, MemoryRecordSource::MemoryCandidate);
        assert_eq!(memories[0].source_id, Some(candidate.id));
        assert!(memories[0].body.contains(&existing_memory.body));
        assert!(memories[0].body.contains(&candidate.body));
        assert_eq!(memories[0].memory_type, MemoryType::WorkflowRule);
        assert_eq!(memories[0].scope, MemoryScope::Project);
        assert_eq!(memories[0].sensitivity, MemorySensitivity::Sensitive);
        assert_eq!(deletions.len(), 1);
        assert_eq!(deletions[0].memory_id, existing_memory.id);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target_memory_id, existing_memory.id);
        assert_eq!(links[0].candidate_id, Some(candidate.id));
        assert_eq!(
            serde_json::to_value(&links[0]).expect("link serializes")["relation"],
            "derives"
        );
    }

    #[test]
    fn merging_memory_candidate_rejects_already_written_candidate_without_resolution() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let existing_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new_with_metadata(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants a richer reusable instruction.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");
        let already_written_memory = MemoryRecord::from_memory_candidate(&candidate);

        store
            .append_memory_record(&existing_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        store
            .append_memory_record(&already_written_memory)
            .expect("already-written candidate memory appends");
        let error = store
            .merge_memory_candidate_with_conflicts(
                candidate.id,
                vec![existing_memory.id],
                "Merge stale unresolved candidate.".to_string(),
            )
            .expect_err("already-written candidate must not resolve again");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let deletions = store
            .list_memory_record_deletions()
            .expect("deletions load");
        let links = store.list_memory_record_links().expect("links load");

        assert!(matches!(error, EventStoreError::InvalidState(_)));
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert!(memories
            .iter()
            .any(|memory| memory.id == existing_memory.id));
        assert!(deletions.is_empty());
        assert!(links.is_empty());
    }

    #[test]
    fn merging_memory_candidate_rejects_non_conflicting_sources_without_resolution() {
        let store = EventStore::open_memory().expect("memory store opens");
        let conflict_task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let unrelated_task = TaskRecord::new(
            "Source citation rule".to_string(),
            "Always keep source traceability visible.".to_string(),
        )
        .expect("task is valid");
        let conflict_memory = MemoryRecord::from_task_record(&conflict_task);
        let unrelated_memory = MemoryRecord::from_task_record(&unrelated_task);
        let candidate = MemoryCandidate::new_with_metadata(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants a richer reusable instruction.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&conflict_memory)
            .expect("conflict memory appends");
        store
            .append_memory_record(&unrelated_memory)
            .expect("unrelated memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let error = store
            .merge_memory_candidate_with_conflicts(
                candidate.id,
                vec![unrelated_memory.id],
                "Attempt merge with non-conflicting memory.".to_string(),
            )
            .expect_err("non-conflicting merge source is rejected");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let deletions = store
            .list_memory_record_deletions()
            .expect("deletions load");
        let links = store.list_memory_record_links().expect("links load");

        assert!(matches!(error, EventStoreError::InvalidState(_)));
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert!(memories
            .iter()
            .any(|memory| memory.id == conflict_memory.id));
        assert!(memories
            .iter()
            .any(|memory| memory.id == unrelated_memory.id));
        assert!(deletions.is_empty());
        assert!(links.is_empty());
    }

    #[test]
    fn replacing_memory_candidate_accepts_replacement_and_tombstones_targets() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let existing_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new_with_metadata(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants the richer instruction to supersede the old one.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&existing_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let resolution = store
            .replace_memory_candidate_conflicts(
                candidate.id,
                vec![existing_memory.id],
                "Replace with accepted candidate.".to_string(),
            )
            .expect("candidate replaces");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let deletions = store
            .list_memory_record_deletions()
            .expect("deletions load");
        let links = store.list_memory_record_links().expect("links load");

        assert!(resolution.accepted);
        assert_eq!(resolution.candidate_id, candidate.id);
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Accepted
        );
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].source, MemoryRecordSource::MemoryCandidate);
        assert_eq!(memories[0].source_id, Some(candidate.id));
        assert_eq!(memories[0].body, candidate.body);
        assert!(!memories[0].body.contains(&existing_memory.body));
        assert_eq!(deletions.len(), 1);
        assert_eq!(deletions[0].memory_id, existing_memory.id);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target_memory_id, existing_memory.id);
        assert_eq!(links[0].candidate_id, Some(candidate.id));
        assert_eq!(
            serde_json::to_value(&links[0]).expect("link serializes")["relation"],
            "updates"
        );
    }

    #[test]
    fn updating_memory_candidate_conflict_updates_target_without_writing_new_memory() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let existing_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new_with_metadata(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User said the existing memory should be updated, not duplicated.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&existing_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let resolution = store
            .update_memory_candidate_conflict(
                candidate.id,
                existing_memory.id,
                "Update existing memory from accepted candidate.".to_string(),
            )
            .expect("candidate updates existing memory");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let updates = store.list_memory_record_updates().expect("updates load");

        assert!(resolution.accepted);
        assert_eq!(resolution.candidate_id, candidate.id);
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Accepted
        );
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].id, existing_memory.id);
        assert_eq!(memories[0].source, MemoryRecordSource::TaskRecord);
        assert_eq!(memories[0].source_id, Some(task.id));
        assert_eq!(memories[0].title, candidate.title);
        assert_eq!(memories[0].body, candidate.body);
        assert_eq!(memories[0].memory_type, MemoryType::WorkflowRule);
        assert_eq!(memories[0].scope, MemoryScope::Project);
        assert_eq!(memories[0].sensitivity, MemorySensitivity::Sensitive);
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].memory_id, existing_memory.id);
        assert_eq!(
            updates[0].note,
            "Update existing memory from accepted candidate."
        );
    }

    #[test]
    fn archiving_memory_candidate_conflicts_resolves_candidate_and_hides_targets() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Outdated project rule".to_string(),
            "Use the old weekly review flow.".to_string(),
        )
        .expect("task is valid");
        let stale_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new(
            "Outdated project rule".to_string(),
            "This memory is stale and should not guide retrieval.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User marked the selected memory as stale.".to_string(),
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&stale_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let resolution = store
            .archive_memory_candidate_conflicts(
                candidate.id,
                vec![stale_memory.id],
                "Archive stale target from candidate review.".to_string(),
            )
            .expect("candidate archives stale target");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let deletions = store
            .list_memory_record_deletions()
            .expect("deletions load");

        assert!(resolution.accepted);
        assert_eq!(resolution.candidate_id, candidate.id);
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Accepted
        );
        assert!(memories.is_empty());
        assert_eq!(deletions.len(), 1);
        assert_eq!(deletions[0].memory_id, stale_memory.id);
        assert_eq!(
            deletions[0].note,
            "Archive stale target from candidate review."
        );
    }

    #[test]
    fn replacing_memory_candidate_rejects_already_written_candidate_without_resolution() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let existing_memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new_with_metadata(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants the richer instruction to supersede the old one.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");
        let already_written_memory = MemoryRecord::from_memory_candidate(&candidate);

        store
            .append_memory_record(&existing_memory)
            .expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        store
            .append_memory_record(&already_written_memory)
            .expect("already-written candidate memory appends");
        let error = store
            .replace_memory_candidate_conflicts(
                candidate.id,
                vec![existing_memory.id],
                "Replace stale unresolved candidate.".to_string(),
            )
            .expect_err("already-written candidate must not resolve again");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let deletions = store
            .list_memory_record_deletions()
            .expect("deletions load");
        let links = store.list_memory_record_links().expect("links load");

        assert!(matches!(error, EventStoreError::InvalidState(_)));
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert!(memories
            .iter()
            .any(|memory| memory.id == existing_memory.id));
        assert!(deletions.is_empty());
        assert!(links.is_empty());
    }

    #[test]
    fn replacing_memory_candidate_rejects_non_conflicting_targets_without_resolution() {
        let store = EventStore::open_memory().expect("memory store opens");
        let conflict_task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let unrelated_task = TaskRecord::new(
            "Source citation rule".to_string(),
            "Always keep source traceability visible.".to_string(),
        )
        .expect("task is valid");
        let conflict_memory = MemoryRecord::from_task_record(&conflict_task);
        let unrelated_memory = MemoryRecord::from_task_record(&unrelated_task);
        let candidate = MemoryCandidate::new_with_metadata(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User wants the richer instruction to supersede the old one.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");

        store
            .append_memory_record(&conflict_memory)
            .expect("conflict memory appends");
        store
            .append_memory_record(&unrelated_memory)
            .expect("unrelated memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let error = store
            .replace_memory_candidate_conflicts(
                candidate.id,
                vec![unrelated_memory.id],
                "Attempt replace with non-conflicting memory.".to_string(),
            )
            .expect_err("non-conflicting replace target is rejected");
        let candidate_records = store
            .list_memory_candidate_records()
            .expect("candidate records load");
        let memories = store.list_memory_records().expect("memories load");
        let deletions = store
            .list_memory_record_deletions()
            .expect("deletions load");
        let links = store.list_memory_record_links().expect("links load");

        assert!(matches!(error, EventStoreError::InvalidState(_)));
        assert_eq!(
            candidate_records[0].effective_status,
            MemoryCandidateStatus::Pending
        );
        assert!(memories
            .iter()
            .any(|memory| memory.id == conflict_memory.id));
        assert!(memories
            .iter()
            .any(|memory| memory.id == unrelated_memory.id));
        assert!(deletions.is_empty());
        assert!(links.is_empty());
    }

    #[test]
    fn legacy_memory_record_links_default_to_related_relation() {
        let source_memory_id = Uuid::new_v4();
        let target_memory_id = Uuid::new_v4();
        let legacy_link: crate::kernel::models::MemoryRecordLink =
            serde_json::from_value(serde_json::json!({
                "id": Uuid::new_v4(),
                "source_memory_id": source_memory_id,
                "target_memory_id": target_memory_id,
                "candidate_id": null,
                "note": "Legacy link event without relation.",
                "created_at": Utc::now()
            }))
            .expect("legacy link deserializes");

        assert_eq!(
            serde_json::to_value(&legacy_link).expect("link serializes")["relation"],
            "related"
        );
    }

    #[test]
    fn rejecting_memory_candidate_does_not_write_memory() {
        let store = EventStore::open_memory().expect("memory store opens");
        let candidate = MemoryCandidate::new(
            "Temporary report instruction".to_string(),
            "Only applies to today's draft.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User proposed this as reusable guidance.".to_string(),
        )
        .expect("candidate is valid");

        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        store
            .resolve_memory_candidate(candidate.id, false, "Too temporary.".to_string())
            .expect("candidate rejects");
        let resolved = store
            .list_memory_candidate_records()
            .expect("candidates reload");
        let memories = store.list_memory_records().expect("memories load");

        assert_eq!(
            resolved[0].effective_status,
            MemoryCandidateStatus::Rejected
        );
        assert!(memories.is_empty());
    }

    #[test]
    fn memory_candidate_records_surface_conflicting_memory_ids() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Preferred report tone".to_string(),
            "Use concise operating language.".to_string(),
        )
        .expect("task is valid");
        let memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new(
            "Preferred report tone".to_string(),
            "Use concise operating language with owners and evidence.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User proposed this as reusable guidance.".to_string(),
        )
        .expect("candidate is valid");

        store.append_memory_record(&memory).expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let records = store
            .list_memory_candidate_records()
            .expect("candidates load");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].conflicting_memory_ids, vec![memory.id]);
        assert_eq!(records[0].conflicting_memories.len(), 1);
        assert_eq!(records[0].conflicting_memories[0].id, memory.id);
        assert_eq!(
            records[0].conflicting_memories[0].title,
            "Preferred report tone"
        );
        assert_eq!(
            records[0].conflicting_memories[0].body,
            "Use concise operating language."
        );
    }

    #[test]
    fn memory_candidate_source_id_keeps_a_rewritten_update_bound_to_its_target() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Previous workflow wording".to_string(),
            "Use the previous workflow instructions.".to_string(),
        )
        .expect("task is valid");
        let memory = MemoryRecord::from_task_record(&task);
        let mut candidate = MemoryCandidate::new(
            "Replacement operating principle".to_string(),
            "A fully rewritten instruction with no shared phrasing.".to_string(),
            MemoryCandidateSource::Manual,
            Some(memory.id),
            "Model-assisted maintenance rewrite.".to_string(),
        )
        .expect("candidate is valid");
        candidate.suggested_action = MemoryCandidateSuggestedAction::Update;

        store.append_memory_record(&memory).expect("memory appends");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let records = store
            .list_memory_candidate_records()
            .expect("candidates load");

        assert_eq!(records[0].conflicting_memory_ids, vec![memory.id]);
    }

    #[test]
    fn memory_candidate_conflicts_ignore_deleted_memories() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Retired memory".to_string(),
            "This memory is no longer active.".to_string(),
        )
        .expect("task is valid");
        let memory = MemoryRecord::from_task_record(&task);
        let candidate = MemoryCandidate::new(
            "Retired memory".to_string(),
            "A fresh candidate with the same title.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User proposed this as reusable guidance.".to_string(),
        )
        .expect("candidate is valid");

        store.append_memory_record(&memory).expect("memory appends");
        store
            .delete_memory_record(memory.id, "No longer useful.".to_string())
            .expect("memory deletes");
        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        let records = store
            .list_memory_candidate_records()
            .expect("candidates load");

        assert_eq!(records.len(), 1);
        assert!(records[0].conflicting_memory_ids.is_empty());
    }

    #[test]
    fn deleting_memory_record_hides_it_from_list_and_search() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Memory cleanup".to_string(),
            "Keep accepted memory reviewable.".to_string(),
        )
        .expect("task is valid");
        let memory = MemoryRecord::from_task_record(&task);

        store.append_memory_record(&memory).expect("memory appends");
        let deletion = store
            .delete_memory_record(memory.id, "No longer useful.".to_string())
            .expect("memory deletes");

        assert_eq!(deletion.memory_id, memory.id);
        assert_eq!(deletion.note, "No longer useful.");
        assert!(store
            .list_memory_records()
            .expect("memories load")
            .is_empty());
        assert!(store
            .search_memory_records("cleanup")
            .expect("memories search")
            .is_empty());
    }

    #[test]
    fn deleting_missing_memory_record_returns_not_found() {
        let store = EventStore::open_memory().expect("memory store opens");
        let error = store
            .delete_memory_record(Uuid::new_v4(), "Missing.".to_string())
            .expect_err("missing memory cannot be deleted");

        assert!(matches!(error, EventStoreError::NotFound(_)));
    }

    #[test]
    fn updating_memory_record_replaces_visible_version_for_list_and_search() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Old memory title".to_string(),
            "Old body should stop matching searches.".to_string(),
        )
        .expect("task is valid");
        let memory = MemoryRecord::from_task_record(&task);

        store.append_memory_record(&memory).expect("memory appends");
        let update = store
            .update_memory_record(
                memory.id,
                "Updated memory title".to_string(),
                "New body should be searchable.".to_string(),
                MemoryType::WorkflowRule,
                MemoryScope::Project,
                MemorySensitivity::Sensitive,
                MemoryLifecycle::Archived,
                None,
                "User corrected the accepted memory.".to_string(),
            )
            .expect("memory updates");

        let memories = store.list_memory_records().expect("memories load");
        let old_matches = store
            .search_memory_records("old body")
            .expect("old body search works");
        let new_matches = store
            .search_memory_records("new body")
            .expect("new body search works");

        assert_eq!(update.memory_id, memory.id);
        assert_eq!(update.note, "User corrected the accepted memory.");
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].id, memory.id);
        assert_eq!(memories[0].title, "Updated memory title");
        assert_eq!(memories[0].body, "New body should be searchable.");
        assert_eq!(memories[0].memory_type, MemoryType::WorkflowRule);
        assert_eq!(memories[0].scope, MemoryScope::Project);
        assert_eq!(memories[0].sensitivity, MemorySensitivity::Sensitive);
        assert_eq!(memories[0].lifecycle, MemoryLifecycle::Archived);
        assert_eq!(memories[0].source, MemoryRecordSource::TaskRecord);
        assert_eq!(memories[0].source_id, Some(task.id));
        assert_eq!(memories[0].created_at, memory.created_at);
        assert!(memories[0].updated_at >= memory.updated_at);
        assert!(old_matches.is_empty());
        assert_eq!(new_matches.len(), 1);
        assert_eq!(new_matches[0].id, memory.id);
    }

    #[test]
    fn updating_deleted_memory_record_returns_not_found() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Deleted memory".to_string(),
            "This memory should not be editable.".to_string(),
        )
        .expect("task is valid");
        let memory = MemoryRecord::from_task_record(&task);

        store.append_memory_record(&memory).expect("memory appends");
        store
            .delete_memory_record(memory.id, "Remove before editing.".to_string())
            .expect("memory deletes");
        let error = store
            .update_memory_record(
                memory.id,
                "Edited deleted memory".to_string(),
                "This should not be written.".to_string(),
                MemoryType::Preference,
                MemoryScope::Workspace,
                MemorySensitivity::Normal,
                MemoryLifecycle::Active,
                None,
                "Attempted edit after deletion.".to_string(),
            )
            .expect_err("deleted memory cannot be updated");

        assert!(matches!(error, EventStoreError::NotFound(_)));
    }

    #[test]
    fn selected_memory_feedback_appends_without_mutating_memory_records() {
        let store = EventStore::open_memory().expect("memory store opens");
        let task = TaskRecord::new(
            "Project memory rule".to_string(),
            "Keep selected memory snippets compact.".to_string(),
        )
        .expect("task is valid");
        let memory = MemoryRecord::from_task_record(&task);
        let receipt_id = Uuid::new_v4();

        store.append_memory_record(&memory).expect("memory appends");
        let feedback = store
            .record_selected_memory_feedback(
                memory.id,
                Some(receipt_id),
                MemorySelectedFeedbackKind::ShouldUpdate,
                "The selected memory is useful but needs fresher wording.".to_string(),
            )
            .expect("feedback appends");
        let feedback_events = store
            .list_selected_memory_feedback()
            .expect("feedback events load");
        let memories = store.list_memory_records().expect("memories load");

        assert_eq!(feedback.memory_id, memory.id);
        assert_eq!(feedback.context_receipt_id, Some(receipt_id));
        assert_eq!(feedback.feedback, MemorySelectedFeedbackKind::ShouldUpdate);
        assert_eq!(
            feedback.note,
            "The selected memory is useful but needs fresher wording."
        );
        assert_eq!(feedback_events, vec![feedback]);
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].id, memory.id);
        assert_eq!(memories[0].title, memory.title);
        assert_eq!(memories[0].body, memory.body);
    }

    #[test]
    fn expired_memory_records_are_hidden_from_list_and_search() {
        let store = EventStore::open_memory().expect("memory store opens");
        let now = Utc::now();
        let mut memory = MemoryRecord::from_task_record(
            &TaskRecord::new(
                "Expired operating note".to_string(),
                "This instruction should no longer guide the agent.".to_string(),
            )
            .expect("task is valid"),
        );
        memory.lifecycle = MemoryLifecycle::Expires;
        memory.expires_at = Some(now - Duration::days(1));

        store.append_memory_record(&memory).expect("memory appends");

        assert!(store
            .list_memory_records_at(now)
            .expect("memories load")
            .is_empty());
        assert!(store
            .search_memory_records_at("operating", now)
            .expect("memories search")
            .is_empty());
    }

    #[test]
    fn future_expiring_memory_candidate_preserves_expiration_when_accepted() {
        let store = EventStore::open_memory().expect("memory store opens");
        let now = Utc::now();
        let expires_at = now + Duration::days(30);
        let candidate = MemoryCandidate::new_with_metadata_and_expiration(
            "Quarterly briefing rule".to_string(),
            "Use this guidance until the current quarterly cycle closes.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User proposed this as time-bound guidance.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Normal,
            MemoryLifecycle::Expires,
            Some(expires_at),
        )
        .expect("candidate is valid");

        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        store
            .resolve_memory_candidate(candidate.id, true, "Accept timed rule.".to_string())
            .expect("candidate resolves");
        let memories = store.list_memory_records_at(now).expect("memories load");

        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].title, candidate.title);
        assert_eq!(memories[0].lifecycle, MemoryLifecycle::Expires);
        assert_eq!(memories[0].expires_at, Some(expires_at));
    }

    #[test]
    fn memory_metadata_accepting_candidate_preserves_review_tags() {
        let store = EventStore::open_memory().expect("memory store opens");
        let candidate = MemoryCandidate::new_with_metadata(
            "Evidence routing rule".to_string(),
            "Keep source scans as the highest authority when restoring text.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "User confirmed this as reusable workflow guidance.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Sensitive,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");

        store
            .append_memory_candidate(&candidate)
            .expect("candidate appends");
        store
            .resolve_memory_candidate(candidate.id, true, "Promote rule.".to_string())
            .expect("candidate resolves");
        let memories = store.list_memory_records().expect("memories load");

        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].source_id, Some(candidate.id));
        assert_eq!(memories[0].memory_type, MemoryType::WorkflowRule);
        assert_eq!(memories[0].scope, MemoryScope::Project);
        assert_eq!(memories[0].sensitivity, MemorySensitivity::Sensitive);
        assert_eq!(memories[0].lifecycle, MemoryLifecycle::Active);
    }

    #[test]
    fn appends_and_lists_permission_audit_entries() {
        let store = EventStore::open_memory().expect("memory store opens");
        let entry =
            PermissionAuditEntry::evaluate(AccessMode::AskOnRisk, CapabilityKind::BrowserBrowse);

        store
            .append_permission_audit_entry(&entry)
            .expect("permission audit appends");
        let entries = store
            .list_permission_audit_entries()
            .expect("permission audits load");

        assert_eq!(entries, vec![entry]);
    }

    #[test]
    fn appends_and_lists_deepseek_chat_telemetry() {
        let store = EventStore::open_memory().expect("memory store opens");
        let telemetry = DeepSeekChatTelemetry {
            id: Uuid::new_v4(),
            request_hash: "abc123".to_string(),
            model: "deepseek-v4-pro".to_string(),
            cache_status: DeepSeekChatCacheStatus::Miss,
            elapsed_ms: 42,
            prompt_tokens: Some(100),
            completion_tokens: Some(20),
            total_tokens: Some(120),
            estimated_cost_micro_usd: None,
            created_at: chrono::Utc::now(),
        };

        store
            .append_deepseek_chat_telemetry(&telemetry)
            .expect("telemetry appends");
        let entries = store
            .list_deepseek_chat_telemetry()
            .expect("telemetry loads");

        assert_eq!(entries, vec![telemetry]);
    }

    #[test]
    fn appends_and_lists_agent_context_receipts() {
        let store = EventStore::open_memory().expect("memory store opens");
        let mut receipt =
            AgentContextReceipt::new("file_read", "succeeded", "auto", "fast", "cache: miss");
        receipt
            .selected_evidence
            .push("target:reports/source.md".to_string());
        receipt
            .validation_results
            .push("capability invocation recorded".to_string());
        receipt
            .intentional_omissions
            .push("raw user prompt not stored".to_string());

        store
            .append_agent_context_receipt(&receipt)
            .expect("context receipt appends");
        let receipts = store
            .list_agent_context_receipts()
            .expect("context receipts load");

        assert_eq!(receipts, vec![receipt]);
    }

    #[test]
    fn resolves_pending_capability_access_request() {
        let store = EventStore::open_memory().expect("memory store opens");
        let request = request_capability_access(AccessMode::FullAccess, CapabilityKind::EmailSend)
            .expect("email send request builds");

        store
            .append_capability_access_request(&request)
            .expect("request appends");
        let pending = store
            .list_pending_capability_access_records()
            .expect("pending requests load");
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].effective_status,
            CapabilityAccessStatus::PendingApproval
        );

        let resolution = store
            .resolve_capability_access_request(
                request.id,
                true,
                "Approved after user reviewed the outbound message.".to_string(),
            )
            .expect("request resolves");
        let pending_after_resolution = store
            .list_pending_capability_access_records()
            .expect("pending requests reload");
        let records = store
            .list_capability_access_records()
            .expect("access records load");

        assert!(resolution.approved);
        assert!(pending_after_resolution.is_empty());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].request.id, request.id);
        assert_eq!(
            records[0].effective_status,
            CapabilityAccessStatus::Approved
        );
        assert_eq!(
            records[0]
                .resolution
                .as_ref()
                .expect("resolution exists")
                .note,
            "Approved after user reviewed the outbound message."
        );
    }

    #[test]
    fn auto_approved_capability_access_request_is_not_pending() {
        let store = EventStore::open_memory().expect("memory store opens");
        let request = request_capability_access(AccessMode::AskOnRisk, CapabilityKind::DriveRead)
            .expect("drive read request builds");

        store
            .append_capability_access_request(&request)
            .expect("request appends");
        let pending = store
            .list_pending_capability_access_records()
            .expect("pending requests load");
        let records = store
            .list_capability_access_records()
            .expect("access records load");

        assert!(pending.is_empty());
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].effective_status,
            CapabilityAccessStatus::AutoApproved
        );
    }

    #[test]
    fn appends_and_lists_capability_invocations() {
        let store = EventStore::open_memory().expect("memory store opens");
        let invocation = CapabilityInvocation {
            id: uuid::Uuid::new_v4(),
            capability: CapabilityKind::BrowserBrowse,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: crate::kernel::policy::PolicyDecision::Allow,
            approval_request_id: None,
            requested_resource: Some("https://example.com".to_string()),
            evidence_ref: Some("https://example.com/final".to_string()),
            requested_url: Some("https://example.com".to_string()),
            evidence_url: Some("https://example.com/final".to_string()),
            title: Some("Example Domain".to_string()),
            excerpt: Some("Example evidence text".to_string()),
            warnings: Vec::new(),
            elapsed_ms: 24,
            created_at: chrono::Utc::now(),
        };

        store
            .append_capability_invocation(&invocation)
            .expect("invocation appends");
        let invocations = store
            .list_capability_invocations()
            .expect("invocations load");

        assert_eq!(invocations, vec![invocation]);
    }

    #[test]
    fn appends_and_lists_verified_tool_invocations() {
        let store = EventStore::open_memory().expect("memory store opens");
        let plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: APP_UPDATE_CHECK_TOOL_ID.to_string(),
            input: json!({}),
            access_mode: AccessMode::AskOnRisk,
            run_id: None,
        })
        .expect("tool execution plan");
        let invocation = ToolInvocationRecord::succeeded(
            &plan,
            json!({"current_version": "0.1.2", "update_available": false}),
            vec![ToolEvidence {
                kind: "release_status".to_string(),
                reference: "github:Lee-take/deepseek-agent-os/releases".to_string(),
                summary: "Release status was parsed.".to_string(),
            }],
            ToolVerificationResult::passed("release status verified"),
            None,
            18,
        )
        .expect("verified tool invocation");

        store
            .append_tool_invocation(&invocation)
            .expect("tool invocation appends");
        let invocations = store
            .list_tool_invocations()
            .expect("tool invocations load");

        assert_eq!(invocations, vec![invocation]);
    }

    #[test]
    fn appends_and_lists_operations_briefing_runs() {
        let store = EventStore::open_memory().expect("memory store opens");
        let run = OperationsBriefingRun {
            id: uuid::Uuid::new_v4(),
            workflow_id: OPERATIONS_BRIEFING_WORKFLOW_ID.to_string(),
            status: OperationsBriefingRunStatus::DraftReady,
            archived_from_package: false,
            evidence_folder_path: Some("fixtures/evidence".to_string()),
            evidence_invocation_id: Some(uuid::Uuid::new_v4()),
            title: "Operations Briefing Draft".to_string(),
            summary: "Draft ready from evidence folder manifest.".to_string(),
            anomalies: vec![OperationsBriefingAnomaly {
                area: "Evidence review".to_string(),
                signal: "Review accepted text files.".to_string(),
                evidence_ref: Some("fixtures/evidence".to_string()),
            }],
            action_plan: vec![OperationsBriefingAction {
                owner: "Operations owner".to_string(),
                action: "Confirm evidence set.".to_string(),
                due_hint: "Next briefing cycle".to_string(),
            }],
            warnings: Vec::new(),
            context_receipt: Default::default(),
            created_at: chrono::Utc::now(),
        };

        store
            .append_operations_briefing_run(&run)
            .expect("operations briefing run appends");
        let runs = store
            .list_operations_briefing_runs()
            .expect("operations briefing runs load");

        assert_eq!(runs, vec![run]);
    }

    #[test]
    fn archive_replay_imports_new_runs_as_archived_and_skips_existing_ids() {
        let store = EventStore::open_memory().expect("memory store opens");
        let existing = sample_operations_briefing_run();
        let incoming = sample_operations_briefing_run();
        store
            .append_operations_briefing_run(&existing)
            .expect("existing run appends");

        let summary = store
            .import_operations_briefing_runs(&[existing.clone(), incoming.clone()])
            .expect("runs import");
        let runs = store
            .list_operations_briefing_runs()
            .expect("operations briefing runs load");
        let imported = runs
            .iter()
            .find(|run| run.id == incoming.id)
            .expect("incoming run is imported");
        let existing_after_import = runs
            .iter()
            .find(|run| run.id == existing.id)
            .expect("existing run is still present");

        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped, 1);
        assert!(imported.archived_from_package);
        assert!(!existing_after_import.archived_from_package);
    }

    #[test]
    fn archive_replay_import_redacts_local_evidence_handles() {
        let store = EventStore::open_memory().expect("memory store opens");
        let mut incoming = sample_operations_briefing_run();
        incoming.evidence_folder_path = Some("D:\\operator\\private-evidence".to_string());
        incoming.evidence_invocation_id = Some(uuid::Uuid::new_v4());
        incoming.anomalies[0].evidence_ref = Some("source file: revenue.md".to_string());

        store
            .import_operations_briefing_runs(&[incoming.clone()])
            .expect("run imports");
        let runs = store
            .list_operations_briefing_runs()
            .expect("operations briefing runs load");
        let imported = runs
            .iter()
            .find(|run| run.id == incoming.id)
            .expect("incoming run is imported");

        assert!(imported.archived_from_package);
        assert_eq!(imported.evidence_folder_path, None);
        assert_eq!(imported.evidence_invocation_id, None);
        assert_eq!(
            imported.anomalies[0].evidence_ref.as_deref(),
            Some("source file: revenue.md")
        );
    }

    #[test]
    fn archive_replay_import_redacts_local_evidence_path_mentions() {
        let store = EventStore::open_memory().expect("memory store opens");
        let local_evidence_path = "D:\\operator\\private-evidence".to_string();
        let mut incoming = sample_operations_briefing_run();
        incoming.evidence_folder_path = Some(local_evidence_path.clone());
        incoming.evidence_invocation_id = Some(uuid::Uuid::new_v4());
        incoming.summary = format!("Imported summary referenced {local_evidence_path}.");
        incoming.anomalies[0].signal =
            format!("Imported anomaly referenced {local_evidence_path}.");
        incoming.anomalies[0].evidence_ref = Some(format!("{local_evidence_path}\\revenue.md"));
        incoming.action_plan[0].action =
            format!("Imported action referenced {local_evidence_path}.");
        incoming.warnings = vec![format!(
            "Imported warning referenced {local_evidence_path}."
        )];

        store
            .import_operations_briefing_runs(&[incoming.clone()])
            .expect("run imports");
        let runs = store
            .list_operations_briefing_runs()
            .expect("operations briefing runs load");
        let imported = runs
            .iter()
            .find(|run| run.id == incoming.id)
            .expect("incoming run is imported");
        let imported_json = serde_json::to_string(imported).expect("run serializes");

        assert!(imported
            .summary
            .contains("redacted source-machine evidence handle"));
        assert!(imported.anomalies[0]
            .signal
            .contains("redacted source-machine evidence handle"));
        assert_eq!(
            imported.anomalies[0].evidence_ref.as_deref(),
            Some("redacted source-machine evidence handle")
        );
        assert!(imported.action_plan[0]
            .action
            .contains("redacted source-machine evidence handle"));
        assert!(imported.warnings[0].contains("redacted source-machine evidence handle"));
        assert!(!imported_json.contains("private-evidence"));
        assert!(!imported_json.contains("operator"));
    }

    #[test]
    fn reusable_capability_grant_requires_explicit_user_approval() {
        let store = EventStore::open_memory().expect("memory store opens");
        let auto_request =
            request_capability_access(AccessMode::LimitedAuto, CapabilityKind::ComputerScreenshot)
                .expect("auto-approved screenshot request builds");
        assert_eq!(auto_request.decision, PolicyDecision::Allow);
        store
            .append_capability_access_request(&auto_request)
            .expect("auto-approved request appends");

        assert!(!store
            .has_user_approved_capability(CapabilityKind::ComputerScreenshot)
            .expect("grant check works"));

        let pending_request =
            request_capability_access(AccessMode::AskOnRisk, CapabilityKind::ComputerScreenshot)
                .expect("pending screenshot request builds");
        assert_eq!(pending_request.decision, PolicyDecision::Ask);
        store
            .append_capability_access_request(&pending_request)
            .expect("pending request appends");
        store
            .resolve_capability_access_request(
                pending_request.id,
                true,
                "User approved screen capture.".to_string(),
            )
            .expect("pending request resolves");

        assert!(store
            .has_user_approved_capability(CapabilityKind::ComputerScreenshot)
            .expect("grant check works"));
        let records = store
            .list_capability_access_records()
            .expect("records load");
        let approved_record = records
            .iter()
            .find(|record| record.request.id == pending_request.id)
            .expect("approved screenshot record exists");
        assert_eq!(approved_record.grant_state, CapabilityGrantState::Reusable);
    }

    #[test]
    fn critical_capability_approval_is_consumed_after_next_invocation() {
        let store = EventStore::open_memory().expect("memory store opens");
        let request = request_capability_access(AccessMode::FullAccess, CapabilityKind::EmailSend)
            .expect("critical request builds");
        store
            .append_capability_access_request(&request)
            .expect("request appends");
        let resolution = store
            .resolve_capability_access_request(
                request.id,
                true,
                "Approved one outbound email.".to_string(),
            )
            .expect("request resolves");

        assert!(store
            .has_user_approved_capability(CapabilityKind::EmailSend)
            .expect("grant check works"));
        let records_before_invocation = store
            .list_capability_access_records()
            .expect("records load before invocation");
        assert_eq!(
            records_before_invocation[0].grant_state,
            CapabilityGrantState::OneShotAvailable
        );

        store
            .append_capability_invocation(&CapabilityInvocation {
                id: uuid::Uuid::new_v4(),
                capability: CapabilityKind::EmailSend,
                status: CapabilityInvocationStatus::Failed,
                policy_decision: crate::kernel::policy::PolicyDecision::Ask,
                approval_request_id: Some(request.id),
                requested_resource: Some("ops@example.com".to_string()),
                evidence_ref: Some("ops@example.com".to_string()),
                requested_url: None,
                evidence_url: None,
                title: Some("Email send blocked: Weekly brief".to_string()),
                excerpt: Some("Approved email send attempt.".to_string()),
                warnings: vec!["email send execution is not enabled".to_string()],
                elapsed_ms: 1,
                created_at: resolution.created_at + chrono::Duration::milliseconds(1),
            })
            .expect("invocation appends");

        assert!(!store
            .has_user_approved_capability(CapabilityKind::EmailSend)
            .expect("grant check works"));
        let records_after_invocation = store
            .list_capability_access_records()
            .expect("records load after invocation");
        assert_eq!(
            records_after_invocation[0].grant_state,
            CapabilityGrantState::OneShotConsumed
        );
    }

    #[test]
    fn critical_capability_consumption_prefers_explicit_approval_request_id() {
        let store = EventStore::open_memory().expect("memory store opens");
        let first_request =
            request_capability_access(AccessMode::FullAccess, CapabilityKind::EmailSend)
                .expect("first critical request builds");
        store
            .append_capability_access_request(&first_request)
            .expect("first request appends");
        store
            .resolve_capability_access_request(
                first_request.id,
                true,
                "Approved first outbound email.".to_string(),
            )
            .expect("first request resolves");

        let second_request =
            request_capability_access(AccessMode::FullAccess, CapabilityKind::EmailSend)
                .expect("second critical request builds");
        store
            .append_capability_access_request(&second_request)
            .expect("second request appends");
        let second_resolution = store
            .resolve_capability_access_request(
                second_request.id,
                true,
                "Approved second outbound email.".to_string(),
            )
            .expect("second request resolves");

        store
            .append_capability_invocation(&CapabilityInvocation {
                id: uuid::Uuid::new_v4(),
                capability: CapabilityKind::EmailSend,
                status: CapabilityInvocationStatus::Failed,
                policy_decision: crate::kernel::policy::PolicyDecision::Ask,
                approval_request_id: Some(first_request.id),
                requested_resource: Some("ops@example.com".to_string()),
                evidence_ref: Some("ops@example.com".to_string()),
                requested_url: None,
                evidence_url: None,
                title: Some("Email send blocked: First brief".to_string()),
                excerpt: Some("First approved email send attempt.".to_string()),
                warnings: vec!["email send execution is not enabled".to_string()],
                elapsed_ms: 1,
                created_at: second_resolution.created_at + chrono::Duration::milliseconds(1),
            })
            .expect("linked invocation appends");

        let records = store
            .list_capability_access_records()
            .expect("records load");
        let first_record = records
            .iter()
            .find(|record| record.request.id == first_request.id)
            .expect("first record exists");
        let second_record = records
            .iter()
            .find(|record| record.request.id == second_request.id)
            .expect("second record exists");

        assert_eq!(
            first_record.grant_state,
            CapabilityGrantState::OneShotConsumed
        );
        assert_eq!(
            second_record.grant_state,
            CapabilityGrantState::OneShotAvailable
        );
    }

    #[test]
    fn connector_attachment_reservation_consumes_exact_tool_approval_atomically() {
        use crate::kernel::connectors::landing::ConnectorAttachmentMetadata;
        use crate::kernel::connectors::{
            ConnectorAccount, ConnectorCapability, ConnectorCredentialHandle, ConnectorHealth,
        };
        use crate::kernel::policy::CapabilityGrantState;
        use crate::kernel::tool_runtime::ToolExecutionStatus;

        let store = EventStore::open_memory().unwrap();
        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            display_name: "Attachment test account".to_string(),
            tenant_ref: Some("tenant:test".to_string()),
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailReadAttachment],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        store.upsert_connector_account(&account).unwrap();
        let metadata = ConnectorAttachmentMetadata {
            account_id: account.id,
            provider_id: "microsoft".to_string(),
            parent_remote_ref: "private-message-marker".to_string(),
            attachment_remote_ref: "private-attachment-marker".to_string(),
            file_name: "report.pdf".to_string(),
            declared_media_type: "application/pdf".to_string(),
            size_bytes: 4096,
            contains_macros: false,
            untrusted_evidence: true,
        };
        let workspace = tempfile::tempdir().unwrap();
        let (approval, tool_record) = store
            .prepare_connector_attachment_download_approval(&metadata, workspace.path(), None, now)
            .expect("exact attachment approval prepares atomically");
        let request = approval.request;
        let scope = request.exact_tool.clone().unwrap();
        let (replayed_approval, replayed_tool) = store
            .prepare_connector_attachment_download_approval(&metadata, workspace.path(), None, now)
            .expect("prepare retry returns the existing exact approval");
        assert_eq!(replayed_approval.request.id, request.id);
        assert_eq!(replayed_tool.id, tool_record.id);
        let pending_events = serde_json::to_string(&store.list_recent(100).unwrap()).unwrap();
        assert!(!pending_events.contains("private-message-marker"));
        assert!(!pending_events.contains("private-attachment-marker"));
        let permit = store
            .approve_and_reserve_connector_attachment_download(
                request.id,
                0,
                scope.preview_revision,
                &scope.preview_hash,
                "Download this exact attachment".to_string(),
                now,
            )
            .expect("approval and reservation commit atomically");
        assert_eq!(permit.generation(), 0);
        assert!(store
            .approve_and_reserve_connector_attachment_download(
                request.id,
                0,
                scope.preview_revision,
                &scope.preview_hash,
                "Replay".to_string(),
                now,
            )
            .is_err());
        let record = store
            .list_capability_access_records()
            .unwrap()
            .into_iter()
            .find(|record| record.request.id == request.id)
            .unwrap();
        assert_eq!(record.grant_state, CapabilityGrantState::OneShotConsumed);
        let latest_tool = store
            .list_tool_invocations()
            .unwrap()
            .into_iter()
            .find(|record| record.id == tool_record.id)
            .unwrap();
        assert_eq!(latest_tool.status, ToolExecutionStatus::Running);
        let candidates = store
            .claim_startup_connector_attachment_cleanup_candidates(now, 32)
            .unwrap();
        let candidate = candidates
            .into_iter()
            .find(|candidate| candidate.landing_id == permit.reservation_id())
            .expect("startup claims the incomplete durable reservation");
        let terminal_tool = store
            .list_tool_invocations()
            .unwrap()
            .into_iter()
            .find(|record| record.id == tool_record.id)
            .unwrap();
        assert_eq!(terminal_tool.status, ToolExecutionStatus::Failed);
        crate::kernel::connectors::landing::cleanup_incomplete_connector_attachment(&candidate)
            .expect("missing landing files need no manual repair");
        store
            .fail_connector_attachment_after_cleanup(
                permit.reservation_id(),
                candidate.claim_id,
                now,
            )
            .unwrap();
        assert_eq!(
            store
                .connector_attachment_status(permit.reservation_id())
                .unwrap(),
            "failed"
        );
        let failed_tool = store
            .list_tool_invocations()
            .unwrap()
            .into_iter()
            .find(|record| record.id == tool_record.id)
            .unwrap();
        assert_eq!(failed_tool.status, ToolExecutionStatus::Failed);
        let events = serde_json::to_string(&store.list_recent(100).unwrap()).unwrap();
        assert!(!events.contains("private-message-marker"));
        assert!(!events.contains("private-attachment-marker"));
        let stored_metadata: String = store
            .conn
            .query_row(
                "SELECT metadata_json FROM connector_attachment_landings WHERE id = ?1",
                rusqlite::params![permit.reservation_id().to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!stored_metadata.contains("private-message-marker"));
        assert!(!stored_metadata.contains("private-attachment-marker"));
        assert!(stored_metadata.contains("redacted:parent"));
    }

    #[test]
    fn execution_projections_scale_beyond_legacy_event_scan_limits() {
        let store = EventStore::open_memory().expect("memory store opens");
        for _ in 0..256 {
            let request = request_capability_access(
                AccessMode::AskOnRisk,
                CapabilityKind::ComputerScreenshot,
            )
            .expect("request builds");
            store
                .append_capability_access_request(&request)
                .expect("request projects");
        }
        assert_eq!(
            store
                .list_capability_access_records()
                .expect("all projected requests load")
                .len(),
            256
        );

        let plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: APP_UPDATE_CHECK_TOOL_ID.to_string(),
            input: json!({}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .expect("tool plan prepares");
        let mut oldest_id = None;
        for index in 0..520 {
            let mut invocation = ToolInvocationRecord::running(&plan, None);
            invocation.id = Uuid::new_v4();
            if index == 0 {
                oldest_id = Some(invocation.id);
            }
            store
                .append_tool_invocation(&invocation)
                .expect("tool invocation projects");
        }
        assert_eq!(
            store
                .tool_invocation_by_id(oldest_id.expect("oldest id captured"))
                .expect("oldest invocation remains addressable")
                .tool_id,
            APP_UPDATE_CHECK_TOOL_ID
        );
        assert_eq!(
            store
                .list_tool_invocations()
                .expect("bounded recent tool list loads")
                .len(),
            500
        );
    }

    #[test]
    fn execution_projection_replay_is_idempotent_across_restarts() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("execution-projection.sqlite3");
        let request_id;
        {
            let store = EventStore::open(&path).expect("store opens");
            let request = request_capability_access(
                AccessMode::AskOnRisk,
                CapabilityKind::ComputerScreenshot,
            )
            .expect("request builds");
            request_id = request.id;
            store
                .append_capability_access_request(&request)
                .expect("request appends");
            store
                .resolve_capability_access_request(
                    request.id,
                    true,
                    "Approve this screenshot".to_string(),
                )
                .expect("request resolves");
            assert_eq!(
                store
                    .capability_access_record_by_id(request.id)
                    .expect("projection loads")
                    .projection_revision,
                1
            );
        }
        for _ in 0..2 {
            let store = EventStore::open(&path).expect("store reopens");
            let record = store
                .capability_access_record_by_id(request_id)
                .expect("projection survives restart");
            assert_eq!(record.projection_revision, 1);
            assert_eq!(record.effective_status, CapabilityAccessStatus::Approved);
            let applied: i64 = store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM execution_projection_applied_events",
                    [],
                    |row| row.get(0),
                )
                .expect("applied event count loads");
            assert_eq!(applied, 2);
        }
    }

    #[test]
    fn legacy_execution_projection_schema_migrates_once_and_reopens_idempotently() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("legacy-execution-projection.sqlite3");
        let request =
            request_capability_access(AccessMode::AskOnRisk, CapabilityKind::ComputerScreenshot)
                .expect("request builds");
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true);
        {
            let connection = Connection::open(&path).expect("legacy database opens");
            connection
                .execute_batch(
                    r#"
                    CREATE TABLE capability_access_state (
                      request_id TEXT PRIMARY KEY NOT NULL,
                      request_json TEXT NOT NULL,
                      resolution_json TEXT,
                      effective_status TEXT NOT NULL,
                      updated_at TEXT NOT NULL
                    );
                    CREATE TABLE tool_invocation_state (
                      id TEXT PRIMARY KEY NOT NULL,
                      invocation_json TEXT NOT NULL,
                      tool_id TEXT NOT NULL,
                      capability TEXT NOT NULL,
                      status TEXT NOT NULL,
                      approval_request_id TEXT,
                      request_fingerprint TEXT NOT NULL,
                      updated_at TEXT NOT NULL
                    );
                    CREATE TABLE connector_authorization_sessions (
                      id TEXT PRIMARY KEY NOT NULL,
                      session_json TEXT NOT NULL,
                      expires_at TEXT NOT NULL,
                      consumed_at TEXT,
                      updated_at TEXT NOT NULL
                    );
                    CREATE TABLE review_queue_items (
                      id TEXT PRIMARY KEY NOT NULL,
                      automation_run_id TEXT NOT NULL UNIQUE,
                      item_json TEXT NOT NULL,
                      status TEXT NOT NULL,
                      updated_at TEXT NOT NULL
                    );
                    CREATE TABLE connector_attachment_landings (
                      id TEXT PRIMARY KEY NOT NULL,
                      account_id TEXT NOT NULL,
                      account_generation INTEGER NOT NULL,
                      metadata_json TEXT NOT NULL,
                      tool_invocation_id TEXT NOT NULL UNIQUE,
                      approval_request_id TEXT NOT NULL UNIQUE,
                      request_fingerprint TEXT NOT NULL,
                      landing_fingerprint TEXT NOT NULL,
                      status TEXT NOT NULL,
                      receipt_json TEXT,
                      updated_at TEXT NOT NULL
                    );
                    CREATE TABLE connector_invocations (
                      id TEXT PRIMARY KEY NOT NULL,
                      account_id TEXT NOT NULL,
                      idempotency_key TEXT NOT NULL,
                      invocation_json TEXT NOT NULL,
                      status TEXT NOT NULL,
                      updated_at TEXT NOT NULL
                    );
                    "#,
                )
                .expect("legacy schema builds");
            connection
                .execute(
                    r#"INSERT INTO capability_access_state
                       (request_id, request_json, resolution_json, effective_status, updated_at)
                       VALUES (?1, ?2, NULL, ?3, ?4)"#,
                    params![
                        request.id.to_string(),
                        serde_json::to_string(&request).expect("request serializes"),
                        serde_json::to_string(&CapabilityAccessStatus::PendingApproval)
                            .expect("status serializes"),
                        now,
                    ],
                )
                .expect("legacy request inserts");
        }

        for _ in 0..2 {
            let store = EventStore::open(&path).expect("legacy store migrates and reopens");
            let record = store
                .capability_access_record_by_id(request.id)
                .expect("legacy request remains projected");
            assert_eq!(record.projection_revision, 0);
            assert_eq!(
                record.effective_status,
                CapabilityAccessStatus::PendingApproval
            );
            for (table, column) in [
                ("capability_access_state", "row_revision"),
                ("capability_access_state", "created_at"),
                ("tool_invocation_state", "row_revision"),
                ("tool_invocation_state", "created_at"),
                ("connector_attachment_landings", "cleanup_claim_id"),
                ("connector_attachment_landings", "cleanup_claim_expires_at"),
                ("connector_invocations", "account_generation"),
                ("connector_invocations", "reconciliation_claim_id"),
                ("connector_invocations", "reconciliation_claim_expires_at"),
                ("connector_invocations", "next_reconciliation_at"),
                ("connector_invocations", "reconciliation_attempt_count"),
                ("connector_invocations", "reconciliation_quarantine_code"),
                ("connector_invocations", "reconciliation_quarantined_at"),
            ] {
                let present = store
                    .conn
                    .prepare(&format!("PRAGMA table_info(\"{table}\")"))
                    .expect("table info prepares")
                    .query_map([], |row| row.get::<_, String>(1))
                    .expect("columns query")
                    .collect::<Result<Vec<_>, _>>()
                    .expect("columns load")
                    .iter()
                    .any(|candidate| candidate == column);
                assert!(present, "missing migrated column {table}.{column}");
            }
        }
    }

    #[test]
    fn recovery_retry_is_fingerprint_bound_audited_and_secret_free() {
        let store = EventStore::open_memory().expect("memory store opens");
        let landing_id = Uuid::new_v4();
        let now = Utc::now();
        let metadata = ConnectorAttachmentMetadata {
            account_id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            parent_remote_ref: "private-parent-marker".to_string(),
            attachment_remote_ref: "private-attachment-marker".to_string(),
            file_name: "quarterly-report.pdf".to_string(),
            declared_media_type: "application/pdf".to_string(),
            size_bytes: 128,
            contains_macros: false,
            untrusted_evidence: true,
        };
        store
            .conn
            .execute(
                r#"INSERT INTO connector_attachment_landings
                   (id, account_id, account_generation, metadata_json, size_bytes,
                    tool_invocation_id, approval_request_id, request_fingerprint,
                    landing_fingerprint, workspace_root, workspace_identity,
                    storage_identity, status, failure_kind, attempt_count,
                    created_at, updated_at)
                   VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6, 'request-fingerprint',
                           'landing-fingerprint', ?7, 'workspace-file-id',
                           'attachment-file-id', 'repair_required',
                           'unsafe_cleanup_boundary', 1, ?8, ?8)"#,
                params![
                    landing_id.to_string(),
                    metadata.account_id.to_string(),
                    serde_json::to_string(&metadata).expect("metadata serializes"),
                    metadata.size_bytes as i64,
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    r"D:\private-workspace",
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("repair row inserts");

        let item = store
            .list_connector_recovery_items()
            .expect("recovery items load")
            .into_iter()
            .find(|item| item.id == landing_id)
            .expect("attachment recovery item exists");
        let fingerprint = match &item.action {
            Some(crate::kernel::connectors::ConnectorRecoveryAction::RetryAttachmentCleanup {
                action_revision,
            }) => action_revision.clone(),
            Some(crate::kernel::connectors::ConnectorRecoveryAction::ResumeSync { .. }) => {
                panic!("attachment recovery cannot resume sync")
            }
            Some(crate::kernel::connectors::ConnectorRecoveryAction::InspectExternalResult {
                ..
            }) => panic!("attachment recovery cannot inspect an external result"),
            None => panic!("retryable attachment action exists"),
        };
        let serialized = serde_json::to_string(&item).expect("item serializes");
        for secret in [
            "private-parent-marker",
            "private-attachment-marker",
            r"D:\private-workspace",
            "workspace-file-id",
            "attachment-file-id",
        ] {
            assert!(!serialized.contains(secret));
        }
        assert!(store
            .retry_connector_attachment_recovery(landing_id, "stale-fingerprint", Utc::now())
            .is_err());
        assert_eq!(
            store
                .connector_attachment_status(landing_id)
                .expect("status remains repair"),
            "repair_required"
        );

        assert_eq!(
            store
                .retry_connector_attachment_recovery(landing_id, &fingerprint, Utc::now())
                .expect("exact recovery queues"),
            crate::kernel::connectors::ConnectorRecoveryAcceptance::Accepted
        );
        for _ in 0..100 {
            assert_eq!(
                store
                    .retry_connector_attachment_recovery(landing_id, &fingerprint, Utc::now())
                    .expect("accepted cleanup replay is idempotent"),
                crate::kernel::connectors::ConnectorRecoveryAcceptance::AlreadyAccepted
            );
        }
        assert_eq!(
            store
                .connector_attachment_status(landing_id)
                .expect("status loads"),
            "cleanup_required"
        );
        let events = store
            .conn
            .prepare("SELECT payload_json FROM kernel_events WHERE event_type = ?1")
            .expect("retry event query prepares")
            .query_map(params![CONNECTOR_RECOVERY_RETRY_QUEUED_EVENT], |row| {
                row.get::<_, String>(0)
            })
            .expect("retry events query")
            .collect::<Result<Vec<_>, _>>()
            .expect("retry events load");
        assert_eq!(events.len(), 1);
        for secret in [
            "private-parent-marker",
            "private-attachment-marker",
            r"D:\private-workspace",
            "workspace-file-id",
            "attachment-file-id",
        ] {
            assert!(!events[0].contains(secret));
        }
    }

    #[test]
    fn concurrent_recovery_acceptance_is_exactly_once_and_durable() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("recovery-acceptance.sqlite3");
        let landing_id = Uuid::new_v4();
        let now = Utc::now();
        let metadata = ConnectorAttachmentMetadata {
            account_id: Uuid::new_v4(),
            provider_id: "fake".to_string(),
            parent_remote_ref: "parent".to_string(),
            attachment_remote_ref: "attachment".to_string(),
            file_name: "report.pdf".to_string(),
            declared_media_type: "application/pdf".to_string(),
            size_bytes: 1,
            contains_macros: false,
            untrusted_evidence: true,
        };
        let store = EventStore::open(&path).expect("store opens");
        store
            .conn
            .execute(
                r#"INSERT INTO connector_attachment_landings
                   (id, account_id, account_generation, metadata_json, size_bytes,
                    tool_invocation_id, approval_request_id, request_fingerprint,
                    landing_fingerprint, workspace_root, workspace_identity,
                    storage_identity, status, failure_kind, attempt_count,
                    created_at, updated_at)
                   VALUES (?1, ?2, 0, ?3, 1, ?4, ?5, 'request', 'landing',
                           'private-root', 'workspace-id', 'storage-id',
                           'repair_required', 'unsafe_cleanup_boundary', 1, ?6, ?6)"#,
                params![
                    landing_id.to_string(),
                    metadata.account_id.to_string(),
                    serde_json::to_string(&metadata).expect("metadata serializes"),
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("repair row inserts");
        let action_revision = match store
            .list_connector_recovery_items()
            .expect("recovery item loads")
            .into_iter()
            .find(|item| item.id == landing_id)
            .and_then(|item| item.action)
        {
            Some(crate::kernel::connectors::ConnectorRecoveryAction::RetryAttachmentCleanup {
                action_revision,
            }) => action_revision,
            _ => panic!("attachment action exists"),
        };
        drop(store);

        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let path = path.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            let action_revision = action_revision.clone();
            workers.push(std::thread::spawn(move || {
                let store = EventStore::open(path).expect("concurrent store opens");
                barrier.wait();
                store
                    .retry_connector_attachment_recovery(
                        landing_id,
                        &action_revision,
                        now + Duration::seconds(1),
                    )
                    .expect("concurrent request resolves")
            }));
        }
        let mut outcomes = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker joins"))
            .collect::<Vec<_>>();
        outcomes.sort_by_key(|outcome| match outcome {
            crate::kernel::connectors::ConnectorRecoveryAcceptance::Accepted => 0,
            crate::kernel::connectors::ConnectorRecoveryAcceptance::AlreadyAccepted => 1,
        });
        assert_eq!(
            outcomes,
            vec![
                crate::kernel::connectors::ConnectorRecoveryAcceptance::Accepted,
                crate::kernel::connectors::ConnectorRecoveryAcceptance::AlreadyAccepted,
            ]
        );

        let store = EventStore::open(&path).expect("store restarts");
        assert_eq!(
            store
                .retry_connector_attachment_recovery(
                    landing_id,
                    &action_revision,
                    now + Duration::seconds(2),
                )
                .expect("receipt survives restart"),
            crate::kernel::connectors::ConnectorRecoveryAcceptance::AlreadyAccepted
        );
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM connector_recovery_action_receipts",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("receipt count loads"),
            1
        );

        let next_episode_at = now + Duration::seconds(1);
        store
            .conn
            .execute(
                r#"UPDATE connector_attachment_landings
                   SET status = 'repair_required', failure_kind = 'unsafe_cleanup_boundary',
                       recovery_revision = recovery_revision + 1, updated_at = ?2
                   WHERE id = ?1"#,
                params![
                    landing_id.to_string(),
                    next_episode_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("next recovery episode is projected");
        let next_revision = match store
            .list_connector_recovery_items()
            .expect("next recovery item loads")
            .into_iter()
            .find(|item| item.id == landing_id)
            .and_then(|item| item.action)
        {
            Some(crate::kernel::connectors::ConnectorRecoveryAction::RetryAttachmentCleanup {
                action_revision,
            }) => action_revision,
            _ => panic!("next attachment episode has an action"),
        };
        assert_ne!(next_revision, action_revision);
        assert_eq!(
            store
                .retry_connector_attachment_recovery(landing_id, &action_revision, next_episode_at,)
                .expect("old episode remains an accepted receipt"),
            crate::kernel::connectors::ConnectorRecoveryAcceptance::AlreadyAccepted
        );
        assert_eq!(
            store
                .connector_attachment_status(landing_id)
                .expect("new episode remains untouched"),
            "repair_required"
        );
        assert_eq!(
            store
                .retry_connector_attachment_recovery(
                    landing_id,
                    &next_revision,
                    next_episode_at + Duration::seconds(1),
                )
                .expect("new episode is independently accepted"),
            crate::kernel::connectors::ConnectorRecoveryAcceptance::Accepted
        );
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM kernel_events WHERE event_type = ?1",
                    params![CONNECTOR_RECOVERY_RETRY_QUEUED_EVENT],
                    |row| row.get::<_, i64>(0),
                )
                .expect("event count loads"),
            2
        );
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM connector_recovery_action_receipts",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("episode receipt count loads"),
            2
        );
    }

    #[test]
    fn expired_recovery_receipt_cleanup_is_bounded_to_sixty_four_rows() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("recovery-receipt-retention.sqlite3");
        let store = EventStore::open(&path).expect("store opens");
        for index in 0..70 {
            store
                .conn
                .execute(
                    r#"INSERT INTO connector_recovery_action_receipts
                       (action_kind, item_id, action_revision, accepted_at, retain_until)
                       VALUES ('attachment_cleanup', ?1, ?2, ?3, ?3)"#,
                    params![
                        Uuid::new_v4().to_string(),
                        format!("{index:064x}"),
                        (Utc::now() - Duration::days(100))
                            .to_rfc3339_opts(SecondsFormat::Nanos, true),
                    ],
                )
                .expect("expired receipt inserts");
        }
        drop(store);

        let store = EventStore::open(&path).expect("migration performs bounded cleanup");
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM connector_recovery_action_receipts",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("remaining receipt count loads"),
            6
        );
    }

    #[test]
    fn recovery_cards_project_structured_effects_without_connector_secrets() {
        use crate::kernel::connectors::domain::{MailAddress, MailMessage};
        use crate::kernel::connectors::provider::ConnectorProviderFailure;
        use crate::kernel::connectors::sync::{
            ConnectorOpaqueContinuation, ConnectorSyncContinuation, ConnectorSyncFailure,
            ConnectorSyncPage, ConnectorSyncPlan, ConnectorSyncState, ConnectorSyncStateRecovery,
        };
        use crate::kernel::connectors::{
            ConnectorAccount, ConnectorCapability, ConnectorCredentialHandle, ConnectorEvidenceRef,
            ConnectorHealth, ConnectorInvocation, ConnectorInvocationStatus,
            ConnectorRecoveryAction, ConnectorRecoveryExternalEffectState, ConnectorRecoveryKind,
            ConnectorRecoveryNextStepCode, ConnectorRecoveryReasonCode, ConnectorRecoveryStatus,
            ConnectorRecoverySyncCapability,
        };

        let store = EventStore::open_memory().expect("memory store opens");
        let now = Utc::now();
        let account = |display_name: &str, health: ConnectorHealth| ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "secret-provider-marker".to_string(),
            display_name: display_name.to_string(),
            tenant_ref: Some("secret-tenant-marker".to_string()),
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailSyncInbox],
            health,
            connected_at: now,
            updated_at: now,
        };
        let needs_repair = account("Repair account", ConnectorHealth::NeedsRepair);
        let disconnect_pending = account("Disconnect account", ConnectorHealth::DisconnectPending);
        let revocation_pending = account("Revocation account", ConnectorHealth::Connected);
        let sync_account = account("Sync account", ConnectorHealth::Connected);
        let credential_markers = [
            &needs_repair,
            &disconnect_pending,
            &revocation_pending,
            &sync_account,
        ]
        .into_iter()
        .map(|account| {
            serde_json::to_value(&account.credential_handle)
                .expect("credential handle serializes")
                .as_str()
                .expect("credential handle is text")
                .to_string()
        })
        .collect::<Vec<_>>();
        for account in [
            &needs_repair,
            &disconnect_pending,
            &revocation_pending,
            &sync_account,
        ] {
            store
                .upsert_connector_account(account)
                .expect("account persists");
        }
        store
            .begin_connector_revocation(revocation_pending.id, now)
            .expect("revocation begins");

        let stream_fingerprint = "secret-stream-fingerprint-marker";
        let initial = ConnectorSyncState::initial(
            sync_account.id,
            ConnectorCapability::MailSyncInbox,
            stream_fingerprint.to_string(),
            now,
        )
        .expect("sync state initializes");
        let page = ConnectorSyncPage::<serde_json::Value>::new(
            Vec::new(),
            ConnectorSyncContinuation::Delta(
                ConnectorOpaqueContinuation::new("secret-continuation-marker".to_string())
                    .expect("continuation builds"),
            ),
        );
        let advanced = initial
            .advance(&page, now + Duration::seconds(1))
            .expect("sync state advances");
        let retry = match advanced
            .recovery(
                ConnectorSyncFailure::NetworkUnavailable,
                0,
                3,
                now + Duration::seconds(2),
            )
            .expect("retry state builds")
        {
            ConnectorSyncStateRecovery::Persist { next, .. } => next,
            ConnectorSyncStateRecovery::RepairAccount => panic!("network failure should retry"),
        };
        let stopped = match retry
            .recovery(
                ConnectorSyncFailure::NetworkUnavailable,
                3,
                3,
                now + Duration::seconds(3),
            )
            .expect("stopped state builds")
        {
            ConnectorSyncStateRecovery::Persist { next, .. } => next,
            ConnectorSyncStateRecovery::RepairAccount => panic!("exhausted retry should stop"),
        };
        assert!(stopped.stopped());
        store
            .conn
            .execute(
                r#"INSERT INTO connector_sync_streams
                   (account_id, capability, stream_fingerprint, state_json, revision,
                    request_json, updated_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
                params![
                    sync_account.id.to_string(),
                    ConnectorCapability::MailSyncInbox.contract_name(),
                    stream_fingerprint,
                    stopped.persistence_json().expect("sync state serializes"),
                    stopped.revision() as i64,
                    ConnectorSyncPlan::MailInbox { max_changes: 25 }
                        .persistence_json()
                        .expect("sync plan serializes"),
                    stopped
                        .updated_at()
                        .to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("stopped sync state persists");
        store
            .conn
            .execute(
                r#"INSERT INTO connector_sync_streams
                   (account_id, capability, stream_fingerprint, state_json, revision, updated_at)
                   VALUES (?1, ?2, 'malformed-stream', '{not-json', 0, ?3)"#,
                params![
                    Uuid::new_v4().to_string(),
                    ConnectorCapability::MailSyncInbox.contract_name(),
                    (now + Duration::seconds(4)).to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("malformed sync state persists ahead of healthy row");

        let invocation = ConnectorInvocation {
            id: Uuid::new_v4(),
            provider_id: "secret-provider-marker".to_string(),
            account_id: sync_account.id,
            account_generation: None,
            capability: ConnectorCapability::CalendarCreateEvent,
            automation_run_id: Some(Uuid::new_v4()),
            tool_invocation_id: Some(Uuid::new_v4()),
            request_fingerprint: "secret-request-fingerprint-marker".to_string(),
            idempotency_key: "secret-idempotency-marker".to_string(),
            mutation: None,
            status: ConnectorInvocationStatus::ReconciliationRequired,
            evidence: vec![ConnectorEvidenceRef {
                provider_id: "secret-provider-marker".to_string(),
                account_id: sync_account.id,
                remote_object_ref: "secret-remote-ref-marker".to_string(),
                retrieved_at: now,
                bounded_summary: Some("secret-provider-body-marker".to_string()),
            }],
            created_at: now,
            updated_at: now,
        };
        store
            .append_connector_invocation(&invocation)
            .expect("reconciliation invocation persists");

        let items = store
            .list_connector_recovery_items()
            .expect("recovery items load");
        let sync_item = items
            .iter()
            .find(|item| item.kind == ConnectorRecoveryKind::Sync)
            .expect("stopped sync is projected");
        assert_eq!(sync_item.status, ConnectorRecoveryStatus::SyncExhausted);
        assert_eq!(
            sync_item.reason_code,
            ConnectorRecoveryReasonCode::SyncRetryExhausted
        );
        assert_eq!(
            sync_item.external_effect_state,
            ConnectorRecoveryExternalEffectState::NoExternalWrite
        );
        assert_eq!(
            sync_item.next_step_code,
            ConnectorRecoveryNextStepCode::ReviewAccountConnection
        );
        assert_eq!(
            sync_item.sync_capability,
            Some(ConnectorRecoverySyncCapability::Mail)
        );
        let changes_before_reads = store.conn.total_changes();
        let event_count_before_reads: i64 = store
            .conn
            .query_row("SELECT count(*) FROM kernel_events", [], |row| row.get(0))
            .unwrap();
        let legacy_action_count_before_reads: i64 = store
            .conn
            .query_row(
                "SELECT (SELECT count(*) FROM connector_sync_recovery_actions) + (SELECT count(*) FROM connector_reconciliation_recovery_actions)",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let mut reloaded_sync_item = None;
        for _ in 0..100 {
            reloaded_sync_item = store
                .list_connector_recovery_items()
                .expect("recovery items reload")
                .into_iter()
                .find(|item| item.kind == ConnectorRecoveryKind::Sync);
        }
        assert_eq!(store.conn.total_changes(), changes_before_reads);
        assert_eq!(
            store
                .conn
                .query_row("SELECT count(*) FROM kernel_events", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            event_count_before_reads
        );
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT (SELECT count(*) FROM connector_sync_recovery_actions) + (SELECT count(*) FROM connector_reconciliation_recovery_actions)",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            legacy_action_count_before_reads
        );
        let reloaded_sync_item =
            reloaded_sync_item.expect("stopped sync remains projected after repeated reads");
        assert_eq!(sync_item.id, reloaded_sync_item.id);
        let sync_token = match reloaded_sync_item.action {
            Some(ConnectorRecoveryAction::ResumeSync { action_revision }) => action_revision,
            _ => panic!("stopped sync has an exact Recovery action"),
        };
        let production_syncs =
            crate::kernel::connectors::runtime_registry::ConnectorRuntimeRegistries::empty()
                .syncs();
        let production_items = store
            .list_connector_recovery_items_with_runtime_registries(
                &crate::kernel::connectors::reconciliation::EmptyConnectorReconcilerRegistry,
                production_syncs.as_ref(),
            )
            .expect("production recovery items load without execution authority");
        let production_sync_item = production_items
            .iter()
            .find(|item| item.id == sync_item.id)
            .expect("unavailable sync remains visible");
        assert!(production_sync_item.action.is_none());
        let changes_before_unavailable_resume = store.conn.total_changes();
        assert!(store
            .resume_connector_read_sync_from_recovery_with_sync_registry(
                sync_item.id,
                &sync_token,
                production_syncs.as_ref(),
                now + Duration::seconds(4),
            )
            .is_err());
        assert_eq!(
            store.conn.total_changes(),
            changes_before_unavailable_resume
        );

        let account_item = |reason_code| {
            items
                .iter()
                .find(|item| item.reason_code == reason_code)
                .expect("account recovery item exists")
        };
        assert_eq!(
            account_item(ConnectorRecoveryReasonCode::AccountNeedsRepair).next_step_code,
            ConnectorRecoveryNextStepCode::ReviewAccountConnection
        );
        assert_eq!(
            account_item(ConnectorRecoveryReasonCode::AccountDisconnectPending)
                .external_effect_state,
            ConnectorRecoveryExternalEffectState::LocalCredentialRemovalPending
        );
        assert_eq!(
            account_item(ConnectorRecoveryReasonCode::AccountRevocationPending)
                .external_effect_state,
            ConnectorRecoveryExternalEffectState::NoExternalWrite
        );
        let reconciliation = items
            .iter()
            .find(|item| item.kind == ConnectorRecoveryKind::Reconciliation)
            .expect("reconciliation is projected");
        assert_eq!(
            reconciliation.external_effect_state,
            ConnectorRecoveryExternalEffectState::ExternalResultUncertain
        );
        assert_eq!(
            reconciliation.next_step_code,
            ConnectorRecoveryNextStepCode::VerifyProviderState
        );
        assert!(reconciliation.action.is_none());

        let serialized = serde_json::to_string(&items).expect("recovery items serialize");
        for secret in [
            "secret-provider-marker",
            "secret-tenant-marker",
            "secret-stream-fingerprint-marker",
            "secret-continuation-marker",
            "secret-request-fingerprint-marker",
            "secret-idempotency-marker",
            "secret-remote-ref-marker",
            "secret-provider-body-marker",
        ] {
            assert!(!serialized.contains(secret));
        }
        for credential_marker in credential_markers {
            assert!(!serialized.contains(&credential_marker));
        }
        assert!(!serialized.contains(&sync_account.id.to_string()));
        assert!(!serialized.contains("retry_at"));

        assert!(store
            .resume_connector_read_sync_from_recovery(
                sync_item.id,
                &format!("{sync_token}x"),
                now + Duration::seconds(4),
            )
            .is_err());
        for authority_tamper in ["provider", "credential", "capability"] {
            let mut tampered = sync_account.clone();
            match authority_tamper {
                "provider" => tampered.provider_id = "tampered-provider".to_string(),
                "credential" => tampered.credential_handle = ConnectorCredentialHandle::new(),
                "capability" => tampered.granted_capabilities.clear(),
                _ => unreachable!(),
            }
            store
                .conn
                .execute(
                    "UPDATE connector_accounts SET account_json = ?2 WHERE id = ?1",
                    params![
                        sync_account.id.to_string(),
                        serde_json::to_string(&tampered).unwrap()
                    ],
                )
                .unwrap();
            assert!(store
                .resume_connector_read_sync_from_recovery(
                    sync_item.id,
                    &sync_token,
                    now + Duration::seconds(4),
                )
                .is_err());
            store
                .conn
                .execute(
                    "UPDATE connector_accounts SET account_json = ?2 WHERE id = ?1",
                    params![
                        sync_account.id.to_string(),
                        serde_json::to_string(&sync_account).unwrap()
                    ],
                )
                .unwrap();
        }
        store
            .conn
            .execute(
                "UPDATE connector_account_generations SET generation = generation + 1 WHERE account_id = ?1",
                params![sync_account.id.to_string()],
            )
            .unwrap();
        assert!(store
            .resume_connector_read_sync_from_recovery(
                sync_item.id,
                &sync_token,
                now + Duration::seconds(4),
            )
            .is_err());
        store
            .conn
            .execute(
                "UPDATE connector_account_generations SET generation = generation - 1 WHERE account_id = ?1",
                params![sync_account.id.to_string()],
            )
            .unwrap();

        assert_eq!(
            store
                .resume_connector_read_sync_from_recovery(
                    sync_item.id,
                    &sync_token,
                    now + Duration::seconds(4),
                )
                .expect("stopped read sync is rescheduled"),
            crate::kernel::connectors::ConnectorRecoveryAcceptance::Accepted
        );
        let resumed = store
            .connector_sync_state(
                sync_account.id,
                ConnectorCapability::MailSyncInbox,
                stream_fingerprint,
            )
            .unwrap()
            .expect("resumed state loads");
        assert!(!resumed.stopped());
        assert!(resumed.retry_state().is_none());
        assert_eq!(resumed.revision(), stopped.revision() + 1);
        for _ in 0..100 {
            assert_eq!(
                store
                    .resume_connector_read_sync_from_recovery(
                        sync_item.id,
                        &sync_token,
                        now + Duration::seconds(5),
                    )
                    .expect("accepted sync replay is idempotent"),
                crate::kernel::connectors::ConnectorRecoveryAcceptance::AlreadyAccepted
            );
        }
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM connector_sync_recovery_jobs",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        for malformed_index in 1_u128..=64 {
            store
                .conn
                .execute(
                    r#"INSERT INTO connector_sync_recovery_jobs
                   (id, recovery_item_id, action_revision, account_id, account_generation,
                    capability, stream_fingerprint, expected_state_revision, status,
                    next_attempt_at, attempt_count, created_at, updated_at)
                   VALUES (?1, ?2, ?3,
                           'malformed', 0, 'mail_sync_inbox', 'malformed', 0, 'queued',
                           ?4, 0, ?4, ?4)"#,
                    params![
                        Uuid::from_u128(malformed_index).to_string(),
                        format!("malformed-item-{malformed_index}"),
                        format!("malformed-action-{malformed_index}"),
                        now.to_rfc3339_opts(SecondsFormat::Nanos, true)
                    ],
                )
                .expect("malformed job is inserted ahead of healthy job");
        }
        let cross_capability_plan = ConnectorSyncPlan::CalendarRange {
            starts_at: now,
            ends_at: now + Duration::days(1),
            max_changes: 25,
        }
        .persistence_json()
        .unwrap();
        store
            .conn
            .execute(
                r#"UPDATE connector_sync_streams SET request_json = ?2
                   WHERE account_id = ?1"#,
                params![sync_account.id.to_string(), cross_capability_plan],
            )
            .expect("valid cross-capability plan tampers the due row");
        assert!(store
            .claim_due_connector_sync_recovery_jobs(now + Duration::seconds(6), 1)
            .expect("cross-capability row is isolated after the malformed first page")
            .is_empty());
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT status FROM connector_sync_recovery_jobs WHERE account_id = ?1",
                    params![sync_account.id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "repair_required"
        );
        store
            .conn
            .execute(
                r#"UPDATE connector_sync_streams SET request_json = ?2
                   WHERE account_id = ?1"#,
                params![
                    sync_account.id.to_string(),
                    ConnectorSyncPlan::MailInbox { max_changes: 25 }
                        .persistence_json()
                        .unwrap(),
                ],
            )
            .expect("healthy Mail plan is restored");
        store
            .conn
            .execute(
                r#"UPDATE connector_sync_recovery_jobs
                   SET status = 'queued', quarantine_code = NULL
                   WHERE account_id = ?1"#,
                params![sync_account.id.to_string()],
            )
            .expect("isolated test job is restored for the remaining claim tests");
        let mut claims = store
            .claim_due_connector_sync_recovery_jobs(now + Duration::seconds(6), 1)
            .expect("healthy job is claimable after malformed job");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].account().id, sync_account.id);
        assert_eq!(claims[0].state().revision(), resumed.revision());
        assert!(matches!(
            claims[0].plan(),
            ConnectorSyncPlan::MailInbox { max_changes: 25 }
        ));
        assert_eq!(claims[0].attempt_count(), 1);
        let first_claim_id = claims[0].claim_id();
        let first_expiry = claims[0].claim_expires_at();
        store
            .renew_connector_sync_recovery_claim(&mut claims[0], now + Duration::seconds(7))
            .expect("live claim renews");
        assert!(claims[0].claim_expires_at() > first_expiry);
        for tamper in [
            "provider",
            "tenant",
            "credential",
            "capability",
            "health",
            "generation",
            "state",
            "plan",
        ] {
            match tamper {
                "generation" => {
                    store
                        .conn
                        .execute(
                            "UPDATE connector_account_generations SET generation = generation + 1 WHERE account_id = ?1",
                            params![sync_account.id.to_string()],
                        )
                        .unwrap();
                }
                "state" => {
                    let (tampered_state, _) = match resumed
                        .recovery(
                            ConnectorSyncFailure::NetworkUnavailable,
                            0,
                            3,
                            now + Duration::seconds(8),
                        )
                        .unwrap()
                    {
                        ConnectorSyncStateRecovery::Persist { next, reason } => (next, reason),
                        ConnectorSyncStateRecovery::RepairAccount => unreachable!(),
                    };
                    store
                        .conn
                        .execute(
                            r#"UPDATE connector_sync_streams SET state_json = ?4
                               WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3"#,
                            params![
                                sync_account.id.to_string(),
                                ConnectorCapability::MailSyncInbox.contract_name(),
                                stream_fingerprint,
                                tampered_state.persistence_json().unwrap(),
                            ],
                        )
                        .unwrap();
                }
                "plan" => {
                    let tampered_plan = ConnectorSyncPlan::CalendarRange {
                        starts_at: now,
                        ends_at: now + Duration::days(1),
                        max_changes: 25,
                    }
                    .persistence_json()
                    .unwrap();
                    store
                        .conn
                        .execute(
                            r#"UPDATE connector_sync_streams SET request_json = ?4
                               WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3"#,
                            params![
                                sync_account.id.to_string(),
                                ConnectorCapability::MailSyncInbox.contract_name(),
                                stream_fingerprint,
                                tampered_plan,
                            ],
                        )
                        .unwrap();
                }
                _ => {
                    let mut tampered = sync_account.clone();
                    match tamper {
                        "provider" => tampered.provider_id = "tampered-provider".to_string(),
                        "tenant" => tampered.tenant_ref = Some("tampered-tenant".to_string()),
                        "credential" => {
                            tampered.credential_handle = ConnectorCredentialHandle::new()
                        }
                        "capability" => tampered.granted_capabilities.clear(),
                        "health" => tampered.health = ConnectorHealth::NeedsRepair,
                        _ => unreachable!(),
                    }
                    store
                        .conn
                        .execute(
                            "UPDATE connector_accounts SET account_json = ?2 WHERE id = ?1",
                            params![
                                sync_account.id.to_string(),
                                serde_json::to_string(&tampered).unwrap()
                            ],
                        )
                        .unwrap();
                }
            }
            assert!(
                store
                    .renew_connector_sync_recovery_claim(
                        &mut claims[0],
                        now + Duration::seconds(8),
                    )
                    .is_err(),
                "{tamper} tamper must fail the post-I/O authority fence"
            );
            match tamper {
                "generation" => {
                    store
                        .conn
                        .execute(
                            "UPDATE connector_account_generations SET generation = generation - 1 WHERE account_id = ?1",
                            params![sync_account.id.to_string()],
                        )
                        .unwrap();
                }
                "state" => {
                    store
                        .conn
                        .execute(
                            r#"UPDATE connector_sync_streams SET state_json = ?4
                               WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3"#,
                            params![
                                sync_account.id.to_string(),
                                ConnectorCapability::MailSyncInbox.contract_name(),
                                stream_fingerprint,
                                resumed.persistence_json().unwrap(),
                            ],
                        )
                        .unwrap();
                }
                "plan" => {
                    store
                        .conn
                        .execute(
                            r#"UPDATE connector_sync_streams SET request_json = ?4
                               WHERE account_id = ?1 AND capability = ?2 AND stream_fingerprint = ?3"#,
                            params![
                                sync_account.id.to_string(),
                                ConnectorCapability::MailSyncInbox.contract_name(),
                                stream_fingerprint,
                                ConnectorSyncPlan::MailInbox { max_changes: 25 }
                                    .persistence_json()
                                    .unwrap(),
                            ],
                        )
                        .unwrap();
                }
                _ => {
                    store
                        .conn
                        .execute(
                            "UPDATE connector_accounts SET account_json = ?2 WHERE id = ?1",
                            params![
                                sync_account.id.to_string(),
                                serde_json::to_string(&sync_account).unwrap()
                            ],
                        )
                        .unwrap();
                }
            }
        }
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT status FROM connector_sync_recovery_jobs WHERE id = '00000000-0000-0000-0000-000000000001'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "repair_required"
        );
        assert!(store
            .claim_due_connector_sync_recovery_jobs(now + Duration::seconds(306), 1)
            .unwrap()
            .is_empty());
        let takeover_at = now + Duration::seconds(308);
        let takeover = store
            .claim_due_connector_sync_recovery_jobs(takeover_at, 1)
            .expect("expired running job is taken over");
        assert_eq!(takeover.len(), 1);
        assert_ne!(takeover[0].claim_id(), first_claim_id);
        assert!(store
            .renew_connector_sync_recovery_claim(&mut claims[0], takeover_at)
            .is_err());
        let completed_at = takeover_at + Duration::seconds(1);
        let next_page = ConnectorSyncPage::new(
            vec![
                crate::kernel::connectors::sync::ConnectorSyncChange::Upsert(MailMessage {
                    remote_ref: "private-remote-message".to_string(),
                    thread_ref: "private-thread".to_string(),
                    from: MailAddress {
                        display_name: None,
                        address: "sender@example.com".to_string(),
                    },
                    to: Vec::new(),
                    subject: "Untrusted".to_string(),
                    received_at: completed_at,
                    bounded_body_summary: None,
                    attachments: Vec::new(),
                    has_attachments: false,
                    untrusted_evidence: true,
                }),
            ],
            ConnectorSyncContinuation::Next(
                ConnectorOpaqueContinuation::new("private-next-page".to_string()).unwrap(),
            ),
        );
        assert!(store
            .complete_claimed_mail_sync_recovery(&claims[0], &next_page, completed_at)
            .is_err());
        let paged = store
            .complete_claimed_mail_sync_recovery(&takeover[0], &next_page, completed_at)
            .expect("current lease commits page, cursor and queued job atomically");
        assert_eq!(paged.revision(), resumed.revision() + 1);
        assert!(paged.has_committed_delta());
        assert!(paged.has_resume_page());
        let final_claim = store
            .claim_due_connector_sync_recovery_jobs(completed_at, 1)
            .expect("next page is immediately claimable");
        assert_eq!(final_claim.len(), 1);
        assert!(final_claim[0].state() == &paged);
        let delta_page = ConnectorSyncPage::<MailMessage>::new(
            Vec::new(),
            ConnectorSyncContinuation::Delta(
                ConnectorOpaqueContinuation::new("private-final-delta".to_string()).unwrap(),
            ),
        );
        let completed = store
            .complete_claimed_mail_sync_recovery(
                &final_claim[0],
                &delta_page,
                completed_at + Duration::seconds(1),
            )
            .expect("final delta commits cursor and completed job atomically");
        assert_eq!(completed.revision(), paged.revision() + 1);
        assert!(completed.has_committed_delta());
        assert!(!completed.has_resume_page());
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT status FROM connector_sync_recovery_jobs WHERE id = ?1",
                    params![final_claim[0].job_id().to_string()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "completed"
        );
        assert!(store
            .claim_due_connector_sync_recovery_jobs(completed_at + Duration::seconds(1), 1)
            .unwrap()
            .is_empty());
        let failure_at = completed_at + Duration::seconds(2);
        store
            .conn
            .execute(
                r#"UPDATE connector_sync_recovery_jobs
                   SET status = 'queued', expected_state_revision = ?2,
                       next_attempt_at = ?3, attempt_count = 0,
                       claim_id = NULL, claim_expires_at = NULL
                   WHERE id = ?1"#,
                params![
                    final_claim[0].job_id().to_string(),
                    completed.revision() as i64,
                    failure_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("test schedules another recovery page");
        let transient_claim = store
            .claim_due_connector_sync_recovery_jobs(failure_at, 1)
            .expect("transient page is claimed");
        let deferred = store
            .finalize_claimed_connector_sync_recovery_failure(
                &transient_claim[0],
                ConnectorProviderFailure::NetworkUnavailable,
                failure_at + Duration::seconds(1),
            )
            .expect("transient failure atomically schedules backoff");
        assert!(!deferred.stopped());
        let retry_at = deferred.retry_state().unwrap().retry_at;
        assert!(store
            .finalize_claimed_connector_sync_recovery_failure(
                &transient_claim[0],
                ConnectorProviderFailure::NetworkUnavailable,
                failure_at + Duration::seconds(1),
            )
            .is_err());
        let cursor_claim = store
            .claim_due_connector_sync_recovery_jobs(retry_at, 1)
            .expect("deferred page is reclaimed when due");
        let preserved_cursor = cursor_claim[0]
            .state()
            .locator()
            .unwrap()
            .expose()
            .to_string();
        let stopped = store
            .finalize_claimed_connector_sync_recovery_failure(
                &cursor_claim[0],
                ConnectorProviderFailure::CursorExpired,
                retry_at + Duration::seconds(1),
            )
            .expect("expired cursor becomes explicit repair state");
        assert!(stopped.stopped());
        assert_eq!(stopped.locator().unwrap().expose(), preserved_cursor);
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT status FROM connector_sync_recovery_jobs WHERE id = ?1",
                    params![cursor_claim[0].job_id().to_string()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "repair_required"
        );
        let events = serde_json::to_string(&store.list_recent(100).unwrap()).unwrap();
        assert!(!events.contains(&sync_token));
        assert!(!events.contains(stream_fingerprint));
        assert!(!events.contains("secret-continuation-marker"));
        assert!(!events.contains("private-final-delta"));
        assert!(!events.contains("private-next-page"));
        assert!(!events.contains("private-remote-message"));
        assert!(!events.contains(&retry_at.to_rfc3339()));
        assert!(!events.contains("attempt_count"));
        assert_eq!(events.matches("connector.sync_recovery.resumed").count(), 1);
    }

    #[test]
    fn revocation_pending_account_cannot_be_overwritten_by_local_disconnect() {
        use crate::kernel::connectors::{
            ConnectorAccount, ConnectorCapability, ConnectorCredentialHandle, ConnectorHealth,
        };

        let store = EventStore::open_memory().expect("memory store opens");
        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "fake".to_string(),
            display_name: "Uncertain revocation".to_string(),
            tenant_ref: Some("tenant:private".to_string()),
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailSearch],
            health: ConnectorHealth::RevocationPending,
            connected_at: now,
            updated_at: now,
        };
        store
            .upsert_connector_account(&account)
            .expect("legacy pending account persists");

        assert!(store
            .begin_connector_disconnect(account.id, now + Duration::seconds(1))
            .is_err());
        let stored = store
            .list_connector_accounts()
            .expect("accounts load")
            .into_iter()
            .find(|candidate| candidate.id == account.id)
            .expect("account remains");
        assert_eq!(stored.health, ConnectorHealth::RevocationPending);
        let generation: i64 = store
            .conn
            .query_row(
                "SELECT generation FROM connector_account_generations WHERE account_id = ?1",
                params![account.id.to_string()],
                |row| row.get(0),
            )
            .expect("generation loads");
        assert_eq!(generation, 0);
    }

    #[test]
    fn runtime_recovery_does_not_claim_active_attachment_execution() {
        let store = EventStore::open_memory().expect("memory store opens");
        let landing_id = Uuid::new_v4();
        let now = Utc::now();
        let metadata = ConnectorAttachmentMetadata {
            account_id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            parent_remote_ref: "redacted:parent".to_string(),
            attachment_remote_ref: "redacted:attachment".to_string(),
            file_name: "active.pdf".to_string(),
            declared_media_type: "application/pdf".to_string(),
            size_bytes: 64,
            contains_macros: false,
            untrusted_evidence: true,
        };
        store
            .conn
            .execute(
                r#"INSERT INTO connector_attachment_landings
                   (id, account_id, account_generation, metadata_json, size_bytes,
                    tool_invocation_id, approval_request_id, request_fingerprint,
                    landing_fingerprint, workspace_root, workspace_identity,
                    storage_identity, status, attempt_count, created_at, updated_at)
                   VALUES (?1, ?2, 0, ?3, ?4, ?5, ?6, 'request-fingerprint',
                           'landing-fingerprint', 'D:\workspace', 'workspace-file-id',
                           'attachment-file-id', 'staging', 0, ?7, ?7)"#,
                params![
                    landing_id.to_string(),
                    metadata.account_id.to_string(),
                    serde_json::to_string(&metadata).expect("metadata serializes"),
                    metadata.size_bytes as i64,
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("active staging row inserts");

        assert!(store
            .claim_runtime_connector_attachment_cleanup_candidates(Utc::now(), 32)
            .expect("runtime recovery query succeeds")
            .is_empty());
        assert_eq!(
            store
                .connector_attachment_status(landing_id)
                .expect("active status loads"),
            "staging"
        );
    }

    #[test]
    fn broken_tool_projection_is_quarantined_without_starving_cleanup_batch() {
        let store = EventStore::open_memory().expect("memory store opens");
        let now = Utc::now();
        let plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: APP_UPDATE_CHECK_TOOL_ID.to_string(),
            input: json!({}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .expect("tool plan prepares");
        let good_tool = ToolInvocationRecord::running(&plan, None);
        store
            .append_tool_invocation(&good_tool)
            .expect("good tool projects");
        let good_landing_id = Uuid::new_v4();
        let bad_landing_id = Uuid::new_v4();
        for (landing_id, tool_id, suffix) in [
            (good_landing_id, good_tool.id, "good"),
            (bad_landing_id, Uuid::new_v4(), "bad"),
        ] {
            let account_id = Uuid::new_v4();
            let metadata = ConnectorAttachmentMetadata {
                account_id,
                provider_id: "microsoft".to_string(),
                parent_remote_ref: "redacted:parent".to_string(),
                attachment_remote_ref: "redacted:attachment".to_string(),
                file_name: format!("projection-{suffix}.pdf"),
                declared_media_type: "application/pdf".to_string(),
                size_bytes: 64,
                contains_macros: false,
                untrusted_evidence: true,
            };
            store
                .conn
                .execute(
                    r#"INSERT INTO connector_attachment_landings
                       (id, account_id, account_generation, metadata_json, size_bytes,
                        tool_invocation_id, approval_request_id, request_fingerprint,
                        landing_fingerprint, workspace_root, workspace_identity,
                        storage_identity, status, failure_kind, attempt_count,
                        created_at, updated_at)
                       VALUES (?1, ?2, 0, ?3, 64, ?4, ?5, ?6, ?7,
                               'D:\workspace', 'workspace-file-id', ?8,
                               'cleanup_required', 'test_cleanup', 0, ?9, ?9)"#,
                    params![
                        landing_id.to_string(),
                        account_id.to_string(),
                        serde_json::to_string(&metadata).expect("metadata serializes"),
                        tool_id.to_string(),
                        Uuid::new_v4().to_string(),
                        format!("request-{suffix}"),
                        format!("landing-{suffix}"),
                        format!("file-id-{suffix}"),
                        now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    ],
                )
                .expect("cleanup fixture inserts");
        }

        let candidates = store
            .claim_runtime_connector_attachment_cleanup_candidates(now, 32)
            .expect("mixed cleanup batch is isolated");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].landing_id, good_landing_id);
        assert_eq!(
            store
                .connector_attachment_status(bad_landing_id)
                .expect("bad projection status loads"),
            "repair_required"
        );
        let failure_kind: String = store
            .conn
            .query_row(
                "SELECT failure_kind FROM connector_attachment_landings WHERE id = ?1",
                params![bad_landing_id.to_string()],
                |row| row.get(0),
            )
            .expect("quarantine failure kind loads");
        assert_eq!(failure_kind, "recovery_projection_unavailable");
        let recovery_item = store
            .list_connector_recovery_items()
            .expect("recovery items load")
            .into_iter()
            .find(|item| item.id == bad_landing_id)
            .expect("bad projection is visible for recovery");
        assert!(recovery_item.action.is_none());
        let serialized = serde_json::to_string(&recovery_item).expect("item serializes");
        assert!(!serialized.contains("cleanup_claim"));
        assert!(!serialized.contains(r"D:\workspace"));
        assert_eq!(
            store
                .tool_invocation_by_id(good_tool.id)
                .expect("good tool remains projected")
                .status,
            ToolExecutionStatus::Failed
        );
    }

    #[cfg(windows)]
    #[test]
    fn ready_recovery_backoff_prevents_first_batch_starvation() {
        use crate::kernel::connectors::landing::ConnectorAttachmentLandingReceipt;

        let store = EventStore::open_memory().expect("memory store opens");
        let now = Utc::now();
        let account_id = Uuid::new_v4();
        let mut landing_ids = Vec::new();
        for index in 0..65 {
            let landing_id = Uuid::new_v4();
            landing_ids.push(landing_id);
            let metadata = ConnectorAttachmentMetadata {
                account_id,
                provider_id: "microsoft".to_string(),
                parent_remote_ref: "redacted:parent".to_string(),
                attachment_remote_ref: "redacted:attachment".to_string(),
                file_name: format!("ready-{index}.pdf"),
                declared_media_type: "application/pdf".to_string(),
                size_bytes: 64,
                contains_macros: false,
                untrusted_evidence: true,
            };
            let receipt = ConnectorAttachmentLandingReceipt {
                landing_id,
                account_id,
                provider_id: "microsoft".to_string(),
                account_generation: 0,
                landing_ref: format!("ready-{index}.pdf"),
                media_type: "application/pdf".to_string(),
                byte_size: 64,
                sha256: "a".repeat(64),
                storage_identity: format!("file-id-{index}"),
                untrusted_evidence: true,
                completed_at: now,
            };
            store
                .conn
                .execute(
                    r#"INSERT INTO connector_attachment_landings
                       (id, account_id, account_generation, metadata_json, size_bytes,
                        tool_invocation_id, approval_request_id, request_fingerprint,
                        landing_fingerprint, workspace_root, workspace_identity,
                        storage_identity, status, receipt_json, attempt_count,
                        created_at, updated_at)
                       VALUES (?1, ?2, 0, ?3, 64, ?4, ?5, ?6, ?7,
                               'D:\workspace', 'workspace-file-id', ?8,
                               'ready', ?9, 0, ?10, ?10)"#,
                    params![
                        landing_id.to_string(),
                        account_id.to_string(),
                        serde_json::to_string(&metadata).expect("metadata serializes"),
                        Uuid::new_v4().to_string(),
                        Uuid::new_v4().to_string(),
                        format!("request-{index}"),
                        format!("landing-{index}"),
                        receipt.storage_identity,
                        serde_json::to_string(&receipt).expect("receipt serializes"),
                        now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    ],
                )
                .expect("ready row inserts");
        }

        let first_batch = store
            .claim_startup_ready_connector_attachment_recovery_candidates(now, 64)
            .expect("first ready batch loads");
        assert_eq!(first_batch.len(), 64);
        let deferred_at = now;
        for candidate in &first_batch {
            store
                .defer_connector_attachment_ready_recovery(
                    candidate.landing_id,
                    candidate.claim_id,
                    deferred_at,
                )
                .expect("ready retry defers");
        }
        let next_batch = store
            .claim_startup_ready_connector_attachment_recovery_candidates(deferred_at, 64)
            .expect("next ready batch loads");
        assert_eq!(next_batch.len(), 1);
        assert!(landing_ids.contains(&next_batch[0].landing_id));
        assert!(!first_batch
            .iter()
            .any(|candidate| candidate.landing_id == next_batch[0].landing_id));
    }

    #[cfg(windows)]
    #[test]
    fn attachment_recovery_claims_are_persistent_fenced_and_runtime_retryable() {
        use crate::kernel::connectors::landing::ConnectorAttachmentLandingReceipt;

        fn insert_ready(store: &EventStore, landing_id: Uuid, suffix: &str, now: DateTime<Utc>) {
            let account_id = Uuid::new_v4();
            let metadata = ConnectorAttachmentMetadata {
                account_id,
                provider_id: "microsoft".to_string(),
                parent_remote_ref: "redacted:parent".to_string(),
                attachment_remote_ref: "redacted:attachment".to_string(),
                file_name: format!("claim-{suffix}.pdf"),
                declared_media_type: "application/pdf".to_string(),
                size_bytes: 64,
                contains_macros: false,
                untrusted_evidence: true,
            };
            let receipt = ConnectorAttachmentLandingReceipt {
                landing_id,
                account_id,
                provider_id: "microsoft".to_string(),
                account_generation: 0,
                landing_ref: format!("claim-{suffix}.pdf"),
                media_type: "application/pdf".to_string(),
                byte_size: 64,
                sha256: "a".repeat(64),
                storage_identity: format!("file-id-{suffix}"),
                untrusted_evidence: true,
                completed_at: now,
            };
            store
                .conn
                .execute(
                    r#"INSERT INTO connector_attachment_landings
                       (id, account_id, account_generation, metadata_json, size_bytes,
                        tool_invocation_id, approval_request_id, request_fingerprint,
                        landing_fingerprint, workspace_root, workspace_identity,
                        storage_identity, status, receipt_json, attempt_count,
                        created_at, updated_at)
                       VALUES (?1, ?2, 0, ?3, 64, ?4, ?5, ?6, ?7,
                               'D:\workspace', 'workspace-file-id', ?8,
                               'ready', ?9, 0, ?10, ?10)"#,
                    params![
                        landing_id.to_string(),
                        account_id.to_string(),
                        serde_json::to_string(&metadata).expect("metadata serializes"),
                        Uuid::new_v4().to_string(),
                        Uuid::new_v4().to_string(),
                        format!("request-{suffix}"),
                        format!("landing-{suffix}"),
                        receipt.storage_identity,
                        serde_json::to_string(&receipt).expect("receipt serializes"),
                        now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    ],
                )
                .expect("ready claim fixture inserts");
        }

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let database = temp_dir.path().join("attachment-recovery-claims.sqlite3");
        let landing_id = Uuid::new_v4();
        let now = Utc::now();
        let first_claim;
        {
            let store = EventStore::open(&database).expect("store opens");
            insert_ready(&store, landing_id, "primary", now);
            let candidate = store
                .claim_startup_ready_connector_attachment_recovery_candidates(now, 8)
                .expect("startup ready claim succeeds")
                .into_iter()
                .find(|candidate| candidate.landing_id == landing_id)
                .expect("ready candidate is claimed");
            first_claim = candidate.claim_id;
            assert!(store
                .claim_startup_ready_connector_attachment_recovery_candidates(now, 8)
                .expect("second startup scan succeeds")
                .is_empty());
            assert_eq!(
                store
                    .claim_connector_attachment_cleanup(landing_id, "competing_cleanup", now)
                    .expect("competing cleanup claim is evaluated"),
                ConnectorAttachmentCleanupClaim::KeepFile
            );
            assert!(store
                .defer_connector_attachment_ready_recovery(landing_id, Uuid::new_v4(), now,)
                .is_err());
            assert!(store
                .transition_ready_recovery_to_cleanup(
                    landing_id,
                    Uuid::new_v4(),
                    "wrong_token",
                    now,
                )
                .is_err());
        }

        let store = EventStore::open(&database).expect("store reopens");
        assert!(store
            .claim_runtime_ready_connector_attachment_recovery_candidates(now, 8)
            .expect("runtime ready scan succeeds")
            .is_empty());
        let takeover_at = now + Duration::seconds(301);
        store
            .conn
            .execute(
                r#"UPDATE connector_attachment_landings
                   SET cleanup_claim_expires_at = ?2 WHERE id = ?1"#,
                params![
                    landing_id.to_string(),
                    (now - Duration::seconds(1)).to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("first claim expires");
        let takeover = store
            .claim_runtime_ready_connector_attachment_recovery_candidates(takeover_at, 8)
            .expect("expired ready claim is reclaimed")
            .into_iter()
            .find(|candidate| candidate.landing_id == landing_id)
            .expect("runtime takes expired claim");
        assert_ne!(takeover.claim_id, first_claim);
        assert!(store
            .defer_connector_attachment_ready_recovery(landing_id, first_claim, takeover_at)
            .is_err());
        store
            .defer_connector_attachment_ready_recovery(landing_id, takeover.claim_id, takeover_at)
            .expect("current owner defers ready recovery");
        assert!(store
            .claim_runtime_ready_connector_attachment_recovery_candidates(takeover_at, 8)
            .expect("early runtime retry scan succeeds")
            .is_empty());
        let runtime_retry = store
            .claim_runtime_ready_connector_attachment_recovery_candidates(
                takeover_at + Duration::seconds(6),
                8,
            )
            .expect("due runtime retry claims")
            .into_iter()
            .find(|candidate| candidate.landing_id == landing_id)
            .expect("deferred ready recovery is retried at runtime");
        assert_ne!(runtime_retry.claim_id, takeover.claim_id);

        let reset_landing_id = Uuid::new_v4();
        insert_ready(&store, reset_landing_id, "reset", takeover_at);
        let before_reset = store
            .claim_startup_ready_connector_attachment_recovery_candidates(takeover_at, 8)
            .expect("second ready claim succeeds")
            .into_iter()
            .find(|candidate| candidate.landing_id == reset_landing_id)
            .expect("reset candidate is claimed");
        store
            .reset_stale_connector_attachment_recovery_claims(takeover_at)
            .expect("single-instance startup resets old-process claims");
        let after_reset = store
            .claim_startup_ready_connector_attachment_recovery_candidates(takeover_at, 8)
            .expect("reset candidate is reclaimed")
            .into_iter()
            .find(|candidate| candidate.landing_id == reset_landing_id)
            .expect("reset candidate receives a new claim");
        assert_ne!(after_reset.claim_id, before_reset.claim_id);
        assert!(store
            .defer_connector_attachment_ready_recovery(
                reset_landing_id,
                before_reset.claim_id,
                takeover_at,
            )
            .is_err());

        let cleanup_landing_id = Uuid::new_v4();
        let cleanup_claim_id = Uuid::new_v4();
        insert_ready(&store, cleanup_landing_id, "cleanup", now);
        store
            .conn
            .execute(
                r#"UPDATE connector_attachment_landings
                   SET status = 'cleanup_required', cleanup_claim_id = ?2,
                       cleanup_claim_expires_at = ?3
                   WHERE id = ?1"#,
                params![
                    cleanup_landing_id.to_string(),
                    cleanup_claim_id.to_string(),
                    (takeover_at + Duration::seconds(300))
                        .to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("cleanup claim fixture updates");
        let wrong_cleanup_token = Uuid::new_v4();
        assert!(store
            .defer_connector_attachment_cleanup(
                cleanup_landing_id,
                wrong_cleanup_token,
                takeover_at,
            )
            .is_err());
        assert!(store
            .mark_connector_attachment_repair_required(
                cleanup_landing_id,
                wrong_cleanup_token,
                "wrong_token",
                takeover_at,
            )
            .is_err());
        assert!(store
            .fail_connector_attachment_after_cleanup(
                cleanup_landing_id,
                wrong_cleanup_token,
                takeover_at,
            )
            .is_err());
        assert_eq!(
            store
                .connector_attachment_status(cleanup_landing_id)
                .expect("cleanup status loads"),
            "cleanup_required"
        );
        store
            .defer_connector_attachment_cleanup(cleanup_landing_id, cleanup_claim_id, takeover_at)
            .expect("current cleanup owner defers");

        let retention_landing_id = Uuid::new_v4();
        insert_ready(&store, retention_landing_id, "retention", now);
        store
            .conn
            .execute(
                r#"UPDATE connector_attachment_landings
                   SET status = 'completed', expires_at = ?2, next_cleanup_at = ?2
                   WHERE id = ?1"#,
                params![
                    retention_landing_id.to_string(),
                    (now - Duration::seconds(1)).to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("retention fixture becomes due");
        let retention = store
            .claim_expired_connector_attachment_retention_candidates(now, 8)
            .expect("retention claim succeeds")
            .into_iter()
            .find(|candidate| candidate.landing_id == retention_landing_id)
            .expect("retention candidate is claimed");
        let wrong_token = Uuid::new_v4();
        assert!(store
            .complete_connector_attachment_retention(retention_landing_id, wrong_token, now,)
            .is_err());
        assert!(store
            .mark_connector_attachment_retention_repair_required(
                retention_landing_id,
                wrong_token,
                "wrong_token",
                now,
            )
            .is_err());
        store
            .conn
            .execute(
                r#"UPDATE connector_attachment_landings
                   SET cleanup_claim_expires_at = ?2 WHERE id = ?1"#,
                params![
                    retention_landing_id.to_string(),
                    (now - Duration::seconds(1)).to_rfc3339_opts(SecondsFormat::Nanos, true),
                ],
            )
            .expect("retention claim expires");
        let retention_takeover = store
            .claim_expired_connector_attachment_retention_candidates(takeover_at, 8)
            .expect("retention takeover succeeds")
            .into_iter()
            .find(|candidate| candidate.landing_id == retention_landing_id)
            .expect("retention is reclaimed");
        assert_ne!(retention_takeover.claim_id, retention.claim_id);
        assert!(store
            .complete_connector_attachment_retention(
                retention_landing_id,
                retention.claim_id,
                takeover_at,
            )
            .is_err());
        store
            .complete_connector_attachment_retention(
                retention_landing_id,
                retention_takeover.claim_id,
                takeover_at,
            )
            .expect("current retention owner completes");
    }

    #[cfg(windows)]
    #[test]
    fn ready_attachment_recovers_before_and_after_handle_rename_across_restart() {
        use std::io::Cursor;

        use crate::kernel::connectors::landing::{
            recover_ready_connector_attachment, stage_connector_attachment,
            ConnectorAttachmentMetadata,
        };

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let database = temp_dir.path().join("ready-recovery.sqlite3");
        let workspace = tempfile::tempdir().expect("workspace");
        let bytes = b"%PDF-1.7\nready crash recovery";
        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            display_name: "Ready recovery account".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailReadAttachment],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        let metadata = ConnectorAttachmentMetadata {
            account_id: account.id,
            provider_id: "microsoft".to_string(),
            parent_remote_ref: "ready-message".to_string(),
            attachment_remote_ref: "ready-attachment".to_string(),
            file_name: "recovery.pdf".to_string(),
            declared_media_type: "application/pdf".to_string(),
            size_bytes: bytes.len() as u64,
            contains_macros: false,
            untrusted_evidence: true,
        };

        let first_landing_id;
        {
            let store = EventStore::open(&database).expect("store opens");
            store
                .upsert_connector_account(&account)
                .expect("account persists");
            let (approval, _) = store
                .prepare_connector_attachment_download_approval(
                    &metadata,
                    workspace.path(),
                    None,
                    now,
                )
                .expect("approval prepares");
            let scope = approval.request.exact_tool.as_ref().expect("scope exists");
            let permit = store
                .approve_and_reserve_connector_attachment_download(
                    approval.request.id,
                    approval.projection_revision,
                    scope.preview_revision,
                    &scope.preview_hash,
                    "Approve pre-rename crash".to_string(),
                    now,
                )
                .expect("reservation commits");
            first_landing_id = permit.reservation_id();
            let staged =
                stage_connector_attachment(workspace.path(), &metadata, permit, Cursor::new(bytes))
                    .expect("attachment stages");
            store
                .mark_connector_attachment_staging(
                    first_landing_id,
                    &staged.receipt().storage_identity,
                    now,
                )
                .expect("staging identity persists");
            store
                .mark_connector_attachment_ready(&staged, now)
                .expect("ready persists");
            drop(staged);
        }
        {
            let store = EventStore::open(&database).expect("store reopens");
            let candidate = store
                .claim_startup_ready_connector_attachment_recovery_candidates(Utc::now(), 8)
                .expect("ready candidates load")
                .into_iter()
                .find(|candidate| candidate.landing_id == first_landing_id)
                .expect("pre-rename candidate exists");
            let landed = recover_ready_connector_attachment(&candidate)
                .expect("temp file is committed during recovery");
            assert!(store
                .complete_recovered_connector_attachment_landing(
                    &landed,
                    Uuid::new_v4(),
                    Utc::now(),
                )
                .is_err());
            store
                .complete_recovered_connector_attachment_landing(
                    &landed,
                    candidate.claim_id,
                    Utc::now(),
                )
                .expect("completion is repaired");
            assert!(store
                .complete_recovered_connector_attachment_landing(
                    &landed,
                    candidate.claim_id,
                    Utc::now(),
                )
                .is_err());
            assert_eq!(
                store
                    .connector_attachment_status(first_landing_id)
                    .expect("status loads"),
                "completed"
            );
        }

        let second_landing_id;
        {
            let store = EventStore::open(&database).expect("store reopens");
            let (approval, _) = store
                .prepare_connector_attachment_download_approval(
                    &metadata,
                    workspace.path(),
                    None,
                    Utc::now(),
                )
                .expect("second approval prepares");
            let scope = approval.request.exact_tool.as_ref().expect("scope exists");
            let permit = store
                .approve_and_reserve_connector_attachment_download(
                    approval.request.id,
                    approval.projection_revision,
                    scope.preview_revision,
                    &scope.preview_hash,
                    "Approve post-rename crash".to_string(),
                    Utc::now(),
                )
                .expect("second reservation commits");
            second_landing_id = permit.reservation_id();
            let staged =
                stage_connector_attachment(workspace.path(), &metadata, permit, Cursor::new(bytes))
                    .expect("second attachment stages");
            store
                .mark_connector_attachment_staging(
                    second_landing_id,
                    &staged.receipt().storage_identity,
                    Utc::now(),
                )
                .expect("second staging identity persists");
            store
                .mark_connector_attachment_ready(&staged, Utc::now())
                .expect("second ready persists");
            let landed = staged.commit().expect("handle rename commits");
            drop(landed);
        }
        {
            let store = EventStore::open(&database).expect("store reopens again");
            let candidate = store
                .claim_startup_ready_connector_attachment_recovery_candidates(Utc::now(), 8)
                .expect("ready candidates load")
                .into_iter()
                .find(|candidate| candidate.landing_id == second_landing_id)
                .expect("post-rename candidate exists");
            let landed = recover_ready_connector_attachment(&candidate)
                .expect("final file is recognized during recovery");
            store
                .complete_recovered_connector_attachment_landing(
                    &landed,
                    candidate.claim_id,
                    Utc::now(),
                )
                .expect("post-rename completion is repaired");
            let retained_path = landed.path().to_path_buf();
            drop(landed);
            assert_eq!(
                store
                    .connector_attachment_status(second_landing_id)
                    .expect("status loads"),
                "completed"
            );
            let expired_at = Utc::now() - Duration::seconds(1);
            store
                .conn
                .execute(
                    r#"UPDATE connector_attachment_landings
                       SET expires_at = ?2, next_cleanup_at = ?2 WHERE id = ?1"#,
                    rusqlite::params![second_landing_id.to_string(), expired_at.to_rfc3339(),],
                )
                .expect("retention is made due");
            let retention = store
                .claim_expired_connector_attachment_retention_candidates(Utc::now(), 8)
                .expect("retention claims")
                .into_iter()
                .find(|candidate| candidate.landing_id == second_landing_id)
                .expect("expired candidate exists");
            crate::kernel::connectors::landing::cleanup_incomplete_connector_attachment(&retention)
                .expect("exact retained file is deleted");
            store
                .complete_connector_attachment_retention(
                    second_landing_id,
                    retention.claim_id,
                    Utc::now(),
                )
                .expect("retention completes");
            assert!(!retained_path.exists());
            assert_eq!(
                store
                    .connector_attachment_status(second_landing_id)
                    .expect("expired status loads"),
                "expired"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn staging_attachment_persists_identity_before_bytes_and_cleans_exact_file_after_restart() {
        use std::io::Write;

        use crate::kernel::connectors::landing::ConnectorAttachmentMetadata;
        use crate::kernel::connectors::landing_windows::ManagedLandingRoot;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let database = temp_dir.path().join("staging-recovery.sqlite3");
        let workspace = tempfile::tempdir().expect("workspace");
        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            display_name: "Staging recovery account".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailReadAttachment],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        let metadata = ConnectorAttachmentMetadata {
            account_id: account.id,
            provider_id: "microsoft".to_string(),
            parent_remote_ref: "staging-message".to_string(),
            attachment_remote_ref: "staging-attachment".to_string(),
            file_name: "staging.pdf".to_string(),
            declared_media_type: "application/pdf".to_string(),
            size_bytes: 4096,
            contains_macros: false,
            untrusted_evidence: true,
        };
        let landing_id;
        let temp_path;
        {
            let store = EventStore::open(&database).expect("store opens");
            store
                .upsert_connector_account(&account)
                .expect("account persists");
            let (approval, _) = store
                .prepare_connector_attachment_download_approval(
                    &metadata,
                    workspace.path(),
                    None,
                    now,
                )
                .expect("approval prepares");
            let scope = approval.request.exact_tool.as_ref().expect("scope exists");
            let permit = store
                .approve_and_reserve_connector_attachment_download(
                    approval.request.id,
                    approval.projection_revision,
                    scope.preview_revision,
                    &scope.preview_hash,
                    "Approve staging crash".to_string(),
                    now,
                )
                .expect("reservation commits");
            landing_id = permit.reservation_id();
            let managed = ManagedLandingRoot::open(workspace.path()).expect("managed root opens");
            let basename = format!(".{landing_id}.part");
            temp_path = managed.landing_root().join(&basename);
            let mut file = managed
                .create_staged_file(&basename)
                .expect("staging file opens");
            let identity = managed
                .file_identity(&file)
                .expect("identity loads")
                .encoded();
            store
                .mark_connector_attachment_staging(landing_id, &identity, now)
                .expect("identity persists before bytes");
            file.write_all(b"%PDF-partial")
                .expect("partial bytes write");
            file.sync_all().expect("partial bytes sync");
            store
                .begin_connector_disconnect(account.id, now)
                .expect("disconnect invalidates staging generation");
            assert!(store
                .assert_connector_attachment_execution_current(landing_id)
                .is_err());
            let locked_candidate = store
                .claim_runtime_connector_attachment_cleanup_candidates(now, 8)
                .expect("disconnect cleanup claims")
                .into_iter()
                .find(|candidate| candidate.landing_id == landing_id)
                .expect("disconnect cleanup candidate loads");
            assert_eq!(
                crate::kernel::connectors::landing::cleanup_incomplete_connector_attachment(
                    &locked_candidate
                ),
                Err(crate::kernel::connectors::landing::ConnectorAttachmentCleanupFailure::Transient)
            );
            store
                .defer_connector_attachment_cleanup(landing_id, locked_candidate.claim_id, now)
                .expect("locked cleanup is deferred");
            assert!(store
                .claim_startup_connector_attachment_cleanup_candidates(now, 8)
                .expect("immediate retry scan succeeds")
                .is_empty());
        }
        assert!(temp_path.exists());
        {
            let store = EventStore::open(&database).expect("store reopens");
            store
                .conn
                .execute(
                    r#"UPDATE connector_attachment_landings
                       SET next_cleanup_at = ?2 WHERE id = ?1"#,
                    rusqlite::params![
                        landing_id.to_string(),
                        (Utc::now() - Duration::seconds(1)).to_rfc3339(),
                    ],
                )
                .expect("deferred cleanup is made due");
            let candidate = store
                .claim_startup_connector_attachment_cleanup_candidates(Utc::now(), 8)
                .expect("staging cleanup claims")
                .into_iter()
                .find(|candidate| candidate.landing_id == landing_id)
                .expect("staging candidate exists");
            assert!(candidate.receipt.is_none());
            assert!(candidate.storage_identity.is_some());
            crate::kernel::connectors::landing::cleanup_incomplete_connector_attachment(&candidate)
                .expect("exact partial file is removed");
            store
                .fail_connector_attachment_after_cleanup(landing_id, candidate.claim_id, Utc::now())
                .expect("staging landing terminalizes");
            assert_eq!(
                store
                    .connector_attachment_status(landing_id)
                    .expect("status loads"),
                "failed"
            );
        }
        assert!(!temp_path.exists());
    }

    #[test]
    fn attachment_reservation_budget_fails_closed_without_consuming_approval() {
        use crate::kernel::connectors::landing::ConnectorAttachmentMetadata;

        let store = EventStore::open_memory().expect("store opens");
        let workspace = tempfile::tempdir().expect("workspace");
        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            display_name: "Budget account".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailReadAttachment],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let metadata = ConnectorAttachmentMetadata {
            account_id: account.id,
            provider_id: "microsoft".to_string(),
            parent_remote_ref: "budget-message".to_string(),
            attachment_remote_ref: "budget-attachment".to_string(),
            file_name: "budget.pdf".to_string(),
            declared_media_type: "application/pdf".to_string(),
            size_bytes: 4096,
            contains_macros: false,
            untrusted_evidence: true,
        };
        let (approval, _) = store
            .prepare_connector_attachment_download_approval(&metadata, workspace.path(), None, now)
            .expect("approval prepares");
        let scope = approval.request.exact_tool.as_ref().expect("scope exists");
        let workspace_identity: String = store
            .conn
            .query_row(
                "SELECT workspace_identity FROM connector_attachment_sources WHERE request_id = ?1",
                rusqlite::params![approval.request.id.to_string()],
                |row| row.get(0),
            )
            .expect("workspace identity loads");
        for index in 0..super::MAX_RETAINED_ATTACHMENTS_PER_WORKSPACE {
            store
                .conn
                .execute(
                    r#"INSERT INTO connector_attachment_landings
                       (id, account_id, account_generation, metadata_json, size_bytes,
                        tool_invocation_id, approval_request_id, request_fingerprint,
                        landing_fingerprint, workspace_root, workspace_identity, status,
                        created_at, updated_at)
                       VALUES (?1, ?2, 0, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9,
                               'completed', ?10, ?10)"#,
                    rusqlite::params![
                        Uuid::new_v4().to_string(),
                        account.id.to_string(),
                        serde_json::to_string(&super::durable_connector_attachment_metadata(
                            &metadata
                        ))
                        .unwrap(),
                        Uuid::new_v4().to_string(),
                        Uuid::new_v4().to_string(),
                        format!("budget-request-{index}"),
                        format!("budget-landing-{index}"),
                        workspace.path().to_string_lossy(),
                        workspace_identity,
                        now.to_rfc3339(),
                    ],
                )
                .expect("budget row inserts");
        }
        assert!(store
            .approve_and_reserve_connector_attachment_download(
                approval.request.id,
                approval.projection_revision,
                scope.preview_revision,
                &scope.preview_hash,
                "Approve over budget".to_string(),
                now,
            )
            .is_err());
        let pending = store
            .capability_access_record_by_id(approval.request.id)
            .expect("approval remains projected");
        assert_eq!(
            pending.effective_status,
            CapabilityAccessStatus::PendingApproval
        );
        assert!(store
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM connector_attachment_sources WHERE request_id = ?1)",
                rusqlite::params![approval.request.id.to_string()],
                |row| row.get::<_, bool>(0),
            )
            .expect("source remains available"));
    }

    #[test]
    fn connector_attachment_rejects_generic_and_stale_preview_resolution() {
        use crate::kernel::connectors::landing::ConnectorAttachmentMetadata;

        let store = EventStore::open_memory().expect("memory store opens");
        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            display_name: "Attachment test account".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailReadAttachment],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let metadata = ConnectorAttachmentMetadata {
            account_id: account.id,
            provider_id: "microsoft".to_string(),
            parent_remote_ref: "private-message-ref".to_string(),
            attachment_remote_ref: "private-attachment-ref".to_string(),
            file_name: "report.pdf".to_string(),
            declared_media_type: "application/pdf".to_string(),
            size_bytes: 4096,
            contains_macros: false,
            untrusted_evidence: true,
        };
        let workspace = tempfile::tempdir().expect("workspace");
        let (record, tool) = store
            .prepare_connector_attachment_download_approval(&metadata, workspace.path(), None, now)
            .expect("approval prepares");
        let request = record.request;
        let scope = request.exact_tool.clone().expect("scope exists");

        assert!(store
            .resolve_capability_access_request(request.id, true, "generic".to_string())
            .is_err());
        assert!(store
            .approve_and_reserve_connector_attachment_download(
                request.id,
                0,
                scope.preview_revision,
                "stale-hash",
                "stale preview".to_string(),
                now,
            )
            .is_err());
        assert_eq!(
            store
                .capability_access_record_by_id(request.id)
                .expect("request remains pending")
                .effective_status,
            CapabilityAccessStatus::PendingApproval
        );
        let resolution = store
            .reject_connector_attachment_download(
                request.id,
                0,
                scope.preview_revision,
                &scope.preview_hash,
                "Do not download this attachment".to_string(),
                now,
            )
            .expect("rejection terminalizes the exact tool atomically");
        assert!(!resolution.approved);
        assert_eq!(
            store
                .tool_invocation_by_id(tool.id)
                .expect("tool projection loads")
                .status,
            crate::kernel::tool_runtime::ToolExecutionStatus::Blocked
        );
        let source_count: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM connector_attachment_sources WHERE request_id = ?1",
                rusqlite::params![request.id.to_string()],
                |row| row.get(0),
            )
            .expect("source count loads");
        assert_eq!(source_count, 0);
        let events = serde_json::to_string(&store.list_recent(100).unwrap()).unwrap();
        assert!(!events.contains("private-message-ref"));
        assert!(!events.contains("private-attachment-ref"));
    }

    fn revocation_test_account(now: DateTime<Utc>, label: &str) -> ConnectorAccount {
        ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "fake".to_string(),
            display_name: label.to_string(),
            tenant_ref: Some("tenant:revocation-test".to_string()),
            credential_handle: crate::kernel::connectors::ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailSearch],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn malformed_due_revocations_are_quarantined_without_starving_healthy_work() {
        let store = EventStore::open_memory().expect("memory store opens");
        let now = Utc::now();
        let mut malformed_ids = Vec::new();
        for index in 0..9 {
            let account = revocation_test_account(now, &format!("Malformed {index}"));
            store.upsert_connector_account(&account).unwrap();
            malformed_ids.push(
                store
                    .begin_connector_revocation(account.id, now)
                    .unwrap()
                    .id(),
            );
        }
        let healthy = revocation_test_account(now, "Healthy");
        store.upsert_connector_account(&healthy).unwrap();
        let healthy_ticket = store.begin_connector_revocation(healthy.id, now).unwrap();
        for id in &malformed_ids {
            store
                .conn
                .execute(
                    "UPDATE connector_revocations SET ticket_json = '{' WHERE id = ?1",
                    params![id.to_string()],
                )
                .unwrap();
        }

        let claims = store.claim_due_connector_revocations(now, 1).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].ticket().id(), healthy_ticket.id());
        let quarantined: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM connector_revocations WHERE quarantine_code = 'invalid_projection_binding'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(quarantined, malformed_ids.len() as i64);
    }

    #[test]
    fn preclaim_revocation_binding_tampering_fails_closed() {
        for tamper in ["provider", "tenant", "credential", "generation"] {
            let store = EventStore::open_memory().expect("memory store opens");
            let now = Utc::now();
            let account = revocation_test_account(now, tamper);
            store.upsert_connector_account(&account).unwrap();
            let ticket = store.begin_connector_revocation(account.id, now).unwrap();
            if tamper == "generation" {
                store
                    .conn
                    .execute(
                        "UPDATE connector_account_generations SET generation = generation + 1 WHERE account_id = ?1",
                        params![account.id.to_string()],
                    )
                    .unwrap();
            } else {
                let mut current = store.list_connector_accounts().unwrap().pop().unwrap();
                match tamper {
                    "provider" => current.provider_id = "other-provider".to_string(),
                    "tenant" => current.tenant_ref = Some("tenant:other".to_string()),
                    "credential" => {
                        current.credential_handle =
                            crate::kernel::connectors::ConnectorCredentialHandle::new()
                    }
                    _ => unreachable!(),
                }
                store
                    .conn
                    .execute(
                        "UPDATE connector_accounts SET account_json = ?2 WHERE id = ?1",
                        params![
                            account.id.to_string(),
                            serde_json::to_string(&current).unwrap()
                        ],
                    )
                    .unwrap();
            }

            assert!(store
                .claim_due_connector_revocations(now, 1)
                .unwrap()
                .is_empty());
            let quarantine: Option<String> = store
                .conn
                .query_row(
                    "SELECT quarantine_code FROM connector_revocations WHERE id = ?1",
                    params![ticket.id().to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(quarantine.as_deref(), Some("invalid_projection_binding"));
        }
    }
}
