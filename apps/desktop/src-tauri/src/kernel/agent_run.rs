use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Queued,
    Running,
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
pub struct AgentRunQueuedGuidance {
    pub id: Uuid,
    pub run_id: Uuid,
    pub guidance: String,
    pub queued_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentRunCancelRequest {
    pub id: Uuid,
    pub run_id: Uuid,
    pub reason: String,
    pub requested_at: DateTime<Utc>,
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
    pub attachment_count: usize,
    pub status: AgentRunStatus,
    pub worker_id: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub queued_guidance: Vec<AgentRunQueuedGuidance>,
    pub steps: Vec<AgentRunStepRecord>,
    pub artifacts: Vec<AgentRunArtifactRecord>,
    pub cancel_requested: bool,
    pub cancel_reason: Option<String>,
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

impl AgentRunQueuedGuidance {
    pub fn new(run_id: Uuid, guidance: String) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            run_id,
            guidance: required_text(guidance, "agent run queued guidance")?,
            queued_at: Utc::now(),
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

impl AgentRunFinish {
    pub fn new(
        run_id: Uuid,
        status: AgentRunStatus,
        summary: Option<String>,
        error: Option<String>,
    ) -> Result<Self, String> {
        if matches!(
            status,
            AgentRunStatus::Queued | AgentRunStatus::Running | AgentRunStatus::CancelRequested
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
