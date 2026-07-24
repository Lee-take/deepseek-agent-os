use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use super::{EventStore, EventStoreError, EventStoreResult};
use crate::kernel::computer_use_session::{
    ComputerUseRecoverySweep, ComputerUseSession, ComputerUseStep, ComputerUseStepStatus,
};

const COMPUTER_USE_RECOVERY_SCAN_LIMIT: i64 = 1_024;

pub(super) fn migrate(store: &EventStore) -> EventStoreResult<()> {
    store.conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS computer_use_sessions (
            id TEXT PRIMARY KEY NOT NULL,
            session_json TEXT NOT NULL,
            run_id TEXT,
            active_step_id TEXT,
            row_revision INTEGER NOT NULL,
            quarantine_code TEXT,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS computer_use_steps (
            id TEXT PRIMARY KEY NOT NULL,
            session_id TEXT NOT NULL,
            sequence INTEGER NOT NULL,
            step_json TEXT NOT NULL,
            status TEXT NOT NULL,
            action_fingerprint TEXT,
            approval_request_id TEXT,
            row_revision INTEGER NOT NULL,
            quarantine_code TEXT,
            updated_at TEXT NOT NULL,
            UNIQUE (session_id, sequence),
            FOREIGN KEY (session_id) REFERENCES computer_use_sessions(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_computer_use_steps_recovery
            ON computer_use_steps (status, updated_at);
        CREATE INDEX IF NOT EXISTS idx_computer_use_steps_session
            ON computer_use_steps (session_id, sequence);
        "#,
    )?;
    Ok(())
}

impl EventStore {
    pub fn insert_computer_use_session(
        &self,
        session: &ComputerUseSession,
    ) -> EventStoreResult<()> {
        session.validate().map_err(EventStoreError::InvalidState)?;
        let json = serde_json::to_string(session)?;
        let inserted = self.conn.execute(
            r#"INSERT OR IGNORE INTO computer_use_sessions
               (id, session_json, run_id, active_step_id, row_revision, quarantine_code, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)"#,
            params![
                session.id.to_string(),
                &json,
                session.run_id.map(|value| value.to_string()),
                session.active_step_id.map(|value| value.to_string()),
                revision_i64(session.revision)?,
                timestamp(session.updated_at),
            ],
        )?;
        if inserted == 0 {
            let existing: Option<(String, i64)> = self
                .conn
                .query_row(
                    "SELECT session_json, row_revision FROM computer_use_sessions WHERE id=?1 AND quarantine_code IS NULL",
                    params![session.id.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if existing != Some((json, revision_i64(session.revision)?)) {
                return Err(EventStoreError::InvalidState(
                    "computer use session identity was reused with different state".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn insert_computer_use_step(&self, step: &ComputerUseStep) -> EventStoreResult<()> {
        step.validate().map_err(EventStoreError::InvalidState)?;
        if step.revision != 0 || step.status != ComputerUseStepStatus::Observed {
            return Err(EventStoreError::InvalidState(
                "new computer use step must start as observed revision zero".to_string(),
            ));
        }
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let (session_json, session_row_revision): (String, i64) = transaction
            .query_row(
                "SELECT session_json, row_revision FROM computer_use_sessions WHERE id=?1 AND quarantine_code IS NULL",
                params![step.session_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                EventStoreError::NotFound("computer use session does not exist".to_string())
            })?;
        let mut session: ComputerUseSession = serde_json::from_str(&session_json)?;
        session.validate().map_err(EventStoreError::InvalidState)?;
        if revision_u64(session_row_revision)? != session.revision {
            return Err(EventStoreError::InvalidState(
                "computer use session revision is inconsistent".to_string(),
            ));
        }
        if let Some(active_step_id) = session.active_step_id {
            let active_status: Option<String> = transaction
                .query_row(
                    "SELECT status FROM computer_use_steps WHERE id=?1 AND quarantine_code IS NULL",
                    params![active_step_id.to_string()],
                    |row| row.get(0),
                )
                .optional()?;
            if active_status
                .as_deref()
                .is_some_and(|status| !status_allows_reobserve(status))
            {
                return Err(EventStoreError::InvalidState(
                    "computer use session already has an active step".to_string(),
                ));
            }
        }
        let inserted = transaction.execute(
            r#"INSERT OR IGNORE INTO computer_use_steps
               (id, session_id, sequence, step_json, status, action_fingerprint,
                approval_request_id, row_revision, quarantine_code, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, 0, NULL, ?6)"#,
            params![
                step.id.to_string(),
                step.session_id.to_string(),
                i64::from(step.sequence),
                serde_json::to_string(step)?,
                status_text(step.status),
                timestamp(step.updated_at),
            ],
        )?;
        if inserted == 0 {
            return Err(EventStoreError::InvalidState(
                "computer use step identity or sequence already exists".to_string(),
            ));
        }
        let expected_session_revision = session.revision;
        session
            .activate_step(step.id, step.updated_at)
            .map_err(EventStoreError::InvalidState)?;
        let changed = transaction.execute(
            r#"UPDATE computer_use_sessions
                  SET session_json=?2, active_step_id=?3, row_revision=?4,
                      quarantine_code=NULL, updated_at=?5
                WHERE id=?1 AND row_revision=?6 AND quarantine_code IS NULL"#,
            params![
                session.id.to_string(),
                serde_json::to_string(&session)?,
                step.id.to_string(),
                revision_i64(session.revision)?,
                timestamp(session.updated_at),
                revision_i64(expected_session_revision)?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "computer use session changed while adding a step".to_string(),
            ));
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn update_computer_use_step(
        &self,
        step: &ComputerUseStep,
        expected_revision: u64,
    ) -> EventStoreResult<()> {
        step.validate().map_err(EventStoreError::InvalidState)?;
        let next = expected_revision.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState("computer use step revision is exhausted".to_string())
        })?;
        if step.revision != next {
            return Err(EventStoreError::InvalidState(
                "computer use step revision does not match the requested transition".to_string(),
            ));
        }
        let changed = self.conn.execute(
            r#"UPDATE computer_use_steps
                  SET step_json=?2, status=?3, action_fingerprint=?4,
                      approval_request_id=?5, row_revision=?6,
                      quarantine_code=NULL, updated_at=?7
                WHERE id=?1 AND row_revision=?8 AND quarantine_code IS NULL"#,
            params![
                step.id.to_string(),
                serde_json::to_string(step)?,
                status_text(step.status),
                step.action
                    .as_ref()
                    .map(|action| action.action_fingerprint.clone()),
                step.approval_request_id.map(|value| value.to_string()),
                revision_i64(step.revision)?,
                timestamp(step.updated_at),
                revision_i64(expected_revision)?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "computer use step changed concurrently or is quarantined".to_string(),
            ));
        }
        Ok(())
    }

    pub fn get_computer_use_step(&self, id: Uuid) -> EventStoreResult<ComputerUseStep> {
        let row: Option<(i64, String, String, i64)> = self
            .conn
            .query_row(
                "SELECT rowid, id, step_json, row_revision FROM computer_use_steps WHERE id=?1 AND quarantine_code IS NULL",
                params![id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let (rowid, stored_id, json, revision) = row.ok_or_else(|| {
            EventStoreError::NotFound("computer use step does not exist".to_string())
        })?;
        match decode_step(&stored_id, &json, revision) {
            Ok(step) => Ok(step),
            Err((code, _)) => {
                quarantine_step_row(&self.conn, rowid, code)?;
                Err(EventStoreError::InvalidState(
                    "computer use step record was quarantined".to_string(),
                ))
            }
        }
    }

    pub fn list_computer_use_sessions(&self) -> EventStoreResult<Vec<ComputerUseSession>> {
        let mut statement = self.conn.prepare(
            "SELECT rowid, id, session_json, row_revision FROM computer_use_sessions WHERE quarantine_code IS NULL ORDER BY updated_at DESC, rowid DESC LIMIT 128",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut valid = Vec::new();
        for (rowid, id, json, revision) in rows {
            match decode_session(&id, &json, revision) {
                Ok(session) => valid.push(session),
                Err((code, _)) => quarantine_session_row(&self.conn, rowid, code)?,
            }
        }
        Ok(valid)
    }

    pub fn list_computer_use_steps(
        &self,
        session_id: Uuid,
    ) -> EventStoreResult<Vec<ComputerUseStep>> {
        let mut statement = self.conn.prepare(
            "SELECT rowid, id, step_json, row_revision FROM computer_use_steps WHERE session_id=?1 AND quarantine_code IS NULL ORDER BY sequence, rowid",
        )?;
        let rows = statement
            .query_map(params![session_id.to_string()], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut valid = Vec::new();
        for (rowid, id, json, revision) in rows {
            match decode_step(&id, &json, revision) {
                Ok(step) if step.session_id == session_id => valid.push(step),
                Ok(_) => quarantine_step_row(&self.conn, rowid, "session_mismatch")?,
                Err((code, _)) => quarantine_step_row(&self.conn, rowid, code)?,
            }
        }
        Ok(valid)
    }

    pub fn recover_computer_use_steps_after_restart(
        &self,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ComputerUseRecoverySweep> {
        let mut statement = self.conn.prepare(
            r#"SELECT rowid, id, step_json, row_revision FROM computer_use_steps
                WHERE quarantine_code IS NULL
                  AND status IN ('observed','awaiting_approval','ready','action_started','awaiting_verification')
                ORDER BY updated_at, rowid LIMIT ?1"#,
        )?;
        let rows = statement
            .query_map(params![COMPUTER_USE_RECOVERY_SCAN_LIMIT], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut sweep = ComputerUseRecoverySweep::default();
        for (rowid, id, json, row_revision) in rows {
            let mut step = match decode_step(&id, &json, row_revision) {
                Ok(step) => step,
                Err((code, _)) => {
                    quarantine_step_row(&self.conn, rowid, code)?;
                    sweep.quarantined += 1;
                    continue;
                }
            };
            let previous_revision = step.revision;
            let previous_status = step.status;
            step.recover_after_restart(now)
                .map_err(EventStoreError::InvalidState)?;
            if step.revision != previous_revision {
                self.update_computer_use_step(&step, previous_revision)?;
            }
            match step.status {
                ComputerUseStepStatus::NeedsReplan
                    if previous_status != ComputerUseStepStatus::NeedsReplan =>
                {
                    sweep.needs_replan += 1
                }
                ComputerUseStepStatus::EffectUnknown
                    if previous_status != ComputerUseStepStatus::EffectUnknown =>
                {
                    sweep.effect_unknown += 1
                }
                ComputerUseStepStatus::AwaitingVerification => sweep.awaiting_verification += 1,
                _ => {}
            }
        }
        Ok(sweep)
    }
}

fn decode_session(
    stored_id: &str,
    json: &str,
    row_revision: i64,
) -> Result<ComputerUseSession, (&'static str, String)> {
    let session: ComputerUseSession =
        serde_json::from_str(json).map_err(|error| ("invalid_json", error.to_string()))?;
    if session.id.to_string() != stored_id {
        return Err(("identity_mismatch", "session id changed".to_string()));
    }
    let row_revision =
        revision_u64(row_revision).map_err(|error| ("revision_mismatch", error.to_string()))?;
    if session.revision != row_revision {
        return Err(("revision_mismatch", "session revision changed".to_string()));
    }
    session
        .validate()
        .map_err(|error| ("invalid_record", error))?;
    Ok(session)
}

fn decode_step(
    stored_id: &str,
    json: &str,
    row_revision: i64,
) -> Result<ComputerUseStep, (&'static str, String)> {
    let step: ComputerUseStep =
        serde_json::from_str(json).map_err(|error| ("invalid_json", error.to_string()))?;
    if step.id.to_string() != stored_id {
        return Err(("identity_mismatch", "step id changed".to_string()));
    }
    let row_revision =
        revision_u64(row_revision).map_err(|error| ("revision_mismatch", error.to_string()))?;
    if step.revision != row_revision {
        return Err(("revision_mismatch", "step revision changed".to_string()));
    }
    step.validate().map_err(|error| ("invalid_record", error))?;
    Ok(step)
}

fn quarantine_step_row(
    connection: &rusqlite::Connection,
    rowid: i64,
    code: &str,
) -> EventStoreResult<()> {
    connection.execute(
        "UPDATE computer_use_steps SET quarantine_code=?2 WHERE rowid=?1",
        params![rowid, code],
    )?;
    Ok(())
}

fn quarantine_session_row(
    connection: &rusqlite::Connection,
    rowid: i64,
    code: &str,
) -> EventStoreResult<()> {
    connection.execute(
        "UPDATE computer_use_sessions SET quarantine_code=?2 WHERE rowid=?1",
        params![rowid, code],
    )?;
    Ok(())
}

fn status_allows_reobserve(status: &str) -> bool {
    matches!(
        status,
        "verified"
            | "needs_replan"
            | "user_taken_over"
            | "effect_unknown"
            | "verification_failed"
            | "cancelled"
    )
}

fn status_text(status: ComputerUseStepStatus) -> &'static str {
    match status {
        ComputerUseStepStatus::Observed => "observed",
        ComputerUseStepStatus::AwaitingApproval => "awaiting_approval",
        ComputerUseStepStatus::Ready => "ready",
        ComputerUseStepStatus::ActionStarted => "action_started",
        ComputerUseStepStatus::AwaitingVerification => "awaiting_verification",
        ComputerUseStepStatus::Verified => "verified",
        ComputerUseStepStatus::NeedsReplan => "needs_replan",
        ComputerUseStepStatus::UserTakenOver => "user_taken_over",
        ComputerUseStepStatus::EffectUnknown => "effect_unknown",
        ComputerUseStepStatus::VerificationFailed => "verification_failed",
        ComputerUseStepStatus::Cancelled => "cancelled",
    }
}

fn revision_i64(value: u64) -> EventStoreResult<i64> {
    i64::try_from(value).map_err(|_| {
        EventStoreError::InvalidState("computer use revision is too large".to_string())
    })
}

fn revision_u64(value: i64) -> EventStoreResult<u64> {
    u64::try_from(value)
        .map_err(|_| EventStoreError::InvalidState("computer use revision is invalid".to_string()))
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::capability::ComputerControlAction;
    use crate::kernel::computer_use_session::{
        ComputerUseActionBinding, ComputerUseApprovalActor, ComputerUseObservation,
        ComputerUseObservationPhase, ComputerUsePostcondition, ComputerUseUndoCapability,
    };
    use sha2::{Digest, Sha256};

    fn fingerprint(value: &str) -> String {
        hex::encode(Sha256::digest(value.as_bytes()))
    }

    fn observed_records(
        now: DateTime<Utc>,
    ) -> (
        ComputerUseSession,
        ComputerUseStep,
        ComputerUseActionBinding,
    ) {
        let session =
            ComputerUseSession::new(None, "Update an isolated Notepad editor.".to_string(), now)
                .unwrap();
        let observation = ComputerUseObservation::new(
            ComputerUseObservationPhase::PreAction,
            fingerprint("application"),
            fingerprint("process"),
            fingerprint("window"),
            fingerprint("window-title"),
            fingerprint("frame"),
            Some(fingerprint("target")),
            Some(fingerprint("before")),
            "computer-screenshots/before-notepad.png".to_string(),
            "Isolated Notepad editor is visible.".to_string(),
            now,
        )
        .unwrap();
        let action = ComputerUseActionBinding::new(
            &observation,
            ComputerControlAction::TypeText {
                text: "verified text".to_string(),
            },
            "Type approved text in isolated Notepad.".to_string(),
            ComputerUsePostcondition::TargetSemanticFingerprintEquals {
                expected: fingerprint("after"),
            },
        )
        .unwrap();
        let step = ComputerUseStep::new_observed(
            session.id,
            1,
            observation,
            ComputerUseUndoCapability::None,
            now,
        )
        .unwrap();
        (session, step, action)
    }

    #[test]
    fn action_started_recovers_as_effect_unknown_without_replay() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("computer-use.sqlite3");
        let now = Utc::now();
        let (session, mut step, action) = observed_records(now);
        let approval_id = Uuid::new_v4();
        let action_fingerprint = action.action_fingerprint.clone();
        {
            let store = EventStore::open(&path).unwrap();
            store.insert_computer_use_session(&session).unwrap();
            store.insert_computer_use_step(&step).unwrap();
            let expected = step.revision;
            step.bind_action(action, now).unwrap();
            store.update_computer_use_step(&step, expected).unwrap();
            let expected = step.revision;
            step.approve(
                approval_id,
                &action_fingerprint,
                ComputerUseApprovalActor::User,
                now,
            )
            .unwrap();
            store.update_computer_use_step(&step, expected).unwrap();
            let expected = step.revision;
            let application = step.pre_observation.application_fingerprint.clone();
            let process = step.pre_observation.process_fingerprint.clone();
            let window = step.pre_observation.window_fingerprint.clone();
            let window_title = step.pre_observation.window_title_fingerprint.clone();
            let frame = step.pre_observation.frame_fingerprint.clone();
            let target = step.pre_observation.target_fingerprint.clone().unwrap();
            step.mark_action_started(
                approval_id,
                &application,
                &process,
                &window,
                &window_title,
                &frame,
                &target,
                now,
            )
            .unwrap();
            store.update_computer_use_step(&step, expected).unwrap();
        }

        let reopened = EventStore::open(&path).unwrap();
        let sweep = reopened
            .recover_computer_use_steps_after_restart(now)
            .unwrap();
        assert_eq!(sweep.effect_unknown, 1);
        let recovered = reopened.get_computer_use_step(step.id).unwrap();
        assert_eq!(recovered.status, ComputerUseStepStatus::EffectUnknown);
        assert_eq!(recovered.action_start_count, 1);
        let second = reopened
            .recover_computer_use_steps_after_restart(now)
            .unwrap();
        assert_eq!(second.effect_unknown, 0);
    }

    #[test]
    fn ready_step_requires_reobservation_and_drops_stale_approval_after_restart() {
        let store = EventStore::open_memory().unwrap();
        let now = Utc::now();
        let (session, mut step, action) = observed_records(now);
        let action_fingerprint = action.action_fingerprint.clone();
        store.insert_computer_use_session(&session).unwrap();
        store.insert_computer_use_step(&step).unwrap();
        let expected = step.revision;
        step.bind_action(action, now).unwrap();
        store.update_computer_use_step(&step, expected).unwrap();
        let expected = step.revision;
        step.approve(
            Uuid::new_v4(),
            &action_fingerprint,
            ComputerUseApprovalActor::User,
            now,
        )
        .unwrap();
        store.update_computer_use_step(&step, expected).unwrap();

        let sweep = store.recover_computer_use_steps_after_restart(now).unwrap();
        assert_eq!(sweep.needs_replan, 1);
        let recovered = store.get_computer_use_step(step.id).unwrap();
        assert_eq!(recovered.status, ComputerUseStepStatus::NeedsReplan);
        assert!(recovered.approval_request_id.is_none());
    }

    #[test]
    fn malformed_step_is_quarantined_without_starving_healthy_recovery() {
        let store = EventStore::open_memory().unwrap();
        let now = Utc::now();
        let (session, step, _) = observed_records(now);
        store.insert_computer_use_session(&session).unwrap();
        store.insert_computer_use_step(&step).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO computer_use_steps (id, session_id, sequence, step_json, status, row_revision, updated_at) VALUES (?1, ?2, 99, '{broken', 'ready', 0, ?3)",
                params![Uuid::new_v4().to_string(), session.id.to_string(), timestamp(now)],
            )
            .unwrap();

        let sweep = store.recover_computer_use_steps_after_restart(now).unwrap();
        assert_eq!(sweep.quarantined, 1);
        assert_eq!(sweep.needs_replan, 1);
        let quarantined: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM computer_use_steps WHERE quarantine_code='invalid_json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(quarantined, 1);
    }

    #[test]
    fn obsolete_contract_version_is_quarantined_without_starving_healthy_recovery() {
        let store = EventStore::open_memory().unwrap();
        let now = Utc::now();
        let (session, step, _) = observed_records(now);
        store.insert_computer_use_session(&session).unwrap();
        store.insert_computer_use_step(&step).unwrap();

        let mut obsolete = step.clone();
        obsolete.id = Uuid::new_v4();
        obsolete.sequence = 99;
        obsolete.contract_version = "computer-use-step/v1".to_string();
        store
            .conn
            .execute(
                "INSERT INTO computer_use_steps (id, session_id, sequence, step_json, status, row_revision, updated_at) VALUES (?1, ?2, ?3, ?4, 'observed', 0, ?5)",
                params![
                    obsolete.id.to_string(),
                    session.id.to_string(),
                    i64::from(obsolete.sequence),
                    serde_json::to_string(&obsolete).unwrap(),
                    timestamp(now)
                ],
            )
            .unwrap();

        let sweep = store.recover_computer_use_steps_after_restart(now).unwrap();
        assert_eq!(sweep.quarantined, 1);
        assert_eq!(sweep.needs_replan, 1);
        let quarantined: i64 = store
            .conn
            .query_row(
                "SELECT COUNT(*) FROM computer_use_steps WHERE quarantine_code='invalid_record'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(quarantined, 1);
    }
}
