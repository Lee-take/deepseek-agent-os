use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

use super::oauth::{
    prepare_authorization, ConnectorAuthorizationAccountProfile, ConnectorOAuthExchange,
    ConnectorOAuthProvider,
};
use super::provider::{
    collect_calendar_events, collect_mail_search, CalendarListRequest, MailSearchRequest,
};
use super::reconciliation::{reconcile_due_connector_mutations, ConnectorReconcilerRegistry};
use super::revocation::{
    revoke_due_connector_accounts, ConnectorRemoteRevocationOutcome, ConnectorRevocationProvider,
    ConnectorRevocationRegistry,
};
use super::runtime_registry::ConnectorOAuthRegistry;
use super::sync::{
    run_calendar_sync_step, run_mail_sync_step, CalendarSyncRequest, ConnectorSyncStep,
    MailSyncRequest,
};
use super::{
    ConnectorAccount, ConnectorCapability, ConnectorCredentialDeleteOutcome,
    ConnectorCredentialHandle, ConnectorCredentialStore, ConnectorDraftProvider, ConnectorHealth,
    ConnectorInvocation, ConnectorMutationApplyOutcome, ConnectorMutationProvider,
    ConnectorProvider, ConnectorRecoveryExternalEffectState, ConnectorRecoveryStatus,
    ConnectorRuntime, ConnectorSecret, FakeConnectorProvider, FAKE_PROVIDER_CAPABILITIES,
};
use crate::connector_commands::{
    resolve_connector_authorization_review_with_registry, ConnectorAuthorizationResolveRequest,
    ConnectorAuthorizationReviewIntent, ConnectorAuthorizationUserStatus,
};
use crate::kernel::automation::{AutomationDefinition, ReviewQueueItem, ReviewQueueItemStatus};
use crate::kernel::event_store::EventStore;
use crate::kernel::models::{AccessMode, FoundationState};
use crate::kernel::policy::{request_capability_access, CapabilityKind};
use crate::kernel::tool_runtime::{
    prepare_tool_execution, ToolExecutionRequest, ToolInvocationRecord, CONNECTOR_MUTATE_TOOL_ID,
};
use crate::kernel::work_package::export_work_package;

#[derive(Clone, Default)]
struct RestartableFakeVault {
    secrets: Arc<Mutex<HashMap<ConnectorCredentialHandle, String>>>,
    fail_delete: Arc<AtomicBool>,
}

impl ConnectorCredentialStore for RestartableFakeVault {
    fn put_at(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        self.secrets
            .lock()
            .map_err(|_| "fake vault is unavailable".to_string())?
            .insert(handle.clone(), secret.expose().to_string());
        Ok(())
    }

    fn read(&self, handle: &ConnectorCredentialHandle) -> Result<ConnectorSecret, String> {
        let secret = self
            .secrets
            .lock()
            .map_err(|_| "fake vault is unavailable".to_string())?
            .get(handle)
            .cloned()
            .ok_or_else(|| "connector credential is unavailable".to_string())?;
        ConnectorSecret::new(secret)
    }

    fn replace(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        if !self.contains(handle) {
            return Err("connector credential is unavailable".to_string());
        }
        self.put_at(handle, secret)
    }

    fn delete(
        &mut self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<ConnectorCredentialDeleteOutcome, String> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err("fake local credential deletion failed".to_string());
        }
        Ok(
            if self
                .secrets
                .lock()
                .map_err(|_| "fake vault is unavailable".to_string())?
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
        self.secrets
            .lock()
            .map(|secrets| secrets.contains_key(handle))
            .unwrap_or(false)
    }
}

struct CountingRevoker {
    calls: AtomicUsize,
    expected_marker: String,
}

impl ConnectorRevocationProvider for CountingRevoker {
    fn provider_id(&self) -> &'static str {
        "fake"
    }

    fn revoke_credential(
        &self,
        current: &ConnectorSecret,
        _revocation_id: Uuid,
        _attempt_id: Uuid,
    ) -> Result<ConnectorRemoteRevocationOutcome, String> {
        assert_eq!(current.expose(), self.expected_marker);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ConnectorRemoteRevocationOutcome::Revoked)
    }
}

struct SingleRevoker<'a>(&'a CountingRevoker);

impl ConnectorRevocationRegistry for SingleRevoker<'_> {
    fn provider(&self, provider_id: &str) -> Option<&dyn ConnectorRevocationProvider> {
        (provider_id == "fake").then_some(self.0)
    }
}

struct EmptyRevokerRegistry;

impl ConnectorRevocationRegistry for EmptyRevokerRegistry {
    fn provider(&self, _provider_id: &str) -> Option<&dyn ConnectorRevocationProvider> {
        None
    }
}

struct M2FakeOAuth {
    credential_marker: String,
}

struct SingleOAuth<'a>(&'a M2FakeOAuth);

impl ConnectorOAuthRegistry for SingleOAuth<'_> {
    fn provider(&self, provider_key: &str) -> Option<&dyn ConnectorOAuthProvider> {
        (provider_key == "fake").then_some(self.0)
    }
}

struct SingleFakeReconciler<'a>(&'a FakeConnectorProvider);

impl ConnectorReconcilerRegistry for SingleFakeReconciler<'_> {
    fn reconciler(&self, provider_id: &str) -> Option<&dyn super::ConnectorMutationReconciler> {
        (provider_id == self.0.provider_id()).then_some(self.0)
    }
}

fn stage_exact_approved_mutation(
    store: &EventStore,
    account: &ConnectorAccount,
    capability: ConnectorCapability,
    target_ref: &str,
    idempotency_key: &str,
    now: chrono::DateTime<Utc>,
    start: bool,
) -> ConnectorInvocation {
    let definition = AutomationDefinition::once(
        format!("M2 lifecycle {}", capability.contract_name()),
        "UTC".to_string(),
        now - Duration::minutes(1),
    )
    .unwrap();
    store.upsert_automation_definition(&definition).unwrap();
    let automation_run_id = store
        .claim_due_automation_run(definition.id, now, "m2-lifecycle".to_string())
        .unwrap()
        .unwrap()
        .id;
    let request = ToolExecutionRequest {
        tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
        input: json!({
            "provider_id": account.provider_id,
            "account_id": account.id.to_string(),
            "account_generation": 0,
            "capability": capability.contract_name(),
            "target_ref": target_ref,
            "preview_hash": format!("sha256:{idempotency_key}"),
            "idempotency_key": idempotency_key,
            "automation_run_id": automation_run_id.to_string()
        }),
        access_mode: AccessMode::FullAccess,
        run_id: Some(Uuid::new_v4()),
    };
    let plan = prepare_tool_execution(&request).unwrap();
    let access =
        request_capability_access(AccessMode::FullAccess, CapabilityKind::ConnectorWrite).unwrap();
    let tool = ToolInvocationRecord::waiting_for_confirmation(&plan, access.id);
    let invocation = ConnectorInvocation::from_tool_request(&request, &tool).unwrap();
    let mut review = ReviewQueueItem {
        id: Uuid::new_v4(),
        automation_run_id,
        agent_run_id: request.run_id,
        tool_invocation_id: None,
        status: ReviewQueueItemStatus::PendingReview,
        preview_fingerprint: Some(tool.request_fingerprint.clone()),
        revision: 0,
        title: format!("Review {}", capability.contract_name()),
        evidence_ref: Some(target_ref.to_string()),
        created_at: now,
        updated_at: now,
    };
    review
        .request_approval(tool.id, tool.request_fingerprint.clone(), now)
        .unwrap();
    store.append_capability_access_request(&access).unwrap();
    store.append_tool_invocation(&tool).unwrap();
    store.upsert_review_queue_item(&review).unwrap();
    assert!(store.append_connector_invocation(&invocation).unwrap());
    assert!(store
        .start_approved_connector_invocation(invocation.id, now)
        .is_err());
    store
        .resolve_capability_access_request(access.id, true, "M2 exact approval".to_string())
        .unwrap();
    if start {
        store
            .start_approved_connector_invocation(invocation.id, now)
            .unwrap()
    } else {
        invocation
    }
}

impl ConnectorOAuthProvider for M2FakeOAuth {
    fn provider_id(&self) -> &'static str {
        "fake"
    }

    fn scopes_for(&self, capability: ConnectorCapability) -> Option<&'static [&'static str]> {
        match capability {
            ConnectorCapability::MailSearch
            | ConnectorCapability::MailReadThread
            | ConnectorCapability::MailSyncInbox => Some(&["mail.read"]),
            ConnectorCapability::MailCreateDraft => Some(&["mail.draft"]),
            ConnectorCapability::MailSendDraft => Some(&["mail.send"]),
            ConnectorCapability::CalendarListEvents | ConnectorCapability::CalendarSyncEvents => {
                Some(&["calendar.read"])
            }
            ConnectorCapability::CalendarCreateEvent => Some(&["calendar.write"]),
            _ => None,
        }
    }

    fn exchange_code(
        &self,
        code: &str,
        verifier: &ConnectorSecret,
        _redirect_uri: &str,
        requested_scopes: &[String],
    ) -> Result<ConnectorOAuthExchange, String> {
        if code != "m2-valid-code" || verifier.expose().len() < 43 {
            return Err("invalid fake exchange".to_string());
        }
        ConnectorOAuthExchange::new(
            ConnectorSecret::new(self.credential_marker.clone())?,
            requested_scopes.to_vec(),
        )
    }

    fn complete_review(
        &self,
        verifier: &ConnectorSecret,
        redirect_uri: &str,
        requested_scopes: &[String],
    ) -> Result<ConnectorOAuthExchange, String> {
        self.exchange_code("m2-valid-code", verifier, redirect_uri, requested_scopes)
    }

    fn account_profile(
        &self,
        _exchange: &ConnectorOAuthExchange,
    ) -> Result<ConnectorAuthorizationAccountProfile, String> {
        ConnectorAuthorizationAccountProfile::new(
            "M2 fake account".to_string(),
            Some("fake-tenant".to_string()),
        )
    }
}

#[test]
fn fake_m2_full_lifecycle_is_restart_safe_idempotent_and_secret_free() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let path = temp_dir.path().join("fake-m2-lifecycle.sqlite3");
    let wal_guard = rusqlite::Connection::open(&path).expect("WAL guard opens");
    wal_guard
        .pragma_update(None, "journal_mode", "WAL")
        .expect("WAL mode is active");
    let marker = format!("m2-refresh-secret:{}", Uuid::new_v4());
    let oauth = M2FakeOAuth {
        credential_marker: marker.clone(),
    };
    let vault_state = RestartableFakeVault::default();
    let now = Utc::now();
    let (session, verifier) = prepare_authorization(
        &oauth,
        FAKE_PROVIDER_CAPABILITIES.to_vec(),
        "http://127.0.0.1:43821/callback".to_string(),
        now,
    )
    .expect("fake authorization starts");
    let runtime = ConnectorRuntime::new(vault_state.clone());
    let review_id;
    {
        let store = EventStore::open(&path).expect("event store opens");
        store
            .insert_preparing_connector_authorization(&session, now)
            .expect("preparing authorization persists before vault");
        runtime
            .put_authorization_verifier(&session.verifier_handle, verifier)
            .expect("verifier persists outside SQLite");
        store
            .activate_preparing_connector_authorization(session.id, now)
            .expect("authorization activates pending");
        let provision = store
            .prepare_connector_authorization_review(session.id, now)
            .expect("durable review prepares");
        review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        runtime
            .put_authorization_review_authority(&authority_handle, authority)
            .expect("review authority persists outside SQLite");
        store
            .activate_connector_authorization_review(review_id, now)
            .expect("durable review activates");
    }

    let restarted_store = Arc::new(Mutex::new(
        EventStore::open(&path).expect("event store restarts"),
    ));
    let connected = resolve_connector_authorization_review_with_registry(
        &restarted_store,
        &runtime,
        &SingleOAuth(&oauth),
        ConnectorAuthorizationResolveRequest {
            review_id,
            intent: ConnectorAuthorizationReviewIntent::Approve,
        },
        now + Duration::seconds(1),
    )
    .expect("durable review authorization completes after restart");
    assert_eq!(
        connected.status,
        ConnectorAuthorizationUserStatus::Connected
    );
    let connected_json =
        serde_json::to_string(&connected).expect("safe authorization DTO serializes");
    assert!(!connected_json.contains(&marker));
    assert!(!connected_json.contains("fake-tenant"));
    let mut account = restarted_store
        .lock()
        .expect("store locks")
        .list_connector_accounts()
        .expect("authorized account lists")
        .into_iter()
        .next()
        .expect("authorized account persists atomically");
    assert!(vault_state.contains(&account.credential_handle));
    assert!(!vault_state.contains(&session.verifier_handle));
    let handle_json = serde_json::to_string(&account.credential_handle).unwrap();
    let handle_marker = handle_json.trim_matches('"');
    assert!(!connected_json.contains(handle_marker));
    let replay_error = resolve_connector_authorization_review_with_registry(
        &restarted_store,
        &runtime,
        &SingleOAuth(&oauth),
        ConnectorAuthorizationResolveRequest {
            review_id,
            intent: ConnectorAuthorizationReviewIntent::Approve,
        },
        now + Duration::seconds(2),
    )
    .expect_err("consumed durable approval cannot be replayed");
    assert_eq!(replay_error, "connector authorization is unavailable");
    assert!(!replay_error.contains(&marker));
    assert!(!replay_error.contains(handle_marker));
    drop(restarted_store);
    drop(runtime);

    let store = EventStore::open(&path).expect("event store continues after authorization");
    let provider = FakeConnectorProvider::default();
    let mail = collect_mail_search(
        &provider,
        &account,
        &MailSearchRequest::new("m2".to_string(), 2).unwrap(),
    )
    .expect("bounded mail read succeeds");
    assert_eq!(mail.len(), 1);
    let starts_at = now;
    let calendar = collect_calendar_events(
        &provider,
        &account,
        &CalendarListRequest::new(starts_at, starts_at + Duration::days(1), 2).unwrap(),
    )
    .expect("bounded calendar read succeeds");
    assert_eq!(calendar.len(), 1);
    assert!(matches!(
        run_mail_sync_step(
            &store,
            &provider,
            &mut account,
            &MailSyncRequest::inbox(2).unwrap(),
            now + Duration::seconds(3),
        )
        .expect("mail sync commits"),
        ConnectorSyncStep::Committed {
            delta_committed: true,
            ..
        }
    ));
    assert!(matches!(
        run_calendar_sync_step(
            &store,
            &provider,
            &mut account,
            &CalendarSyncRequest::new(starts_at, starts_at + Duration::days(1), 2).unwrap(),
            now + Duration::seconds(4),
        )
        .expect("calendar sync commits"),
        ConnectorSyncStep::Committed {
            delta_committed: true,
            ..
        }
    ));

    let remote = provider.remote_state();
    let draft = provider
        .create_draft(&account, "M2 reviewed reply")
        .expect("mail draft is created");
    let mail_invocation = stage_exact_approved_mutation(
        &store,
        &account,
        ConnectorCapability::MailSendDraft,
        &draft.remote_object_ref,
        "m2:mail-send:once",
        now + Duration::seconds(5),
        true,
    );
    let calendar_invocation = stage_exact_approved_mutation(
        &store,
        &account,
        ConnectorCapability::CalendarCreateEvent,
        "fake:event-draft:m2",
        "m2:calendar-create:once",
        now + Duration::seconds(6),
        true,
    );
    provider.timeout_after_next_apply();
    assert_eq!(
        provider.apply_mutation(&account, &mail_invocation).unwrap(),
        ConnectorMutationApplyOutcome::ReconciliationRequired
    );
    store
        .mark_connector_invocation_reconciliation_required(mail_invocation.id, now)
        .unwrap();
    provider.timeout_after_next_apply();
    assert_eq!(
        provider
            .apply_mutation(&account, &calendar_invocation)
            .unwrap(),
        ConnectorMutationApplyOutcome::ReconciliationRequired
    );
    store
        .mark_connector_invocation_reconciliation_required(calendar_invocation.id, now)
        .unwrap();
    assert_eq!(provider.applied_count(), 2);
    drop(provider);
    drop(store);

    let store = EventStore::open(&path).expect("event store restarts for reconciliation");
    store
        .reset_abandoned_connector_reconciliation_claims(now + Duration::seconds(7))
        .unwrap();
    let restarted_provider = FakeConnectorProvider::with_remote_state(remote);
    let sweep = reconcile_due_connector_mutations(
        &store,
        &SingleFakeReconciler(&restarted_provider),
        now + Duration::seconds(7),
        2,
    )
    .expect("read-only reconciliation completes both effects");
    assert_eq!(sweep.claimed, 2);
    assert_eq!(sweep.completed, 2);
    assert_eq!(restarted_provider.applied_count(), 2);
    for invocation_id in [mail_invocation.id, calendar_invocation.id] {
        assert_eq!(
            store.connector_invocation(invocation_id).unwrap().status,
            super::ConnectorInvocationStatus::Succeeded
        );
    }
    assert_eq!(
        reconcile_due_connector_mutations(
            &store,
            &SingleFakeReconciler(&restarted_provider),
            now + Duration::seconds(8),
            2,
        )
        .unwrap()
        .claimed,
        0
    );

    let stale_invocation = stage_exact_approved_mutation(
        &store,
        &account,
        ConnectorCapability::MailSendDraft,
        "fake:draft:stale-generation",
        "m2:stale-generation:once",
        now + Duration::seconds(9),
        false,
    );
    let ticket = store
        .begin_connector_revocation(account.id, now + Duration::seconds(10))
        .expect("durable revocation begins");
    assert_eq!(ticket.generation(), 1);
    assert!(store
        .start_approved_connector_invocation(stale_invocation.id, now + Duration::seconds(10))
        .is_err());
    assert_eq!(restarted_provider.applied_count(), 2);
    vault_state.fail_delete.store(true, Ordering::SeqCst);
    let revoker = CountingRevoker {
        calls: AtomicUsize::new(0),
        expected_marker: marker.clone(),
    };
    let runtime = ConnectorRuntime::new(vault_state.clone());
    let revocation = revoke_due_connector_accounts(
        &store,
        &runtime,
        &SingleRevoker(&revoker),
        now + Duration::seconds(10),
        1,
    )
    .expect("remote revocation confirms while local delete defers");
    assert_eq!(revocation.remote_confirmed, 1);
    assert_eq!(revocation.completed, 0);
    assert_eq!(revocation.deferred, 1);
    assert_eq!(revoker.calls.load(Ordering::SeqCst), 1);
    let recovery_items = store
        .list_connector_recovery_items()
        .expect("remote-confirmed revocation projects into Recovery Center");
    let recovery_item = recovery_items
        .iter()
        .find(|item| item.id == account.id)
        .expect("revocation recovery item exists");
    assert_eq!(
        recovery_item.status,
        ConnectorRecoveryStatus::RevocationPending
    );
    assert_eq!(
        recovery_item.external_effect_state,
        ConnectorRecoveryExternalEffectState::LocalCredentialRemovalPending
    );
    assert!(recovery_item.action.is_none());
    let recovery_json = serde_json::to_string(recovery_item).expect("safe Recovery DTO serializes");
    assert!(!recovery_json.contains(&marker));
    assert!(!recovery_json.contains(handle_marker));
    for private_field in ["ticket", "claim", "attempt", "credential_handle"] {
        assert!(!recovery_json.contains(private_field));
    }
    drop(runtime);
    drop(store);

    vault_state.fail_delete.store(false, Ordering::SeqCst);
    let store = EventStore::open(&path).expect("event store restarts after remote confirmation");
    store
        .reset_abandoned_connector_revocation_claims(now + Duration::seconds(50))
        .unwrap();
    let runtime = ConnectorRuntime::new(vault_state.clone());
    let finalized = revoke_due_connector_accounts(
        &store,
        &runtime,
        &EmptyRevokerRegistry,
        now + Duration::seconds(50),
        1,
    )
    .expect("restart performs local finalization only");
    assert_eq!(finalized.remote_claimed, 0);
    assert_eq!(finalized.finalization_claimed, 1);
    assert_eq!(finalized.completed, 1);
    assert_eq!(revoker.calls.load(Ordering::SeqCst), 1);
    assert!(!vault_state.contains(&account.credential_handle));
    assert_eq!(
        store.list_connector_accounts().unwrap()[0].health,
        ConnectorHealth::Disconnected
    );
    assert!(store
        .start_approved_connector_invocation(stale_invocation.id, now + Duration::seconds(51))
        .is_err());
    assert_eq!(restarted_provider.applied_count(), 2);

    let package = export_work_package(
        FoundationState::default(),
        store.list_task_records().unwrap(),
        store.list_memory_candidates().unwrap(),
        store.list_operations_briefing_runs().unwrap(),
    );
    let package_json = serde_json::to_string(&package).unwrap();
    assert!(!package_json.contains(&marker));
    assert!(!package_json.contains(handle_marker));

    let serialized_events = serde_json::to_vec(&store.list_recent(100).unwrap()).unwrap();
    assert!(!serialized_events
        .windows(marker.len())
        .any(|window| window == marker.as_bytes()));
    let handle_marker = handle_marker.as_bytes();
    assert!(!serialized_events
        .windows(handle_marker.len())
        .any(|window| window == handle_marker));
    assert!(std::path::PathBuf::from(format!("{}-wal", path.display())).is_file());
    assert!(std::path::PathBuf::from(format!("{}-shm", path.display())).is_file());
    for candidate in [
        path.clone(),
        std::path::PathBuf::from(format!("{}-wal", path.display())),
        std::path::PathBuf::from(format!("{}-shm", path.display())),
    ] {
        if candidate.is_file() {
            let bytes = std::fs::read(candidate).unwrap();
            assert!(!bytes
                .windows(marker.len())
                .any(|window| window == marker.as_bytes()));
        }
    }
    drop(store);
    drop(wal_guard);
    let main_db = std::fs::read(&path).unwrap();
    assert!(!main_db
        .windows(marker.len())
        .any(|window| window == marker.as_bytes()));
}
