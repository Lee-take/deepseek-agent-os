use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroize;

use super::domain::{CalendarEvent, MailMessage};
use super::provider::{
    ConnectorProviderFailure, ConnectorProviderResult, MAX_CALENDAR_EVENTS, MAX_MAIL_SEARCH_RESULTS,
};
use super::runtime_registry::ConnectorSyncRegistry;
use super::{ConnectorAccount, ConnectorCapability, ConnectorHealth};
use crate::kernel::event_store::{EventStore, EventStoreError, EventStoreResult};

const MAX_CONTINUATION_CHARS: usize = 8192;
pub const MAX_SYNC_PROJECTION_ITEMS_PER_STREAM: usize = 500;
pub const MAX_SYNC_PROJECTION_ITEMS_PER_ACCOUNT: usize = 1000;
pub const MAX_SYNC_PROJECTION_BYTES_PER_ACCOUNT: usize = 2 * 1024 * 1024;
pub const MAX_SYNC_PROJECTION_ITEM_BYTES: usize = 32 * 1024;
pub const MAX_SYNC_STREAMS_PER_ACCOUNT: usize = 8;
pub const MAX_SYNC_STREAM_IDLE_DAYS: i64 = 90;

#[derive(Clone, Eq, PartialEq)]
pub struct ConnectorOpaqueContinuation(String);

impl ConnectorOpaqueContinuation {
    pub(crate) fn new(mut value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.chars().count() > MAX_CONTINUATION_CHARS {
            value.zeroize();
            return Err("connector continuation is invalid".to_string());
        }
        if trimmed.len() != value.len() {
            let normalized = trimmed.to_string();
            value.zeroize();
            value = normalized;
        }
        Ok(Self(value))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for ConnectorOpaqueContinuation {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub enum ConnectorSyncChange<T> {
    Upsert(T),
    Deleted { remote_ref: String },
}

pub enum ConnectorSyncContinuation {
    Next(ConnectorOpaqueContinuation),
    Delta(ConnectorOpaqueContinuation),
}

pub struct ConnectorSyncPage<T> {
    changes: Vec<ConnectorSyncChange<T>>,
    continuation: ConnectorSyncContinuation,
}

impl<T> ConnectorSyncPage<T> {
    pub fn new(
        changes: Vec<ConnectorSyncChange<T>>,
        continuation: ConnectorSyncContinuation,
    ) -> Self {
        Self {
            changes,
            continuation,
        }
    }

    pub fn changes(&self) -> &[ConnectorSyncChange<T>] {
        &self.changes
    }

    pub fn continuation(&self) -> &ConnectorSyncContinuation {
        &self.continuation
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailSyncRequest {
    max_changes: u16,
}

impl MailSyncRequest {
    pub fn inbox(max_changes: u16) -> Result<Self, String> {
        if max_changes == 0 || max_changes > MAX_MAIL_SEARCH_RESULTS {
            return Err(format!(
                "mail sync page size must be between 1 and {MAX_MAIL_SEARCH_RESULTS}"
            ));
        }
        Ok(Self { max_changes })
    }

    pub fn max_changes(&self) -> u16 {
        self.max_changes
    }

    pub fn stream_fingerprint(&self, provider_id: &str) -> String {
        stream_fingerprint(&format!("{provider_id}|mail|inbox"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarSyncRequest {
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
    max_changes: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum ConnectorSyncPlan {
    MailInbox {
        max_changes: u16,
    },
    CalendarRange {
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        max_changes: u16,
    },
}

impl ConnectorSyncPlan {
    fn mail(request: &MailSyncRequest) -> Self {
        Self::MailInbox {
            max_changes: request.max_changes(),
        }
    }

    fn calendar(request: &CalendarSyncRequest) -> Self {
        Self::CalendarRange {
            starts_at: request.starts_at(),
            ends_at: request.ends_at(),
            max_changes: request.max_changes(),
        }
    }

    pub(crate) fn persistence_json(&self) -> Result<String, String> {
        serde_json::to_string(self)
            .map_err(|_| "connector sync plan could not be encoded".to_string())
    }

    pub(crate) fn from_persistence_json(value: &str) -> Result<Self, String> {
        let plan: Self = serde_json::from_str(value)
            .map_err(|_| "connector sync plan is invalid".to_string())?;
        match &plan {
            Self::MailInbox { max_changes } => {
                MailSyncRequest::inbox(*max_changes)?;
            }
            Self::CalendarRange {
                starts_at,
                ends_at,
                max_changes,
            } => {
                CalendarSyncRequest::new(*starts_at, *ends_at, *max_changes)?;
            }
        }
        Ok(plan)
    }

    pub(crate) fn mail_request(&self) -> Result<MailSyncRequest, String> {
        match self {
            Self::MailInbox { max_changes } => MailSyncRequest::inbox(*max_changes),
            Self::CalendarRange { .. } => {
                Err("connector sync plan is not a mail request".to_string())
            }
        }
    }

    pub(crate) fn calendar_request(&self) -> Result<CalendarSyncRequest, String> {
        match self {
            Self::CalendarRange {
                starts_at,
                ends_at,
                max_changes,
            } => CalendarSyncRequest::new(*starts_at, *ends_at, *max_changes),
            Self::MailInbox { .. } => {
                Err("connector sync plan is not a calendar request".to_string())
            }
        }
    }
}

impl CalendarSyncRequest {
    pub fn new(
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        max_changes: u16,
    ) -> Result<Self, String> {
        if ends_at <= starts_at || ends_at.signed_duration_since(starts_at) > Duration::days(366) {
            return Err("calendar sync range is invalid".to_string());
        }
        if max_changes == 0 || max_changes > MAX_CALENDAR_EVENTS {
            return Err(format!(
                "calendar sync page size must be between 1 and {MAX_CALENDAR_EVENTS}"
            ));
        }
        Ok(Self {
            starts_at,
            ends_at,
            max_changes,
        })
    }

    pub fn starts_at(&self) -> DateTime<Utc> {
        self.starts_at
    }

    pub fn ends_at(&self) -> DateTime<Utc> {
        self.ends_at
    }

    pub fn max_changes(&self) -> u16 {
        self.max_changes
    }

    pub fn stream_fingerprint(&self, provider_id: &str) -> String {
        stream_fingerprint(&format!(
            "{provider_id}|calendar|{}|{}",
            self.starts_at.to_rfc3339(),
            self.ends_at.to_rfc3339()
        ))
    }
}

pub trait MailSyncProvider: Send + Sync {
    fn sync_mail_page(
        &self,
        account: &ConnectorAccount,
        request: &MailSyncRequest,
        continuation: Option<&ConnectorOpaqueContinuation>,
    ) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>>;
}

pub trait CalendarSyncProvider: Send + Sync {
    fn sync_calendar_page(
        &self,
        account: &ConnectorAccount,
        request: &CalendarSyncRequest,
        continuation: Option<&ConnectorOpaqueContinuation>,
    ) -> ConnectorProviderResult<ConnectorSyncPage<CalendarEvent>>;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ConnectorSyncRecoverySweep {
    pub claimed: usize,
    pub completed: usize,
    pub deferred: usize,
    pub repair_required: usize,
    pub lost_claim: usize,
}

pub(crate) fn run_connector_sync_recovery_with_shared_store(
    event_store: &Arc<Mutex<EventStore>>,
    registry: &dyn ConnectorSyncRegistry,
    limit: usize,
) -> EventStoreResult<ConnectorSyncRecoverySweep> {
    if !registry.execution_enabled() || limit == 0 {
        return Ok(ConnectorSyncRecoverySweep::default());
    }
    let claims = event_store
        .lock()
        .map_err(|_| {
            EventStoreError::InvalidState(
                "connector sync recovery store is unavailable".to_string(),
            )
        })?
        .claim_due_connector_sync_recovery_jobs(Utc::now(), limit)?;
    let mut sweep = ConnectorSyncRecoverySweep {
        claimed: claims.len(),
        ..ConnectorSyncRecoverySweep::default()
    };
    for mut claim in claims {
        let renew_now = Utc::now();
        if event_store
            .lock()
            .map_err(|_| {
                EventStoreError::InvalidState(
                    "connector sync recovery store is unavailable".to_string(),
                )
            })?
            .renew_connector_sync_recovery_claim(&mut claim, renew_now)
            .is_err()
        {
            sweep.lost_claim += 1;
            continue;
        }
        let provider_key = claim.account().provider_id.as_str();
        match claim.plan() {
            ConnectorSyncPlan::MailInbox { .. } => {
                let Some(provider) = registry.mail_provider(provider_key) else {
                    let unavailable_now = Utc::now();
                    match event_store
                        .lock()
                        .map_err(|_| {
                            EventStoreError::InvalidState(
                                "connector sync recovery store is unavailable".to_string(),
                            )
                        })?
                        .relinquish_unavailable_connector_sync_recovery_claim(
                            &claim,
                            unavailable_now,
                        ) {
                        Ok(()) => sweep.deferred += 1,
                        Err(_) => sweep.lost_claim += 1,
                    }
                    continue;
                };
                let result = if claim.state().capability() != ConnectorCapability::MailSyncInbox {
                    Err(ConnectorProviderFailure::InvalidResponse)
                } else {
                    claim
                        .plan()
                        .mail_request()
                        .map_err(|_| ConnectorProviderFailure::InvalidResponse)
                        .and_then(|request| {
                            provider.sync_mail_page(
                                claim.account(),
                                &request,
                                claim.state().locator(),
                            )
                        })
                };
                let completion_now = Utc::now();
                if event_store
                    .lock()
                    .map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector sync recovery store is unavailable".to_string(),
                        )
                    })?
                    .renew_connector_sync_recovery_claim(&mut claim, completion_now)
                    .is_err()
                {
                    sweep.lost_claim += 1;
                    continue;
                }
                let store = event_store.lock().map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync recovery store is unavailable".to_string(),
                    )
                })?;
                match result {
                    Ok(page) => match store.complete_claimed_mail_sync_recovery(
                        &claim,
                        &page,
                        completion_now,
                    ) {
                        Ok(next) if next.stopped() => sweep.repair_required += 1,
                        Ok(next) if next.has_resume_page() => sweep.deferred += 1,
                        Ok(_) => sweep.completed += 1,
                        Err(_) => sweep.lost_claim += 1,
                    },
                    Err(failure) => match store.finalize_claimed_connector_sync_recovery_failure(
                        &claim,
                        failure,
                        completion_now,
                    ) {
                        Ok(next) if next.stopped() => sweep.repair_required += 1,
                        Ok(_) => sweep.deferred += 1,
                        Err(_) => sweep.lost_claim += 1,
                    },
                }
            }
            ConnectorSyncPlan::CalendarRange { .. } => {
                let Some(provider) = registry.calendar_provider(provider_key) else {
                    let unavailable_now = Utc::now();
                    match event_store
                        .lock()
                        .map_err(|_| {
                            EventStoreError::InvalidState(
                                "connector sync recovery store is unavailable".to_string(),
                            )
                        })?
                        .relinquish_unavailable_connector_sync_recovery_claim(
                            &claim,
                            unavailable_now,
                        ) {
                        Ok(()) => sweep.deferred += 1,
                        Err(_) => sweep.lost_claim += 1,
                    }
                    continue;
                };
                let result =
                    if claim.state().capability() != ConnectorCapability::CalendarSyncEvents {
                        Err(ConnectorProviderFailure::InvalidResponse)
                    } else {
                        claim
                            .plan()
                            .calendar_request()
                            .map_err(|_| ConnectorProviderFailure::InvalidResponse)
                            .and_then(|request| {
                                provider.sync_calendar_page(
                                    claim.account(),
                                    &request,
                                    claim.state().locator(),
                                )
                            })
                    };
                let completion_now = Utc::now();
                if event_store
                    .lock()
                    .map_err(|_| {
                        EventStoreError::InvalidState(
                            "connector sync recovery store is unavailable".to_string(),
                        )
                    })?
                    .renew_connector_sync_recovery_claim(&mut claim, completion_now)
                    .is_err()
                {
                    sweep.lost_claim += 1;
                    continue;
                }
                let store = event_store.lock().map_err(|_| {
                    EventStoreError::InvalidState(
                        "connector sync recovery store is unavailable".to_string(),
                    )
                })?;
                match result {
                    Ok(page) => match store.complete_claimed_calendar_sync_recovery(
                        &claim,
                        &page,
                        completion_now,
                    ) {
                        Ok(next) if next.stopped() => sweep.repair_required += 1,
                        Ok(next) if next.has_resume_page() => sweep.deferred += 1,
                        Ok(_) => sweep.completed += 1,
                        Err(_) => sweep.lost_claim += 1,
                    },
                    Err(failure) => match store.finalize_claimed_connector_sync_recovery_failure(
                        &claim,
                        failure,
                        completion_now,
                    ) {
                        Ok(next) if next.stopped() => sweep.repair_required += 1,
                        Ok(_) => sweep.deferred += 1,
                        Err(_) => sweep.lost_claim += 1,
                    },
                }
            }
        }
    }
    Ok(sweep)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorSyncReceipt {
    pub account_id: Uuid,
    pub account_generation: u64,
    pub capability: ConnectorCapability,
    pub stream_fingerprint: String,
    pub change_count: usize,
    pub has_resume_page: bool,
    pub has_committed_delta: bool,
    pub revision: u64,
    pub committed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorSyncStateReceipt {
    pub account_id: Uuid,
    pub account_generation: u64,
    pub capability: ConnectorCapability,
    pub stream_fingerprint: String,
    pub reason: String,
    pub has_resume_page: bool,
    pub has_committed_delta: bool,
    pub retry_at: Option<DateTime<Utc>>,
    pub stopped: bool,
    pub rebuild_attempt: u32,
    pub revision: u64,
    pub changed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorSyncProjectionSummary {
    pub remote_ref: String,
    pub deleted: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ConnectorSyncState {
    account_id: Uuid,
    account_generation: u64,
    capability: ConnectorCapability,
    stream_fingerprint: String,
    committed_delta: Option<ConnectorOpaqueContinuation>,
    resume_page: Option<ConnectorOpaqueContinuation>,
    revision: u64,
    retry_state: Option<ConnectorRetryState>,
    rebuild_attempt: u32,
    stopped: bool,
    updated_at: DateTime<Utc>,
}

pub enum ConnectorSyncStateRecovery {
    Persist {
        next: ConnectorSyncState,
        reason: &'static str,
    },
    RepairAccount,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ConnectorSyncStep {
    Committed {
        revision: u64,
        change_count: usize,
        delta_committed: bool,
    },
    Deferred {
        retry_at: DateTime<Utc>,
    },
    RebuildScheduled {
        revision: u64,
    },
    RepairRequired,
    Stopped,
}

impl ConnectorSyncState {
    pub fn initial(
        account_id: Uuid,
        capability: ConnectorCapability,
        stream_fingerprint: String,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        Self::initial_with_generation(account_id, 0, capability, stream_fingerprint, now)
    }

    pub(crate) fn initial_with_generation(
        account_id: Uuid,
        account_generation: u64,
        capability: ConnectorCapability,
        stream_fingerprint: String,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        if stream_fingerprint.trim().is_empty() {
            return Err("connector sync stream fingerprint is required".to_string());
        }
        Ok(Self {
            account_id,
            account_generation,
            capability,
            stream_fingerprint,
            committed_delta: None,
            resume_page: None,
            revision: 0,
            retry_state: None,
            rebuild_attempt: 0,
            stopped: false,
            updated_at: now,
        })
    }

    pub fn account_id(&self) -> Uuid {
        self.account_id
    }

    pub fn account_generation(&self) -> u64 {
        self.account_generation
    }

    pub fn capability(&self) -> ConnectorCapability {
        self.capability
    }

    pub fn stream_fingerprint(&self) -> &str {
        &self.stream_fingerprint
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    pub fn retry_state(&self) -> Option<&ConnectorRetryState> {
        self.retry_state.as_ref()
    }

    pub fn rebuild_attempt(&self) -> u32 {
        self.rebuild_attempt
    }

    pub fn ready(&self, now: DateTime<Utc>) -> bool {
        !self.stopped
            && self
                .retry_state
                .as_ref()
                .is_none_or(|retry| retry.retry_at <= now)
    }

    pub fn stopped(&self) -> bool {
        self.stopped
    }

    pub(crate) fn resume_after_user_recovery(&self, now: DateTime<Utc>) -> Result<Self, String> {
        if !self.stopped {
            return Err("connector sync stream is not stopped".to_string());
        }
        self.next_state(
            self.committed_delta.clone(),
            self.resume_page.clone(),
            None,
            0,
            false,
            now,
        )
    }

    pub(crate) fn stop_after_recovery_failure(&self, now: DateTime<Utc>) -> Result<Self, String> {
        self.next_state(
            self.committed_delta.clone(),
            self.resume_page.clone(),
            None,
            self.rebuild_attempt,
            true,
            now,
        )
    }

    pub fn locator(&self) -> Option<&ConnectorOpaqueContinuation> {
        self.resume_page.as_ref().or(self.committed_delta.as_ref())
    }

    pub fn has_resume_page(&self) -> bool {
        self.resume_page.is_some()
    }

    pub fn has_committed_delta(&self) -> bool {
        self.committed_delta.is_some()
    }

    pub fn advance<T>(
        &self,
        page: &ConnectorSyncPage<T>,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "connector sync revision overflowed".to_string())?;
        let (committed_delta, resume_page, rebuild_attempt) = match page.continuation() {
            ConnectorSyncContinuation::Next(next) => (
                self.committed_delta.clone(),
                Some(next.clone()),
                self.rebuild_attempt,
            ),
            ConnectorSyncContinuation::Delta(delta) => (Some(delta.clone()), None, 0),
        };
        Ok(Self {
            account_id: self.account_id,
            account_generation: self.account_generation,
            capability: self.capability,
            stream_fingerprint: self.stream_fingerprint.clone(),
            committed_delta,
            resume_page,
            revision,
            retry_state: None,
            rebuild_attempt,
            stopped: false,
            updated_at: now,
        })
    }

    pub fn recovery(
        &self,
        failure: ConnectorSyncFailure,
        attempt: u32,
        max_attempts: u32,
        now: DateTime<Utc>,
    ) -> Result<ConnectorSyncStateRecovery, String> {
        match sync_recovery(failure, attempt, max_attempts, now) {
            ConnectorSyncRecovery::RebuildCursor => Ok(ConnectorSyncStateRecovery::Persist {
                next: self.next_state(
                    None,
                    None,
                    None,
                    self.rebuild_attempt.saturating_add(1),
                    false,
                    now,
                )?,
                reason: "cursor_rebuilt",
            }),
            ConnectorSyncRecovery::Retry(retry) => Ok(ConnectorSyncStateRecovery::Persist {
                next: self.next_state(
                    self.committed_delta.clone(),
                    self.resume_page.clone(),
                    Some(retry),
                    self.rebuild_attempt,
                    false,
                    now,
                )?,
                reason: "retry_scheduled",
            }),
            ConnectorSyncRecovery::RepairAccount => Ok(ConnectorSyncStateRecovery::RepairAccount),
            ConnectorSyncRecovery::Stop => Ok(ConnectorSyncStateRecovery::Persist {
                next: self.next_state(
                    self.committed_delta.clone(),
                    self.resume_page.clone(),
                    self.retry_state.clone(),
                    self.rebuild_attempt,
                    true,
                    now,
                )?,
                reason: "retry_exhausted",
            }),
        }
    }

    fn next_state(
        &self,
        committed_delta: Option<ConnectorOpaqueContinuation>,
        resume_page: Option<ConnectorOpaqueContinuation>,
        retry_state: Option<ConnectorRetryState>,
        rebuild_attempt: u32,
        stopped: bool,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        Ok(Self {
            account_id: self.account_id,
            account_generation: self.account_generation,
            capability: self.capability,
            stream_fingerprint: self.stream_fingerprint.clone(),
            committed_delta,
            resume_page,
            revision: self
                .revision
                .checked_add(1)
                .ok_or_else(|| "connector sync revision overflowed".to_string())?,
            retry_state,
            rebuild_attempt,
            stopped,
            updated_at: now,
        })
    }

    pub(crate) fn persistence_json(&self) -> Result<String, String> {
        serde_json::to_string(&PersistedConnectorSyncState::from(self))
            .map_err(|_| "connector sync state could not be encoded".to_string())
    }

    pub(crate) fn from_persistence_json(mut value: String) -> Result<Self, String> {
        let stored =
            serde_json::from_str(&value).map_err(|_| "connector sync state is invalid".to_string());
        value.zeroize();
        let stored: PersistedConnectorSyncState = stored?;
        stored.try_into()
    }
}

#[derive(Deserialize, Serialize)]
struct PersistedConnectorSyncState {
    account_id: Uuid,
    #[serde(default)]
    account_generation: u64,
    capability: ConnectorCapability,
    stream_fingerprint: String,
    committed_delta: Option<String>,
    resume_page: Option<String>,
    revision: u64,
    retry_state: Option<ConnectorRetryState>,
    #[serde(default)]
    rebuild_attempt: u32,
    #[serde(default)]
    stopped: bool,
    updated_at: DateTime<Utc>,
}

impl Drop for PersistedConnectorSyncState {
    fn drop(&mut self) {
        if let Some(value) = &mut self.committed_delta {
            value.zeroize();
        }
        if let Some(value) = &mut self.resume_page {
            value.zeroize();
        }
    }
}

impl From<&ConnectorSyncState> for PersistedConnectorSyncState {
    fn from(state: &ConnectorSyncState) -> Self {
        Self {
            account_id: state.account_id,
            account_generation: state.account_generation,
            capability: state.capability,
            stream_fingerprint: state.stream_fingerprint.clone(),
            committed_delta: state
                .committed_delta
                .as_ref()
                .map(|value| value.expose().to_string()),
            resume_page: state
                .resume_page
                .as_ref()
                .map(|value| value.expose().to_string()),
            revision: state.revision,
            retry_state: state.retry_state.clone(),
            rebuild_attempt: state.rebuild_attempt,
            stopped: state.stopped,
            updated_at: state.updated_at,
        }
    }
}

impl TryFrom<PersistedConnectorSyncState> for ConnectorSyncState {
    type Error = String;

    fn try_from(mut stored: PersistedConnectorSyncState) -> Result<Self, Self::Error> {
        let committed_delta = stored
            .committed_delta
            .take()
            .map(ConnectorOpaqueContinuation::new)
            .transpose()?;
        let resume_page = stored
            .resume_page
            .take()
            .map(ConnectorOpaqueContinuation::new)
            .transpose()?;
        Ok(Self {
            account_id: stored.account_id,
            account_generation: stored.account_generation,
            capability: stored.capability,
            stream_fingerprint: std::mem::take(&mut stored.stream_fingerprint),
            committed_delta,
            resume_page,
            revision: stored.revision,
            retry_state: stored.retry_state.take(),
            rebuild_attempt: stored.rebuild_attempt,
            stopped: stored.stopped,
            updated_at: stored.updated_at,
        })
    }
}

fn stream_fingerprint(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

pub fn run_mail_sync_step(
    store: &EventStore,
    provider: &dyn MailSyncProvider,
    account: &mut ConnectorAccount,
    request: &MailSyncRequest,
    now: DateTime<Utc>,
) -> Result<ConnectorSyncStep, String> {
    if account.health != ConnectorHealth::Connected {
        return Ok(ConnectorSyncStep::RepairRequired);
    }
    if !account
        .granted_capabilities
        .contains(&ConnectorCapability::MailSyncInbox)
    {
        return Err("mail inbox sync requires separate durable-sync consent".to_string());
    }
    let stream = request.stream_fingerprint(&account.provider_id);
    let state = load_or_initialize_sync_state(
        store,
        account,
        ConnectorCapability::MailSyncInbox,
        stream,
        now,
    )?;
    store
        .record_connector_sync_plan(
            &state,
            &ConnectorSyncPlan::mail(request).persistence_json()?,
        )
        .map_err(|_| "connector mail sync plan could not be recorded".to_string())?;
    if state.stopped() {
        return Ok(ConnectorSyncStep::Stopped);
    }
    if let Some(retry) = state.retry_state().filter(|retry| retry.retry_at > now) {
        return Ok(ConnectorSyncStep::Deferred {
            retry_at: retry.retry_at,
        });
    }
    match provider.sync_mail_page(account, request, state.locator()) {
        Ok(page) => {
            let change_count = page.changes().len();
            let next = store
                .commit_connector_sync_page(
                    &state,
                    &page,
                    |message| message.remote_ref.as_str(),
                    now,
                )
                .map_err(|_| "connector mail sync page could not be committed".to_string())?;
            Ok(ConnectorSyncStep::Committed {
                revision: next.revision(),
                change_count,
                delta_committed: next.has_committed_delta() && !next.has_resume_page(),
            })
        }
        Err(failure) => handle_sync_failure(store, account, &state, failure, now),
    }
}

pub fn run_calendar_sync_step(
    store: &EventStore,
    provider: &dyn CalendarSyncProvider,
    account: &mut ConnectorAccount,
    request: &CalendarSyncRequest,
    now: DateTime<Utc>,
) -> Result<ConnectorSyncStep, String> {
    if account.health != ConnectorHealth::Connected {
        return Ok(ConnectorSyncStep::RepairRequired);
    }
    if !account
        .granted_capabilities
        .contains(&ConnectorCapability::CalendarSyncEvents)
    {
        return Err("calendar sync requires separate durable-sync consent".to_string());
    }
    let stream = request.stream_fingerprint(&account.provider_id);
    let state = load_or_initialize_sync_state(
        store,
        account,
        ConnectorCapability::CalendarSyncEvents,
        stream,
        now,
    )?;
    store
        .record_connector_sync_plan(
            &state,
            &ConnectorSyncPlan::calendar(request).persistence_json()?,
        )
        .map_err(|_| "connector calendar sync plan could not be recorded".to_string())?;
    if state.stopped() {
        return Ok(ConnectorSyncStep::Stopped);
    }
    if let Some(retry) = state.retry_state().filter(|retry| retry.retry_at > now) {
        return Ok(ConnectorSyncStep::Deferred {
            retry_at: retry.retry_at,
        });
    }
    match provider.sync_calendar_page(account, request, state.locator()) {
        Ok(page) => {
            let change_count = page.changes().len();
            let next = store
                .commit_connector_sync_page(&state, &page, |event| event.remote_ref.as_str(), now)
                .map_err(|_| "connector calendar sync page could not be committed".to_string())?;
            Ok(ConnectorSyncStep::Committed {
                revision: next.revision(),
                change_count,
                delta_committed: next.has_committed_delta() && !next.has_resume_page(),
            })
        }
        Err(failure) => handle_sync_failure(store, account, &state, failure, now),
    }
}

fn load_or_initialize_sync_state(
    store: &EventStore,
    account: &ConnectorAccount,
    capability: ConnectorCapability,
    stream_fingerprint: String,
    now: DateTime<Utc>,
) -> Result<ConnectorSyncState, String> {
    let account_generation = store
        .connector_account_sync_generation(account)
        .map_err(|_| "connector account changed before sync started".to_string())?;
    store
        .connector_sync_state(account.id, capability, &stream_fingerprint)
        .map_err(|_| "connector sync state could not be loaded".to_string())?
        .map(|state| {
            if state.account_generation() != account_generation {
                return Err("connector sync state belongs to an old account generation".to_string());
            }
            Ok(state)
        })
        .unwrap_or_else(|| {
            ConnectorSyncState::initial_with_generation(
                account.id,
                account_generation,
                capability,
                stream_fingerprint,
                now,
            )
        })
}

fn handle_sync_failure(
    store: &EventStore,
    account: &mut ConnectorAccount,
    state: &ConnectorSyncState,
    failure: ConnectorProviderFailure,
    now: DateTime<Utc>,
) -> Result<ConnectorSyncStep, String> {
    match failure {
        ConnectorProviderFailure::AuthorizationExpired
        | ConnectorProviderFailure::PermissionDenied => {
            let mut next_account = account.clone();
            next_account.health = ConnectorHealth::NeedsRepair;
            next_account.updated_at = now;
            store
                .mark_connector_account_needs_repair(&next_account, state.account_generation())
                .map_err(|_| "connector account repair state could not be persisted".to_string())?;
            *account = next_account;
            Ok(ConnectorSyncStep::RepairRequired)
        }
        ConnectorProviderFailure::CursorExpired => {
            let (next, reason) = match state.recovery(
                ConnectorSyncFailure::CursorExpired,
                state.rebuild_attempt(),
                3,
                now,
            )? {
                ConnectorSyncStateRecovery::Persist { next, reason } => (next, reason),
                _ => return Err("connector cursor recovery was invalid".to_string()),
            };
            store
                .compare_and_swap_connector_sync_state(state, &next, reason)
                .map_err(|_| "connector cursor rebuild could not be persisted".to_string())?;
            if next.stopped() {
                Ok(ConnectorSyncStep::Stopped)
            } else {
                Ok(ConnectorSyncStep::RebuildScheduled {
                    revision: next.revision(),
                })
            }
        }
        ConnectorProviderFailure::RateLimited {
            retry_after_seconds,
        } => {
            let attempt = state.retry_state().map(|retry| retry.attempt).unwrap_or(0);
            match state.recovery(
                ConnectorSyncFailure::RateLimited {
                    retry_after_seconds,
                },
                attempt,
                3,
                now,
            )? {
                ConnectorSyncStateRecovery::Persist { next, reason } => {
                    if next.stopped() {
                        store
                            .compare_and_swap_connector_sync_state(state, &next, reason)
                            .map_err(|_| {
                                "connector stopped state could not be persisted".to_string()
                            })?;
                        return Ok(ConnectorSyncStep::Stopped);
                    }
                    let retry_at = next
                        .retry_state()
                        .map(|retry| retry.retry_at)
                        .ok_or_else(|| "connector retry state is invalid".to_string())?;
                    store
                        .compare_and_swap_connector_sync_state(state, &next, reason)
                        .map_err(|_| "connector retry state could not be persisted".to_string())?;
                    Ok(ConnectorSyncStep::Deferred { retry_at })
                }
                _ => Err("connector rate-limit recovery was invalid".to_string()),
            }
        }
        ConnectorProviderFailure::NetworkUnavailable
        | ConnectorProviderFailure::RemoteNotFound
        | ConnectorProviderFailure::InvalidResponse => {
            let failure = if matches!(failure, ConnectorProviderFailure::NetworkUnavailable) {
                ConnectorSyncFailure::NetworkUnavailable
            } else {
                ConnectorSyncFailure::InvalidResponse
            };
            let attempt = state.retry_state().map(|retry| retry.attempt).unwrap_or(0);
            match state.recovery(failure, attempt, 3, now)? {
                ConnectorSyncStateRecovery::Persist { next, reason } => {
                    store
                        .compare_and_swap_connector_sync_state(state, &next, reason)
                        .map_err(|_| "connector retry state could not be persisted".to_string())?;
                    if next.stopped() {
                        Ok(ConnectorSyncStep::Stopped)
                    } else {
                        Ok(ConnectorSyncStep::Deferred {
                            retry_at: next
                                .retry_state()
                                .ok_or_else(|| "connector retry state is invalid".to_string())?
                                .retry_at,
                        })
                    }
                }
                _ => Err("connector retry recovery was invalid".to_string()),
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorRetryState {
    pub attempt: u32,
    pub max_attempts: u32,
    pub retry_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorSyncFailure {
    CursorExpired,
    RateLimited { retry_after_seconds: Option<u64> },
    AuthorizationExpired,
    NetworkUnavailable,
    InvalidResponse,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorSyncRecovery {
    RebuildCursor,
    Retry(ConnectorRetryState),
    RepairAccount,
    Stop,
}

pub fn sync_recovery(
    failure: ConnectorSyncFailure,
    attempt: u32,
    max_attempts: u32,
    now: DateTime<Utc>,
) -> ConnectorSyncRecovery {
    match failure {
        ConnectorSyncFailure::CursorExpired => {
            if attempt >= max_attempts {
                ConnectorSyncRecovery::Stop
            } else {
                ConnectorSyncRecovery::RebuildCursor
            }
        }
        ConnectorSyncFailure::AuthorizationExpired => ConnectorSyncRecovery::RepairAccount,
        ConnectorSyncFailure::NetworkUnavailable | ConnectorSyncFailure::InvalidResponse => {
            rate_limit_retry(attempt, max_attempts, None, now)
                .map(ConnectorSyncRecovery::Retry)
                .unwrap_or(ConnectorSyncRecovery::Stop)
        }
        ConnectorSyncFailure::RateLimited {
            retry_after_seconds,
        } => rate_limit_retry(attempt, max_attempts, retry_after_seconds, now)
            .map(ConnectorSyncRecovery::Retry)
            .unwrap_or(ConnectorSyncRecovery::Stop),
    }
}

pub fn rate_limit_retry(
    attempt: u32,
    max_attempts: u32,
    retry_after_seconds: Option<u64>,
    now: DateTime<Utc>,
) -> Option<ConnectorRetryState> {
    if attempt >= max_attempts {
        return None;
    }
    let exponential = 2u64.saturating_pow(attempt.min(10)).min(300);
    let delay = retry_after_seconds.unwrap_or(exponential).clamp(1, 900);
    Some(ConnectorRetryState {
        attempt: attempt + 1,
        max_attempts,
        retry_at: now + Duration::seconds(delay as i64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::connectors::ConnectorCredentialHandle;
    use crate::kernel::event_store::EventStore;
    use serde::Serialize;

    #[derive(Serialize)]
    struct ProjectionItem {
        remote_ref: String,
        payload: String,
    }

    struct FailingMailSyncProvider {
        failure: ConnectorProviderFailure,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl FailingMailSyncProvider {
        fn new(failure: ConnectorProviderFailure) -> Self {
            Self {
                failure,
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl MailSyncProvider for FailingMailSyncProvider {
        fn sync_mail_page(
            &self,
            _account: &ConnectorAccount,
            _request: &MailSyncRequest,
            _continuation: Option<&ConnectorOpaqueContinuation>,
        ) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(self.failure.clone())
        }
    }

    struct FailingCalendarSyncProvider {
        failure: ConnectorProviderFailure,
    }

    impl CalendarSyncProvider for FailingCalendarSyncProvider {
        fn sync_calendar_page(
            &self,
            _account: &ConnectorAccount,
            _request: &CalendarSyncRequest,
            _continuation: Option<&ConnectorOpaqueContinuation>,
        ) -> ConnectorProviderResult<ConnectorSyncPage<CalendarEvent>> {
            Err(self.failure.clone())
        }
    }

    #[derive(Default)]
    struct SuccessfulMailSyncProvider {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl MailSyncProvider for SuccessfulMailSyncProvider {
        fn sync_mail_page(
            &self,
            _account: &ConnectorAccount,
            _request: &MailSyncRequest,
            _continuation: Option<&ConnectorOpaqueContinuation>,
        ) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ConnectorSyncPage::new(
                Vec::new(),
                ConnectorSyncContinuation::Delta(
                    ConnectorOpaqueContinuation::new("mail-delta".to_string())
                        .expect("mail delta builds"),
                ),
            ))
        }
    }

    #[derive(Default)]
    struct SuccessfulCalendarSyncProvider {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl CalendarSyncProvider for SuccessfulCalendarSyncProvider {
        fn sync_calendar_page(
            &self,
            _account: &ConnectorAccount,
            _request: &CalendarSyncRequest,
            _continuation: Option<&ConnectorOpaqueContinuation>,
        ) -> ConnectorProviderResult<ConnectorSyncPage<CalendarEvent>> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(ConnectorSyncPage::new(
                Vec::new(),
                ConnectorSyncContinuation::Delta(
                    ConnectorOpaqueContinuation::new("calendar-delta".to_string())
                        .expect("calendar delta builds"),
                ),
            ))
        }
    }

    struct TestSyncRegistry {
        provider_key: String,
        enabled: bool,
        mail: SuccessfulMailSyncProvider,
        calendar: SuccessfulCalendarSyncProvider,
        mail_lookups: std::sync::atomic::AtomicUsize,
        calendar_lookups: std::sync::atomic::AtomicUsize,
    }

    impl TestSyncRegistry {
        fn enabled(provider_key: &str) -> Self {
            Self {
                provider_key: provider_key.to_string(),
                enabled: true,
                mail: SuccessfulMailSyncProvider::default(),
                calendar: SuccessfulCalendarSyncProvider::default(),
                mail_lookups: std::sync::atomic::AtomicUsize::new(0),
                calendar_lookups: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn disabled() -> Self {
            Self {
                provider_key: "disabled".to_string(),
                enabled: false,
                mail: SuccessfulMailSyncProvider::default(),
                calendar: SuccessfulCalendarSyncProvider::default(),
                mail_lookups: std::sync::atomic::AtomicUsize::new(0),
                calendar_lookups: std::sync::atomic::AtomicUsize::new(0),
            }
        }
    }

    impl ConnectorSyncRegistry for TestSyncRegistry {
        fn mail_provider(&self, provider_key: &str) -> Option<&dyn MailSyncProvider> {
            self.mail_lookups
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            (provider_key == self.provider_key).then_some(&self.mail)
        }

        fn calendar_provider(&self, provider_key: &str) -> Option<&dyn CalendarSyncProvider> {
            self.calendar_lookups
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            (provider_key == self.provider_key).then_some(&self.calendar)
        }

        fn execution_enabled(&self) -> bool {
            self.enabled
        }
    }

    struct BlockingMailSyncProvider {
        entered: std::sync::mpsc::SyncSender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl MailSyncProvider for BlockingMailSyncProvider {
        fn sync_mail_page(
            &self,
            _account: &ConnectorAccount,
            _request: &MailSyncRequest,
            _continuation: Option<&ConnectorOpaqueContinuation>,
        ) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>> {
            self.entered.send(()).expect("test observes provider entry");
            self.release
                .lock()
                .expect("release channel locks")
                .recv()
                .expect("test releases provider");
            Ok(ConnectorSyncPage::new(
                Vec::new(),
                ConnectorSyncContinuation::Delta(
                    ConnectorOpaqueContinuation::new("blocked-mail-delta".to_string())
                        .expect("delta builds"),
                ),
            ))
        }
    }

    struct BlockingSyncRegistry {
        provider: BlockingMailSyncProvider,
    }

    impl ConnectorSyncRegistry for BlockingSyncRegistry {
        fn mail_provider(&self, provider_key: &str) -> Option<&dyn MailSyncProvider> {
            (provider_key == "fake-a").then_some(&self.provider)
        }

        fn calendar_provider(&self, _provider_key: &str) -> Option<&dyn CalendarSyncProvider> {
            None
        }

        fn execution_enabled(&self) -> bool {
            true
        }
    }

    struct TakeoverMailSyncProvider {
        calls: std::sync::atomic::AtomicUsize,
        first_entered: std::sync::mpsc::SyncSender<()>,
        release_first: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl MailSyncProvider for TakeoverMailSyncProvider {
        fn sync_mail_page(
            &self,
            _account: &ConnectorAccount,
            _request: &MailSyncRequest,
            _continuation: Option<&ConnectorOpaqueContinuation>,
        ) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>> {
            let call = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if call == 0 {
                self.first_entered
                    .send(())
                    .expect("test observes first provider entry");
                self.release_first
                    .lock()
                    .expect("release channel locks")
                    .recv()
                    .expect("test releases first provider call");
            }
            Ok(ConnectorSyncPage::new(
                Vec::new(),
                ConnectorSyncContinuation::Delta(
                    ConnectorOpaqueContinuation::new("takeover-delta".to_string())
                        .expect("delta builds"),
                ),
            ))
        }
    }

    struct TakeoverSyncRegistry {
        provider: TakeoverMailSyncProvider,
    }

    impl ConnectorSyncRegistry for TakeoverSyncRegistry {
        fn mail_provider(&self, provider_key: &str) -> Option<&dyn MailSyncProvider> {
            (provider_key == "fake-a").then_some(&self.provider)
        }

        fn calendar_provider(&self, _provider_key: &str) -> Option<&dyn CalendarSyncProvider> {
            None
        }

        fn execution_enabled(&self) -> bool {
            true
        }
    }

    fn sync_account() -> ConnectorAccount {
        let now = Utc::now();
        ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            display_name: "Sync account".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailSyncInbox],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        }
    }

    fn queue_stopped_recovery(
        store: &EventStore,
        account: &ConnectorAccount,
        capability: ConnectorCapability,
        stream_fingerprint: String,
        plan: ConnectorSyncPlan,
        registry: &dyn ConnectorSyncRegistry,
        now: DateTime<Utc>,
    ) {
        let generation = store
            .connector_account_sync_generation(account)
            .expect("account generation reads");
        let initial = ConnectorSyncState::initial_with_generation(
            account.id,
            generation,
            capability,
            stream_fingerprint,
            now,
        )
        .expect("sync state starts");
        store
            .record_connector_sync_plan(
                &initial,
                &plan.persistence_json().expect("sync plan serializes"),
            )
            .expect("sync plan persists");
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
            ConnectorSyncStateRecovery::RepairAccount => {
                panic!("invalid response exhaustion stops the stream")
            }
        };
        store
            .compare_and_swap_connector_sync_state(&initial, &stopped, reason)
            .expect("stopped state persists");
        let item = store
            .list_connector_recovery_items_with_runtime_registries(
                &crate::kernel::connectors::reconciliation::EmptyConnectorReconcilerRegistry,
                registry,
            )
            .expect("Recovery items load")
            .into_iter()
            .find(|item| {
                item.sync_capability.is_some_and(|item_capability| {
                    matches!(
                        (item_capability, capability),
                        (
                            crate::kernel::connectors::ConnectorRecoverySyncCapability::Mail,
                            ConnectorCapability::MailSyncInbox
                        ) | (
                            crate::kernel::connectors::ConnectorRecoverySyncCapability::Calendar,
                            ConnectorCapability::CalendarSyncEvents
                        )
                    )
                })
            })
            .expect("stopped stream has a Recovery item");
        let action_revision = match item.action {
            Some(crate::kernel::connectors::ConnectorRecoveryAction::ResumeSync {
                action_revision,
            }) => action_revision,
            _ => panic!("enabled registry exposes an exact Resume action"),
        };
        assert_eq!(
            store
                .resume_connector_read_sync_from_recovery_with_sync_registry(
                    item.id,
                    &action_revision,
                    registry,
                    now + Duration::seconds(2),
                )
                .expect("Recovery action is accepted"),
            crate::kernel::connectors::ConnectorRecoveryAcceptance::Accepted
        );
    }

    #[test]
    fn shared_sync_recovery_worker_is_typed_exact_keyed_and_empty_registry_quiet() {
        let now = Utc::now() - Duration::minutes(1);
        let mut account = sync_account();
        account.provider_id = "fake-a".to_string();
        account.granted_capabilities = vec![
            ConnectorCapability::MailSyncInbox,
            ConnectorCapability::CalendarSyncEvents,
        ];
        let store = EventStore::open_memory().expect("store opens");
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let registry = TestSyncRegistry::enabled("fake-a");
        let mail_request = MailSyncRequest::inbox(25).expect("mail request builds");
        queue_stopped_recovery(
            &store,
            &account,
            ConnectorCapability::MailSyncInbox,
            mail_request.stream_fingerprint(&account.provider_id),
            ConnectorSyncPlan::mail(&mail_request),
            &registry,
            now,
        );
        let calendar_request =
            CalendarSyncRequest::new(now - Duration::days(1), now + Duration::days(1), 25)
                .expect("calendar request builds");
        queue_stopped_recovery(
            &store,
            &account,
            ConnectorCapability::CalendarSyncEvents,
            calendar_request.stream_fingerprint(&account.provider_id),
            ConnectorSyncPlan::calendar(&calendar_request),
            &registry,
            now + Duration::seconds(3),
        );
        let shared = Arc::new(Mutex::new(store));
        let sweep = run_connector_sync_recovery_with_shared_store(&shared, &registry, 8)
            .expect("worker completes queued Mail and Calendar Recovery");
        assert_eq!(sweep.claimed, 2);
        assert_eq!(sweep.completed, 2);
        assert_eq!(sweep.deferred, 0);
        assert_eq!(sweep.repair_required, 0);
        assert_eq!(sweep.lost_claim, 0);
        assert_eq!(
            registry
                .mail
                .calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            registry
                .calendar
                .calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert!(
            registry
                .mail_lookups
                .load(std::sync::atomic::Ordering::SeqCst)
                >= 1
        );
        assert!(
            registry
                .calendar_lookups
                .load(std::sync::atomic::Ordering::SeqCst)
                >= 1
        );

        let disabled = TestSyncRegistry::disabled();
        let guard = shared.lock().expect("store locks");
        assert_eq!(
            run_connector_sync_recovery_with_shared_store(&shared, &disabled, 8)
                .expect("disabled worker returns without locking"),
            ConnectorSyncRecoverySweep::default()
        );
        drop(guard);
        assert_eq!(
            disabled
                .mail_lookups
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
        assert_eq!(
            disabled
                .calendar_lookups
                .load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }

    #[test]
    fn shared_sync_recovery_worker_releases_store_and_discards_authority_tamper() {
        let now = Utc::now() - Duration::minutes(1);
        let mut account = sync_account();
        account.provider_id = "fake-a".to_string();
        let store = EventStore::open_memory().expect("store opens");
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let registry = Arc::new(BlockingSyncRegistry {
            provider: BlockingMailSyncProvider {
                entered: entered_tx,
                release: Mutex::new(release_rx),
            },
        });
        let request = MailSyncRequest::inbox(25).expect("mail request builds");
        queue_stopped_recovery(
            &store,
            &account,
            ConnectorCapability::MailSyncInbox,
            request.stream_fingerprint(&account.provider_id),
            ConnectorSyncPlan::mail(&request),
            registry.as_ref(),
            now,
        );
        let shared = Arc::new(Mutex::new(store));
        let worker_store = Arc::clone(&shared);
        let worker_registry = Arc::clone(&registry);
        let worker = std::thread::spawn(move || {
            run_connector_sync_recovery_with_shared_store(
                &worker_store,
                worker_registry.as_ref(),
                1,
            )
            .expect("worker returns")
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("provider is entered");
        let store_guard = shared
            .try_lock()
            .expect("EventStore lock is released during provider I/O");
        let mut tampered = account.clone();
        tampered.provider_id = "fake-b".to_string();
        tampered.updated_at = Utc::now();
        store_guard
            .upsert_connector_account(&tampered)
            .expect("authority tamper persists while provider is blocked");
        drop(store_guard);
        release_tx.send(()).expect("provider is released");
        let sweep = worker.join().expect("worker joins");
        assert_eq!(sweep.claimed, 1);
        assert_eq!(sweep.completed, 0);
        assert_eq!(sweep.deferred, 0);
        assert_eq!(sweep.repair_required, 0);
        assert_eq!(sweep.lost_claim, 1);
    }

    #[test]
    fn shared_sync_recovery_worker_relinquishes_missing_provider_without_retry_churn() {
        let now = Utc::now() - Duration::minutes(1);
        let mut account = sync_account();
        account.provider_id = "fake-a".to_string();
        let store = EventStore::open_memory().expect("store opens");
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let available = TestSyncRegistry::enabled("fake-a");
        let request = MailSyncRequest::inbox(25).expect("mail request builds");
        queue_stopped_recovery(
            &store,
            &account,
            ConnectorCapability::MailSyncInbox,
            request.stream_fingerprint(&account.provider_id),
            ConnectorSyncPlan::mail(&request),
            &available,
            now,
        );
        let shared = Arc::new(Mutex::new(store));
        let missing = TestSyncRegistry::enabled("different-provider");
        let first = run_connector_sync_recovery_with_shared_store(&shared, &missing, 1)
            .expect("missing provider is safely relinquished");
        assert_eq!(first.claimed, 1);
        assert_eq!(first.deferred, 1);
        assert_eq!(first.completed, 0);
        assert_eq!(first.repair_required, 0);
        assert_eq!(first.lost_claim, 0);
        let second = run_connector_sync_recovery_with_shared_store(&shared, &missing, 1)
            .expect("relinquished claim observes bounded delay");
        assert_eq!(second, ConnectorSyncRecoverySweep::default());
        assert_eq!(
            missing.mail.calls.load(std::sync::atomic::Ordering::SeqCst),
            0
        );
    }

    #[test]
    fn shared_sync_recovery_worker_discards_old_result_after_expiry_takeover() {
        let now = Utc::now() - Duration::minutes(1);
        let mut account = sync_account();
        account.provider_id = "fake-a".to_string();
        let store = EventStore::open_memory().expect("store opens");
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let registry = Arc::new(TakeoverSyncRegistry {
            provider: TakeoverMailSyncProvider {
                calls: std::sync::atomic::AtomicUsize::new(0),
                first_entered: entered_tx,
                release_first: Mutex::new(release_rx),
            },
        });
        let request = MailSyncRequest::inbox(25).expect("mail request builds");
        queue_stopped_recovery(
            &store,
            &account,
            ConnectorCapability::MailSyncInbox,
            request.stream_fingerprint(&account.provider_id),
            ConnectorSyncPlan::mail(&request),
            registry.as_ref(),
            now,
        );
        let shared = Arc::new(Mutex::new(store));
        let first_store = Arc::clone(&shared);
        let first_registry = Arc::clone(&registry);
        let first_worker = std::thread::spawn(move || {
            run_connector_sync_recovery_with_shared_store(&first_store, first_registry.as_ref(), 1)
                .expect("first worker returns")
        });
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("first provider call blocks");

        let takeover_at = Utc::now() + Duration::hours(1);
        let mut takeover = shared
            .try_lock()
            .expect("Store is free during first provider call")
            .claim_due_connector_sync_recovery_jobs(takeover_at, 1)
            .expect("expired claim is taken over");
        assert_eq!(takeover.len(), 1);
        assert_eq!(takeover[0].attempt_count(), 2);
        let takeover_page = ConnectorSyncPage::new(
            Vec::new(),
            ConnectorSyncContinuation::Delta(
                ConnectorOpaqueContinuation::new("takeover-winner-delta".to_string())
                    .expect("winner delta builds"),
            ),
        );
        shared
            .lock()
            .expect("Store locks for winner finalization")
            .complete_claimed_mail_sync_recovery(
                &takeover.remove(0),
                &takeover_page,
                takeover_at + Duration::seconds(1),
            )
            .expect("takeover winner completes");

        release_tx.send(()).expect("first provider call releases");
        let first = first_worker.join().expect("first worker joins");
        assert_eq!(first.claimed, 1);
        assert_eq!(first.completed, 0);
        assert_eq!(first.lost_claim, 1);
        assert_eq!(
            registry
                .provider
                .calls
                .load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert!(shared
            .lock()
            .expect("Store locks after takeover")
            .claim_due_connector_sync_recovery_jobs(takeover_at + Duration::seconds(2), 1,)
            .expect("completed job is not due")
            .is_empty());
    }

    #[test]
    fn sync_failures_choose_bounded_user_repair_paths() {
        let now = Utc::now();
        assert_eq!(
            sync_recovery(ConnectorSyncFailure::CursorExpired, 0, 3, now),
            ConnectorSyncRecovery::RebuildCursor
        );
        assert_eq!(
            sync_recovery(ConnectorSyncFailure::AuthorizationExpired, 0, 3, now),
            ConnectorSyncRecovery::RepairAccount
        );
        assert_eq!(
            sync_recovery(
                ConnectorSyncFailure::RateLimited {
                    retry_after_seconds: None
                },
                3,
                3,
                now
            ),
            ConnectorSyncRecovery::Stop
        );
        assert!(matches!(
            sync_recovery(ConnectorSyncFailure::NetworkUnavailable, 0, 3, now),
            ConnectorSyncRecovery::Retry(_)
        ));
        assert_eq!(
            sync_recovery(ConnectorSyncFailure::InvalidResponse, 3, 3, now),
            ConnectorSyncRecovery::Stop
        );
        assert_eq!(
            sync_recovery(ConnectorSyncFailure::CursorExpired, 3, 3, now),
            ConnectorSyncRecovery::Stop
        );
    }

    #[test]
    fn durable_delta_retry_and_rebuild_use_revision_cas_without_token_events() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("durable-delta.sqlite3");
        let now = Utc::now();
        let account_id = Uuid::new_v4();
        let mut persisted_account = sync_account();
        persisted_account.id = account_id;
        let stream = MailSyncRequest::inbox(10)
            .expect("request builds")
            .stream_fingerprint("microsoft");
        let initial = ConnectorSyncState::initial(
            account_id,
            ConnectorCapability::MailSyncInbox,
            stream.clone(),
            now,
        )
        .expect("state starts");
        let marker = "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$skiptoken=marker-retry-secret";
        let page = ConnectorSyncPage::new(
            vec![ConnectorSyncChange::Upsert("message-1".to_string())],
            ConnectorSyncContinuation::Next(
                ConnectorOpaqueContinuation::new(marker.to_string()).expect("continuation builds"),
            ),
        );
        let first = {
            let store = EventStore::open(&path).expect("store opens");
            store
                .upsert_connector_account(&persisted_account)
                .expect("account persists");
            store
                .commit_connector_sync_page(&initial, &page, |item| item.as_str(), now)
                .expect("first page commits")
        };
        let store = EventStore::open(&path).expect("store reopens");
        let (retry, retry_reason) = match first
            .recovery(
                ConnectorSyncFailure::RateLimited {
                    retry_after_seconds: Some(120),
                },
                0,
                3,
                now,
            )
            .expect("retry recovery builds")
        {
            ConnectorSyncStateRecovery::Persist { next, reason } => (next, reason),
            _ => panic!("rate limit must persist retry"),
        };
        store
            .compare_and_swap_connector_sync_state(&first, &retry, retry_reason)
            .expect("retry state persists");
        assert!(!retry.ready(now + Duration::seconds(119)));
        assert!(retry.ready(now + Duration::seconds(120)));
        assert!(store
            .compare_and_swap_connector_sync_state(&first, &retry, retry_reason)
            .is_err());

        let loaded = store
            .connector_sync_state(account_id, ConnectorCapability::MailSyncInbox, &stream)
            .expect("state reads")
            .expect("state exists");
        assert!(loaded == retry);
        assert!(loaded.locator().is_some());
        let (rebuilt, rebuild_reason) = match loaded
            .recovery(ConnectorSyncFailure::CursorExpired, 0, 3, now)
            .expect("rebuild recovery builds")
        {
            ConnectorSyncStateRecovery::Persist { next, reason } => (next, reason),
            _ => panic!("expired cursor must rebuild"),
        };
        store
            .compare_and_swap_connector_sync_state(&loaded, &rebuilt, rebuild_reason)
            .expect("rebuild state persists");
        assert!(rebuilt.locator().is_none());
        assert!(store
            .connector_sync_projection_summaries(
                account_id,
                ConnectorCapability::MailSyncInbox,
                &stream,
            )
            .expect("projection summaries read")
            .is_empty());
        assert!(store
            .connector_sync_state(
                account_id,
                ConnectorCapability::MailSyncInbox,
                "sha256:different-stream"
            )
            .expect("other stream query succeeds")
            .is_none());
        let events = serde_json::to_string(&store.list_recent(20).expect("events read"))
            .expect("events serialize");
        assert!(!events.contains("marker-retry-secret"));
        assert!(!events.contains(&account_id.to_string()));
        assert!(!events.contains(&stream));
        assert!(!events.contains(&retry.retry_state().unwrap().retry_at.to_rfc3339()));
        assert!(!events.contains("account_generation"));
        assert!(!events.contains("stream_fingerprint"));
        assert!(!events.contains("rebuild_attempt"));
        assert!(!events.contains("retry_at"));
    }

    #[test]
    fn sync_runner_persists_backoff_exhaustion_and_account_repair_across_restart() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let retry_path = temp_dir.path().join("retry-runner.sqlite3");
        let request = MailSyncRequest::inbox(10).expect("request builds");
        let mut account = sync_account();
        let provider = FailingMailSyncProvider::new(ConnectorProviderFailure::RateLimited {
            retry_after_seconds: Some(1),
        });
        let mut now = Utc::now();
        {
            let store = EventStore::open(&retry_path).expect("store opens");
            store
                .upsert_connector_account(&account)
                .expect("account persists");
            for expected_attempt in 1..=3 {
                let step = run_mail_sync_step(&store, &provider, &mut account, &request, now)
                    .expect("rate-limited step persists");
                let retry_at = match step {
                    ConnectorSyncStep::Deferred { retry_at } => retry_at,
                    _ => panic!("rate-limited step must defer"),
                };
                assert_eq!(provider.calls(), expected_attempt);
                let early = run_mail_sync_step(
                    &store,
                    &provider,
                    &mut account,
                    &request,
                    retry_at - Duration::milliseconds(1),
                )
                .expect("early restart remains deferred");
                assert!(matches!(early, ConnectorSyncStep::Deferred { .. }));
                assert_eq!(provider.calls(), expected_attempt);
                now = retry_at;
            }
            assert_eq!(
                run_mail_sync_step(&store, &provider, &mut account, &request, now)
                    .expect("retry exhaustion persists"),
                ConnectorSyncStep::Stopped
            );
            assert_eq!(provider.calls(), 4);
        }
        let store = EventStore::open(&retry_path).expect("store reopens");
        assert_eq!(
            run_mail_sync_step(
                &store,
                &provider,
                &mut account,
                &request,
                now + Duration::seconds(10),
            )
            .expect("stopped state reloads"),
            ConnectorSyncStep::Stopped
        );
        assert_eq!(provider.calls(), 4);

        let repair_path = temp_dir.path().join("repair-runner.sqlite3");
        let mut repair_account = sync_account();
        let expired = FailingMailSyncProvider::new(ConnectorProviderFailure::AuthorizationExpired);
        {
            let store = EventStore::open(&repair_path).expect("repair store opens");
            store
                .upsert_connector_account(&repair_account)
                .expect("repair account persists");
            assert_eq!(
                run_mail_sync_step(&store, &expired, &mut repair_account, &request, now,)
                    .expect("authorization failure persists"),
                ConnectorSyncStep::RepairRequired
            );
            assert_eq!(repair_account.health, ConnectorHealth::NeedsRepair);
            assert_eq!(expired.calls(), 1);
        }
        let store = EventStore::open(&repair_path).expect("repair store reopens");
        let mut recovered = store
            .list_connector_accounts()
            .expect("accounts read")
            .into_iter()
            .find(|item| item.id == repair_account.id)
            .expect("repair account persists");
        let untouched = FailingMailSyncProvider::new(ConnectorProviderFailure::NetworkUnavailable);
        assert_eq!(
            run_mail_sync_step(&store, &untouched, &mut recovered, &request, now)
                .expect("repair state blocks provider"),
            ConnectorSyncStep::RepairRequired
        );
        assert_eq!(untouched.calls(), 0);
    }

    #[test]
    fn sync_runner_persists_cursor_expiry_rebuild() {
        let store = EventStore::open_memory().expect("store opens");
        let request = MailSyncRequest::inbox(10).expect("request builds");
        let mut account = sync_account();
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let provider = FailingMailSyncProvider::new(ConnectorProviderFailure::CursorExpired);
        let step = run_mail_sync_step(&store, &provider, &mut account, &request, Utc::now())
            .expect("cursor rebuild persists");
        assert!(matches!(
            step,
            ConnectorSyncStep::RebuildScheduled { revision: 1 }
        ));
        let stream = request.stream_fingerprint(&account.provider_id);
        let state = store
            .connector_sync_state(account.id, ConnectorCapability::MailSyncInbox, &stream)
            .expect("state reads")
            .expect("state persists");
        assert!(state.locator().is_none());
    }

    #[test]
    fn sync_requires_separate_durable_consent_before_provider_access() {
        let store = EventStore::open_memory().expect("store opens");
        let request = MailSyncRequest::inbox(10).expect("request builds");
        let mut account = sync_account();
        account.granted_capabilities = vec![ConnectorCapability::MailSearch];
        let provider = FailingMailSyncProvider::new(ConnectorProviderFailure::NetworkUnavailable);
        assert!(run_mail_sync_step(&store, &provider, &mut account, &request, Utc::now()).is_err());
        assert_eq!(provider.calls(), 0);
    }

    #[test]
    fn network_retry_and_cursor_rebuild_budgets_survive_restart() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let request = MailSyncRequest::inbox(10).expect("request builds");
        let mut account = sync_account();
        let network = FailingMailSyncProvider::new(ConnectorProviderFailure::NetworkUnavailable);
        let path = temp_dir.path().join("network-retry.sqlite3");
        let retry_at = {
            let store = EventStore::open(&path).expect("store opens");
            store
                .upsert_connector_account(&account)
                .expect("account persists");
            match run_mail_sync_step(&store, &network, &mut account, &request, Utc::now())
                .expect("network retry persists")
            {
                ConnectorSyncStep::Deferred { retry_at } => retry_at,
                _ => panic!("network failure must defer"),
            }
        };
        let store = EventStore::open(&path).expect("store reopens");
        assert!(matches!(
            run_mail_sync_step(
                &store,
                &network,
                &mut account,
                &request,
                retry_at - Duration::milliseconds(1),
            )
            .expect("durable retry reloads"),
            ConnectorSyncStep::Deferred { .. }
        ));
        assert_eq!(network.calls(), 1);

        let rebuild_path = temp_dir.path().join("cursor-rebuild.sqlite3");
        let expired = FailingMailSyncProvider::new(ConnectorProviderFailure::CursorExpired);
        let mut rebuild_account = sync_account();
        {
            let rebuild_store = EventStore::open(&rebuild_path).expect("rebuild store opens");
            rebuild_store
                .upsert_connector_account(&rebuild_account)
                .expect("rebuild account persists");
            for _ in 0..3 {
                assert!(matches!(
                    run_mail_sync_step(
                        &rebuild_store,
                        &expired,
                        &mut rebuild_account,
                        &request,
                        Utc::now(),
                    )
                    .expect("cursor rebuild persists"),
                    ConnectorSyncStep::RebuildScheduled { .. }
                ));
            }
        }
        let rebuild_store = EventStore::open(&rebuild_path).expect("rebuild store reopens");
        assert_eq!(
            run_mail_sync_step(
                &rebuild_store,
                &expired,
                &mut rebuild_account,
                &request,
                Utc::now(),
            )
            .expect("cursor exhaustion persists"),
            ConnectorSyncStep::Stopped
        );
        assert_eq!(expired.calls(), 4);
        assert_eq!(
            run_mail_sync_step(
                &rebuild_store,
                &expired,
                &mut rebuild_account,
                &request,
                Utc::now(),
            )
            .expect("stopped cursor state reloads"),
            ConnectorSyncStep::Stopped
        );
        assert_eq!(expired.calls(), 4);
    }

    #[test]
    fn sync_projection_retention_prevents_an_unbounded_local_mirror() {
        let store = EventStore::open_memory().expect("store opens");
        let now = Utc::now();
        let account_id = Uuid::new_v4();
        let mut account = sync_account();
        account.id = account_id;
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let stream = "sha256:bounded-projection".to_string();
        let mut state = ConnectorSyncState::initial(
            account_id,
            ConnectorCapability::MailSearch,
            stream.clone(),
            now,
        )
        .expect("state starts");
        for batch in 0..6 {
            let items = (0..100)
                .map(|offset| ConnectorSyncChange::Upsert(format!("item-{}", batch * 100 + offset)))
                .collect::<Vec<_>>();
            let continuation = if batch == 5 {
                ConnectorSyncContinuation::Delta(
                    ConnectorOpaqueContinuation::new(format!("delta-{batch}"))
                        .expect("delta builds"),
                )
            } else {
                ConnectorSyncContinuation::Next(
                    ConnectorOpaqueContinuation::new(format!("page-{batch}")).expect("page builds"),
                )
            };
            let page = ConnectorSyncPage::new(items, continuation);
            state = store
                .commit_connector_sync_page(
                    &state,
                    &page,
                    |item| item.as_str(),
                    now + Duration::seconds(batch as i64),
                )
                .expect("bounded page commits");
        }
        let summaries = store
            .connector_sync_projection_summaries(
                account_id,
                ConnectorCapability::MailSearch,
                &stream,
            )
            .expect("projection summaries read");
        assert_eq!(summaries.len(), MAX_SYNC_PROJECTION_ITEMS_PER_STREAM);
        assert!(!summaries.iter().any(|item| item.remote_ref == "item-0"));
        assert!(summaries.iter().any(|item| item.remote_ref == "item-599"));
    }

    #[test]
    fn sync_projection_has_account_stream_and_byte_budgets() {
        let store = EventStore::open_memory().expect("store opens");
        let now = Utc::now();
        let account_id = Uuid::new_v4();
        let mut account = sync_account();
        account.id = account_id;
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        for offset in 0..=MAX_SYNC_STREAMS_PER_ACCOUNT {
            let stream = format!("sha256:stream-{offset}");
            let state = ConnectorSyncState::initial(
                account_id,
                ConnectorCapability::MailSyncInbox,
                stream,
                now + Duration::seconds(offset as i64),
            )
            .expect("state starts");
            let page = ConnectorSyncPage::new(
                vec![ConnectorSyncChange::Upsert(ProjectionItem {
                    remote_ref: format!("stream-item-{offset}"),
                    payload: "bounded".to_string(),
                })],
                ConnectorSyncContinuation::Delta(
                    ConnectorOpaqueContinuation::new(format!("delta-{offset}"))
                        .expect("continuation builds"),
                ),
            );
            store
                .commit_connector_sync_page(
                    &state,
                    &page,
                    |item| item.remote_ref.as_str(),
                    now + Duration::seconds(offset as i64),
                )
                .expect("stream page commits");
        }
        assert!(store
            .connector_sync_state(
                account_id,
                ConnectorCapability::MailSyncInbox,
                "sha256:stream-0",
            )
            .expect("state query succeeds")
            .is_none());

        let stream = "sha256:byte-budget".to_string();
        let state = ConnectorSyncState::initial(
            account_id,
            ConnectorCapability::MailSyncInbox,
            stream.clone(),
            now + Duration::hours(1),
        )
        .expect("state starts");
        let page = ConnectorSyncPage::new(
            (0..70)
                .map(|index| {
                    ConnectorSyncChange::Upsert(ProjectionItem {
                        remote_ref: format!("large-{index}"),
                        payload: "x".repeat(31_000),
                    })
                })
                .collect(),
            ConnectorSyncContinuation::Delta(
                ConnectorOpaqueContinuation::new("delta-byte-budget".to_string())
                    .expect("continuation builds"),
            ),
        );
        store
            .commit_connector_sync_page(
                &state,
                &page,
                |item| item.remote_ref.as_str(),
                now + Duration::hours(1),
            )
            .expect("large page commits within per-item budget");
        let summaries = store
            .connector_sync_projection_summaries(
                account_id,
                ConnectorCapability::MailSyncInbox,
                &stream,
            )
            .expect("projection summaries read");
        assert!(!summaries.is_empty());
        assert!(summaries.len() < 70);
        let disconnect = store
            .begin_connector_disconnect(account_id, now + Duration::hours(2))
            .expect("disconnect begins atomically");
        store
            .complete_connector_disconnect(
                &disconnect,
                super::super::ConnectorDisconnectSource::User,
                super::super::ConnectorCredentialDeleteOutcome::Deleted,
                now + Duration::hours(2),
            )
            .expect("disconnect completes");
        assert!(store
            .connector_sync_state(account_id, ConnectorCapability::MailSyncInbox, &stream,)
            .expect("state query succeeds")
            .is_none());
        assert!(store
            .connector_sync_projection_summaries(
                account_id,
                ConnectorCapability::MailSyncInbox,
                &stream,
            )
            .expect("projection summaries read")
            .is_empty());
    }

    #[test]
    fn disconnect_generation_blocks_stale_page_and_repair_commits() {
        let store = EventStore::open_memory().expect("store opens");
        let now = Utc::now();
        let account = sync_account();
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let generation = store
            .connector_account_sync_generation(&account)
            .expect("generation reads");
        let stream = "sha256:inflight-before-disconnect".to_string();
        let stale_state = ConnectorSyncState::initial_with_generation(
            account.id,
            generation,
            ConnectorCapability::MailSyncInbox,
            stream.clone(),
            now,
        )
        .expect("state starts");

        let disconnect = store
            .begin_connector_disconnect(account.id, now + Duration::seconds(1))
            .expect("disconnect invalidates generation");

        let page = ConnectorSyncPage::new(
            vec![ConnectorSyncChange::Upsert(ProjectionItem {
                remote_ref: "late-message".to_string(),
                payload: "must not return".to_string(),
            })],
            ConnectorSyncContinuation::Delta(
                ConnectorOpaqueContinuation::new("late-delta".to_string())
                    .expect("continuation builds"),
            ),
        );
        assert!(store
            .commit_connector_sync_page(
                &stale_state,
                &page,
                |item| item.remote_ref.as_str(),
                now + Duration::seconds(2),
            )
            .is_err());
        assert!(store
            .connector_sync_state(account.id, ConnectorCapability::MailSyncInbox, &stream,)
            .expect("state query succeeds")
            .is_none());

        let mut stale_repair = account.clone();
        stale_repair.health = ConnectorHealth::NeedsRepair;
        stale_repair.updated_at = now + Duration::seconds(2);
        assert!(store
            .mark_connector_account_needs_repair(&stale_repair, generation)
            .is_err());
        store
            .complete_connector_disconnect(
                &disconnect,
                super::super::ConnectorDisconnectSource::User,
                super::super::ConnectorCredentialDeleteOutcome::Deleted,
                now + Duration::seconds(3),
            )
            .expect("disconnect completes after stale work is rejected");
        let persisted = store
            .list_connector_accounts()
            .expect("accounts read")
            .into_iter()
            .find(|item| item.id == account.id)
            .expect("account remains");
        assert_eq!(persisted.health, ConnectorHealth::Disconnected);
    }

    #[test]
    fn failed_sync_streams_share_the_account_lru_budget() {
        let store = EventStore::open_memory().expect("store opens");
        let now = Utc::now();
        let account = sync_account();
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let generation = store
            .connector_account_sync_generation(&account)
            .expect("generation reads");
        for offset in 0..=MAX_SYNC_STREAMS_PER_ACCOUNT {
            let stream = format!("sha256:failed-stream-{offset}");
            let state = ConnectorSyncState::initial_with_generation(
                account.id,
                generation,
                ConnectorCapability::CalendarSyncEvents,
                stream,
                now + Duration::seconds(offset as i64),
            )
            .expect("state starts");
            let (next, reason) = match state
                .recovery(
                    ConnectorSyncFailure::NetworkUnavailable,
                    0,
                    3,
                    now + Duration::seconds(offset as i64),
                )
                .expect("retry builds")
            {
                ConnectorSyncStateRecovery::Persist { next, reason } => (next, reason),
                _ => panic!("network recovery must persist"),
            };
            store
                .compare_and_swap_connector_sync_state(&state, &next, reason)
                .expect("failed stream state persists");
        }
        assert!(store
            .connector_sync_state(
                account.id,
                ConnectorCapability::CalendarSyncEvents,
                "sha256:failed-stream-0",
            )
            .expect("oldest state query succeeds")
            .is_none());
        assert!(store
            .connector_sync_state(
                account.id,
                ConnectorCapability::CalendarSyncEvents,
                &format!("sha256:failed-stream-{MAX_SYNC_STREAMS_PER_ACCOUNT}"),
            )
            .expect("latest state query succeeds")
            .is_some());
    }
}
