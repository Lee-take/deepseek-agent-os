use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;
use crate::kernel::connectors::catalog::{
    production_connector_catalog, provider_label, user_abilities, ConnectorAbility,
    ConnectorAccountHealthView, ConnectorProviderDescriptor, ConnectorProviderLabelCode,
};
use crate::kernel::connectors::landing::{
    cleanup_incomplete_connector_attachment, ConnectorAttachmentCleanupFailure,
};
#[cfg(windows)]
use crate::kernel::connectors::landing::{
    recover_ready_connector_attachment, ConnectorAttachmentReadyRecoveryFailure,
};
use crate::kernel::connectors::read_execution::{
    ConnectorReadExecutionView, ConnectorReadPlan, ConnectorReadSubmission,
};
use crate::kernel::connectors::reconciliation::reconcile_due_connector_mutations_with_shared_store;
use crate::kernel::connectors::runtime_registry::ConnectorOAuthRegistry;
use crate::kernel::connectors::sync::run_connector_sync_recovery_with_shared_store;
use crate::kernel::connectors::{
    ConnectorAccount, ConnectorCredentialStore, ConnectorDisconnectSource, ConnectorHealth,
    ConnectorRecoveryAcceptance, ConnectorRecoveryItem, ConnectorRuntime,
};
use crate::kernel::event_store::{
    ConnectorAuthorizationReviewIntentState, ConnectorAuthorizationReviewSnapshot,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorAuthorizationUserStatus {
    AwaitingConfirmation,
    Connecting,
    Connected,
    Cancelling,
    Cancelled,
    RepairRequired,
}

#[derive(Debug, Serialize)]
pub struct ConnectorAuthorizationReviewView {
    pub review_id: Uuid,
    pub provider_label: ConnectorProviderLabelCode,
    pub abilities: Vec<ConnectorAbility>,
    pub status: ConnectorAuthorizationUserStatus,
    pub expires_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<ConnectorAccountHealthView>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorAuthorizationReviewIntent {
    Approve,
    Cancel,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorAuthorizationResolveRequest {
    pub review_id: Uuid,
    pub intent: ConnectorAuthorizationReviewIntent,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryConnectorAttachmentCleanupRequest {
    pub item_id: Uuid,
    pub action_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeConnectorReadSyncRequest {
    pub item_id: Uuid,
    pub action_revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectConnectorExternalResultRequest {
    pub item_id: Uuid,
    pub action_revision: String,
}

#[derive(Debug, Serialize)]
pub struct ConnectorRecoveryCommandResult {
    pub acceptance: ConnectorRecoveryAcceptance,
    pub items: Vec<ConnectorRecoveryItem>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExplicitConnectorReadAcceptance {
    Accepted,
    AlreadyAccepted,
}

#[derive(Debug, Serialize)]
pub struct ExplicitConnectorReadCommandResult {
    pub acceptance: ExplicitConnectorReadAcceptance,
    pub execution: ConnectorReadExecutionView,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplicitConnectorMailSearchRequest {
    pub source_invocation_id: Uuid,
    pub account_id: Uuid,
    pub query: String,
    pub max_results: u16,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplicitConnectorCalendarListRequest {
    pub source_invocation_id: Uuid,
    pub account_id: Uuid,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub max_results: u16,
}

fn validate_recovery_action_revision(value: &str) -> Result<(), String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err("recovery action revision is invalid".to_string())
    }
}

fn store_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn safe_account_health_view(
    store: &crate::kernel::event_store::EventStore,
    account: &ConnectorAccount,
) -> Result<ConnectorAccountHealthView, String> {
    let snapshot = store
        .connector_account_sync_health_snapshot(account, chrono::Utc::now())
        .map_err(store_error)?;
    Ok(ConnectorAccountHealthView::from_private_account(
        account, snapshot, false,
    ))
}

fn safe_authorization_review_view<S: ConnectorCredentialStore + Send>(
    event_store: &Arc<Mutex<crate::kernel::event_store::EventStore>>,
    runtime: &ConnectorRuntime<S>,
    snapshot: ConnectorAuthorizationReviewSnapshot,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationReviewView, String> {
    let session = snapshot.session();
    let mut account = None;
    let status = if session.status
        == crate::kernel::connectors::oauth::ConnectorAuthorizationStatus::RepairRequired
    {
        ConnectorAuthorizationUserStatus::RepairRequired
    } else {
        match snapshot.intent_state() {
            ConnectorAuthorizationReviewIntentState::Active => {
                let authority_valid = match snapshot.authority_handle() {
                    Some(handle) => runtime
                        .read_authorization_review_authority(handle)
                        .ok()
                        .is_some_and(|authority| {
                            event_store
                                .lock()
                                .ok()
                                .and_then(|store| {
                                    store
                                        .validate_connector_authorization_active_review_authority(
                                            snapshot.review_id(),
                                            &authority,
                                            now,
                                        )
                                        .ok()
                                })
                                .is_some()
                        }),
                    None => false,
                };
                if session.status
                    == crate::kernel::connectors::oauth::ConnectorAuthorizationStatus::Pending
                    && session.expires_at > now
                    && authority_valid
                {
                    ConnectorAuthorizationUserStatus::AwaitingConfirmation
                } else {
                    ConnectorAuthorizationUserStatus::RepairRequired
                }
            }
            ConnectorAuthorizationReviewIntentState::Approve => match session.status {
                crate::kernel::connectors::oauth::ConnectorAuthorizationStatus::Exchanging
                    if snapshot.exchange_claim_live() =>
                {
                    ConnectorAuthorizationUserStatus::Connecting
                }
                crate::kernel::connectors::oauth::ConnectorAuthorizationStatus::Completed
                    if snapshot.account_binding_valid() =>
                {
                    let private_account = snapshot.connected_account().ok_or_else(|| {
                        "connector authorization status could not be loaded safely".to_string()
                    })?;
                    let store = event_store.lock().map_err(|_| {
                        "connector authorization status could not be loaded safely".to_string()
                    })?;
                    account = Some(safe_account_health_view(&store, private_account).map_err(
                        |_| "connector authorization status could not be loaded safely".to_string(),
                    )?);
                    ConnectorAuthorizationUserStatus::Connected
                }
                _ => ConnectorAuthorizationUserStatus::RepairRequired,
            },
            ConnectorAuthorizationReviewIntentState::Cancel => {
                if session.status
                    == crate::kernel::connectors::oauth::ConnectorAuthorizationStatus::Cancelled
                {
                    if session.cleanup_required {
                        ConnectorAuthorizationUserStatus::Cancelling
                    } else {
                        ConnectorAuthorizationUserStatus::Cancelled
                    }
                } else {
                    ConnectorAuthorizationUserStatus::RepairRequired
                }
            }
        }
    };
    if status != ConnectorAuthorizationUserStatus::Connected {
        account = None;
    }
    Ok(ConnectorAuthorizationReviewView {
        review_id: snapshot.review_id(),
        provider_label: provider_label(&session.provider_id),
        abilities: user_abilities(&session.requested_capabilities),
        status,
        expires_at: session.expires_at,
        account,
    })
}

fn load_safe_authorization_review<S: ConnectorCredentialStore + Send>(
    event_store: &Arc<Mutex<crate::kernel::event_store::EventStore>>,
    runtime: &ConnectorRuntime<S>,
    review_id: Uuid,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationReviewView, String> {
    let snapshot = event_store
        .lock()
        .map_err(|_| "connector authorization status could not be loaded safely".to_string())?
        .connector_authorization_review_snapshot(review_id, now)
        .map_err(|_| "connector authorization status is unavailable".to_string())?;
    safe_authorization_review_view(event_store, runtime, snapshot, now)
}

fn cancel_connector_authorization_review<S: ConnectorCredentialStore + Send>(
    event_store: &Arc<Mutex<crate::kernel::event_store::EventStore>>,
    runtime: &ConnectorRuntime<S>,
    review_id: Uuid,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationReviewView, String> {
    let active = event_store
        .lock()
        .map_err(|_| "connector authorization review is unavailable".to_string())?
        .connector_authorization_active_review(review_id, now)
        .map_err(|_| "connector authorization review is unavailable".to_string())?;
    let authority = runtime
        .read_authorization_review_authority(active.authority_handle())
        .map_err(|_| "connector authorization review is unavailable".to_string())?;
    let resolution = event_store
        .lock()
        .map_err(|_| "connector authorization review is unavailable".to_string())?
        .resolve_connector_authorization_review(
            review_id,
            &authority,
            crate::kernel::connectors::oauth::ConnectorAuthorizationIntent::Cancel,
            now,
        )
        .map_err(|_| "connector authorization review is unavailable".to_string())?;
    let crate::kernel::event_store::ConnectorAuthorizationResolution::Cancelled(claim) = resolution
    else {
        return Err("connector authorization review is unavailable".to_string());
    };
    let authorization_id = claim.session().id;
    let _ = runtime.with_authorization_fence(authorization_id, || {
        runtime.delete_authorization_handles_and_review(
            claim.session(),
            claim.action_authority_handle(),
        )?;
        event_store
            .lock()
            .map_err(|_| "connector authorization cleanup is still pending".to_string())?
            .finish_connector_authorization_cleanup(&claim, now)
            .map_err(|_| "connector authorization cleanup is still pending".to_string())
    });
    let _ = recover_authorization_authority_cleanup_with_runtime(event_store, runtime, now, 1);
    load_safe_authorization_review(event_store, runtime, review_id, now)
}

pub(crate) fn resolve_connector_authorization_review_with_registry<
    S: ConnectorCredentialStore + Send,
>(
    event_store: &Arc<Mutex<crate::kernel::event_store::EventStore>>,
    runtime: &ConnectorRuntime<S>,
    registry: &dyn ConnectorOAuthRegistry,
    request: ConnectorAuthorizationResolveRequest,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationReviewView, String> {
    if request.intent == ConnectorAuthorizationReviewIntent::Cancel {
        return cancel_connector_authorization_review(event_store, runtime, request.review_id, now);
    }

    let provider_id = event_store
        .lock()
        .map_err(|_| "connector authorization could not be completed safely".to_string())?
        .connector_authorization_review_snapshot(request.review_id, now)
        .map_err(|_| "connector authorization is unavailable".to_string())?
        .session()
        .provider_id
        .clone();
    let provider = registry
        .provider(&provider_id)
        .ok_or_else(|| "connector authorization is not available yet".to_string())?;

    let active = event_store
        .lock()
        .map_err(|_| "connector authorization could not be completed safely".to_string())?
        .connector_authorization_active_review(request.review_id, now)
        .map_err(|_| "connector authorization is unavailable".to_string())?;
    let authority = runtime
        .read_authorization_review_authority(active.authority_handle())
        .map_err(|_| "connector authorization could not be completed safely".to_string())?;
    let resolution = event_store
        .lock()
        .map_err(|_| "connector authorization could not be completed safely".to_string())?
        .resolve_connector_authorization_review(
            request.review_id,
            &authority,
            crate::kernel::connectors::oauth::ConnectorAuthorizationIntent::Approve,
            now,
        )
        .map_err(|_| "connector authorization could not be completed safely".to_string())?;
    let crate::kernel::event_store::ConnectorAuthorizationResolution::Approved(claim) = resolution
    else {
        return Err("connector authorization could not be completed safely".to_string());
    };

    let completion = crate::kernel::connectors::oauth::execute_review_authorization_with_runtime(
        runtime, provider, &claim,
    )
    .map_err(|_| "connector authorization could not be completed safely".to_string())?;
    let completion_now = Utc::now();
    crate::kernel::connectors::oauth::finalize_claimed_authorization_with_shared_runtime(
        event_store,
        runtime,
        claim,
        completion,
        completion_now,
    )
    .map_err(|_| "connector authorization could not be completed safely".to_string())?;
    let _ = recover_authorization_authority_cleanup_with_runtime(
        event_store,
        runtime,
        completion_now,
        1,
    );
    load_safe_authorization_review(event_store, runtime, request.review_id, completion_now)
}

fn reconcile_connector_attachment_landings(
    state: &AppState,
    include_abandoned_executions: bool,
) -> Result<usize, String> {
    let event_store = state.event_store();
    let mut reconciled = 0usize;
    #[cfg(windows)]
    {
        let ready = {
            let store = event_store
                .lock()
                .map_err(|_| "event store lock failed".to_string())?;
            if include_abandoned_executions {
                store.claim_startup_ready_connector_attachment_recovery_candidates(
                    chrono::Utc::now(),
                    64,
                )
            } else {
                store.claim_runtime_ready_connector_attachment_recovery_candidates(
                    chrono::Utc::now(),
                    64,
                )
            }
            .map_err(store_error)?
        };
        for candidate in ready {
            if event_store
                .lock()
                .map_err(|_| "event store lock failed".to_string())?
                .renew_connector_attachment_recovery_claim(
                    candidate.landing_id,
                    candidate.claim_id,
                    chrono::Utc::now(),
                )
                .is_err()
            {
                continue;
            }
            match recover_ready_connector_attachment(&candidate) {
                Ok(landed) => {
                    let completed = event_store
                        .lock()
                        .map_err(|_| "event store lock failed".to_string())?
                        .complete_recovered_connector_attachment_landing(
                            &landed,
                            candidate.claim_id,
                            chrono::Utc::now(),
                        )
                        .is_ok();
                    drop(landed);
                    if completed {
                        reconciled += 1;
                        continue;
                    }
                    let transitioned = event_store
                        .lock()
                        .map_err(|_| "event store lock failed".to_string())?
                        .transition_ready_recovery_to_cleanup(
                            candidate.landing_id,
                            candidate.claim_id,
                            "ready_generation_changed",
                            chrono::Utc::now(),
                        )
                        .is_ok();
                    if !transitioned {
                        continue;
                    }
                    match cleanup_incomplete_connector_attachment(&candidate) {
                        Ok(()) => {
                            let _ = event_store
                                .lock()
                                .map_err(|_| "event store lock failed".to_string())?
                                .fail_connector_attachment_after_cleanup(
                                    candidate.landing_id,
                                    candidate.claim_id,
                                    chrono::Utc::now(),
                                );
                        }
                        Err(ConnectorAttachmentCleanupFailure::Unsafe) => {
                            let _ = event_store
                                .lock()
                                .map_err(|_| "event store lock failed".to_string())?
                                .mark_connector_attachment_repair_required(
                                    candidate.landing_id,
                                    candidate.claim_id,
                                    "ready_cleanup_identity_conflict",
                                    chrono::Utc::now(),
                                );
                        }
                        Err(ConnectorAttachmentCleanupFailure::Transient) => {
                            let _ = event_store
                                .lock()
                                .map_err(|_| "event store lock failed".to_string())?
                                .defer_connector_attachment_cleanup(
                                    candidate.landing_id,
                                    candidate.claim_id,
                                    chrono::Utc::now(),
                                );
                        }
                    }
                }
                Err(ConnectorAttachmentReadyRecoveryFailure::Missing) => {
                    let transitioned = event_store
                        .lock()
                        .map_err(|_| "event store lock failed".to_string())?
                        .transition_ready_recovery_to_cleanup(
                            candidate.landing_id,
                            candidate.claim_id,
                            "ready_file_missing",
                            chrono::Utc::now(),
                        )
                        .is_ok();
                    if transitioned {
                        let _ = event_store
                            .lock()
                            .map_err(|_| "event store lock failed".to_string())?
                            .fail_connector_attachment_after_cleanup(
                                candidate.landing_id,
                                candidate.claim_id,
                                chrono::Utc::now(),
                            );
                    }
                }
                Err(ConnectorAttachmentReadyRecoveryFailure::Unsafe) => {
                    let transitioned = event_store
                        .lock()
                        .map_err(|_| "event store lock failed".to_string())?
                        .transition_ready_recovery_to_cleanup(
                            candidate.landing_id,
                            candidate.claim_id,
                            "ready_identity_conflict",
                            chrono::Utc::now(),
                        )
                        .is_ok();
                    if transitioned {
                        let _ = event_store
                            .lock()
                            .map_err(|_| "event store lock failed".to_string())?
                            .mark_connector_attachment_repair_required(
                                candidate.landing_id,
                                candidate.claim_id,
                                "ready_identity_conflict",
                                chrono::Utc::now(),
                            );
                    }
                }
                Err(ConnectorAttachmentReadyRecoveryFailure::Transient) => {
                    let _ = event_store
                        .lock()
                        .map_err(|_| "event store lock failed".to_string())?
                        .defer_connector_attachment_ready_recovery(
                            candidate.landing_id,
                            candidate.claim_id,
                            chrono::Utc::now(),
                        );
                }
            }
        }

        let retention = event_store
            .lock()
            .map_err(|_| "event store lock failed".to_string())?
            .claim_expired_connector_attachment_retention_candidates(chrono::Utc::now(), 64)
            .map_err(store_error)?;
        for candidate in retention {
            if event_store
                .lock()
                .map_err(|_| "event store lock failed".to_string())?
                .renew_connector_attachment_recovery_claim(
                    candidate.landing_id,
                    candidate.claim_id,
                    chrono::Utc::now(),
                )
                .is_err()
            {
                continue;
            }
            match cleanup_incomplete_connector_attachment(&candidate) {
                Ok(()) => {
                    if event_store
                        .lock()
                        .map_err(|_| "event store lock failed".to_string())?
                        .complete_connector_attachment_retention(
                            candidate.landing_id,
                            candidate.claim_id,
                            chrono::Utc::now(),
                        )
                        .is_ok()
                    {
                        reconciled += 1;
                    }
                }
                Err(ConnectorAttachmentCleanupFailure::Unsafe) => {
                    let _ = event_store
                        .lock()
                        .map_err(|_| "event store lock failed".to_string())?
                        .mark_connector_attachment_retention_repair_required(
                            candidate.landing_id,
                            candidate.claim_id,
                            "retention_identity_conflict",
                            chrono::Utc::now(),
                        );
                }
                Err(ConnectorAttachmentCleanupFailure::Transient) => {
                    let _ = event_store
                        .lock()
                        .map_err(|_| "event store lock failed".to_string())?
                        .defer_connector_attachment_cleanup(
                            candidate.landing_id,
                            candidate.claim_id,
                            chrono::Utc::now(),
                        );
                }
            }
        }
    }
    let mut attempted = std::collections::HashSet::new();
    for _ in 0..32 {
        let candidates = if include_abandoned_executions {
            event_store
                .lock()
                .map_err(|_| "event store lock failed".to_string())?
                .claim_startup_connector_attachment_cleanup_candidates(chrono::Utc::now(), 32)
                .map_err(store_error)?
        } else {
            event_store
                .lock()
                .map_err(|_| "event store lock failed".to_string())?
                .claim_runtime_connector_attachment_cleanup_candidates(chrono::Utc::now(), 32)
                .map_err(store_error)?
        };
        let candidates = candidates
            .into_iter()
            .filter(|candidate| attempted.insert(candidate.landing_id))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            break;
        }
        for candidate in candidates {
            if event_store
                .lock()
                .map_err(|_| "event store lock failed".to_string())?
                .renew_connector_attachment_recovery_claim(
                    candidate.landing_id,
                    candidate.claim_id,
                    chrono::Utc::now(),
                )
                .is_err()
            {
                continue;
            }
            match cleanup_incomplete_connector_attachment(&candidate) {
                Ok(()) => {}
                Err(ConnectorAttachmentCleanupFailure::Unsafe) => {
                    let _ = event_store
                        .lock()
                        .map_err(|_| "event store lock failed".to_string())?
                        .mark_connector_attachment_repair_required(
                            candidate.landing_id,
                            candidate.claim_id,
                            "unsafe_cleanup_boundary",
                            chrono::Utc::now(),
                        );
                    continue;
                }
                Err(ConnectorAttachmentCleanupFailure::Transient) => {
                    let _ = event_store
                        .lock()
                        .map_err(|_| "event store lock failed".to_string())?
                        .defer_connector_attachment_cleanup(
                            candidate.landing_id,
                            candidate.claim_id,
                            chrono::Utc::now(),
                        );
                    continue;
                }
            }
            if event_store
                .lock()
                .map_err(|_| "event store lock failed".to_string())?
                .fail_connector_attachment_after_cleanup(
                    candidate.landing_id,
                    candidate.claim_id,
                    chrono::Utc::now(),
                )
                .is_ok()
            {
                reconciled += 1;
            }
        }
    }
    Ok(reconciled)
}

pub fn reconcile_incomplete_connector_attachment_landings(
    state: &AppState,
) -> Result<usize, String> {
    state
        .event_store()
        .lock()
        .map_err(|_| "event store lock failed".to_string())?
        .reset_stale_connector_attachment_recovery_claims(chrono::Utc::now())
        .map_err(store_error)?;
    reconcile_connector_attachment_landings(state, true)
}

pub fn spawn_connector_attachment_recovery_worker(state: AppState) {
    let _ = std::thread::Builder::new()
        .name("ds-agent-connector-recovery".to_string())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            let _ = reconcile_connector_attachment_landings(&state, false);
        });
}

const CONNECTOR_RECONCILIATION_WORKER_LIMIT: usize = 8;
const CONNECTOR_RECONCILIATION_WORKER_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(30);

fn connector_reconciliation_worker_signal() -> &'static (Mutex<u64>, Condvar) {
    static SIGNAL: OnceLock<(Mutex<u64>, Condvar)> = OnceLock::new();
    SIGNAL.get_or_init(|| (Mutex::new(0), Condvar::new()))
}

fn run_connector_reconciliation_worker_once(state: &AppState) {
    let registry = state.connector_reconcilers();
    if !registry.execution_enabled() {
        return;
    }
    let _ = reconcile_due_connector_mutations_with_shared_store(
        &state.event_store(),
        registry.as_ref(),
        CONNECTOR_RECONCILIATION_WORKER_LIMIT,
    );
}

pub fn spawn_connector_reconciliation_worker(state: AppState) {
    if !state.connector_reconcilers().execution_enabled() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("ds-agent-connector-reconciliation".to_string())
        .spawn(move || {
            run_connector_reconciliation_worker_once(&state);
            loop {
                let (generation, wake) = connector_reconciliation_worker_signal();
                let observed = generation.lock().map(|value| *value).unwrap_or_default();
                if let Ok(guard) = generation.lock() {
                    let _ = wake.wait_timeout_while(
                        guard,
                        CONNECTOR_RECONCILIATION_WORKER_INTERVAL,
                        |current| *current == observed,
                    );
                }
                run_connector_reconciliation_worker_once(&state);
            }
        });
}

fn wake_connector_reconciliation_worker(state: &AppState) {
    if !state.connector_reconcilers().execution_enabled() {
        return;
    }
    let (generation, wake) = connector_reconciliation_worker_signal();
    if let Ok(mut current) = generation.lock() {
        *current = current.wrapping_add(1);
        wake.notify_one();
    }
}

const CONNECTOR_SYNC_RECOVERY_WORKER_LIMIT: usize = 8;
const CONNECTOR_SYNC_RECOVERY_WORKER_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(30);

struct ConnectorSyncRecoveryWorkerSignal {
    generation: Mutex<u64>,
    wake: Condvar,
}

impl ConnectorSyncRecoveryWorkerSignal {
    fn new() -> Self {
        Self {
            generation: Mutex::new(0),
            wake: Condvar::new(),
        }
    }

    fn snapshot(&self) -> u64 {
        self.generation
            .lock()
            .map(|value| *value)
            .unwrap_or_default()
    }

    fn notify(&self) {
        if let Ok(mut current) = self.generation.lock() {
            *current = current.wrapping_add(1);
            self.wake.notify_one();
        }
    }

    fn wait_after_sweep(&self, last_seen: u64, timeout: std::time::Duration) -> (u64, bool) {
        let Ok(guard) = self.generation.lock() else {
            return (last_seen, false);
        };
        if *guard != last_seen {
            return (*guard, false);
        }
        match self
            .wake
            .wait_timeout_while(guard, timeout, |current| *current == last_seen)
        {
            Ok((guard, outcome)) => (*guard, outcome.timed_out()),
            Err(poisoned) => {
                let (guard, _) = poisoned.into_inner();
                (*guard, false)
            }
        }
    }
}

fn connector_sync_recovery_worker_signal() -> &'static ConnectorSyncRecoveryWorkerSignal {
    static SIGNAL: OnceLock<ConnectorSyncRecoveryWorkerSignal> = OnceLock::new();
    SIGNAL.get_or_init(ConnectorSyncRecoveryWorkerSignal::new)
}

fn run_connector_sync_recovery_worker_once(state: &AppState) {
    let registry = state.connector_syncs();
    if !registry.execution_enabled() {
        return;
    }
    let _ = run_connector_sync_recovery_with_shared_store(
        &state.event_store(),
        registry.as_ref(),
        CONNECTOR_SYNC_RECOVERY_WORKER_LIMIT,
    );
}

pub fn spawn_connector_sync_recovery_worker(state: AppState) {
    if !state.connector_syncs().execution_enabled() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("ds-agent-connector-sync-recovery".to_string())
        .spawn(move || {
            let signal = connector_sync_recovery_worker_signal();
            let mut last_seen = signal.snapshot();
            loop {
                run_connector_sync_recovery_worker_once(&state);
                (last_seen, _) =
                    signal.wait_after_sweep(last_seen, CONNECTOR_SYNC_RECOVERY_WORKER_INTERVAL);
            }
        });
}

fn notify_connector_sync_recovery_if_accepted(
    signal: &ConnectorSyncRecoveryWorkerSignal,
    acceptance: ConnectorRecoveryAcceptance,
) {
    if acceptance == ConnectorRecoveryAcceptance::Accepted {
        signal.notify();
    }
}

fn wake_connector_sync_recovery_worker(state: &AppState, acceptance: ConnectorRecoveryAcceptance) {
    if !state.connector_syncs().execution_enabled() {
        return;
    }
    notify_connector_sync_recovery_if_accepted(connector_sync_recovery_worker_signal(), acceptance);
}

const CONNECTOR_READ_WORKER_LIMIT: usize = 8;
const CONNECTOR_READ_WORKER_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

fn connector_read_worker_signal() -> &'static ConnectorSyncRecoveryWorkerSignal {
    static SIGNAL: OnceLock<ConnectorSyncRecoveryWorkerSignal> = OnceLock::new();
    SIGNAL.get_or_init(ConnectorSyncRecoveryWorkerSignal::new)
}

fn run_connector_read_worker_once(state: &AppState) {
    let registry = state.connector_reads();
    if !registry.execution_enabled() {
        return;
    }
    let _ =
        crate::kernel::connectors::read_execution::run_connector_read_executions_with_shared_store(
            &state.event_store(),
            registry.as_ref(),
            CONNECTOR_READ_WORKER_LIMIT,
        );
}

pub fn spawn_connector_read_worker(state: AppState) {
    if !state.connector_reads().execution_enabled() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("ds-agent-connector-read".to_string())
        .spawn(move || {
            let signal = connector_read_worker_signal();
            let mut last_seen = signal.snapshot();
            loop {
                run_connector_read_worker_once(&state);
                (last_seen, _) = signal.wait_after_sweep(last_seen, CONNECTOR_READ_WORKER_INTERVAL);
            }
        });
}

fn wake_connector_read_worker(state: &AppState, acceptance: ExplicitConnectorReadAcceptance) {
    if acceptance == ExplicitConnectorReadAcceptance::Accepted
        && state.connector_reads().execution_enabled()
    {
        connector_read_worker_signal().notify();
    }
}

#[tauri::command]
pub fn list_connector_account_summaries(
    state: State<'_, AppState>,
) -> Result<Vec<ConnectorAccountHealthView>, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .list_connector_accounts()
        .map_err(store_error)?
        .iter()
        .map(|account| safe_account_health_view(&store, account))
        .collect()
}

#[tauri::command]
pub fn list_connector_read_activity(
    state: State<'_, AppState>,
) -> Result<Vec<ConnectorReadExecutionView>, String> {
    let event_store = state.event_store();
    let store = event_store
        .lock()
        .map_err(|_| "connector read activity is unavailable".to_string())?;
    store
        .list_connector_read_activity(50)
        .map_err(|_| "connector read activity is unavailable".to_string())
}

#[tauri::command]
pub fn start_explicit_connector_mail_search(
    request: ExplicitConnectorMailSearchRequest,
    state: State<'_, AppState>,
) -> Result<ExplicitConnectorReadCommandResult, String> {
    let plan = ConnectorReadPlan::mail_search(request.query, request.max_results)
        .map_err(|_| "connector read request is invalid".to_string())?;
    let result = submit_explicit_connector_read_for_state(
        request.source_invocation_id,
        request.account_id,
        plan,
        state.inner(),
        Utc::now(),
    )?;
    wake_connector_read_worker(state.inner(), result.acceptance);
    Ok(result)
}

#[tauri::command]
pub fn start_explicit_connector_calendar_list(
    request: ExplicitConnectorCalendarListRequest,
    state: State<'_, AppState>,
) -> Result<ExplicitConnectorReadCommandResult, String> {
    let plan =
        ConnectorReadPlan::calendar_list(request.starts_at, request.ends_at, request.max_results)
            .map_err(|_| "connector read request is invalid".to_string())?;
    let result = submit_explicit_connector_read_for_state(
        request.source_invocation_id,
        request.account_id,
        plan,
        state.inner(),
        Utc::now(),
    )?;
    wake_connector_read_worker(state.inner(), result.acceptance);
    Ok(result)
}

fn submit_explicit_connector_read_for_state(
    source_invocation_id: Uuid,
    account_id: Uuid,
    plan: ConnectorReadPlan,
    state: &AppState,
    now: DateTime<Utc>,
) -> Result<ExplicitConnectorReadCommandResult, String> {
    const UNAVAILABLE: &str = "connector read is unavailable";
    let registry = state.connector_reads();
    if !registry.execution_enabled() {
        return Err(UNAVAILABLE.to_string());
    }
    let event_store = state.event_store();
    let store = event_store.lock().map_err(|_| UNAVAILABLE.to_string())?;
    let account = store
        .connector_account_for_read(account_id)
        .map_err(|_| UNAVAILABLE.to_string())?
        .ok_or_else(|| UNAVAILABLE.to_string())?;
    let provider_available = match &plan {
        ConnectorReadPlan::MailSearch { .. } => {
            registry.mail_provider(&account.provider_id).is_some()
        }
        ConnectorReadPlan::CalendarList { .. } => {
            registry.calendar_provider(&account.provider_id).is_some()
        }
    };
    if !provider_available {
        return Err(UNAVAILABLE.to_string());
    }
    let (acceptance, execution) = store
        .submit_explicit_connector_read_execution(source_invocation_id, account_id, plan, now)
        .map_err(|_| UNAVAILABLE.to_string())?;
    Ok(ExplicitConnectorReadCommandResult {
        acceptance: match acceptance {
            ConnectorReadSubmission::Accepted => ExplicitConnectorReadAcceptance::Accepted,
            ConnectorReadSubmission::AlreadyAccepted => {
                ExplicitConnectorReadAcceptance::AlreadyAccepted
            }
        },
        execution: execution.public_view(),
    })
}

#[tauri::command]
pub fn list_connector_authorization_reviews(
    state: State<'_, AppState>,
) -> Result<Vec<ConnectorAuthorizationReviewView>, String> {
    #[cfg(not(windows))]
    {
        let _ = state;
        return Ok(Vec::new());
    }
    #[cfg(windows)]
    {
        let event_store = state.event_store();
        let runtime = state.connector_runtime();
        let now = Utc::now();
        let snapshots = {
            let store = event_store.lock().map_err(|_| {
                "connector authorization status could not be loaded safely".to_string()
            })?;
            store
                .connector_authorization_review_ids(32)
                .map_err(|_| {
                    "connector authorization status could not be loaded safely".to_string()
                })?
                .into_iter()
                .filter_map(|review_id| {
                    store
                        .connector_authorization_review_snapshot(review_id, now)
                        .ok()
                })
                .collect::<Vec<_>>()
        };
        snapshots
            .into_iter()
            .map(|snapshot| {
                safe_authorization_review_view(&event_store, runtime.as_ref(), snapshot, now)
            })
            .collect()
    }
}

#[tauri::command]
pub fn get_connector_authorization_review(
    review_id: Uuid,
    state: State<'_, AppState>,
) -> Result<ConnectorAuthorizationReviewView, String> {
    #[cfg(not(windows))]
    {
        let _ = (review_id, state);
        return Err("connector authorization status is unavailable".to_string());
    }
    #[cfg(windows)]
    {
        let event_store = state.event_store();
        let runtime = state.connector_runtime();
        let now = Utc::now();
        let snapshot = event_store
            .lock()
            .map_err(|_| "connector authorization status could not be loaded safely".to_string())?
            .connector_authorization_review_snapshot(review_id, now)
            .map_err(|_| "connector authorization status is unavailable".to_string())?;
        safe_authorization_review_view(&event_store, runtime.as_ref(), snapshot, now)
    }
}

#[tauri::command]
pub fn resolve_connector_authorization_review(
    request: ConnectorAuthorizationResolveRequest,
    state: State<'_, AppState>,
) -> Result<ConnectorAuthorizationReviewView, String> {
    #[cfg(not(windows))]
    {
        let _ = (request, state);
        return Err("connector authorization is unavailable".to_string());
    }
    #[cfg(windows)]
    {
        let event_store = state.event_store();
        let runtime = state.connector_runtime();
        let registry = state.connector_oauth_providers();
        resolve_connector_authorization_review_with_registry(
            &event_store,
            runtime.as_ref(),
            registry.as_ref(),
            request,
            Utc::now(),
        )
    }
}

#[tauri::command]
pub fn list_connector_provider_catalog() -> Vec<ConnectorProviderDescriptor> {
    production_connector_catalog()
}

#[tauri::command]
pub fn list_connector_recovery_items(
    state: State<'_, AppState>,
) -> Result<Vec<ConnectorRecoveryItem>, String> {
    list_connector_recovery_items_for_state(state.inner())
}

fn list_connector_recovery_items_for_state(
    state: &AppState,
) -> Result<Vec<ConnectorRecoveryItem>, String> {
    let store = state.event_store();
    let registry = state.connector_reconcilers();
    let sync_registry = state.connector_syncs();
    let store = store
        .lock()
        .map_err(|_| "recovery items could not be loaded safely".to_string())?;
    store
        .list_connector_recovery_items_with_runtime_registries(
            registry.as_ref(),
            sync_registry.as_ref(),
        )
        .map_err(|_| "recovery items could not be loaded safely".to_string())
}

#[tauri::command]
pub fn retry_connector_attachment_cleanup(
    request: RetryConnectorAttachmentCleanupRequest,
    state: State<'_, AppState>,
) -> Result<ConnectorRecoveryCommandResult, String> {
    validate_recovery_action_revision(&request.action_revision)?;
    let event_store = state.event_store();
    let acceptance = event_store
        .lock()
        .map_err(|_| "recovery item could not be queued safely".to_string())?
        .retry_connector_attachment_recovery(
            request.item_id,
            &request.action_revision,
            chrono::Utc::now(),
        )
        .map_err(|_| "recovery item could not be queued safely".to_string())?;
    let items = event_store
        .lock()
        .map_err(|_| "recovery items could not be loaded safely".to_string())?
        .list_connector_recovery_items_with_runtime_registries(
            state.connector_reconcilers().as_ref(),
            state.connector_syncs().as_ref(),
        )
        .map_err(|_| "recovery items could not be loaded safely".to_string())?;
    Ok(ConnectorRecoveryCommandResult { acceptance, items })
}

#[tauri::command]
pub fn resume_connector_read_sync(
    request: ResumeConnectorReadSyncRequest,
    state: State<'_, AppState>,
) -> Result<ConnectorRecoveryCommandResult, String> {
    let result = resume_connector_read_sync_for_state(request, state.inner(), chrono::Utc::now())?;
    wake_connector_sync_recovery_worker(state.inner(), result.acceptance);
    Ok(result)
}

fn resume_connector_read_sync_for_state(
    request: ResumeConnectorReadSyncRequest,
    state: &AppState,
    changed_at: chrono::DateTime<chrono::Utc>,
) -> Result<ConnectorRecoveryCommandResult, String> {
    validate_recovery_action_revision(&request.action_revision)?;
    let event_store = state.event_store();
    let sync_registry = state.connector_syncs();
    let acceptance = event_store
        .lock()
        .map_err(|_| "sync recovery could not be scheduled safely".to_string())?
        .resume_connector_read_sync_from_recovery_with_sync_registry(
            request.item_id,
            &request.action_revision,
            sync_registry.as_ref(),
            changed_at,
        )
        .map_err(|_| "sync recovery could not be scheduled safely".to_string())?;
    let items = event_store
        .lock()
        .map_err(|_| "recovery items could not be loaded safely".to_string())?
        .list_connector_recovery_items_with_runtime_registries(
            state.connector_reconcilers().as_ref(),
            sync_registry.as_ref(),
        )
        .map_err(|_| "recovery items could not be loaded safely".to_string())?;
    Ok(ConnectorRecoveryCommandResult { acceptance, items })
}

#[tauri::command]
pub fn inspect_connector_external_result(
    request: InspectConnectorExternalResultRequest,
    state: State<'_, AppState>,
) -> Result<ConnectorRecoveryCommandResult, String> {
    validate_recovery_action_revision(&request.action_revision)?;
    let event_store = state.event_store();
    let registry = state.connector_reconcilers();
    let acceptance = event_store
        .lock()
        .map_err(|_| "external result check could not be scheduled safely".to_string())?
        .schedule_connector_reconciliation_from_recovery(
            request.item_id,
            &request.action_revision,
            registry.as_ref(),
            chrono::Utc::now(),
        )
        .map_err(|_| "external result check could not be scheduled safely".to_string())?;
    if acceptance == ConnectorRecoveryAcceptance::Accepted {
        wake_connector_reconciliation_worker(state.inner());
    }
    let items = event_store
        .lock()
        .map_err(|_| "recovery items could not be loaded safely".to_string())?
        .list_connector_recovery_items_with_runtime_registries(
            registry.as_ref(),
            state.connector_syncs().as_ref(),
        )
        .map_err(|_| "recovery items could not be loaded safely".to_string())?;
    Ok(ConnectorRecoveryCommandResult { acceptance, items })
}

#[tauri::command]
pub fn disconnect_connector_account(
    account_id: Uuid,
    state: State<'_, AppState>,
) -> Result<ConnectorAccountHealthView, String> {
    #[cfg(not(windows))]
    {
        let _ = (account_id, state);
        return Err("connector credential storage is not available on this platform".to_string());
    }

    #[cfg(windows)]
    {
        let event_store = state.event_store();
        let account = {
            let store = event_store
                .lock()
                .map_err(|_| "event store lock failed".to_string())?;
            store
                .list_connector_accounts()
                .map_err(store_error)?
                .into_iter()
                .find(|account| account.id == account_id)
                .ok_or_else(|| "connector account was not found".to_string())?
        };
        if account.health == ConnectorHealth::Disconnected {
            let store = event_store
                .lock()
                .map_err(|_| "event store lock failed".to_string())?;
            return safe_account_health_view(&store, &account);
        };
        let ticket = event_store
            .lock()
            .map_err(|_| "event store lock failed".to_string())?
            .begin_connector_disconnect(account_id, chrono::Utc::now())
            .map_err(store_error)?;
        let credential_delete_outcome = match state
            .connector_runtime()
            .delete_account_credential(ticket.account())
        {
            Ok(outcome) => outcome,
            Err(error) => {
                let _ = event_store
                    .lock()
                    .map_err(|_| "event store lock failed".to_string())?
                    .record_connector_disconnect_failure(
                        &ticket,
                        ConnectorDisconnectSource::User,
                        chrono::Utc::now(),
                    );
                return Err(error);
            }
        };
        let account = event_store
            .lock()
            .map_err(|_| "event store lock failed".to_string())?
            .complete_connector_disconnect(
                &ticket,
                ConnectorDisconnectSource::User,
                credential_delete_outcome,
                chrono::Utc::now(),
            )
            .map_err(store_error)?;
        let store = event_store
            .lock()
            .map_err(|_| "event store lock failed".to_string())?;
        safe_account_health_view(&store, &account)
    }
}

#[cfg(windows)]
pub fn reconcile_pending_connector_disconnects(state: &AppState) -> Result<usize, String> {
    let event_store = state.event_store();
    reconcile_pending_connector_disconnects_with(&event_store, state.connector_runtime().as_ref())
}

#[cfg(windows)]
pub fn recover_pending_connector_authorizations(state: &AppState) -> Result<usize, String> {
    let event_store = state.event_store();
    recover_due_connector_authorizations_with_runtime(
        &event_store,
        state.connector_runtime().as_ref(),
        chrono::Utc::now(),
        64,
    )
}

#[cfg(windows)]
fn recover_due_connector_authorizations_with_runtime<S>(
    event_store: &std::sync::Arc<std::sync::Mutex<crate::kernel::event_store::EventStore>>,
    runtime: &ConnectorRuntime<S>,
    now: chrono::DateTime<chrono::Utc>,
    limit: usize,
) -> Result<usize, String>
where
    S: ConnectorCredentialStore + Send,
{
    let mut completed = 0usize;
    let candidates = event_store
        .lock()
        .map_err(|_| "authorization recovery state is unavailable".to_string())?
        .connector_authorization_cleanup_candidates(now, limit)
        .map_err(|_| "authorization recovery state is unavailable".to_string())?;
    for id in candidates {
        let recovered = runtime.with_authorization_fence(id, || {
            let claim = event_store
                .lock()
                .map_err(|_| "authorization recovery state is unavailable".to_string())?
                .begin_connector_authorization_cleanup(id, now)
                .map_err(|_| "authorization recovery state is unavailable".to_string())?;
            runtime.delete_authorization_handles_and_review(
                claim.session(),
                claim.action_authority_handle(),
            )?;
            event_store
                .lock()
                .map_err(|_| "authorization recovery state is unavailable".to_string())?
                .finish_connector_authorization_cleanup(&claim, now)
                .map_err(|_| "authorization recovery state is unavailable".to_string())?;
            Ok(())
        });
        if recovered.is_ok() {
            completed += 1;
        }
    }
    completed +=
        recover_authorization_authority_cleanup_with_runtime(event_store, runtime, now, limit)?;
    Ok(completed)
}

fn recover_authorization_authority_cleanup_with_runtime<S>(
    event_store: &std::sync::Arc<std::sync::Mutex<crate::kernel::event_store::EventStore>>,
    runtime: &ConnectorRuntime<S>,
    now: chrono::DateTime<chrono::Utc>,
    limit: usize,
) -> Result<usize, String>
where
    S: ConnectorCredentialStore + Send,
{
    let candidates = event_store
        .lock()
        .map_err(|_| "authorization recovery state is unavailable".to_string())?
        .connector_authorization_authority_cleanup_candidates(now, limit)
        .map_err(|_| "authorization recovery state is unavailable".to_string())?;
    let mut completed = 0usize;
    for review_id in candidates {
        let claim = match event_store
            .lock()
            .map_err(|_| "authorization recovery state is unavailable".to_string())?
            .begin_connector_authorization_authority_cleanup(review_id, now)
        {
            Ok(claim) => claim,
            Err(_) => continue,
        };
        if runtime
            .delete_authorization_review_authority(claim.authority_handle())
            .is_err()
        {
            continue;
        }
        if event_store
            .lock()
            .map_err(|_| "authorization recovery state is unavailable".to_string())?
            .finish_connector_authorization_authority_cleanup(&claim, now)
            .is_ok()
        {
            completed += 1;
        }
    }
    Ok(completed)
}

fn reconcile_pending_connector_disconnects_with<S>(
    event_store: &std::sync::Arc<std::sync::Mutex<crate::kernel::event_store::EventStore>>,
    runtime: &ConnectorRuntime<S>,
) -> Result<usize, String>
where
    S: ConnectorCredentialStore + Send,
{
    let tickets = event_store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?
        .list_pending_connector_disconnects(32)
        .map_err(store_error)?;
    let mut recovered = 0;
    for ticket in tickets {
        let credential_delete_outcome = match runtime.delete_account_credential(ticket.account()) {
            Ok(outcome) => outcome,
            Err(_) => {
                let _ = event_store
                    .lock()
                    .map_err(|_| "event store lock failed".to_string())?
                    .record_connector_disconnect_failure(
                        &ticket,
                        ConnectorDisconnectSource::Startup,
                        chrono::Utc::now(),
                    );
                continue;
            }
        };
        if event_store
            .lock()
            .map_err(|_| "event store lock failed".to_string())?
            .complete_connector_disconnect(
                &ticket,
                ConnectorDisconnectSource::Startup,
                credential_delete_outcome,
                chrono::Utc::now(),
            )
            .is_ok()
        {
            recovered += 1;
        }
    }
    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    };
    use uuid::Uuid;

    use crate::kernel::connectors::domain::{CalendarEvent, MailMessage};
    use crate::kernel::connectors::provider::{
        CalendarConnectorProvider, CalendarListRequest, ConnectorProviderFailure,
        ConnectorProviderResult, ConnectorReadContinuation, ConnectorReadPage,
        MailConnectorProvider, MailSearchRequest, MailThreadRequest,
    };
    use crate::kernel::connectors::read_execution::ConnectorReadExecutionKind;
    use crate::kernel::connectors::reconciliation::EmptyConnectorReconcilerRegistry;
    use crate::kernel::connectors::runtime_registry::{
        ConnectorReadRegistry, ConnectorSyncRegistry,
    };
    use crate::kernel::connectors::sync::{
        CalendarSyncProvider, CalendarSyncRequest, ConnectorOpaqueContinuation,
        ConnectorSyncContinuation, ConnectorSyncFailure, ConnectorSyncPage, ConnectorSyncPlan,
        ConnectorSyncState, ConnectorSyncStateRecovery, MailSyncProvider, MailSyncRequest,
    };
    use crate::kernel::connectors::{
        ConnectorAccount, ConnectorCapability, ConnectorCredentialDeleteOutcome,
        ConnectorCredentialHandle, ConnectorCredentialStore, ConnectorHealth, ConnectorSecret,
        FakeConnectorCredentialStore,
    };
    use crate::kernel::event_store::EventStore;

    struct EnabledEmptySyncRegistry;

    struct EnabledEmptyReadRegistry;

    struct CommandReadRegistry;

    impl MailConnectorProvider for CommandReadRegistry {
        fn search_mail_page(
            &self,
            _account: &ConnectorAccount,
            _request: &MailSearchRequest,
            _continuation: Option<&ConnectorReadContinuation>,
        ) -> ConnectorProviderResult<ConnectorReadPage<crate::kernel::connectors::domain::MailThread>>
        {
            Err(ConnectorProviderFailure::InvalidResponse)
        }

        fn read_thread(
            &self,
            _account: &ConnectorAccount,
            _request: &MailThreadRequest,
        ) -> ConnectorProviderResult<crate::kernel::connectors::domain::MailThread> {
            Err(ConnectorProviderFailure::InvalidResponse)
        }
    }

    impl CalendarConnectorProvider for CommandReadRegistry {
        fn list_events_page(
            &self,
            _account: &ConnectorAccount,
            _request: &CalendarListRequest,
            _continuation: Option<&ConnectorReadContinuation>,
        ) -> ConnectorProviderResult<ConnectorReadPage<CalendarEvent>> {
            Err(ConnectorProviderFailure::InvalidResponse)
        }
    }

    impl ConnectorReadRegistry for CommandReadRegistry {
        fn mail_provider(&self, provider_key: &str) -> Option<&dyn MailConnectorProvider> {
            (provider_key == "command-read").then_some(self)
        }

        fn calendar_provider(&self, provider_key: &str) -> Option<&dyn CalendarConnectorProvider> {
            (provider_key == "command-read").then_some(self)
        }

        fn execution_enabled(&self) -> bool {
            true
        }
    }

    impl ConnectorReadRegistry for EnabledEmptyReadRegistry {
        fn mail_provider(&self, _provider_key: &str) -> Option<&dyn MailConnectorProvider> {
            None
        }

        fn calendar_provider(&self, _provider_key: &str) -> Option<&dyn CalendarConnectorProvider> {
            None
        }

        fn execution_enabled(&self) -> bool {
            true
        }
    }

    impl crate::kernel::connectors::runtime_registry::ConnectorSyncRegistry
        for EnabledEmptySyncRegistry
    {
        fn mail_provider(
            &self,
            _provider_key: &str,
        ) -> Option<&dyn crate::kernel::connectors::sync::MailSyncProvider> {
            None
        }

        fn calendar_provider(
            &self,
            _provider_key: &str,
        ) -> Option<&dyn crate::kernel::connectors::sync::CalendarSyncProvider> {
            None
        }

        fn execution_enabled(&self) -> bool {
            true
        }
    }

    #[derive(Default)]
    struct CommandMailSyncProvider {
        calls: AtomicUsize,
    }

    impl MailSyncProvider for CommandMailSyncProvider {
        fn sync_mail_page(
            &self,
            _account: &ConnectorAccount,
            _request: &MailSyncRequest,
            _continuation: Option<&ConnectorOpaqueContinuation>,
        ) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ConnectorSyncPage::new(
                Vec::new(),
                ConnectorSyncContinuation::Delta(
                    ConnectorOpaqueContinuation::new("command-mail-delta".to_string())
                        .expect("mail delta builds"),
                ),
            ))
        }
    }

    #[derive(Default)]
    struct CommandCalendarSyncProvider {
        calls: AtomicUsize,
    }

    impl CalendarSyncProvider for CommandCalendarSyncProvider {
        fn sync_calendar_page(
            &self,
            _account: &ConnectorAccount,
            _request: &CalendarSyncRequest,
            _continuation: Option<&ConnectorOpaqueContinuation>,
        ) -> ConnectorProviderResult<ConnectorSyncPage<CalendarEvent>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ConnectorSyncPage::new(
                Vec::new(),
                ConnectorSyncContinuation::Delta(
                    ConnectorOpaqueContinuation::new("command-calendar-delta".to_string())
                        .expect("calendar delta builds"),
                ),
            ))
        }
    }

    #[derive(Default)]
    struct CommandSyncRegistry {
        mail: CommandMailSyncProvider,
        calendar: CommandCalendarSyncProvider,
    }

    impl ConnectorSyncRegistry for CommandSyncRegistry {
        fn mail_provider(&self, provider_key: &str) -> Option<&dyn MailSyncProvider> {
            (provider_key == "fake-command-provider").then_some(&self.mail)
        }

        fn calendar_provider(&self, provider_key: &str) -> Option<&dyn CalendarSyncProvider> {
            (provider_key == "fake-command-provider").then_some(&self.calendar)
        }

        fn execution_enabled(&self) -> bool {
            true
        }
    }

    fn command_sync_account(
        capability: ConnectorCapability,
        now: DateTime<Utc>,
    ) -> ConnectorAccount {
        ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "fake-command-provider".to_string(),
            display_name: "Command recovery account".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![capability],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        }
    }

    fn stopped_command_sync_request(
        store: &EventStore,
        account: &ConnectorAccount,
        capability: ConnectorCapability,
        registry: &dyn ConnectorSyncRegistry,
        now: DateTime<Utc>,
    ) -> (ResumeConnectorReadSyncRequest, String) {
        store
            .upsert_connector_account(account)
            .expect("account persists");
        let generation = store
            .connector_account_sync_generation(account)
            .expect("generation reads");
        let (stream, plan) = match capability {
            ConnectorCapability::MailSyncInbox => {
                let request = MailSyncRequest::inbox(25).expect("mail request builds");
                (
                    request.stream_fingerprint(&account.provider_id),
                    ConnectorSyncPlan::MailInbox {
                        max_changes: request.max_changes(),
                    },
                )
            }
            ConnectorCapability::CalendarSyncEvents => {
                let request =
                    CalendarSyncRequest::new(now - Duration::days(1), now + Duration::days(1), 25)
                        .expect("calendar request builds");
                (
                    request.stream_fingerprint(&account.provider_id),
                    ConnectorSyncPlan::CalendarRange {
                        starts_at: request.starts_at(),
                        ends_at: request.ends_at(),
                        max_changes: request.max_changes(),
                    },
                )
            }
            _ => panic!("fixture accepts only read-sync capabilities"),
        };
        let initial = ConnectorSyncState::initial_with_generation(
            account.id,
            generation,
            capability,
            stream.clone(),
            now,
        )
        .expect("sync state starts");
        store
            .record_connector_sync_plan(
                &initial,
                &plan.persistence_json().expect("plan serializes"),
            )
            .expect("plan persists");
        let (stopped, reason) = match initial
            .recovery(
                ConnectorSyncFailure::InvalidResponse,
                3,
                3,
                now + Duration::seconds(1),
            )
            .expect("stopped recovery builds")
        {
            ConnectorSyncStateRecovery::Persist { next, reason } => (next, reason),
            ConnectorSyncStateRecovery::RepairAccount => panic!("stream should stop"),
        };
        store
            .compare_and_swap_connector_sync_state(&initial, &stopped, reason)
            .expect("stopped state persists");
        let item = store
            .list_connector_recovery_items_with_runtime_registries(
                &EmptyConnectorReconcilerRegistry,
                registry,
            )
            .expect("Recovery items load")
            .into_iter()
            .find(|item| item.sync_capability.is_some())
            .expect("stopped sync projects");
        let action_revision = match item.action {
            Some(crate::kernel::connectors::ConnectorRecoveryAction::ResumeSync {
                action_revision,
            }) => action_revision,
            _ => panic!("exact registry exposes Resume"),
        };
        (
            ResumeConnectorReadSyncRequest {
                item_id: item.id,
                action_revision,
            },
            stream,
        )
    }

    fn command_sync_durable_snapshot(path: &std::path::Path) -> (String, i64, i64, i64, i64) {
        let connection = rusqlite::Connection::open(path).expect("observer opens");
        connection
            .query_row(
                r#"SELECT
                       COALESCE((SELECT group_concat(state_json || ':' || revision || ':' || request_json, '|') FROM connector_sync_streams), ''),
                       (SELECT count(*) FROM connector_sync_recovery_jobs),
                       (SELECT count(*) FROM connector_recovery_action_receipts),
                       (SELECT count(*) FROM kernel_events),
                       (SELECT count(*) FROM connector_sync_projection)"#,
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("durable snapshot reads")
    }

    struct LockCheckingStore {
        event_store: Arc<Mutex<EventStore>>,
        inner: FakeConnectorCredentialStore,
    }

    struct CountingCredentialStore {
        inner: FakeConnectorCredentialStore,
        mutations: Arc<AtomicUsize>,
    }

    impl CountingCredentialStore {
        fn new(mutations: Arc<AtomicUsize>) -> Self {
            Self {
                inner: FakeConnectorCredentialStore::default(),
                mutations,
            }
        }
    }

    impl ConnectorCredentialStore for CountingCredentialStore {
        fn put_new_at(
            &mut self,
            handle: &ConnectorCredentialHandle,
            secret: ConnectorSecret,
        ) -> Result<(), String> {
            self.inner.put_new_at(handle, secret)?;
            self.mutations.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn put_at(
            &mut self,
            handle: &ConnectorCredentialHandle,
            secret: ConnectorSecret,
        ) -> Result<(), String> {
            self.inner.put_at(handle, secret)?;
            self.mutations.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn read(&self, handle: &ConnectorCredentialHandle) -> Result<ConnectorSecret, String> {
            self.inner.read(handle)
        }

        fn replace(
            &mut self,
            handle: &ConnectorCredentialHandle,
            secret: ConnectorSecret,
        ) -> Result<(), String> {
            self.inner.replace(handle, secret)?;
            self.mutations.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn delete(
            &mut self,
            handle: &ConnectorCredentialHandle,
        ) -> Result<ConnectorCredentialDeleteOutcome, String> {
            let outcome = self.inner.delete(handle)?;
            self.mutations.fetch_add(1, Ordering::SeqCst);
            Ok(outcome)
        }

        fn contains(&self, handle: &ConnectorCredentialHandle) -> bool {
            self.inner.contains(handle)
        }
    }

    impl ConnectorCredentialStore for LockCheckingStore {
        fn put_at(
            &mut self,
            handle: &ConnectorCredentialHandle,
            secret: ConnectorSecret,
        ) -> Result<(), String> {
            self.inner.put_at(handle, secret)
        }
        fn read(&self, handle: &ConnectorCredentialHandle) -> Result<ConnectorSecret, String> {
            self.inner.read(handle)
        }
        fn replace(
            &mut self,
            handle: &ConnectorCredentialHandle,
            secret: ConnectorSecret,
        ) -> Result<(), String> {
            self.inner.replace(handle, secret)
        }
        fn delete(
            &mut self,
            handle: &ConnectorCredentialHandle,
        ) -> Result<ConnectorCredentialDeleteOutcome, String> {
            assert!(
                self.event_store.try_lock().is_ok(),
                "vault delete must not hold EventStore lock"
            );
            self.inner.delete(handle)
        }
        fn contains(&self, handle: &ConnectorCredentialHandle) -> bool {
            self.inner.contains(handle)
        }
    }

    struct RecoveryOAuth;

    struct RecoveryOAuthRegistry;

    impl crate::kernel::connectors::runtime_registry::ConnectorOAuthRegistry for RecoveryOAuthRegistry {
        fn provider(
            &self,
            provider_key: &str,
        ) -> Option<&dyn crate::kernel::connectors::oauth::ConnectorOAuthProvider> {
            (provider_key == "local-demo").then_some(&RecoveryOAuth)
        }
    }

    struct BlockingReviewOAuth {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        calls: Arc<AtomicUsize>,
    }

    impl crate::kernel::connectors::oauth::ConnectorOAuthProvider for BlockingReviewOAuth {
        fn provider_id(&self) -> &'static str {
            "local-demo"
        }

        fn scopes_for(&self, _capability: ConnectorCapability) -> Option<&'static [&'static str]> {
            Some(&["demo.read"])
        }

        fn exchange_code(
            &self,
            _code: &str,
            _verifier: &ConnectorSecret,
            _redirect_uri: &str,
            requested_scopes: &[String],
        ) -> Result<crate::kernel::connectors::oauth::ConnectorOAuthExchange, String> {
            crate::kernel::connectors::oauth::ConnectorOAuthExchange::new(
                ConnectorSecret::new("blocking-result-secret".to_string())?,
                requested_scopes.to_vec(),
            )
        }

        fn complete_review(
            &self,
            _verifier: &ConnectorSecret,
            _redirect_uri: &str,
            requested_scopes: &[String],
        ) -> Result<crate::kernel::connectors::oauth::ConnectorOAuthExchange, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered
                .send(())
                .map_err(|_| "provider barrier failed".to_string())?;
            self.release
                .lock()
                .map_err(|_| "provider barrier failed".to_string())?
                .recv()
                .map_err(|_| "provider barrier failed".to_string())?;
            self.exchange_code("unused", _verifier, _redirect_uri, requested_scopes)
        }

        fn account_profile(
            &self,
            _exchange: &crate::kernel::connectors::oauth::ConnectorOAuthExchange,
        ) -> Result<crate::kernel::connectors::oauth::ConnectorAuthorizationAccountProfile, String>
        {
            crate::kernel::connectors::oauth::ConnectorAuthorizationAccountProfile::new(
                "Barrier account".to_string(),
                None,
            )
        }
    }

    struct BlockingReviewOAuthRegistry {
        provider: BlockingReviewOAuth,
    }

    impl crate::kernel::connectors::runtime_registry::ConnectorOAuthRegistry
        for BlockingReviewOAuthRegistry
    {
        fn provider(
            &self,
            provider_key: &str,
        ) -> Option<&dyn crate::kernel::connectors::oauth::ConnectorOAuthProvider> {
            (provider_key == "local-demo").then_some(&self.provider)
        }
    }

    impl crate::kernel::connectors::oauth::ConnectorOAuthProvider for RecoveryOAuth {
        fn provider_id(&self) -> &'static str {
            "local-demo"
        }
        fn scopes_for(&self, _capability: ConnectorCapability) -> Option<&'static [&'static str]> {
            Some(&["demo.read"])
        }
        fn exchange_code(
            &self,
            code: &str,
            _verifier: &ConnectorSecret,
            _redirect_uri: &str,
            requested_scopes: &[String],
        ) -> Result<crate::kernel::connectors::oauth::ConnectorOAuthExchange, String> {
            if code != "local-demo-code" {
                return Err("invalid local demo code".to_string());
            }
            crate::kernel::connectors::oauth::ConnectorOAuthExchange::new(
                ConnectorSecret::new("local-demo-result-secret".to_string())?,
                requested_scopes.to_vec(),
            )
        }

        fn complete_review(
            &self,
            verifier: &ConnectorSecret,
            redirect_uri: &str,
            requested_scopes: &[String],
        ) -> Result<crate::kernel::connectors::oauth::ConnectorOAuthExchange, String> {
            self.exchange_code("local-demo-code", verifier, redirect_uri, requested_scopes)
        }

        fn account_profile(
            &self,
            _exchange: &crate::kernel::connectors::oauth::ConnectorOAuthExchange,
        ) -> Result<crate::kernel::connectors::oauth::ConnectorAuthorizationAccountProfile, String>
        {
            crate::kernel::connectors::oauth::ConnectorAuthorizationAccountProfile::new(
                "Local Demo account".to_string(),
                Some("private-tenant-marker".to_string()),
            )
        }
    }

    fn prepare_local_demo_authorization_command<S: ConnectorCredentialStore + Send>(
        event_store: &Arc<Mutex<EventStore>>,
        runtime: &ConnectorRuntime<S>,
        now: chrono::DateTime<Utc>,
    ) -> Result<ConnectorAuthorizationReviewView, String> {
        let (session, verifier) = crate::kernel::connectors::oauth::prepare_authorization(
            &RecoveryOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43846/callback".to_string(),
            now,
        )?;
        event_store
            .lock()
            .map_err(|_| "local demo authorization is unavailable".to_string())?
            .insert_preparing_connector_authorization(&session, now)
            .map_err(|_| "local demo authorization is unavailable".to_string())?;
        runtime
            .put_authorization_verifier(&session.verifier_handle, verifier)
            .map_err(|_| "local demo authorization is unavailable".to_string())?;
        event_store
            .lock()
            .map_err(|_| "local demo authorization is unavailable".to_string())?
            .activate_preparing_connector_authorization(session.id, now)
            .map_err(|_| "local demo authorization is unavailable".to_string())?;
        let provision = event_store
            .lock()
            .map_err(|_| "local demo authorization is unavailable".to_string())?
            .prepare_connector_authorization_review(session.id, now)
            .map_err(|_| "local demo authorization is unavailable".to_string())?;
        let review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        runtime
            .put_authorization_review_authority(&authority_handle, authority)
            .map_err(|_| "local demo authorization is unavailable".to_string())?;
        event_store
            .lock()
            .map_err(|_| "local demo authorization is unavailable".to_string())?
            .activate_connector_authorization_review(review_id, now)
            .map_err(|_| "local demo authorization is unavailable".to_string())?;
        load_safe_authorization_review(event_store, runtime, review_id, now)
    }

    fn approve_local_demo_authorization_command<S: ConnectorCredentialStore + Send>(
        event_store: &Arc<Mutex<EventStore>>,
        runtime: &ConnectorRuntime<S>,
        review_id: Uuid,
        now: chrono::DateTime<Utc>,
    ) -> Result<ConnectorAuthorizationReviewView, String> {
        resolve_connector_authorization_review_with_registry(
            event_store,
            runtime,
            &RecoveryOAuthRegistry,
            ConnectorAuthorizationResolveRequest {
                review_id,
                intent: ConnectorAuthorizationReviewIntent::Approve,
            },
            now,
        )
    }

    #[test]
    fn authorization_startup_recovery_releases_store_lock_before_vault_delete() {
        let store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        crate::kernel::connectors::oauth::begin_persisted_authorization(
            &store,
            &mut credentials,
            &RecoveryOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43834/callback".to_string(),
            now - Duration::minutes(20),
        )
        .expect("expired authorization persists");
        let event_store = Arc::new(Mutex::new(store));
        let runtime = ConnectorRuntime::new(LockCheckingStore {
            event_store: Arc::clone(&event_store),
            inner: credentials,
        });
        assert_eq!(
            recover_due_connector_authorizations_with_runtime(&event_store, &runtime, now, 64)
                .expect("recovery runs"),
            1
        );
    }

    fn prepare_review_fixture() -> (
        Arc<Mutex<EventStore>>,
        ConnectorRuntime<FakeConnectorCredentialStore>,
        Uuid,
        crate::kernel::connectors::oauth::ConnectorAuthorizationSession,
    ) {
        let store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = crate::kernel::connectors::oauth::begin_persisted_authorization(
            &store,
            &mut credentials,
            &RecoveryOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43845/callback".to_string(),
            now,
        )
        .expect("authorization begins");
        let provision = store
            .prepare_connector_authorization_review(session.id, now)
            .expect("review prepares");
        let review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        credentials
            .put_new_at(&authority_handle, authority)
            .expect("authority persists");
        store
            .activate_connector_authorization_review(review_id, now)
            .expect("review activates");
        (
            Arc::new(Mutex::new(store)),
            ConnectorRuntime::new(credentials),
            review_id,
            session,
        )
    }

    fn review_snapshot(
        event_store: &Arc<Mutex<EventStore>>,
        review_id: Uuid,
        now: chrono::DateTime<Utc>,
    ) -> ConnectorAuthorizationReviewSnapshot {
        event_store
            .lock()
            .expect("store locks")
            .connector_authorization_review_snapshot(review_id, now)
            .expect("snapshot loads")
    }

    #[test]
    fn empty_registry_approve_preserves_active_review_and_vault_authority() {
        let (event_store, runtime, review_id, session) = prepare_review_fixture();
        let now = Utc::now();
        let before = review_snapshot(&event_store, review_id, now);
        let authority_handle = before
            .authority_handle()
            .expect("active review has authority")
            .clone();
        let registries =
            crate::kernel::connectors::runtime_registry::ConnectorRuntimeRegistries::empty();
        let registry = registries.oauth();

        let error = resolve_connector_authorization_review_with_registry(
            &event_store,
            &runtime,
            registry.as_ref(),
            ConnectorAuthorizationResolveRequest {
                review_id,
                intent: ConnectorAuthorizationReviewIntent::Approve,
            },
            now,
        )
        .expect_err("empty production registry fails closed");

        assert_eq!(error, "connector authorization is not available yet");
        let after = review_snapshot(&event_store, review_id, now);
        assert_eq!(
            after.intent_state(),
            ConnectorAuthorizationReviewIntentState::Active
        );
        assert_eq!(
            after.session().status,
            crate::kernel::connectors::oauth::ConnectorAuthorizationStatus::Pending
        );
        assert!(runtime
            .contains_credential(&authority_handle)
            .expect("authority inspects"));
        assert!(runtime
            .contains_credential(&session.verifier_handle)
            .expect("verifier inspects"));
        assert!(!runtime
            .contains_credential(&session.result_credential_handle)
            .expect("result inspects"));
    }

    #[test]
    fn approve_winner_blocks_cancel_loser_without_duplicate_provider_or_vault_mutation() {
        let mutations = Arc::new(AtomicUsize::new(0));
        let runtime = Arc::new(ConnectorRuntime::new(CountingCredentialStore::new(
            Arc::clone(&mutations),
        )));
        let event_store = Arc::new(Mutex::new(EventStore::open_memory().expect("store opens")));
        let now = Utc::now();
        let awaiting =
            prepare_local_demo_authorization_command(&event_store, runtime.as_ref(), now)
                .expect("review prepares");
        mutations.store(0, Ordering::SeqCst);

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let provider_calls = Arc::new(AtomicUsize::new(0));
        let registry = Arc::new(BlockingReviewOAuthRegistry {
            provider: BlockingReviewOAuth {
                entered: entered_tx,
                release: Mutex::new(release_rx),
                calls: Arc::clone(&provider_calls),
            },
        });
        let approve_store = Arc::clone(&event_store);
        let approve_runtime = Arc::clone(&runtime);
        let approve_registry = Arc::clone(&registry);
        let review_id = awaiting.review_id;
        let approve = std::thread::spawn(move || {
            resolve_connector_authorization_review_with_registry(
                &approve_store,
                approve_runtime.as_ref(),
                approve_registry.as_ref(),
                ConnectorAuthorizationResolveRequest {
                    review_id,
                    intent: ConnectorAuthorizationReviewIntent::Approve,
                },
                now,
            )
        });

        entered_rx.recv().expect("approve reaches provider");
        assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
        assert_eq!(mutations.load(Ordering::SeqCst), 0);
        let cancel_error = resolve_connector_authorization_review_with_registry(
            &event_store,
            runtime.as_ref(),
            registry.as_ref(),
            ConnectorAuthorizationResolveRequest {
                review_id,
                intent: ConnectorAuthorizationReviewIntent::Cancel,
            },
            now,
        )
        .expect_err("consumed approve review rejects cancel loser");
        assert_eq!(
            cancel_error,
            "connector authorization review is unavailable"
        );
        assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
        assert_eq!(mutations.load(Ordering::SeqCst), 0);

        release_tx.send(()).expect("provider releases");
        let connected = approve
            .join()
            .expect("approve thread joins")
            .expect("approve winner completes");
        assert_eq!(
            connected.status,
            ConnectorAuthorizationUserStatus::Connected
        );
        assert_eq!(provider_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            mutations.load(Ordering::SeqCst),
            3,
            "winner performs one result write plus verifier and authority deletes"
        );
    }

    #[test]
    fn shared_finalize_rejects_claim_expired_at_provider_return_time_without_vault_write() {
        let (event_store, runtime, review_id, session) = prepare_review_fixture();
        let now = Utc::now();
        let claim = match crate::kernel::connectors::oauth::resolve_persisted_authorization_review(
            &event_store.lock().expect("store locks"),
            &runtime,
            review_id,
            crate::kernel::connectors::oauth::ConnectorAuthorizationIntent::Approve,
            now,
        )
        .expect("review approves")
        {
            crate::kernel::event_store::ConnectorAuthorizationResolution::Approved(claim) => claim,
            crate::kernel::event_store::ConnectorAuthorizationResolution::Cancelled(_) => {
                panic!("approve returns claim")
            }
        };
        let result_handle = claim.result_credential_handle.clone();
        let completion =
            crate::kernel::connectors::oauth::execute_review_authorization_with_runtime(
                &runtime,
                &RecoveryOAuth,
                &claim,
            )
            .expect("provider completes outside fence");

        assert!(
            crate::kernel::connectors::oauth::finalize_claimed_authorization_with_shared_runtime(
                &event_store,
                &runtime,
                claim,
                completion,
                now + Duration::minutes(6),
            )
            .is_err()
        );
        assert!(!runtime
            .contains_credential(&result_handle)
            .expect("result inspects"));
        assert!(runtime
            .contains_credential(&session.verifier_handle)
            .expect("verifier remains for cleanup"));
        assert!(event_store
            .lock()
            .expect("store locks")
            .list_connector_accounts()
            .expect("accounts list")
            .is_empty());
    }

    #[test]
    fn shared_finalize_never_holds_store_while_waiting_for_authorization_fence() {
        let (event_store, runtime, review_id, _) = prepare_review_fixture();
        let runtime = Arc::new(runtime);
        let now = Utc::now();
        let claim = match crate::kernel::connectors::oauth::resolve_persisted_authorization_review(
            &event_store.lock().expect("store locks"),
            runtime.as_ref(),
            review_id,
            crate::kernel::connectors::oauth::ConnectorAuthorizationIntent::Approve,
            now,
        )
        .expect("review approves")
        {
            crate::kernel::event_store::ConnectorAuthorizationResolution::Approved(claim) => claim,
            crate::kernel::event_store::ConnectorAuthorizationResolution::Cancelled(_) => {
                panic!("approve returns claim")
            }
        };
        let authorization_id = claim.session().id;
        let completion =
            crate::kernel::connectors::oauth::execute_review_authorization_with_runtime(
                runtime.as_ref(),
                &RecoveryOAuth,
                &claim,
            )
            .expect("provider completes");
        let (fence_held_tx, fence_held_rx) = mpsc::channel();
        let (try_store_tx, try_store_rx) = mpsc::channel();
        let (store_acquired_tx, store_acquired_rx) = mpsc::channel();
        let (release_fence_tx, release_fence_rx) = mpsc::channel();
        let fence_runtime = Arc::clone(&runtime);
        let fence_store = Arc::clone(&event_store);
        let fence_holder = std::thread::spawn(move || {
            fence_runtime.with_authorization_fence(authorization_id, || {
                fence_held_tx
                    .send(())
                    .map_err(|_| "barrier failed".to_string())?;
                try_store_rx
                    .recv()
                    .map_err(|_| "barrier failed".to_string())?;
                let _store = fence_store
                    .lock()
                    .map_err(|_| "store lock failed".to_string())?;
                store_acquired_tx
                    .send(())
                    .map_err(|_| "barrier failed".to_string())?;
                release_fence_rx
                    .recv()
                    .map_err(|_| "barrier failed".to_string())?;
                Ok(())
            })
        });
        fence_held_rx.recv().expect("fence is held");

        let completion_runtime = Arc::clone(&runtime);
        let completion_store = Arc::clone(&event_store);
        let finalize = std::thread::spawn(move || {
            crate::kernel::connectors::oauth::finalize_claimed_authorization_with_shared_runtime(
                &completion_store,
                completion_runtime.as_ref(),
                claim,
                completion,
                Utc::now(),
            )
        });
        try_store_tx.send(()).expect("holder attempts store");
        store_acquired_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("fence holder acquires Store because finalizer holds no Store guard");
        release_fence_tx.send(()).expect("fence releases");
        fence_holder
            .join()
            .expect("fence holder joins")
            .expect("fence holder succeeds");
        finalize
            .join()
            .expect("finalizer joins")
            .expect("finalizer succeeds");
    }

    #[test]
    fn repeated_authorization_status_reads_do_not_change_sqlite_or_vault() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let database = temp_dir.path().join("authorization-status-read.sqlite3");
        let event_store = Arc::new(Mutex::new(
            EventStore::open(&database).expect("store opens"),
        ));
        let mutations = Arc::new(AtomicUsize::new(0));
        let runtime = ConnectorRuntime::new(CountingCredentialStore::new(Arc::clone(&mutations)));
        let now = Utc::now();
        let awaiting = prepare_local_demo_authorization_command(&event_store, &runtime, now)
            .expect("review prepares");
        let snapshot = review_snapshot(&event_store, awaiting.review_id, now);
        let authority_handle = snapshot
            .authority_handle()
            .expect("active review has authority")
            .clone();
        let verifier_before = runtime
            .read_authorization_verifier(&snapshot.session().verifier_handle)
            .expect("verifier reads")
            .expose()
            .to_string();
        let authority_before = runtime
            .read_authorization_review_authority(&authority_handle)
            .expect("authority reads")
            .expose()
            .to_string();
        let wal = std::path::PathBuf::from(format!("{}-wal", database.display()));
        let sqlite_before = std::fs::read(&database).expect("database bytes read");
        let wal_before = std::fs::read(&wal).unwrap_or_default();
        mutations.store(0, Ordering::SeqCst);

        for _ in 0..100 {
            let view =
                load_safe_authorization_review(&event_store, &runtime, awaiting.review_id, now)
                    .expect("status reads");
            assert_eq!(
                view.status,
                ConnectorAuthorizationUserStatus::AwaitingConfirmation
            );
        }

        assert_eq!(mutations.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::read(&database).expect("database bytes reread"),
            sqlite_before
        );
        assert_eq!(std::fs::read(&wal).unwrap_or_default(), wal_before);
        assert_eq!(
            runtime
                .read_authorization_verifier(&snapshot.session().verifier_handle)
                .expect("verifier rereads")
                .expose(),
            verifier_before
        );
        assert_eq!(
            runtime
                .read_authorization_review_authority(&authority_handle)
                .expect("authority rereads")
                .expose(),
            authority_before
        );
        assert!(!runtime
            .contains_credential(&snapshot.session().result_credential_handle)
            .expect("result inspects"));
    }

    #[test]
    fn safe_authorization_view_projects_awaiting_connecting_and_connected() {
        let (event_store, runtime, review_id, _session) = prepare_review_fixture();
        let now = Utc::now();
        let awaiting = safe_authorization_review_view(
            &event_store,
            &runtime,
            review_snapshot(&event_store, review_id, now),
            now,
        )
        .expect("awaiting view projects");
        assert_eq!(
            awaiting.status,
            ConnectorAuthorizationUserStatus::AwaitingConfirmation
        );
        assert!(awaiting.account.is_none());

        let claim = match crate::kernel::connectors::oauth::resolve_persisted_authorization_review(
            &event_store.lock().expect("store locks"),
            &runtime,
            review_id,
            crate::kernel::connectors::oauth::ConnectorAuthorizationIntent::Approve,
            now,
        )
        .expect("review approves")
        {
            crate::kernel::event_store::ConnectorAuthorizationResolution::Approved(claim) => claim,
            crate::kernel::event_store::ConnectorAuthorizationResolution::Cancelled(_) => {
                panic!("approve returns exchange claim")
            }
        };
        let connecting = safe_authorization_review_view(
            &event_store,
            &runtime,
            review_snapshot(&event_store, review_id, now),
            now,
        )
        .expect("connecting view projects");
        assert_eq!(
            connecting.status,
            ConnectorAuthorizationUserStatus::Connecting
        );
        assert!(connecting.account.is_none());

        crate::kernel::connectors::oauth::complete_claimed_authorization_with_runtime(
            &event_store.lock().expect("store locks"),
            &runtime,
            &RecoveryOAuth,
            claim,
            "local-demo-code",
            now,
        )
        .expect("completion succeeds");
        let connected = safe_authorization_review_view(
            &event_store,
            &runtime,
            review_snapshot(&event_store, review_id, now),
            now,
        )
        .expect("connected view projects");
        assert_eq!(
            connected.status,
            ConnectorAuthorizationUserStatus::Connected
        );
        assert!(connected.account.is_some());
        let json = serde_json::to_string(&connected).expect("safe view serializes");
        for forbidden in [
            "private-tenant-marker",
            "local-demo-result-secret",
            "credential_handle",
            "claim_id",
            "authority_handle",
            "requested_scopes",
            "provider_id",
        ] {
            assert!(!json.contains(forbidden), "safe JSON leaked {forbidden}");
        }
    }

    #[test]
    fn cancel_remains_cancelling_until_fenced_cleanup_finishes() {
        let (event_store, runtime, review_id, _) = prepare_review_fixture();
        let now = Utc::now();
        let claim = match crate::kernel::connectors::oauth::resolve_persisted_authorization_review(
            &event_store.lock().expect("store locks"),
            &runtime,
            review_id,
            crate::kernel::connectors::oauth::ConnectorAuthorizationIntent::Cancel,
            now,
        )
        .expect("review cancels")
        {
            crate::kernel::event_store::ConnectorAuthorizationResolution::Cancelled(claim) => claim,
            crate::kernel::event_store::ConnectorAuthorizationResolution::Approved(_) => {
                panic!("cancel returns cleanup claim")
            }
        };
        let cancelling = safe_authorization_review_view(
            &event_store,
            &runtime,
            review_snapshot(&event_store, review_id, now),
            now,
        )
        .expect("cancelling view projects");
        assert_eq!(
            cancelling.status,
            ConnectorAuthorizationUserStatus::Cancelling
        );
        assert!(cancelling.account.is_none());

        runtime
            .delete_authorization_handles_and_review(
                claim.session(),
                claim.action_authority_handle(),
            )
            .expect("vault cleanup succeeds");
        event_store
            .lock()
            .expect("store locks")
            .finish_connector_authorization_cleanup(&claim, now)
            .expect("cleanup finalizes");
        let cancelled = safe_authorization_review_view(
            &event_store,
            &runtime,
            review_snapshot(&event_store, review_id, now),
            now,
        )
        .expect("cancelled view projects");
        assert_eq!(
            cancelled.status,
            ConnectorAuthorizationUserStatus::Cancelled
        );
        assert!(cancelled.account.is_none());
    }

    #[test]
    fn authorization_resolve_request_rejects_every_internal_authority_field() {
        let review_id = Uuid::new_v4();
        let valid: ConnectorAuthorizationResolveRequest =
            serde_json::from_value(serde_json::json!({"review_id": review_id, "intent": "cancel"}))
                .expect("minimal request parses");
        assert_eq!(valid.review_id, review_id);
        assert_eq!(valid.intent, ConnectorAuthorizationReviewIntent::Cancel);
        for forbidden in [
            "authorization_id",
            "provider_id",
            "state",
            "code",
            "token",
            "secret",
            "claim_id",
            "attempt",
            "credential_handle",
            "now",
        ] {
            let mut value = serde_json::json!({
                "review_id": review_id,
                "intent": "cancel"
            });
            value
                .as_object_mut()
                .expect("request is object")
                .insert(forbidden.to_string(), serde_json::json!("forbidden"));
            assert!(
                serde_json::from_value::<ConnectorAuthorizationResolveRequest>(value).is_err(),
                "request accepted {forbidden}"
            );
        }
    }

    #[test]
    fn recovery_requests_are_strict_public_locator_allowlists() {
        let item_id = Uuid::new_v4();
        let revision = "a".repeat(64);
        assert!(validate_recovery_action_revision(&revision).is_ok());
        for invalid in [
            "",
            "a",
            "A234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "g234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        ] {
            assert!(validate_recovery_action_revision(invalid).is_err());
        }
        assert!(
            serde_json::from_value::<RetryConnectorAttachmentCleanupRequest>(
                serde_json::json!({"item_id": item_id, "action_revision": &revision})
            )
            .is_ok()
        );
        assert!(
            serde_json::from_value::<ResumeConnectorReadSyncRequest>(serde_json::json!({
                "item_id": item_id,
                "action_revision": &revision
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<InspectConnectorExternalResultRequest>(
                serde_json::json!({"item_id": item_id, "action_revision": &revision})
            )
            .is_ok()
        );

        for forbidden in [
            "token",
            "fingerprint",
            "kind",
            "account_id",
            "provider_id",
            "capability",
            "ticket",
            "claim",
            "attempt",
            "credential_handle",
            "authority_handle",
            "secret",
            "now",
        ] {
            let mut value = serde_json::json!({
                "item_id": item_id,
                "action_revision": &revision
            });
            value
                .as_object_mut()
                .expect("request is object")
                .insert(forbidden.to_string(), serde_json::json!("forbidden"));
            assert!(
                serde_json::from_value::<RetryConnectorAttachmentCleanupRequest>(value.clone())
                    .is_err(),
                "attachment request accepted {forbidden}"
            );
            assert!(
                serde_json::from_value::<ResumeConnectorReadSyncRequest>(value.clone()).is_err(),
                "sync request accepted {forbidden}"
            );
            assert!(
                serde_json::from_value::<InspectConnectorExternalResultRequest>(value).is_err(),
                "inspection request accepted {forbidden}"
            );
        }

        let result = serde_json::to_value(ConnectorRecoveryCommandResult {
            acceptance: ConnectorRecoveryAcceptance::AlreadyAccepted,
            items: Vec::new(),
        })
        .expect("command result serializes");
        assert_eq!(
            result,
            serde_json::json!({
                "acceptance": "already_accepted",
                "items": [],
            })
        );
        let serialized = serde_json::to_string(&result).expect("result serializes as text");
        for forbidden in [
            "action_revision",
            "receipt",
            "ticket",
            "claim",
            "attempt",
            "credential_handle",
            "provider_id",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn sync_recovery_source_keeps_registry_and_accepted_only_execution_gates() {
        let source = include_str!("connector_commands.rs");
        let worker_start = source
            .find("fn run_connector_sync_recovery_worker_once")
            .expect("sync worker remains wired");
        let worker_end = source[worker_start..]
            .find("pub fn spawn_connector_sync_recovery_worker")
            .map(|offset| worker_start + offset)
            .expect("sync worker boundary remains stable");
        let worker = &source[worker_start..worker_end];
        let enabled = worker
            .find("if !registry.execution_enabled()")
            .expect("worker checks execution authority");
        let sweep = worker
            .find("run_connector_sync_recovery_with_shared_store")
            .expect("worker invokes the shared sweep");
        assert!(enabled < sweep);

        let resume_start = source
            .find("pub fn resume_connector_read_sync(")
            .expect("Resume command remains wired");
        let resume_end = source[resume_start..]
            .find("fn resume_connector_read_sync_for_state(")
            .map(|offset| resume_start + offset)
            .expect("Resume command boundary remains stable");
        let resume = &source[resume_start..resume_end];
        let helper = resume
            .find("resume_connector_read_sync_for_state")
            .expect("Resume delegates to the state helper");
        let wake = resume
            .find("wake_connector_sync_recovery_worker")
            .expect("Resume wakes the worker");
        assert!(helper < wake);

        let notifier_start = source
            .find("fn notify_connector_sync_recovery_if_accepted")
            .expect("Accepted-only notifier remains wired");
        let notifier_end = source[notifier_start..]
            .find("fn wake_connector_sync_recovery_worker")
            .map(|offset| notifier_start + offset)
            .expect("notifier boundary remains stable");
        assert!(source[notifier_start..notifier_end]
            .contains("acceptance == ConnectorRecoveryAcceptance::Accepted"));

        let helper_start = resume_end;
        let helper_end = source[helper_start..]
            .find("pub fn inspect_connector_external_result(")
            .map(|offset| helper_start + offset)
            .expect("Resume helper boundary remains stable");
        let helper = &source[helper_start..helper_end];
        let registry = helper
            .find("let sync_registry = state.connector_syncs()")
            .expect("Resume obtains the private sync registry");
        let acceptance = helper
            .find("resume_connector_read_sync_from_recovery_with_sync_registry")
            .expect("Resume uses the registry-aware EventStore entrypoint");
        assert!(registry < acceptance);
        assert!(!helper.contains("wake_connector_sync_recovery_worker"));
    }

    #[test]
    fn app_state_sync_registry_injection_is_test_only_and_production_stays_empty() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let state = AppState::new(
            EventStore::open_memory().expect("store opens"),
            temp_dir.path(),
        )
        .expect("production state builds");
        assert!(!state.connector_syncs().execution_enabled());

        let state = state.with_connector_sync_registry_for_test(Arc::new(EnabledEmptySyncRegistry));
        assert!(state.connector_syncs().execution_enabled());

        let commands_source = include_str!("commands.rs");
        let constructor_start = commands_source
            .find("pub fn new(")
            .expect("AppState constructor remains wired");
        let constructor_end = commands_source[constructor_start..]
            .find("#[cfg(test)]")
            .map(|offset| constructor_start + offset)
            .expect("test injection follows production constructor");
        let constructor = &commands_source[constructor_start..constructor_end];
        assert!(constructor.contains("ConnectorRuntimeRegistries::empty()"));
        assert!(!constructor.contains("with_sync_for_test"));
        let injection = &commands_source[constructor_end..];
        assert!(injection.starts_with("#[cfg(test)]"));
        assert!(injection.contains("with_connector_sync_registry_for_test"));

        let read_state = AppState::new(
            EventStore::open_memory().expect("store opens"),
            temp_dir.path(),
        )
        .expect("production state builds")
        .with_connector_read_registry_for_test(Arc::new(EnabledEmptyReadRegistry));
        assert!(read_state.connector_reads().execution_enabled());
        assert!(injection.contains("with_connector_read_registry_for_test"));
    }

    #[test]
    fn explicit_read_commands_gate_registry_before_writes_and_return_safe_views() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "command-read".to_string(),
            display_name: "Command read".to_string(),
            tenant_ref: Some("private-command-tenant".to_string()),
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![
                ConnectorCapability::MailSearch,
                ConnectorCapability::CalendarListEvents,
            ],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        let state = AppState::new(EventStore::open_memory().unwrap(), temp_dir.path()).unwrap();
        state
            .event_store()
            .lock()
            .unwrap()
            .upsert_connector_account(&account)
            .unwrap();
        let before = state
            .event_store()
            .lock()
            .unwrap()
            .connector_read_durable_counts_for_test()
            .unwrap();
        assert_eq!(
            submit_explicit_connector_read_for_state(
                Uuid::new_v4(),
                account.id,
                ConnectorReadPlan::mail_search("private-query".to_string(), 1).unwrap(),
                &state,
                now,
            )
            .unwrap_err(),
            "connector read is unavailable"
        );
        assert_eq!(
            state
                .event_store()
                .lock()
                .unwrap()
                .connector_read_durable_counts_for_test()
                .unwrap(),
            before
        );

        let state = state.with_connector_read_registry_for_test(Arc::new(CommandReadRegistry));
        let source_id = Uuid::new_v4();
        let first = submit_explicit_connector_read_for_state(
            source_id,
            account.id,
            ConnectorReadPlan::mail_search("private-query".to_string(), 1).unwrap(),
            &state,
            now,
        )
        .unwrap();
        let repeat = submit_explicit_connector_read_for_state(
            source_id,
            account.id,
            ConnectorReadPlan::mail_search("private-query".to_string(), 1).unwrap(),
            &state,
            now + Duration::seconds(1),
        )
        .unwrap();
        assert_eq!(first.acceptance, ExplicitConnectorReadAcceptance::Accepted);
        assert_eq!(
            repeat.acceptance,
            ExplicitConnectorReadAcceptance::AlreadyAccepted
        );
        assert_eq!(first.execution.id, repeat.execution.id);
        assert_eq!(
            state
                .event_store()
                .lock()
                .unwrap()
                .connector_read_durable_counts_for_test()
                .unwrap()
                .0,
            before.0 + 1
        );
        let calendar = submit_explicit_connector_read_for_state(
            Uuid::new_v4(),
            account.id,
            ConnectorReadPlan::calendar_list(now, now + Duration::hours(1), 1).unwrap(),
            &state,
            now,
        )
        .unwrap();
        assert_eq!(
            calendar.acceptance,
            ExplicitConnectorReadAcceptance::Accepted
        );
        assert_eq!(
            calendar.execution.kind,
            ConnectorReadExecutionKind::Calendar
        );
        let serialized = serde_json::to_string(&first).unwrap();
        for forbidden in [
            "private-query",
            "command-read",
            "private-command-tenant",
            "account_id",
            "source_invocation",
            "generation",
            "fingerprint",
            "credential",
            "claim",
        ] {
            assert!(
                !serialized.contains(forbidden),
                "command result leaked {forbidden}"
            );
        }
        assert!(
            serde_json::from_value::<ExplicitConnectorMailSearchRequest>(serde_json::json!({
                "source_invocation_id": Uuid::new_v4(),
                "account_id": account.id,
                "query": "safe",
                "max_results": 1,
                "provider_id": "forbidden"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ExplicitConnectorCalendarListRequest>(serde_json::json!({
                "source_invocation_id": Uuid::new_v4(),
                "account_id": account.id,
                "starts_at": now,
                "ends_at": now + Duration::hours(1),
                "max_results": 1,
                "generation": 0
            }))
            .is_err()
        );
        let mut missing_provider = account.clone();
        missing_provider.id = Uuid::new_v4();
        missing_provider.provider_id = "missing-command-provider".to_string();
        state
            .event_store()
            .lock()
            .unwrap()
            .upsert_connector_account(&missing_provider)
            .unwrap();
        let before_missing = state
            .event_store()
            .lock()
            .unwrap()
            .connector_read_durable_counts_for_test()
            .unwrap();
        assert_eq!(
            submit_explicit_connector_read_for_state(
                Uuid::new_v4(),
                missing_provider.id,
                ConnectorReadPlan::mail_search("never-written".to_string(), 1).unwrap(),
                &state,
                now,
            )
            .unwrap_err(),
            "connector read is unavailable"
        );
        assert_eq!(
            state
                .event_store()
                .lock()
                .unwrap()
                .connector_read_durable_counts_for_test()
                .unwrap(),
            before_missing
        );
    }

    #[test]
    fn explicit_read_source_guards_keep_model_actions_out_and_wake_only_new_acceptance() {
        let source = include_str!("connector_commands.rs");
        let helper_start = source
            .find("fn submit_explicit_connector_read_for_state")
            .expect("explicit helper remains wired");
        let helper = &source[helper_start..];
        let registry = helper
            .find("let registry = state.connector_reads()")
            .unwrap();
        let enabled = helper.find("if !registry.execution_enabled()").unwrap();
        let provider = helper.find("let provider_available = match &plan").unwrap();
        let submit = helper
            .find("submit_explicit_connector_read_execution")
            .unwrap();
        assert!(registry < enabled && enabled < provider && provider < submit);

        let wake_start = source.find("fn wake_connector_read_worker").unwrap();
        let wake_end = source[wake_start..]
            .find("#[tauri::command]")
            .map(|offset| wake_start + offset)
            .unwrap();
        let wake = &source[wake_start..wake_end];
        assert!(wake.contains("acceptance == ExplicitConnectorReadAcceptance::Accepted"));
        assert!(!wake.contains("AlreadyAccepted"));

        let commands = include_str!("commands.rs");
        let tool_catalog = include_str!("kernel/tool_runtime.rs");
        for forbidden in [
            "start_explicit_connector_mail_search",
            "start_explicit_connector_calendar_list",
            "submit_automation_connector_read_execution",
            "bind_automation_connector_read_plan",
        ] {
            assert!(!commands.contains(forbidden));
            assert!(!tool_catalog.contains(forbidden));
        }
    }

    #[test]
    fn sync_worker_signal_retains_wakes_that_arrive_during_a_sweep() {
        let signal = ConnectorSyncRecoveryWorkerSignal::new();
        let retained = signal.snapshot();

        signal.notify();
        let (changed, timed_out) = signal.wait_after_sweep(retained, std::time::Duration::ZERO);
        assert_eq!(changed, retained.wrapping_add(1));
        assert!(!timed_out);

        let (unchanged, timed_out) = signal.wait_after_sweep(changed, std::time::Duration::ZERO);
        assert_eq!(unchanged, changed);
        assert!(timed_out);
    }

    #[test]
    fn sync_worker_signal_notifies_only_for_first_accepted_resume() {
        let signal = ConnectorSyncRecoveryWorkerSignal::new();
        let initial = signal.snapshot();
        notify_connector_sync_recovery_if_accepted(&signal, ConnectorRecoveryAcceptance::Accepted);
        assert_eq!(signal.snapshot(), initial.wrapping_add(1));
        notify_connector_sync_recovery_if_accepted(
            &signal,
            ConnectorRecoveryAcceptance::AlreadyAccepted,
        );
        assert_eq!(signal.snapshot(), initial.wrapping_add(1));
    }

    #[test]
    fn command_recovery_resume_is_exact_idempotent_typed_and_restart_durable() {
        for capability in [
            ConnectorCapability::MailSyncInbox,
            ConnectorCapability::CalendarSyncEvents,
        ] {
            let temp_dir = tempfile::tempdir().expect("temp dir");
            let database_path = temp_dir.path().join(match capability {
                ConnectorCapability::MailSyncInbox => "mail.sqlite3",
                ConnectorCapability::CalendarSyncEvents => "calendar.sqlite3",
                _ => unreachable!(),
            });
            let vault_path = temp_dir.path().join("vault-a");
            let now = Utc::now() - Duration::minutes(1);
            let account = command_sync_account(capability, now);
            let registry = Arc::new(CommandSyncRegistry::default());
            let store = EventStore::open(&database_path).expect("file store opens");
            let (request, stream) =
                stopped_command_sync_request(&store, &account, capability, registry.as_ref(), now);
            let state = AppState::new(store, &vault_path)
                .expect("state builds")
                .with_connector_sync_registry_for_test(registry.clone());

            let first = resume_connector_read_sync_for_state(
                ResumeConnectorReadSyncRequest {
                    item_id: request.item_id,
                    action_revision: request.action_revision.clone(),
                },
                &state,
                now + Duration::seconds(2),
            )
            .expect("exact Resume accepts");
            assert_eq!(first.acceptance, ConnectorRecoveryAcceptance::Accepted);
            let second = resume_connector_read_sync_for_state(
                ResumeConnectorReadSyncRequest {
                    item_id: request.item_id,
                    action_revision: request.action_revision.clone(),
                },
                &state,
                now + Duration::seconds(3),
            )
            .expect("repeat Resume is idempotent");
            assert_eq!(
                second.acceptance,
                ConnectorRecoveryAcceptance::AlreadyAccepted
            );
            assert_eq!(registry.mail.calls.load(Ordering::SeqCst), 0);
            assert_eq!(registry.calendar.calls.load(Ordering::SeqCst), 0);

            run_connector_sync_recovery_worker_once(&state);
            match capability {
                ConnectorCapability::MailSyncInbox => {
                    assert_eq!(registry.mail.calls.load(Ordering::SeqCst), 1);
                    assert_eq!(registry.calendar.calls.load(Ordering::SeqCst), 0);
                }
                ConnectorCapability::CalendarSyncEvents => {
                    assert_eq!(registry.mail.calls.load(Ordering::SeqCst), 0);
                    assert_eq!(registry.calendar.calls.load(Ordering::SeqCst), 1);
                }
                _ => unreachable!(),
            }
            let store = state.event_store();
            let store = store.lock().expect("store locks");
            let synced = store
                .connector_sync_state(account.id, capability, &stream)
                .expect("sync state reads")
                .expect("sync state remains");
            assert!(!synced.stopped());
            assert!(synced.has_committed_delta());
            drop(store);
            drop(state);

            let restarted_registry = Arc::new(CommandSyncRegistry::default());
            let restarted = AppState::new(
                EventStore::open(&database_path).expect("file store reopens"),
                temp_dir.path().join("vault-b"),
            )
            .expect("restarted state builds")
            .with_connector_sync_registry_for_test(restarted_registry.clone());
            let repeated_after_restart = resume_connector_read_sync_for_state(
                ResumeConnectorReadSyncRequest {
                    item_id: request.item_id,
                    action_revision: request.action_revision,
                },
                &restarted,
                now + Duration::seconds(4),
            )
            .expect("durable receipt survives restart");
            assert_eq!(
                repeated_after_restart.acceptance,
                ConnectorRecoveryAcceptance::AlreadyAccepted
            );
            run_connector_sync_recovery_worker_once(&restarted);
            assert_eq!(restarted_registry.mail.calls.load(Ordering::SeqCst), 0);
            assert_eq!(restarted_registry.calendar.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn unavailable_command_recovery_is_actionless_and_zero_write() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let database_path = temp_dir.path().join("unavailable.sqlite3");
        let now = Utc::now() - Duration::minutes(1);
        let account = command_sync_account(ConnectorCapability::MailSyncInbox, now);
        let exact_registry = CommandSyncRegistry::default();
        let store = EventStore::open(&database_path).expect("file store opens");
        let (request, _) = stopped_command_sync_request(
            &store,
            &account,
            ConnectorCapability::MailSyncInbox,
            &exact_registry,
            now,
        );

        let production_state =
            AppState::new(store, temp_dir.path().join("vault-production")).expect("state builds");
        let items = list_connector_recovery_items_for_state(&production_state)
            .expect("actionless items load");
        let item = items
            .iter()
            .find(|item| item.id == request.item_id)
            .expect("stopped item remains visible");
        assert!(item.action.is_none());
        let serialized = serde_json::to_string(item).expect("item serializes");
        for forbidden in [
            "fake-command-provider",
            "credential",
            "stream_fingerprint",
            "continuation",
            "claim",
            "attempt",
            "ticket",
            "secret",
        ] {
            assert!(!serialized.contains(forbidden), "item leaked {forbidden}");
        }
        let before = command_sync_durable_snapshot(&database_path);
        let error = resume_connector_read_sync_for_state(
            ResumeConnectorReadSyncRequest {
                item_id: request.item_id,
                action_revision: request.action_revision.clone(),
            },
            &production_state,
            now + Duration::seconds(2),
        )
        .expect_err("disabled registry rejects Resume");
        assert_eq!(error, "sync recovery could not be scheduled safely");
        assert_eq!(command_sync_durable_snapshot(&database_path), before);
        drop(production_state);

        let wrong_registry = Arc::new(EnabledEmptySyncRegistry);
        let wrong_state = AppState::new(
            EventStore::open(&database_path).expect("store reopens"),
            temp_dir.path().join("vault-wrong"),
        )
        .expect("state builds")
        .with_connector_sync_registry_for_test(wrong_registry);
        let error = resume_connector_read_sync_for_state(
            ResumeConnectorReadSyncRequest {
                item_id: request.item_id,
                action_revision: request.action_revision,
            },
            &wrong_state,
            now + Duration::seconds(3),
        )
        .expect_err("missing exact provider rejects Resume");
        assert_eq!(error, "sync recovery could not be scheduled safely");
        assert_eq!(command_sync_durable_snapshot(&database_path), before);
    }

    #[test]
    fn tampered_review_authority_projects_repair_without_leaking_vault_error() {
        let (event_store, runtime, review_id, _) = prepare_review_fixture();
        let now = Utc::now();
        let snapshot = review_snapshot(&event_store, review_id, now);
        let authority_handle = snapshot
            .authority_handle()
            .expect("active review has authority")
            .clone();
        runtime
            .replace_credential_for_test(
                &authority_handle,
                ConnectorSecret::new("tampered-authority-marker".to_string())
                    .expect("tampered secret builds"),
            )
            .expect("authority tampers");

        let view = safe_authorization_review_view(
            &event_store,
            &runtime,
            review_snapshot(&event_store, review_id, now),
            now,
        )
        .expect("tamper maps safely");
        assert_eq!(
            view.status,
            ConnectorAuthorizationUserStatus::RepairRequired
        );
        assert!(view.account.is_none());
        let json = serde_json::to_string(&view).expect("view serializes");
        assert!(!json.contains("tampered-authority-marker"));
        assert!(!json.contains("credential"));
    }

    #[test]
    fn local_demo_command_flow_survives_store_restart_and_returns_only_safe_views() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("local-demo-authorization.sqlite3");
        let event_store = Arc::new(Mutex::new(EventStore::open(&path).expect("store opens")));
        let runtime = ConnectorRuntime::new(FakeConnectorCredentialStore::default());
        let now = Utc::now();

        let awaiting = prepare_local_demo_authorization_command(&event_store, &runtime, now)
            .expect("local demo review prepares");
        assert_eq!(
            awaiting.status,
            ConnectorAuthorizationUserStatus::AwaitingConfirmation
        );
        let review_id = awaiting.review_id;
        drop(event_store);

        let restarted = Arc::new(Mutex::new(EventStore::open(&path).expect("store restarts")));
        let restored = load_safe_authorization_review(&restarted, &runtime, review_id, now)
            .expect("same review restores");
        assert_eq!(restored.review_id, review_id);
        assert_eq!(
            restored.status,
            ConnectorAuthorizationUserStatus::AwaitingConfirmation
        );

        let connected =
            approve_local_demo_authorization_command(&restarted, &runtime, review_id, now)
                .expect("local demo approves");
        assert_eq!(
            connected.status,
            ConnectorAuthorizationUserStatus::Connected
        );
        assert!(connected.account.is_some());
        let json = serde_json::to_string(&connected).expect("view serializes");
        for forbidden in [
            "local-demo-result-secret",
            "\"state\":",
            "requested_scopes",
            "redirect",
            "verifier",
            "credential",
            "handle",
            "claim",
            "lease",
            "token",
            "tenant",
            "revision",
        ] {
            assert!(!json.contains(forbidden), "safe JSON leaked {forbidden}");
        }
    }

    #[test]
    fn connected_review_fails_closed_for_every_account_binding_tamper() {
        for tamper in ["provider", "capability", "credential", "generation"] {
            let temp_dir = tempfile::tempdir().expect("temp dir");
            let path = temp_dir.path().join(format!("binding-{tamper}.sqlite3"));
            let event_store = Arc::new(Mutex::new(EventStore::open(&path).expect("store opens")));
            let runtime = ConnectorRuntime::new(FakeConnectorCredentialStore::default());
            let now = Utc::now();
            let awaiting = prepare_local_demo_authorization_command(&event_store, &runtime, now)
                .expect("review prepares");
            let connected = approve_local_demo_authorization_command(
                &event_store,
                &runtime,
                awaiting.review_id,
                now,
            )
            .expect("review connects");
            assert_eq!(
                connected.status,
                ConnectorAuthorizationUserStatus::Connected
            );
            let mut account = event_store
                .lock()
                .expect("store locks")
                .list_connector_accounts()
                .expect("accounts load")
                .into_iter()
                .next()
                .expect("account exists");
            let connection = rusqlite::Connection::open(&path).expect("inspection opens");
            match tamper {
                "provider" => account.provider_id = "tampered-provider".to_string(),
                "capability" => {
                    account.granted_capabilities = vec![ConnectorCapability::CalendarListEvents]
                }
                "credential" => account.credential_handle = ConnectorCredentialHandle::new(),
                "generation" => {
                    connection
                        .execute(
                            "UPDATE connector_account_generations SET generation = 1 WHERE account_id = ?1",
                            rusqlite::params![account.id.to_string()],
                        )
                        .expect("generation tampers");
                }
                _ => unreachable!(),
            }
            if tamper != "generation" {
                connection
                    .execute(
                        r#"UPDATE connector_accounts
                           SET provider_id = ?2, account_json = ?3, health = ?4
                           WHERE id = ?1"#,
                        rusqlite::params![
                            account.id.to_string(),
                            account.provider_id,
                            serde_json::to_string(&account).expect("account serializes"),
                            serde_json::to_string(&account.health).expect("health serializes"),
                        ],
                    )
                    .expect("account binding tampers");
            }
            drop(connection);

            let view =
                load_safe_authorization_review(&event_store, &runtime, awaiting.review_id, now)
                    .expect("tampered review maps safely");
            assert_eq!(
                view.status,
                ConnectorAuthorizationUserStatus::RepairRequired,
                "{tamper} tamper must fail closed"
            );
            assert!(view.account.is_none());
        }
    }

    fn connected_account(
        credentials: &mut FakeConnectorCredentialStore,
        marker: &str,
    ) -> ConnectorAccount {
        let now = Utc::now();
        ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            display_name: "Disconnect recovery".to_string(),
            tenant_ref: None,
            credential_handle: credentials
                .put(ConnectorSecret::new(marker.to_string()).expect("secret builds"))
                .expect("credential persists"),
            granted_capabilities: vec![ConnectorCapability::MailSearch],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        }
    }

    struct FailingDeleteStore {
        inner: FakeConnectorCredentialStore,
    }

    impl ConnectorCredentialStore for FailingDeleteStore {
        fn put_at(
            &mut self,
            handle: &ConnectorCredentialHandle,
            secret: ConnectorSecret,
        ) -> Result<(), String> {
            self.inner.put_at(handle, secret)
        }

        fn read(&self, handle: &ConnectorCredentialHandle) -> Result<ConnectorSecret, String> {
            self.inner.read(handle)
        }

        fn replace(
            &mut self,
            handle: &ConnectorCredentialHandle,
            secret: ConnectorSecret,
        ) -> Result<(), String> {
            self.inner.replace(handle, secret)
        }

        fn delete(
            &mut self,
            _handle: &ConnectorCredentialHandle,
        ) -> Result<ConnectorCredentialDeleteOutcome, String> {
            Err("normalized local credential delete failure".to_string())
        }

        fn contains(&self, handle: &ConnectorCredentialHandle) -> bool {
            self.inner.contains(handle)
        }
    }

    #[test]
    fn startup_reconciles_disconnect_before_and_after_vault_delete_crashes() {
        for delete_before_restart in [false, true] {
            let temp_dir = tempfile::tempdir().expect("temp dir");
            let path = temp_dir.path().join("disconnect-recovery.sqlite3");
            let marker = format!("disconnect-marker-{delete_before_restart}");
            let mut credentials = FakeConnectorCredentialStore::default();
            let account = connected_account(&mut credentials, &marker);
            let runtime = ConnectorRuntime::new(credentials);
            {
                let store = EventStore::open(&path).expect("store opens");
                store
                    .upsert_connector_account(&account)
                    .expect("account persists");
                let ticket = store
                    .begin_connector_disconnect(account.id, Utc::now())
                    .expect("disconnect begins");
                let duplicate = store
                    .begin_connector_disconnect(account.id, Utc::now())
                    .expect("duplicate begin resumes same intent");
                assert_eq!(ticket.generation(), duplicate.generation());
                if delete_before_restart {
                    assert_eq!(
                        runtime
                            .delete_account_credential(ticket.account())
                            .expect("credential deletes"),
                        ConnectorCredentialDeleteOutcome::Deleted
                    );
                }
            }

            let event_store = std::sync::Arc::new(std::sync::Mutex::new(
                EventStore::open(&path).expect("store reopens"),
            ));
            assert_eq!(
                reconcile_pending_connector_disconnects_with(&event_store, &runtime)
                    .expect("startup recovery runs"),
                1
            );
            let store = event_store.lock().expect("store locks");
            let recovered = store
                .list_connector_accounts()
                .expect("accounts read")
                .into_iter()
                .find(|item| item.id == account.id)
                .expect("account remains");
            assert_eq!(recovered.health, ConnectorHealth::Disconnected);
            assert_eq!(
                runtime
                    .delete_account_credential(&recovered)
                    .expect("repeat delete is idempotent"),
                ConnectorCredentialDeleteOutcome::AlreadyAbsent
            );
            let events = serde_json::to_string(&store.list_recent(20).expect("events read"))
                .expect("events serialize");
            assert!(events.contains("connector.disconnect.started"));
            assert!(events.contains("connector.disconnect.completed"));
            assert!(!events.contains(&marker));
            assert!(!events.contains("connector-credential:"));
        }
    }

    #[test]
    fn pending_disconnect_rejects_generic_account_overwrite_and_records_failure() {
        let store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let account = connected_account(&mut credentials, "pending-marker");
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let ticket = store
            .begin_connector_disconnect(account.id, Utc::now())
            .expect("disconnect begins");
        assert!(store.upsert_connector_account(&account).is_err());
        store
            .record_connector_disconnect_failure(
                &ticket,
                ConnectorDisconnectSource::Startup,
                Utc::now(),
            )
            .expect("failure receipt persists");
        assert_eq!(
            store
                .list_pending_connector_disconnects(32)
                .expect("pending work reads")
                .len(),
            1
        );
        let events = serde_json::to_string(&store.list_recent(20).expect("events read"))
            .expect("events serialize");
        assert!(events.contains("connector.disconnect.retry_required"));
        assert!(!events.contains("pending-marker"));

        store
            .complete_connector_disconnect(
                &ticket,
                ConnectorDisconnectSource::User,
                ConnectorCredentialDeleteOutcome::Deleted,
                Utc::now(),
            )
            .expect("disconnect completes");
        let mut reconnected = account.clone();
        reconnected.credential_handle = credentials
            .put(ConnectorSecret::new("new-generation-marker".to_string()).expect("secret builds"))
            .expect("new credential persists");
        reconnected.health = ConnectorHealth::Connected;
        reconnected.updated_at = Utc::now();
        store
            .upsert_connector_account(&reconnected)
            .expect("new account generation persists");
        assert!(store
            .complete_connector_disconnect(
                &ticket,
                ConnectorDisconnectSource::Startup,
                ConnectorCredentialDeleteOutcome::AlreadyAbsent,
                Utc::now(),
            )
            .is_err());
    }

    #[test]
    fn startup_keeps_failed_vault_delete_pending_and_fail_closed() {
        let mut credentials = FakeConnectorCredentialStore::default();
        let account = connected_account(&mut credentials, "failed-delete-marker");
        let store = EventStore::open_memory().expect("store opens");
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        store
            .begin_connector_disconnect(account.id, Utc::now())
            .expect("disconnect begins");
        let event_store = std::sync::Arc::new(std::sync::Mutex::new(store));
        let runtime = ConnectorRuntime::new(FailingDeleteStore { inner: credentials });
        assert_eq!(
            reconcile_pending_connector_disconnects_with(&event_store, &runtime)
                .expect("startup recovery runs"),
            0
        );
        let store = event_store.lock().expect("store locks");
        let pending = store
            .list_connector_accounts()
            .expect("accounts read")
            .into_iter()
            .find(|item| item.id == account.id)
            .expect("account remains");
        assert_eq!(pending.health, ConnectorHealth::DisconnectPending);
        let events = serde_json::to_string(&store.list_recent(20).expect("events read"))
            .expect("events serialize");
        assert!(events.contains("connector.disconnect.retry_required"));
        assert!(!events.contains("failed-delete-marker"));
    }
}
