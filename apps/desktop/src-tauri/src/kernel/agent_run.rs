use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::kernel::expert_team::{ExpertAttemptContract, ExpertAttemptResult, ExpertMergeReceipt};

pub const AGENT_RUN_GUIDANCE_MAX_CHARS: usize = 4_000;
pub const AGENT_RUN_MAX_PARALLEL_SUBAGENTS: usize = 3;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunRole {
    #[default]
    Parent,
    Subagent,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Queued,
    Running,
    WaitingForPrerequisite,
    WaitingForConfirmation,
    Blocked,
    CancelRequested,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunStart {
    pub id: Uuid,
    pub conversation_id: String,
    pub prompt: String,
    pub attachment_count: usize,
    #[serde(default)]
    pub role: AgentRunRole,
    #[serde(default)]
    pub parent_run_id: Option<Uuid>,
    #[serde(default)]
    pub subtask_key: Option<String>,
    #[serde(default)]
    pub expert_contract: Option<ExpertAttemptContract>,
    #[serde(default = "default_agent_run_initial_status")]
    pub initial_status: AgentRunStatus,
    pub started_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunClaim {
    pub id: Uuid,
    pub run_id: Uuid,
    pub worker_id: String,
    pub claimed_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunRecovery {
    pub id: Uuid,
    pub run_id: Uuid,
    pub previous_worker_id: String,
    pub previous_lease_expires_at: DateTime<Utc>,
    pub reason: String,
    pub recovered_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunRecoverySweep {
    pub recovered: usize,
    pub blocked: usize,
    pub cancelled: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunExecutionContext {
    pub id: Uuid,
    pub run_id: Uuid,
    pub execution_prompt: String,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunContinuationQueued {
    pub id: Uuid,
    pub run_id: Uuid,
    pub tool_invocation_id: Uuid,
    pub reason: String,
    pub queued_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunQueuedGuidance {
    pub id: Uuid,
    pub run_id: Uuid,
    pub guidance: String,
    pub queued_at: DateTime<Utc>,
    #[serde(default)]
    pub applied_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunGuidanceApplied {
    pub id: Uuid,
    pub run_id: Uuid,
    pub guidance_id: Uuid,
    pub applied_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunCancelRequest {
    pub id: Uuid,
    pub run_id: Uuid,
    pub reason: String,
    pub requested_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunTransition {
    pub id: Uuid,
    pub run_id: Uuid,
    pub status: AgentRunStatus,
    pub reason: String,
    pub tool_invocation_id: Option<Uuid>,
    pub transitioned_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunResourceAccess {
    Read,
    Write,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunResourceClaim {
    pub id: Uuid,
    pub run_id: Option<Uuid>,
    pub tool_invocation_id: Uuid,
    pub resource_key: String,
    pub access: AgentRunResourceAccess,
    pub claimed_at: DateTime<Utc>,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunResourceRelease {
    pub id: Uuid,
    pub claim_id: Uuid,
    pub run_id: Option<Uuid>,
    pub tool_invocation_id: Uuid,
    pub resource_key: String,
    pub outcome: String,
    pub released_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunFinish {
    pub id: Uuid,
    pub run_id: Uuid,
    pub status: AgentRunStatus,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub finished_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStepStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunStepRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub sequence: u32,
    pub status: AgentRunStepStatus,
    pub label: String,
    pub detail: String,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunArtifactRecord {
    pub id: Uuid,
    pub run_id: Uuid,
    pub kind: String,
    pub title: String,
    pub path: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunRecord {
    pub id: Uuid,
    pub conversation_id: String,
    pub prompt: String,
    #[serde(default)]
    pub execution_prompt: Option<String>,
    #[serde(default)]
    pub execution_context_recorded_at: Option<DateTime<Utc>>,
    pub attachment_count: usize,
    pub role: AgentRunRole,
    pub parent_run_id: Option<Uuid>,
    pub subtask_key: Option<String>,
    #[serde(default)]
    pub expert_contract: Option<ExpertAttemptContract>,
    #[serde(default)]
    pub expert_result: Option<ExpertAttemptResult>,
    #[serde(default)]
    pub expert_merge_receipt: Option<ExpertMergeReceipt>,
    pub status: AgentRunStatus,
    pub worker_id: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub recovery_count: usize,
    #[serde(default)]
    pub last_recovered_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub recovery_reason: Option<String>,
    #[serde(default)]
    pub continuation_count: usize,
    #[serde(default)]
    pub continuation_queued_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub continuation_tool_invocation_id: Option<Uuid>,
    pub queued_guidance: Vec<AgentRunQueuedGuidance>,
    pub steps: Vec<AgentRunStepRecord>,
    pub artifacts: Vec<AgentRunArtifactRecord>,
    pub cancel_requested: bool,
    pub cancel_reason: Option<String>,
    pub status_reason: Option<String>,
    pub waiting_tool_invocation_id: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub finish_summary: Option<String>,
    pub finish_error: Option<String>,
}

impl AgentRunStart {
    pub fn new(
        conversation_id: String,
        prompt: String,
        attachment_count: usize,
    ) -> Result<Self, String> {
        let conversation_id = required_text(conversation_id, "agent run conversation_id")?;
        let prompt = required_text(prompt, "agent run prompt")?;
        Ok(Self {
            id: Uuid::new_v4(),
            conversation_id,
            prompt,
            attachment_count,
            role: AgentRunRole::Parent,
            parent_run_id: None,
            subtask_key: None,
            expert_contract: None,
            initial_status: AgentRunStatus::Running,
            started_at: Utc::now(),
        })
    }

    pub fn queued(
        conversation_id: String,
        prompt: String,
        attachment_count: usize,
    ) -> Result<Self, String> {
        let mut start = Self::new(conversation_id, prompt, attachment_count)?;
        start.initial_status = AgentRunStatus::Queued;
        Ok(start)
    }

    pub fn queued_subagent(
        parent_run_id: Uuid,
        conversation_id: String,
        subtask_key: String,
        prompt: String,
    ) -> Result<Self, String> {
        if parent_run_id.is_nil() {
            return Err("subagent parent run id is required".to_string());
        }
        let mut start = Self::queued(conversation_id, prompt, 0)?;
        start.role = AgentRunRole::Subagent;
        start.parent_run_id = Some(parent_run_id);
        start.subtask_key = Some(required_text(subtask_key, "subagent subtask key")?);
        Ok(start)
    }

    pub fn queued_expert(
        parent_run_id: Uuid,
        conversation_id: String,
        contract: ExpertAttemptContract,
    ) -> Result<Self, String> {
        if contract.parent_run_id != parent_run_id {
            return Err("expert contract parent run id does not match its run".to_string());
        }
        let mut start = Self::queued_subagent(
            parent_run_id,
            conversation_id,
            contract.key.clone(),
            contract.prompt.clone(),
        )?;
        start.expert_contract = Some(contract);
        Ok(start)
    }
}

impl AgentRunClaim {
    pub fn new(run_id: Uuid, worker_id: String, lease_seconds: i64) -> Result<Self, String> {
        let worker_id = required_text(worker_id, "agent run worker_id")?;
        let lease_seconds = lease_seconds.max(1);
        let claimed_at = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            worker_id,
            claimed_at,
            lease_expires_at: claimed_at + chrono::Duration::seconds(lease_seconds),
        })
    }
}

impl AgentRunRecovery {
    pub fn new(
        run_id: Uuid,
        previous_worker_id: String,
        previous_lease_expires_at: DateTime<Utc>,
        reason: String,
    ) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            previous_worker_id: required_text(
                previous_worker_id,
                "agent run recovery previous_worker_id",
            )?,
            previous_lease_expires_at,
            reason: required_text(reason, "agent run recovery reason")?,
            recovered_at: Utc::now(),
        })
    }
}

impl AgentRunExecutionContext {
    pub fn new(run_id: Uuid, execution_prompt: String) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            execution_prompt: required_text(execution_prompt, "agent run execution prompt")?,
            recorded_at: Utc::now(),
        })
    }
}

impl AgentRunContinuationQueued {
    pub fn new(run_id: Uuid, tool_invocation_id: Uuid, reason: String) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            tool_invocation_id,
            reason: required_text(reason, "agent run continuation reason")?,
            queued_at: Utc::now(),
        })
    }
}

impl AgentRunQueuedGuidance {
    pub fn new(run_id: Uuid, guidance: String) -> Result<Self, String> {
        let guidance = required_text(guidance, "agent run queued guidance")?;
        if guidance.chars().count() > AGENT_RUN_GUIDANCE_MAX_CHARS {
            return Err(format!(
                "agent run queued guidance exceeds {AGENT_RUN_GUIDANCE_MAX_CHARS} characters"
            ));
        }
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            guidance,
            queued_at: Utc::now(),
            applied_at: None,
        })
    }
}

impl AgentRunGuidanceApplied {
    pub fn new(run_id: Uuid, guidance_id: Uuid) -> Result<Self, String> {
        if guidance_id.is_nil() {
            return Err("agent run applied guidance id is required".to_string());
        }
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            guidance_id,
            applied_at: Utc::now(),
        })
    }
}

impl AgentRunCancelRequest {
    pub fn new(run_id: Uuid, reason: String) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            reason: required_text(reason, "agent run cancel reason")?,
            requested_at: Utc::now(),
        })
    }
}

impl AgentRunTransition {
    pub fn new(
        run_id: Uuid,
        status: AgentRunStatus,
        reason: String,
        tool_invocation_id: Option<Uuid>,
    ) -> Result<Self, String> {
        if !matches!(
            status,
            AgentRunStatus::Queued
                | AgentRunStatus::Running
                | AgentRunStatus::WaitingForPrerequisite
                | AgentRunStatus::WaitingForConfirmation
                | AgentRunStatus::Blocked
        ) {
            return Err("agent run transition status must be queued or non-terminal".to_string());
        }
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            status,
            reason: required_text(reason, "agent run transition reason")?,
            tool_invocation_id,
            transitioned_at: Utc::now(),
        })
    }
}

impl AgentRunResourceClaim {
    pub fn new(
        run_id: impl Into<Option<Uuid>>,
        tool_invocation_id: Uuid,
        resource_key: String,
        access: AgentRunResourceAccess,
        lease_seconds: i64,
    ) -> Result<Self, String> {
        let claimed_at = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            run_id: run_id.into(),
            tool_invocation_id,
            resource_key: required_text(resource_key, "agent run resource key")?,
            access,
            claimed_at,
            lease_expires_at: claimed_at + chrono::Duration::seconds(lease_seconds.max(1)),
        })
    }
}

impl AgentRunResourceRelease {
    pub fn new(claim: &AgentRunResourceClaim, outcome: String) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            claim_id: claim.id,
            run_id: claim.run_id,
            tool_invocation_id: claim.tool_invocation_id,
            resource_key: claim.resource_key.clone(),
            outcome: required_text(outcome, "agent run resource release outcome")?,
            released_at: Utc::now(),
        })
    }
}

impl AgentRunFinish {
    pub fn new(
        run_id: Uuid,
        status: AgentRunStatus,
        summary: Option<String>,
        error: Option<String>,
    ) -> Result<Self, String> {
        if matches!(
            status,
            AgentRunStatus::Queued
                | AgentRunStatus::Running
                | AgentRunStatus::WaitingForPrerequisite
                | AgentRunStatus::WaitingForConfirmation
                | AgentRunStatus::Blocked
                | AgentRunStatus::CancelRequested
        ) {
            return Err("agent run finish status must be terminal".to_string());
        }
        let summary = normalize_optional_text(summary);
        let error = normalize_optional_text(error);
        if status == AgentRunStatus::Failed && error.is_none() {
            return Err("agent run failure requires an error".to_string());
        }
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            status,
            summary,
            error,
            finished_at: Utc::now(),
        })
    }

    pub fn completed(run_id: Uuid, summary: String) -> Result<Self, String> {
        Self::new(run_id, AgentRunStatus::Completed, Some(summary), None)
    }
}

impl AgentRunStepRecord {
    pub fn new(
        run_id: Uuid,
        sequence: u32,
        status: AgentRunStepStatus,
        label: String,
        detail: String,
    ) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            sequence,
            status,
            label: required_text(label, "agent run step label")?,
            detail: required_text(detail, "agent run step detail")?,
            recorded_at: Utc::now(),
        })
    }
}

impl AgentRunArtifactRecord {
    pub fn new(run_id: Uuid, kind: String, title: String, path: String) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            kind: required_text(kind, "agent run artifact kind")?,
            title: required_text(title, "agent run artifact title")?,
            path: required_text(path, "agent run artifact path")?,
            created_at: Utc::now(),
        })
    }
}

fn default_agent_run_initial_status() -> AgentRunStatus {
    AgentRunStatus::Running
}

fn required_text(value: String, field: &'static str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{field} is required"));
    }
    Ok(value)
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
