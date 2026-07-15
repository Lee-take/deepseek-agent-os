use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashSet;
use uuid::Uuid;

use crate::kernel::agent_run::{AgentRunRecord, AgentRunStatus};
use crate::kernel::artifacts::{ArtifactDeliveryView, ArtifactPhase};
use crate::kernel::automation::{
    AutomationRun, AutomationRunStatus, ReviewQueueItem, ReviewQueueItemStatus,
};
use crate::kernel::computer_use_session::{
    ComputerUseSession, ComputerUseStep, ComputerUseStepStatus, ComputerUseUndoCapability,
};
use crate::kernel::connectors::{
    ConnectorInvocation, ConnectorInvocationStatus, ConnectorRecoveryAction,
    ConnectorRecoveryExternalEffectState, ConnectorRecoveryItem, ConnectorRecoveryStatus,
};
use crate::kernel::event_store::{EventStore, EventStoreResult};
use crate::kernel::expert_team::ExpertExternalEffectState;
use crate::kernel::models::{TaskRecord, TaskRecordStatus};
use crate::kernel::policy::CapabilityKind;
use crate::kernel::tool_runtime::{ToolExecutionStatus, ToolInvocationRecord};
use crate::kernel::workspace_undo::{
    WorkspaceCheckpointEffectState, WorkspaceMutationCheckpointStatus, WorkspaceUndoView,
};

const TASK_LIFECYCLE_LIMIT: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskLifecycleSource {
    TaskRecord,
    AgentRun,
    ExpertAttempt,
    AutomationRun,
    Review,
    ToolInvocation,
    ConnectorInvocation,
    ConnectorRecovery,
    Artifact,
    ComputerUse,
    WorkspaceCheckpoint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskLifecyclePhase {
    Queued,
    Running,
    WaitingPrerequisite,
    WaitingReview,
    WaitingApproval,
    NeedsRecovery,
    EffectUnknown,
    RepairRequired,
    Blocked,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEffectState {
    NoEffect,
    ReadOnly,
    LocalReversible,
    LocalApplied,
    LocalUncertain,
    RemoteKnownNotApplied,
    RemoteKnownApplied,
    RemoteUncertain,
    CompensationRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskLifecycleActionKind {
    ReviewDecision,
    RetryLocalCleanup,
    ResumeSync,
    InspectExternalResult,
    UndoLocalChange,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskLifecycleAction {
    pub kind: TaskLifecycleActionKind,
    pub action_revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskLifecycleItem {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub source: TaskLifecycleSource,
    pub phase: TaskLifecyclePhase,
    pub effect_state: TaskEffectState,
    pub title: String,
    pub detail_code: String,
    pub action: Option<TaskLifecycleAction>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TaskLifecycleSnapshot {
    pub items: Vec<TaskLifecycleItem>,
    pub generated_at: DateTime<Utc>,
}

impl EventStore {
    pub fn task_lifecycle_snapshot(&self) -> EventStoreResult<TaskLifecycleSnapshot> {
        let mut items = Vec::new();
        let workspace_checkpoints = self.list_workspace_undo_views()?;
        let checkpointed_invocations = workspace_checkpoints
            .iter()
            .map(|checkpoint| checkpoint.tool_invocation_id)
            .collect::<HashSet<_>>();
        items.extend(self.list_task_records()?.iter().map(task_record_item));
        items.extend(self.list_agent_run_records()?.iter().map(agent_run_item));
        items.extend(self.list_automation_runs()?.iter().map(automation_run_item));
        items.extend(self.list_review_queue_items()?.iter().map(review_item));
        items.extend(
            self.list_tool_invocations()?
                .iter()
                .filter(|invocation| !checkpointed_invocations.contains(&invocation.id))
                .map(tool_invocation_item),
        );
        items.extend(
            self.list_connector_invocations()?
                .iter()
                .map(connector_invocation_item),
        );
        items.extend(
            self.list_connector_recovery_items()?
                .iter()
                .map(connector_recovery_item),
        );
        items.extend(
            self.list_artifact_deliveries(TASK_LIFECYCLE_LIMIT)?
                .iter()
                .map(artifact_item),
        );
        items.extend(workspace_checkpoints.iter().map(workspace_checkpoint_item));
        for session in self.list_computer_use_sessions()? {
            let steps = self.list_computer_use_steps(session.id)?;
            items.push(computer_use_item(&session, steps.last()));
        }
        items.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        items.truncate(TASK_LIFECYCLE_LIMIT);
        Ok(TaskLifecycleSnapshot {
            items,
            generated_at: Utc::now(),
        })
    }
}

fn task_record_item(record: &TaskRecord) -> TaskLifecycleItem {
    let phase = match record.status {
        TaskRecordStatus::Active => TaskLifecyclePhase::Running,
        TaskRecordStatus::Done => TaskLifecyclePhase::Completed,
        TaskRecordStatus::Blocked => TaskLifecyclePhase::Blocked,
    };
    item(
        record.id,
        None,
        TaskLifecycleSource::TaskRecord,
        phase,
        TaskEffectState::NoEffect,
        record.title.clone(),
        task_phase_code(phase),
        None,
        record.updated_at,
    )
}

fn agent_run_item(record: &AgentRunRecord) -> TaskLifecycleItem {
    let phase = match record.status {
        AgentRunStatus::Queued => TaskLifecyclePhase::Queued,
        AgentRunStatus::Running | AgentRunStatus::CancelRequested => TaskLifecyclePhase::Running,
        AgentRunStatus::WaitingForPrerequisite => TaskLifecyclePhase::WaitingPrerequisite,
        AgentRunStatus::WaitingForConfirmation => TaskLifecyclePhase::WaitingApproval,
        AgentRunStatus::Blocked => TaskLifecyclePhase::Blocked,
        AgentRunStatus::Completed => TaskLifecyclePhase::Completed,
        AgentRunStatus::Failed => TaskLifecyclePhase::Failed,
        AgentRunStatus::Cancelled => TaskLifecyclePhase::Cancelled,
    };
    let (source, title, effect_state) = match record.expert_contract.as_ref() {
        Some(contract) => {
            let effect = record
                .expert_result
                .as_ref()
                .map(|result| match result.external_effect_state {
                    ExpertExternalEffectState::None => TaskEffectState::NoEffect,
                    ExpertExternalEffectState::VerifiedReadOnly => TaskEffectState::ReadOnly,
                    ExpertExternalEffectState::ManagedStagingOnly => {
                        TaskEffectState::LocalReversible
                    }
                    ExpertExternalEffectState::Uncertain => TaskEffectState::RemoteUncertain,
                })
                .unwrap_or(TaskEffectState::NoEffect);
            (
                TaskLifecycleSource::ExpertAttempt,
                format!("Expert task: {}", contract.key),
                effect,
            )
        }
        None => (
            TaskLifecycleSource::AgentRun,
            "Agent task".to_string(),
            TaskEffectState::NoEffect,
        ),
    };
    item(
        record.id,
        record.parent_run_id,
        source,
        phase,
        effect_state,
        title,
        task_phase_code(phase),
        None,
        record.updated_at,
    )
}

fn automation_run_item(run: &AutomationRun) -> TaskLifecycleItem {
    let phase = match run.status {
        AutomationRunStatus::Queued => TaskLifecyclePhase::Queued,
        AutomationRunStatus::Running => TaskLifecyclePhase::Running,
        AutomationRunStatus::WaitingReview => TaskLifecyclePhase::WaitingReview,
        AutomationRunStatus::WaitingApproval => TaskLifecyclePhase::WaitingApproval,
        AutomationRunStatus::Completed => TaskLifecyclePhase::Completed,
        AutomationRunStatus::Failed => TaskLifecyclePhase::Failed,
        AutomationRunStatus::Cancelled => TaskLifecyclePhase::Cancelled,
    };
    item(
        run.id,
        run.agent_run_id,
        TaskLifecycleSource::AutomationRun,
        phase,
        TaskEffectState::NoEffect,
        "Automatic task".to_string(),
        task_phase_code(phase),
        None,
        run.updated_at,
    )
}

fn review_item(review: &ReviewQueueItem) -> TaskLifecycleItem {
    let phase = match review.status {
        ReviewQueueItemStatus::PendingReview => TaskLifecyclePhase::WaitingReview,
        ReviewQueueItemStatus::PendingApproval => TaskLifecyclePhase::WaitingApproval,
        ReviewQueueItemStatus::Accepted => TaskLifecyclePhase::Completed,
        ReviewQueueItemStatus::Rejected => TaskLifecyclePhase::Cancelled,
    };
    let action =
        (review.status == ReviewQueueItemStatus::PendingReview).then(|| TaskLifecycleAction {
            kind: TaskLifecycleActionKind::ReviewDecision,
            action_revision: review.action_revision(),
        });
    item(
        review.id,
        Some(review.automation_run_id),
        TaskLifecycleSource::Review,
        phase,
        TaskEffectState::NoEffect,
        review.title.clone(),
        task_phase_code(phase),
        action,
        review.updated_at,
    )
}

fn tool_invocation_item(invocation: &ToolInvocationRecord) -> TaskLifecycleItem {
    let phase = match invocation.status {
        ToolExecutionStatus::WaitingForConfirmation => TaskLifecyclePhase::WaitingApproval,
        ToolExecutionStatus::Running => TaskLifecyclePhase::Running,
        ToolExecutionStatus::Succeeded => TaskLifecyclePhase::Completed,
        ToolExecutionStatus::Failed => TaskLifecyclePhase::Failed,
        ToolExecutionStatus::Blocked => TaskLifecyclePhase::Blocked,
    };
    let mutating = capability_may_mutate(invocation.capability);
    let effect_state = match invocation.status {
        ToolExecutionStatus::Running if mutating => TaskEffectState::RemoteUncertain,
        ToolExecutionStatus::Succeeded if mutating => TaskEffectState::LocalApplied,
        ToolExecutionStatus::Succeeded => TaskEffectState::ReadOnly,
        _ => TaskEffectState::NoEffect,
    };
    item(
        invocation.id,
        invocation.run_id,
        TaskLifecycleSource::ToolInvocation,
        phase,
        effect_state,
        format!("Tool: {}", invocation.tool_id),
        task_phase_code(phase),
        None,
        invocation.finished_at.unwrap_or(invocation.created_at),
    )
}

fn connector_invocation_item(invocation: &ConnectorInvocation) -> TaskLifecycleItem {
    let phase = match invocation.status {
        ConnectorInvocationStatus::PendingApproval => TaskLifecyclePhase::WaitingApproval,
        ConnectorInvocationStatus::Running => TaskLifecyclePhase::Running,
        ConnectorInvocationStatus::Succeeded => TaskLifecyclePhase::Completed,
        ConnectorInvocationStatus::Failed => TaskLifecyclePhase::Failed,
        ConnectorInvocationStatus::ReconciliationRequired => TaskLifecyclePhase::EffectUnknown,
    };
    let mutating = invocation.capability.external_mutation();
    let effect_state = match invocation.status {
        ConnectorInvocationStatus::ReconciliationRequired => TaskEffectState::RemoteUncertain,
        ConnectorInvocationStatus::Succeeded if mutating => TaskEffectState::RemoteKnownApplied,
        ConnectorInvocationStatus::Succeeded => TaskEffectState::ReadOnly,
        ConnectorInvocationStatus::Running if mutating => TaskEffectState::RemoteUncertain,
        _ => TaskEffectState::RemoteKnownNotApplied,
    };
    item(
        invocation.id,
        invocation.automation_run_id,
        TaskLifecycleSource::ConnectorInvocation,
        phase,
        effect_state,
        format!("Connected action: {}", invocation.provider_id),
        task_phase_code(phase),
        None,
        invocation.updated_at,
    )
}

fn connector_recovery_item(recovery: &ConnectorRecoveryItem) -> TaskLifecycleItem {
    let phase = match recovery.status {
        ConnectorRecoveryStatus::ReconciliationRequired => TaskLifecyclePhase::EffectUnknown,
        ConnectorRecoveryStatus::RepairRequired | ConnectorRecoveryStatus::NeedsRepair => {
            TaskLifecyclePhase::RepairRequired
        }
        _ => TaskLifecyclePhase::NeedsRecovery,
    };
    let effect_state = match recovery.external_effect_state {
        ConnectorRecoveryExternalEffectState::LocalFilePreserved => {
            TaskEffectState::LocalReversible
        }
        ConnectorRecoveryExternalEffectState::NoExternalWrite => {
            TaskEffectState::RemoteKnownNotApplied
        }
        ConnectorRecoveryExternalEffectState::LocalCredentialRemovalPending => {
            TaskEffectState::CompensationRequired
        }
        ConnectorRecoveryExternalEffectState::ExternalResultUncertain => {
            TaskEffectState::RemoteUncertain
        }
    };
    let action = recovery.action.as_ref().map(|action| {
        let (kind, action_revision) = match action {
            ConnectorRecoveryAction::RetryAttachmentCleanup { action_revision } => (
                TaskLifecycleActionKind::RetryLocalCleanup,
                action_revision.clone(),
            ),
            ConnectorRecoveryAction::ResumeSync { action_revision } => {
                (TaskLifecycleActionKind::ResumeSync, action_revision.clone())
            }
            ConnectorRecoveryAction::InspectExternalResult { action_revision } => (
                TaskLifecycleActionKind::InspectExternalResult,
                action_revision.clone(),
            ),
        };
        TaskLifecycleAction {
            kind,
            action_revision,
        }
    });
    item(
        recovery.id,
        None,
        TaskLifecycleSource::ConnectorRecovery,
        phase,
        effect_state,
        recovery.title.clone(),
        task_phase_code(phase),
        action,
        recovery.updated_at,
    )
}

fn artifact_item(artifact: &ArtifactDeliveryView) -> TaskLifecycleItem {
    let phase = match artifact.phase {
        ArtifactPhase::Generated
        | ArtifactPhase::StructureChecked
        | ArtifactPhase::VisualChecked
        | ArtifactPhase::RevisionPrepared => TaskLifecyclePhase::Running,
        ArtifactPhase::RevisionRequired => TaskLifecyclePhase::WaitingReview,
        ArtifactPhase::ReadyForDelivery => TaskLifecyclePhase::WaitingReview,
        ArtifactPhase::Completed => TaskLifecyclePhase::Completed,
        ArtifactPhase::Failed => TaskLifecyclePhase::Failed,
    };
    item(
        artifact.id,
        None,
        TaskLifecycleSource::Artifact,
        phase,
        TaskEffectState::LocalApplied,
        "Generated document".to_string(),
        artifact.status_code.clone(),
        None,
        artifact.updated_at,
    )
}

fn computer_use_item(
    session: &ComputerUseSession,
    step: Option<&ComputerUseStep>,
) -> TaskLifecycleItem {
    let Some(step) = step else {
        return item(
            session.id,
            session.run_id,
            TaskLifecycleSource::ComputerUse,
            TaskLifecyclePhase::Queued,
            TaskEffectState::NoEffect,
            "Computer action".to_string(),
            "observation_pending".to_string(),
            None,
            session.updated_at,
        );
    };
    let phase = match step.status {
        ComputerUseStepStatus::Observed | ComputerUseStepStatus::Ready => {
            TaskLifecyclePhase::Running
        }
        ComputerUseStepStatus::AwaitingApproval => TaskLifecyclePhase::WaitingApproval,
        ComputerUseStepStatus::ActionStarted | ComputerUseStepStatus::AwaitingVerification => {
            TaskLifecyclePhase::EffectUnknown
        }
        ComputerUseStepStatus::Verified => TaskLifecyclePhase::Completed,
        ComputerUseStepStatus::NeedsReplan | ComputerUseStepStatus::VerificationFailed => {
            TaskLifecyclePhase::NeedsRecovery
        }
        ComputerUseStepStatus::UserTakenOver | ComputerUseStepStatus::Cancelled => {
            TaskLifecyclePhase::Cancelled
        }
        ComputerUseStepStatus::EffectUnknown => TaskLifecyclePhase::EffectUnknown,
    };
    let effect_state = match step.status {
        ComputerUseStepStatus::ActionStarted
        | ComputerUseStepStatus::AwaitingVerification
        | ComputerUseStepStatus::EffectUnknown => TaskEffectState::RemoteUncertain,
        ComputerUseStepStatus::Verified => match step.checkpoint.undo_capability {
            ComputerUseUndoCapability::None => TaskEffectState::RemoteKnownApplied,
            ComputerUseUndoCapability::CompensationRequired => {
                TaskEffectState::CompensationRequired
            }
        },
        _ => TaskEffectState::NoEffect,
    };
    item(
        step.id,
        Some(session.id),
        TaskLifecycleSource::ComputerUse,
        phase,
        effect_state,
        "Computer action".to_string(),
        task_phase_code(phase),
        None,
        step.updated_at,
    )
}

fn workspace_checkpoint_item(checkpoint: &WorkspaceUndoView) -> TaskLifecycleItem {
    let phase = match checkpoint.status {
        WorkspaceMutationCheckpointStatus::Intent | WorkspaceMutationCheckpointStatus::Prepared => {
            TaskLifecyclePhase::Running
        }
        WorkspaceMutationCheckpointStatus::EffectStarted
        | WorkspaceMutationCheckpointStatus::UndoStarted => TaskLifecyclePhase::EffectUnknown,
        WorkspaceMutationCheckpointStatus::Ready
        | WorkspaceMutationCheckpointStatus::NotUndoable
        | WorkspaceMutationCheckpointStatus::Undone => TaskLifecyclePhase::Completed,
        WorkspaceMutationCheckpointStatus::Failed => TaskLifecyclePhase::Failed,
        WorkspaceMutationCheckpointStatus::RepairRequired => TaskLifecyclePhase::RepairRequired,
    };
    let effect_state = match checkpoint.effect_state {
        WorkspaceCheckpointEffectState::NoEffect => TaskEffectState::NoEffect,
        WorkspaceCheckpointEffectState::KnownApplied if checkpoint.undo_available => {
            TaskEffectState::LocalReversible
        }
        WorkspaceCheckpointEffectState::KnownApplied => TaskEffectState::LocalApplied,
        WorkspaceCheckpointEffectState::EffectUnknown => TaskEffectState::LocalUncertain,
    };
    let action = checkpoint
        .action_revision
        .as_ref()
        .map(|action_revision| TaskLifecycleAction {
            kind: TaskLifecycleActionKind::UndoLocalChange,
            action_revision: action_revision.clone(),
        });
    item(
        checkpoint.id,
        checkpoint.run_id,
        TaskLifecycleSource::WorkspaceCheckpoint,
        phase,
        effect_state,
        "Local file change".to_string(),
        checkpoint.title_code.clone(),
        action,
        checkpoint.updated_at,
    )
}

#[allow(clippy::too_many_arguments)]
fn item(
    id: Uuid,
    parent_id: Option<Uuid>,
    source: TaskLifecycleSource,
    phase: TaskLifecyclePhase,
    effect_state: TaskEffectState,
    title: String,
    detail_code: String,
    action: Option<TaskLifecycleAction>,
    updated_at: DateTime<Utc>,
) -> TaskLifecycleItem {
    TaskLifecycleItem {
        id,
        parent_id,
        source,
        phase,
        effect_state,
        title,
        detail_code,
        action,
        updated_at,
    }
}

fn task_phase_code(phase: TaskLifecyclePhase) -> String {
    match phase {
        TaskLifecyclePhase::Queued => "queued",
        TaskLifecyclePhase::Running => "running",
        TaskLifecyclePhase::WaitingPrerequisite => "waiting_prerequisite",
        TaskLifecyclePhase::WaitingReview => "waiting_review",
        TaskLifecyclePhase::WaitingApproval => "waiting_approval",
        TaskLifecyclePhase::NeedsRecovery => "needs_recovery",
        TaskLifecyclePhase::EffectUnknown => "effect_unknown",
        TaskLifecyclePhase::RepairRequired => "repair_required",
        TaskLifecyclePhase::Blocked => "blocked",
        TaskLifecyclePhase::Completed => "completed",
        TaskLifecyclePhase::Failed => "failed",
        TaskLifecyclePhase::Cancelled => "cancelled",
    }
    .to_string()
}

fn capability_may_mutate(capability: CapabilityKind) -> bool {
    matches!(
        capability,
        CapabilityKind::FileWrite
            | CapabilityKind::BrowserSubmit
            | CapabilityKind::EmailDraft
            | CapabilityKind::EmailSend
            | CapabilityKind::ConnectorWrite
            | CapabilityKind::DriveWrite
            | CapabilityKind::TerminalWrite
            | CapabilityKind::ComputerControl
            | CapabilityKind::AppUpdateDownload
            | CapabilityKind::AppUpdateInstall
    )
}

#[cfg(test)]
mod tests {
    use super::{TaskLifecyclePhase, TaskLifecycleSource};
    use crate::kernel::agent_run::{AgentRunStart, AgentRunStatus};
    use crate::kernel::automation::{AutomationDefinition, ReviewQueueItem, ReviewQueueItemStatus};
    use crate::kernel::event_store::EventStore;
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    #[test]
    fn lifecycle_snapshot_unifies_agent_automation_and_exact_review_action() {
        let store = EventStore::open_memory().unwrap();
        let agent =
            AgentRunStart::queued("conversation".to_string(), "private prompt".to_string(), 0)
                .unwrap();
        store.append_agent_run_start(&agent).unwrap();
        let definition = AutomationDefinition::once(
            "Prepare a draft".to_string(),
            "Asia/Shanghai".to_string(),
            Utc::now() - Duration::minutes(1),
        )
        .unwrap();
        store.upsert_automation_definition(&definition).unwrap();
        let run = store
            .claim_due_automation_run(definition.id, Utc::now(), "worker".to_string())
            .unwrap()
            .unwrap();
        let now = Utc::now();
        let review = ReviewQueueItem {
            id: Uuid::new_v4(),
            automation_run_id: run.id,
            agent_run_id: Some(agent.id),
            tool_invocation_id: None,
            status: ReviewQueueItemStatus::PendingReview,
            preview_fingerprint: Some("sha256:preview".to_string()),
            revision: 0,
            title: "Review draft".to_string(),
            evidence_ref: None,
            created_at: now,
            updated_at: now,
        };
        store.upsert_review_queue_item(&review).unwrap();

        let snapshot = store.task_lifecycle_snapshot().unwrap();
        assert!(snapshot.items.iter().any(|item| {
            item.id == agent.id
                && item.source == TaskLifecycleSource::AgentRun
                && item.phase == TaskLifecyclePhase::Queued
                && item.title == "Agent task"
        }));
        let projected_review = snapshot
            .items
            .iter()
            .find(|item| item.id == review.id)
            .unwrap();
        assert_eq!(projected_review.source, TaskLifecycleSource::Review);
        assert_eq!(projected_review.phase, TaskLifecyclePhase::WaitingReview);
        assert_eq!(
            projected_review.action.as_ref().unwrap().action_revision,
            review.action_revision()
        );
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("private prompt"));
        assert_eq!(agent.initial_status, AgentRunStatus::Queued);
    }
}
