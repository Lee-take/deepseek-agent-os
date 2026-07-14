use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{params, OptionalExtension, Transaction};
use uuid::Uuid;

use super::{EventStore, EventStoreError, EventStoreResult};
use crate::kernel::connectors::revocation::{
    ConnectorRemoteRevocationOutcome, ConnectorRevocationClaim, ConnectorRevocationClaimKind,
    ConnectorRevocationPhase, ConnectorRevocationReceipt, ConnectorRevocationTicket,
};
use crate::kernel::connectors::{
    ConnectorAccount, ConnectorCredentialDeleteOutcome, ConnectorHealth,
};
use crate::kernel::models::KernelEvent;

const CONNECTOR_REVOCATION_LEASE_SECONDS: i64 = 300;
const CONNECTOR_REVOCATION_MAX_BACKOFF_SECONDS: i64 = 3600;

pub(super) fn migrate(store: &EventStore) -> EventStoreResult<()> {
    store.conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS connector_revocations (
            id TEXT PRIMARY KEY NOT NULL,
            account_id TEXT NOT NULL,
            account_generation INTEGER NOT NULL,
            provider_id TEXT NOT NULL,
            ticket_json TEXT NOT NULL,
            phase TEXT NOT NULL,
            ticket_revision INTEGER NOT NULL,
            claim_id TEXT,
            claim_expires_at TEXT,
            next_action_at TEXT,
            remote_attempt_count INTEGER NOT NULL DEFAULT 0,
            local_attempt_count INTEGER NOT NULL DEFAULT 0,
            quarantine_code TEXT,
            quarantined_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_connector_revocations_active_account
            ON connector_revocations (account_id)
            WHERE phase != 'completed';

        CREATE INDEX IF NOT EXISTS idx_connector_revocations_due
            ON connector_revocations
               (phase, next_action_at, claim_expires_at,
                remote_attempt_count, local_attempt_count);
        "#,
    )?;
    super::ensure_sqlite_column(
        &store.conn,
        "connector_revocations",
        "quarantine_code",
        "ALTER TABLE connector_revocations ADD COLUMN quarantine_code TEXT",
    )?;
    super::ensure_sqlite_column(
        &store.conn,
        "connector_revocations",
        "quarantined_at",
        "ALTER TABLE connector_revocations ADD COLUMN quarantined_at TEXT",
    )?;
    Ok(())
}

fn as_i64(value: u64, label: &str) -> EventStoreResult<i64> {
    i64::try_from(value).map_err(|_| EventStoreError::InvalidState(format!("{label} is too large")))
}

fn parse_u64(value: i64, label: &str) -> EventStoreResult<u64> {
    u64::try_from(value).map_err(|_| EventStoreError::InvalidState(format!("{label} is invalid")))
}

fn parse_u32(value: i64, label: &str) -> EventStoreResult<u32> {
    u32::try_from(value).map_err(|_| EventStoreError::InvalidState(format!("{label} is invalid")))
}

fn revocation_backoff(attempt_count: u32) -> Duration {
    let exponent = attempt_count.saturating_sub(1).min(10);
    let seconds = 30_i64
        .saturating_mul(1_i64 << exponent)
        .min(CONNECTOR_REVOCATION_MAX_BACKOFF_SECONDS);
    Duration::seconds(seconds)
}

fn ticket_binding_matches(current: &ConnectorAccount, frozen: &ConnectorAccount) -> bool {
    current.id == frozen.id
        && current.provider_id == frozen.provider_id
        && current.tenant_ref == frozen.tenant_ref
        && current.credential_handle == frozen.credential_handle
        && current.granted_capabilities == frozen.granted_capabilities
}

fn read_current_account(
    transaction: &Transaction<'_>,
    account_id: Uuid,
) -> EventStoreResult<(ConnectorAccount, u64)> {
    let (account_json, generation) = transaction
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
    Ok((
        serde_json::from_str(&account_json)?,
        parse_u64(generation, "connector account generation")?,
    ))
}

fn validate_ticket_row(
    ticket: &ConnectorRevocationTicket,
    id: &str,
    account_id: &str,
    generation: i64,
    provider_id: &str,
    phase: &str,
    revision: i64,
) -> EventStoreResult<()> {
    if ticket.id().to_string() != id
        || ticket.account().id.to_string() != account_id
        || ticket.generation() != parse_u64(generation, "connector revocation generation")?
        || ticket.account().provider_id != provider_id
        || ticket.phase().as_str() != phase
        || ticket.revision() != parse_u64(revision, "connector revocation revision")?
    {
        return Err(EventStoreError::InvalidState(
            "connector revocation projection binding is invalid".to_string(),
        ));
    }
    Ok(())
}

fn read_ticket_by_id(
    transaction: &Transaction<'_>,
    id: Uuid,
) -> EventStoreResult<(ConnectorRevocationTicket, u32, u32)> {
    let row = transaction
        .query_row(
            r#"SELECT id, account_id, account_generation, provider_id, ticket_json,
                      phase, ticket_revision, remote_attempt_count, local_attempt_count
               FROM connector_revocations WHERE id = ?1"#,
            params![id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| EventStoreError::NotFound("connector revocation".to_string()))?;
    let ticket: ConnectorRevocationTicket = serde_json::from_str(&row.4)?;
    validate_ticket_row(&ticket, &row.0, &row.1, row.2, &row.3, &row.5, row.6)?;
    Ok((
        ticket,
        parse_u32(row.7, "connector revocation remote attempt count")?,
        parse_u32(row.8, "connector revocation local attempt count")?,
    ))
}

fn validate_current_ticket(
    transaction: &Transaction<'_>,
    ticket: &ConnectorRevocationTicket,
) -> EventStoreResult<()> {
    let (current, generation) = read_current_account(transaction, ticket.account().id)?;
    if current.health != ConnectorHealth::RevocationPending
        || generation != ticket.generation()
        || !ticket_binding_matches(&current, ticket.account())
    {
        return Err(EventStoreError::InvalidState(
            "connector revocation account binding changed".to_string(),
        ));
    }
    Ok(())
}

impl EventStore {
    pub(super) fn active_connector_revocation_phase(
        &self,
        account_id: Uuid,
    ) -> EventStoreResult<ConnectorRevocationPhase> {
        let transaction = self.conn.unchecked_transaction()?;
        let id = transaction
            .query_row(
                r#"SELECT id FROM connector_revocations
                   WHERE account_id = ?1 AND phase != 'completed'"#,
                params![account_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| EventStoreError::NotFound("connector revocation".to_string()))?;
        let id = Uuid::parse_str(&id)?;
        let (ticket, _, _) = read_ticket_by_id(&transaction, id)?;
        validate_current_ticket(&transaction, &ticket)?;
        Ok(ticket.phase())
    }
}

fn receipt(
    ticket: &ConnectorRevocationTicket,
    outcome: Option<ConnectorRemoteRevocationOutcome>,
    delete_outcome: Option<ConnectorCredentialDeleteOutcome>,
    now: DateTime<Utc>,
) -> ConnectorRevocationReceipt {
    ConnectorRevocationReceipt {
        revocation_id: ticket.id(),
        account_id: ticket.account().id,
        provider_id: ticket.account().provider_id.clone(),
        generation: ticket.generation(),
        phase: ticket.phase(),
        remote_outcome: outcome,
        credential_delete_outcome: delete_outcome,
        changed_at: now,
    }
}

fn persist_event(
    transaction: &Transaction<'_>,
    event_type: &str,
    receipt: &ConnectorRevocationReceipt,
) -> EventStoreResult<()> {
    let event = KernelEvent::new(event_type, receipt)?;
    EventStore::insert_kernel_event(transaction, &event)
}

impl EventStore {
    pub(crate) fn begin_connector_revocation(
        &self,
        account_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorRevocationTicket> {
        let transaction = self.conn.unchecked_transaction()?;
        let (mut account, generation) = read_current_account(&transaction, account_id)?;
        if account.health == ConnectorHealth::RevocationPending {
            let active_id = transaction
                .query_row(
                    r#"SELECT id FROM connector_revocations
                       WHERE account_id = ?1 AND phase != 'completed'"#,
                    params![account_id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .ok_or_else(|| {
                    EventStoreError::InvalidState(
                        "legacy connector revocation has no durable ticket".to_string(),
                    )
                })?;
            let (ticket, _, _) = read_ticket_by_id(&transaction, Uuid::parse_str(&active_id)?)?;
            validate_current_ticket(&transaction, &ticket)?;
            return Ok(ticket);
        }
        if matches!(
            account.health,
            ConnectorHealth::DisconnectPending | ConnectorHealth::Disconnected
        ) {
            return Err(EventStoreError::InvalidState(
                "connector account cannot start remote revocation".to_string(),
            ));
        }
        if transaction
            .query_row(
                r#"SELECT 1 FROM connector_revocations
                   WHERE account_id = ?1 AND phase != 'completed'"#,
                params![account_id.to_string()],
                |_| Ok(()),
            )
            .optional()?
            .is_some()
        {
            return Err(EventStoreError::InvalidState(
                "connector account already has an active revocation".to_string(),
            ));
        }

        let previous_health = account.health;
        let next_generation = generation.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState("connector account generation overflowed".to_string())
        })?;
        account.health = ConnectorHealth::RevocationPending;
        account.updated_at = now;
        let ticket = ConnectorRevocationTicket::new(account.clone(), next_generation, now);
        let generation_changed = transaction.execute(
            r#"UPDATE connector_account_generations SET generation = ?2
               WHERE account_id = ?1 AND generation = ?3"#,
            params![
                account_id.to_string(),
                as_i64(next_generation, "connector account generation")?,
                as_i64(generation, "connector account generation")?,
            ],
        )?;
        if generation_changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector revocation raced with another account transition".to_string(),
            ));
        }
        transaction.execute(
            r#"UPDATE connector_attachment_landings
               SET status = 'cleanup_required', failure_kind = 'account_revocation_started',
                   updated_at = ?3
               WHERE account_id = ?1 AND account_generation = ?2
                 AND status IN ('reserved', 'staging', 'ready')"#,
            params![
                account_id.to_string(),
                as_i64(generation, "connector account generation")?,
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
                "connector revocation could not be started".to_string(),
            ));
        }
        transaction.execute(
            r#"INSERT INTO connector_revocations
               (id, account_id, account_generation, provider_id, ticket_json, phase,
                ticket_revision, claim_id, claim_expires_at, next_action_at,
                remote_attempt_count, local_attempt_count, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8, 0, 0, ?8, ?8)"#,
            params![
                ticket.id().to_string(),
                account_id.to_string(),
                as_i64(next_generation, "connector revocation generation")?,
                ticket.account().provider_id,
                serde_json::to_string(&ticket)?,
                ticket.phase().as_str(),
                as_i64(ticket.revision(), "connector revocation revision")?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        persist_event(
            &transaction,
            "connector.revocation.started",
            &receipt(&ticket, None, None, now),
        )?;
        transaction.commit()?;
        let _ = self.terminalize_connector_attachment_cleanup_tools_for_account_generation(
            account_id, generation, now,
        );
        Ok(ticket)
    }

    fn claim_connector_revocation(
        &self,
        id: Uuid,
        kind: ConnectorRevocationClaimKind,
        now: DateTime<Utc>,
    ) -> EventStoreResult<Option<ConnectorRevocationClaim>> {
        let transaction = self.conn.unchecked_transaction()?;
        let (ticket, remote_attempt_count, local_attempt_count) =
            read_ticket_by_id(&transaction, id)?;
        let allowed = match kind {
            ConnectorRevocationClaimKind::Remote => matches!(
                ticket.phase(),
                ConnectorRevocationPhase::PendingRemote | ConnectorRevocationPhase::RetryScheduled
            ),
            ConnectorRevocationClaimKind::FinalizeLocal => {
                ticket.phase() == ConnectorRevocationPhase::RemoteConfirmed
            }
        };
        if !allowed {
            return Ok(None);
        }
        validate_current_ticket(&transaction, &ticket)?;
        let now_text = now.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let claim_id = Uuid::new_v4();
        let expires_at = now + Duration::seconds(CONNECTOR_REVOCATION_LEASE_SECONDS);
        let updated = transaction.execute(
            r#"UPDATE connector_revocations
               SET claim_id = ?2, claim_expires_at = ?3, updated_at = ?4
               WHERE id = ?1 AND phase = ?5 AND ticket_revision = ?6
                 AND next_action_at IS NOT NULL AND next_action_at <= ?4
                 AND (claim_id IS NULL OR claim_expires_at IS NULL OR claim_expires_at <= ?4)"#,
            params![
                ticket.id().to_string(),
                claim_id.to_string(),
                expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                now_text,
                ticket.phase().as_str(),
                as_i64(ticket.revision(), "connector revocation revision")?,
            ],
        )?;
        if updated != 1 {
            return Ok(None);
        }
        transaction.commit()?;
        Ok(Some(ConnectorRevocationClaim {
            claim_id,
            ticket,
            kind,
            attempt_count: match kind {
                ConnectorRevocationClaimKind::Remote => remote_attempt_count,
                ConnectorRevocationClaimKind::FinalizeLocal => local_attempt_count,
            },
            claim_expires_at: expires_at,
        }))
    }

    fn claim_due_connector_revocations_by_kind(
        &self,
        now: DateTime<Utc>,
        limit: usize,
        kind: ConnectorRevocationClaimKind,
    ) -> EventStoreResult<Vec<ConnectorRevocationClaim>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let phases: &[&str] = match kind {
            ConnectorRevocationClaimKind::Remote => &["pending_remote", "retry_scheduled"],
            ConnectorRevocationClaimKind::FinalizeLocal => &["remote_confirmed"],
        };
        let now_text = now.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let scan_limit = i64::try_from(limit.saturating_mul(8)).map_err(|_| {
            EventStoreError::InvalidState(
                "connector revocation claim limit is too large".to_string(),
            )
        })?;
        let second_phase = phases.get(1).copied().unwrap_or(phases[0]);
        let mut claims = Vec::new();
        loop {
            let mut statement = self.conn.prepare(
                r#"SELECT id FROM connector_revocations
                   WHERE phase IN (?1, ?2) AND quarantine_code IS NULL
                     AND next_action_at IS NOT NULL AND next_action_at <= ?3
                     AND (claim_id IS NULL OR claim_expires_at IS NULL OR claim_expires_at <= ?3)
                   ORDER BY next_action_at ASC, remote_attempt_count ASC,
                            local_attempt_count ASC, rowid ASC
                   LIMIT ?4"#,
            )?;
            let ids = statement
                .query_map(
                    params![phases[0], second_phase, now_text, scan_limit],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?;
            drop(statement);
            if ids.is_empty() {
                break;
            }
            let mut progressed = false;
            for raw_id in ids {
                let id = match Uuid::parse_str(&raw_id) {
                    Ok(id) => id,
                    Err(_) => {
                        progressed |=
                            self.quarantine_due_connector_revocation(&raw_id, phases, now)?;
                        continue;
                    }
                };
                match self.claim_connector_revocation(id, kind, now) {
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
                        progressed |=
                            self.quarantine_due_connector_revocation(&raw_id, phases, now)?;
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

    fn quarantine_due_connector_revocation(
        &self,
        id: &str,
        phases: &[&str],
        now: DateTime<Utc>,
    ) -> EventStoreResult<bool> {
        let now_text = now.to_rfc3339_opts(SecondsFormat::Nanos, true);
        let second_phase = phases.get(1).copied().unwrap_or(phases[0]);
        let changed = self.conn.execute(
            r#"UPDATE connector_revocations
               SET quarantine_code = 'invalid_projection_binding', quarantined_at = ?4
               WHERE id = ?1 AND phase IN (?2, ?3) AND quarantine_code IS NULL
                 AND next_action_at IS NOT NULL AND next_action_at <= ?4
                 AND (claim_id IS NULL OR claim_expires_at IS NULL OR claim_expires_at <= ?4)"#,
            params![id, phases[0], second_phase, now_text],
        )?;
        Ok(changed == 1)
    }

    pub(crate) fn claim_due_connector_revocations(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<ConnectorRevocationClaim>> {
        self.claim_due_connector_revocations_by_kind(
            now,
            limit,
            ConnectorRevocationClaimKind::Remote,
        )
    }

    pub(crate) fn claim_due_connector_revocation_finalizations(
        &self,
        now: DateTime<Utc>,
        limit: usize,
    ) -> EventStoreResult<Vec<ConnectorRevocationClaim>> {
        self.claim_due_connector_revocations_by_kind(
            now,
            limit,
            ConnectorRevocationClaimKind::FinalizeLocal,
        )
    }

    pub(crate) fn defer_claimed_connector_revocation_before_remote_call(
        &self,
        claim: &ConnectorRevocationClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if claim.kind() != ConnectorRevocationClaimKind::Remote
            || !matches!(
                claim.ticket().phase(),
                ConnectorRevocationPhase::PendingRemote | ConnectorRevocationPhase::RetryScheduled
            )
        {
            return Err(EventStoreError::InvalidState(
                "connector revocation claim cannot be deferred before a remote call".to_string(),
            ));
        }
        let next = now + revocation_backoff(claim.attempt_count.saturating_add(1));
        let attempt_count = claim.attempt_count.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState(
                "connector revocation scheduling attempt count overflowed".to_string(),
            )
        })?;
        let updated = self.conn.execute(
            r#"UPDATE connector_revocations
               SET claim_id = NULL, claim_expires_at = NULL, next_action_at = ?3,
                   remote_attempt_count = ?4, updated_at = ?5
               WHERE id = ?1 AND claim_id = ?2 AND phase = ?6
                 AND ticket_revision = ?7 AND claim_expires_at > ?5"#,
            params![
                claim.ticket().id().to_string(),
                claim.claim_id().to_string(),
                next.to_rfc3339_opts(SecondsFormat::Nanos, true),
                i64::from(attempt_count),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                claim.ticket().phase().as_str(),
                as_i64(claim.ticket().revision(), "connector revocation revision")?,
            ],
        )?;
        if updated != 1 {
            return Err(EventStoreError::InvalidState(
                "connector revocation claim was lost before remote execution".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn start_claimed_connector_revocation_remote_call(
        &self,
        claim: &ConnectorRevocationClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorRevocationClaim> {
        if claim.kind() != ConnectorRevocationClaimKind::Remote
            || !matches!(
                claim.ticket().phase(),
                ConnectorRevocationPhase::PendingRemote | ConnectorRevocationPhase::RetryScheduled
            )
        {
            return Err(EventStoreError::InvalidState(
                "connector revocation claim cannot start a remote call".to_string(),
            ));
        }
        let transaction = self.conn.unchecked_transaction()?;
        validate_current_ticket(&transaction, claim.ticket())?;
        let mut ticket = claim.ticket().clone();
        ticket.remote_attempt_id = Some(Uuid::new_v4());
        ticket
            .transition(ConnectorRevocationPhase::RemoteCallStarted, now)
            .map_err(EventStoreError::InvalidState)?;
        let attempt_count = claim.attempt_count.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState(
                "connector revocation attempt count overflowed".to_string(),
            )
        })?;
        let expires_at = now + Duration::seconds(CONNECTOR_REVOCATION_LEASE_SECONDS);
        let changed = transaction.execute(
            r#"UPDATE connector_revocations
               SET ticket_json = ?3, phase = ?4, ticket_revision = ?5,
                   remote_attempt_count = ?6, claim_expires_at = ?7,
                   next_action_at = NULL, updated_at = ?8
               WHERE id = ?1 AND claim_id = ?2 AND phase = ?9
                 AND ticket_revision = ?10 AND claim_expires_at > ?8"#,
            params![
                ticket.id().to_string(),
                claim.claim_id().to_string(),
                serde_json::to_string(&ticket)?,
                ticket.phase().as_str(),
                as_i64(ticket.revision(), "connector revocation revision")?,
                i64::from(attempt_count),
                expires_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                claim.ticket().phase().as_str(),
                as_i64(claim.ticket().revision(), "connector revocation revision")?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector revocation claim was lost before the remote call".to_string(),
            ));
        }
        persist_event(
            &transaction,
            "connector.revocation.remote_call_started",
            &receipt(&ticket, None, None, now),
        )?;
        transaction.commit()?;
        Ok(ConnectorRevocationClaim {
            claim_id: claim.claim_id(),
            ticket,
            kind: ConnectorRevocationClaimKind::Remote,
            attempt_count,
            claim_expires_at: expires_at,
        })
    }

    pub(crate) fn record_claimed_connector_revocation_outcome(
        &self,
        claim: &ConnectorRevocationClaim,
        outcome: ConnectorRemoteRevocationOutcome,
        now: DateTime<Utc>,
    ) -> EventStoreResult<Option<ConnectorRevocationClaim>> {
        if claim.kind() != ConnectorRevocationClaimKind::Remote
            || claim.ticket().phase() != ConnectorRevocationPhase::RemoteCallStarted
        {
            return Err(EventStoreError::InvalidState(
                "connector revocation outcome requires a started remote call".to_string(),
            ));
        }
        let transaction = self.conn.unchecked_transaction()?;
        validate_current_ticket(&transaction, claim.ticket())?;
        let mut ticket = claim.ticket().clone();
        let (next_phase, next_action_at, keep_claim) = match outcome {
            ConnectorRemoteRevocationOutcome::Revoked
            | ConnectorRemoteRevocationOutcome::AlreadyRevoked => {
                (ConnectorRevocationPhase::RemoteConfirmed, Some(now), true)
            }
            ConnectorRemoteRevocationOutcome::KnownNotApplied => (
                ConnectorRevocationPhase::RetryScheduled,
                Some(now + revocation_backoff(claim.attempt_count)),
                false,
            ),
            ConnectorRemoteRevocationOutcome::Uncertain => (
                ConnectorRevocationPhase::ReconciliationRequired,
                None,
                false,
            ),
        };
        ticket
            .transition(next_phase, now)
            .map_err(EventStoreError::InvalidState)?;
        let changed = transaction.execute(
            r#"UPDATE connector_revocations
               SET ticket_json = ?3, phase = ?4, ticket_revision = ?5,
                   claim_id = CASE WHEN ?6 THEN claim_id ELSE NULL END,
                   claim_expires_at = CASE WHEN ?6 THEN claim_expires_at ELSE NULL END,
                   next_action_at = ?7, updated_at = ?8
               WHERE id = ?1 AND claim_id = ?2 AND phase = ?9
                 AND ticket_revision = ?10 AND claim_expires_at > ?8"#,
            params![
                ticket.id().to_string(),
                claim.claim_id().to_string(),
                serde_json::to_string(&ticket)?,
                ticket.phase().as_str(),
                as_i64(ticket.revision(), "connector revocation revision")?,
                keep_claim,
                next_action_at.map(|value| value.to_rfc3339_opts(SecondsFormat::Nanos, true)),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                claim.ticket().phase().as_str(),
                as_i64(claim.ticket().revision(), "connector revocation revision")?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector revocation outcome lost its fenced claim".to_string(),
            ));
        }
        persist_event(
            &transaction,
            match next_phase {
                ConnectorRevocationPhase::RemoteConfirmed => {
                    "connector.revocation.remote_confirmed"
                }
                ConnectorRevocationPhase::RetryScheduled => "connector.revocation.retry_scheduled",
                ConnectorRevocationPhase::ReconciliationRequired => {
                    "connector.revocation.reconciliation_required"
                }
                _ => unreachable!(),
            },
            &receipt(&ticket, Some(outcome), None, now),
        )?;
        transaction.commit()?;
        Ok(keep_claim.then_some(ConnectorRevocationClaim {
            claim_id: claim.claim_id(),
            ticket,
            kind: ConnectorRevocationClaimKind::FinalizeLocal,
            attempt_count: 0,
            claim_expires_at: claim.claim_expires_at(),
        }))
    }

    pub(crate) fn defer_claimed_connector_revocation_finalization(
        &self,
        claim: &ConnectorRevocationClaim,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if claim.kind() != ConnectorRevocationClaimKind::FinalizeLocal
            || claim.ticket().phase() != ConnectorRevocationPhase::RemoteConfirmed
        {
            return Err(EventStoreError::InvalidState(
                "connector revocation claim cannot defer local finalization".to_string(),
            ));
        }
        let attempt_count = claim.attempt_count.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState(
                "connector revocation local attempt count overflowed".to_string(),
            )
        })?;
        let next = now + revocation_backoff(attempt_count);
        let changed = self.conn.execute(
            r#"UPDATE connector_revocations
               SET claim_id = NULL, claim_expires_at = NULL, next_action_at = ?3,
                   local_attempt_count = ?4, updated_at = ?5
               WHERE id = ?1 AND claim_id = ?2 AND phase = 'remote_confirmed'
                 AND ticket_revision = ?6 AND claim_expires_at > ?5"#,
            params![
                claim.ticket().id().to_string(),
                claim.claim_id().to_string(),
                next.to_rfc3339_opts(SecondsFormat::Nanos, true),
                i64::from(attempt_count),
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                as_i64(claim.ticket().revision(), "connector revocation revision")?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector revocation finalization claim was lost".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn complete_claimed_connector_revocation(
        &self,
        claim: &ConnectorRevocationClaim,
        delete_outcome: ConnectorCredentialDeleteOutcome,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorAccount> {
        if claim.kind() != ConnectorRevocationClaimKind::FinalizeLocal
            || claim.ticket().phase() != ConnectorRevocationPhase::RemoteConfirmed
        {
            return Err(EventStoreError::InvalidState(
                "connector revocation completion requires remote confirmation".to_string(),
            ));
        }
        let transaction = self.conn.unchecked_transaction()?;
        validate_current_ticket(&transaction, claim.ticket())?;
        let (mut current, generation) =
            read_current_account(&transaction, claim.ticket().account().id)?;
        if generation != claim.ticket().generation() {
            return Err(EventStoreError::InvalidState(
                "connector revocation generation changed before completion".to_string(),
            ));
        }
        let mut ticket = claim.ticket().clone();
        ticket
            .transition(ConnectorRevocationPhase::Completed, now)
            .map_err(EventStoreError::InvalidState)?;
        current.health = ConnectorHealth::Disconnected;
        current.updated_at = now;
        let account_changed = transaction.execute(
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
                serde_json::to_string(&ConnectorHealth::RevocationPending)?,
                as_i64(ticket.generation(), "connector revocation generation")?,
            ],
        )?;
        if account_changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector revocation account completion raced".to_string(),
            ));
        }
        let ticket_changed = transaction.execute(
            r#"UPDATE connector_revocations
               SET ticket_json = ?3, phase = 'completed', ticket_revision = ?4,
                   claim_id = NULL, claim_expires_at = NULL, next_action_at = NULL,
                   updated_at = ?5
               WHERE id = ?1 AND claim_id = ?2 AND phase = 'remote_confirmed'
                 AND ticket_revision = ?6 AND claim_expires_at > ?5"#,
            params![
                ticket.id().to_string(),
                claim.claim_id().to_string(),
                serde_json::to_string(&ticket)?,
                as_i64(ticket.revision(), "connector revocation revision")?,
                now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                as_i64(claim.ticket().revision(), "connector revocation revision")?,
            ],
        )?;
        if ticket_changed != 1 {
            return Err(EventStoreError::InvalidState(
                "connector revocation completion lost its claim".to_string(),
            ));
        }
        persist_event(
            &transaction,
            "connector.revocation.completed",
            &receipt(&ticket, None, Some(delete_outcome), now),
        )?;
        transaction.commit()?;
        Ok(current)
    }

    pub(crate) fn reset_abandoned_connector_revocation_claims(
        &self,
        now: DateTime<Utc>,
    ) -> EventStoreResult<usize> {
        let transaction = self.conn.unchecked_transaction()?;
        let mut statement = transaction.prepare(
            r#"SELECT id FROM connector_revocations
               WHERE claim_id IS NOT NULL OR phase = 'remote_call_started'"#,
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut reset = 0usize;
        for id in ids {
            let id = match Uuid::parse_str(&id) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let (mut ticket, _, _) = match read_ticket_by_id(&transaction, id) {
                Ok(row) => row,
                Err(EventStoreError::InvalidState(_))
                | Err(EventStoreError::NotFound(_))
                | Err(EventStoreError::Json(_))
                | Err(EventStoreError::Uuid(_))
                | Err(EventStoreError::Timestamp(_)) => continue,
                Err(error) => return Err(error),
            };
            if ticket.phase() == ConnectorRevocationPhase::RemoteCallStarted {
                ticket
                    .transition(ConnectorRevocationPhase::ReconciliationRequired, now)
                    .map_err(EventStoreError::InvalidState)?;
                let changed = transaction.execute(
                    r#"UPDATE connector_revocations
                       SET ticket_json = ?2, phase = 'reconciliation_required',
                           ticket_revision = ?3, claim_id = NULL,
                           claim_expires_at = NULL, next_action_at = NULL, updated_at = ?4
                       WHERE id = ?1 AND phase = 'remote_call_started'"#,
                    params![
                        ticket.id().to_string(),
                        serde_json::to_string(&ticket)?,
                        as_i64(ticket.revision(), "connector revocation revision")?,
                        now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    ],
                )?;
                if changed == 1 {
                    persist_event(
                        &transaction,
                        "connector.revocation.reconciliation_required",
                        &receipt(
                            &ticket,
                            Some(ConnectorRemoteRevocationOutcome::Uncertain),
                            None,
                            now,
                        ),
                    )?;
                    reset += 1;
                }
            } else {
                reset += transaction.execute(
                    r#"UPDATE connector_revocations
                       SET claim_id = NULL, claim_expires_at = NULL, updated_at = ?2
                       WHERE id = ?1 AND claim_id IS NOT NULL"#,
                    params![
                        ticket.id().to_string(),
                        now.to_rfc3339_opts(SecondsFormat::Nanos, true),
                    ],
                )?;
            }
        }
        transaction.commit()?;
        Ok(reset)
    }

    #[cfg(test)]
    pub(crate) fn connector_revocation_ticket(
        &self,
        id: Uuid,
    ) -> EventStoreResult<ConnectorRevocationTicket> {
        let transaction = self.conn.unchecked_transaction()?;
        let (ticket, _, _) = read_ticket_by_id(&transaction, id)?;
        transaction.commit()?;
        Ok(ticket)
    }
}
