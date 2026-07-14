use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::domain::{CalendarEvent, MailThread};
use super::provider::{collect_calendar_events, collect_mail_search};
use super::provider::{CalendarListRequest, MailSearchRequest};
use super::runtime_registry::ConnectorReadRegistry;
use super::ConnectorCapability;
use crate::kernel::event_store::{EventStore, EventStoreError, EventStoreResult};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ConnectorReadPlan {
    MailSearch {
        query: String,
        max_results: u16,
    },
    CalendarList {
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        max_results: u16,
    },
}

impl ConnectorReadPlan {
    pub(crate) fn from_persistence_json(value: &str) -> Result<Self, String> {
        let decoded: Self = serde_json::from_str(value)
            .map_err(|_| "connector read plan is invalid".to_string())?;
        match decoded {
            Self::MailSearch { query, max_results } => Self::mail_search(query, max_results),
            Self::CalendarList {
                starts_at,
                ends_at,
                max_results,
            } => Self::calendar_list(starts_at, ends_at, max_results),
        }
    }

    pub(crate) fn mail_search(query: String, max_results: u16) -> Result<Self, String> {
        let request = MailSearchRequest::new(query, max_results)?;
        Ok(Self::MailSearch {
            query: request.query().to_string(),
            max_results: request.max_results(),
        })
    }

    pub(crate) fn calendar_list(
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        max_results: u16,
    ) -> Result<Self, String> {
        let request = CalendarListRequest::new(starts_at, ends_at, max_results)?;
        Ok(Self::CalendarList {
            starts_at: request.starts_at(),
            ends_at: request.ends_at(),
            max_results: request.max_results(),
        })
    }

    pub(crate) fn capability(&self) -> ConnectorCapability {
        match self {
            Self::MailSearch { .. } => ConnectorCapability::MailSearch,
            Self::CalendarList { .. } => ConnectorCapability::CalendarListEvents,
        }
    }

    pub(crate) fn canonical_json(&self) -> Result<String, String> {
        serde_json::to_string(self)
            .map_err(|_| "connector read plan could not be encoded".to_string())
    }

    pub(crate) fn fingerprint(&self) -> Result<String, String> {
        Ok(format!(
            "sha256:{:x}",
            Sha256::digest(self.canonical_json()?.as_bytes())
        ))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectorReadSourceKind {
    Explicit,
    Automation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectorReadExecutionPhase {
    Pending,
    Claimed,
    RemoteCallStarted,
    ResultPersisted,
    Applied,
    AuthorityLost,
    ReconciliationRequired,
    Cancelled,
    RepairRequired,
}

impl ConnectorReadExecutionPhase {
    pub(crate) fn can_claim(self) -> bool {
        self == Self::Pending
    }

    pub(crate) fn restart_phase(self) -> Self {
        match self {
            Self::Claimed => Self::Pending,
            Self::RemoteCallStarted => Self::ReconciliationRequired,
            phase => phase,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectorReadSubmission {
    Accepted,
    AlreadyAccepted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConnectorReadExecution {
    pub id: Uuid,
    pub source_kind: ConnectorReadSourceKind,
    pub source_invocation_id: String,
    pub account_id: Uuid,
    pub account_generation: u64,
    pub capability: ConnectorCapability,
    pub plan: ConnectorReadPlan,
    pub plan_fingerprint: String,
    pub authority_fingerprint: String,
    pub phase: ConnectorReadExecutionPhase,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConnectorReadExecution {
    pub(crate) fn public_view(&self) -> ConnectorReadExecutionView {
        ConnectorReadExecutionView {
            id: self.id,
            kind: match self.capability {
                ConnectorCapability::MailSearch => ConnectorReadExecutionKind::Mail,
                ConnectorCapability::CalendarListEvents => ConnectorReadExecutionKind::Calendar,
                _ => unreachable!(),
            },
            phase: match self.phase {
                ConnectorReadExecutionPhase::Pending => ConnectorReadExecutionPublicPhase::Queued,
                ConnectorReadExecutionPhase::Claimed
                | ConnectorReadExecutionPhase::RemoteCallStarted
                | ConnectorReadExecutionPhase::ResultPersisted => {
                    ConnectorReadExecutionPublicPhase::Running
                }
                ConnectorReadExecutionPhase::Applied => {
                    ConnectorReadExecutionPublicPhase::Completed
                }
                ConnectorReadExecutionPhase::AuthorityLost
                | ConnectorReadExecutionPhase::ReconciliationRequired
                | ConnectorReadExecutionPhase::RepairRequired => {
                    ConnectorReadExecutionPublicPhase::NeedsAttention
                }
                ConnectorReadExecutionPhase::Cancelled => {
                    ConnectorReadExecutionPublicPhase::Cancelled
                }
            },
            item_count: None,
            evidence_ref: None,
            error_code: None,
            updated_at: self.updated_at,
        }
    }
}

pub(crate) struct ConnectorReadExecutionClaim {
    pub(crate) execution_id: Uuid,
    pub(crate) claim_id: Uuid,
    pub(crate) claim_expires_at: DateTime<Utc>,
    pub(crate) source_kind: ConnectorReadSourceKind,
    pub(crate) source_invocation_id: String,
    pub(crate) request_fingerprint: String,
    pub(crate) source_authority_fingerprint: String,
    pub(crate) source_revision: u64,
    pub(crate) account: super::ConnectorAccount,
    pub(crate) account_generation: u64,
    pub(crate) capability: ConnectorCapability,
    pub(crate) plan: ConnectorReadPlan,
    pub(crate) plan_fingerprint: String,
    pub(crate) authority_fingerprint: String,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", content = "items", rename_all = "snake_case")]
pub(crate) enum ConnectorReadResult {
    Mail(Vec<MailThread>),
    Calendar(Vec<CalendarEvent>),
}

impl ConnectorReadResult {
    pub(crate) fn from_persistence_json(value: &str) -> Result<Self, String> {
        match serde_json::from_str(value)
            .map_err(|_| "connector read result is invalid".to_string())?
        {
            Self::Mail(items) => Self::mail(items),
            Self::Calendar(items) => Self::calendar(items),
        }
    }

    pub(crate) fn mail(items: Vec<MailThread>) -> Result<Self, String> {
        for item in &items {
            item.validate()?;
        }
        Ok(Self::Mail(items))
    }

    pub(crate) fn calendar(items: Vec<CalendarEvent>) -> Result<Self, String> {
        for item in &items {
            item.validate()?;
        }
        Ok(Self::Calendar(items))
    }

    pub(crate) fn capability(&self) -> ConnectorCapability {
        match self {
            Self::Mail(_) => ConnectorCapability::MailSearch,
            Self::Calendar(_) => ConnectorCapability::CalendarListEvents,
        }
    }

    pub(crate) fn item_count(&self) -> usize {
        match self {
            Self::Mail(items) => items.len(),
            Self::Calendar(items) => items.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ConnectorReadExecutionSweep {
    pub(crate) claimed: usize,
    pub(crate) completed: usize,
    pub(crate) recovered_applies: usize,
    pub(crate) unavailable: usize,
    pub(crate) failed: usize,
    pub(crate) lost_claim: usize,
}

pub(crate) fn run_connector_read_executions_with_shared_store(
    event_store: &Arc<Mutex<EventStore>>,
    registry: &dyn ConnectorReadRegistry,
    limit: usize,
) -> EventStoreResult<ConnectorReadExecutionSweep> {
    if !registry.execution_enabled() || limit == 0 {
        return Ok(ConnectorReadExecutionSweep::default());
    }
    let recovered_applies = event_store
        .lock()
        .map_err(|_| {
            EventStoreError::InvalidState("connector read store is unavailable".to_string())
        })?
        .apply_due_connector_read_results(Utc::now(), limit)?;
    let claims = event_store
        .lock()
        .map_err(|_| {
            EventStoreError::InvalidState("connector read store is unavailable".to_string())
        })?
        .claim_due_connector_read_executions(Utc::now(), limit)?;
    let mut sweep = ConnectorReadExecutionSweep {
        claimed: claims.len(),
        recovered_applies,
        ..ConnectorReadExecutionSweep::default()
    };
    for claim in claims {
        let provider_key = claim.account.provider_id.as_str();
        let result = match &claim.plan {
            ConnectorReadPlan::MailSearch { query, max_results } => {
                let Some(provider) = registry.mail_provider(provider_key) else {
                    event_store
                        .lock()
                        .map_err(|_| {
                            EventStoreError::InvalidState(
                                "connector read store is unavailable".to_string(),
                            )
                        })?
                        .stop_unavailable_connector_read_claim(&claim, Utc::now())?;
                    sweep.unavailable += 1;
                    continue;
                };
                if event_store
                    .lock()
                    .map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector read store is unavailable".to_string(),
                        )
                    })?
                    .mark_connector_read_remote_call_started(&claim, Utc::now())
                    .is_err()
                {
                    sweep.lost_claim += 1;
                    continue;
                }
                MailSearchRequest::new(query.clone(), *max_results)
                    .map_err(|_| super::provider::ConnectorProviderFailure::InvalidResponse)
                    .and_then(|request| collect_mail_search(provider, &claim.account, &request))
                    .and_then(|items| {
                        ConnectorReadResult::mail(items)
                            .map_err(|_| super::provider::ConnectorProviderFailure::InvalidResponse)
                    })
            }
            ConnectorReadPlan::CalendarList {
                starts_at,
                ends_at,
                max_results,
            } => {
                let Some(provider) = registry.calendar_provider(provider_key) else {
                    event_store
                        .lock()
                        .map_err(|_| {
                            EventStoreError::InvalidState(
                                "connector read store is unavailable".to_string(),
                            )
                        })?
                        .stop_unavailable_connector_read_claim(&claim, Utc::now())?;
                    sweep.unavailable += 1;
                    continue;
                };
                if event_store
                    .lock()
                    .map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector read store is unavailable".to_string(),
                        )
                    })?
                    .mark_connector_read_remote_call_started(&claim, Utc::now())
                    .is_err()
                {
                    sweep.lost_claim += 1;
                    continue;
                }
                CalendarListRequest::new(*starts_at, *ends_at, *max_results)
                    .map_err(|_| super::provider::ConnectorProviderFailure::InvalidResponse)
                    .and_then(|request| collect_calendar_events(provider, &claim.account, &request))
                    .and_then(|items| {
                        ConnectorReadResult::calendar(items)
                            .map_err(|_| super::provider::ConnectorProviderFailure::InvalidResponse)
                    })
            }
        };
        let Ok(result) = result else {
            let finalized = event_store
                .lock()
                .map_err(|_| {
                    EventStoreError::InvalidState("connector read store is unavailable".to_string())
                })?
                .fail_connector_read_after_remote_call(&claim, Utc::now());
            if finalized.is_ok() {
                sweep.failed += 1;
            } else {
                sweep.lost_claim += 1;
            }
            continue;
        };
        let store = event_store.lock().map_err(|_| {
            EventStoreError::InvalidState("connector read store is unavailable".to_string())
        })?;
        match store.persist_connector_read_result(&claim, &result, Utc::now()) {
            Ok(_) => {
                if store.apply_connector_read_result(claim.execution_id, Utc::now())? {
                    sweep.completed += 1;
                }
            }
            Err(_) => sweep.lost_claim += 1,
        }
    }
    Ok(sweep)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConnectorReadExecutionView {
    pub id: Uuid,
    pub kind: ConnectorReadExecutionKind,
    pub phase: ConnectorReadExecutionPublicPhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<ConnectorReadEvidenceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ConnectorReadExecutionErrorCode>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ConnectorReadEvidenceRef(String);

impl ConnectorReadEvidenceRef {
    pub(crate) fn new(id: Uuid) -> Self {
        Self(format!("read-evidence:{id}"))
    }

    pub(crate) fn from_persistence(value: &str) -> Option<Self> {
        let id = value.strip_prefix("read-evidence:")?;
        Uuid::parse_str(id).ok()?;
        Some(Self(value.to_string()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorReadExecutionErrorCode {
    ConnectionNeedsAttention,
    ProviderTemporarilyUnavailable,
    ExternalResultUncertain,
    EvidenceUnavailable,
    ExecutionRecordUnavailable,
    ReadCouldNotComplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorReadExecutionKind {
    Mail,
    Calendar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorReadExecutionPublicPhase {
    Queued,
    Running,
    Completed,
    NeedsAttention,
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::super::domain::{CalendarAttendee, MailAddress, MailMessage};
    use super::super::provider::{
        CalendarConnectorProvider, ConnectorProviderFailure, ConnectorProviderResult,
        ConnectorReadContinuation, ConnectorReadPage, MailConnectorProvider, MailThreadRequest,
    };
    use super::*;
    use crate::kernel::automation::{AutomationDefinition, AutomationRunStatus};
    use crate::kernel::connectors::{ConnectorCredentialHandle, ConnectorHealth};
    use chrono::Duration;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;

    struct FakeReadRegistry {
        mail_calls: AtomicUsize,
        calendar_calls: AtomicUsize,
    }

    struct BlockingReadRegistry {
        calls: AtomicUsize,
        entered: Barrier,
        release: Barrier,
    }

    impl BlockingReadRegistry {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                entered: Barrier::new(2),
                release: Barrier::new(2),
            }
        }
    }

    impl MailConnectorProvider for BlockingReadRegistry {
        fn search_mail_page(
            &self,
            _account: &super::super::ConnectorAccount,
            _request: &MailSearchRequest,
            _continuation: Option<&ConnectorReadContinuation>,
        ) -> ConnectorProviderResult<ConnectorReadPage<MailThread>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.entered.wait();
            self.release.wait();
            Ok(ConnectorReadPage::new(Vec::new(), None))
        }

        fn read_thread(
            &self,
            _account: &super::super::ConnectorAccount,
            _request: &MailThreadRequest,
        ) -> ConnectorProviderResult<MailThread> {
            Err(ConnectorProviderFailure::InvalidResponse)
        }
    }

    impl ConnectorReadRegistry for BlockingReadRegistry {
        fn mail_provider(&self, provider_key: &str) -> Option<&dyn MailConnectorProvider> {
            (provider_key == "fake-read-provider").then_some(self)
        }

        fn calendar_provider(&self, _provider_key: &str) -> Option<&dyn CalendarConnectorProvider> {
            None
        }

        fn execution_enabled(&self) -> bool {
            true
        }
    }

    impl FakeReadRegistry {
        fn new() -> Self {
            Self {
                mail_calls: AtomicUsize::new(0),
                calendar_calls: AtomicUsize::new(0),
            }
        }
    }

    impl MailConnectorProvider for FakeReadRegistry {
        fn search_mail_page(
            &self,
            _account: &super::super::ConnectorAccount,
            _request: &MailSearchRequest,
            _continuation: Option<&ConnectorReadContinuation>,
        ) -> ConnectorProviderResult<ConnectorReadPage<MailThread>> {
            self.mail_calls.fetch_add(1, Ordering::SeqCst);
            let address = MailAddress {
                display_name: None,
                address: "reader@example.com".to_string(),
            };
            Ok(ConnectorReadPage::new(
                vec![MailThread {
                    remote_ref: "thread:fake".to_string(),
                    messages: vec![MailMessage {
                        remote_ref: "message:fake".to_string(),
                        thread_ref: "thread:fake".to_string(),
                        from: address.clone(),
                        to: vec![address],
                        subject: "Evidence".to_string(),
                        received_at: Utc::now(),
                        bounded_body_summary: Some("Untrusted".to_string()),
                        attachments: vec![],
                        has_attachments: false,
                        untrusted_evidence: true,
                    }],
                }],
                None,
            ))
        }

        fn read_thread(
            &self,
            _account: &super::super::ConnectorAccount,
            _request: &MailThreadRequest,
        ) -> ConnectorProviderResult<MailThread> {
            Err(ConnectorProviderFailure::InvalidResponse)
        }
    }

    impl CalendarConnectorProvider for FakeReadRegistry {
        fn list_events_page(
            &self,
            _account: &super::super::ConnectorAccount,
            request: &CalendarListRequest,
            _continuation: Option<&ConnectorReadContinuation>,
        ) -> ConnectorProviderResult<ConnectorReadPage<CalendarEvent>> {
            self.calendar_calls.fetch_add(1, Ordering::SeqCst);
            Ok(ConnectorReadPage::new(
                vec![CalendarEvent {
                    remote_ref: "event:fake".to_string(),
                    calendar_ref: "calendar:fake".to_string(),
                    title: "Evidence".to_string(),
                    starts_at: request.starts_at(),
                    ends_at: request.starts_at() + Duration::hours(1),
                    timezone: "UTC".to_string(),
                    attendees: vec![CalendarAttendee {
                        address: MailAddress {
                            display_name: None,
                            address: "reader@example.com".to_string(),
                        },
                        response: None,
                    }],
                    meeting_url: None,
                    recurrence: None,
                    untrusted_evidence: true,
                }],
                None,
            ))
        }
    }

    impl ConnectorReadRegistry for FakeReadRegistry {
        fn mail_provider(&self, provider_key: &str) -> Option<&dyn MailConnectorProvider> {
            (provider_key == "fake-read-provider").then_some(self)
        }
        fn calendar_provider(&self, provider_key: &str) -> Option<&dyn CalendarConnectorProvider> {
            (provider_key == "fake-read-provider").then_some(self)
        }
        fn execution_enabled(&self) -> bool {
            true
        }
    }

    fn connected_account(now: DateTime<Utc>) -> super::super::ConnectorAccount {
        super::super::ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "fake-read-provider".to_string(),
            display_name: "Read account".to_string(),
            tenant_ref: Some("tenant".to_string()),
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![
                ConnectorCapability::MailSearch,
                ConnectorCapability::CalendarListEvents,
            ],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn read_plans_are_typed_bounded_and_canonically_fingerprinted() {
        let mail =
            ConnectorReadPlan::mail_search(" urgent ".to_string(), 10).expect("mail plan builds");
        assert_eq!(mail.capability(), ConnectorCapability::MailSearch);
        assert_eq!(mail.fingerprint().unwrap(), mail.fingerprint().unwrap());
        assert!(ConnectorReadPlan::mail_search(String::new(), 10).is_err());
        assert!(ConnectorReadPlan::mail_search("urgent".to_string(), 26).is_err());

        let now = Utc::now();
        let calendar = ConnectorReadPlan::calendar_list(now, now + Duration::days(1), 20)
            .expect("calendar plan builds");
        assert_eq!(
            calendar.capability(),
            ConnectorCapability::CalendarListEvents
        );
        assert!(ConnectorReadPlan::calendar_list(now, now, 20).is_err());
    }

    #[test]
    fn restart_never_replays_a_started_remote_read() {
        assert_eq!(
            ConnectorReadExecutionPhase::Claimed.restart_phase(),
            ConnectorReadExecutionPhase::Pending
        );
        assert_eq!(
            ConnectorReadExecutionPhase::RemoteCallStarted.restart_phase(),
            ConnectorReadExecutionPhase::ReconciliationRequired
        );
        assert!(!ConnectorReadExecutionPhase::ReconciliationRequired.can_claim());
        assert!(!ConnectorReadExecutionPhase::ResultPersisted.can_claim());
        assert!(!ConnectorReadExecutionPhase::Applied.can_claim());
    }

    #[test]
    fn public_view_serialization_excludes_private_authority_fields() {
        let view = ConnectorReadExecutionView {
            id: Uuid::new_v4(),
            kind: ConnectorReadExecutionKind::Mail,
            phase: ConnectorReadExecutionPublicPhase::Completed,
            item_count: Some(3),
            evidence_ref: Some(ConnectorReadEvidenceRef::new(Uuid::new_v4())),
            error_code: None,
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&view).expect("view serializes");
        for forbidden in [
            "provider",
            "tenant",
            "credential",
            "generation",
            "cursor",
            "remote_ref",
            "claim",
            "attempt",
            "query",
        ] {
            assert!(!json.contains(forbidden), "view leaked {forbidden}");
        }
    }

    #[test]
    fn fake_mail_and_calendar_reads_persist_started_before_one_provider_call() {
        let store = Arc::new(Mutex::new(EventStore::open_memory().unwrap()));
        let now = Utc::now();
        let account = connected_account(now);
        {
            let store = store.lock().unwrap();
            store.upsert_connector_account(&account).unwrap();
            store
                .submit_explicit_connector_read_execution(
                    Uuid::new_v4(),
                    account.id,
                    ConnectorReadPlan::mail_search("mail".to_string(), 1).unwrap(),
                    now,
                )
                .unwrap();
            store
                .submit_explicit_connector_read_execution(
                    Uuid::new_v4(),
                    account.id,
                    ConnectorReadPlan::calendar_list(now, now + Duration::hours(2), 1).unwrap(),
                    now,
                )
                .unwrap();
        }
        let registry = FakeReadRegistry::new();
        let sweep = run_connector_read_executions_with_shared_store(&store, &registry, 2).unwrap();
        assert_eq!(sweep.claimed, 2);
        assert_eq!(sweep.completed, 2);
        assert_eq!(registry.mail_calls.load(Ordering::SeqCst), 1);
        assert_eq!(registry.calendar_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            store
                .lock()
                .unwrap()
                .connector_read_applied_count_for_test()
                .unwrap(),
            2
        );
    }

    #[test]
    fn enabled_registry_missing_exact_provider_stops_before_remote_call() {
        let store = Arc::new(Mutex::new(EventStore::open_memory().unwrap()));
        let now = Utc::now();
        let mut account = connected_account(now);
        account.provider_id = "missing-provider".to_string();
        let execution_id = {
            let store = store.lock().unwrap();
            store.upsert_connector_account(&account).unwrap();
            store
                .submit_explicit_connector_read_execution(
                    Uuid::new_v4(),
                    account.id,
                    ConnectorReadPlan::mail_search("mail".to_string(), 1).unwrap(),
                    now,
                )
                .unwrap()
                .1
                .id
        };
        let registry = FakeReadRegistry::new();
        let sweep = run_connector_read_executions_with_shared_store(&store, &registry, 1).unwrap();
        assert_eq!(sweep.claimed, 1);
        assert_eq!(sweep.unavailable, 1);
        assert_eq!(registry.mail_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            store
                .lock()
                .unwrap()
                .connector_read_phase_for_test(execution_id)
                .unwrap(),
            (
                "repair_required".to_string(),
                Some("provider_temporarily_unavailable".to_string())
            )
        );
    }

    #[test]
    fn post_io_private_authority_tamper_never_persists_or_applies_result() {
        for tamper in [
            "provider",
            "tenant",
            "credential",
            "generation",
            "health",
            "capability",
            "source",
        ] {
            let store = Arc::new(Mutex::new(EventStore::open_memory().unwrap()));
            let now = Utc::now();
            let account = connected_account(now);
            let execution_id = {
                let store = store.lock().unwrap();
                store.upsert_connector_account(&account).unwrap();
                store
                    .submit_explicit_connector_read_execution(
                        Uuid::new_v4(),
                        account.id,
                        ConnectorReadPlan::mail_search("blocked".to_string(), 1).unwrap(),
                        now,
                    )
                    .unwrap()
                    .1
                    .id
            };
            let registry = Arc::new(BlockingReadRegistry::new());
            let worker_store = Arc::clone(&store);
            let worker_registry = Arc::clone(&registry);
            let worker = std::thread::spawn(move || {
                run_connector_read_executions_with_shared_store(
                    &worker_store,
                    worker_registry.as_ref(),
                    1,
                )
            });
            registry.entered.wait();
            let store_guard = store.lock().unwrap();
            if tamper == "generation" {
                store_guard
                    .advance_connector_read_generation_for_test(account.id)
                    .unwrap();
            } else if tamper == "source" {
                store_guard
                    .cancel_connector_read_source_for_test(execution_id)
                    .unwrap();
            } else {
                let mut changed = account.clone();
                match tamper {
                    "provider" => changed.provider_id = "changed-provider".to_string(),
                    "tenant" => changed.tenant_ref = Some("changed-tenant".to_string()),
                    "credential" => changed.credential_handle = ConnectorCredentialHandle::new(),
                    "health" => changed.health = ConnectorHealth::NeedsRepair,
                    "capability" => changed
                        .granted_capabilities
                        .retain(|capability| *capability != ConnectorCapability::MailSearch),
                    _ => unreachable!(),
                }
                store_guard.upsert_connector_account(&changed).unwrap();
            }
            drop(store_guard);
            registry.release.wait();
            let sweep = worker.join().unwrap().unwrap();
            assert_eq!(registry.calls.load(Ordering::SeqCst), 1, "{tamper}");
            assert_eq!(sweep.completed, 0, "{tamper}");
            assert_eq!(sweep.lost_claim, 1, "{tamper}");
            assert_eq!(
                store
                    .lock()
                    .unwrap()
                    .connector_read_phase_for_test(execution_id)
                    .unwrap(),
                (
                    "authority_lost".to_string(),
                    Some("connection_needs_attention".to_string())
                ),
                "{tamper}"
            );
            assert_eq!(
                store
                    .lock()
                    .unwrap()
                    .connector_read_applied_count_for_test()
                    .unwrap(),
                0,
                "{tamper}"
            );
        }
    }

    #[test]
    fn restart_never_recalls_started_and_replays_only_local_result_apply() {
        let store = Arc::new(Mutex::new(EventStore::open_memory().unwrap()));
        let now = Utc::now();
        let account = connected_account(now);
        let (started_id, persisted_id) = {
            let store = store.lock().unwrap();
            store.upsert_connector_account(&account).unwrap();
            let started = store
                .submit_explicit_connector_read_execution(
                    Uuid::new_v4(),
                    account.id,
                    ConnectorReadPlan::mail_search("started".to_string(), 1).unwrap(),
                    now,
                )
                .unwrap()
                .1;
            let persisted = store
                .submit_explicit_connector_read_execution(
                    Uuid::new_v4(),
                    account.id,
                    ConnectorReadPlan::mail_search("persisted".to_string(), 1).unwrap(),
                    now,
                )
                .unwrap()
                .1;
            let claims = store.claim_due_connector_read_executions(now, 2).unwrap();
            let started_claim = claims
                .iter()
                .find(|claim| claim.execution_id == started.id)
                .unwrap();
            let persisted_claim = claims
                .iter()
                .find(|claim| claim.execution_id == persisted.id)
                .unwrap();
            store
                .mark_connector_read_remote_call_started(started_claim, now)
                .unwrap();
            store
                .mark_connector_read_remote_call_started(persisted_claim, now)
                .unwrap();
            let result = ConnectorReadResult::mail(Vec::new()).unwrap();
            store
                .persist_connector_read_result(persisted_claim, &result, now)
                .unwrap();
            store
                .reset_connector_read_executions_after_restart(now + Duration::seconds(1))
                .unwrap();
            (started.id, persisted.id)
        };
        let registry = FakeReadRegistry::new();
        let sweep = run_connector_read_executions_with_shared_store(&store, &registry, 4).unwrap();
        assert_eq!(sweep.claimed, 0);
        assert_eq!(sweep.recovered_applies, 1);
        assert_eq!(registry.mail_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            store
                .lock()
                .unwrap()
                .connector_read_phase_for_test(started_id)
                .unwrap()
                .0,
            "reconciliation_required"
        );
        assert_eq!(
            store
                .lock()
                .unwrap()
                .connector_read_phase_for_test(persisted_id)
                .unwrap()
                .0,
            "applied"
        );
    }

    #[test]
    fn file_backed_restart_preserves_started_uncertainty_and_replays_only_local_apply() {
        let temp_dir = tempfile::tempdir().unwrap();
        let database_path = temp_dir.path().join("connector-read.sqlite3");
        let now = Utc::now();
        let account = connected_account(now);
        let (started_id, persisted_id) = {
            let store = EventStore::open(&database_path).unwrap();
            store.upsert_connector_account(&account).unwrap();
            let started = store
                .submit_explicit_connector_read_execution(
                    Uuid::new_v4(),
                    account.id,
                    ConnectorReadPlan::mail_search("crash-after-start".to_string(), 1).unwrap(),
                    now,
                )
                .unwrap()
                .1;
            let persisted = store
                .submit_explicit_connector_read_execution(
                    Uuid::new_v4(),
                    account.id,
                    ConnectorReadPlan::mail_search("crash-after-result".to_string(), 1).unwrap(),
                    now + Duration::milliseconds(1),
                )
                .unwrap()
                .1;
            let claims = store
                .claim_due_connector_read_executions(now + Duration::seconds(1), 2)
                .unwrap();
            let started_claim = claims
                .iter()
                .find(|claim| claim.execution_id == started.id)
                .unwrap();
            let persisted_claim = claims
                .iter()
                .find(|claim| claim.execution_id == persisted.id)
                .unwrap();
            store
                .mark_connector_read_remote_call_started(started_claim, now + Duration::seconds(1))
                .unwrap();
            store
                .mark_connector_read_remote_call_started(
                    persisted_claim,
                    now + Duration::seconds(1),
                )
                .unwrap();
            store
                .persist_connector_read_result(
                    persisted_claim,
                    &ConnectorReadResult::mail(Vec::new()).unwrap(),
                    now + Duration::seconds(2),
                )
                .unwrap();
            (started.id, persisted.id)
        };
        let reopened = EventStore::open(&database_path).unwrap();
        reopened
            .reset_connector_read_executions_after_restart(now + Duration::seconds(3))
            .unwrap();
        let store = Arc::new(Mutex::new(reopened));
        let registry = FakeReadRegistry::new();
        let sweep = run_connector_read_executions_with_shared_store(&store, &registry, 4).unwrap();
        assert_eq!(registry.mail_calls.load(Ordering::SeqCst), 0);
        assert_eq!(sweep.claimed, 0);
        assert_eq!(sweep.recovered_applies, 1);
        assert_eq!(
            store
                .lock()
                .unwrap()
                .connector_read_phase_for_test(started_id)
                .unwrap(),
            (
                "reconciliation_required".to_string(),
                Some("external_result_uncertain".to_string())
            )
        );
        assert_eq!(
            store
                .lock()
                .unwrap()
                .connector_read_phase_for_test(persisted_id)
                .unwrap()
                .0,
            "applied"
        );
    }

    #[test]
    fn automation_file_backed_restart_never_recalls_started_and_applies_persisted_only() {
        let temp_dir = tempfile::tempdir().unwrap();
        let database_path = temp_dir.path().join("automation-connector-read.sqlite3");
        let now = Utc::now();
        let account = connected_account(now);
        let (started_id, persisted_id) = {
            let store = EventStore::open(&database_path).unwrap();
            store.upsert_connector_account(&account).unwrap();
            let definition = AutomationDefinition::once(
                "untrusted goal cannot select connector inputs".to_string(),
                "UTC".to_string(),
                now,
            )
            .unwrap();
            let definition = store.upsert_automation_definition(&definition).unwrap();
            let plan =
                ConnectorReadPlan::mail_search("typed-restart-query".to_string(), 1).unwrap();
            store
                .bind_automation_connector_read_plan(
                    definition.id,
                    definition.revision,
                    account.id,
                    plan.clone(),
                    now,
                )
                .unwrap();
            let mut runs = Vec::new();
            for label in ["started", "persisted"] {
                let (run, _) = store
                    .enqueue_manual_automation_agent_run(
                        definition.id,
                        Uuid::new_v4(),
                        now,
                        format!("automation-{label}"),
                    )
                    .unwrap();
                store
                    .transition_automation_run(
                        run.id,
                        AutomationRunStatus::Running,
                        None,
                        None,
                        now,
                    )
                    .unwrap();
                let execution = store
                    .submit_automation_connector_read_execution(
                        run.id,
                        account.id,
                        plan.clone(),
                        now,
                    )
                    .unwrap()
                    .1;
                runs.push(execution);
            }
            let claims = store.claim_due_connector_read_executions(now, 2).unwrap();
            for claim in &claims {
                store
                    .mark_connector_read_remote_call_started(claim, now)
                    .unwrap();
            }
            let persisted_claim = claims
                .iter()
                .find(|claim| claim.execution_id == runs[1].id)
                .unwrap();
            store
                .persist_connector_read_result(
                    persisted_claim,
                    &ConnectorReadResult::mail(Vec::new()).unwrap(),
                    now,
                )
                .unwrap();
            (runs[0].id, runs[1].id)
        };
        let reopened = EventStore::open(&database_path).unwrap();
        reopened
            .reset_connector_read_executions_after_restart(now + Duration::seconds(1))
            .unwrap();
        let store = Arc::new(Mutex::new(reopened));
        let registry = FakeReadRegistry::new();
        let sweep = run_connector_read_executions_with_shared_store(&store, &registry, 4).unwrap();
        assert_eq!(registry.mail_calls.load(Ordering::SeqCst), 0);
        assert_eq!(sweep.claimed, 0);
        assert_eq!(sweep.recovered_applies, 1);
        assert_eq!(
            store
                .lock()
                .unwrap()
                .connector_read_phase_for_test(started_id)
                .unwrap()
                .0,
            "reconciliation_required"
        );
        assert_eq!(
            store
                .lock()
                .unwrap()
                .connector_read_phase_for_test(persisted_id)
                .unwrap()
                .0,
            "applied"
        );
    }
}
