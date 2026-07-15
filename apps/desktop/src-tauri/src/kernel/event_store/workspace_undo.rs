use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use super::{EventStore, EventStoreError, EventStoreResult};
use crate::kernel::workspace_undo::{
    WorkspaceCheckpointEffectState, WorkspaceMutationCheckpoint, WorkspaceMutationCheckpointStatus,
    WorkspaceUndoView, MAX_WORKSPACE_UNDO_PREIMAGE_BYTES,
};

const RECOVERY_SCAN_LIMIT: i64 = 1024;

pub(super) fn migrate(store: &EventStore) -> EventStoreResult<()> {
    store.conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS workspace_mutation_checkpoints (
            id TEXT PRIMARY KEY NOT NULL,
            tool_invocation_id TEXT NOT NULL UNIQUE,
            run_id TEXT,
            checkpoint_json TEXT NOT NULL,
            status TEXT NOT NULL,
            effect_state TEXT NOT NULL,
            row_revision INTEGER NOT NULL,
            preimage BLOB,
            consumed_action_revision TEXT,
            quarantine_code TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_workspace_mutation_checkpoint_recovery
            ON workspace_mutation_checkpoints (status, updated_at)
            WHERE quarantine_code IS NULL;
        "#,
    )?;
    super::ensure_sqlite_column(
        &store.conn,
        "workspace_mutation_checkpoints",
        "consumed_action_revision",
        "ALTER TABLE workspace_mutation_checkpoints ADD COLUMN consumed_action_revision TEXT",
    )?;
    Ok(())
}

impl EventStore {
    pub(crate) fn insert_workspace_mutation_intent(
        &self,
        checkpoint: &WorkspaceMutationCheckpoint,
    ) -> EventStoreResult<()> {
        if checkpoint.status != WorkspaceMutationCheckpointStatus::Intent
            || checkpoint.effect_state != WorkspaceCheckpointEffectState::NoEffect
            || checkpoint.revision != 0
        {
            return Err(invalid("workspace mutation intent is invalid"));
        }
        let inserted = self.conn.execute(
            r#"INSERT OR IGNORE INTO workspace_mutation_checkpoints
               (id, tool_invocation_id, run_id, checkpoint_json, status, effect_state,
                row_revision, preimage, consumed_action_revision, quarantine_code, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, NULL, NULL, NULL, ?7, ?8)"#,
            params![
                checkpoint.id.to_string(),
                checkpoint.tool_invocation_id.to_string(),
                checkpoint.run_id.map(|value| value.to_string()),
                serde_json::to_string(checkpoint)?,
                status_text(checkpoint.status),
                effect_text(checkpoint.effect_state),
                timestamp(checkpoint.created_at),
                timestamp(checkpoint.updated_at),
            ],
        )?;
        if inserted != 1 {
            return Err(invalid(
                "workspace mutation checkpoint identity already exists",
            ));
        }
        Ok(())
    }

    pub(crate) fn prepare_workspace_mutation_checkpoint(
        &self,
        checkpoint: &WorkspaceMutationCheckpoint,
        preimage: Option<&[u8]>,
    ) -> EventStoreResult<WorkspaceMutationCheckpoint> {
        if checkpoint.status != WorkspaceMutationCheckpointStatus::Prepared
            || checkpoint.effect_state != WorkspaceCheckpointEffectState::NoEffect
            || checkpoint.revision != 1
            || preimage.is_some_and(|bytes| bytes.len() as u64 > MAX_WORKSPACE_UNDO_PREIMAGE_BYTES)
        {
            return Err(invalid(
                "workspace mutation checkpoint preparation is invalid",
            ));
        }
        update_checkpoint(
            self,
            checkpoint,
            WorkspaceMutationCheckpointStatus::Intent,
            0,
            Some(preimage),
        )?;
        Ok(checkpoint.clone())
    }

    pub(crate) fn start_workspace_mutation_effect(
        &self,
        prepared: &WorkspaceMutationCheckpoint,
    ) -> EventStoreResult<WorkspaceMutationCheckpoint> {
        if prepared.status != WorkspaceMutationCheckpointStatus::Prepared
            || prepared.effect_state != WorkspaceCheckpointEffectState::NoEffect
        {
            return Err(invalid("workspace mutation checkpoint is not prepared"));
        }
        let mut started = prepared.clone();
        started.status = WorkspaceMutationCheckpointStatus::EffectStarted;
        started.effect_state = WorkspaceCheckpointEffectState::EffectUnknown;
        started.revision = started
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("workspace mutation checkpoint revision is exhausted"))?;
        started.updated_at = Utc::now();
        update_checkpoint(
            self,
            &started,
            WorkspaceMutationCheckpointStatus::Prepared,
            prepared.revision,
            None,
        )?;
        Ok(started)
    }

    pub(crate) fn finish_workspace_mutation_checkpoint(
        &self,
        checkpoint: &WorkspaceMutationCheckpoint,
    ) -> EventStoreResult<WorkspaceMutationCheckpoint> {
        if !matches!(
            checkpoint.status,
            WorkspaceMutationCheckpointStatus::Ready
                | WorkspaceMutationCheckpointStatus::NotUndoable
        ) || checkpoint.effect_state != WorkspaceCheckpointEffectState::KnownApplied
            || checkpoint.revision == 0
        {
            return Err(invalid("workspace mutation completion is invalid"));
        }
        update_checkpoint(
            self,
            checkpoint,
            WorkspaceMutationCheckpointStatus::EffectStarted,
            checkpoint.revision - 1,
            None,
        )?;
        Ok(checkpoint.clone())
    }

    pub(crate) fn fail_workspace_mutation_checkpoint(
        &self,
        tool_invocation_id: Uuid,
        safe_error_code: &str,
    ) -> EventStoreResult<()> {
        let mut checkpoint =
            self.workspace_mutation_checkpoint_for_invocation(tool_invocation_id)?;
        if !matches!(
            checkpoint.status,
            WorkspaceMutationCheckpointStatus::Intent | WorkspaceMutationCheckpointStatus::Prepared
        ) {
            return Err(invalid(
                "workspace mutation cannot be failed after its effect started",
            ));
        }
        let expected_status = checkpoint.status;
        let expected_revision = checkpoint.revision;
        checkpoint.status = WorkspaceMutationCheckpointStatus::Failed;
        checkpoint.effect_state = WorkspaceCheckpointEffectState::NoEffect;
        checkpoint.safe_error_code = Some(validate_safe_code(safe_error_code)?);
        checkpoint.revision = checkpoint
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("workspace mutation checkpoint revision is exhausted"))?;
        checkpoint.updated_at = Utc::now();
        update_checkpoint(self, &checkpoint, expected_status, expected_revision, None)
    }

    pub(crate) fn repair_workspace_mutation_checkpoint(
        &self,
        current: &WorkspaceMutationCheckpoint,
        safe_error_code: &str,
    ) -> EventStoreResult<()> {
        let mut repaired = current.clone();
        repaired.status = WorkspaceMutationCheckpointStatus::RepairRequired;
        repaired.effect_state = WorkspaceCheckpointEffectState::EffectUnknown;
        repaired.safe_error_code = Some(validate_safe_code(safe_error_code)?);
        repaired.revision = repaired
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("workspace mutation checkpoint revision is exhausted"))?;
        repaired.updated_at = Utc::now();
        update_checkpoint(self, &repaired, current.status, current.revision, None)
    }

    pub(crate) fn repair_workspace_mutation_checkpoint_by_invocation(
        &self,
        tool_invocation_id: Uuid,
        safe_error_code: &str,
    ) -> EventStoreResult<()> {
        let current = self.workspace_mutation_checkpoint_for_invocation(tool_invocation_id)?;
        self.repair_workspace_mutation_checkpoint(&current, safe_error_code)
    }

    pub(crate) fn claim_workspace_undo(
        &self,
        checkpoint_id: Uuid,
        action_revision: &str,
    ) -> EventStoreResult<Option<WorkspaceMutationCheckpoint>> {
        if !valid_action_revision(action_revision) {
            return Ok(None);
        }
        let transaction = self.conn.unchecked_transaction()?;
        let row = transaction
            .query_row(
                r#"SELECT checkpoint_json, status, effect_state, row_revision
                     FROM workspace_mutation_checkpoints
                    WHERE id=?1 AND quarantine_code IS NULL"#,
                params![checkpoint_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((json, status, effect, revision)) = row else {
            return Err(EventStoreError::NotFound(
                "workspace mutation checkpoint does not exist".to_string(),
            ));
        };
        let mut checkpoint = decode_checkpoint(&json, &status, &effect, revision)?;
        if checkpoint.status != WorkspaceMutationCheckpointStatus::Ready
            || checkpoint.action_revision().as_deref() != Some(action_revision)
        {
            transaction.commit()?;
            return Ok(None);
        }
        let expected_revision = checkpoint.revision;
        checkpoint.status = WorkspaceMutationCheckpointStatus::UndoStarted;
        checkpoint.effect_state = WorkspaceCheckpointEffectState::EffectUnknown;
        checkpoint.revision = checkpoint
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("workspace mutation checkpoint revision is exhausted"))?;
        checkpoint.updated_at = Utc::now();
        let changed = transaction.execute(
            r#"UPDATE workspace_mutation_checkpoints
                  SET checkpoint_json=?2, status=?3, effect_state=?4,
                      row_revision=?5, consumed_action_revision=?6, updated_at=?7
                WHERE id=?1 AND status=?8 AND row_revision=?9 AND quarantine_code IS NULL"#,
            params![
                checkpoint.id.to_string(),
                serde_json::to_string(&checkpoint)?,
                status_text(checkpoint.status),
                effect_text(checkpoint.effect_state),
                revision_i64(checkpoint.revision)?,
                action_revision,
                timestamp(checkpoint.updated_at),
                status_text(WorkspaceMutationCheckpointStatus::Ready),
                revision_i64(expected_revision)?,
            ],
        )?;
        if changed != 1 {
            transaction.commit()?;
            return Ok(None);
        }
        transaction.commit()?;
        Ok(Some(checkpoint))
    }

    pub(crate) fn finish_workspace_undo(
        &self,
        checkpoint: &WorkspaceMutationCheckpoint,
    ) -> EventStoreResult<WorkspaceMutationCheckpoint> {
        if checkpoint.status != WorkspaceMutationCheckpointStatus::Undone
            || checkpoint.effect_state != WorkspaceCheckpointEffectState::NoEffect
            || checkpoint.revision == 0
        {
            return Err(invalid("workspace undo completion is invalid"));
        }
        update_checkpoint(
            self,
            checkpoint,
            WorkspaceMutationCheckpointStatus::UndoStarted,
            checkpoint.revision - 1,
            None,
        )?;
        Ok(checkpoint.clone())
    }

    pub(crate) fn workspace_mutation_checkpoint(
        &self,
        checkpoint_id: Uuid,
    ) -> EventStoreResult<WorkspaceMutationCheckpoint> {
        load_checkpoint(self, "id", checkpoint_id)
    }

    pub(crate) fn workspace_mutation_checkpoint_for_invocation(
        &self,
        invocation_id: Uuid,
    ) -> EventStoreResult<WorkspaceMutationCheckpoint> {
        load_checkpoint(self, "tool_invocation_id", invocation_id)
    }

    pub(crate) fn workspace_mutation_preimage(
        &self,
        checkpoint_id: Uuid,
    ) -> EventStoreResult<Option<Vec<u8>>> {
        self.conn
            .query_row(
                r#"SELECT preimage FROM workspace_mutation_checkpoints
                    WHERE id=?1 AND quarantine_code IS NULL"#,
                params![checkpoint_id.to_string()],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                EventStoreError::NotFound(
                    "workspace mutation checkpoint does not exist".to_string(),
                )
            })
    }

    pub(crate) fn workspace_undo_action_was_accepted(
        &self,
        checkpoint_id: Uuid,
        action_revision: &str,
    ) -> EventStoreResult<bool> {
        if !valid_action_revision(action_revision) {
            return Ok(false);
        }
        Ok(self
            .conn
            .query_row(
                r#"SELECT 1 FROM workspace_mutation_checkpoints
                    WHERE id=?1 AND consumed_action_revision=?2
                      AND quarantine_code IS NULL"#,
                params![checkpoint_id.to_string(), action_revision],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub(crate) fn list_workspace_undo_views(&self) -> EventStoreResult<Vec<WorkspaceUndoView>> {
        let mut statement = self.conn.prepare(
            r#"SELECT checkpoint_json, status, effect_state, row_revision
                 FROM workspace_mutation_checkpoints
                WHERE quarantine_code IS NULL
                ORDER BY updated_at DESC, rowid DESC LIMIT 256"#,
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut views = Vec::with_capacity(rows.len());
        for (json, status, effect, revision) in rows {
            views.push(decode_checkpoint(&json, &status, &effect, revision)?.public_view());
        }
        Ok(views)
    }

    pub(crate) fn recover_workspace_mutation_checkpoints(
        &self,
        now: DateTime<Utc>,
    ) -> EventStoreResult<(usize, usize)> {
        let mut statement = self.conn.prepare(
            r#"SELECT checkpoint_json, status, effect_state, row_revision
                 FROM workspace_mutation_checkpoints
                WHERE quarantine_code IS NULL
                  AND status IN ('intent', 'prepared', 'effect_started', 'undo_started')
                ORDER BY updated_at, rowid LIMIT ?1"#,
        )?;
        let rows = statement
            .query_map(params![RECOVERY_SCAN_LIMIT], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut no_effect = 0usize;
        let mut repair_required = 0usize;
        for (json, status, effect, revision) in rows {
            let current = decode_checkpoint(&json, &status, &effect, revision)?;
            let mut recovered = current.clone();
            match current.status {
                WorkspaceMutationCheckpointStatus::Intent
                | WorkspaceMutationCheckpointStatus::Prepared => {
                    recovered.status = WorkspaceMutationCheckpointStatus::Failed;
                    recovered.effect_state = WorkspaceCheckpointEffectState::NoEffect;
                    recovered.safe_error_code = Some("interrupted_before_effect".to_string());
                    no_effect += 1;
                }
                WorkspaceMutationCheckpointStatus::EffectStarted
                | WorkspaceMutationCheckpointStatus::UndoStarted => {
                    recovered.status = WorkspaceMutationCheckpointStatus::RepairRequired;
                    recovered.effect_state = WorkspaceCheckpointEffectState::EffectUnknown;
                    recovered.safe_error_code = Some("interrupted_effect_unknown".to_string());
                    repair_required += 1;
                }
                _ => continue,
            }
            recovered.revision = recovered
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("workspace mutation checkpoint revision is exhausted"))?;
            recovered.updated_at = now;
            update_checkpoint(self, &recovered, current.status, current.revision, None)?;
        }
        Ok((no_effect, repair_required))
    }
}

fn load_checkpoint(
    store: &EventStore,
    column: &str,
    id: Uuid,
) -> EventStoreResult<WorkspaceMutationCheckpoint> {
    debug_assert!(matches!(column, "id" | "tool_invocation_id"));
    let sql = format!(
        "SELECT checkpoint_json, status, effect_state, row_revision FROM workspace_mutation_checkpoints WHERE {column}=?1 AND quarantine_code IS NULL"
    );
    let row = store
        .conn
        .query_row(&sql, params![id.to_string()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .optional()?
        .ok_or_else(|| {
            EventStoreError::NotFound("workspace mutation checkpoint does not exist".to_string())
        })?;
    decode_checkpoint(&row.0, &row.1, &row.2, row.3)
}

fn update_checkpoint(
    store: &EventStore,
    checkpoint: &WorkspaceMutationCheckpoint,
    expected_status: WorkspaceMutationCheckpointStatus,
    expected_revision: u64,
    preimage: Option<Option<&[u8]>>,
) -> EventStoreResult<()> {
    let changed = match preimage {
        Some(preimage) => store.conn.execute(
            r#"UPDATE workspace_mutation_checkpoints
                  SET checkpoint_json=?2, status=?3, effect_state=?4,
                      row_revision=?5, preimage=?6, updated_at=?7
                WHERE id=?1 AND status=?8 AND row_revision=?9 AND quarantine_code IS NULL"#,
            params![
                checkpoint.id.to_string(),
                serde_json::to_string(checkpoint)?,
                status_text(checkpoint.status),
                effect_text(checkpoint.effect_state),
                revision_i64(checkpoint.revision)?,
                preimage,
                timestamp(checkpoint.updated_at),
                status_text(expected_status),
                revision_i64(expected_revision)?,
            ],
        )?,
        None => store.conn.execute(
            r#"UPDATE workspace_mutation_checkpoints
                  SET checkpoint_json=?2, status=?3, effect_state=?4,
                      row_revision=?5, updated_at=?6
                WHERE id=?1 AND status=?7 AND row_revision=?8 AND quarantine_code IS NULL"#,
            params![
                checkpoint.id.to_string(),
                serde_json::to_string(checkpoint)?,
                status_text(checkpoint.status),
                effect_text(checkpoint.effect_state),
                revision_i64(checkpoint.revision)?,
                timestamp(checkpoint.updated_at),
                status_text(expected_status),
                revision_i64(expected_revision)?,
            ],
        )?,
    };
    if changed != 1 {
        return Err(invalid(
            "workspace mutation checkpoint changed concurrently or is quarantined",
        ));
    }
    Ok(())
}

fn decode_checkpoint(
    json: &str,
    status: &str,
    effect: &str,
    revision: i64,
) -> EventStoreResult<WorkspaceMutationCheckpoint> {
    let checkpoint: WorkspaceMutationCheckpoint = serde_json::from_str(json)?;
    if status_text(checkpoint.status) != status
        || effect_text(checkpoint.effect_state) != effect
        || revision_u64(revision)? != checkpoint.revision
    {
        return Err(invalid(
            "workspace mutation checkpoint projection is inconsistent",
        ));
    }
    Ok(checkpoint)
}

fn validate_safe_code(value: &str) -> EventStoreResult<String> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(invalid("workspace mutation safe error code is invalid"));
    }
    Ok(value.to_string())
}

fn valid_action_revision(value: &str) -> bool {
    value.len() == 70
        && value.starts_with("undo1:")
        && value[6..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn status_text(status: WorkspaceMutationCheckpointStatus) -> &'static str {
    match status {
        WorkspaceMutationCheckpointStatus::Intent => "intent",
        WorkspaceMutationCheckpointStatus::Prepared => "prepared",
        WorkspaceMutationCheckpointStatus::EffectStarted => "effect_started",
        WorkspaceMutationCheckpointStatus::Ready => "ready",
        WorkspaceMutationCheckpointStatus::NotUndoable => "not_undoable",
        WorkspaceMutationCheckpointStatus::UndoStarted => "undo_started",
        WorkspaceMutationCheckpointStatus::Undone => "undone",
        WorkspaceMutationCheckpointStatus::Failed => "failed",
        WorkspaceMutationCheckpointStatus::RepairRequired => "repair_required",
    }
}

fn effect_text(effect: WorkspaceCheckpointEffectState) -> &'static str {
    match effect {
        WorkspaceCheckpointEffectState::NoEffect => "no_effect",
        WorkspaceCheckpointEffectState::KnownApplied => "known_applied",
        WorkspaceCheckpointEffectState::EffectUnknown => "effect_unknown",
    }
}

fn revision_i64(value: u64) -> EventStoreResult<i64> {
    i64::try_from(value).map_err(|_| invalid("workspace mutation revision is too large"))
}

fn revision_u64(value: i64) -> EventStoreResult<u64> {
    u64::try_from(value).map_err(|_| invalid("workspace mutation revision is invalid"))
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn invalid(message: &str) -> EventStoreError {
    EventStoreError::InvalidState(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::EventStore;
    use crate::kernel::capability::{FileSystemMutationOperation, LocalFileSystemMutationClient};
    use crate::kernel::models::AccessMode;
    use crate::kernel::tool_runtime::{
        prepare_tool_execution, ToolExecutionRequest, FILESYSTEM_MUTATE_TOOL_ID,
    };
    use crate::kernel::workspace_undo::{
        execute_checkpointed_mutation, WorkspaceCheckpointEffectState, WorkspaceMutationCheckpoint,
        WorkspaceMutationCheckpointStatus,
    };
    use chrono::Utc;

    #[test]
    fn restart_recovery_never_replays_uncertain_local_effects() {
        let store = EventStore::open_memory().unwrap();
        let request = ToolExecutionRequest {
            tool_id: FILESYSTEM_MUTATE_TOOL_ID.to_string(),
            input: serde_json::json!({
                "operation": "create_file",
                "path": std::env::temp_dir().join("ds-agent-recovery.txt"),
                "destination": null,
                "content": "test",
                "summary": "recovery test"
            }),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        };
        let plan = prepare_tool_execution(&request).unwrap();
        let intent = WorkspaceMutationCheckpoint::intent(
            &plan,
            FileSystemMutationOperation::CreateFile,
            request.input["path"].as_str().unwrap(),
            None,
        )
        .unwrap();
        store.insert_workspace_mutation_intent(&intent).unwrap();
        let mut prepared = intent.clone();
        prepared.status = WorkspaceMutationCheckpointStatus::Prepared;
        prepared.revision = 1;
        prepared.updated_at = Utc::now();
        let prepared = store
            .prepare_workspace_mutation_checkpoint(&prepared, None)
            .unwrap();
        store.start_workspace_mutation_effect(&prepared).unwrap();

        let result = store
            .recover_workspace_mutation_checkpoints(Utc::now())
            .unwrap();
        assert_eq!(result, (0, 1));
        let recovered = store
            .workspace_mutation_checkpoint_for_invocation(plan.invocation_id)
            .unwrap();
        assert_eq!(
            recovered.status,
            WorkspaceMutationCheckpointStatus::RepairRequired
        );
        assert_eq!(
            recovered.effect_state,
            WorkspaceCheckpointEffectState::EffectUnknown
        );
    }

    #[test]
    fn restart_during_undo_marks_unknown_without_replaying_the_inverse() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("undo-crash.txt");
        std::fs::write(&path, b"before").unwrap();
        let store = EventStore::open(root.path().join("events.sqlite3")).unwrap();
        let request = ToolExecutionRequest {
            tool_id: FILESYSTEM_MUTATE_TOOL_ID.to_string(),
            input: serde_json::json!({
                "operation": "update_file",
                "path": path,
                "destination": null,
                "content": "after",
                "summary": "undo crash test"
            }),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        };
        let plan = prepare_tool_execution(&request).unwrap();
        execute_checkpointed_mutation(
            &store,
            &plan,
            &LocalFileSystemMutationClient,
            FileSystemMutationOperation::UpdateFile,
            request.input["path"].as_str().unwrap(),
            None,
            Some("after"),
        )
        .unwrap();
        let ready = store
            .workspace_mutation_checkpoint_for_invocation(plan.invocation_id)
            .unwrap();
        store
            .claim_workspace_undo(ready.id, &ready.action_revision().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"after");
        assert_eq!(
            store
                .recover_workspace_mutation_checkpoints(Utc::now())
                .unwrap(),
            (0, 1)
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"after");
        let recovered = store.workspace_mutation_checkpoint(ready.id).unwrap();
        assert_eq!(
            recovered.status,
            WorkspaceMutationCheckpointStatus::RepairRequired
        );
        assert_eq!(
            recovered.effect_state,
            WorkspaceCheckpointEffectState::EffectUnknown
        );
    }
}
