use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use super::{EventStore, EventStoreError, EventStoreResult};
use crate::kernel::agent_run::AgentRunResourceAccess;
use crate::kernel::goal_continuation::{
    identity_fingerprint, ContextArtifactIdentity, ContextAuthorizationIdentity, ContextCheckpoint,
    ContextCheckpointSeed, ContextResourceIdentity, ContextSourceIdentity,
    GoalContinuationObservation, CONTEXT_CHECKPOINT_VERSION,
};
use crate::kernel::goal_lifecycle::completion_projection;
use crate::kernel::models::KernelEvent;
use crate::kernel::tool_runtime::{ToolExecutionStatus, ToolInvocationRecord};

pub(super) const CONTEXT_CHECKPOINT_RECORDED_EVENT: &str = "goal_context_checkpoint.recorded";

pub(super) fn migrate(store: &EventStore) -> EventStoreResult<()> {
    store.conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS goal_context_checkpoints (
            run_id TEXT PRIMARY KEY NOT NULL,
            schema_version TEXT NOT NULL,
            goal_revision TEXT NOT NULL,
            frozen_fingerprint TEXT NOT NULL,
            status TEXT NOT NULL,
            checkpoint_fingerprint TEXT NOT NULL,
            checkpoint_json TEXT NOT NULL,
            row_revision INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_goal_context_checkpoint_status
            ON goal_context_checkpoints (status, updated_at);
        "#,
    )?;
    validate_all_rows(store)
}

impl EventStore {
    pub(crate) fn record_goal_context_checkpoint(
        &self,
        run_id: Uuid,
        observation: GoalContinuationObservation,
    ) -> EventStoreResult<Option<ContextCheckpoint>> {
        let Some(lifecycle) = self.goal_envelope_projection(run_id)? else {
            return Ok(None);
        };
        let Some(goal) = lifecycle.frozen().cloned() else {
            return Ok(None);
        };
        let completion = self
            .goal_completion_projection(run_id)?
            .unwrap_or(completion_projection(&lifecycle, &[]).map_err(invalid)?);
        let previous = context_checkpoint_from_connection(&self.conn, run_id)?;
        if let Some(previous) = previous.as_ref() {
            previous
                .validate_against_goal(&lifecycle)
                .map_err(invalid)?;
        }

        let invocations = self
            .list_tool_invocations()?
            .into_iter()
            .filter(|invocation| invocation.run_id == Some(run_id))
            .collect::<Vec<_>>();
        let seed = ContextCheckpointSeed {
            run_id,
            goal,
            completion,
            authorizations: authorization_identities(self, run_id)?,
            resources: resource_identities(self, run_id)?,
            artifacts: artifact_identities(self, run_id)?,
            sources: source_identities(&invocations)?,
        };
        let checkpoint =
            ContextCheckpoint::advance(previous.as_ref(), seed, observation).map_err(invalid)?;
        if previous
            .as_ref()
            .is_some_and(|previous| previous.fingerprint == checkpoint.fingerprint)
        {
            return Ok(previous);
        }
        persist_checkpoint(self, previous.as_ref(), &checkpoint)?;
        Ok(Some(checkpoint))
    }

    pub fn goal_context_checkpoint(
        &self,
        run_id: Uuid,
    ) -> EventStoreResult<Option<ContextCheckpoint>> {
        let checkpoint = context_checkpoint_from_connection(&self.conn, run_id)?;
        let Some(checkpoint) = checkpoint else {
            return Ok(None);
        };
        let lifecycle = self
            .goal_envelope_projection(run_id)?
            .ok_or_else(|| invalid("context_checkpoint_goal_missing"))?;
        checkpoint
            .validate_against_goal(&lifecycle)
            .map_err(invalid)?;
        Ok(Some(checkpoint))
    }

    pub(crate) fn goal_context_checkpoint_prompt(
        &self,
        run_id: Uuid,
    ) -> EventStoreResult<Option<String>> {
        self.goal_context_checkpoint(run_id)?
            .map(|checkpoint| checkpoint.advisory_prompt().map_err(invalid))
            .transpose()
    }
}

pub(super) fn blocker_reason_from_connection(
    connection: &Connection,
    run_id: Uuid,
) -> EventStoreResult<Option<String>> {
    let Some(checkpoint) = context_checkpoint_from_connection(connection, run_id)? else {
        return Ok(None);
    };
    let Some(lifecycle) = EventStore::goal_envelope_projection_from_connection(connection, run_id)?
    else {
        return Err(invalid("context_checkpoint_goal_missing"));
    };
    checkpoint
        .validate_against_goal(&lifecycle)
        .map_err(invalid)?;
    Ok(checkpoint.blocker_reason())
}

fn persist_checkpoint(
    store: &EventStore,
    previous: Option<&ContextCheckpoint>,
    checkpoint: &ContextCheckpoint,
) -> EventStoreResult<()> {
    checkpoint.validate().map_err(invalid)?;
    let json = serde_json::to_string(checkpoint)?;
    let transaction = store.conn.unchecked_transaction()?;
    match previous {
        Some(previous) => {
            let row_revision = transaction.query_row(
                "SELECT row_revision FROM goal_context_checkpoints WHERE run_id = ?1",
                params![checkpoint.run_id.to_string()],
                |row| row.get::<_, u64>(0),
            )?;
            let changed = transaction.execute(
                r#"UPDATE goal_context_checkpoints
                   SET schema_version = ?2, goal_revision = ?3,
                       frozen_fingerprint = ?4, status = ?5,
                       checkpoint_fingerprint = ?6, checkpoint_json = ?7,
                       row_revision = row_revision + 1, updated_at = ?8
                   WHERE run_id = ?1 AND row_revision = ?9
                     AND checkpoint_fingerprint = ?10"#,
                params![
                    checkpoint.run_id.to_string(),
                    CONTEXT_CHECKPOINT_VERSION,
                    checkpoint.goal.revision,
                    checkpoint.goal.fingerprint,
                    checkpoint_status(checkpoint),
                    checkpoint.fingerprint,
                    json,
                    timestamp(checkpoint.updated_at),
                    row_revision,
                    previous.fingerprint,
                ],
            )?;
            if changed != 1 {
                return Err(invalid("context_checkpoint_changed_concurrently"));
            }
        }
        None => {
            transaction.execute(
                r#"INSERT INTO goal_context_checkpoints
                   (run_id, schema_version, goal_revision, frozen_fingerprint,
                    status, checkpoint_fingerprint, checkpoint_json, row_revision,
                    created_at, updated_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?9)"#,
                params![
                    checkpoint.run_id.to_string(),
                    CONTEXT_CHECKPOINT_VERSION,
                    checkpoint.goal.revision,
                    checkpoint.goal.fingerprint,
                    checkpoint_status(checkpoint),
                    checkpoint.fingerprint,
                    json,
                    timestamp(checkpoint.created_at),
                    timestamp(checkpoint.updated_at),
                ],
            )?;
        }
    }
    let event = KernelEvent::new(CONTEXT_CHECKPOINT_RECORDED_EVENT, checkpoint)?;
    EventStore::insert_kernel_event(&transaction, &event)?;
    transaction.commit()?;
    Ok(())
}

type CheckpointRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    u64,
    String,
    String,
);

fn context_checkpoint_from_connection(
    connection: &Connection,
    run_id: Uuid,
) -> EventStoreResult<Option<ContextCheckpoint>> {
    let row = connection
        .query_row(
            r#"SELECT schema_version, goal_revision, frozen_fingerprint,
                      status, checkpoint_fingerprint, checkpoint_json,
                      row_revision, created_at, updated_at
               FROM goal_context_checkpoints WHERE run_id = ?1"#,
            params![run_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .optional()?;
    row.map(|row| decode_checkpoint(run_id, row)).transpose()
}

fn decode_checkpoint(run_id: Uuid, row: CheckpointRow) -> EventStoreResult<ContextCheckpoint> {
    let (
        schema_version,
        goal_revision,
        frozen_fingerprint,
        status,
        checkpoint_fingerprint,
        checkpoint_json,
        _row_revision,
        created_at,
        updated_at,
    ) = row;
    let checkpoint: ContextCheckpoint = serde_json::from_str(&checkpoint_json)?;
    checkpoint.validate().map_err(invalid)?;
    if checkpoint.run_id != run_id
        || schema_version != CONTEXT_CHECKPOINT_VERSION
        || checkpoint.version != schema_version
        || checkpoint.goal.revision != goal_revision
        || checkpoint.goal.fingerprint != frozen_fingerprint
        || checkpoint_status(&checkpoint) != status
        || checkpoint.fingerprint != checkpoint_fingerprint
        || timestamp(checkpoint.created_at) != created_at
        || timestamp(checkpoint.updated_at) != updated_at
    {
        return Err(invalid("context_checkpoint_projection_columns_drifted"));
    }
    Ok(checkpoint)
}

fn validate_all_rows(store: &EventStore) -> EventStoreResult<()> {
    let run_ids = {
        let mut statement = store
            .conn
            .prepare("SELECT run_id FROM goal_context_checkpoints ORDER BY run_id")?;
        let run_ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        run_ids
    };
    for run_id in run_ids {
        let run_id = Uuid::parse_str(&run_id)?;
        context_checkpoint_from_connection(&store.conn, run_id)?
            .ok_or_else(|| invalid("context_checkpoint_migration_lost_projection"))?;
    }
    Ok(())
}

fn authorization_identities(
    store: &EventStore,
    run_id: Uuid,
) -> EventStoreResult<Vec<ContextAuthorizationIdentity>> {
    let group_ids = {
        let mut statement = store.conn.prepare(
            "SELECT group_id FROM task_grouped_approval_state WHERE task_id = ?1 ORDER BY group_id",
        )?;
        let group_ids = statement
            .query_map(params![run_id.to_string()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        group_ids
    };
    group_ids
        .into_iter()
        .map(|group_id| {
            let group_id = Uuid::parse_str(&group_id)?;
            let group = store
                .task_grouped_approval(group_id)?
                .ok_or_else(|| invalid("context_checkpoint_authorization_missing"))?;
            Ok(ContextAuthorizationIdentity {
                group_id: group.id,
                task_id: group.task_id,
                projection_revision: group.projection_revision,
                manifest_revision: group.manifest.revision.clone(),
                manifest_fingerprint: group.manifest.fingerprint.clone(),
                preview_hash: group.preview.preview_hash.clone(),
                status: group.status.as_str().to_string(),
                capability_request_fingerprints: group
                    .capability_audits
                    .iter()
                    .map(|item| item.request_fingerprint.clone())
                    .collect(),
            })
        })
        .collect()
}

fn resource_identities(
    store: &EventStore,
    run_id: Uuid,
) -> EventStoreResult<Vec<ContextResourceIdentity>> {
    store
        .list_active_agent_run_resource_claims()?
        .into_iter()
        .filter(|claim| claim.run_id == Some(run_id))
        .map(|claim| {
            Ok(ContextResourceIdentity {
                claim_id: claim.id,
                tool_invocation_id: claim.tool_invocation_id,
                access: match claim.access {
                    AgentRunResourceAccess::Read => "read",
                    AgentRunResourceAccess::Write => "write",
                }
                .to_string(),
                resource_key_fingerprint: identity_fingerprint(claim.resource_key.as_bytes()),
                lease_expires_at: claim.lease_expires_at,
            })
        })
        .collect()
}

fn artifact_identities(
    store: &EventStore,
    run_id: Uuid,
) -> EventStoreResult<Vec<ContextArtifactIdentity>> {
    let mut artifacts = store
        .list_agent_run_records()?
        .into_iter()
        .find(|record| record.id == run_id)
        .into_iter()
        .flat_map(|record| record.artifacts)
        .map(|artifact| {
            let canonical =
                serde_json::to_vec(&(artifact.id, &artifact.kind, &artifact.title, &artifact.path))
                    .map_err(EventStoreError::Json)?;
            Ok(ContextArtifactIdentity {
                artifact_id: artifact.id.simple().to_string(),
                kind: safe_identity_code(&artifact.kind, "agent_artifact"),
                identity_fingerprint: identity_fingerprint(&canonical),
            })
        })
        .collect::<EventStoreResult<Vec<_>>>()?;
    if let Some(completion) = store.goal_completion_projection(run_id)? {
        for evidence in completion.evidence {
            for artifact_id in evidence.artifact_ids {
                artifacts.push(ContextArtifactIdentity {
                    artifact_id: safe_identity_code(&artifact_id, "goal_artifact"),
                    kind: "goal_evidence".to_string(),
                    identity_fingerprint: evidence.source_fingerprint.clone(),
                });
            }
        }
    }
    Ok(artifacts)
}

fn source_identities(
    invocations: &[ToolInvocationRecord],
) -> EventStoreResult<Vec<ContextSourceIdentity>> {
    invocations
        .iter()
        .filter(|invocation| terminal_status(invocation.status))
        .map(|invocation| {
            let canonical = serde_json::to_vec(&(
                invocation.id,
                &invocation.output,
                &invocation.evidence,
                invocation.verification.passed,
            ))?;
            Ok(ContextSourceIdentity {
                invocation_id: invocation.id,
                tool_id: invocation.tool_id.clone(),
                tool_version: invocation.tool_version.clone(),
                request_fingerprint: invocation.request_fingerprint.clone(),
                source_fingerprint: identity_fingerprint(&canonical),
            })
        })
        .collect()
}

fn terminal_status(status: ToolExecutionStatus) -> bool {
    matches!(
        status,
        ToolExecutionStatus::Succeeded | ToolExecutionStatus::Failed | ToolExecutionStatus::Blocked
    )
}

fn checkpoint_status(checkpoint: &ContextCheckpoint) -> &'static str {
    match checkpoint.status {
        crate::kernel::goal_continuation::ContextCheckpointStatus::Continue => "continue",
        crate::kernel::goal_continuation::ContextCheckpointStatus::Complete => "complete",
        crate::kernel::goal_continuation::ContextCheckpointStatus::Blocked => "blocked",
    }
}

fn safe_identity_code(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        value.to_string()
    } else {
        fallback.to_string()
    }
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn invalid(message: impl Into<String>) -> EventStoreError {
    EventStoreError::InvalidState(message.into())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use rusqlite::Connection;
    use serde_json::json;

    use super::*;
    use crate::kernel::agent_run::AgentRunStart;
    use crate::kernel::goal_continuation::{
        GoalContinuationBlockerCode, GoalContinuationObservationStage, GoalModelUsage,
        GoalToolUsage,
    };
    use crate::kernel::goal_envelope::GOAL_ENVELOPE_PROPOSAL_VERSION;
    use crate::kernel::goal_lifecycle::{GoalTargetBindingKind, GoalValidationContext};
    use crate::kernel::local_directory::WorkspaceReadinessCode;
    use crate::kernel::models::AccessMode;
    use crate::kernel::tool_runtime::FILE_READ_TOOL_ID;

    fn append_frozen_goal(store: &EventStore) -> Uuid {
        let run = AgentRunStart::new(
            "g1b-context".to_string(),
            "Create one verified brief.".to_string(),
            0,
        )
        .expect("run builds");
        store.append_agent_run_start(&run).expect("run appends");
        let proposal = crate::kernel::goal_envelope::GoalEnvelopeProposal::parse_value(json!({
            "version": GOAL_ENVELOPE_PROPOSAL_VERSION,
            "user_goal": "Create one verified brief.",
            "assumptions": [],
            "constraints": ["Stay inside the selected workspace."],
            "done_when": [{"done_when_id":"brief-ready","description":"The brief is verified."}],
            "required_artifacts": [{"artifact_id":"brief","description":"The brief."}],
            "verifiers": [{"verifier_id":"brief-verifier","done_when_id":"brief-ready","description":"Verify the brief.","evidence_kind":"brief-evidence"}],
            "proposed_capabilities": [FILE_READ_TOOL_ID],
            "external_targets": [{"target_id":"selected-workspace","description":"Bound locally."}],
            "stop_conditions": ["Stop without evidence."]
        }))
        .expect("proposal parses");
        let context =
            GoalValidationContext::new(AccessMode::FullAccess, WorkspaceReadinessCode::Ready)
                .with_enabled_tool(FILE_READ_TOOL_ID, true)
                .with_verifier_kind("brief-evidence")
                .with_target_binding(
                    "selected-workspace",
                    GoalTargetBindingKind::Workspace,
                    b"bounded-workspace-identity",
                );
        let validated = store
            .submit_goal_proposal(run.id, &proposal, &context)
            .expect("goal validates");
        store
            .freeze_goal_envelope(run.id, validated.revision().expect("revision"))
            .expect("goal freezes");
        run.id
    }

    fn observation(
        stage: GoalContinuationObservationStage,
        request_id: Uuid,
        observed_at: DateTime<Utc>,
    ) -> GoalContinuationObservation {
        GoalContinuationObservation {
            stage,
            local_tool_round: u32::from(stage == GoalContinuationObservationStage::AfterToolRound),
            model_usage: vec![GoalModelUsage {
                request_id,
                elapsed_ms: 10,
                total_tokens: Some(20),
                estimated_cost_micro_usd: Some(30),
            }],
            tool_usage: (stage == GoalContinuationObservationStage::AfterToolRound)
                .then(|| {
                    vec![GoalToolUsage {
                        invocation_id: Uuid::new_v4(),
                        elapsed_ms: 5,
                    }]
                })
                .unwrap_or_default(),
            observed_at,
        }
    }

    #[test]
    fn checkpoint_migrates_replays_idempotently_and_survives_restart() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("events.sqlite3");
        let store = EventStore::open(&path).expect("store opens");
        let run_id = append_frozen_goal(&store);
        store
            .conn
            .execute("DROP TABLE goal_context_checkpoints", [])
            .expect("new table can be removed to model a legacy database");
        drop(store);

        let store = EventStore::open(&path).expect("migration recreates table");
        let now = Utc::now();
        let observation = observation(GoalContinuationObservationStage::Final, Uuid::new_v4(), now);
        let first = store
            .record_goal_context_checkpoint(run_id, observation.clone())
            .expect("checkpoint records")
            .expect("checkpoint exists");
        let first_row_revision: u64 = store
            .conn
            .query_row(
                "SELECT row_revision FROM goal_context_checkpoints WHERE run_id = ?1",
                params![run_id.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        let replayed = store
            .record_goal_context_checkpoint(run_id, observation)
            .expect("replay succeeds")
            .expect("checkpoint exists");
        assert_eq!(first, replayed);
        assert_eq!(first_row_revision, 0);
        drop(store);

        let reopened = EventStore::open(&path).expect("store reopens");
        assert_eq!(
            reopened
                .goal_context_checkpoint(run_id)
                .expect("checkpoint loads"),
            Some(first)
        );
    }

    #[test]
    fn checkpoint_tamper_fails_store_reopen() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("events.sqlite3");
        let store = EventStore::open(&path).expect("store opens");
        let run_id = append_frozen_goal(&store);
        store
            .record_goal_context_checkpoint(
                run_id,
                observation(
                    GoalContinuationObservationStage::Final,
                    Uuid::new_v4(),
                    Utc::now(),
                ),
            )
            .expect("checkpoint records");
        drop(store);
        Connection::open(&path)
            .unwrap()
            .execute(
                "UPDATE goal_context_checkpoints SET checkpoint_json = '{}' WHERE run_id = ?1",
                params![run_id.to_string()],
            )
            .unwrap();

        assert!(EventStore::open(&path).is_err());
    }

    #[test]
    fn no_evidence_checkpoint_blocks_run_completion_with_exact_reason() {
        let store = Mutex::new(EventStore::open_memory().expect("store opens"));
        let run_id = append_frozen_goal(&store.lock().unwrap());
        let checkpoint = store
            .lock()
            .unwrap()
            .record_goal_context_checkpoint(
                run_id,
                observation(
                    GoalContinuationObservationStage::AfterToolRound,
                    Uuid::new_v4(),
                    Utc::now(),
                ),
            )
            .expect("checkpoint records")
            .expect("checkpoint exists");

        assert_eq!(
            checkpoint.blocker.as_ref().map(|item| item.code),
            Some(GoalContinuationBlockerCode::NoNewEvidence)
        );
        assert_eq!(
            store
                .lock()
                .unwrap()
                .classify_agent_run_completion(run_id)
                .expect("completion classifies"),
            super::super::AgentRunCompletionClassification::VerificationBlocked(
                "goal_continuation_no_new_evidence".to_string()
            )
        );
    }

    #[test]
    fn generic_event_append_cannot_mint_a_checkpoint() {
        let store = EventStore::open_memory().expect("store opens");
        let event = KernelEvent::new(CONTEXT_CHECKPOINT_RECORDED_EVENT, json!({"forged": true}))
            .expect("event builds");
        assert!(store.append(&event).is_err());
    }
}
