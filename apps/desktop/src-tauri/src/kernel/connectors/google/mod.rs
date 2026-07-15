use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, NaiveDate, SecondsFormat, TimeZone, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::domain::{
    CalendarAttendee, CalendarEvent, MailAddress, MailAttachmentRef, MailMessage, MailThread,
};
use super::http::{
    ConnectorAccessTokenResolver, ConnectorHttpAuthContext, ConnectorHttpBody,
    ConnectorHttpFailure, ConnectorHttpMethod, ConnectorHttpRequest, ConnectorHttpResponse,
    ConnectorHttpTransport,
};
use super::mutation::{
    build_rfc5322_message, deterministic_google_event_id, CalendarMutationEvent,
    ConnectorMailDraftContent, ConnectorMutationIntent,
};
use super::oauth::{
    validate_loopback_redirect_uri, ConnectorAuthorizationSession, ConnectorAuthorizationStatus,
    ConnectorOAuthExchange, ConnectorOAuthProvider,
};
use super::provider::{
    CalendarConnectorProvider, CalendarListRequest, ConnectorAccountDiscovery,
    ConnectorAccountProfile, ConnectorProviderFailure, ConnectorProviderResult,
    ConnectorReadContinuation, ConnectorReadPage, MailConnectorProvider, MailSearchRequest,
    MailThreadRequest,
};
use super::sync::{
    CalendarSyncProvider, CalendarSyncRequest, ConnectorOpaqueContinuation, ConnectorSyncChange,
    ConnectorSyncContinuation, ConnectorSyncPage, MailSyncProvider, MailSyncRequest,
};
use super::{
    ConnectorAccount, ConnectorCapability, ConnectorCredentialStore, ConnectorEvidenceRef,
    ConnectorHealth, ConnectorInvocation, ConnectorInvocationStatus, ConnectorMutationApplyOutcome,
    ConnectorMutationProvider, ConnectorMutationReceipt, ConnectorMutationReconciler,
    ConnectorProvider, ConnectorReconciliationOutcome, ConnectorRuntime, ConnectorSecret,
};

mod token;

#[allow(unused_imports)]
pub use token::{GoogleOAuthTokenClient, GoogleTokenFailure};

pub const GOOGLE_PROVIDER_ID: &str = "google";
pub const GOOGLE_AUTHORIZATION_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
pub const GMAIL_BASE: &str = "https://gmail.googleapis.com/gmail/v1/";
pub const GOOGLE_CALENDAR_BASE: &str = "https://www.googleapis.com/calendar/v3/";

const MAX_GOOGLE_RESPONSE_BYTES: usize = 1024 * 1024;
const GOOGLE_CAPABILITIES: &[ConnectorCapability] = &[
    ConnectorCapability::MailSearch,
    ConnectorCapability::MailReadThread,
    ConnectorCapability::MailSyncInbox,
    ConnectorCapability::MailSendDraft,
    ConnectorCapability::CalendarListEvents,
    ConnectorCapability::CalendarSyncEvents,
    ConnectorCapability::CalendarCreateEvent,
    ConnectorCapability::CalendarUpdateEvent,
    ConnectorCapability::CalendarCancelEvent,
];
const ACCESS_TOKEN_REFRESH_SKEW_SECONDS: i64 = 60;

#[derive(Deserialize, Serialize)]
struct GoogleCredentialEnvelope {
    access_token: String,
    refresh_token: String,
    expires_at: DateTime<Utc>,
    access_scopes: Vec<String>,
}

impl GoogleCredentialEnvelope {
    fn new(
        access_token: &ConnectorSecret,
        refresh_token: &ConnectorSecret,
        expires_at: DateTime<Utc>,
        access_scopes: Vec<String>,
    ) -> Self {
        Self {
            access_token: access_token.expose().to_string(),
            refresh_token: refresh_token.expose().to_string(),
            expires_at,
            access_scopes: normalized_scopes(&access_scopes),
        }
    }

    fn decode(secret: &ConnectorSecret) -> Result<Self, String> {
        let envelope: Self = serde_json::from_str(secret.expose())
            .map_err(|_| "Google credential is invalid".to_string())?;
        if envelope.access_token.trim().is_empty()
            || envelope.refresh_token.trim().is_empty()
            || envelope.access_scopes.is_empty()
            || envelope.access_scopes != normalized_scopes(&envelope.access_scopes)
        {
            return Err("Google credential is invalid".to_string());
        }
        Ok(envelope)
    }

    fn encode(&self) -> Result<ConnectorSecret, String> {
        ConnectorSecret::new(
            serde_json::to_string(self)
                .map_err(|_| "Google credential could not be encoded".to_string())?,
        )
    }

    fn access_secret(&self) -> Result<ConnectorSecret, String> {
        ConnectorSecret::new(self.access_token.clone())
    }
}

impl Drop for GoogleCredentialEnvelope {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

pub struct GoogleRefreshedCredential {
    access_token: ConnectorSecret,
    refresh_token: Option<ConnectorSecret>,
    expires_at: DateTime<Utc>,
    access_scopes: Vec<String>,
}

impl GoogleRefreshedCredential {
    fn new(
        access_token: ConnectorSecret,
        refresh_token: Option<ConnectorSecret>,
        expires_at: DateTime<Utc>,
        access_scopes: Vec<String>,
    ) -> Result<Self, String> {
        let access_scopes = normalized_scopes(&access_scopes);
        if expires_at <= Utc::now() || access_scopes.is_empty() {
            return Err("Google refreshed credential expiry is invalid".to_string());
        }
        Ok(Self {
            access_token,
            refresh_token,
            expires_at,
            access_scopes,
        })
    }
}

pub trait GoogleTokenRefresher: Send + Sync {
    fn refresh(
        &self,
        refresh_token: &ConnectorSecret,
        access_scopes: &[String],
    ) -> Result<GoogleRefreshedCredential, GoogleTokenFailure>;
}

impl<T: GoogleTokenRefresher + ?Sized> GoogleTokenRefresher for Arc<T> {
    fn refresh(
        &self,
        refresh_token: &ConnectorSecret,
        access_scopes: &[String],
    ) -> Result<GoogleRefreshedCredential, GoogleTokenFailure> {
        (**self).refresh(refresh_token, access_scopes)
    }
}

pub struct GoogleAccessTokenResolver<S, R>
where
    S: ConnectorCredentialStore + Send,
{
    runtime: Arc<ConnectorRuntime<S>>,
    refresher: R,
}

impl<S, R> GoogleAccessTokenResolver<S, R>
where
    S: ConnectorCredentialStore + Send,
{
    pub fn new(runtime: Arc<ConnectorRuntime<S>>, refresher: R) -> Self {
        Self { runtime, refresher }
    }
}

impl<S, R> ConnectorAccessTokenResolver for GoogleAccessTokenResolver<S, R>
where
    S: ConnectorCredentialStore + Send,
    R: GoogleTokenRefresher,
{
    fn resolve(
        &self,
        auth: &ConnectorHttpAuthContext,
    ) -> Result<ConnectorSecret, ConnectorHttpFailure> {
        let resolved: Result<ConnectorSecret, GoogleCredentialResolutionFailure> =
            self.runtime.with_account_credential(
                auth.credential_handle(),
                |stored| -> Result<
                    (ConnectorSecret, Option<ConnectorSecret>),
                    GoogleCredentialResolutionFailure,
                > {
                    let current = GoogleCredentialEnvelope::decode(&stored)?;
                    if current.expires_at
                        > Utc::now() + chrono::Duration::seconds(ACCESS_TOKEN_REFRESH_SKEW_SECONDS)
                    {
                        return Ok((current.access_secret()?, None));
                    }
                    let refresh_token = ConnectorSecret::new(current.refresh_token.clone())?;
                    let refreshed = self
                        .refresher
                        .refresh(&refresh_token, &current.access_scopes)
                        .map_err(GoogleCredentialResolutionFailure::Token)?;
                    if normalized_scopes(&current.access_scopes)
                        != normalized_scopes(&refreshed.access_scopes)
                    {
                        return Err("Google refreshed scopes changed".to_string().into());
                    }
                    let rotated_refresh = refreshed
                        .refresh_token
                        .as_ref()
                        .map(|secret| secret.expose())
                        .unwrap_or(&current.refresh_token);
                    let rotated_refresh = ConnectorSecret::new(rotated_refresh.to_string())?;
                    let next = GoogleCredentialEnvelope::new(
                        &refreshed.access_token,
                        &rotated_refresh,
                        refreshed.expires_at,
                        refreshed.access_scopes,
                    );
                    Ok((next.access_secret()?, Some(next.encode()?)))
                },
            );
        resolved.map_err(|failure| match failure {
            GoogleCredentialResolutionFailure::Credential => {
                ConnectorHttpFailure::CredentialUnavailable
            }
            GoogleCredentialResolutionFailure::Token(GoogleTokenFailure::Timeout) => {
                ConnectorHttpFailure::Timeout
            }
            GoogleCredentialResolutionFailure::Token(
                GoogleTokenFailure::Transient
                | GoogleTokenFailure::Network
                | GoogleTokenFailure::ResponseTooLarge
                | GoogleTokenFailure::InvalidResponse,
            ) => ConnectorHttpFailure::Network,
            GoogleCredentialResolutionFailure::Token(
                GoogleTokenFailure::CredentialUnavailable | GoogleTokenFailure::InvalidRequest,
            ) => ConnectorHttpFailure::CredentialUnavailable,
        })
    }
}

enum GoogleCredentialResolutionFailure {
    Credential,
    Token(GoogleTokenFailure),
}

impl From<String> for GoogleCredentialResolutionFailure {
    fn from(_value: String) -> Self {
        Self::Credential
    }
}

pub trait GoogleCodeExchange: Send + Sync {
    fn exchange_code(
        &self,
        code: &str,
        verifier: &ConnectorSecret,
        redirect_uri: &str,
        requested_scopes: &[String],
    ) -> Result<ConnectorOAuthExchange, String>;
}

pub struct GoogleOAuthProvider<E> {
    exchange: E,
}

impl<E> GoogleOAuthProvider<E> {
    pub fn new(exchange: E) -> Self {
        Self { exchange }
    }
}

impl<E: GoogleCodeExchange> ConnectorOAuthProvider for GoogleOAuthProvider<E> {
    fn provider_id(&self) -> &'static str {
        GOOGLE_PROVIDER_ID
    }

    fn base_scopes(&self) -> &'static [&'static str] {
        &["openid", "email"]
    }

    fn scopes_for(&self, capability: ConnectorCapability) -> Option<&'static [&'static str]> {
        google_scopes_for(capability)
    }

    fn validate_granted_scopes(
        &self,
        requested_scopes: &[String],
        granted_scopes: &[String],
    ) -> Result<(), String> {
        if normalized_scopes(requested_scopes) == normalized_scopes(granted_scopes) {
            Ok(())
        } else {
            Err("OAuth token scopes did not match the approved request".to_string())
        }
    }

    fn exchange_code(
        &self,
        code: &str,
        verifier: &ConnectorSecret,
        redirect_uri: &str,
        requested_scopes: &[String],
    ) -> Result<ConnectorOAuthExchange, String> {
        self.exchange
            .exchange_code(code, verifier, redirect_uri, requested_scopes)
    }
}

pub fn authorization_url(
    client_id: &str,
    session: &ConnectorAuthorizationSession,
    now: DateTime<Utc>,
) -> Result<String, String> {
    validate_google_client_id(client_id)?;
    if session.provider_id != GOOGLE_PROVIDER_ID
        || session.pkce_method != "S256"
        || session.status != ConnectorAuthorizationStatus::Pending
        || session.consumed_at.is_some()
        || session.expires_at <= now
        || !valid_hex_nonce(&session.state, 32)
        || !valid_pkce_challenge(&session.pkce_challenge)
    {
        return Err("Google authorization session is invalid".to_string());
    }
    let redirect_uri = validate_loopback_redirect_uri(&session.redirect_uri)
        .map_err(|_| "Google authorization session is invalid".to_string())?;
    let mut expected_scopes = vec!["email".to_string(), "openid".to_string()];
    for capability in &session.requested_capabilities {
        for scope in google_scopes_for(*capability)
            .ok_or_else(|| "Google authorization capability is invalid".to_string())?
        {
            if !expected_scopes.iter().any(|current| current == scope) {
                expected_scopes.push((*scope).to_string());
            }
        }
    }
    expected_scopes.sort();
    if session.requested_scopes != expected_scopes {
        return Err("Google authorization scopes do not match requested capabilities".to_string());
    }
    let mut url = reqwest::Url::parse(GOOGLE_AUTHORIZATION_ENDPOINT)
        .map_err(|_| "Google authorization endpoint is invalid".to_string())?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("scope", &session.requested_scopes.join(" "))
        .append_pair("state", &session.state)
        .append_pair("code_challenge", &session.pkce_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("access_type", "offline")
        .append_pair("include_granted_scopes", "false")
        .append_pair("prompt", "consent");
    Ok(url.to_string())
}

pub struct GoogleWorkspaceAdapter<T> {
    transport: T,
}

impl<T> GoogleWorkspaceAdapter<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }
}

impl<T: ConnectorHttpTransport> ConnectorProvider for GoogleWorkspaceAdapter<T> {
    fn provider_id(&self) -> &'static str {
        GOOGLE_PROVIDER_ID
    }

    fn capabilities(&self) -> &'static [ConnectorCapability] {
        GOOGLE_CAPABILITIES
    }
}

impl<T: ConnectorHttpTransport> ConnectorAccountDiscovery for GoogleWorkspaceAdapter<T> {
    fn discover_account(
        &self,
        account: &ConnectorAccount,
    ) -> ConnectorProviderResult<ConnectorAccountProfile> {
        ensure_account(account, None)?;
        let profile: GmailProfile =
            self.execute_json(account, get_request(gmail_url("users/me/profile")?), None)?;
        let address = required(profile.email_address)?;
        Ok(ConnectorAccountProfile {
            remote_account_ref: address.clone(),
            display_name: address.clone(),
            primary_address: MailAddress {
                display_name: None,
                address,
            },
            tenant_ref: account.tenant_ref.clone(),
        })
    }
}

impl<T: ConnectorHttpTransport> MailConnectorProvider for GoogleWorkspaceAdapter<T> {
    fn search_mail_page(
        &self,
        account: &ConnectorAccount,
        request: &MailSearchRequest,
        continuation: Option<&ConnectorReadContinuation>,
    ) -> ConnectorProviderResult<ConnectorReadPage<MailThread>> {
        ensure_account(account, Some(ConnectorCapability::MailSearch))?;
        let url = if let Some(continuation) = continuation {
            validated_url(
                continuation.expose(),
                "gmail.googleapis.com",
                "/gmail/v1/users/me/threads",
            )?
        } else {
            let mut url = gmail_url("users/me/threads")?;
            url.query_pairs_mut()
                .append_pair("q", request.query())
                .append_pair("maxResults", &request.max_results().to_string());
            url
        };
        let page: GmailThreadList = self.execute_json(
            account,
            get_request(url),
            Some(("gmail.googleapis.com", "/gmail/v1/users/me/threads")),
        )?;
        let refs = page.threads.unwrap_or_default();
        if refs.len() > usize::from(request.max_results()) {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let mut threads = Vec::with_capacity(refs.len());
        for thread_ref in refs {
            threads.push(self.load_thread(account, &required(thread_ref.id)?, 50)?);
        }
        let continuation = page
            .next_page_token
            .map(|token| {
                let mut url = gmail_url("users/me/threads")?;
                url.query_pairs_mut()
                    .append_pair("q", request.query())
                    .append_pair("maxResults", &request.max_results().to_string())
                    .append_pair("pageToken", &required(token)?);
                ConnectorReadContinuation::new(url.to_string())
                    .map_err(|_| ConnectorProviderFailure::InvalidResponse)
            })
            .transpose()?;
        Ok(ConnectorReadPage::new(threads, continuation))
    }

    fn read_thread(
        &self,
        account: &ConnectorAccount,
        request: &MailThreadRequest,
    ) -> ConnectorProviderResult<MailThread> {
        ensure_account(account, Some(ConnectorCapability::MailReadThread))?;
        self.load_thread(account, request.thread_ref(), request.max_messages())
    }
}

impl<T: ConnectorHttpTransport> CalendarConnectorProvider for GoogleWorkspaceAdapter<T> {
    fn list_events_page(
        &self,
        account: &ConnectorAccount,
        request: &CalendarListRequest,
        continuation: Option<&ConnectorReadContinuation>,
    ) -> ConnectorProviderResult<ConnectorReadPage<CalendarEvent>> {
        ensure_account(account, Some(ConnectorCapability::CalendarListEvents))?;
        let path = "/calendar/v3/calendars/primary/events";
        let url = if let Some(continuation) = continuation {
            validated_url(continuation.expose(), "www.googleapis.com", path)?
        } else {
            let mut url = calendar_url("calendars/primary/events")?;
            url.query_pairs_mut()
                .append_pair("timeMin", &rfc3339(request.starts_at()))
                .append_pair("timeMax", &rfc3339(request.ends_at()))
                .append_pair("singleEvents", "true")
                .append_pair("showDeleted", "false")
                .append_pair("maxResults", &request.max_results().to_string());
            url
        };
        let page: GoogleEventsPage = self.execute_json(
            account,
            get_request(url),
            Some(("www.googleapis.com", path)),
        )?;
        let events = normalize_events(page.items.unwrap_or_default(), request.max_results())?;
        for event in &events {
            if event.starts_at >= request.ends_at() || event.ends_at <= request.starts_at() {
                return Err(ConnectorProviderFailure::InvalidResponse);
            }
        }
        let continuation = page
            .next_page_token
            .map(|token| {
                let mut url = calendar_url("calendars/primary/events")?;
                url.query_pairs_mut()
                    .append_pair("timeMin", &rfc3339(request.starts_at()))
                    .append_pair("timeMax", &rfc3339(request.ends_at()))
                    .append_pair("singleEvents", "true")
                    .append_pair("showDeleted", "false")
                    .append_pair("maxResults", &request.max_results().to_string())
                    .append_pair("pageToken", &required(token)?);
                ConnectorReadContinuation::new(url.to_string())
                    .map_err(|_| ConnectorProviderFailure::InvalidResponse)
            })
            .transpose()?;
        Ok(ConnectorReadPage::new(events, continuation))
    }
}

impl<T: ConnectorHttpTransport> MailSyncProvider for GoogleWorkspaceAdapter<T> {
    fn sync_mail_page(
        &self,
        account: &ConnectorAccount,
        request: &MailSyncRequest,
        continuation: Option<&ConnectorOpaqueContinuation>,
    ) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>> {
        ensure_account(account, Some(ConnectorCapability::MailSyncInbox))?;
        if let Some(continuation) = continuation {
            let parsed = reqwest::Url::parse(continuation.expose())
                .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
            return match parsed.path() {
                "/gmail/v1/users/me/messages" => self.sync_mail_snapshot(
                    account,
                    request,
                    validated_url(
                        continuation.expose(),
                        "gmail.googleapis.com",
                        "/gmail/v1/users/me/messages",
                    )?,
                ),
                "/gmail/v1/users/me/history" => self.sync_mail_history(
                    account,
                    request,
                    validated_url(
                        continuation.expose(),
                        "gmail.googleapis.com",
                        "/gmail/v1/users/me/history",
                    )?,
                ),
                _ => Err(ConnectorProviderFailure::InvalidResponse),
            };
        }
        let mut url = gmail_url("users/me/messages")?;
        url.query_pairs_mut()
            .append_pair("labelIds", "INBOX")
            .append_pair("maxResults", &request.max_changes().to_string());
        self.sync_mail_snapshot(account, request, url)
    }
}

impl<T: ConnectorHttpTransport> CalendarSyncProvider for GoogleWorkspaceAdapter<T> {
    fn sync_calendar_page(
        &self,
        account: &ConnectorAccount,
        request: &CalendarSyncRequest,
        continuation: Option<&ConnectorOpaqueContinuation>,
    ) -> ConnectorProviderResult<ConnectorSyncPage<CalendarEvent>> {
        ensure_account(account, Some(ConnectorCapability::CalendarSyncEvents))?;
        let path = "/calendar/v3/calendars/primary/events";
        let url = if let Some(continuation) = continuation {
            validated_url(continuation.expose(), "www.googleapis.com", path)?
        } else {
            let mut url = calendar_url("calendars/primary/events")?;
            url.query_pairs_mut()
                .append_pair("timeMin", &rfc3339(request.starts_at()))
                .append_pair("timeMax", &rfc3339(request.ends_at()))
                .append_pair("singleEvents", "true")
                .append_pair("showDeleted", "true")
                .append_pair("maxResults", &request.max_changes().to_string());
            url
        };
        let page: GoogleEventsPage = self.execute_json(
            account,
            get_request(url.clone()),
            Some(("www.googleapis.com", path)),
        )?;
        let continuation = google_calendar_continuation(&url, &page, request.max_changes())?;
        let items = page.items.unwrap_or_default();
        if items.len() > usize::from(request.max_changes()) {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let mut changes = Vec::with_capacity(items.len());
        for event in items {
            if event.status.as_deref() == Some("cancelled") {
                changes.push(ConnectorSyncChange::Deleted {
                    remote_ref: required(event.id)?,
                });
            } else {
                changes.push(ConnectorSyncChange::Upsert(normalize_event(event)?));
            }
        }
        Ok(ConnectorSyncPage::new(changes, continuation))
    }
}

impl<T: ConnectorHttpTransport> ConnectorMutationProvider for GoogleWorkspaceAdapter<T> {
    fn apply_mutation(
        &self,
        account: &ConnectorAccount,
        invocation: &ConnectorInvocation,
    ) -> Result<ConnectorMutationApplyOutcome, String> {
        super::validate_connector_mutation_invocation(account, self, invocation)?;
        let intent_hash = invocation
            .mutation
            .as_ref()
            .and_then(|mutation| mutation.intent_hash.as_deref())
            .ok_or_else(|| "connector mutation intent hash is unavailable".to_string())?;
        let outcome = match invocation.mutation_intent()? {
            ConnectorMutationIntent::MailSendDraft {
                draft_ref,
                internet_message_id,
                content,
            } => {
                if let Some(content) = content {
                    return self.apply_direct_gmail_send(
                        account,
                        invocation,
                        internet_message_id,
                        content,
                    );
                }
                self.validate_gmail_draft(account, draft_ref, internet_message_id)?;
                let mut url =
                    gmail_url("users/me/drafts/send").map_err(provider_failure_message)?;
                url.query_pairs_mut().append_pair("alt", "json");
                self.execute_mutation_http(
                    account,
                    mutation_request(
                        ConnectorHttpMethod::Post,
                        url,
                        Some(serde_json::json!({ "id": draft_ref })),
                    ),
                    &[200],
                )
            }
            ConnectorMutationIntent::CalendarCreateEvent {
                calendar_ref,
                event,
            } => {
                ensure_primary_calendar(calendar_ref)?;
                let event_id = deterministic_google_event_id(&invocation.idempotency_key)?;
                let mut url =
                    calendar_url("calendars/primary/events").map_err(provider_failure_message)?;
                url.query_pairs_mut()
                    .append_pair("sendUpdates", google_send_updates(event));
                self.execute_mutation_http(
                    account,
                    mutation_request(
                        ConnectorHttpMethod::Post,
                        url,
                        Some(google_event_body(event, Some(&event_id), Some(intent_hash))),
                    ),
                    &[200],
                )
            }
            ConnectorMutationIntent::CalendarUpdateEvent {
                calendar_ref,
                event_ref,
                expected_etag,
                event,
            } => {
                ensure_primary_calendar(calendar_ref)?;
                let mut url = google_event_url(event_ref).map_err(provider_failure_message)?;
                url.query_pairs_mut()
                    .append_pair("sendUpdates", google_send_updates(event));
                let mut request = mutation_request(
                    ConnectorHttpMethod::Patch,
                    url,
                    Some(google_event_body(event, None, Some(intent_hash))),
                );
                request
                    .headers
                    .insert("If-Match".to_string(), expected_etag.clone());
                self.execute_mutation_http(account, request, &[200])
            }
            ConnectorMutationIntent::CalendarCancelEvent {
                calendar_ref,
                event_ref,
                expected_etag,
            } => {
                ensure_primary_calendar(calendar_ref)?;
                let mut url = google_event_url(event_ref).map_err(provider_failure_message)?;
                url.query_pairs_mut().append_pair("sendUpdates", "all");
                let mut request = mutation_request(ConnectorHttpMethod::Delete, url, None);
                request
                    .headers
                    .insert("If-Match".to_string(), expected_etag.clone());
                self.execute_mutation_http(account, request, &[204])
            }
        };
        match outcome {
            GoogleMutationHttpOutcome::Applied(remote_ref) => {
                Ok(ConnectorMutationApplyOutcome::Applied(
                    google_mutation_receipt(invocation, account, remote_ref, false)?,
                ))
            }
            GoogleMutationHttpOutcome::KnownNotApplied(message) => Err(message),
            GoogleMutationHttpOutcome::Uncertain => {
                Ok(ConnectorMutationApplyOutcome::ReconciliationRequired)
            }
        }
    }
}

impl<T: ConnectorHttpTransport> ConnectorMutationReconciler for GoogleWorkspaceAdapter<T> {
    fn reconcile_mutation(
        &self,
        account: &ConnectorAccount,
        invocation: &ConnectorInvocation,
    ) -> Result<ConnectorReconciliationOutcome, String> {
        if account.health != ConnectorHealth::Connected
            || account.provider_id != GOOGLE_PROVIDER_ID
            || invocation.account_id != account.id
            || invocation.provider_id != GOOGLE_PROVIDER_ID
            || invocation.status != ConnectorInvocationStatus::ReconciliationRequired
        {
            return Err("connector reconciliation is not ready".to_string());
        }
        match invocation.mutation_intent()? {
            ConnectorMutationIntent::MailSendDraft {
                draft_ref,
                internet_message_id,
                ..
            } => self.reconcile_gmail_send(account, invocation, draft_ref, internet_message_id),
            ConnectorMutationIntent::CalendarCreateEvent { calendar_ref, .. } => {
                ensure_primary_calendar(calendar_ref)?;
                self.reconcile_google_calendar_create(account, invocation)
            }
            ConnectorMutationIntent::CalendarUpdateEvent {
                calendar_ref,
                event_ref,
                expected_etag,
                ..
            } => {
                ensure_primary_calendar(calendar_ref)?;
                self.reconcile_google_calendar_update(account, invocation, event_ref, expected_etag)
            }
            ConnectorMutationIntent::CalendarCancelEvent {
                calendar_ref,
                event_ref,
                expected_etag,
            } => {
                ensure_primary_calendar(calendar_ref)?;
                self.reconcile_google_calendar_cancel(account, invocation, event_ref, expected_etag)
            }
        }
    }
}

impl<T: ConnectorHttpTransport> GoogleWorkspaceAdapter<T> {
    fn apply_direct_gmail_send(
        &self,
        account: &ConnectorAccount,
        invocation: &ConnectorInvocation,
        internet_message_id: &str,
        content: &ConnectorMailDraftContent,
    ) -> Result<ConnectorMutationApplyOutcome, String> {
        let mut mime = build_rfc5322_message(internet_message_id, content)?;
        let mut encoded = general_purpose::URL_SAFE_NO_PAD.encode(&mime);
        mime.zeroize();
        let mut payload = serde_json::json!({"raw": encoded});
        if let Some(thread_ref) = &content.thread_ref {
            payload["threadId"] = serde_json::Value::String(thread_ref.clone());
        }
        let body = ConnectorHttpBody::json_owned(payload)
            .map_err(|_| "Google MIME payload is invalid".to_string());
        encoded.zeroize();
        let url = gmail_url("users/me/messages/send").map_err(provider_failure_message)?;
        let request = ConnectorHttpRequest {
            method: ConnectorHttpMethod::Post,
            url: url.to_string(),
            headers: BTreeMap::new(),
            body: Some(body?),
            max_response_bytes: MAX_GOOGLE_RESPONSE_BYTES,
        };
        match self.execute_mutation_http(account, request, &[200]) {
            GoogleMutationHttpOutcome::Applied(remote_ref) => {
                Ok(ConnectorMutationApplyOutcome::Applied(
                    google_mutation_receipt(invocation, account, remote_ref, false)?,
                ))
            }
            GoogleMutationHttpOutcome::KnownNotApplied(message) => Err(message),
            GoogleMutationHttpOutcome::Uncertain => {
                Ok(ConnectorMutationApplyOutcome::ReconciliationRequired)
            }
        }
    }

    fn validate_gmail_draft(
        &self,
        account: &ConnectorAccount,
        draft_ref: &str,
        internet_message_id: &str,
    ) -> Result<(), String> {
        let mut url = gmail_draft_url(draft_ref).map_err(provider_failure_message)?;
        url.query_pairs_mut()
            .append_pair("format", "metadata")
            .append_pair("metadataHeaders", "Message-ID");
        let draft: GmailDraft = self
            .execute_json(account, get_request(url), None)
            .map_err(provider_failure_message)?;
        let headers = draft
            .message
            .payload
            .as_ref()
            .map(|payload| header_map(&payload.headers))
            .unwrap_or_default();
        if draft.id != draft_ref
            || headers.get("message-id").map(String::as_str) != Some(internet_message_id)
        {
            return Err("Google mail draft changed after review".to_string());
        }
        Ok(())
    }

    fn reconcile_gmail_send(
        &self,
        account: &ConnectorAccount,
        invocation: &ConnectorInvocation,
        draft_ref: &str,
        internet_message_id: &str,
    ) -> Result<ConnectorReconciliationOutcome, String> {
        let mut url = gmail_url("users/me/messages").map_err(provider_failure_message)?;
        url.query_pairs_mut()
            .append_pair("q", &format!("rfc822msgid:{internet_message_id} in:sent"))
            .append_pair("maxResults", "2");
        let page: GmailMessageList = self
            .execute_json(account, get_request(url), None)
            .map_err(provider_failure_message)?;
        let messages = page.messages.unwrap_or_default();
        if messages.len() == 1 {
            return Ok(ConnectorReconciliationOutcome::Applied(
                google_mutation_receipt(
                    invocation,
                    account,
                    required(messages[0].id.clone()).map_err(provider_failure_message)?,
                    true,
                )?,
            ));
        }
        if messages.len() > 1 {
            return Ok(ConnectorReconciliationOutcome::StillUncertain);
        }
        if invocation.mutation_intent()?.mail_content().is_some() {
            return Ok(ConnectorReconciliationOutcome::StillUncertain);
        }
        match self.validate_gmail_draft(account, draft_ref, internet_message_id) {
            Ok(()) => Ok(ConnectorReconciliationOutcome::KnownNotApplied),
            Err(_) => Ok(ConnectorReconciliationOutcome::StillUncertain),
        }
    }

    fn reconcile_google_calendar_create(
        &self,
        account: &ConnectorAccount,
        invocation: &ConnectorInvocation,
    ) -> Result<ConnectorReconciliationOutcome, String> {
        let event_id = deterministic_google_event_id(&invocation.idempotency_key)?;
        self.reconcile_google_event_hash(account, invocation, &event_id, None)
    }

    fn reconcile_google_calendar_update(
        &self,
        account: &ConnectorAccount,
        invocation: &ConnectorInvocation,
        event_ref: &str,
        expected_etag: &str,
    ) -> Result<ConnectorReconciliationOutcome, String> {
        self.reconcile_google_event_hash(account, invocation, event_ref, Some(expected_etag))
    }

    fn reconcile_google_event_hash(
        &self,
        account: &ConnectorAccount,
        invocation: &ConnectorInvocation,
        event_ref: &str,
        expected_etag: Option<&str>,
    ) -> Result<ConnectorReconciliationOutcome, String> {
        let url = google_event_url(event_ref).map_err(provider_failure_message)?;
        match self.execute_json::<GoogleEvent>(account, get_request(url), None) {
            Err(ConnectorProviderFailure::RemoteNotFound) if expected_etag.is_none() => {
                Ok(ConnectorReconciliationOutcome::KnownNotApplied)
            }
            Err(failure) => Err(provider_failure_message(failure)),
            Ok(event) => {
                if event.id != event_ref {
                    return Ok(ConnectorReconciliationOutcome::StillUncertain);
                }
                if expected_etag.is_some_and(|expected| event.etag.as_deref() == Some(expected)) {
                    return Ok(ConnectorReconciliationOutcome::KnownNotApplied);
                }
                let expected_hash = invocation
                    .mutation
                    .as_ref()
                    .and_then(|mutation| mutation.intent_hash.as_deref());
                if event
                    .extended_properties
                    .as_ref()
                    .and_then(|properties| properties.private.as_ref())
                    .and_then(|properties| properties.get("dsAgentIntentHash"))
                    .map(String::as_str)
                    == expected_hash
                {
                    Ok(ConnectorReconciliationOutcome::Applied(
                        google_mutation_receipt(
                            invocation,
                            account,
                            required(event.id).map_err(provider_failure_message)?,
                            true,
                        )?,
                    ))
                } else {
                    Ok(ConnectorReconciliationOutcome::StillUncertain)
                }
            }
        }
    }

    fn reconcile_google_calendar_cancel(
        &self,
        account: &ConnectorAccount,
        invocation: &ConnectorInvocation,
        event_ref: &str,
        expected_etag: &str,
    ) -> Result<ConnectorReconciliationOutcome, String> {
        let url = google_event_url(event_ref).map_err(provider_failure_message)?;
        match self.execute_json::<GoogleEvent>(account, get_request(url), None) {
            Err(ConnectorProviderFailure::RemoteNotFound) => {
                Ok(ConnectorReconciliationOutcome::Applied(
                    google_mutation_receipt(invocation, account, event_ref.to_string(), true)?,
                ))
            }
            Ok(event) if event.etag.as_deref() == Some(expected_etag) => {
                Ok(ConnectorReconciliationOutcome::KnownNotApplied)
            }
            Ok(_) => Ok(ConnectorReconciliationOutcome::StillUncertain),
            Err(failure) => Err(provider_failure_message(failure)),
        }
    }

    fn execute_mutation_http(
        &self,
        account: &ConnectorAccount,
        request: ConnectorHttpRequest,
        successful_statuses: &[u16],
    ) -> GoogleMutationHttpOutcome {
        let response = match self
            .transport
            .execute(ConnectorHttpAuthContext::for_account(account), request)
        {
            Ok(response) => response,
            Err(
                ConnectorHttpFailure::Timeout
                | ConnectorHttpFailure::Network
                | ConnectorHttpFailure::ResponseTooLarge,
            ) => return GoogleMutationHttpOutcome::Uncertain,
            Err(
                ConnectorHttpFailure::BeforeSend
                | ConnectorHttpFailure::CredentialUnavailable
                | ConnectorHttpFailure::InvalidRequest,
            ) => {
                return GoogleMutationHttpOutcome::KnownNotApplied(
                    "Google mutation did not start".to_string(),
                )
            }
        };
        if successful_statuses.contains(&response.status) {
            let remote_ref = serde_json::from_slice::<GoogleMutationIdentity>(&response.body)
                .ok()
                .and_then(|identity| non_empty(identity.id))
                .unwrap_or_else(|| "google:accepted".to_string());
            return GoogleMutationHttpOutcome::Applied(remote_ref);
        }
        if response.status == 409 || response.status == 429 || response.status >= 500 {
            GoogleMutationHttpOutcome::Uncertain
        } else {
            GoogleMutationHttpOutcome::KnownNotApplied(
                "Google rejected the exact mutation".to_string(),
            )
        }
    }

    fn load_thread(
        &self,
        account: &ConnectorAccount,
        thread_ref: &str,
        maximum: u16,
    ) -> ConnectorProviderResult<MailThread> {
        if thread_ref.trim().is_empty() || thread_ref.len() > 1024 {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let mut url = gmail_url("users/me/threads")?;
        url.path_segments_mut()
            .map_err(|_| ConnectorProviderFailure::InvalidResponse)?
            .push(thread_ref);
        url.query_pairs_mut().append_pair("format", "full");
        let thread: GmailThread = self.execute_json(account, get_request(url), None)?;
        if thread.id != thread_ref || thread.messages.len() > usize::from(maximum) {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let messages = thread
            .messages
            .into_iter()
            .map(normalize_message)
            .collect::<ConnectorProviderResult<Vec<_>>>()?;
        if messages.is_empty()
            || messages
                .iter()
                .any(|message| message.thread_ref != thread_ref)
        {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let thread = MailThread {
            remote_ref: thread_ref.to_string(),
            messages,
        };
        thread
            .validate()
            .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
        Ok(thread)
    }

    fn load_message(
        &self,
        account: &ConnectorAccount,
        message_ref: &str,
    ) -> ConnectorProviderResult<MailMessage> {
        if message_ref.trim().is_empty() || message_ref.len() > 1024 {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let mut url = gmail_url("users/me/messages")?;
        url.path_segments_mut()
            .map_err(|_| ConnectorProviderFailure::InvalidResponse)?
            .push(message_ref);
        url.query_pairs_mut().append_pair("format", "full");
        let message: GmailMessage = self.execute_json(account, get_request(url), None)?;
        if message.id != message_ref {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        normalize_message(message)
    }

    fn sync_mail_snapshot(
        &self,
        account: &ConnectorAccount,
        request: &MailSyncRequest,
        url: reqwest::Url,
    ) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>> {
        let page: GmailMessageList = self.execute_json(
            account,
            get_request(url.clone()),
            Some(("gmail.googleapis.com", "/gmail/v1/users/me/messages")),
        )?;
        let refs = page.messages.unwrap_or_default();
        if refs.len() > usize::from(request.max_changes()) {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let changes = refs
            .into_iter()
            .map(|item| self.load_message(account, &required(item.id)?))
            .map(|result| result.map(ConnectorSyncChange::Upsert))
            .collect::<ConnectorProviderResult<Vec<_>>>()?;
        let continuation = if let Some(token) = page.next_page_token {
            let next = replace_query_pair(&url, "pageToken", &required(token)?);
            ConnectorSyncContinuation::Next(
                ConnectorOpaqueContinuation::new(next.to_string())
                    .map_err(|_| ConnectorProviderFailure::InvalidResponse)?,
            )
        } else {
            let profile: GmailProfile =
                self.execute_json(account, get_request(gmail_url("users/me/profile")?), None)?;
            ConnectorSyncContinuation::Delta(history_url(
                &required(profile.history_id)?,
                None,
                request.max_changes(),
            )?)
        };
        Ok(ConnectorSyncPage::new(changes, continuation))
    }

    fn sync_mail_history(
        &self,
        account: &ConnectorAccount,
        request: &MailSyncRequest,
        url: reqwest::Url,
    ) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>> {
        let original_history = url
            .query_pairs()
            .find(|(key, _)| key == "startHistoryId")
            .map(|(_, value)| value.into_owned())
            .and_then(|value| required(value).ok())
            .ok_or(ConnectorProviderFailure::InvalidResponse)?;
        let page: GmailHistoryPage = self.execute_json(
            account,
            get_request(url.clone()),
            Some(("gmail.googleapis.com", "/gmail/v1/users/me/history")),
        )?;
        let mut added = BTreeSet::new();
        let mut deleted = BTreeSet::new();
        for history in page.history.unwrap_or_default() {
            for item in history.messages_added.unwrap_or_default() {
                added.insert(required(item.message.id)?);
            }
            for item in history.messages_deleted.unwrap_or_default() {
                deleted.insert(required(item.message.id)?);
            }
        }
        for message in &deleted {
            added.remove(message);
        }
        if added.len().saturating_add(deleted.len()) > usize::from(request.max_changes()) {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let mut changes = Vec::with_capacity(added.len() + deleted.len());
        for message_ref in deleted {
            changes.push(ConnectorSyncChange::Deleted {
                remote_ref: message_ref,
            });
        }
        for message_ref in added {
            changes.push(ConnectorSyncChange::Upsert(
                self.load_message(account, &message_ref)?,
            ));
        }
        let continuation = if let Some(token) = page.next_page_token {
            ConnectorSyncContinuation::Next(history_url(
                &original_history,
                Some(&required(token)?),
                request.max_changes(),
            )?)
        } else {
            ConnectorSyncContinuation::Delta(history_url(
                &required(page.history_id)?,
                None,
                request.max_changes(),
            )?)
        };
        Ok(ConnectorSyncPage::new(changes, continuation))
    }

    fn execute_json<R: DeserializeOwned>(
        &self,
        account: &ConnectorAccount,
        request: ConnectorHttpRequest,
        continuation: Option<(&str, &str)>,
    ) -> ConnectorProviderResult<R> {
        let max_response_bytes = request.max_response_bytes;
        let response = self
            .transport
            .execute(ConnectorHttpAuthContext::for_account(account), request)
            .map_err(map_transport_failure)?;
        let mut body = validate_response(response, max_response_bytes)?;
        let value = serde_json::from_slice::<serde_json::Value>(&body);
        body.zeroize();
        let value = value.map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
        if let Some((host, path)) = continuation {
            for key in ["nextPageToken", "nextSyncToken"] {
                if value.get(key).is_some_and(|item| !item.is_string()) {
                    return Err(ConnectorProviderFailure::InvalidResponse);
                }
            }
            if let Some(link) = value.get("nextLink").and_then(|item| item.as_str()) {
                validated_url(link, host, path)?;
            }
        }
        serde_json::from_value(value).map_err(|_| ConnectorProviderFailure::InvalidResponse)
    }
}

fn google_calendar_continuation(
    previous: &reqwest::Url,
    page: &GoogleEventsPage,
    max_changes: u16,
) -> ConnectorProviderResult<ConnectorSyncContinuation> {
    match (&page.next_page_token, &page.next_sync_token) {
        (Some(page_token), None) => {
            let next = replace_query_pair(previous, "pageToken", &required(page_token.clone())?);
            Ok(ConnectorSyncContinuation::Next(
                ConnectorOpaqueContinuation::new(next.to_string())
                    .map_err(|_| ConnectorProviderFailure::InvalidResponse)?,
            ))
        }
        (None, Some(sync_token)) => {
            let mut next = calendar_url("calendars/primary/events")?;
            next.query_pairs_mut()
                .append_pair("singleEvents", "true")
                .append_pair("showDeleted", "true")
                .append_pair("maxResults", &max_changes.to_string())
                .append_pair("syncToken", &required(sync_token.clone())?);
            Ok(ConnectorSyncContinuation::Delta(
                ConnectorOpaqueContinuation::new(next.to_string())
                    .map_err(|_| ConnectorProviderFailure::InvalidResponse)?,
            ))
        }
        _ => Err(ConnectorProviderFailure::InvalidResponse),
    }
}

fn history_url(
    history_id: &str,
    page_token: Option<&str>,
    max_changes: u16,
) -> ConnectorProviderResult<ConnectorOpaqueContinuation> {
    let mut url = gmail_url("users/me/history")?;
    url.query_pairs_mut()
        .append_pair("startHistoryId", history_id)
        .append_pair("labelId", "INBOX")
        .append_pair("historyTypes", "messageAdded")
        .append_pair("historyTypes", "messageDeleted")
        .append_pair("maxResults", &max_changes.to_string());
    if let Some(page_token) = page_token {
        url.query_pairs_mut().append_pair("pageToken", page_token);
    }
    ConnectorOpaqueContinuation::new(url.to_string())
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)
}

fn replace_query_pair(previous: &reqwest::Url, key: &str, value: &str) -> reqwest::Url {
    let retained = previous
        .query_pairs()
        .filter(|(name, _)| name != key)
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    let mut next = previous.clone();
    next.set_query(None);
    {
        let mut pairs = next.query_pairs_mut();
        for (name, value) in retained {
            pairs.append_pair(&name, &value);
        }
        pairs.append_pair(key, value);
    }
    next
}

fn gmail_draft_url(draft_ref: &str) -> ConnectorProviderResult<reqwest::Url> {
    if draft_ref.trim().is_empty() || draft_ref.len() > 1024 || draft_ref.contains(['\r', '\n']) {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    let mut url = gmail_url("users/me/drafts")?;
    url.path_segments_mut()
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)?
        .push(draft_ref);
    Ok(url)
}

fn google_event_url(event_ref: &str) -> ConnectorProviderResult<reqwest::Url> {
    if event_ref.trim().is_empty() || event_ref.len() > 1024 || event_ref.contains(['\r', '\n']) {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    let mut url = calendar_url("calendars/primary/events")?;
    url.path_segments_mut()
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)?
        .push(event_ref);
    Ok(url)
}

fn mutation_request(
    method: ConnectorHttpMethod,
    url: reqwest::Url,
    body: Option<serde_json::Value>,
) -> ConnectorHttpRequest {
    ConnectorHttpRequest {
        method,
        url: url.to_string(),
        headers: BTreeMap::new(),
        body: body
            .map(ConnectorHttpBody::json_owned)
            .transpose()
            .expect("bounded static connector JSON body serializes"),
        max_response_bytes: MAX_GOOGLE_RESPONSE_BYTES,
    }
}

fn google_event_body(
    event: &CalendarMutationEvent,
    event_id: Option<&str>,
    intent_hash: Option<&str>,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "summary": event.title,
        "description": event.description.clone().unwrap_or_default(),
        "location": event.location.clone().unwrap_or_default(),
        "start": {
            "dateTime": event.starts_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            "timeZone": event.timezone,
        },
        "end": {
            "dateTime": event.ends_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            "timeZone": event.timezone,
        },
        "attendees": event.attendees.iter().map(|attendee| serde_json::json!({
            "email": attendee.address,
            "displayName": attendee.display_name,
        })).collect::<Vec<_>>(),
    });
    if let Some(event_id) = event_id {
        body["id"] = serde_json::Value::String(event_id.to_string());
    }
    if let Some(intent_hash) = intent_hash {
        body["extendedProperties"] = serde_json::json!({
            "private": { "dsAgentIntentHash": intent_hash }
        });
    }
    body
}

fn google_send_updates(event: &CalendarMutationEvent) -> &'static str {
    if event.notify_attendees {
        "all"
    } else {
        "none"
    }
}

fn ensure_primary_calendar(calendar_ref: &str) -> Result<(), String> {
    if calendar_ref == "primary" {
        Ok(())
    } else {
        Err("Google calendar reference is unsupported".to_string())
    }
}

fn google_mutation_receipt(
    invocation: &ConnectorInvocation,
    account: &ConnectorAccount,
    remote_ref: String,
    reconciled: bool,
) -> Result<ConnectorMutationReceipt, String> {
    ConnectorMutationReceipt::applied(
        invocation,
        ConnectorEvidenceRef {
            provider_id: GOOGLE_PROVIDER_ID.to_string(),
            account_id: account.id,
            remote_object_ref: remote_ref,
            retrieved_at: Utc::now(),
            bounded_summary: Some(if reconciled {
                "Google external result was verified by read-only reconciliation.".to_string()
            } else {
                "Google accepted the exact approved mutation.".to_string()
            }),
        },
        reconciled,
    )
}

fn provider_failure_message(failure: ConnectorProviderFailure) -> String {
    failure.to_string()
}

enum GoogleMutationHttpOutcome {
    Applied(String),
    KnownNotApplied(String),
    Uncertain,
}

fn normalize_message(message: GmailMessage) -> ConnectorProviderResult<MailMessage> {
    let remote_ref = required(message.id)?;
    let thread_ref = required(message.thread_id)?;
    let headers = message
        .payload
        .as_ref()
        .map(|payload| header_map(&payload.headers))
        .unwrap_or_default();
    let from = parse_address(
        headers
            .get("from")
            .ok_or(ConnectorProviderFailure::InvalidResponse)?,
    )?;
    let to = headers
        .get("to")
        .map(|value| parse_addresses(value))
        .transpose()?
        .unwrap_or_default();
    let received_at = message
        .internal_date
        .parse::<i64>()
        .ok()
        .and_then(DateTime::from_timestamp_millis)
        .ok_or(ConnectorProviderFailure::InvalidResponse)?;
    let mut attachments = Vec::new();
    if let Some(payload) = &message.payload {
        collect_attachments(payload, &mut attachments)?;
    }
    let normalized = MailMessage {
        remote_ref,
        thread_ref,
        from,
        to,
        subject: headers.get("subject").cloned().unwrap_or_default(),
        received_at,
        bounded_body_summary: message.snippet.map(|value| bounded(value, 2000)),
        has_attachments: !attachments.is_empty(),
        attachments,
        untrusted_evidence: true,
    };
    normalized
        .validate()
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
    Ok(normalized)
}

fn collect_attachments(
    part: &GmailMessagePart,
    output: &mut Vec<MailAttachmentRef>,
) -> ConnectorProviderResult<()> {
    if output.len() > 100 {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    if !part.filename.trim().is_empty() {
        let attachment_ref = part
            .body
            .as_ref()
            .and_then(|body| body.attachment_id.clone())
            .and_then(|value| required(value).ok())
            .ok_or(ConnectorProviderFailure::InvalidResponse)?;
        let lower = part.filename.to_ascii_lowercase();
        output.push(MailAttachmentRef {
            remote_ref: attachment_ref,
            file_name: part.filename.clone(),
            media_type: required(part.mime_type.clone())?,
            size_bytes: part.body.as_ref().and_then(|body| body.size).unwrap_or(0),
            contains_macros: lower.ends_with(".docm")
                || lower.ends_with(".xlsm")
                || lower.ends_with(".pptm"),
        });
    }
    for child in &part.parts {
        collect_attachments(child, output)?;
    }
    Ok(())
}

fn normalize_events(
    events: Vec<GoogleEvent>,
    maximum: u16,
) -> ConnectorProviderResult<Vec<CalendarEvent>> {
    if events.len() > usize::from(maximum) {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    events.into_iter().map(normalize_event).collect()
}

fn normalize_event(event: GoogleEvent) -> ConnectorProviderResult<CalendarEvent> {
    let starts_at = parse_google_datetime(
        event
            .start
            .as_ref()
            .ok_or(ConnectorProviderFailure::InvalidResponse)?,
    )?;
    let ends_at = parse_google_datetime(
        event
            .end
            .as_ref()
            .ok_or(ConnectorProviderFailure::InvalidResponse)?,
    )?;
    let timezone = event
        .start
        .as_ref()
        .and_then(|value| value.time_zone.clone())
        .unwrap_or_else(|| "UTC".to_string());
    let normalized = CalendarEvent {
        remote_ref: required(event.id)?,
        calendar_ref: "primary".to_string(),
        title: event.summary.unwrap_or_default(),
        starts_at,
        ends_at,
        timezone,
        attendees: event
            .attendees
            .unwrap_or_default()
            .into_iter()
            .map(|attendee| {
                Ok(CalendarAttendee {
                    address: MailAddress {
                        display_name: attendee.display_name,
                        address: required(attendee.email)?,
                    },
                    response: attendee.response_status,
                })
            })
            .collect::<ConnectorProviderResult<Vec<_>>>()?,
        meeting_url: event.hangout_link,
        recurrence: event.recurrence.and_then(|items| items.into_iter().next()),
        untrusted_evidence: true,
    };
    normalized
        .validate()
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
    Ok(normalized)
}

fn parse_google_datetime(value: &GoogleDateTime) -> ConnectorProviderResult<DateTime<Utc>> {
    if let Some(date_time) = &value.date_time {
        return DateTime::parse_from_rfc3339(date_time)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|_| ConnectorProviderFailure::InvalidResponse);
    }
    let date = value
        .date
        .as_ref()
        .and_then(|value| NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
        .ok_or(ConnectorProviderFailure::InvalidResponse)?;
    Utc.from_local_datetime(
        &date
            .and_hms_opt(0, 0, 0)
            .ok_or(ConnectorProviderFailure::InvalidResponse)?,
    )
    .single()
    .ok_or(ConnectorProviderFailure::InvalidResponse)
}

fn header_map(headers: &[GmailHeader]) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter_map(|header| {
            let name = header.name.trim().to_ascii_lowercase();
            (!name.is_empty() && header.value.len() <= 8192)
                .then(|| (name, header.value.trim().to_string()))
        })
        .collect()
}

fn parse_addresses(value: &str) -> ConnectorProviderResult<Vec<MailAddress>> {
    let values = value.split(',').take(51).collect::<Vec<_>>();
    if values.len() > 50 {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    values.into_iter().map(parse_address).collect()
}

fn parse_address(value: &str) -> ConnectorProviderResult<MailAddress> {
    let value = value.trim();
    let (display_name, address) = match (value.rfind('<'), value.rfind('>')) {
        (Some(start), Some(end)) if start < end => (
            non_empty(value[..start].trim().trim_matches('"').to_string()),
            value[start + 1..end].trim().to_string(),
        ),
        _ => (None, value.to_string()),
    };
    if address.len() > 512 || !address.contains('@') || address.contains(['\r', '\n']) {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    Ok(MailAddress {
        display_name,
        address,
    })
}

fn google_scopes_for(capability: ConnectorCapability) -> Option<&'static [&'static str]> {
    match capability {
        ConnectorCapability::MailSearch
        | ConnectorCapability::MailReadThread
        | ConnectorCapability::MailReadAttachment
        | ConnectorCapability::MailSyncInbox => {
            Some(&["https://www.googleapis.com/auth/gmail.readonly"])
        }
        ConnectorCapability::MailCreateDraft => {
            Some(&["https://www.googleapis.com/auth/gmail.compose"])
        }
        ConnectorCapability::MailSendDraft => Some(&["https://www.googleapis.com/auth/gmail.send"]),
        ConnectorCapability::CalendarListEvents
        | ConnectorCapability::CalendarSyncEvents
        | ConnectorCapability::CalendarFindFreeTime => {
            Some(&["https://www.googleapis.com/auth/calendar.readonly"])
        }
        ConnectorCapability::CalendarCreateEvent
        | ConnectorCapability::CalendarUpdateEvent
        | ConnectorCapability::CalendarCancelEvent => {
            Some(&["https://www.googleapis.com/auth/calendar.events"])
        }
    }
}

fn validate_google_client_id(value: &str) -> Result<(), String> {
    if value.len() > 256
        || !value.ends_with(".apps.googleusercontent.com")
        || value.contains(['\r', '\n', '/', '\\'])
    {
        return Err("Google client id is invalid".to_string());
    }
    Ok(())
}

fn normalized_scopes(scopes: &[String]) -> Vec<String> {
    let mut scopes = scopes
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    scopes.sort();
    scopes.dedup();
    scopes
}

fn ensure_account(
    account: &ConnectorAccount,
    capability: Option<ConnectorCapability>,
) -> ConnectorProviderResult<()> {
    if account.provider_id != GOOGLE_PROVIDER_ID || account.health != ConnectorHealth::Connected {
        return Err(ConnectorProviderFailure::PermissionDenied);
    }
    if capability.is_some_and(|capability| !account.granted_capabilities.contains(&capability)) {
        return Err(ConnectorProviderFailure::PermissionDenied);
    }
    Ok(())
}

fn gmail_url(path: &str) -> ConnectorProviderResult<reqwest::Url> {
    provider_url(GMAIL_BASE, path)
}

fn calendar_url(path: &str) -> ConnectorProviderResult<reqwest::Url> {
    provider_url(GOOGLE_CALENDAR_BASE, path)
}

fn provider_url(base: &str, path: &str) -> ConnectorProviderResult<reqwest::Url> {
    if path.starts_with('/') || path.contains("..") || path.contains(':') {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    reqwest::Url::parse(base)
        .and_then(|base| base.join(path))
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)
}

fn validated_url(value: &str, host: &str, path: &str) -> ConnectorProviderResult<reqwest::Url> {
    let url = reqwest::Url::parse(value).map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
    if url.scheme() != "https"
        || url.host_str() != Some(host)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path() != path
    {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    Ok(url)
}

fn get_request(url: reqwest::Url) -> ConnectorHttpRequest {
    ConnectorHttpRequest {
        method: ConnectorHttpMethod::Get,
        url: url.to_string(),
        headers: BTreeMap::new(),
        body: None,
        max_response_bytes: MAX_GOOGLE_RESPONSE_BYTES,
    }
}

fn validate_response(
    mut response: ConnectorHttpResponse,
    maximum: usize,
) -> ConnectorProviderResult<Vec<u8>> {
    if response.body.len() > maximum {
        response.body.zeroize();
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    if (200..=299).contains(&response.status) {
        return Ok(std::mem::take(&mut response.body));
    }
    let failure = match response.status {
        401 => ConnectorProviderFailure::AuthorizationExpired,
        403 => ConnectorProviderFailure::PermissionDenied,
        404 => ConnectorProviderFailure::RemoteNotFound,
        410 => ConnectorProviderFailure::CursorExpired,
        429 => ConnectorProviderFailure::RateLimited {
            retry_after_seconds: response
                .headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("retry-after"))
                .and_then(|(_, value)| value.parse::<u64>().ok())
                .map(|value| value.clamp(1, 900)),
        },
        500..=599 => ConnectorProviderFailure::NetworkUnavailable,
        _ => ConnectorProviderFailure::InvalidResponse,
    };
    response.body.zeroize();
    Err(failure)
}

fn map_transport_failure(failure: ConnectorHttpFailure) -> ConnectorProviderFailure {
    match failure {
        ConnectorHttpFailure::CredentialUnavailable => {
            ConnectorProviderFailure::AuthorizationExpired
        }
        ConnectorHttpFailure::InvalidRequest | ConnectorHttpFailure::ResponseTooLarge => {
            ConnectorProviderFailure::InvalidResponse
        }
        ConnectorHttpFailure::BeforeSend
        | ConnectorHttpFailure::Timeout
        | ConnectorHttpFailure::Network => ConnectorProviderFailure::NetworkUnavailable,
    }
}

fn rfc3339(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn required(value: String) -> ConnectorProviderResult<String> {
    non_empty(value).ok_or(ConnectorProviderFailure::InvalidResponse)
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty() && value.len() <= 8192).then_some(value)
}

fn bounded(value: String, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}

fn valid_hex_nonce(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_pkce_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailProfile {
    email_address: String,
    history_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailThreadList {
    threads: Option<Vec<GmailRef>>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailMessageList {
    messages: Option<Vec<GmailRef>>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct GmailRef {
    id: String,
}

#[derive(Deserialize)]
struct GmailThread {
    id: String,
    #[serde(default)]
    messages: Vec<GmailMessage>,
}

#[derive(Deserialize)]
struct GmailDraft {
    id: String,
    message: GmailMessage,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailMessage {
    id: String,
    thread_id: String,
    internal_date: String,
    snippet: Option<String>,
    payload: Option<GmailMessagePart>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct GmailMessagePart {
    #[serde(default)]
    mime_type: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    headers: Vec<GmailHeader>,
    body: Option<GmailPartBody>,
    #[serde(default)]
    parts: Vec<GmailMessagePart>,
}

#[derive(Deserialize)]
struct GmailHeader {
    name: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailPartBody {
    attachment_id: Option<String>,
    size: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailHistoryPage {
    history: Option<Vec<GmailHistory>>,
    next_page_token: Option<String>,
    history_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailHistory {
    messages_added: Option<Vec<GmailHistoryMessage>>,
    messages_deleted: Option<Vec<GmailHistoryMessage>>,
}

#[derive(Deserialize)]
struct GmailHistoryMessage {
    message: GmailRef,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleEventsPage {
    items: Option<Vec<GoogleEvent>>,
    next_page_token: Option<String>,
    next_sync_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleEvent {
    id: String,
    etag: Option<String>,
    status: Option<String>,
    summary: Option<String>,
    start: Option<GoogleDateTime>,
    end: Option<GoogleDateTime>,
    attendees: Option<Vec<GoogleAttendee>>,
    hangout_link: Option<String>,
    recurrence: Option<Vec<String>>,
    extended_properties: Option<GoogleExtendedProperties>,
}

#[derive(Deserialize)]
struct GoogleExtendedProperties {
    private: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
struct GoogleMutationIdentity {
    #[serde(default)]
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleDateTime {
    date_time: Option<String>,
    date: Option<String>,
    time_zone: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleAttendee {
    email: String,
    display_name: Option<String>,
    response_status: Option<String>,
}

#[cfg(test)]
mod tests;
