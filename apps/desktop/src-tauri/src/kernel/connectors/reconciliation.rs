use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};

use super::{ConnectorMutationReconciler, ConnectorReconciliationOutcome};
use crate::kernel::event_store::{EventStore, EventStoreResult};

pub(crate) trait ConnectorReconcilerRegistry: Send + Sync {
    fn reconciler(&self, provider_id: &str) -> Option<&dyn ConnectorMutationReconciler>;

    fn execution_enabled(&self) -> bool {
        false
    }

    fn supports(&self, provider_id: &str, capability: super::ConnectorCapability) -> bool {
        self.reconciler(provider_id).is_some_and(|reconciler| {
            reconciler.provider_id() == provider_id
                && reconciler.capabilities().contains(&capability)
        })
    }
}

pub(crate) fn reconcile_due_connector_mutations_with_shared_store(
    event_store: &Arc<Mutex<EventStore>>,
    registry: &dyn ConnectorReconcilerRegistry,
    limit: usize,
) -> EventStoreResult<ConnectorReconciliationSweep> {
    if !registry.execution_enabled() || limit == 0 {
        return Ok(ConnectorReconciliationSweep::default());
    }
    let claims = event_store
        .lock()
        .map_err(|_| {
            crate::kernel::event_store::EventStoreError::InvalidState(
                "connector reconciliation store is unavailable".to_string(),
            )
        })?
        .claim_due_connector_reconciliations(Utc::now(), limit)?;
    let mut sweep = ConnectorReconciliationSweep {
        claimed: claims.len(),
        ..ConnectorReconciliationSweep::default()
    };
    for mut claim in claims {
        let Some(reconciler) = registry.reconciler(&claim.invocation().provider_id) else {
            let completion_now = Utc::now();
            if event_store
                .lock()
                .map_err(|_| {
                    crate::kernel::event_store::EventStoreError::InvalidState(
                        "connector reconciliation store is unavailable".to_string(),
                    )
                })?
                .defer_connector_reconciliation(&claim, completion_now)
                .is_ok()
            {
                sweep.deferred += 1;
            } else {
                sweep.lost_claim += 1;
            }
            continue;
        };
        let renew_now = Utc::now();
        if reconciler.provider_id() != claim.invocation().provider_id
            || !reconciler
                .capabilities()
                .contains(&claim.invocation().capability)
            || event_store
                .lock()
                .map_err(|_| {
                    crate::kernel::event_store::EventStoreError::InvalidState(
                        "connector reconciliation store is unavailable".to_string(),
                    )
                })?
                .renew_connector_reconciliation_claim(&mut claim, renew_now)
                .is_err()
        {
            sweep.lost_claim += 1;
            continue;
        }
        let outcome = reconciler.reconcile_mutation(claim.account(), claim.invocation());
        let completion_now = Utc::now();
        let store = event_store.lock().map_err(|_| {
            crate::kernel::event_store::EventStoreError::InvalidState(
                "connector reconciliation store is unavailable".to_string(),
            )
        })?;
        match outcome {
            Ok(ConnectorReconciliationOutcome::Applied(receipt)) if receipt.reconciled => {
                if store
                    .complete_claimed_connector_reconciliation(&claim, receipt, completion_now)
                    .is_ok()
                {
                    sweep.completed += 1;
                } else {
                    sweep.lost_claim += 1;
                }
            }
            Ok(ConnectorReconciliationOutcome::KnownNotApplied) => {
                if store
                    .fail_claimed_connector_reconciliation_known_not_applied(&claim, completion_now)
                    .is_ok()
                {
                    sweep.not_applied += 1;
                } else {
                    sweep.lost_claim += 1;
                }
            }
            Ok(ConnectorReconciliationOutcome::Applied(_))
            | Ok(ConnectorReconciliationOutcome::StillUncertain)
            | Err(_) => {
                if store
                    .defer_connector_reconciliation(&claim, completion_now)
                    .is_ok()
                {
                    sweep.deferred += 1;
                } else {
                    sweep.lost_claim += 1;
                }
            }
        }
    }
    Ok(sweep)
}

#[derive(Default)]
pub(crate) struct EmptyConnectorReconcilerRegistry;

impl ConnectorReconcilerRegistry for EmptyConnectorReconcilerRegistry {
    fn reconciler(&self, _provider_id: &str) -> Option<&dyn ConnectorMutationReconciler> {
        None
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ConnectorReconciliationSweep {
    pub claimed: usize,
    pub completed: usize,
    pub not_applied: usize,
    pub deferred: usize,
    pub lost_claim: usize,
}

pub(crate) fn reconcile_due_connector_mutations(
    store: &EventStore,
    registry: &dyn ConnectorReconcilerRegistry,
    now: DateTime<Utc>,
    limit: usize,
) -> EventStoreResult<ConnectorReconciliationSweep> {
    let claims = store.claim_due_connector_reconciliations(now, limit)?;
    let mut sweep = ConnectorReconciliationSweep {
        claimed: claims.len(),
        ..ConnectorReconciliationSweep::default()
    };
    for mut claim in claims {
        let Some(reconciler) = registry.reconciler(&claim.invocation().provider_id) else {
            if store.defer_connector_reconciliation(&claim, now).is_ok() {
                sweep.deferred += 1;
            } else {
                sweep.lost_claim += 1;
            }
            continue;
        };
        if reconciler.provider_id() != claim.invocation().provider_id
            || !reconciler
                .capabilities()
                .contains(&claim.invocation().capability)
            || store
                .renew_connector_reconciliation_claim(&mut claim, now)
                .is_err()
        {
            sweep.lost_claim += 1;
            continue;
        }
        let outcome = reconciler.reconcile_mutation(claim.account(), claim.invocation());
        match outcome {
            Ok(ConnectorReconciliationOutcome::Applied(receipt)) if receipt.reconciled => {
                if store
                    .complete_claimed_connector_reconciliation(&claim, receipt, now)
                    .is_ok()
                {
                    sweep.completed += 1;
                } else {
                    sweep.lost_claim += 1;
                }
            }
            Ok(ConnectorReconciliationOutcome::KnownNotApplied) => {
                if store
                    .fail_claimed_connector_reconciliation_known_not_applied(&claim, now)
                    .is_ok()
                {
                    sweep.not_applied += 1;
                } else {
                    sweep.lost_claim += 1;
                }
            }
            Ok(ConnectorReconciliationOutcome::Applied(_))
            | Ok(ConnectorReconciliationOutcome::StillUncertain)
            | Err(_) => {
                if store.defer_connector_reconciliation(&claim, now).is_ok() {
                    sweep.deferred += 1;
                } else {
                    sweep.lost_claim += 1;
                }
            }
        }
    }
    Ok(sweep)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use chrono::Duration;
    use serde_json::json;
    use tempfile::TempDir;
    use uuid::Uuid;

    use super::*;
    use crate::kernel::automation::{AutomationDefinition, ReviewQueueItem, ReviewQueueItemStatus};
    use crate::kernel::connectors::{
        ConnectorAccount, ConnectorCapability, ConnectorCredentialHandle, ConnectorHealth,
        ConnectorInvocation, ConnectorMutationApplyOutcome, ConnectorMutationProvider,
        ConnectorProvider, ConnectorReconciliationOutcome, ConnectorRecoveryAction,
        ConnectorRecoveryKind, FakeConnectorProvider, FakeConnectorRemoteState,
    };
    use crate::kernel::models::AccessMode;
    use crate::kernel::policy::{request_capability_access, CapabilityKind};
    use crate::kernel::tool_runtime::{
        prepare_tool_execution, ToolExecutionRequest, ToolInvocationRecord,
        CONNECTOR_MUTATE_TOOL_ID,
    };

    struct ReconciliationFixture {
        _temp_dir: TempDir,
        path: PathBuf,
        account: ConnectorAccount,
        remote: Arc<FakeConnectorRemoteState>,
        invocation_id: Uuid,
        now: DateTime<Utc>,
    }

    fn reconciliation_fixture() -> ReconciliationFixture {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("reconciliation-worker.sqlite3");
        let store = EventStore::open(&path).expect("store opens");
        let now = Utc::now();
        let remote = Arc::new(FakeConnectorRemoteState::default());
        let provider = FakeConnectorProvider::with_remote_state(Arc::clone(&remote));
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: provider.provider_id().to_string(),
            display_name: "Reconciliation fixture".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: provider.capabilities().to_vec(),
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let definition = AutomationDefinition::once(
            "Reconcile one uncertain fake mutation".to_string(),
            "UTC".to_string(),
            now - Duration::minutes(1),
        )
        .expect("definition builds");
        store
            .upsert_automation_definition(&definition)
            .expect("definition persists");
        let automation_run_id = store
            .claim_due_automation_run(definition.id, now, "fixture".to_string())
            .expect("run claim succeeds")
            .expect("run is due")
            .id;
        let access_request =
            request_capability_access(AccessMode::FullAccess, CapabilityKind::ConnectorWrite)
                .expect("approval request builds");
        let request = ToolExecutionRequest {
            tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            input: json!({
                "provider_id": "fake",
                "account_id": account.id.to_string(),
                "account_generation": 0,
                "capability": "mail_send_draft",
                "target_ref": "draft:reconciliation-fixture",
                "preview_hash": "sha256:reconciliation-fixture",
                "idempotency_key": "fake:reconciliation-fixture:once",
                "automation_run_id": automation_run_id.to_string()
            }),
            access_mode: AccessMode::FullAccess,
            run_id: Some(Uuid::new_v4()),
        };
        let plan = prepare_tool_execution(&request).expect("tool plan builds");
        let tool = ToolInvocationRecord::waiting_for_confirmation(&plan, access_request.id);
        let invocation =
            ConnectorInvocation::from_tool_request(&request, &tool).expect("invocation builds");
        let mut review = ReviewQueueItem {
            id: Uuid::new_v4(),
            automation_run_id,
            agent_run_id: request.run_id,
            tool_invocation_id: None,
            status: ReviewQueueItemStatus::PendingReview,
            preview_fingerprint: Some(tool.request_fingerprint.clone()),
            revision: 0,
            title: "Review uncertain fake mutation".to_string(),
            evidence_ref: None,
            created_at: now,
            updated_at: now,
        };
        review
            .request_approval(tool.id, tool.request_fingerprint.clone(), now)
            .expect("review binds Tool");
        store
            .append_capability_access_request(&access_request)
            .expect("approval persists");
        store.append_tool_invocation(&tool).expect("tool persists");
        store
            .upsert_review_queue_item(&review)
            .expect("review persists");
        assert!(store
            .append_connector_invocation(&invocation)
            .expect("invocation persists"));
        store
            .resolve_capability_access_request(
                access_request.id,
                true,
                "Approve exact reconciliation fixture".to_string(),
            )
            .expect("approval resolves");
        let running = store
            .start_approved_connector_invocation(invocation.id, now)
            .expect("invocation starts");
        provider.timeout_after_next_apply();
        assert_eq!(
            ConnectorMutationProvider::apply_mutation(&provider, &account, &running)
                .expect("fake mutation applies"),
            ConnectorMutationApplyOutcome::ReconciliationRequired
        );
        store
            .mark_connector_invocation_reconciliation_required(invocation.id, now)
            .expect("uncertain state persists");
        drop(store);
        drop(provider);
        ReconciliationFixture {
            _temp_dir: temp_dir,
            path,
            account,
            remote,
            invocation_id: invocation.id,
            now,
        }
    }

    struct SingleReconciler<'a>(&'a dyn ConnectorMutationReconciler);

    impl ConnectorReconcilerRegistry for SingleReconciler<'_> {
        fn reconciler(&self, provider_id: &str) -> Option<&dyn ConnectorMutationReconciler> {
            (provider_id == self.0.provider_id()).then_some(self.0)
        }

        fn execution_enabled(&self) -> bool {
            true
        }
    }

    struct BlockingReconciler {
        inner: FakeConnectorProvider,
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl ConnectorProvider for BlockingReconciler {
        fn provider_id(&self) -> &'static str {
            self.inner.provider_id()
        }

        fn capabilities(&self) -> &'static [ConnectorCapability] {
            self.inner.capabilities()
        }
    }

    impl ConnectorMutationReconciler for BlockingReconciler {
        fn reconcile_mutation(
            &self,
            account: &ConnectorAccount,
            invocation: &ConnectorInvocation,
        ) -> Result<ConnectorReconciliationOutcome, String> {
            self.entered
                .send(())
                .map_err(|_| "blocking reconciler barrier failed".to_string())?;
            self.release
                .lock()
                .map_err(|_| "blocking reconciler barrier failed".to_string())?
                .recv()
                .map_err(|_| "blocking reconciler barrier failed".to_string())?;
            self.inner.reconcile_mutation(account, invocation)
        }
    }

    struct BlockingReconcilerRegistry(BlockingReconciler);

    impl ConnectorReconcilerRegistry for BlockingReconcilerRegistry {
        fn reconciler(&self, provider_id: &str) -> Option<&dyn ConnectorMutationReconciler> {
            (provider_id == self.0.provider_id()).then_some(&self.0)
        }

        fn execution_enabled(&self) -> bool {
            true
        }
    }

    #[test]
    fn shared_reconciliation_worker_is_bounded_lock_free_and_empty_registry_quiet() {
        let fixture = reconciliation_fixture();
        let provider = FakeConnectorProvider::with_remote_state(Arc::clone(&fixture.remote));
        let registry = SingleReconciler(&provider);
        let event_store = Arc::new(Mutex::new(
            EventStore::open(&fixture.path).expect("store opens for shared worker"),
        ));

        let sweep = reconcile_due_connector_mutations_with_shared_store(&event_store, &registry, 1)
            .expect("shared worker reconciles one due item");
        assert_eq!(sweep.claimed, 1);
        assert_eq!(sweep.completed, 1);
        assert_eq!(provider.applied_count(), 1);
        assert_eq!(
            reconcile_due_connector_mutations_with_shared_store(&event_store, &registry, 1)
                .expect("completed item is not claimed twice")
                .claimed,
            0
        );

        let quiet_fixture = reconciliation_fixture();
        let quiet_store = Arc::new(Mutex::new(
            EventStore::open(&quiet_fixture.path).expect("quiet store opens"),
        ));
        let invocation_before = quiet_store
            .lock()
            .unwrap()
            .connector_invocation(quiet_fixture.invocation_id)
            .unwrap();
        let quiet = reconcile_due_connector_mutations_with_shared_store(
            &quiet_store,
            &EmptyConnectorReconcilerRegistry,
            8,
        )
        .expect("empty registry is a no-op");
        assert_eq!(quiet, ConnectorReconciliationSweep::default());
        assert_eq!(
            quiet_store
                .lock()
                .unwrap()
                .connector_invocation(quiet_fixture.invocation_id)
                .unwrap(),
            invocation_before
        );
    }

    #[test]
    fn shared_worker_releases_store_lock_during_blocking_provider_query() {
        let fixture = reconciliation_fixture();
        let event_store = Arc::new(Mutex::new(
            EventStore::open(&fixture.path).expect("store opens for blocking worker"),
        ));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let registry = BlockingReconcilerRegistry(BlockingReconciler {
            inner: FakeConnectorProvider::with_remote_state(Arc::clone(&fixture.remote)),
            entered: entered_tx,
            release: Mutex::new(release_rx),
        });
        let worker_store = Arc::clone(&event_store);
        let worker = std::thread::spawn(move || {
            reconcile_due_connector_mutations_with_shared_store(&worker_store, &registry, 1)
                .expect("blocking worker completes")
        });

        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("provider query starts");
        assert!(
            event_store.try_lock().is_ok(),
            "provider reconciliation must not hold the EventStore mutex"
        );
        release_tx.send(()).expect("provider query releases");
        let sweep = worker.join().expect("worker joins");
        assert_eq!(sweep.claimed, 1);
        assert_eq!(sweep.completed, 1);
    }

    #[test]
    fn reconciliation_claim_is_exclusive_and_expiry_fences_the_old_worker() {
        let fixture = reconciliation_fixture();
        let first_store = EventStore::open(&fixture.path).expect("first store opens");
        let second_store = EventStore::open(&fixture.path).expect("second store opens");
        let mut first = first_store
            .claim_due_connector_reconciliations(fixture.now, 1)
            .expect("first claim succeeds");
        let first = first.pop().expect("first worker owns the claim");
        assert!(second_store
            .claim_due_connector_reconciliations(fixture.now, 1)
            .expect("second claim query succeeds")
            .is_empty());

        let takeover_at = first.claim_expires_at() + Duration::seconds(1);
        let mut second = second_store
            .claim_due_connector_reconciliations(takeover_at, 1)
            .expect("expired claim can be taken over");
        let second = second.pop().expect("second worker owns replacement claim");
        assert_ne!(first.claim_id(), second.claim_id());
        assert!(first_store
            .defer_connector_reconciliation(&first, takeover_at)
            .is_err());
        second_store
            .defer_connector_reconciliation(&second, takeover_at)
            .expect("replacement claim can defer");
    }

    #[test]
    fn malformed_due_rows_are_quarantined_without_starving_healthy_reconciliation() {
        let fixture = reconciliation_fixture();
        let sqlite = rusqlite::Connection::open(&fixture.path).expect("sqlite opens");
        let malformed_due = (fixture.now - Duration::hours(1)).to_rfc3339();
        for index in 0..9 {
            sqlite
                .execute(
                    r#"INSERT INTO connector_invocations
                       (id, account_id, account_generation, idempotency_key,
                        invocation_json, status, next_reconciliation_at,
                        reconciliation_attempt_count, updated_at)
                       SELECT ?1, account_id, account_generation, ?2, '{not-json', status,
                              ?3, 0, ?3
                       FROM connector_invocations WHERE id = ?4"#,
                    rusqlite::params![
                        format!("malformed-reconciliation-{index}"),
                        format!("malformed-idempotency-{index}"),
                        malformed_due,
                        fixture.invocation_id.to_string(),
                    ],
                )
                .expect("malformed due row inserts");
        }
        drop(sqlite);

        let store = EventStore::open(&fixture.path).expect("store opens");
        let claims = store
            .claim_due_connector_reconciliations(fixture.now, 1)
            .expect("claim sweep continues after quarantine");
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].invocation().id, fixture.invocation_id);
        drop(store);

        let sqlite = rusqlite::Connection::open(&fixture.path).expect("sqlite reopens");
        let quarantined: i64 = sqlite
            .query_row(
                "SELECT COUNT(*) FROM connector_invocations WHERE reconciliation_quarantine_code = 'invalid_projection_binding'",
                [],
                |row| row.get(0),
            )
            .expect("quarantine count loads");
        assert_eq!(quarantined, 9);
    }

    #[test]
    fn missing_reconciler_defers_with_persistent_backoff() {
        let fixture = reconciliation_fixture();
        let store = EventStore::open(&fixture.path).expect("store opens");
        let sweep = reconcile_due_connector_mutations(
            &store,
            &EmptyConnectorReconcilerRegistry,
            fixture.now,
            1,
        )
        .expect("missing registry is handled safely");
        assert_eq!(sweep.claimed, 1);
        assert_eq!(sweep.deferred, 1);
        assert!(store
            .claim_due_connector_reconciliations(fixture.now, 1)
            .expect("immediate retry query succeeds")
            .is_empty());
    }

    #[test]
    fn recovery_inspection_is_registry_gated_stable_one_shot_and_apply_free() {
        let fixture = reconciliation_fixture();
        let store = EventStore::open(&fixture.path).expect("store opens");
        let mut claims = store
            .claim_due_connector_reconciliations(fixture.now, 1)
            .expect("claim succeeds");
        let claim = claims.pop().expect("one reconciliation is due");
        let deferred_until = store
            .defer_connector_reconciliation(&claim, fixture.now)
            .expect("reconciliation enters durable backoff");
        let provider = FakeConnectorProvider::with_remote_state(Arc::clone(&fixture.remote));
        let registry = SingleReconciler(&provider);

        let default_item = store
            .list_connector_recovery_items()
            .expect("default recovery projection loads")
            .into_iter()
            .find(|item| item.id == fixture.invocation_id)
            .expect("uncertain mutation remains visible");
        assert!(default_item.action.is_none());

        let inspect_item = || {
            store
                .list_connector_recovery_items_with_registry(&registry)
                .expect("registry-aware recovery projection loads")
                .into_iter()
                .find(|item| item.kind == ConnectorRecoveryKind::Reconciliation)
                .expect("reconciliation item remains visible")
        };
        let first = inspect_item();
        let second = inspect_item();
        let action_revision = match first.action {
            Some(ConnectorRecoveryAction::InspectExternalResult { action_revision }) => {
                action_revision
            }
            _ => panic!("matching read-only reconciler enables exact inspection"),
        };
        assert_eq!(
            second.action,
            Some(ConnectorRecoveryAction::InspectExternalResult {
                action_revision: action_revision.clone()
            })
        );
        assert!(store
            .schedule_connector_reconciliation_from_recovery(
                fixture.invocation_id,
                &action_revision,
                &EmptyConnectorReconcilerRegistry,
                fixture.now + Duration::seconds(1),
            )
            .is_err());
        assert!(store
            .schedule_connector_reconciliation_from_recovery(
                fixture.invocation_id,
                &format!("{action_revision}x"),
                &registry,
                fixture.now + Duration::seconds(1),
            )
            .is_err());
        assert_eq!(provider.applied_count(), 1);

        let scheduled_at = fixture.now;
        assert_eq!(
            store
                .schedule_connector_reconciliation_from_recovery(
                    fixture.invocation_id,
                    &action_revision,
                    &registry,
                    scheduled_at,
                )
                .expect("exact inspection is scheduled"),
            crate::kernel::connectors::ConnectorRecoveryAcceptance::Accepted
        );
        assert_eq!(provider.applied_count(), 1);
        for _ in 0..100 {
            assert_eq!(
                store
                    .schedule_connector_reconciliation_from_recovery(
                        fixture.invocation_id,
                        &action_revision,
                        &registry,
                        scheduled_at,
                    )
                    .expect("accepted inspection replays idempotently"),
                crate::kernel::connectors::ConnectorRecoveryAcceptance::AlreadyAccepted
            );
        }
        assert!(store
            .list_connector_recovery_items_with_registry(&registry)
            .expect("scheduled projection reloads")
            .into_iter()
            .find(|item| item.id == fixture.invocation_id)
            .expect("scheduled item remains visible")
            .action
            .is_none());

        drop(store);
        let restarted_store = Arc::new(Mutex::new(
            EventStore::open(&fixture.path).expect("store restarts before worker"),
        ));
        assert_eq!(
            restarted_store
                .lock()
                .expect("store lock remains available")
                .schedule_connector_reconciliation_from_recovery(
                    fixture.invocation_id,
                    &action_revision,
                    &registry,
                    scheduled_at,
                )
                .expect("accepted inspection survives restart"),
            crate::kernel::connectors::ConnectorRecoveryAcceptance::AlreadyAccepted
        );
        let sweep =
            reconcile_due_connector_mutations_with_shared_store(&restarted_store, &registry, 1)
                .expect("shared worker performs read-only check after restart");
        assert_eq!(sweep.completed, 1);
        assert_eq!(provider.applied_count(), 1);
        assert!(deferred_until > scheduled_at);
        drop(restarted_store);

        let sqlite = rusqlite::Connection::open(&fixture.path).expect("sqlite reopens");
        let events = sqlite
            .prepare("SELECT payload_json FROM kernel_events")
            .expect("event query prepares")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("events query")
            .collect::<Result<Vec<_>, _>>()
            .expect("events load");
        assert!(events
            .iter()
            .all(|payload| !payload.contains(&action_revision)));
    }

    #[test]
    fn recovery_inspection_rejects_same_generation_private_authority_tamper() {
        let fixture = reconciliation_fixture();
        let provider = FakeConnectorProvider::with_remote_state(Arc::clone(&fixture.remote));
        let registry = SingleReconciler(&provider);
        let store = EventStore::open(&fixture.path).expect("store opens");
        let claim = store
            .claim_due_connector_reconciliations(fixture.now, 1)
            .expect("claim succeeds")
            .pop()
            .expect("one reconciliation is due");
        store
            .defer_connector_reconciliation(&claim, fixture.now)
            .expect("reconciliation enters backoff");
        let action_revision = match store
            .list_connector_recovery_items_with_registry(&registry)
            .expect("recovery projection loads")
            .into_iter()
            .find(|item| item.id == fixture.invocation_id)
            .and_then(|item| item.action)
        {
            Some(ConnectorRecoveryAction::InspectExternalResult { action_revision }) => {
                action_revision
            }
            _ => panic!("inspection action exists"),
        };
        let mut tampered = fixture.account.clone();
        tampered.tenant_ref = Some("same-generation-private-tenant-tamper".to_string());
        tampered.credential_handle = ConnectorCredentialHandle::new();
        assert!(store
            .schedule_connector_reconciliation_from_recovery(
                fixture.invocation_id,
                &format!("{action_revision}x"),
                &registry,
                fixture.now + Duration::seconds(1),
            )
            .is_err());
        drop(store);

        let sqlite = rusqlite::Connection::open(&fixture.path).expect("sqlite reopens");
        sqlite
            .execute(
                "UPDATE connector_accounts SET account_json = ?2 WHERE id = ?1",
                rusqlite::params![
                    fixture.account.id.to_string(),
                    serde_json::to_string(&tampered).expect("tampered account serializes"),
                ],
            )
            .expect("private binding is tampered without changing generation");
        drop(sqlite);

        let store = EventStore::open(&fixture.path).expect("store reopens");
        assert!(store
            .schedule_connector_reconciliation_from_recovery(
                fixture.invocation_id,
                &action_revision,
                &registry,
                fixture.now + Duration::seconds(1),
            )
            .is_err());
        assert_eq!(provider.applied_count(), 1);
    }

    #[test]
    fn known_not_applied_is_a_terminal_non_replayable_result() {
        let fixture = reconciliation_fixture();
        fixture
            .remote
            .applied
            .lock()
            .expect("fake remote state locks")
            .clear();
        let provider = FakeConnectorProvider::with_remote_state(Arc::clone(&fixture.remote));
        let store = EventStore::open(&fixture.path).expect("store opens");

        let sweep =
            reconcile_due_connector_mutations(&store, &SingleReconciler(&provider), fixture.now, 1)
                .expect("read-only reconciliation completes");

        assert_eq!(sweep.claimed, 1);
        assert_eq!(sweep.not_applied, 1);
        assert_eq!(provider.applied_count(), 0);
        assert_eq!(
            store
                .connector_invocation(fixture.invocation_id)
                .expect("invocation remains auditable")
                .status,
            crate::kernel::connectors::ConnectorInvocationStatus::Failed
        );
        assert!(store
            .claim_due_connector_reconciliations(fixture.now + Duration::hours(1), 1)
            .expect("terminal invocation is not reclaimable")
            .is_empty());
    }

    struct MarkerFailureReconciler {
        calls: AtomicUsize,
        marker: String,
    }

    impl ConnectorProvider for MarkerFailureReconciler {
        fn provider_id(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> &'static [ConnectorCapability] {
            &[ConnectorCapability::MailSendDraft]
        }
    }

    impl ConnectorMutationReconciler for MarkerFailureReconciler {
        fn reconcile_mutation(
            &self,
            _account: &ConnectorAccount,
            _invocation: &ConnectorInvocation,
        ) -> Result<ConnectorReconciliationOutcome, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(self.marker.clone())
        }
    }

    #[test]
    fn provider_error_is_discarded_and_never_persisted() {
        let fixture = reconciliation_fixture();
        let marker = format!("secret-provider-body:{}", Uuid::new_v4());
        let reconciler = MarkerFailureReconciler {
            calls: AtomicUsize::new(0),
            marker: marker.clone(),
        };
        let store = EventStore::open(&fixture.path).expect("store opens");
        let sweep = reconcile_due_connector_mutations(
            &store,
            &SingleReconciler(&reconciler),
            fixture.now,
            1,
        )
        .expect("provider failure defers safely");
        assert_eq!(reconciler.calls.load(Ordering::SeqCst), 1);
        assert_eq!(sweep.deferred, 1);
        drop(store);
        let sqlite =
            String::from_utf8_lossy(&std::fs::read(&fixture.path).expect("sqlite file reads"))
                .into_owned();
        assert!(!sqlite.contains(&marker));
    }

    #[test]
    fn generation_change_prevents_claim_before_provider_reconciliation() {
        let mut fixture = reconciliation_fixture();
        let store = EventStore::open(&fixture.path).expect("store opens");
        fixture.account.credential_handle = ConnectorCredentialHandle::new();
        fixture.account.updated_at = fixture.now + Duration::seconds(1);
        store
            .upsert_connector_account(&fixture.account)
            .expect("account generation advances");
        let provider = FakeConnectorProvider::with_remote_state(Arc::clone(&fixture.remote));
        let sweep = reconcile_due_connector_mutations(
            &store,
            &SingleReconciler(&provider),
            fixture.now + Duration::seconds(1),
            1,
        )
        .expect("changed account is skipped safely");
        assert_eq!(sweep.claimed, 0);
        assert_eq!(
            store
                .connector_invocation(fixture.invocation_id)
                .expect("invocation remains visible")
                .status,
            crate::kernel::connectors::ConnectorInvocationStatus::ReconciliationRequired
        );
    }

    #[test]
    fn generation_change_after_remote_query_fences_completion() {
        let mut fixture = reconciliation_fixture();
        let store = EventStore::open(&fixture.path).expect("store opens");
        let mut claims = store
            .claim_due_connector_reconciliations(fixture.now, 1)
            .expect("claim succeeds");
        let claim = claims.pop().expect("one claim is due");
        let provider = FakeConnectorProvider::with_remote_state(Arc::clone(&fixture.remote));
        let ConnectorReconciliationOutcome::Applied(receipt) =
            ConnectorMutationReconciler::reconcile_mutation(
                &provider,
                claim.account(),
                claim.invocation(),
            )
            .expect("remote query finds the effect")
        else {
            panic!("remote effect should be present");
        };
        fixture.account.credential_handle = ConnectorCredentialHandle::new();
        fixture.account.updated_at = fixture.now + Duration::seconds(1);
        store
            .upsert_connector_account(&fixture.account)
            .expect("account generation advances");
        assert!(store
            .complete_claimed_connector_reconciliation(
                &claim,
                receipt,
                fixture.now + Duration::seconds(1),
            )
            .is_err());
        assert_eq!(
            store
                .connector_invocation(fixture.invocation_id)
                .expect("uncertain invocation remains visible")
                .status,
            crate::kernel::connectors::ConnectorInvocationStatus::ReconciliationRequired
        );
    }

    #[test]
    fn reconciliation_worker_source_has_no_apply_authority() {
        let source = include_str!("reconciliation.rs");
        let forbidden = [".apply_", "mutation("].concat();
        assert!(!source.contains(&forbidden));
    }
}
