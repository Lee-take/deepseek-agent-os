#![allow(dead_code)]

use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection};
use thiserror::Error;
use uuid::Uuid;

use crate::kernel::agent_context::AgentContextReceipt;
use crate::kernel::agent_run::{
    AgentRunArtifactRecord, AgentRunCancelRequest, AgentRunClaim, AgentRunFinish,
    AgentRunQueuedGuidance, AgentRunRecord, AgentRunStart, AgentRunStatus, AgentRunStepRecord,
};
use crate::kernel::capability::CapabilityInvocation;
use crate::kernel::deepseek::DeepSeekChatTelemetry;
use crate::kernel::models::{
    KernelEvent, MemoryCandidate, MemoryCandidateMergePreview, MemoryCandidateRecord,
    MemoryCandidateReplacePreview, MemoryCandidateResolution, MemoryCandidateSource,
    MemoryCandidateStatus, MemoryConflictSummary, MemoryMaintenanceActionKind,
    MemoryMaintenanceReviewAction, MemoryRecord, MemoryRecordDeletion, MemoryRecordLink,
    MemoryRecordLinkSummary, MemoryRecordUpdate, MemoryRelationKind, MemorySearchMatch,
    MemorySearchMatchSource, MemorySelectedFeedback, MemorySelectedFeedbackKind, TaskRecord,
};
use crate::kernel::policy::{
    capability_risk, CapabilityAccessRecord, CapabilityAccessRequest, CapabilityAccessStatus,
    CapabilityGrantState, CapabilityKind, PermissionAuditEntry, PermissionResolution, RiskLevel,
};
use crate::kernel::skill::{
    SkillEnablementChange, SkillEnablementStatus, SkillExecutionRecord, SkillInstallationRecord,
    SkillRecord, SkillTrustLevel, SkillTrustReset, SkillUninstallRecord,
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
pub const AGENT_RUN_FINISHED_EVENT: &str = "agent_run.finished";
pub const AGENT_RUN_GUIDANCE_QUEUED_EVENT: &str = "agent_run.guidance_queued";
pub const AGENT_RUN_STARTED_EVENT: &str = "agent_run.started";
pub const AGENT_RUN_STEP_RECORDED_EVENT: &str = "agent_run.step_recorded";
pub const AGENT_RUN_ARTIFACT_RECORDED_EVENT: &str = "agent_run.artifact_recorded";
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
pub const TASK_RECORD_CREATED_EVENT: &str = "task_record.created";
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

pub struct EventStore {
    conn: Connection,
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
            "#,
        )?;
        Ok(())
    }

    pub fn append(&self, event: &KernelEvent) -> EventStoreResult<()> {
        self.conn.execute(
            r#"
            INSERT INTO kernel_events (id, event_type, payload_json, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![
                event.id.to_string(),
                event.event_type,
                event.payload_json,
                event.created_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
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
        let existing = self
            .list_agent_run_starts()?
            .into_iter()
            .any(|record| record.id == start.id);
        if existing {
            return Ok(false);
        }

        let event = KernelEvent::new(AGENT_RUN_STARTED_EVENT, start)?;
        self.append(&event)?;
        Ok(true)
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
        let Some(next_run) = self
            .list_agent_run_records()?
            .into_iter()
            .filter(|record| record.status == AgentRunStatus::Queued)
            .min_by_key(|record| record.started_at)
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
        let target = self
            .list_agent_run_records()?
            .into_iter()
            .find(|record| record.id == run_id)
            .ok_or_else(|| EventStoreError::InvalidState("agent run does not exist".to_string()))?;
        if target.status != AgentRunStatus::Queued || target.cancel_requested {
            return Err(EventStoreError::InvalidState(format!(
                "agent run {} cannot be claimed from status {:?}",
                target.id, target.status
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
        let event = KernelEvent::new(AGENT_RUN_CLAIMED_EVENT, claim)?;
        self.append(&event)
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

    pub fn append_agent_run_queued_guidance(
        &self,
        guidance: &AgentRunQueuedGuidance,
    ) -> EventStoreResult<()> {
        self.ensure_agent_run_exists(guidance.run_id)?;
        let event = KernelEvent::new(AGENT_RUN_GUIDANCE_QUEUED_EVENT, guidance)?;
        self.append(&event)
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

    pub fn list_agent_run_records(&self) -> EventStoreResult<Vec<AgentRunRecord>> {
        let mut guidance_by_run_id =
            std::collections::HashMap::<Uuid, Vec<AgentRunQueuedGuidance>>::new();
        for guidance in self.list_agent_run_guidance()? {
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
                let latest_finish = finish_by_run_id.remove(&start.id);
                let steps = steps_by_run_id.remove(&start.id).unwrap_or_default();
                let artifacts = artifacts_by_run_id.remove(&start.id).unwrap_or_default();
                let mut updated_at = start.started_at;
                let mut status = latest_finish
                    .as_ref()
                    .map(|finish| finish.status)
                    .unwrap_or_else(|| {
                        if latest_claim.is_some() {
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

                if let Some(finished_at) = finished_at {
                    updated_at = updated_at.max(finished_at);
                }
                for guidance in &queued_guidance {
                    updated_at = updated_at.max(guidance.queued_at);
                }
                for step in &steps {
                    updated_at = updated_at.max(step.recorded_at);
                }
                for artifact in &artifacts {
                    updated_at = updated_at.max(artifact.created_at);
                }
                let worker_id = latest_claim.as_ref().map(|claim| claim.worker_id.clone());
                let lease_expires_at = latest_claim.as_ref().map(|claim| claim.lease_expires_at);
                if let Some(claim) = &latest_claim {
                    updated_at = updated_at.max(claim.claimed_at);
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
                    }
                }

                Ok(AgentRunRecord {
                    id: start.id,
                    conversation_id: start.conversation_id,
                    prompt: start.prompt,
                    attachment_count: start.attachment_count,
                    status,
                    worker_id,
                    lease_expires_at,
                    queued_guidance,
                    steps,
                    artifacts,
                    cancel_requested,
                    cancel_reason,
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
        let existing = self.list_skill_installations()?.into_iter().any(|record| {
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
                let mut manifest = installation.manifest;
                let mut enablement_status = latest_change
                    .as_ref()
                    .map(|change| change.status)
                    .unwrap_or(SkillEnablementStatus::Enabled);
                let mut last_audit_note = latest_change.as_ref().map(|change| change.note.clone());
                let mut updated_at = latest_change
                    .as_ref()
                    .map(|change| change.changed_at)
                    .unwrap_or(installation.installed_at);
                if let Some(reset) = latest_reset {
                    manifest.trust_level = SkillTrustLevel::Untrusted;
                    enablement_status = SkillEnablementStatus::Disabled;
                    last_audit_note = Some(reset.note);
                    updated_at = updated_at.max(reset.reset_at);
                }

                Ok(SkillRecord {
                    id: installation.id,
                    manifest,
                    installed_from: installation.installed_from,
                    installed_at: installation.installed_at,
                    enablement_status,
                    last_audit_note,
                    updated_at,
                })
            })
            .collect()
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
        let events = self.list_by_type(CAPABILITY_ACCESS_REQUESTED_EVENT, 200)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<CapabilityAccessRequest>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn append_permission_resolution(
        &self,
        resolution: &PermissionResolution,
    ) -> EventStoreResult<()> {
        let event = KernelEvent::new(PERMISSION_RESOLUTION_RECORDED_EVENT, resolution)?;
        self.append(&event)
    }

    pub fn list_permission_resolutions(&self) -> EventStoreResult<Vec<PermissionResolution>> {
        let events = self.list_by_type(PERMISSION_RESOLUTION_RECORDED_EVENT, 200)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<PermissionResolution>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
    }

    pub fn list_capability_access_records(&self) -> EventStoreResult<Vec<CapabilityAccessRecord>> {
        let mut latest_resolution_by_request_id = std::collections::HashMap::new();
        for resolution in self.list_permission_resolutions()? {
            latest_resolution_by_request_id
                .entry(resolution.request_id)
                .or_insert(resolution);
        }
        let invocations = self.list_capability_invocations()?;

        self.list_capability_access_requests()?
            .into_iter()
            .map(|request| {
                let resolution = latest_resolution_by_request_id
                    .remove(&request.id)
                    .map(|resolution| resolution.to_owned());
                let effective_status = match &resolution {
                    Some(resolution) if resolution.approved => CapabilityAccessStatus::Approved,
                    Some(_) => CapabilityAccessStatus::Rejected,
                    None => request.status,
                };
                let grant_state = capability_grant_state(
                    &request,
                    resolution.as_ref(),
                    effective_status,
                    &invocations,
                );

                Ok(CapabilityAccessRecord {
                    request,
                    resolution,
                    effective_status,
                    grant_state,
                })
            })
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
        let record = self
            .list_capability_access_records()?
            .into_iter()
            .find(|record| record.request.id == request_id)
            .ok_or_else(|| {
                EventStoreError::NotFound("capability access request does not exist".to_string())
            })?;

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

        let resolution = PermissionResolution::new(request_id, approved, note);
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
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<CapabilityInvocation>(&event.payload_json)
                    .map_err(Into::into)
            })
            .collect()
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
) -> CapabilityGrantState {
    if effective_status != CapabilityAccessStatus::Approved {
        return CapabilityGrantState::NotGranted;
    }

    if capability_risk(request.capability) != RiskLevel::Critical {
        return CapabilityGrantState::Reusable;
    }

    let Some(resolution) = resolution else {
        return CapabilityGrantState::NotGranted;
    };

    let consumed = invocations.iter().any(|invocation| {
        if invocation.capability != request.capability
            || invocation.created_at < resolution.created_at
        {
            return false;
        }

        match invocation.approval_request_id {
            Some(approval_request_id) => approval_request_id == request.id,
            None => true,
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
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    use super::{EventStore, EventStoreError, MEMORY_RECORD_LINKED_EVENT};
    use crate::kernel::agent_context::AgentContextReceipt;
    use crate::kernel::agent_run::{
        AgentRunArtifactRecord, AgentRunCancelRequest, AgentRunFinish, AgentRunQueuedGuidance,
        AgentRunStart, AgentRunStatus, AgentRunStepRecord, AgentRunStepStatus,
    };
    use crate::kernel::capability::{CapabilityInvocation, CapabilityInvocationStatus};
    use crate::kernel::deepseek::{DeepSeekChatCacheStatus, DeepSeekChatTelemetry};
    use crate::kernel::models::{AccessMode, FoundationState};
    use crate::kernel::models::{
        KernelEvent, MemoryCandidate, MemoryCandidateSource, MemoryCandidateStatus,
        MemoryLifecycle, MemoryRecord, MemoryRecordLink, MemoryRecordLinkSummary,
        MemoryRecordSource, MemoryRelationKind, MemoryScope, MemorySearchMatch,
        MemorySearchMatchSource, MemorySelectedFeedbackKind, MemorySensitivity, MemoryType,
        TaskRecord,
    };
    use crate::kernel::policy::{
        request_capability_access, CapabilityAccessStatus, CapabilityGrantState, CapabilityKind,
        PermissionAuditEntry, PolicyDecision,
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
                approval_request_id: None,
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
}
