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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FoundationState {
    pub app_name: String,
    pub model_route: ModelRoute,
    pub thinking_level: ThinkingLevel,
    pub access_mode: AccessMode,
    pub workspace_scope: WorkspaceScope,
}

impl Default for FoundationState {
    fn default() -> Self {
        Self {
            app_name: "DeepSeek Agent OS".to_string(),
            model_route: ModelRoute::Auto,
            thinking_level: ThinkingLevel::Auto,
            access_mode: AccessMode::AskOnRisk,
            workspace_scope: WorkspaceScope::Workspace,
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
        AccessMode, FoundationState, KernelEvent, ModelRoute, TaskRecord, TaskRecordStatus,
        ThinkingLevel, WorkspaceScope,
    };

    #[test]
    fn foundation_state_defaults_to_deepseek_agent_os() {
        let state = FoundationState::default();

        assert_eq!(state.app_name, "DeepSeek Agent OS");
        assert_eq!(state.model_route, ModelRoute::Auto);
        assert_eq!(state.thinking_level, ThinkingLevel::Auto);
        assert_eq!(state.access_mode, AccessMode::AskOnRisk);
        assert_eq!(state.workspace_scope, WorkspaceScope::Workspace);
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
}
