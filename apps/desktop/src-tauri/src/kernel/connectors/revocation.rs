use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    ConnectorAccount, ConnectorCredentialDeleteOutcome, ConnectorCredentialStore, ConnectorRuntime,
    ConnectorSecret,
};
use crate::kernel::event_store::{EventStore, EventStoreResult};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectorRevocationPhase {
    PendingRemote,
    RemoteCallStarted,
    RetryScheduled,
    RemoteConfirmed,
    ReconciliationRequired,
    Completed,
}

impl ConnectorRevocationPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PendingRemote => "pending_remote",
            Self::RemoteCallStarted => "remote_call_started",
            Self::RetryScheduled => "retry_scheduled",
            Self::RemoteConfirmed => "remote_confirmed",
            Self::ReconciliationRequired => "reconciliation_required",
            Self::Completed => "completed",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectorRemoteRevocationOutcome {
    Revoked,
    AlreadyRevoked,
    KnownNotApplied,
    Uncertain,
}

impl ConnectorRemoteRevocationOutcome {
    pub(crate) fn confirms_revocation(self) -> bool {
        matches!(self, Self::Revoked | Self::AlreadyRevoked)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ConnectorRevocationTicket {
    pub(crate) id: Uuid,
    pub(crate) account: ConnectorAccount,
    pub(crate) generation: u64,
    pub(crate) phase: ConnectorRevocationPhase,
    pub(crate) revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) remote_attempt_id: Option<Uuid>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

impl ConnectorRevocationTicket {
    pub(crate) fn new(account: ConnectorAccount, generation: u64, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            account,
            generation,
            phase: ConnectorRevocationPhase::PendingRemote,
            revision: 0,
            remote_attempt_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    pub(crate) fn account(&self) -> &ConnectorAccount {
        &self.account
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn phase(&self) -> ConnectorRevocationPhase {
        self.phase
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn remote_attempt_id(&self) -> Option<Uuid> {
        self.remote_attempt_id
    }

    pub(crate) fn transition(
        &mut self,
        phase: ConnectorRevocationPhase,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "connector revocation revision overflowed".to_string())?;
        self.phase = phase;
        self.updated_at = now;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectorRevocationClaimKind {
    Remote,
    FinalizeLocal,
}

#[derive(Clone, Debug)]
pub(crate) struct ConnectorRevocationClaim {
    pub(crate) claim_id: Uuid,
    pub(crate) ticket: ConnectorRevocationTicket,
    pub(crate) kind: ConnectorRevocationClaimKind,
    pub(crate) attempt_count: u32,
    pub(crate) claim_expires_at: DateTime<Utc>,
}

impl ConnectorRevocationClaim {
    pub(crate) fn claim_id(&self) -> Uuid {
        self.claim_id
    }

    pub(crate) fn ticket(&self) -> &ConnectorRevocationTicket {
        &self.ticket
    }

    pub(crate) fn kind(&self) -> ConnectorRevocationClaimKind {
        self.kind
    }

    pub(crate) fn claim_expires_at(&self) -> DateTime<Utc> {
        self.claim_expires_at
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ConnectorRevocationReceipt {
    pub(crate) revocation_id: Uuid,
    pub(crate) account_id: Uuid,
    pub(crate) provider_id: String,
    pub(crate) generation: u64,
    pub(crate) phase: ConnectorRevocationPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) remote_outcome: Option<ConnectorRemoteRevocationOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) credential_delete_outcome: Option<ConnectorCredentialDeleteOutcome>,
    pub(crate) changed_at: DateTime<Utc>,
}

pub(crate) trait ConnectorRevocationProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;

    fn revoke_credential(
        &self,
        current: &ConnectorSecret,
        revocation_id: Uuid,
        attempt_id: Uuid,
    ) -> Result<ConnectorRemoteRevocationOutcome, String>;
}

pub(crate) trait ConnectorRevocationRegistry: Send + Sync {
    fn provider(&self, provider_id: &str) -> Option<&dyn ConnectorRevocationProvider>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ConnectorRevocationSweep {
    pub(crate) remote_claimed: usize,
    pub(crate) finalization_claimed: usize,
    pub(crate) remote_confirmed: usize,
    pub(crate) retry_scheduled: usize,
    pub(crate) reconciliation_required: usize,
    pub(crate) completed: usize,
    pub(crate) deferred: usize,
    pub(crate) lost_claim: usize,
}

fn finalize_claimed_revocation<S: ConnectorCredentialStore + Send>(
    store: &EventStore,
    runtime: &ConnectorRuntime<S>,
    claim: &ConnectorRevocationClaim,
    now: DateTime<Utc>,
    sweep: &mut ConnectorRevocationSweep,
) {
    match runtime.delete_account_credential(claim.ticket().account()) {
        Ok(outcome) => {
            if store
                .complete_claimed_connector_revocation(claim, outcome, now)
                .is_ok()
            {
                sweep.completed += 1;
            } else {
                sweep.lost_claim += 1;
            }
        }
        Err(_) => {
            if store
                .defer_claimed_connector_revocation_finalization(claim, now)
                .is_ok()
            {
                sweep.deferred += 1;
            } else {
                sweep.lost_claim += 1;
            }
        }
    }
}

pub(crate) fn revoke_due_connector_accounts<S: ConnectorCredentialStore + Send>(
    store: &EventStore,
    runtime: &ConnectorRuntime<S>,
    registry: &dyn ConnectorRevocationRegistry,
    now: DateTime<Utc>,
    limit: usize,
) -> EventStoreResult<ConnectorRevocationSweep> {
    let remote_claims = store.claim_due_connector_revocations(now, limit)?;
    let mut sweep = ConnectorRevocationSweep {
        remote_claimed: remote_claims.len(),
        ..ConnectorRevocationSweep::default()
    };

    for claim in remote_claims {
        let Some(provider) = registry.provider(&claim.ticket().account().provider_id) else {
            if store
                .defer_claimed_connector_revocation_before_remote_call(&claim, now)
                .is_ok()
            {
                sweep.deferred += 1;
            } else {
                sweep.lost_claim += 1;
            }
            continue;
        };
        if provider.provider_id() != claim.ticket().account().provider_id {
            if store
                .defer_claimed_connector_revocation_before_remote_call(&claim, now)
                .is_ok()
            {
                sweep.deferred += 1;
            } else {
                sweep.lost_claim += 1;
            }
            continue;
        }

        let started = match store.start_claimed_connector_revocation_remote_call(&claim, now) {
            Ok(started) => started,
            Err(_) => {
                sweep.lost_claim += 1;
                continue;
            }
        };
        let attempt_id = started
            .ticket()
            .remote_attempt_id()
            .expect("persisted remote-call checkpoint has an attempt id");
        let outcome = runtime
            .with_account_credential(&started.ticket().account().credential_handle, |current| {
                provider
                    .revoke_credential(&current, started.ticket().id(), attempt_id)
                    .map(|outcome| (outcome, None))
            })
            .unwrap_or(ConnectorRemoteRevocationOutcome::Uncertain);

        match store.record_claimed_connector_revocation_outcome(&started, outcome, now) {
            Ok(Some(finalization)) => {
                sweep.remote_confirmed += 1;
                finalize_claimed_revocation(store, runtime, &finalization, now, &mut sweep);
            }
            Ok(None) if outcome == ConnectorRemoteRevocationOutcome::KnownNotApplied => {
                sweep.retry_scheduled += 1;
            }
            Ok(None) => {
                sweep.reconciliation_required += 1;
            }
            Err(_) => sweep.lost_claim += 1,
        }
    }

    let finalizations = store.claim_due_connector_revocation_finalizations(now, limit)?;
    sweep.finalization_claimed = finalizations.len();
    for claim in finalizations {
        finalize_claimed_revocation(store, runtime, &claim, now, &mut sweep);
    }
    Ok(sweep)
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use chrono::Duration;
    use tempfile::TempDir;

    use super::*;
    use crate::kernel::connectors::{
        ConnectorCapability, ConnectorCredentialHandle, ConnectorHealth,
        ConnectorRecoveryExternalEffectState, ConnectorRecoveryNextStepCode,
    };

    #[derive(Default)]
    struct SharedCredentialState {
        secrets: Mutex<HashMap<ConnectorCredentialHandle, String>>,
        fail_delete: AtomicBool,
    }

    struct TestCredentialStore {
        state: Arc<SharedCredentialState>,
    }

    impl ConnectorCredentialStore for TestCredentialStore {
        fn put_at(
            &mut self,
            handle: &ConnectorCredentialHandle,
            secret: ConnectorSecret,
        ) -> Result<(), String> {
            self.state
                .secrets
                .lock()
                .map_err(|_| "test credential state failed".to_string())?
                .insert(handle.clone(), secret.expose().to_string());
            Ok(())
        }

        fn read(&self, handle: &ConnectorCredentialHandle) -> Result<ConnectorSecret, String> {
            let value = self
                .state
                .secrets
                .lock()
                .map_err(|_| "test credential state failed".to_string())?
                .get(handle)
                .cloned()
                .ok_or_else(|| "connector credential is unavailable".to_string())?;
            ConnectorSecret::new(value)
        }

        fn replace(
            &mut self,
            handle: &ConnectorCredentialHandle,
            secret: ConnectorSecret,
        ) -> Result<(), String> {
            let mut secrets = self
                .state
                .secrets
                .lock()
                .map_err(|_| "test credential state failed".to_string())?;
            if !secrets.contains_key(handle) {
                return Err("connector credential is unavailable".to_string());
            }
            secrets.insert(handle.clone(), secret.expose().to_string());
            Ok(())
        }

        fn delete(
            &mut self,
            handle: &ConnectorCredentialHandle,
        ) -> Result<ConnectorCredentialDeleteOutcome, String> {
            if self.state.fail_delete.load(Ordering::SeqCst) {
                return Err("test delete marker must not be persisted".to_string());
            }
            Ok(
                if self
                    .state
                    .secrets
                    .lock()
                    .map_err(|_| "test credential state failed".to_string())?
                    .remove(handle)
                    .is_some()
                {
                    ConnectorCredentialDeleteOutcome::Deleted
                } else {
                    ConnectorCredentialDeleteOutcome::AlreadyAbsent
                },
            )
        }

        fn contains(&self, handle: &ConnectorCredentialHandle) -> bool {
            self.state
                .secrets
                .lock()
                .map(|secrets| secrets.contains_key(handle))
                .unwrap_or(false)
        }
    }

    struct Fixture {
        _temp_dir: TempDir,
        path: PathBuf,
        store: EventStore,
        runtime: ConnectorRuntime<TestCredentialStore>,
        credentials: Arc<SharedCredentialState>,
        account: ConnectorAccount,
        now: DateTime<Utc>,
    }

    fn fixture() -> Fixture {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("connector-revocation.sqlite3");
        let store = EventStore::open(&path).expect("event store opens");
        let credentials = Arc::new(SharedCredentialState::default());
        let mut credential_store = TestCredentialStore {
            state: Arc::clone(&credentials),
        };
        let handle = credential_store
            .put(ConnectorSecret::new("revocation-token-marker".to_string()).unwrap())
            .expect("credential stores");
        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "fake".to_string(),
            display_name: "Revocation fixture".to_string(),
            tenant_ref: Some("tenant:fixture".to_string()),
            credential_handle: handle,
            granted_capabilities: vec![ConnectorCapability::MailSearch],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        Fixture {
            _temp_dir: temp_dir,
            path,
            store,
            runtime: ConnectorRuntime::new(credential_store),
            credentials,
            account,
            now,
        }
    }

    struct ScriptedProvider {
        outcomes: Mutex<VecDeque<Result<ConnectorRemoteRevocationOutcome, String>>>,
        calls: AtomicUsize,
        observed_started_checkpoint: AtomicBool,
        store_path: PathBuf,
    }

    impl ScriptedProvider {
        fn new(
            store_path: PathBuf,
            outcomes: impl IntoIterator<Item = Result<ConnectorRemoteRevocationOutcome, String>>,
        ) -> Self {
            Self {
                outcomes: Mutex::new(outcomes.into_iter().collect()),
                calls: AtomicUsize::new(0),
                observed_started_checkpoint: AtomicBool::new(false),
                store_path,
            }
        }
    }

    impl ConnectorRevocationProvider for ScriptedProvider {
        fn provider_id(&self) -> &'static str {
            "fake"
        }

        fn revoke_credential(
            &self,
            current: &ConnectorSecret,
            revocation_id: Uuid,
            attempt_id: Uuid,
        ) -> Result<ConnectorRemoteRevocationOutcome, String> {
            assert_eq!(current.expose(), "revocation-token-marker");
            assert_ne!(attempt_id, Uuid::nil());
            let persisted = EventStore::open(&self.store_path)
                .expect("provider can inspect committed checkpoint")
                .connector_revocation_ticket(revocation_id)
                .expect("started ticket is durable");
            self.observed_started_checkpoint.store(
                persisted.phase() == ConnectorRevocationPhase::RemoteCallStarted
                    && persisted.remote_attempt_id() == Some(attempt_id),
                Ordering::SeqCst,
            );
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.outcomes
                .lock()
                .map_err(|_| "provider script failed".to_string())?
                .pop_front()
                .unwrap_or(Ok(ConnectorRemoteRevocationOutcome::Uncertain))
        }
    }

    struct SingleRegistry<'a>(&'a dyn ConnectorRevocationProvider);

    impl ConnectorRevocationRegistry for SingleRegistry<'_> {
        fn provider(&self, provider_id: &str) -> Option<&dyn ConnectorRevocationProvider> {
            (provider_id == self.0.provider_id()).then_some(self.0)
        }
    }

    struct EmptyRegistry;

    impl ConnectorRevocationRegistry for EmptyRegistry {
        fn provider(&self, _provider_id: &str) -> Option<&dyn ConnectorRevocationProvider> {
            None
        }
    }

    #[test]
    fn begin_revocation_fences_account_generation_and_blocks_disconnect() {
        let fixture = fixture();
        let ticket = fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        assert_eq!(ticket.phase(), ConnectorRevocationPhase::PendingRemote);
        assert_eq!(ticket.generation(), 1);
        let current = fixture
            .store
            .list_connector_accounts()
            .expect("account reloads")
            .pop()
            .expect("account exists");
        assert_eq!(current.health, ConnectorHealth::RevocationPending);
        assert!(fixture
            .store
            .begin_connector_disconnect(current.id, fixture.now)
            .is_err());
        assert_eq!(
            fixture
                .store
                .begin_connector_revocation(current.id, fixture.now)
                .expect("begin is idempotent")
                .id(),
            ticket.id()
        );
    }

    #[test]
    fn confirmed_remote_revocation_deletes_credential_and_completes_once() {
        let fixture = fixture();
        let ticket = fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        let provider = ScriptedProvider::new(
            fixture.path.clone(),
            [Ok(ConnectorRemoteRevocationOutcome::Revoked)],
        );
        let sweep = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &SingleRegistry(&provider),
            fixture.now,
            1,
        )
        .expect("revocation sweep succeeds");
        assert_eq!(sweep.remote_claimed, 1);
        assert_eq!(sweep.remote_confirmed, 1);
        assert_eq!(sweep.completed, 1);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert!(provider.observed_started_checkpoint.load(Ordering::SeqCst));
        assert!(!fixture
            .credentials
            .secrets
            .lock()
            .unwrap()
            .contains_key(&fixture.account.credential_handle));
        assert_eq!(
            fixture
                .store
                .connector_revocation_ticket(ticket.id())
                .expect("ticket reloads")
                .phase(),
            ConnectorRevocationPhase::Completed
        );
        assert_eq!(
            fixture
                .store
                .list_connector_accounts()
                .expect("account reloads")[0]
                .health,
            ConnectorHealth::Disconnected
        );
        let second = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &SingleRegistry(&provider),
            fixture.now + Duration::minutes(1),
            1,
        )
        .expect("second sweep is a no-op");
        assert_eq!(second.remote_claimed, 0);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn provider_error_becomes_uncertain_without_persisting_raw_body_or_replaying() {
        let fixture = fixture();
        let marker = format!("secret-provider-body:{}", Uuid::new_v4());
        let ticket = fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        let provider = ScriptedProvider::new(fixture.path.clone(), [Err(marker.clone())]);
        let sweep = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &SingleRegistry(&provider),
            fixture.now,
            1,
        )
        .expect("provider error is handled");
        assert_eq!(sweep.reconciliation_required, 1);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert!(fixture
            .credentials
            .secrets
            .lock()
            .unwrap()
            .contains_key(&fixture.account.credential_handle));
        assert_eq!(
            fixture
                .store
                .connector_revocation_ticket(ticket.id())
                .expect("ticket remains visible")
                .phase(),
            ConnectorRevocationPhase::ReconciliationRequired
        );
        let second = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &SingleRegistry(&provider),
            fixture.now + Duration::days(1),
            1,
        )
        .expect("uncertain state is not replayed");
        assert_eq!(second.remote_claimed, 0);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        drop(fixture.store);
        let sqlite = String::from_utf8_lossy(&std::fs::read(&fixture.path).unwrap()).into_owned();
        assert!(!sqlite.contains(&marker));
    }

    #[test]
    fn missing_provider_defers_before_remote_call_checkpoint() {
        let fixture = fixture();
        let ticket = fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        let sweep = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &EmptyRegistry,
            fixture.now,
            1,
        )
        .expect("missing provider is safely deferred");
        assert_eq!(sweep.remote_claimed, 1);
        assert_eq!(sweep.deferred, 1);
        let persisted = fixture
            .store
            .connector_revocation_ticket(ticket.id())
            .expect("ticket reloads");
        assert_eq!(persisted.phase(), ConnectorRevocationPhase::PendingRemote);
        assert_eq!(persisted.remote_attempt_id(), None);
        assert!(fixture
            .store
            .claim_due_connector_revocations(fixture.now + Duration::seconds(29), 1)
            .expect("early claim query succeeds")
            .is_empty());
    }

    #[test]
    fn known_not_applied_uses_persistent_backoff_before_safe_retry() {
        let fixture = fixture();
        fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        let provider = ScriptedProvider::new(
            fixture.path.clone(),
            [
                Ok(ConnectorRemoteRevocationOutcome::KnownNotApplied),
                Ok(ConnectorRemoteRevocationOutcome::Revoked),
            ],
        );
        let first = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &SingleRegistry(&provider),
            fixture.now,
            1,
        )
        .expect("first sweep records known-not-applied");
        assert_eq!(first.retry_scheduled, 1);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            revoke_due_connector_accounts(
                &fixture.store,
                &fixture.runtime,
                &SingleRegistry(&provider),
                fixture.now + Duration::seconds(29),
                1,
            )
            .expect("early sweep succeeds")
            .remote_claimed,
            0
        );
        let retry = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &SingleRegistry(&provider),
            fixture.now + Duration::seconds(31),
            1,
        )
        .expect("due retry succeeds");
        assert_eq!(retry.completed, 1);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn crash_after_remote_call_started_never_replays_the_remote_revocation() {
        let fixture = fixture();
        let ticket = fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        let claim = fixture
            .store
            .claim_due_connector_revocations(fixture.now, 1)
            .expect("claim succeeds")
            .pop()
            .expect("one revocation is due");
        fixture
            .store
            .start_claimed_connector_revocation_remote_call(&claim, fixture.now)
            .expect("remote-call checkpoint persists");
        fixture
            .store
            .reset_abandoned_connector_revocation_claims(fixture.now + Duration::seconds(1))
            .expect("startup recovery succeeds");
        assert_eq!(
            fixture
                .store
                .connector_revocation_ticket(ticket.id())
                .expect("ticket reloads")
                .phase(),
            ConnectorRevocationPhase::ReconciliationRequired
        );
        let provider = ScriptedProvider::new(
            fixture.path.clone(),
            [Ok(ConnectorRemoteRevocationOutcome::Revoked)],
        );
        let sweep = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &SingleRegistry(&provider),
            fixture.now + Duration::days(1),
            1,
        )
        .expect("uncertain startup state remains actionless");
        assert_eq!(sweep.remote_claimed, 0);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn generation_change_after_remote_effect_fences_local_outcome_commit() {
        let fixture = fixture();
        let ticket = fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        let claim = fixture
            .store
            .claim_due_connector_revocations(fixture.now, 1)
            .expect("claim succeeds")
            .pop()
            .expect("one revocation is due");
        let started = fixture
            .store
            .start_claimed_connector_revocation_remote_call(&claim, fixture.now)
            .expect("remote call starts");
        let provider = ScriptedProvider::new(
            fixture.path.clone(),
            [Ok(ConnectorRemoteRevocationOutcome::Revoked)],
        );
        let attempt_id = started.ticket().remote_attempt_id().unwrap();
        let outcome = fixture
            .runtime
            .with_account_credential(&started.ticket().account().credential_handle, |current| {
                provider
                    .revoke_credential(&current, ticket.id(), attempt_id)
                    .map(|outcome| (outcome, None))
            })
            .expect("remote provider returns a confirmed effect");
        rusqlite::Connection::open(&fixture.path)
            .expect("racing connection opens")
            .execute(
                "UPDATE connector_account_generations SET generation = generation + 1 WHERE account_id = ?1",
                [fixture.account.id.to_string()],
            )
            .expect("account generation changes after remote return");
        assert!(fixture
            .store
            .record_claimed_connector_revocation_outcome(
                &started,
                outcome,
                fixture.now + Duration::seconds(1),
            )
            .is_err());
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            fixture
                .store
                .connector_revocation_ticket(ticket.id())
                .expect("uncertain ticket remains visible")
                .phase(),
            ConnectorRevocationPhase::RemoteCallStarted
        );
    }

    #[test]
    fn expired_claim_takeover_fences_the_old_revocation_worker() {
        let fixture = fixture();
        fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        let second_store = EventStore::open(&fixture.path).expect("second store opens");
        let first = fixture
            .store
            .claim_due_connector_revocations(fixture.now, 1)
            .expect("first claim succeeds")
            .pop()
            .expect("first worker owns claim");
        assert!(second_store
            .claim_due_connector_revocations(fixture.now, 1)
            .expect("second claim query succeeds")
            .is_empty());
        let takeover_at = first.claim_expires_at() + Duration::seconds(1);
        let second = second_store
            .claim_due_connector_revocations(takeover_at, 1)
            .expect("expired claim can be replaced")
            .pop()
            .expect("second worker owns replacement claim");
        assert_ne!(first.claim_id(), second.claim_id());
        assert!(fixture
            .store
            .defer_claimed_connector_revocation_before_remote_call(&first, takeover_at)
            .is_err());
        second_store
            .defer_claimed_connector_revocation_before_remote_call(&second, takeover_at)
            .expect("replacement claim can defer");
    }

    #[test]
    fn remote_confirmation_survives_crash_and_already_absent_local_credential() {
        let fixture = fixture();
        let ticket = fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        let claim = fixture
            .store
            .claim_due_connector_revocations(fixture.now, 1)
            .expect("claim succeeds")
            .pop()
            .expect("one revocation is due");
        let started = fixture
            .store
            .start_claimed_connector_revocation_remote_call(&claim, fixture.now)
            .expect("remote call starts");
        fixture
            .store
            .record_claimed_connector_revocation_outcome(
                &started,
                ConnectorRemoteRevocationOutcome::AlreadyRevoked,
                fixture.now,
            )
            .expect("remote confirmation persists")
            .expect("local finalization retains claim");
        fixture
            .runtime
            .delete_account_credential(ticket.account())
            .expect("credential delete happens before crash");
        fixture
            .store
            .reset_abandoned_connector_revocation_claims(fixture.now + Duration::seconds(1))
            .expect("startup clears old local claim");
        let sweep = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &EmptyRegistry,
            fixture.now + Duration::seconds(1),
            1,
        )
        .expect("local-only recovery completes");
        assert_eq!(sweep.finalization_claimed, 1);
        assert_eq!(sweep.completed, 1);
        assert_eq!(
            fixture
                .store
                .connector_revocation_ticket(ticket.id())
                .expect("ticket reloads")
                .phase(),
            ConnectorRevocationPhase::Completed
        );
    }

    #[test]
    fn local_delete_failure_retries_only_local_finalization() {
        let fixture = fixture();
        let ticket = fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        fixture
            .credentials
            .fail_delete
            .store(true, Ordering::SeqCst);
        let provider = ScriptedProvider::new(
            fixture.path.clone(),
            [Ok(ConnectorRemoteRevocationOutcome::Revoked)],
        );
        let first = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &SingleRegistry(&provider),
            fixture.now,
            1,
        )
        .expect("remote revocation succeeds and local delete defers");
        assert_eq!(first.remote_confirmed, 1);
        assert_eq!(first.deferred, 1);
        assert_eq!(first.completed, 0);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            fixture
                .store
                .connector_revocation_ticket(ticket.id())
                .expect("confirmed ticket remains durable")
                .phase(),
            ConnectorRevocationPhase::RemoteConfirmed
        );
        fixture
            .credentials
            .fail_delete
            .store(false, Ordering::SeqCst);
        let recovered = revoke_due_connector_accounts(
            &fixture.store,
            &fixture.runtime,
            &EmptyRegistry,
            fixture.now + Duration::seconds(31),
            1,
        )
        .expect("local-only retry completes");
        assert_eq!(recovered.remote_claimed, 0);
        assert_eq!(recovered.finalization_claimed, 1);
        assert_eq!(recovered.completed, 1);
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn recovery_projection_exposes_no_ticket_claim_or_credential_handle() {
        let fixture = fixture();
        let ticket = fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .expect("revocation begins");
        let claim = fixture
            .store
            .claim_due_connector_revocations(fixture.now, 1)
            .expect("claim succeeds")
            .pop()
            .expect("claim exists");
        let projection = serde_json::to_string(
            &fixture
                .store
                .list_connector_recovery_items()
                .expect("recovery projection loads"),
        )
        .expect("projection serializes");
        assert!(!projection.contains(&ticket.id().to_string()));
        assert!(!projection.contains(&claim.claim_id().to_string()));
        assert!(!projection.contains(&fixture.account.credential_handle.0));
        assert!(projection.contains("revocation_pending"));
    }

    #[test]
    fn recovery_projection_maps_each_durable_revocation_phase_without_actions() {
        fn assert_projection(
            fixture: &Fixture,
            effect: ConnectorRecoveryExternalEffectState,
            next_step: ConnectorRecoveryNextStepCode,
            forbidden_ids: &[Uuid],
        ) {
            let items = fixture
                .store
                .list_connector_recovery_items()
                .expect("recovery projection loads");
            let item = items
                .iter()
                .find(|item| item.id == fixture.account.id)
                .expect("revocation recovery item exists");
            assert_eq!(item.external_effect_state, effect);
            assert_eq!(item.next_step_code, next_step);
            assert!(item.action.is_none());
            assert!(item.sync_capability.is_none());
            let serialized = serde_json::to_string(item).unwrap();
            for id in forbidden_ids {
                assert!(!serialized.contains(&id.to_string()));
            }
            assert!(!serialized.contains(&fixture.account.credential_handle.0));
            assert!(!serialized.contains("tenant:fixture"));
            assert!(!serialized.contains("fake"));
        }

        let pending = fixture();
        let pending_ticket = pending
            .store
            .begin_connector_revocation(pending.account.id, pending.now)
            .unwrap();
        assert_projection(
            &pending,
            ConnectorRecoveryExternalEffectState::NoExternalWrite,
            ConnectorRecoveryNextStepCode::ReviewAccountConnection,
            &[pending_ticket.id()],
        );

        let started = fixture();
        let started_ticket = started
            .store
            .begin_connector_revocation(started.account.id, started.now)
            .unwrap();
        let started_claim = started
            .store
            .claim_due_connector_revocations(started.now, 1)
            .unwrap()
            .pop()
            .unwrap();
        let started_claim = started
            .store
            .start_claimed_connector_revocation_remote_call(&started_claim, started.now)
            .unwrap();
        let attempt_id = started_claim.ticket().remote_attempt_id().unwrap();
        assert_projection(
            &started,
            ConnectorRecoveryExternalEffectState::ExternalResultUncertain,
            ConnectorRecoveryNextStepCode::VerifyProviderState,
            &[started_ticket.id(), started_claim.claim_id(), attempt_id],
        );

        for (outcome, effect, next_step) in [
            (
                ConnectorRemoteRevocationOutcome::KnownNotApplied,
                ConnectorRecoveryExternalEffectState::NoExternalWrite,
                ConnectorRecoveryNextStepCode::ReviewAccountConnection,
            ),
            (
                ConnectorRemoteRevocationOutcome::Uncertain,
                ConnectorRecoveryExternalEffectState::ExternalResultUncertain,
                ConnectorRecoveryNextStepCode::VerifyProviderState,
            ),
            (
                ConnectorRemoteRevocationOutcome::Revoked,
                ConnectorRecoveryExternalEffectState::LocalCredentialRemovalPending,
                ConnectorRecoveryNextStepCode::WaitForLocalDisconnectRecovery,
            ),
        ] {
            let fixture = fixture();
            let ticket = fixture
                .store
                .begin_connector_revocation(fixture.account.id, fixture.now)
                .unwrap();
            let claim = fixture
                .store
                .claim_due_connector_revocations(fixture.now, 1)
                .unwrap()
                .pop()
                .unwrap();
            let started = fixture
                .store
                .start_claimed_connector_revocation_remote_call(&claim, fixture.now)
                .unwrap();
            let attempt_id = started.ticket().remote_attempt_id().unwrap();
            let _ = fixture
                .store
                .record_claimed_connector_revocation_outcome(&started, outcome, fixture.now)
                .unwrap();
            assert_projection(
                &fixture,
                effect,
                next_step,
                &[ticket.id(), claim.claim_id(), attempt_id],
            );
        }
    }

    #[test]
    fn revocation_kernel_events_redact_private_execution_identifiers() {
        let fixture = fixture();
        let ticket = fixture
            .store
            .begin_connector_revocation(fixture.account.id, fixture.now)
            .unwrap();
        let claim = fixture
            .store
            .claim_due_connector_revocations(fixture.now, 1)
            .unwrap()
            .pop()
            .unwrap();
        let started = fixture
            .store
            .start_claimed_connector_revocation_remote_call(&claim, fixture.now)
            .unwrap();
        let attempt_id = started.ticket().remote_attempt_id().unwrap();
        let events = serde_json::to_string(&fixture.store.list_recent(100).unwrap()).unwrap();
        assert!(events.contains(&ticket.id().to_string()));
        assert!(!events.contains(&claim.claim_id().to_string()));
        assert!(!events.contains(&attempt_id.to_string()));
        assert!(!events.contains(&fixture.account.credential_handle.0));
        assert!(!events.contains("revocation-token-marker"));
        assert!(!events.contains("tenant:fixture"));
    }

    #[test]
    fn revocation_worker_has_no_live_provider_or_uncertain_replay_path() {
        let source = include_str!("revocation.rs");
        let provider_name = ["Micro", "soft"].concat();
        let provider_name_lower = provider_name.to_ascii_lowercase();
        let forbidden_replay = ["ReconciliationRequired", " => ", "provider"].concat();
        assert!(!source.contains(&provider_name));
        assert!(!source.contains(&provider_name_lower));
        assert!(!source.contains(&forbidden_replay));
    }
}
