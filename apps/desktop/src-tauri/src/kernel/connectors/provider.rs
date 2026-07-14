use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::domain::{CalendarEvent, MailAddress, MailThread};
use super::ConnectorAccount;

pub const MAX_MAIL_SEARCH_RESULTS: u16 = 25;
pub const MAX_THREAD_MESSAGES: u16 = 50;
pub const MAX_CALENDAR_EVENTS: u16 = 50;
pub const MAX_MAIL_SEARCH_QUERY_CHARS: usize = 512;
pub const MAX_REMOTE_REFERENCE_CHARS: usize = 1024;
pub const MAX_READ_CONTINUATION_CHARS: usize = 8192;
pub const MAX_READ_PAGES: usize = 20;

#[derive(Clone, Eq, PartialEq)]
pub struct ConnectorReadContinuation(String);

impl ConnectorReadContinuation {
    pub(crate) fn new(mut value: String) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.chars().count() > MAX_READ_CONTINUATION_CHARS {
            value.zeroize();
            return Err("connector read continuation is invalid".to_string());
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

impl Drop for ConnectorReadContinuation {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Eq, PartialEq)]
pub struct ConnectorReadPage<T> {
    items: Vec<T>,
    continuation: Option<ConnectorReadContinuation>,
}

impl<T> ConnectorReadPage<T> {
    pub fn new(items: Vec<T>, continuation: Option<ConnectorReadContinuation>) -> Self {
        Self {
            items,
            continuation,
        }
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn continuation(&self) -> Option<&ConnectorReadContinuation> {
        self.continuation.as_ref()
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for ConnectorReadPage<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectorReadPage")
            .field("items", &self.items)
            .field("has_continuation", &self.continuation.is_some())
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorAccountProfile {
    pub remote_account_ref: String,
    pub display_name: String,
    pub primary_address: MailAddress,
    pub tenant_ref: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailSearchRequest {
    query: String,
    max_results: u16,
}

impl MailSearchRequest {
    pub fn new(query: String, max_results: u16) -> Result<Self, String> {
        Ok(Self {
            query: bounded_required(query, MAX_MAIL_SEARCH_QUERY_CHARS, "mail search query")?,
            max_results: bounded_limit(
                max_results,
                MAX_MAIL_SEARCH_RESULTS,
                "mail search result limit",
            )?,
        })
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn max_results(&self) -> u16 {
        self.max_results
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailThreadRequest {
    thread_ref: String,
    max_messages: u16,
}

impl MailThreadRequest {
    pub fn new(thread_ref: String, max_messages: u16) -> Result<Self, String> {
        Ok(Self {
            thread_ref: bounded_required(
                thread_ref,
                MAX_REMOTE_REFERENCE_CHARS,
                "mail thread reference",
            )?,
            max_messages: bounded_limit(
                max_messages,
                MAX_THREAD_MESSAGES,
                "mail thread message limit",
            )?,
        })
    }

    pub fn thread_ref(&self) -> &str {
        &self.thread_ref
    }

    pub fn max_messages(&self) -> u16 {
        self.max_messages
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CalendarListRequest {
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
    max_results: u16,
}

impl CalendarListRequest {
    pub fn new(
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        max_results: u16,
    ) -> Result<Self, String> {
        if ends_at <= starts_at {
            return Err("calendar range must end after it starts".to_string());
        }
        if ends_at.signed_duration_since(starts_at) > chrono::Duration::days(366) {
            return Err("calendar range cannot exceed 366 days".to_string());
        }
        Ok(Self {
            starts_at,
            ends_at,
            max_results: bounded_limit(max_results, MAX_CALENDAR_EVENTS, "calendar event limit")?,
        })
    }

    pub fn starts_at(&self) -> DateTime<Utc> {
        self.starts_at
    }

    pub fn ends_at(&self) -> DateTime<Utc> {
        self.ends_at
    }

    pub fn max_results(&self) -> u16 {
        self.max_results
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectorProviderFailure {
    AuthorizationExpired,
    PermissionDenied,
    RemoteNotFound,
    CursorExpired,
    RateLimited { retry_after_seconds: Option<u64> },
    NetworkUnavailable,
    InvalidResponse,
}

impl std::fmt::Display for ConnectorProviderFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::AuthorizationExpired => "connector authorization requires repair",
            Self::PermissionDenied => "connector permission is insufficient",
            Self::RemoteNotFound => "connector remote object was not found",
            Self::CursorExpired => "connector sync cursor expired",
            Self::RateLimited { .. } => "connector provider requested bounded backoff",
            Self::NetworkUnavailable => "connector network is unavailable",
            Self::InvalidResponse => "connector provider returned an invalid response",
        })
    }
}

pub type ConnectorProviderResult<T> = Result<T, ConnectorProviderFailure>;

pub trait ConnectorAccountDiscovery: Send + Sync {
    fn discover_account(
        &self,
        account: &ConnectorAccount,
    ) -> ConnectorProviderResult<ConnectorAccountProfile>;
}

pub trait MailConnectorProvider: Send + Sync {
    fn search_mail(
        &self,
        account: &ConnectorAccount,
        request: &MailSearchRequest,
    ) -> ConnectorProviderResult<ConnectorReadPage<MailThread>> {
        self.search_mail_page(account, request, None)
    }

    fn search_mail_page(
        &self,
        account: &ConnectorAccount,
        request: &MailSearchRequest,
        continuation: Option<&ConnectorReadContinuation>,
    ) -> ConnectorProviderResult<ConnectorReadPage<MailThread>>;

    fn read_thread(
        &self,
        account: &ConnectorAccount,
        request: &MailThreadRequest,
    ) -> ConnectorProviderResult<MailThread>;
}

pub trait CalendarConnectorProvider: Send + Sync {
    fn list_events(
        &self,
        account: &ConnectorAccount,
        request: &CalendarListRequest,
    ) -> ConnectorProviderResult<ConnectorReadPage<CalendarEvent>> {
        self.list_events_page(account, request, None)
    }

    fn list_events_page(
        &self,
        account: &ConnectorAccount,
        request: &CalendarListRequest,
        continuation: Option<&ConnectorReadContinuation>,
    ) -> ConnectorProviderResult<ConnectorReadPage<CalendarEvent>>;
}

pub fn collect_mail_search(
    provider: &dyn MailConnectorProvider,
    account: &ConnectorAccount,
    request: &MailSearchRequest,
) -> ConnectorProviderResult<Vec<MailThread>> {
    let maximum_total = usize::from(request.max_results());
    let mut items = Vec::new();
    let mut continuations: Vec<ConnectorReadContinuation> = Vec::new();
    for _ in 0..MAX_READ_PAGES {
        let page = provider.search_mail_page(account, request, continuations.last())?;
        if page.items().len() > usize::from(request.max_results())
            || items.len().saturating_add(page.items().len()) > maximum_total
        {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        items.extend(page.items);
        if items.len() == maximum_total {
            return Ok(items);
        }
        let Some(next) = page.continuation else {
            return Ok(items);
        };
        if continuations.iter().any(|seen| seen == &next) {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        continuations.push(next);
    }
    Err(ConnectorProviderFailure::InvalidResponse)
}

pub fn collect_calendar_events(
    provider: &dyn CalendarConnectorProvider,
    account: &ConnectorAccount,
    request: &CalendarListRequest,
) -> ConnectorProviderResult<Vec<CalendarEvent>> {
    let maximum_total = usize::from(request.max_results());
    let mut items = Vec::new();
    let mut continuations: Vec<ConnectorReadContinuation> = Vec::new();
    for _ in 0..MAX_READ_PAGES {
        let page = provider.list_events_page(account, request, continuations.last())?;
        if page.items().len() > usize::from(request.max_results())
            || items.len().saturating_add(page.items().len()) > maximum_total
        {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        items.extend(page.items);
        if items.len() == maximum_total {
            return Ok(items);
        }
        let Some(next) = page.continuation else {
            return Ok(items);
        };
        if continuations.iter().any(|seen| seen == &next) {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        continuations.push(next);
    }
    Err(ConnectorProviderFailure::InvalidResponse)
}

fn required(value: String, field: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{field} is required"));
    }
    Ok(value)
}

fn bounded_required(value: String, maximum: usize, field: &str) -> Result<String, String> {
    let value = required(value, field)?;
    if value.chars().count() > maximum {
        return Err(format!("{field} cannot exceed {maximum} characters"));
    }
    Ok(value)
}

fn bounded_limit(value: u16, maximum: u16, field: &str) -> Result<u16, String> {
    if value == 0 || value > maximum {
        return Err(format!("{field} must be between 1 and {maximum}"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;

    struct LoopingReadProvider;

    struct TwoPageReadProvider(AtomicUsize);
    struct OverBudgetReadProvider(AtomicUsize);

    impl MailConnectorProvider for TwoPageReadProvider {
        fn search_mail_page(
            &self,
            _account: &ConnectorAccount,
            _request: &MailSearchRequest,
            continuation: Option<&ConnectorReadContinuation>,
        ) -> ConnectorProviderResult<ConnectorReadPage<MailThread>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            if continuation.is_none() {
                Ok(ConnectorReadPage::new(
                    vec![super::super::fake_mail_thread("page:1", Utc::now())],
                    Some(ConnectorReadContinuation::new("page-2".to_string()).unwrap()),
                ))
            } else {
                Ok(ConnectorReadPage::new(
                    vec![super::super::fake_mail_thread("page:2", Utc::now())],
                    None,
                ))
            }
        }

        fn read_thread(
            &self,
            _account: &ConnectorAccount,
            _request: &MailThreadRequest,
        ) -> ConnectorProviderResult<MailThread> {
            Err(ConnectorProviderFailure::RemoteNotFound)
        }
    }

    impl MailConnectorProvider for OverBudgetReadProvider {
        fn search_mail_page(
            &self,
            _account: &ConnectorAccount,
            _request: &MailSearchRequest,
            continuation: Option<&ConnectorReadContinuation>,
        ) -> ConnectorProviderResult<ConnectorReadPage<MailThread>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            if continuation.is_none() {
                Ok(ConnectorReadPage::new(
                    vec![super::super::fake_mail_thread("page:1", Utc::now())],
                    Some(ConnectorReadContinuation::new("page-2".to_string()).unwrap()),
                ))
            } else {
                Ok(ConnectorReadPage::new(
                    vec![
                        super::super::fake_mail_thread("page:2", Utc::now()),
                        super::super::fake_mail_thread("page:3", Utc::now()),
                    ],
                    None,
                ))
            }
        }

        fn read_thread(
            &self,
            _account: &ConnectorAccount,
            _request: &MailThreadRequest,
        ) -> ConnectorProviderResult<MailThread> {
            Err(ConnectorProviderFailure::RemoteNotFound)
        }
    }

    impl MailConnectorProvider for LoopingReadProvider {
        fn search_mail_page(
            &self,
            _account: &ConnectorAccount,
            _request: &MailSearchRequest,
            _continuation: Option<&ConnectorReadContinuation>,
        ) -> ConnectorProviderResult<ConnectorReadPage<MailThread>> {
            Ok(ConnectorReadPage::new(
                Vec::new(),
                Some(ConnectorReadContinuation::new("opaque-loop".to_string()).unwrap()),
            ))
        }

        fn read_thread(
            &self,
            _account: &ConnectorAccount,
            _request: &MailThreadRequest,
        ) -> ConnectorProviderResult<MailThread> {
            Err(ConnectorProviderFailure::RemoteNotFound)
        }
    }

    #[test]
    fn typed_connector_requests_enforce_bounded_inputs() {
        assert!(MailSearchRequest::new("".to_string(), 10).is_err());
        assert!(MailSearchRequest::new("urgent".to_string(), 26).is_err());
        assert!(MailSearchRequest::new("x".repeat(513), 10).is_err());
        assert!(MailThreadRequest::new("thread:1".to_string(), 0).is_err());
        assert!(MailThreadRequest::new("x".repeat(1025), 10).is_err());
        let now = Utc::now();
        assert!(CalendarListRequest::new(now, now, 10).is_err());
        assert!(CalendarListRequest::new(now, now + chrono::Duration::days(367), 10).is_err());
        assert!(CalendarListRequest::new(
            now,
            now + chrono::Duration::days(366) + chrono::Duration::seconds(1),
            10,
        )
        .is_err());
    }

    #[test]
    fn read_continuations_are_bounded_redacted_and_loops_fail_closed() {
        assert!(ConnectorReadContinuation::new(String::new()).is_err());
        assert!(
            ConnectorReadContinuation::new("x".repeat(MAX_READ_CONTINUATION_CHARS + 1)).is_err()
        );
        let page: ConnectorReadPage<()> = ConnectorReadPage::new(
            Vec::new(),
            Some(ConnectorReadContinuation::new("marker-secret".to_string()).unwrap()),
        );
        let debug = format!("{page:?}");
        assert!(debug.contains("has_continuation"));
        assert!(!debug.contains("marker-secret"));

        let now = Utc::now();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "loop".to_string(),
            display_name: "Loop fixture".to_string(),
            tenant_ref: None,
            credential_handle: super::super::ConnectorCredentialHandle::new(),
            granted_capabilities: vec![super::super::ConnectorCapability::MailSearch],
            health: super::super::ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        let request = MailSearchRequest::new("bounded".to_string(), 2).unwrap();
        let paged = TwoPageReadProvider(AtomicUsize::new(0));
        assert_eq!(
            collect_mail_search(&paged, &account, &request)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(paged.0.load(Ordering::SeqCst), 2);
        let over_budget = OverBudgetReadProvider(AtomicUsize::new(0));
        assert_eq!(
            collect_mail_search(&over_budget, &account, &request),
            Err(ConnectorProviderFailure::InvalidResponse)
        );
        assert_eq!(
            collect_mail_search(&LoopingReadProvider, &account, &request),
            Err(ConnectorProviderFailure::InvalidResponse)
        );
    }
}
