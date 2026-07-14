use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

use super::domain::{CalendarAttendee, CalendarEvent, MailAddress, MailMessage, MailThread};
use super::http::{
    ConnectorAccessTokenResolver, ConnectorHttpAuthContext, ConnectorHttpFailure,
    ConnectorHttpMethod, ConnectorHttpRequest, ConnectorHttpResponse, ConnectorHttpTransport,
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
    ConnectorAccount, ConnectorCapability, ConnectorCredentialStore, ConnectorHealth,
    ConnectorProvider, ConnectorRuntime, ConnectorSecret,
};

mod attachment;
mod token;

#[allow(unused_imports)]
pub use token::{MicrosoftOAuthTokenClient, MicrosoftTokenFailure};

pub const MICROSOFT_PROVIDER_ID: &str = "microsoft";
pub const MICROSOFT_AUTHORITY: &str = "https://login.microsoftonline.com/organizations/oauth2/v2.0";
pub const MICROSOFT_GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0/";
const MAX_GRAPH_RESPONSE_BYTES: usize = 1024 * 1024;
const MICROSOFT_READ_CAPABILITIES: &[ConnectorCapability] = &[
    ConnectorCapability::MailSearch,
    ConnectorCapability::MailReadThread,
    ConnectorCapability::MailSyncInbox,
    ConnectorCapability::CalendarListEvents,
    ConnectorCapability::CalendarSyncEvents,
];
const ACCESS_TOKEN_REFRESH_SKEW_SECONDS: i64 = 60;
const MICROSOFT_MAIL_DELTA_PATH: &str = "/v1.0/me/mailFolders/inbox/messages/delta";
const MICROSOFT_CALENDAR_DELTA_PATH: &str = "/v1.0/me/calendarView/delta";

#[derive(Deserialize, Serialize)]
struct MicrosoftCredentialEnvelope {
    access_token: String,
    refresh_token: String,
    expires_at: DateTime<Utc>,
    access_scopes: Vec<String>,
}

impl MicrosoftCredentialEnvelope {
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
            access_scopes: normalized_microsoft_access_scopes(&access_scopes),
        }
    }

    fn decode(secret: &ConnectorSecret) -> Result<Self, String> {
        let envelope: Self = serde_json::from_str(secret.expose())
            .map_err(|_| "Microsoft credential is invalid".to_string())?;
        if envelope.access_token.trim().is_empty()
            || envelope.refresh_token.trim().is_empty()
            || envelope.access_scopes.is_empty()
            || envelope.access_scopes != normalized_microsoft_access_scopes(&envelope.access_scopes)
        {
            return Err("Microsoft credential is invalid".to_string());
        }
        Ok(envelope)
    }

    fn encode(&self) -> Result<ConnectorSecret, String> {
        ConnectorSecret::new(
            serde_json::to_string(self)
                .map_err(|_| "Microsoft credential could not be encoded".to_string())?,
        )
    }

    fn access_secret(&self) -> Result<ConnectorSecret, String> {
        ConnectorSecret::new(self.access_token.clone())
    }
}

impl Drop for MicrosoftCredentialEnvelope {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

pub struct MicrosoftRefreshedCredential {
    access_token: ConnectorSecret,
    refresh_token: Option<ConnectorSecret>,
    expires_at: DateTime<Utc>,
    access_scopes: Vec<String>,
}

impl MicrosoftRefreshedCredential {
    pub fn new(
        access_token: ConnectorSecret,
        refresh_token: Option<ConnectorSecret>,
        expires_at: DateTime<Utc>,
        access_scopes: Vec<String>,
    ) -> Result<Self, String> {
        let access_scopes = normalized_microsoft_access_scopes(&access_scopes);
        if expires_at <= Utc::now() || access_scopes.is_empty() {
            return Err("Microsoft refreshed credential expiry is invalid".to_string());
        }
        Ok(Self {
            access_token,
            refresh_token,
            expires_at,
            access_scopes,
        })
    }
}

pub trait MicrosoftTokenRefresher: Send + Sync {
    fn refresh(
        &self,
        refresh_token: &ConnectorSecret,
        access_scopes: &[String],
    ) -> Result<MicrosoftRefreshedCredential, MicrosoftTokenFailure>;
}

impl<T: MicrosoftTokenRefresher + ?Sized> MicrosoftTokenRefresher for Arc<T> {
    fn refresh(
        &self,
        refresh_token: &ConnectorSecret,
        access_scopes: &[String],
    ) -> Result<MicrosoftRefreshedCredential, MicrosoftTokenFailure> {
        (**self).refresh(refresh_token, access_scopes)
    }
}

pub struct MicrosoftAccessTokenResolver<S, R>
where
    S: ConnectorCredentialStore + Send,
{
    runtime: Arc<ConnectorRuntime<S>>,
    refresher: R,
}

impl<S, R> MicrosoftAccessTokenResolver<S, R>
where
    S: ConnectorCredentialStore + Send,
{
    pub fn new(runtime: Arc<ConnectorRuntime<S>>, refresher: R) -> Self {
        Self { runtime, refresher }
    }
}

impl<S, R> ConnectorAccessTokenResolver for MicrosoftAccessTokenResolver<S, R>
where
    S: ConnectorCredentialStore + Send,
    R: MicrosoftTokenRefresher,
{
    fn resolve(
        &self,
        auth: &ConnectorHttpAuthContext,
    ) -> Result<ConnectorSecret, ConnectorHttpFailure> {
        let resolved: Result<ConnectorSecret, MicrosoftCredentialResolutionFailure> = self
            .runtime
            .with_account_credential(auth.credential_handle(), |stored| {
                let current = MicrosoftCredentialEnvelope::decode(&stored)?;
                if current.expires_at
                    > Utc::now() + chrono::Duration::seconds(ACCESS_TOKEN_REFRESH_SKEW_SECONDS)
                {
                    return Ok((current.access_secret()?, None));
                }
                let refresh_token = ConnectorSecret::new(current.refresh_token.clone())?;
                let refreshed = self
                    .refresher
                    .refresh(&refresh_token, &current.access_scopes)
                    .map_err(MicrosoftCredentialResolutionFailure::Token)?;
                validate_microsoft_access_scopes(&current.access_scopes, &refreshed.access_scopes)?;
                let rotated_refresh = refreshed
                    .refresh_token
                    .as_ref()
                    .map(|secret| secret.expose())
                    .unwrap_or(&current.refresh_token);
                let rotated_refresh = ConnectorSecret::new(rotated_refresh.to_string())?;
                let next = MicrosoftCredentialEnvelope::new(
                    &refreshed.access_token,
                    &rotated_refresh,
                    refreshed.expires_at,
                    refreshed.access_scopes,
                );
                let access_token = next.access_secret()?;
                Ok((access_token, Some(next.encode()?)))
            });
        resolved.map_err(|failure| match failure {
            MicrosoftCredentialResolutionFailure::Credential => {
                ConnectorHttpFailure::CredentialUnavailable
            }
            MicrosoftCredentialResolutionFailure::Token(MicrosoftTokenFailure::Timeout) => {
                ConnectorHttpFailure::Timeout
            }
            MicrosoftCredentialResolutionFailure::Token(
                MicrosoftTokenFailure::Transient
                | MicrosoftTokenFailure::Network
                | MicrosoftTokenFailure::ResponseTooLarge
                | MicrosoftTokenFailure::InvalidResponse,
            ) => ConnectorHttpFailure::Network,
            MicrosoftCredentialResolutionFailure::Token(
                MicrosoftTokenFailure::CredentialUnavailable
                | MicrosoftTokenFailure::InvalidRequest,
            ) => ConnectorHttpFailure::CredentialUnavailable,
        })
    }
}

enum MicrosoftCredentialResolutionFailure {
    Credential,
    Token(MicrosoftTokenFailure),
}

impl From<String> for MicrosoftCredentialResolutionFailure {
    fn from(_value: String) -> Self {
        Self::Credential
    }
}

pub trait MicrosoftCodeExchange: Send + Sync {
    fn exchange_code(
        &self,
        code: &str,
        verifier: &ConnectorSecret,
        redirect_uri: &str,
        requested_scopes: &[String],
    ) -> Result<ConnectorOAuthExchange, String>;
}

pub struct MicrosoftOAuthProvider<E> {
    exchange: E,
}

impl<E> MicrosoftOAuthProvider<E> {
    pub fn new(exchange: E) -> Self {
        Self { exchange }
    }
}

impl<E: MicrosoftCodeExchange> ConnectorOAuthProvider for MicrosoftOAuthProvider<E> {
    fn provider_id(&self) -> &'static str {
        MICROSOFT_PROVIDER_ID
    }

    fn base_scopes(&self) -> &'static [&'static str] {
        &["offline_access", "User.Read"]
    }

    fn scopes_for(&self, capability: ConnectorCapability) -> Option<&'static [&'static str]> {
        microsoft_scopes_for(capability)
    }

    fn validate_granted_scopes(
        &self,
        requested_scopes: &[String],
        granted_scopes: &[String],
    ) -> Result<(), String> {
        validate_microsoft_access_scopes(requested_scopes, granted_scopes)
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

fn normalized_microsoft_access_scopes(scopes: &[String]) -> Vec<String> {
    let mut scopes = scopes
        .iter()
        .map(|scope| scope.trim())
        .filter(|scope| !scope.is_empty() && *scope != "offline_access")
        .map(str::to_string)
        .collect::<Vec<_>>();
    scopes.sort();
    scopes.dedup();
    scopes
}

fn validate_microsoft_access_scopes(
    requested_scopes: &[String],
    granted_scopes: &[String],
) -> Result<(), String> {
    if normalized_microsoft_access_scopes(requested_scopes)
        == normalized_microsoft_access_scopes(granted_scopes)
    {
        Ok(())
    } else {
        Err("OAuth token scopes did not match the approved request".to_string())
    }
}

pub fn authorization_url(
    client_id: &str,
    session: &ConnectorAuthorizationSession,
    now: DateTime<Utc>,
) -> Result<String, String> {
    Uuid::parse_str(client_id).map_err(|_| "Microsoft client id is invalid".to_string())?;
    if session.provider_id != MICROSOFT_PROVIDER_ID
        || session.pkce_method != "S256"
        || session.status != ConnectorAuthorizationStatus::Pending
        || session.consumed_at.is_some()
        || session.expires_at <= now
        || !valid_oauth_nonce(&session.state, 32)
        || !valid_pkce_challenge(&session.pkce_challenge)
    {
        return Err("Microsoft authorization session is invalid".to_string());
    }
    let redirect_uri = validate_loopback_redirect_uri(&session.redirect_uri)
        .map_err(|_| "Microsoft authorization session is invalid".to_string())?;
    let mut expected_scopes = vec!["offline_access".to_string(), "User.Read".to_string()];
    for capability in &session.requested_capabilities {
        for scope in microsoft_scopes_for(*capability)
            .ok_or_else(|| "Microsoft authorization capability is invalid".to_string())?
        {
            if !expected_scopes.iter().any(|current| current == scope) {
                expected_scopes.push((*scope).to_string());
            }
        }
    }
    expected_scopes.sort();
    if session.requested_scopes != expected_scopes {
        return Err(
            "Microsoft authorization scopes do not match requested capabilities".to_string(),
        );
    }
    let mut url = reqwest::Url::parse(&format!("{MICROSOFT_AUTHORITY}/authorize"))
        .map_err(|_| "Microsoft authority is invalid".to_string())?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &redirect_uri)
        .append_pair("response_mode", "query")
        .append_pair("scope", &session.requested_scopes.join(" "))
        .append_pair("state", &session.state)
        .append_pair("code_challenge", &session.pkce_challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url.to_string())
}

pub struct MicrosoftGraphAdapter<T> {
    transport: T,
}

impl<T> MicrosoftGraphAdapter<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }
}

impl<T: ConnectorHttpTransport> ConnectorProvider for MicrosoftGraphAdapter<T> {
    fn provider_id(&self) -> &'static str {
        MICROSOFT_PROVIDER_ID
    }

    fn capabilities(&self) -> &'static [ConnectorCapability] {
        MICROSOFT_READ_CAPABILITIES
    }
}

impl<T: ConnectorHttpTransport> ConnectorAccountDiscovery for MicrosoftGraphAdapter<T> {
    fn discover_account(
        &self,
        account: &ConnectorAccount,
    ) -> ConnectorProviderResult<ConnectorAccountProfile> {
        ensure_account(account, None)?;
        let mut url = graph_url("me")?;
        url.query_pairs_mut()
            .append_pair("$select", "id,displayName,mail,userPrincipalName");
        let profile: GraphProfile = self.execute_json(account, get_request(url), None)?;
        let address = profile
            .mail
            .or(profile.user_principal_name)
            .ok_or(ConnectorProviderFailure::InvalidResponse)?;
        if profile.id.trim().is_empty() || address.trim().is_empty() {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        Ok(ConnectorAccountProfile {
            remote_account_ref: profile.id,
            display_name: non_empty(profile.display_name).unwrap_or_else(|| address.clone()),
            primary_address: MailAddress {
                display_name: None,
                address,
            },
            tenant_ref: account.tenant_ref.clone(),
        })
    }
}

impl<T: ConnectorHttpTransport> MailConnectorProvider for MicrosoftGraphAdapter<T> {
    fn search_mail_page(
        &self,
        account: &ConnectorAccount,
        request: &MailSearchRequest,
        continuation: Option<&ConnectorReadContinuation>,
    ) -> ConnectorProviderResult<ConnectorReadPage<MailThread>> {
        ensure_account(account, Some(ConnectorCapability::MailSearch))?;
        let url = if let Some(continuation) = continuation {
            read_continuation_url(continuation, "/v1.0/me/messages")?
        } else {
            let mut url = graph_url("me/messages")?;
            let search = format!("\"{}\"", escape_search(request.query()));
            url.query_pairs_mut()
                .append_pair("$search", &search)
                .append_pair(
                    "$select",
                    "id,conversationId,sender,toRecipients,subject,receivedDateTime,bodyPreview,hasAttachments",
                )
                .append_pair("$top", &request.max_results().to_string());
            url
        };
        let mut http = get_request(url);
        http.headers
            .insert("ConsistencyLevel".to_string(), "eventual".to_string());
        let page: GraphCollection<GraphMessage> =
            self.execute_json(account, http, Some("/v1.0/me/messages"))?;
        let continuation = read_continuation(page.next_link)?;
        Ok(ConnectorReadPage::new(
            normalize_threads(page.value, request.max_results())?,
            continuation,
        ))
    }

    fn read_thread(
        &self,
        account: &ConnectorAccount,
        request: &MailThreadRequest,
    ) -> ConnectorProviderResult<MailThread> {
        ensure_account(account, Some(ConnectorCapability::MailReadThread))?;
        let mut url = graph_url("me/messages")?;
        let filter = format!(
            "conversationId eq '{}'",
            request.thread_ref().replace('\'', "''")
        );
        url.query_pairs_mut()
            .append_pair("$filter", &filter)
            .append_pair("$orderby", "receivedDateTime asc")
            .append_pair(
                "$select",
                "id,conversationId,sender,toRecipients,subject,receivedDateTime,bodyPreview,hasAttachments",
            )
            .append_pair("$top", &request.max_messages().to_string());
        let page: GraphCollection<GraphMessage> =
            self.execute_json(account, get_request(url), Some("/v1.0/me/messages"))?;
        let mut threads = normalize_threads(page.value, request.max_messages())?;
        if threads.len() != 1 || threads[0].remote_ref != request.thread_ref() {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        Ok(threads.remove(0))
    }
}

impl<T: ConnectorHttpTransport> CalendarConnectorProvider for MicrosoftGraphAdapter<T> {
    fn list_events_page(
        &self,
        account: &ConnectorAccount,
        request: &CalendarListRequest,
        continuation: Option<&ConnectorReadContinuation>,
    ) -> ConnectorProviderResult<ConnectorReadPage<CalendarEvent>> {
        ensure_account(account, Some(ConnectorCapability::CalendarListEvents))?;
        let url = if let Some(continuation) = continuation {
            read_continuation_url(continuation, "/v1.0/me/calendarView")?
        } else {
            let mut url = graph_url("me/calendarView")?;
            url.query_pairs_mut()
                .append_pair(
                    "startDateTime",
                    &request
                        .starts_at()
                        .to_rfc3339_opts(SecondsFormat::Secs, true),
                )
                .append_pair(
                    "endDateTime",
                    &request.ends_at().to_rfc3339_opts(SecondsFormat::Secs, true),
                )
                .append_pair(
                    "$select",
                    "id,subject,start,end,attendees,onlineMeeting,recurrence",
                )
                .append_pair("$top", &request.max_results().to_string());
            url
        };
        let mut http = get_request(url);
        http.headers
            .insert("Prefer".to_string(), "outlook.timezone=\"UTC\"".to_string());
        let page: GraphCollection<GraphEvent> =
            self.execute_json(account, http, Some("/v1.0/me/calendarView"))?;
        if page.value.len() > usize::from(request.max_results()) {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let continuation = read_continuation(page.next_link)?;
        let items = page
            .value
            .into_iter()
            .map(normalize_event)
            .map(|event| {
                let event = event?;
                ensure_calendar_overlap(&event, request.starts_at(), request.ends_at())?;
                Ok(event)
            })
            .collect::<ConnectorProviderResult<Vec<_>>>()?;
        Ok(ConnectorReadPage::new(items, continuation))
    }
}

impl<T: ConnectorHttpTransport> MailSyncProvider for MicrosoftGraphAdapter<T> {
    fn sync_mail_page(
        &self,
        account: &ConnectorAccount,
        request: &MailSyncRequest,
        continuation: Option<&ConnectorOpaqueContinuation>,
    ) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>> {
        ensure_account(account, Some(ConnectorCapability::MailSyncInbox))?;
        let http = if let Some(continuation) = continuation {
            get_request(continuation_url(continuation, MICROSOFT_MAIL_DELTA_PATH)?)
        } else {
            let mut url = graph_url("me/mailFolders/inbox/messages/delta")?;
            url.query_pairs_mut()
                .append_pair(
                    "$select",
                    "id,conversationId,sender,toRecipients,subject,receivedDateTime,bodyPreview,hasAttachments",
                )
                .append_pair("$top", &request.max_changes().to_string());
            get_request(url)
        };
        let value = self.execute_value(account, http, Some(MICROSOFT_MAIL_DELTA_PATH))?;
        normalize_mail_sync_page(value, request.max_changes())
    }
}

impl<T: ConnectorHttpTransport> CalendarSyncProvider for MicrosoftGraphAdapter<T> {
    fn sync_calendar_page(
        &self,
        account: &ConnectorAccount,
        request: &CalendarSyncRequest,
        continuation: Option<&ConnectorOpaqueContinuation>,
    ) -> ConnectorProviderResult<ConnectorSyncPage<CalendarEvent>> {
        ensure_account(account, Some(ConnectorCapability::CalendarSyncEvents))?;
        let mut http = if let Some(continuation) = continuation {
            get_request(continuation_url(
                continuation,
                MICROSOFT_CALENDAR_DELTA_PATH,
            )?)
        } else {
            let mut url = graph_url("me/calendarView/delta")?;
            url.query_pairs_mut()
                .append_pair(
                    "startDateTime",
                    &request
                        .starts_at()
                        .to_rfc3339_opts(SecondsFormat::Secs, true),
                )
                .append_pair(
                    "endDateTime",
                    &request.ends_at().to_rfc3339_opts(SecondsFormat::Secs, true),
                )
                .append_pair(
                    "$select",
                    "id,subject,start,end,attendees,onlineMeeting,recurrence",
                )
                .append_pair("$top", &request.max_changes().to_string());
            get_request(url)
        };
        http.headers
            .insert("Prefer".to_string(), "outlook.timezone=\"UTC\"".to_string());
        let value = self.execute_value(account, http, Some(MICROSOFT_CALENDAR_DELTA_PATH))?;
        normalize_calendar_sync_page(
            value,
            request.max_changes(),
            request.starts_at(),
            request.ends_at(),
        )
    }
}

impl<T: ConnectorHttpTransport> MicrosoftGraphAdapter<T> {
    pub(crate) fn attachment_metadata(
        &self,
        account: &ConnectorAccount,
        message_remote_ref: &str,
        attachment_remote_ref: &str,
    ) -> ConnectorProviderResult<super::landing::ConnectorAttachmentMetadata> {
        ensure_account(account, Some(ConnectorCapability::MailReadAttachment))?;
        if message_remote_ref.trim().is_empty()
            || message_remote_ref.len() > 1024
            || attachment_remote_ref.trim().is_empty()
            || attachment_remote_ref.len() > 1024
        {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let mut url = graph_url("me")?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
            segments
                .push("messages")
                .push(message_remote_ref)
                .push("attachments")
                .push(attachment_remote_ref);
        }
        url.query_pairs_mut()
            .append_pair("$select", "id,name,contentType,size,isInline,@odata.type");
        let attachment: GraphFileAttachment = self.execute_json(account, get_request(url), None)?;
        if attachment.odata_type != "#microsoft.graph.fileAttachment"
            || attachment.id != attachment_remote_ref
            || attachment.name.trim().is_empty()
            || attachment.content_type.trim().is_empty()
            || attachment.size == 0
        {
            return Err(ConnectorProviderFailure::InvalidResponse);
        }
        let lower_name = attachment.name.to_ascii_lowercase();
        Ok(super::landing::ConnectorAttachmentMetadata {
            account_id: account.id,
            provider_id: MICROSOFT_PROVIDER_ID.to_string(),
            parent_remote_ref: message_remote_ref.to_string(),
            attachment_remote_ref: attachment.id,
            file_name: attachment.name,
            declared_media_type: attachment.content_type,
            size_bytes: attachment.size,
            contains_macros: lower_name.ends_with(".docm")
                || lower_name.ends_with(".xlsm")
                || lower_name.ends_with(".pptm"),
            untrusted_evidence: true,
        })
    }

    fn execute_json<R: DeserializeOwned>(
        &self,
        account: &ConnectorAccount,
        request: ConnectorHttpRequest,
        continuation_path: Option<&str>,
    ) -> ConnectorProviderResult<R> {
        let value = self.execute_value(account, request, continuation_path)?;
        serde_json::from_value(value).map_err(|_| ConnectorProviderFailure::InvalidResponse)
    }

    fn execute_value(
        &self,
        account: &ConnectorAccount,
        request: ConnectorHttpRequest,
        continuation_path: Option<&str>,
    ) -> ConnectorProviderResult<serde_json::Value> {
        let max_response_bytes = request.max_response_bytes;
        let response = self
            .transport
            .execute(ConnectorHttpAuthContext::for_account(account), request)
            .map_err(map_transport_failure)?;
        let mut body = validate_response(response, max_response_bytes)?;
        let value = serde_json::from_slice(&body);
        body.zeroize();
        let value: serde_json::Value =
            value.map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
        if let Some(path) = continuation_path {
            for key in ["@odata.nextLink", "@odata.deltaLink"] {
                if let Some(item) = value.get(key) {
                    let link = item
                        .as_str()
                        .ok_or(ConnectorProviderFailure::InvalidResponse)?;
                    validate_continuation_link(link, path)?;
                }
            }
        }
        Ok(value)
    }
}

fn ensure_account(
    account: &ConnectorAccount,
    capability: Option<ConnectorCapability>,
) -> ConnectorProviderResult<()> {
    if account.provider_id != MICROSOFT_PROVIDER_ID || account.health != ConnectorHealth::Connected
    {
        return Err(ConnectorProviderFailure::PermissionDenied);
    }
    if capability.is_some_and(|value| !account.granted_capabilities.contains(&value)) {
        return Err(ConnectorProviderFailure::PermissionDenied);
    }
    Ok(())
}

fn graph_url(path: &str) -> ConnectorProviderResult<reqwest::Url> {
    if path.starts_with('/') || path.contains("..") || path.contains(':') {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    reqwest::Url::parse(MICROSOFT_GRAPH_BASE)
        .and_then(|base| base.join(path))
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)
}

fn get_request(url: reqwest::Url) -> ConnectorHttpRequest {
    ConnectorHttpRequest {
        method: ConnectorHttpMethod::Get,
        url: url.to_string(),
        headers: BTreeMap::new(),
        body: None,
        max_response_bytes: MAX_GRAPH_RESPONSE_BYTES,
    }
}

fn validate_response(
    mut response: ConnectorHttpResponse,
    max_response_bytes: usize,
) -> ConnectorProviderResult<Vec<u8>> {
    if response.body.len() > max_response_bytes {
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
            retry_after_seconds: header(&response.headers, "retry-after")
                .and_then(|value| value.parse::<u64>().ok())
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

fn header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn validate_continuation_link(link: &str, expected_path: &str) -> ConnectorProviderResult<()> {
    let url = reqwest::Url::parse(link).map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
    let valid = url.scheme() == "https"
        && url.host_str() == Some("graph.microsoft.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.path() == expected_path;
    if valid {
        Ok(())
    } else {
        Err(ConnectorProviderFailure::InvalidResponse)
    }
}

fn continuation_url(
    continuation: &ConnectorOpaqueContinuation,
    expected_path: &str,
) -> ConnectorProviderResult<reqwest::Url> {
    validate_continuation_link(continuation.expose(), expected_path)?;
    reqwest::Url::parse(continuation.expose())
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)
}

fn read_continuation_url(
    continuation: &ConnectorReadContinuation,
    expected_path: &str,
) -> ConnectorProviderResult<reqwest::Url> {
    validate_continuation_link(continuation.expose(), expected_path)?;
    reqwest::Url::parse(continuation.expose())
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)
}

fn read_continuation(
    value: Option<String>,
) -> ConnectorProviderResult<Option<ConnectorReadContinuation>> {
    value
        .map(|value| {
            ConnectorReadContinuation::new(value)
                .map_err(|_| ConnectorProviderFailure::InvalidResponse)
        })
        .transpose()
}

fn sync_continuation(
    value: &serde_json::Value,
) -> ConnectorProviderResult<ConnectorSyncContinuation> {
    let next = value.get("@odata.nextLink").and_then(|item| item.as_str());
    let delta = value.get("@odata.deltaLink").and_then(|item| item.as_str());
    match (next, delta) {
        (Some(next), None) => ConnectorOpaqueContinuation::new(next.to_string())
            .map(ConnectorSyncContinuation::Next)
            .map_err(|_| ConnectorProviderFailure::InvalidResponse),
        (None, Some(delta)) => ConnectorOpaqueContinuation::new(delta.to_string())
            .map(ConnectorSyncContinuation::Delta)
            .map_err(|_| ConnectorProviderFailure::InvalidResponse),
        _ => Err(ConnectorProviderFailure::InvalidResponse),
    }
}

fn normalize_mail_sync_page(
    value: serde_json::Value,
    maximum: u16,
) -> ConnectorProviderResult<ConnectorSyncPage<MailMessage>> {
    let items = value
        .get("value")
        .and_then(serde_json::Value::as_array)
        .ok_or(ConnectorProviderFailure::InvalidResponse)?;
    if items.len() > usize::from(maximum) {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    let mut changes = Vec::with_capacity(items.len());
    for item in items {
        if removed_marker(item)? {
            changes.push(ConnectorSyncChange::Deleted {
                remote_ref: removed_remote_ref(item)?,
            });
        } else {
            let message: GraphMessage = serde_json::from_value(item.clone())
                .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
            changes.push(ConnectorSyncChange::Upsert(normalize_message(message)?));
        }
    }
    Ok(ConnectorSyncPage::new(changes, sync_continuation(&value)?))
}

fn normalize_calendar_sync_page(
    value: serde_json::Value,
    maximum: u16,
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
) -> ConnectorProviderResult<ConnectorSyncPage<CalendarEvent>> {
    let items = value
        .get("value")
        .and_then(serde_json::Value::as_array)
        .ok_or(ConnectorProviderFailure::InvalidResponse)?;
    if items.len() > usize::from(maximum) {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    let mut changes = Vec::with_capacity(items.len());
    for item in items {
        if removed_marker(item)? {
            changes.push(ConnectorSyncChange::Deleted {
                remote_ref: removed_remote_ref(item)?,
            });
        } else {
            let event: GraphEvent = serde_json::from_value(item.clone())
                .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
            let event = normalize_event(event)?;
            ensure_calendar_overlap(&event, starts_at, ends_at)?;
            changes.push(ConnectorSyncChange::Upsert(event));
        }
    }
    Ok(ConnectorSyncPage::new(changes, sync_continuation(&value)?))
}

fn removed_marker(value: &serde_json::Value) -> ConnectorProviderResult<bool> {
    match value.get("@removed") {
        None => Ok(false),
        Some(marker) if marker.is_object() => Ok(true),
        Some(_) => Err(ConnectorProviderFailure::InvalidResponse),
    }
}

fn ensure_calendar_overlap(
    event: &CalendarEvent,
    starts_at: DateTime<Utc>,
    ends_at: DateTime<Utc>,
) -> ConnectorProviderResult<()> {
    if event.starts_at < ends_at && event.ends_at > starts_at {
        Ok(())
    } else {
        Err(ConnectorProviderFailure::InvalidResponse)
    }
}

fn removed_remote_ref(value: &serde_json::Value) -> ConnectorProviderResult<String> {
    value
        .get("id")
        .and_then(serde_json::Value::as_str)
        .map(ToString::to_string)
        .and_then(non_empty)
        .ok_or(ConnectorProviderFailure::InvalidResponse)
}

fn escape_search(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn valid_oauth_nonce(value: &str, expected_length: usize) -> bool {
    value.len() == expected_length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_pkce_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
}

fn microsoft_scopes_for(capability: ConnectorCapability) -> Option<&'static [&'static str]> {
    match capability {
        ConnectorCapability::MailSearch
        | ConnectorCapability::MailReadThread
        | ConnectorCapability::MailReadAttachment
        | ConnectorCapability::MailSyncInbox => Some(&["Mail.Read"]),
        ConnectorCapability::CalendarListEvents | ConnectorCapability::CalendarSyncEvents => {
            Some(&["Calendars.Read"])
        }
        _ => None,
    }
}

fn normalize_threads(
    messages: Vec<GraphMessage>,
    maximum: u16,
) -> ConnectorProviderResult<Vec<MailThread>> {
    if messages.len() > usize::from(maximum) {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    let mut threads: Vec<MailThread> = Vec::new();
    for message in messages {
        let message = normalize_message(message)?;
        if let Some(thread) = threads
            .iter_mut()
            .find(|thread| thread.remote_ref == message.thread_ref)
        {
            thread.messages.push(message);
        } else {
            threads.push(MailThread {
                remote_ref: message.thread_ref.clone(),
                messages: vec![message],
            });
        }
    }
    for thread in &threads {
        thread
            .validate()
            .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
    }
    Ok(threads)
}

fn normalize_message(message: GraphMessage) -> ConnectorProviderResult<MailMessage> {
    let sender = message
        .sender
        .ok_or(ConnectorProviderFailure::InvalidResponse)?;
    let received_at = DateTime::parse_from_rfc3339(&message.received_date_time)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
    let normalized = MailMessage {
        remote_ref: required_value(message.id)?,
        thread_ref: required_value(message.conversation_id)?,
        from: normalize_address(sender.email_address)?,
        to: message
            .to_recipients
            .into_iter()
            .map(|recipient| normalize_address(recipient.email_address))
            .collect::<ConnectorProviderResult<Vec<_>>>()?,
        subject: bounded(message.subject.unwrap_or_default(), 512),
        received_at,
        bounded_body_summary: message.body_preview.map(|value| bounded(value, 1000)),
        attachments: Vec::new(),
        has_attachments: message.has_attachments,
        untrusted_evidence: true,
    };
    normalized
        .validate()
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
    Ok(normalized)
}

fn normalize_event(event: GraphEvent) -> ConnectorProviderResult<CalendarEvent> {
    let starts_at = parse_graph_datetime(&event.start)?;
    let ends_at = parse_graph_datetime(&event.end)?;
    let normalized = CalendarEvent {
        remote_ref: required_value(event.id)?,
        calendar_ref: "microsoft:default-calendar".to_string(),
        title: bounded(event.subject.unwrap_or_default(), 512),
        starts_at,
        ends_at,
        timezone: event.start.time_zone,
        attendees: event
            .attendees
            .into_iter()
            .map(|attendee| {
                Ok(CalendarAttendee {
                    address: normalize_address(attendee.email_address)?,
                    response: attendee
                        .status
                        .and_then(|status| non_empty(status.response)),
                })
            })
            .collect::<ConnectorProviderResult<Vec<_>>>()?,
        meeting_url: event
            .online_meeting
            .and_then(|meeting| non_empty(meeting.join_url)),
        recurrence: event
            .recurrence
            .and_then(|recurrence| recurrence.pattern)
            .and_then(|pattern| non_empty(pattern.kind)),
        untrusted_evidence: true,
    };
    normalized
        .validate()
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)?;
    Ok(normalized)
}

fn parse_graph_datetime(value: &GraphDateTime) -> ConnectorProviderResult<DateTime<Utc>> {
    if let Ok(value) = DateTime::parse_from_rfc3339(&value.date_time) {
        return Ok(value.with_timezone(&Utc));
    }
    if value.time_zone != "UTC" {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    NaiveDateTime::parse_from_str(&value.date_time, "%Y-%m-%dT%H:%M:%S%.f")
        .map(|value| DateTime::from_naive_utc_and_offset(value, Utc))
        .map_err(|_| ConnectorProviderFailure::InvalidResponse)
}

fn normalize_address(value: GraphEmailAddress) -> ConnectorProviderResult<MailAddress> {
    let address = required_value(value.address)?;
    if !address.contains('@') {
        return Err(ConnectorProviderFailure::InvalidResponse);
    }
    Ok(MailAddress {
        display_name: value.name.and_then(non_empty),
        address,
    })
}

fn required_value(value: String) -> ConnectorProviderResult<String> {
    non_empty(value).ok_or(ConnectorProviderFailure::InvalidResponse)
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn bounded(value: String, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphProfile {
    id: String,
    display_name: String,
    mail: Option<String>,
    user_principal_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphFileAttachment {
    #[serde(rename = "@odata.type")]
    odata_type: String,
    id: String,
    name: String,
    content_type: String,
    size: u64,
}

#[derive(Deserialize)]
struct GraphCollection<T> {
    value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    next_link: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphMessage {
    id: String,
    conversation_id: String,
    sender: Option<GraphRecipient>,
    #[serde(default)]
    to_recipients: Vec<GraphRecipient>,
    subject: Option<String>,
    received_date_time: String,
    body_preview: Option<String>,
    #[serde(default)]
    has_attachments: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphRecipient {
    email_address: GraphEmailAddress,
}

#[derive(Deserialize)]
struct GraphEmailAddress {
    name: Option<String>,
    address: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphEvent {
    id: String,
    subject: Option<String>,
    start: GraphDateTime,
    end: GraphDateTime,
    #[serde(default)]
    attendees: Vec<GraphAttendee>,
    online_meeting: Option<GraphOnlineMeeting>,
    recurrence: Option<GraphRecurrence>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphDateTime {
    date_time: String,
    time_zone: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphAttendee {
    email_address: GraphEmailAddress,
    status: Option<GraphAttendeeStatus>,
}

#[derive(Deserialize)]
struct GraphAttendeeStatus {
    response: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphOnlineMeeting {
    join_url: String,
}

#[derive(Deserialize)]
struct GraphRecurrence {
    pattern: Option<GraphRecurrencePattern>,
}

#[derive(Deserialize)]
struct GraphRecurrencePattern {
    #[serde(rename = "type")]
    kind: String,
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use chrono::Duration;
    use serde_json::json;

    use super::*;
    use crate::kernel::connectors::contract::{
        validate_calendar_read_contract, validate_calendar_sync_contract,
        validate_mail_read_contract, validate_mail_sync_contract, validate_provider_contract,
    };
    use crate::kernel::connectors::http::{
        json_response, ConnectorHttpResponse, ScriptedConnectorHttpTransport,
    };
    use crate::kernel::connectors::oauth::begin_authorization;
    use crate::kernel::connectors::sync::ConnectorSyncState;
    use crate::kernel::connectors::{ConnectorCredentialHandle, FakeConnectorCredentialStore};
    use crate::kernel::event_store::EventStore;

    struct OfflineExchange;

    impl MicrosoftCodeExchange for OfflineExchange {
        fn exchange_code(
            &self,
            _code: &str,
            _verifier: &ConnectorSecret,
            _redirect_uri: &str,
            _requested_scopes: &[String],
        ) -> Result<ConnectorOAuthExchange, String> {
            Err("offline Microsoft slice does not exchange a real code".to_string())
        }
    }

    fn account() -> ConnectorAccount {
        let now = Utc::now();
        ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: MICROSOFT_PROVIDER_ID.to_string(),
            display_name: "Microsoft test account".to_string(),
            tenant_ref: Some("tenant:test".to_string()),
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: MICROSOFT_READ_CAPABILITIES.to_vec(),
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        }
    }

    fn credential_account(
        expires_at: DateTime<Utc>,
    ) -> (
        Arc<ConnectorRuntime<FakeConnectorCredentialStore>>,
        ConnectorAccount,
    ) {
        let access = ConnectorSecret::new("access-token-old".to_string()).unwrap();
        let refresh = ConnectorSecret::new("refresh-token-marker".to_string()).unwrap();
        let stored = MicrosoftCredentialEnvelope::new(
            &access,
            &refresh,
            expires_at,
            vec!["Mail.Read".to_string(), "User.Read".to_string()],
        )
        .encode()
        .expect("credential encodes");
        let mut store = FakeConnectorCredentialStore::default();
        let handle = store.put(stored).expect("credential stores");
        let mut account = account();
        account.credential_handle = handle;
        (Arc::new(ConnectorRuntime::new(store)), account)
    }

    struct CountingMicrosoftRefresher {
        calls: AtomicUsize,
    }

    impl MicrosoftTokenRefresher for CountingMicrosoftRefresher {
        fn refresh(
            &self,
            refresh_token: &ConnectorSecret,
            access_scopes: &[String],
        ) -> Result<MicrosoftRefreshedCredential, MicrosoftTokenFailure> {
            assert_eq!(refresh_token.expose(), "refresh-token-marker");
            assert_eq!(access_scopes, ["Mail.Read", "User.Read"]);
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            MicrosoftRefreshedCredential::new(
                ConnectorSecret::new("access-token-new".to_string())
                    .map_err(|_| MicrosoftTokenFailure::InvalidResponse)?,
                None,
                Utc::now() + Duration::hours(1),
                access_scopes.to_vec(),
            )
            .map_err(|_| MicrosoftTokenFailure::InvalidResponse)
        }
    }

    #[test]
    fn microsoft_access_token_resolution_is_typed_and_single_flight() {
        let (runtime, account) = credential_account(Utc::now() - Duration::minutes(1));
        let refresher = Arc::new(CountingMicrosoftRefresher {
            calls: AtomicUsize::new(0),
        });
        let resolver = Arc::new(MicrosoftAccessTokenResolver::new(
            Arc::clone(&runtime),
            Arc::clone(&refresher),
        ));
        let threads = (0..4)
            .map(|_| {
                let resolver = Arc::clone(&resolver);
                let account = account.clone();
                std::thread::spawn(move || {
                    let token = resolver
                        .resolve(&ConnectorHttpAuthContext::for_account(&account))
                        .expect("access token resolves");
                    assert_eq!(token.expose(), "access-token-new");
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().expect("resolver thread finishes");
        }
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
        let token = resolver
            .resolve(&ConnectorHttpAuthContext::for_account(&account))
            .expect("refreshed access token resolves");
        assert_eq!(token.expose(), "access-token-new");
        assert_ne!(token.expose(), "refresh-token-marker");
    }

    struct FailingMicrosoftRefresher(MicrosoftTokenFailure);

    impl MicrosoftTokenRefresher for FailingMicrosoftRefresher {
        fn refresh(
            &self,
            _refresh_token: &ConnectorSecret,
            _access_scopes: &[String],
        ) -> Result<MicrosoftRefreshedCredential, MicrosoftTokenFailure> {
            Err(self.0)
        }
    }

    #[test]
    fn microsoft_refresh_preserves_transient_timeout_and_invalid_grant_classes() {
        let cases = [
            (
                MicrosoftTokenFailure::Timeout,
                ConnectorHttpFailure::Timeout,
            ),
            (
                MicrosoftTokenFailure::Transient,
                ConnectorHttpFailure::Network,
            ),
            (
                MicrosoftTokenFailure::CredentialUnavailable,
                ConnectorHttpFailure::CredentialUnavailable,
            ),
        ];
        for (token_failure, expected) in cases {
            let (runtime, account) = credential_account(Utc::now() - Duration::minutes(1));
            let resolver = MicrosoftAccessTokenResolver::new(
                runtime,
                FailingMicrosoftRefresher(token_failure),
            );
            assert_eq!(
                resolver
                    .resolve(&ConnectorHttpAuthContext::for_account(&account))
                    .err(),
                Some(expected)
            );
        }
    }

    #[test]
    fn microsoft_attachment_metadata_is_bounded_and_never_requests_content_bytes() {
        let transport = Arc::new(ScriptedConnectorHttpTransport::new(vec![Ok(
            json_response(
                200,
                json!({
                    "@odata.type": "#microsoft.graph.fileAttachment",
                    "id": "attachment/one",
                    "name": "report.pdf",
                    "contentType": "application/pdf",
                    "size": 4096,
                    "isInline": false,
                    "contentBytes": "provider-secret-content-marker"
                }),
            ),
        )]));
        let adapter = MicrosoftGraphAdapter::new(Arc::clone(&transport));
        let mut account = account();
        account
            .granted_capabilities
            .push(ConnectorCapability::MailReadAttachment);
        let metadata = adapter
            .attachment_metadata(&account, "message/one", "attachment/one")
            .expect("attachment metadata normalizes");
        assert_eq!(metadata.account_id, account.id);
        assert_eq!(metadata.file_name, "report.pdf");
        assert_eq!(metadata.size_bytes, 4096);
        assert!(metadata.untrusted_evidence);
        let requests = transport.take_requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0]
            .url
            .contains("/me/messages/message%2Fone/attachments/attachment%2Fone"));
        assert!(requests[0]
            .url
            .contains("%24select=id%2Cname%2CcontentType%2Csize%2CisInline%2C%40odata.type"));
        assert!(!requests[0].url.contains("contentBytes"));
        assert!(!serde_json::to_string(&metadata.file_name)
            .unwrap()
            .contains("provider-secret-content-marker"));
    }

    fn mail_page(next_link: Option<&str>) -> serde_json::Value {
        let mut value = json!({
            "value": [{
                "id": "message-1",
                "conversationId": "thread-1",
                "sender": {"emailAddress": {"name": "Sender", "address": "sender@example.com"}},
                "toRecipients": [{"emailAddress": {"name": "Recipient", "address": "recipient@example.com"}}],
                "subject": "Untrusted subject",
                "receivedDateTime": "2026-07-12T01:02:03Z",
                "bodyPreview": "Untrusted mail evidence",
                "hasAttachments": true
            }]
        });
        if let Some(link) = next_link {
            value["@odata.nextLink"] = json!(link);
        }
        value
    }

    fn calendar_page() -> serde_json::Value {
        json!({
            "value": [{
                "id": "event-1",
                "subject": "Untrusted meeting",
                "start": {"dateTime": "2026-07-13T01:00:00", "timeZone": "UTC"},
                "end": {"dateTime": "2026-07-13T02:00:00", "timeZone": "UTC"},
                "attendees": [{
                    "emailAddress": {"name": "Guest", "address": "guest@example.com"},
                    "status": {"response": "accepted"}
                }],
                "onlineMeeting": {"joinUrl": "https://teams.example/join"},
                "recurrence": {"pattern": {"type": "weekly"}}
            }]
        })
    }

    fn mail_delta_page(
        next_link: Option<&str>,
        delta_link: Option<&str>,
        include_deleted: bool,
    ) -> serde_json::Value {
        let mut value = mail_page(None);
        if include_deleted {
            value["value"]
                .as_array_mut()
                .expect("mail values")
                .push(json!({"id": "message-deleted", "@removed": {"reason": "deleted"}}));
        }
        if let Some(link) = next_link {
            value["@odata.nextLink"] = json!(link);
        }
        if let Some(link) = delta_link {
            value["@odata.deltaLink"] = json!(link);
        }
        value
    }

    fn calendar_delta_page(delta_link: &str) -> serde_json::Value {
        let mut value = calendar_page();
        value["@odata.deltaLink"] = json!(delta_link);
        value
    }

    #[test]
    fn microsoft_scope_mapping_and_authority_are_exact_and_minimal() {
        let provider = MicrosoftOAuthProvider::new(OfflineExchange);
        let mut credentials = FakeConnectorCredentialStore::default();
        let session = begin_authorization(
            &mut credentials,
            &provider,
            vec![
                ConnectorCapability::MailSearch,
                ConnectorCapability::CalendarListEvents,
            ],
            "http://127.0.0.1:43821/callback".to_string(),
            Utc::now(),
        )
        .expect("authorization begins");
        assert_eq!(
            session.requested_scopes,
            vec!["Calendars.Read", "Mail.Read", "User.Read", "offline_access"]
        );
        let url = authorization_url("00000000-0000-4000-8000-000000000001", &session, Utc::now())
            .expect("authorization URL builds");
        let url = reqwest::Url::parse(&url).expect("authorization URL parses");
        assert_eq!(url.host_str(), Some("login.microsoftonline.com"));
        assert_eq!(url.path(), "/organizations/oauth2/v2.0/authorize");
        assert!(url
            .query_pairs()
            .any(|(key, value)| key == "code_challenge_method" && value == "S256"));
        assert!(!url.query_pairs().any(|(key, _)| key == "client_secret"));
        assert!(provider
            .validate_granted_scopes(
                &session.requested_scopes,
                &[
                    "User.Read".to_string(),
                    "Mail.Read".to_string(),
                    "Calendars.Read".to_string(),
                ],
            )
            .is_ok());
        assert!(provider
            .validate_granted_scopes(
                &session.requested_scopes,
                &["User.Read".to_string(), "Mail.Read".to_string()],
            )
            .is_err());
        assert!(provider
            .validate_granted_scopes(
                &session.requested_scopes,
                &[
                    "User.Read".to_string(),
                    "Mail.Read".to_string(),
                    "Calendars.Read".to_string(),
                    "Mail.Send".to_string(),
                ],
            )
            .is_err());

        let mut tampered = session.clone();
        tampered
            .requested_scopes
            .push("Files.ReadWrite.All".to_string());
        assert!(authorization_url(
            "00000000-0000-4000-8000-000000000001",
            &tampered,
            Utc::now(),
        )
        .is_err());

        let mut tampered = session.clone();
        tampered.redirect_uri = "http://127.0.0.1:80@evil.example/callback".to_string();
        assert!(authorization_url(
            "00000000-0000-4000-8000-000000000001",
            &tampered,
            Utc::now(),
        )
        .is_err());

        let mut tampered = session;
        tampered.state = "short".to_string();
        assert!(authorization_url(
            "00000000-0000-4000-8000-000000000001",
            &tampered,
            Utc::now(),
        )
        .is_err());

        let mut expired = tampered;
        expired.state = Uuid::new_v4().simple().to_string();
        expired.expires_at = Utc::now() - Duration::seconds(1);
        assert!(
            authorization_url("00000000-0000-4000-8000-000000000001", &expired, Utc::now(),)
                .is_err()
        );

        assert!(begin_authorization(
            &mut credentials,
            &provider,
            vec![ConnectorCapability::MailSendDraft],
            "http://127.0.0.1:43821/callback".to_string(),
            Utc::now(),
        )
        .is_err());
    }

    #[test]
    fn microsoft_typed_mail_calendar_contract_is_bounded_and_untrusted() {
        let mail_delta = "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=contract-mail";
        let calendar_delta =
            "https://graph.microsoft.com/v1.0/me/calendarView/delta?$deltatoken=contract-calendar";
        let transport = Arc::new(ScriptedConnectorHttpTransport::new(vec![
            Ok(json_response(
                200,
                json!({
                    "id": "account-1",
                    "displayName": "Test Person",
                    "mail": "person@example.com",
                    "userPrincipalName": "person@example.com"
                }),
            )),
            Ok(json_response(200, mail_page(None))),
            Ok(json_response(200, mail_page(None))),
            Ok(json_response(200, calendar_page())),
            Ok(json_response(
                200,
                mail_delta_page(None, Some(mail_delta), false),
            )),
            Ok(json_response(200, calendar_delta_page(calendar_delta))),
        ]));
        let adapter = MicrosoftGraphAdapter::new(Arc::clone(&transport));
        let account = account();
        let mut coverage =
            validate_provider_contract(&adapter, &account).expect("metadata contract passes");
        let profile = adapter
            .discover_account(&account)
            .expect("account discovery succeeds");
        assert_eq!(profile.primary_address.address, "person@example.com");
        let mail_search = MailSearchRequest::new("urgent".to_string(), 10).unwrap();
        let thread_read = MailThreadRequest::new("thread-1".to_string(), 20).unwrap();
        let starts_at = DateTime::parse_from_rfc3339("2026-07-13T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let calendar_list =
            CalendarListRequest::new(starts_at, starts_at + Duration::days(1), 20).unwrap();
        validate_mail_read_contract(
            &adapter,
            &account,
            &mail_search,
            &thread_read,
            &mut coverage,
        )
        .expect("typed provider contract passes");
        validate_calendar_read_contract(&adapter, &account, &calendar_list, &mut coverage)
            .expect("typed calendar contract passes");
        let mail_sync = MailSyncRequest::inbox(10).unwrap();
        validate_mail_sync_contract(&adapter, &account, &mail_sync, &mut coverage)
            .expect("typed mail sync contract passes");
        let calendar_sync =
            CalendarSyncRequest::new(starts_at, starts_at + Duration::days(1), 10).unwrap();
        validate_calendar_sync_contract(&adapter, &account, &calendar_sync, &mut coverage)
            .expect("typed calendar sync contract passes");
        coverage.finish().expect("all capabilities are covered");
        let requests = transport.take_requests();
        let auth_contexts = transport.take_auth_contexts();
        assert_eq!(requests.len(), 6);
        assert_eq!(auth_contexts.len(), requests.len());
        assert!(auth_contexts.iter().all(|auth| {
            auth.account_id() == account.id
                && auth.credential_handle() == &account.credential_handle
        }));
        assert!(requests.iter().all(|request| {
            reqwest::Url::parse(&request.url)
                .ok()
                .and_then(|url| url.host_str().map(ToString::to_string))
                .as_deref()
                == Some("graph.microsoft.com")
        }));
        assert!(requests.iter().all(|request| request.body.is_none()));
        assert!(requests
            .iter()
            .all(|request| request.max_response_bytes == MAX_GRAPH_RESPONSE_BYTES));
    }

    #[test]
    fn microsoft_mail_search_follows_only_bounded_validated_read_continuations() {
        let next = "https://graph.microsoft.com/v1.0/me/messages?$skiptoken=opaque-page-2";
        let transport = Arc::new(ScriptedConnectorHttpTransport::new(vec![
            Ok(json_response(200, mail_page(Some(next)))),
            Ok(json_response(200, mail_page(None))),
        ]));
        let adapter = MicrosoftGraphAdapter::new(Arc::clone(&transport));
        let request = MailSearchRequest::new("urgent".to_string(), 10).unwrap();
        let threads = crate::kernel::connectors::provider::collect_mail_search(
            &adapter,
            &account(),
            &request,
        )
        .expect("validated Graph continuation completes");
        assert_eq!(threads.len(), 2);

        let hostile = ConnectorReadContinuation::new(
            "https://evil.example/v1.0/me/messages?$skiptoken=secret".to_string(),
        )
        .unwrap();
        assert_eq!(
            adapter.search_mail_page(&account(), &request, Some(&hostile)),
            Err(ConnectorProviderFailure::InvalidResponse)
        );
    }

    #[test]
    fn microsoft_mail_delta_resumes_after_restart_and_commits_final_cursor_atomically() {
        let next_link = "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$skiptoken=marker-next-secret";
        let delta_link = "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=marker-delta-secret";
        let transport = Arc::new(ScriptedConnectorHttpTransport::new(vec![
            Ok(json_response(
                200,
                mail_delta_page(Some(next_link), None, false),
            )),
            Ok(json_response(
                200,
                mail_delta_page(None, Some(delta_link), true),
            )),
        ]));
        let adapter = MicrosoftGraphAdapter::new(Arc::clone(&transport));
        let account = account();
        let request = MailSyncRequest::inbox(10).expect("sync request builds");
        let stream = request.stream_fingerprint(MICROSOFT_PROVIDER_ID);
        let initial = ConnectorSyncState::initial(
            account.id,
            ConnectorCapability::MailSyncInbox,
            stream.clone(),
            Utc::now(),
        )
        .expect("sync state starts");
        let first_page = adapter
            .sync_mail_page(&account, &request, None)
            .expect("first delta page reads");
        assert!(matches!(
            first_page.continuation(),
            ConnectorSyncContinuation::Next(_)
        ));

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("microsoft-sync.sqlite3");
        let first_state = {
            let store = EventStore::open(&path).expect("event store opens");
            store
                .upsert_connector_account(&account)
                .expect("account persists");
            store
                .commit_connector_sync_page(
                    &initial,
                    &first_page,
                    |message| message.remote_ref.as_str(),
                    Utc::now(),
                )
                .expect("first page commits")
        };
        assert!(first_state.has_resume_page());
        assert!(!first_state.has_committed_delta());

        let store = EventStore::open(&path).expect("event store reopens");
        let resumed = store
            .connector_sync_state(account.id, ConnectorCapability::MailSyncInbox, &stream)
            .expect("sync state reads")
            .expect("sync state exists");
        assert!(resumed == first_state);
        let final_page = adapter
            .sync_mail_page(&account, &request, resumed.locator())
            .expect("final delta page reads");
        let completed = store
            .commit_connector_sync_page(
                &resumed,
                &final_page,
                |message| message.remote_ref.as_str(),
                Utc::now(),
            )
            .expect("final page commits");
        assert!(!completed.has_resume_page());
        assert!(completed.has_committed_delta());
        assert_eq!(completed.revision(), 2);
        assert!(store
            .commit_connector_sync_page(
                &initial,
                &first_page,
                |message| message.remote_ref.as_str(),
                Utc::now(),
            )
            .is_err());
        let summaries = store
            .connector_sync_projection_summaries(
                account.id,
                ConnectorCapability::MailSyncInbox,
                &stream,
            )
            .expect("projection summaries read");
        assert_eq!(summaries.len(), 2);
        assert!(summaries
            .iter()
            .any(|item| item.remote_ref == "message-deleted" && item.deleted));
        let events = serde_json::to_string(&store.list_recent(20).expect("events read"))
            .expect("events serialize");
        assert!(!events.contains("marker-next-secret"));
        assert!(!events.contains("marker-delta-secret"));

        let requests = transport.take_requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].url.contains("marker-next-secret"));
    }

    #[test]
    fn microsoft_calendar_delta_is_separate_from_bounded_calendar_list() {
        let delta_link =
            "https://graph.microsoft.com/v1.0/me/calendarView/delta?$deltatoken=calendar-marker";
        let transport = ScriptedConnectorHttpTransport::new(vec![Ok(json_response(
            200,
            calendar_delta_page(delta_link),
        ))]);
        let adapter = MicrosoftGraphAdapter::new(transport);
        let starts_at = DateTime::parse_from_rfc3339("2026-07-13T00:00:00Z")
            .expect("fixed calendar start parses")
            .with_timezone(&Utc);
        let request = CalendarSyncRequest::new(starts_at, starts_at + Duration::days(1), 10)
            .expect("calendar sync request builds");
        let page = adapter
            .sync_calendar_page(&account(), &request, None)
            .expect("calendar delta page reads");
        assert_eq!(page.changes().len(), 1);
        assert!(matches!(
            page.continuation(),
            ConnectorSyncContinuation::Delta(_)
        ));
    }

    #[test]
    fn microsoft_delta_rejects_ambiguous_or_over_budget_pages() {
        let next_link =
            "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$skiptoken=next";
        let delta_link = "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages/delta?$deltatoken=delta";
        let ambiguous = ScriptedConnectorHttpTransport::new(vec![Ok(json_response(
            200,
            mail_delta_page(Some(next_link), Some(delta_link), false),
        ))]);
        let adapter = MicrosoftGraphAdapter::new(ambiguous);
        let request = MailSyncRequest::inbox(10).expect("sync request builds");
        assert!(matches!(
            adapter.sync_mail_page(&account(), &request, None),
            Err(ConnectorProviderFailure::InvalidResponse)
        ));

        let missing = ScriptedConnectorHttpTransport::new(vec![Ok(json_response(
            200,
            mail_delta_page(None, None, false),
        ))]);
        let adapter = MicrosoftGraphAdapter::new(missing);
        assert!(matches!(
            adapter.sync_mail_page(&account(), &request, None),
            Err(ConnectorProviderFailure::InvalidResponse)
        ));

        let mut over_budget = mail_delta_page(None, Some(delta_link), false);
        let duplicate = over_budget["value"][0].clone();
        over_budget["value"]
            .as_array_mut()
            .expect("mail values")
            .push(duplicate);
        let transport =
            ScriptedConnectorHttpTransport::new(vec![Ok(json_response(200, over_budget))]);
        let adapter = MicrosoftGraphAdapter::new(transport);
        let request = MailSyncRequest::inbox(1).expect("sync request builds");
        assert!(matches!(
            adapter.sync_mail_page(&account(), &request, None),
            Err(ConnectorProviderFailure::InvalidResponse)
        ));

        let mut malformed_removed = mail_delta_page(None, Some(delta_link), false);
        malformed_removed["value"] = json!([{"id": "message-deleted", "@removed": true}]);
        let transport =
            ScriptedConnectorHttpTransport::new(vec![Ok(json_response(200, malformed_removed))]);
        let adapter = MicrosoftGraphAdapter::new(transport);
        let request = MailSyncRequest::inbox(10).expect("sync request builds");
        assert!(matches!(
            adapter.sync_mail_page(&account(), &request, None),
            Err(ConnectorProviderFailure::InvalidResponse)
        ));

        let outside_start = DateTime::parse_from_rfc3339("2026-07-14T00:00:00Z")
            .expect("time parses")
            .with_timezone(&Utc);
        let list_transport =
            ScriptedConnectorHttpTransport::new(vec![Ok(json_response(200, calendar_page()))]);
        let list_adapter = MicrosoftGraphAdapter::new(list_transport);
        let list_request =
            CalendarListRequest::new(outside_start, outside_start + Duration::days(1), 10)
                .expect("calendar list request builds");
        assert!(matches!(
            list_adapter.list_events(&account(), &list_request),
            Err(ConnectorProviderFailure::InvalidResponse)
        ));

        let calendar_delta_link =
            "https://graph.microsoft.com/v1.0/me/calendarView/delta?$deltatoken=outside";
        let delta_transport = ScriptedConnectorHttpTransport::new(vec![Ok(json_response(
            200,
            calendar_delta_page(calendar_delta_link),
        ))]);
        let delta_adapter = MicrosoftGraphAdapter::new(delta_transport);
        let delta_request =
            CalendarSyncRequest::new(outside_start, outside_start + Duration::days(1), 10)
                .expect("calendar sync request builds");
        assert!(matches!(
            delta_adapter.sync_calendar_page(&account(), &delta_request, None),
            Err(ConnectorProviderFailure::InvalidResponse)
        ));
    }

    #[test]
    fn microsoft_sync_revalidates_persisted_continuation_before_transport() {
        let store = EventStore::open_memory().expect("event store opens");
        let mut account = account();
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let request = MailSyncRequest::inbox(10).expect("request builds");
        let stream = request.stream_fingerprint(MICROSOFT_PROVIDER_ID);
        let initial = ConnectorSyncState::initial(
            account.id,
            ConnectorCapability::MailSyncInbox,
            stream,
            Utc::now(),
        )
        .expect("state starts");
        let tampered: ConnectorSyncPage<MailMessage> = ConnectorSyncPage::new(
            Vec::new(),
            ConnectorSyncContinuation::Next(
                ConnectorOpaqueContinuation::new(
                    "https://evil.example/v1.0/me/mailFolders/inbox/messages/delta?$skiptoken=marker"
                        .to_string(),
                )
                .expect("opaque state accepts provider-owned text"),
            ),
        );
        store
            .commit_connector_sync_page(
                &initial,
                &tampered,
                |message| message.remote_ref.as_str(),
                Utc::now(),
            )
            .expect("tampered persistence fixture commits");
        let transport = Arc::new(ScriptedConnectorHttpTransport::new(Vec::new()));
        let adapter = MicrosoftGraphAdapter::new(Arc::clone(&transport));
        assert!(matches!(
            crate::kernel::connectors::sync::run_mail_sync_step(
                &store,
                &adapter,
                &mut account,
                &request,
                Utc::now(),
            ),
            Ok(super::super::sync::ConnectorSyncStep::Deferred { .. })
        ));
        assert!(transport.take_requests().is_empty());
    }

    #[test]
    fn microsoft_rejects_hostile_continuation_and_disconnected_account_before_transport() {
        let request = MailSearchRequest::new("urgent".to_string(), 10).unwrap();
        for link in [
            "http://graph.microsoft.com/v1.0/me/messages?$skiptoken=secret",
            "https://evil.example/v1.0/me/messages?$skiptoken=secret",
            "https://graph.microsoft.com@evil.example/v1.0/me/messages?$skiptoken=secret",
            "https://graph.microsoft.com/v1.0/me/messages#secret",
            "https://graph.microsoft.com/v1.0/me/messages-extra?$skiptoken=secret",
        ] {
            let hostile = Arc::new(ScriptedConnectorHttpTransport::new(vec![Ok(
                json_response(200, mail_page(Some(link))),
            )]));
            let adapter = MicrosoftGraphAdapter::new(Arc::clone(&hostile));
            assert_eq!(
                adapter.search_mail(&account(), &request),
                Err(ConnectorProviderFailure::InvalidResponse)
            );
        }

        let mut malformed_link = mail_page(None);
        malformed_link["@odata.nextLink"] = json!({"url": "https://graph.microsoft.com"});
        let malformed =
            ScriptedConnectorHttpTransport::new(vec![Ok(json_response(200, malformed_link))]);
        let adapter = MicrosoftGraphAdapter::new(malformed);
        assert_eq!(
            adapter.search_mail(&account(), &request),
            Err(ConnectorProviderFailure::InvalidResponse)
        );

        let mut malformed_link = mail_page(None);
        malformed_link["@odata.deltaLink"] = json!(42);
        let malformed =
            ScriptedConnectorHttpTransport::new(vec![Ok(json_response(200, malformed_link))]);
        let adapter = MicrosoftGraphAdapter::new(malformed);
        assert_eq!(
            adapter.search_mail(&account(), &request),
            Err(ConnectorProviderFailure::InvalidResponse)
        );

        let untouched = Arc::new(ScriptedConnectorHttpTransport::new(vec![Ok(
            json_response(200, mail_page(None)),
        )]));
        let adapter = MicrosoftGraphAdapter::new(Arc::clone(&untouched));
        let mut disconnected = account();
        disconnected.health = ConnectorHealth::Disconnected;
        assert_eq!(
            adapter.search_mail(&disconnected, &request),
            Err(ConnectorProviderFailure::PermissionDenied)
        );
        assert!(untouched.take_requests().is_empty());
    }

    #[test]
    fn microsoft_normalizes_http_failures_without_provider_body() {
        for (failure, expected) in [
            (
                ConnectorHttpFailure::CredentialUnavailable,
                ConnectorProviderFailure::AuthorizationExpired,
            ),
            (
                ConnectorHttpFailure::InvalidRequest,
                ConnectorProviderFailure::InvalidResponse,
            ),
            (
                ConnectorHttpFailure::ResponseTooLarge,
                ConnectorProviderFailure::InvalidResponse,
            ),
            (
                ConnectorHttpFailure::Timeout,
                ConnectorProviderFailure::NetworkUnavailable,
            ),
        ] {
            let adapter =
                MicrosoftGraphAdapter::new(ScriptedConnectorHttpTransport::new(vec![Err(failure)]));
            let request = MailSearchRequest::new("urgent".to_string(), 10).unwrap();
            assert_eq!(adapter.search_mail(&account(), &request), Err(expected));
        }

        let cases = [
            (401, ConnectorProviderFailure::AuthorizationExpired),
            (403, ConnectorProviderFailure::PermissionDenied),
            (404, ConnectorProviderFailure::RemoteNotFound),
            (410, ConnectorProviderFailure::CursorExpired),
            (500, ConnectorProviderFailure::NetworkUnavailable),
        ];
        for (status, expected) in cases {
            let transport = ScriptedConnectorHttpTransport::new(vec![Ok(json_response(
                status,
                json!({"error": {"message": "marker-secret-provider-body"}}),
            ))]);
            let adapter = MicrosoftGraphAdapter::new(transport);
            let request = MailSearchRequest::new("urgent".to_string(), 10).unwrap();
            assert_eq!(adapter.search_mail(&account(), &request), Err(expected));
        }

        let mut throttled = ConnectorHttpResponse {
            status: 429,
            headers: BTreeMap::new(),
            body: serde_json::to_vec(&json!({"error": "marker-secret"})).unwrap(),
        };
        throttled
            .headers
            .insert("Retry-After".to_string(), "9999".to_string());
        let adapter =
            MicrosoftGraphAdapter::new(ScriptedConnectorHttpTransport::new(vec![Ok(throttled)]));
        let request = MailSearchRequest::new("urgent".to_string(), 10).unwrap();
        assert_eq!(
            adapter.search_mail(&account(), &request),
            Err(ConnectorProviderFailure::RateLimited {
                retry_after_seconds: Some(900)
            })
        );

        let adapter = MicrosoftGraphAdapter::new(ScriptedConnectorHttpTransport::new(vec![Ok(
            ConnectorHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: b"not-json".to_vec(),
            },
        )]));
        assert_eq!(
            adapter.search_mail(&account(), &request),
            Err(ConnectorProviderFailure::InvalidResponse)
        );

        let adapter = MicrosoftGraphAdapter::new(ScriptedConnectorHttpTransport::new(vec![Ok(
            ConnectorHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: vec![b'x'; MAX_GRAPH_RESPONSE_BYTES + 1],
            },
        )]));
        assert_eq!(
            adapter.search_mail(&account(), &request),
            Err(ConnectorProviderFailure::InvalidResponse)
        );
    }
}
