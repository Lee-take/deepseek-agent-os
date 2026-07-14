use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use super::{
    ConnectorAccount, ConnectorCapability, ConnectorCredentialHandle, ConnectorCredentialStore,
    ConnectorRuntime, ConnectorSecret,
};
use crate::kernel::event_store::ConnectorAuthorizationResolution;
use crate::kernel::event_store::{ConnectorAuthorizationExchangeClaim, EventStore};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectorAuthorizationStatus {
    Preparing,
    Pending,
    Exchanging,
    Completed,
    RepairRequired,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectorAuthorizationIntent {
    Approve,
    Cancel,
}

fn default_authorization_status() -> ConnectorAuthorizationStatus {
    ConnectorAuthorizationStatus::Pending
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ConnectorAuthorizationSession {
    pub id: Uuid,
    pub provider_id: String,
    pub state: String,
    pub pkce_challenge: String,
    pub pkce_method: String,
    pub verifier_handle: ConnectorCredentialHandle,
    pub result_credential_handle: ConnectorCredentialHandle,
    pub requested_capabilities: Vec<ConnectorCapability>,
    pub requested_scopes: Vec<String>,
    pub redirect_uri: String,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
    #[serde(default = "default_authorization_status")]
    pub status: ConnectorAuthorizationStatus,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub cleanup_required: bool,
    #[serde(default)]
    pub cleanup_completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ConnectorAuthorizationResult {
    pub session_id: Uuid,
    pub provider_id: String,
    pub credential_handle: ConnectorCredentialHandle,
    pub granted_capabilities: Vec<ConnectorCapability>,
    pub granted_scopes: Vec<String>,
    pub completed_at: DateTime<Utc>,
}

pub(crate) struct ConnectorOAuthExchange {
    pub credential: ConnectorSecret,
    pub granted_scopes: Vec<String>,
}

pub(crate) struct ConnectorAuthorizationAccountProfile {
    display_name: String,
    tenant_ref: Option<String>,
}

impl ConnectorAuthorizationAccountProfile {
    pub(crate) fn new(display_name: String, tenant_ref: Option<String>) -> Result<Self, String> {
        let display_name = display_name.trim().to_string();
        let tenant_ref = tenant_ref
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if display_name.is_empty()
            || display_name.len() > 200
            || display_name.chars().any(char::is_control)
            || tenant_ref.as_ref().is_some_and(|value| value.len() > 200)
            || tenant_ref
                .as_ref()
                .is_some_and(|value| value.chars().any(char::is_control))
        {
            return Err("OAuth account profile is invalid".to_string());
        }
        Ok(Self {
            display_name,
            tenant_ref,
        })
    }
}

impl ConnectorOAuthExchange {
    pub(crate) fn new(
        credential: ConnectorSecret,
        granted_scopes: Vec<String>,
    ) -> Result<Self, String> {
        let mut granted_scopes = granted_scopes
            .into_iter()
            .map(|scope| scope.trim().to_string())
            .collect::<Vec<_>>();
        if granted_scopes.is_empty() || granted_scopes.iter().any(String::is_empty) {
            return Err("OAuth granted scope is invalid".to_string());
        }
        granted_scopes.sort();
        granted_scopes.dedup();
        Ok(Self {
            credential,
            granted_scopes,
        })
    }
}

pub(crate) trait ConnectorOAuthProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn base_scopes(&self) -> &'static [&'static str] {
        &[]
    }
    fn scopes_for(&self, capability: ConnectorCapability) -> Option<&'static [&'static str]>;
    fn validate_granted_scopes(
        &self,
        requested_scopes: &[String],
        granted_scopes: &[String],
    ) -> Result<(), String> {
        if requested_scopes == granted_scopes {
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
    ) -> Result<ConnectorOAuthExchange, String>;
    fn complete_review(
        &self,
        _verifier: &ConnectorSecret,
        _redirect_uri: &str,
        _requested_scopes: &[String],
    ) -> Result<ConnectorOAuthExchange, String> {
        Err("OAuth review completion is unavailable".to_string())
    }
    fn account_profile(
        &self,
        _exchange: &ConnectorOAuthExchange,
    ) -> Result<ConnectorAuthorizationAccountProfile, String> {
        Err("OAuth account discovery is unavailable".to_string())
    }
}

pub(crate) struct ConnectorAuthorizationProviderCompletion {
    exchange: ConnectorOAuthExchange,
    account_profile: ConnectorAuthorizationAccountProfile,
}

pub(crate) fn execute_claimed_authorization_with_runtime<S: ConnectorCredentialStore + Send>(
    runtime: &ConnectorRuntime<S>,
    provider: &dyn ConnectorOAuthProvider,
    claim: &ConnectorAuthorizationExchangeClaim,
    code: &str,
) -> Result<ConnectorAuthorizationProviderCompletion, String> {
    let session = claim.session();
    if code.trim().is_empty()
        || session.status != ConnectorAuthorizationStatus::Exchanging
        || session.provider_id != provider.provider_id()
    {
        return Err("OAuth callback did not match an active authorization session".to_string());
    }
    let verifier = runtime
        .read_authorization_verifier(&session.verifier_handle)
        .map_err(|_| "OAuth authorization verifier is unavailable".to_string())?;
    let exchange = provider
        .exchange_code(
            code,
            &verifier,
            &session.redirect_uri,
            &session.requested_scopes,
        )
        .map_err(|_| "OAuth token exchange failed".to_string())?;
    provider
        .validate_granted_scopes(&session.requested_scopes, &exchange.granted_scopes)
        .map_err(|_| "OAuth token scopes did not match the approved request".to_string())?;
    let account_profile = provider
        .account_profile(&exchange)
        .map_err(|_| "OAuth account discovery failed".to_string())?;
    Ok(ConnectorAuthorizationProviderCompletion {
        exchange,
        account_profile,
    })
}

pub(crate) fn execute_review_authorization_with_runtime<S: ConnectorCredentialStore + Send>(
    runtime: &ConnectorRuntime<S>,
    provider: &dyn ConnectorOAuthProvider,
    claim: &ConnectorAuthorizationExchangeClaim,
) -> Result<ConnectorAuthorizationProviderCompletion, String> {
    let session = claim.session();
    if session.status != ConnectorAuthorizationStatus::Exchanging
        || session.provider_id != provider.provider_id()
    {
        return Err("OAuth review did not match an active authorization session".to_string());
    }
    let verifier = runtime
        .read_authorization_verifier(&session.verifier_handle)
        .map_err(|_| "OAuth authorization verifier is unavailable".to_string())?;
    let exchange = provider
        .complete_review(&verifier, &session.redirect_uri, &session.requested_scopes)
        .map_err(|_| "OAuth review completion failed".to_string())?;
    provider
        .validate_granted_scopes(&session.requested_scopes, &exchange.granted_scopes)
        .map_err(|_| "OAuth token scopes did not match the approved request".to_string())?;
    let account_profile = provider
        .account_profile(&exchange)
        .map_err(|_| "OAuth account discovery failed".to_string())?;
    Ok(ConnectorAuthorizationProviderCompletion {
        exchange,
        account_profile,
    })
}

pub(crate) fn finalize_claimed_authorization_with_runtime<S: ConnectorCredentialStore + Send>(
    event_store: &EventStore,
    runtime: &ConnectorRuntime<S>,
    claim: ConnectorAuthorizationExchangeClaim,
    completion: ConnectorAuthorizationProviderCompletion,
    completion_now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationResult, String> {
    let authorization_id = claim.session().id;
    runtime.with_authorization_fence(authorization_id, || {
        event_store
            .validate_connector_authorization_exchange_claim(&claim, completion_now)
            .map_err(|_| "OAuth authorization claim is no longer active".to_string())?;
        let (mut session, claim_id, claim_expires_at) = claim.into_parts();
        let granted_scopes = completion.exchange.granted_scopes.clone();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: session.provider_id.clone(),
            display_name: completion.account_profile.display_name,
            tenant_ref: completion.account_profile.tenant_ref,
            credential_handle: session.result_credential_handle.clone(),
            granted_capabilities: session.requested_capabilities.clone(),
            health: super::ConnectorHealth::Connected,
            connected_at: completion_now,
            updated_at: completion_now,
        };
        runtime
            .put_authorization_result(
                &session.result_credential_handle,
                completion.exchange.credential,
            )
            .map_err(|_| "OAuth authorization result could not be stored".to_string())?;
        runtime
            .delete_authorization_verifier(&session.verifier_handle)
            .map_err(|_| "OAuth authorization verifier could not be removed".to_string())?;
        session.consumed_at = Some(completion_now);
        session.status = ConnectorAuthorizationStatus::Completed;
        session.revision = session
            .revision
            .checked_add(1)
            .ok_or_else(|| "OAuth authorization revision overflowed".to_string())?;
        event_store
            .finish_connector_authorization_with_account(
                &session,
                &account,
                claim_id,
                claim_expires_at,
                completion_now,
            )
            .map_err(|_| "OAuth authorization state could not be finalized".to_string())?;
        Ok(ConnectorAuthorizationResult {
            session_id: session.id,
            provider_id: session.provider_id.clone(),
            credential_handle: session.result_credential_handle.clone(),
            granted_capabilities: session.requested_capabilities.clone(),
            granted_scopes,
            completed_at: completion_now,
        })
    })
}

pub(crate) fn finalize_claimed_authorization_with_shared_runtime<
    S: ConnectorCredentialStore + Send,
>(
    event_store: &Arc<Mutex<EventStore>>,
    runtime: &ConnectorRuntime<S>,
    claim: ConnectorAuthorizationExchangeClaim,
    completion: ConnectorAuthorizationProviderCompletion,
    completion_now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationResult, String> {
    let authorization_id = claim.session().id;
    runtime.with_authorization_fence(authorization_id, || {
        event_store
            .lock()
            .map_err(|_| "OAuth authorization state could not be loaded".to_string())?
            .validate_connector_authorization_exchange_claim(&claim, completion_now)
            .map_err(|_| "OAuth authorization claim is no longer active".to_string())?;
        let (mut session, claim_id, claim_expires_at) = claim.into_parts();
        let granted_scopes = completion.exchange.granted_scopes.clone();
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: session.provider_id.clone(),
            display_name: completion.account_profile.display_name,
            tenant_ref: completion.account_profile.tenant_ref,
            credential_handle: session.result_credential_handle.clone(),
            granted_capabilities: session.requested_capabilities.clone(),
            health: super::ConnectorHealth::Connected,
            connected_at: completion_now,
            updated_at: completion_now,
        };
        runtime
            .put_authorization_result(
                &session.result_credential_handle,
                completion.exchange.credential,
            )
            .map_err(|_| "OAuth authorization result could not be stored".to_string())?;
        runtime
            .delete_authorization_verifier(&session.verifier_handle)
            .map_err(|_| "OAuth authorization verifier could not be removed".to_string())?;
        session.consumed_at = Some(completion_now);
        session.status = ConnectorAuthorizationStatus::Completed;
        session.revision = session
            .revision
            .checked_add(1)
            .ok_or_else(|| "OAuth authorization revision overflowed".to_string())?;
        event_store
            .lock()
            .map_err(|_| "OAuth authorization state could not be finalized".to_string())?
            .finish_connector_authorization_with_account(
                &session,
                &account,
                claim_id,
                claim_expires_at,
                completion_now,
            )
            .map_err(|_| "OAuth authorization state could not be finalized".to_string())?;
        Ok(ConnectorAuthorizationResult {
            session_id: session.id,
            provider_id: session.provider_id.clone(),
            credential_handle: session.result_credential_handle.clone(),
            granted_capabilities: session.requested_capabilities.clone(),
            granted_scopes,
            completed_at: completion_now,
        })
    })
}

#[cfg(test)]
pub(crate) fn begin_authorization(
    store: &mut dyn ConnectorCredentialStore,
    provider: &dyn ConnectorOAuthProvider,
    requested_capabilities: Vec<ConnectorCapability>,
    redirect_uri: String,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationSession, String> {
    let (mut session, verifier) =
        prepare_authorization(provider, requested_capabilities, redirect_uri, now)?;
    store.put_new_at(&session.verifier_handle, verifier)?;
    session.status = ConnectorAuthorizationStatus::Pending;
    Ok(session)
}

pub(crate) fn begin_persisted_authorization(
    event_store: &EventStore,
    store: &mut dyn ConnectorCredentialStore,
    provider: &dyn ConnectorOAuthProvider,
    requested_capabilities: Vec<ConnectorCapability>,
    redirect_uri: String,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationSession, String> {
    let (session, verifier) =
        prepare_authorization(provider, requested_capabilities, redirect_uri, now)?;
    event_store
        .insert_preparing_connector_authorization(&session, now)
        .map_err(|_| "OAuth authorization state could not be prepared".to_string())?;
    if store
        .put_new_at(&session.verifier_handle, verifier)
        .is_err()
    {
        let _ = event_store.mark_connector_authorization_repair(session.id, now);
        return Err("OAuth authorization verifier could not be stored".to_string());
    }
    match event_store.activate_preparing_connector_authorization(session.id, now) {
        Ok(session) => Ok(session),
        Err(_) => {
            let _ = event_store.mark_connector_authorization_repair(session.id, now);
            Err("OAuth authorization state could not be activated".to_string())
        }
    }
}

pub(crate) fn resolve_persisted_authorization_review<S: ConnectorCredentialStore + Send>(
    event_store: &EventStore,
    runtime: &ConnectorRuntime<S>,
    review_id: Uuid,
    intent: ConnectorAuthorizationIntent,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationResolution, String> {
    let review = event_store
        .connector_authorization_active_review(review_id, now)
        .map_err(|_| "connector authorization review is unavailable".to_string())?;
    let authority = runtime
        .read_authorization_review_authority(review.authority_handle())
        .map_err(|_| "connector authorization review is unavailable".to_string())?;
    event_store
        .resolve_connector_authorization_review(review_id, &authority, intent, now)
        .map_err(|_| "connector authorization review is unavailable".to_string())
}

pub(crate) fn prepare_authorization(
    provider: &dyn ConnectorOAuthProvider,
    requested_capabilities: Vec<ConnectorCapability>,
    redirect_uri: String,
    now: DateTime<Utc>,
) -> Result<(ConnectorAuthorizationSession, ConnectorSecret), String> {
    if requested_capabilities.is_empty() {
        return Err("at least one connector capability is required".to_string());
    }
    let redirect_uri = validate_loopback_redirect_uri(&redirect_uri)?;
    let mut scopes = provider
        .base_scopes()
        .iter()
        .map(|scope| (*scope).to_string())
        .collect::<Vec<_>>();
    for capability in &requested_capabilities {
        let mapped = provider.scopes_for(*capability).ok_or_else(|| {
            "connector capability has no approved OAuth scope mapping".to_string()
        })?;
        for scope in mapped {
            if !scopes.iter().any(|existing| existing == scope) {
                scopes.push((*scope).to_string());
            }
        }
    }
    scopes.sort();
    let verifier_text = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let challenge = base64_url_sha256(verifier_text.as_bytes());
    let verifier = ConnectorSecret::new(verifier_text)?;
    Ok((
        ConnectorAuthorizationSession {
            id: Uuid::new_v4(),
            provider_id: provider.provider_id().to_string(),
            state: Uuid::new_v4().simple().to_string(),
            pkce_challenge: challenge,
            pkce_method: "S256".to_string(),
            verifier_handle: ConnectorCredentialHandle::new(),
            result_credential_handle: ConnectorCredentialHandle::new(),
            requested_capabilities,
            requested_scopes: scopes,
            redirect_uri,
            expires_at: now + Duration::minutes(10),
            consumed_at: None,
            status: ConnectorAuthorizationStatus::Preparing,
            revision: 0,
            cleanup_required: false,
            cleanup_completed_at: None,
        },
        verifier,
    ))
}

pub(super) fn validate_loopback_redirect_uri(value: &str) -> Result<String, String> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| "OAuth callback must be a valid loopback URL".to_string())?;
    let valid = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some_and(|port| port >= 1024)
        && url.path() == "/callback"
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none();
    if !valid {
        return Err("OAuth callback must use http://127.0.0.1:<port>/callback".to_string());
    }
    Ok(url.to_string())
}

pub(crate) fn complete_claimed_authorization_with_runtime<S: ConnectorCredentialStore + Send>(
    event_store: &EventStore,
    runtime: &ConnectorRuntime<S>,
    provider: &dyn ConnectorOAuthProvider,
    claim: ConnectorAuthorizationExchangeClaim,
    code: &str,
    completion_now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationResult, String> {
    let completion = execute_claimed_authorization_with_runtime(runtime, provider, &claim, code)?;
    finalize_claimed_authorization_with_runtime(
        event_store,
        runtime,
        claim,
        completion,
        completion_now,
    )
}

#[cfg(test)]
pub fn complete_authorization(
    event_store: &EventStore,
    store: &mut dyn ConnectorCredentialStore,
    provider: &dyn ConnectorOAuthProvider,
    session_id: Uuid,
    returned_state: &str,
    code: &str,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationResult, String> {
    if code.trim().is_empty() {
        return Err("OAuth callback did not match an active authorization session".to_string());
    }
    let session = event_store
        .claim_connector_authorization_session(session_id, returned_state, now)
        .map_err(|_| "OAuth callback did not match an active authorization session".to_string())?;
    complete_claimed_authorization(event_store, store, provider, session, code, now)
}

#[cfg(test)]
pub(crate) fn complete_claimed_authorization(
    event_store: &EventStore,
    store: &mut dyn ConnectorCredentialStore,
    provider: &dyn ConnectorOAuthProvider,
    claim: ConnectorAuthorizationExchangeClaim,
    code: &str,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationResult, String> {
    complete_claimed_authorization_inner(event_store, store, provider, claim, code, None, now)
}

#[cfg(test)]
pub(crate) fn complete_claimed_authorization_with_account(
    event_store: &EventStore,
    store: &mut dyn ConnectorCredentialStore,
    provider: &dyn ConnectorOAuthProvider,
    claim: ConnectorAuthorizationExchangeClaim,
    code: &str,
    account: &super::ConnectorAccount,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationResult, String> {
    complete_claimed_authorization_inner(
        event_store,
        store,
        provider,
        claim,
        code,
        Some(account),
        now,
    )
}

#[cfg(test)]
fn complete_claimed_authorization_inner(
    event_store: &EventStore,
    store: &mut dyn ConnectorCredentialStore,
    provider: &dyn ConnectorOAuthProvider,
    claim: ConnectorAuthorizationExchangeClaim,
    code: &str,
    account: Option<&super::ConnectorAccount>,
    now: DateTime<Utc>,
) -> Result<ConnectorAuthorizationResult, String> {
    let (mut session, claim_id, claim_expires_at) = claim.into_parts();
    if code.trim().is_empty() || session.status != ConnectorAuthorizationStatus::Exchanging {
        let _ = event_store.mark_connector_authorization_exchange_repair(
            session.id,
            claim_id,
            claim_expires_at,
            now,
        );
        return Err("OAuth callback did not match an active authorization session".to_string());
    }
    if session.provider_id != provider.provider_id() {
        let _ = event_store.mark_connector_authorization_exchange_repair(
            session.id,
            claim_id,
            claim_expires_at,
            now,
        );
        return Err("OAuth callback did not match an active authorization session".to_string());
    }
    let completed = (|| {
        let verifier = store.read(&session.verifier_handle)?;
        let exchange = provider
            .exchange_code(
                code,
                &verifier,
                &session.redirect_uri,
                &session.requested_scopes,
            )
            .map_err(|_| "OAuth token exchange failed".to_string())?;
        provider.validate_granted_scopes(&session.requested_scopes, &exchange.granted_scopes)?;
        store.put_new_at(&session.result_credential_handle, exchange.credential)?;
        store.delete(&session.verifier_handle)?;
        session.consumed_at = Some(now);
        session.status = ConnectorAuthorizationStatus::Completed;
        session.revision = session
            .revision
            .checked_add(1)
            .ok_or_else(|| "OAuth authorization revision overflowed".to_string())?;
        match account {
            Some(account) => event_store.finish_connector_authorization_with_account(
                &session,
                account,
                claim_id,
                claim_expires_at,
                now,
            ),
            None => event_store.finish_connector_authorization_session(
                &session,
                claim_id,
                claim_expires_at,
                now,
            ),
        }
        .map_err(|_| "OAuth authorization state could not be finalized".to_string())?;
        Ok(ConnectorAuthorizationResult {
            session_id: session.id,
            provider_id: session.provider_id.clone(),
            credential_handle: session.result_credential_handle.clone(),
            granted_capabilities: session.requested_capabilities.clone(),
            granted_scopes: exchange.granted_scopes,
            completed_at: now,
        })
    })();
    if completed.is_err() {
        let authoritative_completed = event_store
            .connector_authorization_session(session.id)
            .is_ok_and(|authoritative| {
                authoritative.status == ConnectorAuthorizationStatus::Completed
                    && authoritative.result_credential_handle == session.result_credential_handle
            });
        if !authoritative_completed
            && event_store
                .mark_connector_authorization_exchange_repair(
                    session.id,
                    claim_id,
                    claim_expires_at,
                    now,
                )
                .is_ok()
        {
            let _ = store.delete(&session.result_credential_handle);
        }
    }
    completed
}

pub fn recover_interrupted_authorization(
    event_store: &EventStore,
    store: &mut dyn ConnectorCredentialStore,
    session_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let session = event_store
        .connector_authorization_session(session_id)
        .map_err(|_| "OAuth authorization session is unavailable".to_string())?;
    if session.status != ConnectorAuthorizationStatus::Exchanging {
        return Ok(());
    }
    let session = event_store
        .begin_connector_authorization_cleanup(session.id, now)
        .map_err(|_| "OAuth authorization recovery could not persist".to_string())?;
    store.delete(&session.result_credential_handle)?;
    store.delete(&session.verifier_handle)?;
    event_store
        .finish_connector_authorization_cleanup(&session, now)
        .map_err(|_| "OAuth authorization recovery could not persist".to_string())?;
    Ok(())
}

pub(crate) fn recover_due_connector_authorizations<S: ConnectorCredentialStore + Send>(
    event_store: &EventStore,
    runtime: &ConnectorRuntime<S>,
    now: DateTime<Utc>,
    limit: usize,
) -> Result<usize, String> {
    let mut completed = 0usize;
    let candidates = event_store
        .connector_authorization_cleanup_candidates(now, limit)
        .map_err(|_| "OAuth authorization recovery could not load safely".to_string())?;
    for id in candidates {
        let recovered = runtime.with_authorization_fence(id, || {
            let claim = event_store
                .begin_connector_authorization_cleanup(id, now)
                .map_err(|_| "OAuth authorization recovery could not persist".to_string())?;
            runtime.delete_authorization_handles_and_review(
                claim.session(),
                claim.action_authority_handle(),
            )?;
            event_store
                .finish_connector_authorization_cleanup(&claim, now)
                .map_err(|_| "OAuth authorization recovery could not persist".to_string())?;
            Ok(())
        });
        if recovered.is_ok() {
            completed += 1;
        }
    }
    completed += recover_authorization_authority_cleanup(event_store, runtime, now, limit)?;
    Ok(completed)
}

fn recover_authorization_authority_cleanup<S: ConnectorCredentialStore + Send>(
    event_store: &EventStore,
    runtime: &ConnectorRuntime<S>,
    now: DateTime<Utc>,
    limit: usize,
) -> Result<usize, String> {
    let candidates = event_store
        .connector_authorization_authority_cleanup_candidates(now, limit)
        .map_err(|_| "OAuth authorization recovery could not load safely".to_string())?;
    let mut completed = 0usize;
    for review_id in candidates {
        let Ok(claim) = event_store.begin_connector_authorization_authority_cleanup(review_id, now)
        else {
            continue;
        };
        if runtime
            .delete_authorization_review_authority(claim.authority_handle())
            .is_err()
        {
            continue;
        }
        if event_store
            .finish_connector_authorization_authority_cleanup(&claim, now)
            .is_ok()
        {
            completed += 1;
        }
    }
    Ok(completed)
}

fn base64_url_sha256(value: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc, Mutex};

    use crate::kernel::connectors::{
        ConnectorAccount, ConnectorHealth, FakeConnectorCredentialStore,
    };
    use crate::kernel::event_store::EventStore;

    struct FakeOAuth;

    impl ConnectorOAuthProvider for FakeOAuth {
        fn provider_id(&self) -> &'static str {
            "fake"
        }
        fn scopes_for(&self, capability: ConnectorCapability) -> Option<&'static [&'static str]> {
            match capability {
                ConnectorCapability::MailSearch | ConnectorCapability::MailReadThread => {
                    Some(&["mail.read"])
                }
                ConnectorCapability::MailCreateDraft => Some(&["mail.draft"]),
                ConnectorCapability::MailSendDraft => Some(&["mail.send"]),
                ConnectorCapability::CalendarListEvents => Some(&["calendar.read"]),
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
            if !matches!(code, "valid-code" | "wrong-scope") || verifier.expose().len() < 43 {
                return Err("invalid exchange".to_string());
            }
            let granted_scopes = if code == "wrong-scope" {
                vec!["unexpected.write".to_string()]
            } else {
                requested_scopes.to_vec()
            };
            ConnectorOAuthExchange::new(
                ConnectorSecret::new("fake-refresh-token".to_string())?,
                granted_scopes,
            )
        }

        fn account_profile(
            &self,
            _exchange: &ConnectorOAuthExchange,
        ) -> Result<ConnectorAuthorizationAccountProfile, String> {
            ConnectorAuthorizationAccountProfile::new("Fake account".to_string(), None)
        }
    }

    struct BlockingFakeOAuth {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl ConnectorOAuthProvider for BlockingFakeOAuth {
        fn provider_id(&self) -> &'static str {
            "fake"
        }

        fn scopes_for(&self, capability: ConnectorCapability) -> Option<&'static [&'static str]> {
            FakeOAuth.scopes_for(capability)
        }

        fn exchange_code(
            &self,
            code: &str,
            verifier: &ConnectorSecret,
            redirect_uri: &str,
            requested_scopes: &[String],
        ) -> Result<ConnectorOAuthExchange, String> {
            self.entered
                .send(())
                .map_err(|_| "provider barrier failed".to_string())?;
            self.release
                .lock()
                .map_err(|_| "provider barrier failed".to_string())?
                .recv()
                .map_err(|_| "provider barrier failed".to_string())?;
            FakeOAuth.exchange_code(code, verifier, redirect_uri, requested_scopes)
        }

        fn account_profile(
            &self,
            exchange: &ConnectorOAuthExchange,
        ) -> Result<ConnectorAuthorizationAccountProfile, String> {
            FakeOAuth.account_profile(exchange)
        }
    }

    #[test]
    fn authorization_account_profile_rejects_blank_oversized_and_control_text() {
        for (display_name, tenant_ref) in [
            (" ".to_string(), None),
            ("x".repeat(201), None),
            ("unsafe\nname".to_string(), None),
            ("Safe name".to_string(), Some("tenant\rmarker".to_string())),
            ("Safe name".to_string(), Some("x".repeat(201))),
        ] {
            assert!(ConnectorAuthorizationAccountProfile::new(display_name, tenant_ref).is_err());
        }
        assert!(ConnectorAuthorizationAccountProfile::new(
            " Safe name ".to_string(),
            Some(" tenant ".to_string()),
        )
        .is_ok());
    }

    #[test]
    fn fake_pkce_flow_minimizes_scopes_and_consumes_state_once() {
        let mut store = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_authorization(
            &mut store,
            &FakeOAuth,
            vec![
                ConnectorCapability::MailSearch,
                ConnectorCapability::MailReadThread,
            ],
            "http://127.0.0.1:43821/callback".to_string(),
            now,
        )
        .expect("authorization starts");
        assert_eq!(session.pkce_method, "S256");
        assert_eq!(session.requested_scopes, vec!["mail.read"]);
        let verifier = store
            .read(&session.verifier_handle)
            .expect("verifier reads");
        assert!(!serde_json::to_string(&session)
            .expect("session serializes")
            .contains(verifier.expose()));
        let event_store = EventStore::open_memory().expect("event store opens");
        event_store
            .upsert_connector_authorization_session(&session)
            .expect("session persists");
        assert!(complete_authorization(
            &event_store,
            &mut store,
            &FakeOAuth,
            session.id,
            "wrong-state",
            "valid-code",
            now
        )
        .is_err());
        let state = session.state.clone();
        let result = complete_authorization(
            &event_store,
            &mut store,
            &FakeOAuth,
            session.id,
            &state,
            "valid-code",
            now,
        )
        .expect("authorization completes");
        assert!(store.contains(&result.credential_handle));
        assert!(complete_authorization(
            &event_store,
            &mut store,
            &FakeOAuth,
            session.id,
            &state,
            "valid-code",
            now
        )
        .is_err());
        event_store
            .upsert_connector_authorization_session(&session)
            .expect("stale session write is safely ignored");
        assert_eq!(
            event_store
                .connector_authorization_session(session.id)
                .expect("completed session remains durable")
                .status,
            ConnectorAuthorizationStatus::Completed
        );
    }

    #[test]
    fn oauth_callback_rejects_userinfo_host_confusion() {
        let mut store = FakeConnectorCredentialStore::default();
        assert!(begin_authorization(
            &mut store,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43821@evil.example/callback".to_string(),
            Utc::now(),
        )
        .is_err());
        assert!(begin_authorization(
            &mut store,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43821/callback#fragment".to_string(),
            Utc::now(),
        )
        .is_err());
    }

    #[test]
    fn oauth_exchange_rejects_scopes_not_granted_by_provider() {
        let mut store = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_authorization(
            &mut store,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43821/callback".to_string(),
            now,
        )
        .expect("authorization starts");
        let event_store = EventStore::open_memory().expect("event store opens");
        event_store
            .upsert_connector_authorization_session(&session)
            .expect("session persists");
        assert!(complete_authorization(
            &event_store,
            &mut store,
            &FakeOAuth,
            session.id,
            &session.state,
            "wrong-scope",
            now,
        )
        .is_err());
        assert!(!store.contains(&session.result_credential_handle));
        assert_eq!(
            event_store
                .connector_authorization_session(session.id)
                .expect("failed exchange remains durable")
                .status,
            ConnectorAuthorizationStatus::RepairRequired
        );
    }

    #[test]
    fn authorization_session_survives_restart_without_persisting_verifier() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("oauth.sqlite3");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_authorization(
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::CalendarListEvents],
            "http://127.0.0.1:43821/callback".to_string(),
            now,
        )
        .expect("authorization starts");
        let verifier = credentials
            .read(&session.verifier_handle)
            .expect("verifier reads")
            .expose()
            .to_string();
        {
            let event_store = EventStore::open(&path).expect("store opens");
            event_store
                .upsert_connector_authorization_session(&session)
                .expect("session persists");
        }
        let event_store = EventStore::open(&path).expect("store reopens");
        let recovered = event_store
            .connector_authorization_session(session.id)
            .expect("session reloads");
        let state = recovered.state.clone();
        let result = complete_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            recovered.id,
            &state,
            "valid-code",
            now,
        )
        .expect("callback completes after restart");
        assert!(credentials.contains(&result.credential_handle));
        assert_eq!(
            event_store
                .connector_authorization_session(session.id)
                .expect("completed session reloads")
                .status,
            ConnectorAuthorizationStatus::Completed
        );
        let sqlite_bytes = std::fs::read(&path).expect("sqlite reads");
        assert!(!String::from_utf8_lossy(&sqlite_bytes).contains(&verifier));
    }

    #[test]
    fn opaque_authorization_action_is_durable_one_shot_and_session_bound() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("oauth-action.sqlite3");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_authorization(
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43822/callback".to_string(),
            now,
        )
        .expect("authorization starts");
        let token = {
            let event_store = EventStore::open(&path).expect("store opens");
            event_store
                .upsert_connector_authorization_session(&session)
                .expect("session persists");
            event_store
                .issue_connector_authorization_action(session.id, now)
                .expect("opaque action issues")
        };

        let event_store = EventStore::open(&path).expect("store reopens");
        assert!(event_store
            .claim_connector_authorization_action(session.id, "tampered-token", now)
            .is_err());
        let claimed = event_store
            .claim_connector_authorization_action(session.id, &token, now)
            .expect("exact action claims once");
        assert_eq!(claimed.status, ConnectorAuthorizationStatus::Exchanging);
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: claimed.provider_id.clone(),
            display_name: "Fake authorization account".to_string(),
            tenant_ref: Some("tenant:fake-authorization".to_string()),
            credential_handle: claimed.result_credential_handle.clone(),
            granted_capabilities: claimed.requested_capabilities.clone(),
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        let result = complete_claimed_authorization_with_account(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            claimed,
            "valid-code",
            &account,
            now,
        )
        .expect("claimed action completes without an IPC callback");
        assert!(credentials.contains(&result.credential_handle));
        assert_eq!(
            event_store
                .list_connector_accounts()
                .expect("accounts list"),
            vec![account]
        );
        assert!(event_store
            .claim_connector_authorization_action(session.id, &token, now)
            .is_err());
        assert_eq!(
            event_store
                .connector_authorization_session(session.id)
                .expect("claimed session reloads")
                .status,
            ConnectorAuthorizationStatus::Completed
        );
        let sqlite_bytes = std::fs::read(&path).expect("sqlite reads");
        assert!(!String::from_utf8_lossy(&sqlite_bytes).contains(&token));
    }

    #[test]
    fn authorization_action_cannot_be_reissued_or_rotate_the_original_token() {
        let event_store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_authorization(
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43829/callback".to_string(),
            now,
        )
        .expect("authorization starts");
        event_store
            .upsert_connector_authorization_session(&session)
            .expect("session persists");
        let original_token = event_store
            .issue_connector_authorization_action(session.id, now)
            .expect("first action issues");

        assert!(event_store
            .issue_connector_authorization_action(session.id, now)
            .is_err());
        let claimed = event_store
            .claim_connector_authorization_action(session.id, &original_token, now)
            .expect("original token remains valid after rejected reissue");
        assert_eq!(claimed.status, ConnectorAuthorizationStatus::Exchanging);
    }

    #[test]
    fn approve_and_cancel_compete_for_the_same_one_shot_action() {
        for winner in [
            ConnectorAuthorizationIntent::Approve,
            ConnectorAuthorizationIntent::Cancel,
        ] {
            let temp_dir = tempfile::tempdir().expect("temp dir");
            let path = temp_dir.path().join("oauth-action-intent-race.sqlite3");
            let first = EventStore::open(&path).expect("first store opens");
            let second = EventStore::open(&path).expect("second store opens");
            let mut credentials = FakeConnectorCredentialStore::default();
            let now = Utc::now();
            let session = begin_persisted_authorization(
                &first,
                &mut credentials,
                &FakeOAuth,
                vec![ConnectorCapability::MailSearch],
                "http://127.0.0.1:43833/callback".to_string(),
                now,
            )
            .expect("authorization persists");
            let token = first
                .issue_connector_authorization_action(session.id, now)
                .expect("action issues");
            let resolution = first
                .resolve_connector_authorization_action(session.id, &token, winner, now)
                .expect("winning intent resolves");
            let loser = if winner == ConnectorAuthorizationIntent::Approve {
                ConnectorAuthorizationIntent::Cancel
            } else {
                ConnectorAuthorizationIntent::Approve
            };
            assert!(second
                .resolve_connector_authorization_action(session.id, &token, loser, now)
                .is_err());

            match resolution {
                crate::kernel::event_store::ConnectorAuthorizationResolution::Approved(claim) => {
                    assert_eq!(claim.status, ConnectorAuthorizationStatus::Exchanging);
                    assert!(credentials.contains(&claim.verifier_handle));
                }
                crate::kernel::event_store::ConnectorAuthorizationResolution::Cancelled(claim) => {
                    assert!(second
                        .begin_connector_authorization_cleanup(session.id, now)
                        .is_err());
                    let takeover = second
                        .begin_connector_authorization_cleanup(
                            session.id,
                            now + Duration::minutes(6),
                        )
                        .expect("expired cleanup lease is taken over");
                    let runtime = ConnectorRuntime::new(credentials);
                    runtime
                        .delete_authorization_handles(takeover.session())
                        .expect("cancelled handles delete");
                    assert!(first
                        .finish_connector_authorization_cleanup(&claim, now + Duration::minutes(6),)
                        .is_err());
                    first
                        .finish_connector_authorization_cleanup(
                            &takeover,
                            now + Duration::minutes(6),
                        )
                        .expect("cancel cleanup fences and completes");
                    let cancelled = first
                        .connector_authorization_session(session.id)
                        .expect("cancelled session reloads");
                    assert_eq!(cancelled.status, ConnectorAuthorizationStatus::Cancelled);
                    assert!(!cancelled.cleanup_required);
                }
            }
        }
    }

    #[test]
    fn exchange_claim_is_storage_only_and_finalizes_exactly_one_account() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("oauth-exchange-claim.sqlite3");
        let event_store = EventStore::open(&path).expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_authorization(
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43830/callback".to_string(),
            now,
        )
        .expect("authorization starts");
        event_store
            .upsert_connector_authorization_session(&session)
            .expect("session persists");
        let token = event_store
            .issue_connector_authorization_action(session.id, now)
            .expect("action issues");
        let claimed = event_store
            .claim_connector_authorization_action(session.id, &token, now)
            .expect("action claims");
        let claim_id = claimed.claim_id();
        let session_json = serde_json::to_string(claimed.session()).expect("session serializes");
        assert!(!session_json.contains("claim"));
        assert!(!session_json.contains(&claim_id.to_string()));
        let stored_claim: String = rusqlite::Connection::open(&path)
            .expect("claim connection opens")
            .query_row(
                "SELECT exchange_claim_id FROM connector_authorization_sessions WHERE id = ?1",
                rusqlite::params![session.id.to_string()],
                |row| row.get(0),
            )
            .expect("claim persists privately");
        assert_eq!(stored_claim, claim_id.to_string());
        let account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: claimed.provider_id.clone(),
            display_name: "Winner account".to_string(),
            tenant_ref: None,
            credential_handle: claimed.result_credential_handle.clone(),
            granted_capabilities: claimed.requested_capabilities.clone(),
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };

        complete_claimed_authorization_with_account(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            claimed,
            "valid-code",
            &account,
            now,
        )
        .expect("winner completes");
        assert!(credentials.contains(&account.credential_handle));
        assert_eq!(
            event_store
                .list_connector_accounts()
                .expect("accounts list"),
            vec![account]
        );
        assert_eq!(
            event_store
                .connector_authorization_session(session.id)
                .expect("session reloads")
                .status,
            ConnectorAuthorizationStatus::Completed
        );
    }

    #[test]
    fn expired_authorization_action_does_not_claim_or_change_session() {
        let event_store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_authorization(
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::CalendarListEvents],
            "http://127.0.0.1:43823/callback".to_string(),
            now,
        )
        .expect("authorization starts");
        event_store
            .upsert_connector_authorization_session(&session)
            .expect("session persists");
        let token = event_store
            .issue_connector_authorization_action(session.id, now)
            .expect("action issues");

        assert!(event_store
            .claim_connector_authorization_action(session.id, &token, session.expires_at)
            .is_err());
        assert_eq!(
            event_store
                .connector_authorization_session(session.id)
                .expect("pending session remains")
                .status,
            ConnectorAuthorizationStatus::Pending
        );
    }

    #[test]
    fn authorization_projection_and_action_digest_tampering_fail_closed() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("oauth-action-tamper.sqlite3");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_authorization(
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43824/callback".to_string(),
            now,
        )
        .expect("authorization starts");
        let token = {
            let event_store = EventStore::open(&path).expect("store opens");
            event_store
                .upsert_connector_authorization_session(&session)
                .expect("session persists");
            event_store
                .issue_connector_authorization_action(session.id, now)
                .expect("action issues")
        };
        let mut tampered = session.clone();
        tampered.requested_capabilities = vec![ConnectorCapability::MailSendDraft];
        rusqlite::Connection::open(&path)
            .expect("tamper connection opens")
            .execute(
                "UPDATE connector_authorization_sessions SET session_json = ?2 WHERE id = ?1",
                rusqlite::params![
                    session.id.to_string(),
                    serde_json::to_string(&tampered).unwrap()
                ],
            )
            .expect("private session row tampers");
        let event_store = EventStore::open(&path).expect("store reopens");
        assert!(event_store
            .claim_connector_authorization_action(session.id, &token, now)
            .is_err());
        assert_eq!(
            event_store
                .connector_authorization_session(session.id)
                .expect("projection remains internally consistent")
                .status,
            ConnectorAuthorizationStatus::Pending
        );
        drop(event_store);

        rusqlite::Connection::open(&path)
            .expect("projection tamper connection opens")
            .execute(
                "UPDATE connector_authorization_sessions SET status = ?2 WHERE id = ?1",
                rusqlite::params![
                    session.id.to_string(),
                    serde_json::to_string(&ConnectorAuthorizationStatus::Completed).unwrap()
                ],
            )
            .expect("status projection tampers");
        assert!(EventStore::open(&path)
            .expect("store reopens after projection tamper")
            .connector_authorization_session(session.id)
            .is_err());
    }

    #[test]
    fn persisted_begin_records_preparing_before_vault_and_activates_pending() {
        let event_store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43825/callback".to_string(),
            now,
        )
        .expect("persisted authorization begins");

        assert_eq!(session.status, ConnectorAuthorizationStatus::Pending);
        assert_eq!(session.revision, 1);
        assert!(credentials.contains(&session.verifier_handle));
        assert!(event_store
            .issue_connector_authorization_action(session.id, now)
            .is_ok());
    }

    #[test]
    fn authorization_review_provisions_secret_outside_sqlite_and_survives_restart() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("oauth-review-authority.sqlite3");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let event_store = EventStore::open(&path).expect("store opens");
        let session = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43833/callback".to_string(),
            now,
        )
        .expect("authorization begins");
        let provision = event_store
            .prepare_connector_authorization_review(session.id, now)
            .expect("review receipt persists before vault write");
        let review_id = provision.review_id();
        assert_eq!(provision.authorization_id(), session.id);
        assert!(event_store
            .connector_authorization_active_review(review_id, now)
            .is_err());
        let (authority_handle, authority) = provision.into_vault_parts();
        let authority_marker = authority.expose().to_string();
        credentials
            .put_new_at(&authority_handle, authority)
            .expect("review authority writes after receipt");
        let active = event_store
            .activate_connector_authorization_review(review_id, now)
            .expect("review activates after vault write");
        assert_eq!(active.review_id(), review_id);
        assert_eq!(active.authorization_id(), session.id);
        assert_eq!(active.authority_handle(), &authority_handle);
        drop(event_store);

        let reopened = EventStore::open(&path).expect("store reopens");
        let restored = reopened
            .connector_authorization_active_review(review_id, now)
            .expect("same active review restores");
        assert_eq!(restored.review_id(), review_id);
        assert_eq!(restored.authorization_id(), session.id);
        assert_eq!(restored.authority_handle(), &authority_handle);
        assert!(credentials.contains(restored.authority_handle()));
        let sqlite = std::fs::read(&path).expect("sqlite reads");
        assert!(!String::from_utf8_lossy(&sqlite).contains(&authority_marker));
    }

    #[test]
    fn preparing_and_legacy_hash_only_actions_are_not_restart_reviews() {
        let event_store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let first = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43834/callback".to_string(),
            now,
        )
        .expect("first authorization begins");
        let preparing = event_store
            .prepare_connector_authorization_review(first.id, now)
            .expect("preparing review persists");
        assert!(event_store
            .connector_authorization_active_review(preparing.review_id(), now)
            .is_err());

        let second = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::CalendarListEvents],
            "http://127.0.0.1:43835/callback".to_string(),
            now,
        )
        .expect("second authorization begins");
        event_store
            .issue_connector_authorization_action(second.id, now)
            .expect("legacy action issues");
        assert!(event_store
            .connector_authorization_active_review(Uuid::new_v4(), now)
            .is_err());
    }

    #[test]
    fn startup_cleanup_owns_and_deletes_preparing_review_authority() {
        let event_store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43836/callback".to_string(),
            now,
        )
        .expect("authorization begins");
        let provision = event_store
            .prepare_connector_authorization_review(session.id, now)
            .expect("preparing review persists");
        let review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        credentials
            .put_new_at(&authority_handle, authority)
            .expect("simulated crash leaves authority after vault write");
        let runtime = ConnectorRuntime::new(credentials);

        assert_eq!(
            recover_due_connector_authorizations(&event_store, &runtime, now, 64)
                .expect("startup cleanup converges preparing review"),
            1
        );
        assert!(!runtime
            .contains_credential(&authority_handle)
            .expect("vault inspects"));
        assert!(event_store
            .connector_authorization_active_review(review_id, now)
            .is_err());
        let cleaned = event_store
            .connector_authorization_session(session.id)
            .expect("cleaned session remains auditable");
        assert_eq!(cleaned.status, ConnectorAuthorizationStatus::RepairRequired);
        assert!(!cleaned.cleanup_required);
    }

    #[test]
    fn active_review_approve_and_cancel_have_one_consumed_intent_and_cleanup_authority() {
        use std::sync::{Arc, Barrier};

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("oauth-review-resolve-race.sqlite3");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let store = EventStore::open(&path).expect("store opens");
        let session = begin_persisted_authorization(
            &store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43837/callback".to_string(),
            now,
        )
        .expect("authorization begins");
        let provision = store
            .prepare_connector_authorization_review(session.id, now)
            .expect("review prepares");
        let review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        let authority_marker = authority.expose().to_string();
        credentials
            .put_new_at(&authority_handle, authority)
            .expect("authority writes");
        store
            .activate_connector_authorization_review(review_id, now)
            .expect("review activates");
        drop(store);

        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for intent in [
            ConnectorAuthorizationIntent::Approve,
            ConnectorAuthorizationIntent::Cancel,
        ] {
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            let authority_marker = authority_marker.clone();
            workers.push(std::thread::spawn(move || {
                let store = EventStore::open(path).expect("worker store opens");
                let authority =
                    ConnectorSecret::new(authority_marker).expect("worker authority builds");
                barrier.wait();
                store
                    .resolve_connector_authorization_review(review_id, &authority, intent, now)
                    .is_ok()
            }));
        }
        assert_eq!(
            workers
                .into_iter()
                .map(|worker| worker.join().expect("worker joins"))
                .filter(|won| *won)
                .count(),
            1
        );

        let connection = rusqlite::Connection::open(&path).expect("inspection opens");
        let (action_status, resolved_intent, handle_json, cleanup_required): (
            String,
            String,
            Option<String>,
            i64,
        ) = connection
            .query_row(
                r#"SELECT action_status, resolved_intent, authority_handle_json,
                          authority_cleanup_required
                   FROM connector_authorization_actions WHERE review_id = ?1"#,
                rusqlite::params![review_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("consumed action remains durable");
        assert_eq!(action_status, "consumed");
        assert!(matches!(resolved_intent.as_str(), "approve" | "cancel"));
        assert!(handle_json.is_some());
        assert_eq!(cleanup_required, 1);
        drop(connection);

        let runtime = ConnectorRuntime::new(credentials);
        let store = EventStore::open(&path).expect("recovery store opens");
        assert!(
            recover_due_connector_authorizations(&store, &runtime, now, 64)
                .expect("authority cleanup recovers")
                >= 1
        );
        assert!(!runtime
            .contains_credential(&authority_handle)
            .expect("vault inspects"));
        let connection = rusqlite::Connection::open(&path).expect("tombstone inspection opens");
        let (status, token_hash, session_hash, handle_json, cleanup_required): (
            String,
            String,
            String,
            Option<String>,
            i64,
        ) = connection
            .query_row(
                r#"SELECT action_status, token_hash, session_hash,
                          authority_handle_json, authority_cleanup_required
                   FROM connector_authorization_actions WHERE review_id = ?1"#,
                rusqlite::params![review_id.to_string()],
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
            .expect("safe tombstone remains");
        assert_eq!(status, "resolved");
        assert_eq!(token_hash, "redacted");
        assert_eq!(session_hash, "redacted");
        assert!(handle_json.is_none());
        assert_eq!(cleanup_required, 0);
    }

    #[test]
    fn review_id_is_not_authority_and_does_not_consume_active_review() {
        let event_store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43838/callback".to_string(),
            now,
        )
        .expect("authorization begins");
        let provision = event_store
            .prepare_connector_authorization_review(session.id, now)
            .expect("review prepares");
        let review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        credentials
            .put_new_at(&authority_handle, authority)
            .expect("authority writes");
        event_store
            .activate_connector_authorization_review(review_id, now)
            .expect("review activates");

        let fake_authority = ConnectorSecret::new(review_id.to_string()).expect("fake builds");
        assert!(event_store
            .resolve_connector_authorization_review(
                review_id,
                &fake_authority,
                ConnectorAuthorizationIntent::Approve,
                now,
            )
            .is_err());
        assert!(event_store
            .connector_authorization_active_review(review_id, now)
            .is_ok());
        assert_eq!(
            event_store
                .connector_authorization_session(session.id)
                .expect("session remains")
                .status,
            ConnectorAuthorizationStatus::Pending
        );
    }

    #[test]
    fn persisted_review_resolver_reads_vault_authority_and_is_one_shot() {
        let event_store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43839/callback".to_string(),
            now,
        )
        .expect("authorization begins");
        let provision = event_store
            .prepare_connector_authorization_review(session.id, now)
            .expect("review prepares");
        let review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        credentials
            .put_new_at(&authority_handle, authority)
            .expect("authority writes");
        event_store
            .activate_connector_authorization_review(review_id, now)
            .expect("review activates");
        let runtime = ConnectorRuntime::new(credentials);

        let resolution = resolve_persisted_authorization_review(
            &event_store,
            &runtime,
            review_id,
            ConnectorAuthorizationIntent::Approve,
            now,
        )
        .expect("vault-backed review resolves");
        let ConnectorAuthorizationResolution::Approved(claim) = resolution else {
            panic!("approve returns exchange claim");
        };
        assert_eq!(claim.session().id, session.id);
        assert_eq!(claim.action_authority_handle(), Some(&authority_handle));
        assert!(event_store
            .validate_connector_authorization_exchange_claim(&claim, now)
            .is_ok());
        assert!(event_store
            .validate_connector_authorization_exchange_claim(&claim, now + Duration::minutes(6),)
            .is_err());
        assert!(resolve_persisted_authorization_review(
            &event_store,
            &runtime,
            review_id,
            ConnectorAuthorizationIntent::Cancel,
            now,
        )
        .is_err());
        assert!(runtime
            .contains_credential(&authority_handle)
            .expect("authority remains for durable cleanup"));
        assert_eq!(
            recover_due_connector_authorizations(&event_store, &runtime, now, 64)
                .expect("authority cleanup runs"),
            1
        );
        assert!(!runtime
            .contains_credential(&authority_handle)
            .expect("authority deletes"));
    }

    #[test]
    fn runtime_completion_rechecks_claim_then_materializes_exact_account() {
        let event_store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43841/callback".to_string(),
            now,
        )
        .expect("authorization begins");
        let provision = event_store
            .prepare_connector_authorization_review(session.id, now)
            .expect("review prepares");
        let review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        credentials
            .put_new_at(&authority_handle, authority)
            .expect("authority writes");
        event_store
            .activate_connector_authorization_review(review_id, now)
            .expect("review activates");
        let runtime = ConnectorRuntime::new(credentials);
        let claim = match resolve_persisted_authorization_review(
            &event_store,
            &runtime,
            review_id,
            ConnectorAuthorizationIntent::Approve,
            now,
        )
        .expect("review approves")
        {
            ConnectorAuthorizationResolution::Approved(claim) => claim,
            ConnectorAuthorizationResolution::Cancelled(_) => panic!("approve returns claim"),
        };
        let result_handle = claim.result_credential_handle.clone();

        let result = complete_claimed_authorization_with_runtime(
            &event_store,
            &runtime,
            &FakeOAuth,
            claim,
            "valid-code",
            now,
        )
        .expect("runtime completion succeeds");

        assert_eq!(result.credential_handle, result_handle);
        assert!(runtime
            .contains_credential(&result_handle)
            .expect("result credential inspects"));
        assert!(!runtime
            .contains_credential(&session.verifier_handle)
            .expect("verifier inspects"));
        let accounts = event_store
            .list_connector_accounts()
            .expect("accounts list");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].provider_id, "fake");
        assert_eq!(accounts[0].credential_handle, result_handle);
        assert_eq!(
            recover_due_connector_authorizations(&event_store, &runtime, now, 64)
                .expect("authority tombstone cleans"),
            1
        );
        assert!(!runtime
            .contains_credential(&authority_handle)
            .expect("authority inspects"));
    }

    #[test]
    fn provider_result_after_claim_expiry_performs_no_vault_write() {
        let event_store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43842/callback".to_string(),
            now,
        )
        .expect("authorization begins");
        let provision = event_store
            .prepare_connector_authorization_review(session.id, now)
            .expect("review prepares");
        let review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        credentials
            .put_new_at(&authority_handle, authority)
            .expect("authority writes");
        event_store
            .activate_connector_authorization_review(review_id, now)
            .expect("review activates");
        let runtime = ConnectorRuntime::new(credentials);
        let claim = match resolve_persisted_authorization_review(
            &event_store,
            &runtime,
            review_id,
            ConnectorAuthorizationIntent::Approve,
            now,
        )
        .expect("review approves")
        {
            ConnectorAuthorizationResolution::Approved(claim) => claim,
            ConnectorAuthorizationResolution::Cancelled(_) => panic!("approve returns claim"),
        };
        let result_handle = claim.result_credential_handle.clone();
        assert!(complete_claimed_authorization_with_runtime(
            &event_store,
            &runtime,
            &FakeOAuth,
            claim,
            "valid-code",
            now + Duration::minutes(6),
        )
        .is_err());
        assert!(!runtime
            .contains_credential(&result_handle)
            .expect("result credential inspects"));
        assert!(runtime
            .contains_credential(&session.verifier_handle)
            .expect("verifier remains owned by cleanup"));
        assert!(event_store
            .list_connector_accounts()
            .expect("accounts list")
            .is_empty());
    }

    #[test]
    fn cleanup_first_fences_blocked_provider_result_from_vault_and_account() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("oauth-cleanup-first.sqlite3");
        let event_store = EventStore::open(&path).expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43843/callback".to_string(),
            now,
        )
        .expect("authorization begins");
        let provision = event_store
            .prepare_connector_authorization_review(session.id, now)
            .expect("review prepares");
        let review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        credentials
            .put_new_at(&authority_handle, authority)
            .expect("authority writes");
        event_store
            .activate_connector_authorization_review(review_id, now)
            .expect("review activates");
        let runtime = Arc::new(ConnectorRuntime::new(credentials));
        let claim = match resolve_persisted_authorization_review(
            &event_store,
            runtime.as_ref(),
            review_id,
            ConnectorAuthorizationIntent::Approve,
            now,
        )
        .expect("review approves")
        {
            ConnectorAuthorizationResolution::Approved(claim) => claim,
            ConnectorAuthorizationResolution::Cancelled(_) => panic!("approve returns claim"),
        };
        let result_handle = claim.result_credential_handle.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let provider = BlockingFakeOAuth {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        };
        let completion_path = path.clone();
        let completion_runtime = Arc::clone(&runtime);
        let completion = std::thread::spawn(move || {
            let completion_store =
                EventStore::open(&completion_path).expect("completion store opens");
            complete_claimed_authorization_with_runtime(
                &completion_store,
                completion_runtime.as_ref(),
                &provider,
                claim,
                "valid-code",
                now + Duration::minutes(6),
            )
        });
        entered_rx.recv().expect("provider exchange starts");

        assert_eq!(
            recover_due_connector_authorizations(
                &event_store,
                runtime.as_ref(),
                now + Duration::minutes(6),
                64,
            )
            .expect("cleanup wins"),
            2
        );
        release_tx.send(()).expect("provider exchange releases");
        assert!(completion.join().expect("completion joins").is_err());
        assert!(!runtime
            .contains_credential(&result_handle)
            .expect("result credential inspects"));
        assert!(event_store
            .list_connector_accounts()
            .expect("accounts list")
            .is_empty());
    }

    #[test]
    fn authorization_runtime_fence_serializes_same_authorization_only() {
        use std::sync::{mpsc, Arc};

        let runtime = Arc::new(ConnectorRuntime::new(
            FakeConnectorCredentialStore::default(),
        ));
        let authorization_id = Uuid::new_v4();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first_runtime = Arc::clone(&runtime);
        let first = std::thread::spawn(move || {
            first_runtime
                .with_authorization_fence(authorization_id, || {
                    entered_tx.send("first").expect("first signals");
                    release_rx.recv().expect("first releases");
                    Ok(())
                })
                .expect("first fence runs");
        });
        assert_eq!(entered_rx.recv().expect("first enters"), "first");

        let (second_tx, second_rx) = mpsc::channel();
        let second_runtime = Arc::clone(&runtime);
        let second = std::thread::spawn(move || {
            second_runtime
                .with_authorization_fence(authorization_id, || {
                    second_tx.send(()).expect("second signals");
                    Ok(())
                })
                .expect("second fence runs");
        });
        assert!(second_rx.try_recv().is_err());
        release_tx.send(()).expect("first release signals");
        second_rx.recv().expect("second enters after release");
        first.join().expect("first joins");
        second.join().expect("second joins");
    }

    #[test]
    fn expired_active_review_cleans_session_and_authority_in_one_startup_pass() {
        let event_store = EventStore::open_memory().expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_persisted_authorization(
            &event_store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43840/callback".to_string(),
            now,
        )
        .expect("authorization begins");
        let provision = event_store
            .prepare_connector_authorization_review(session.id, now)
            .expect("review prepares");
        let review_id = provision.review_id();
        let (authority_handle, authority) = provision.into_vault_parts();
        credentials
            .put_new_at(&authority_handle, authority)
            .expect("authority writes");
        event_store
            .activate_connector_authorization_review(review_id, now)
            .expect("review activates");
        let runtime = ConnectorRuntime::new(credentials);

        assert_eq!(
            recover_due_connector_authorizations(&event_store, &runtime, session.expires_at, 64,)
                .expect("single startup pass converges expired review"),
            2
        );
        assert!(!runtime
            .contains_credential(&authority_handle)
            .expect("authority deletes"));
        let cleaned = event_store
            .connector_authorization_session(session.id)
            .expect("session remains auditable");
        assert_eq!(cleaned.status, ConnectorAuthorizationStatus::Cancelled);
        assert!(!cleaned.cleanup_required);
        assert!(event_store
            .connector_authorization_active_review(review_id, session.expires_at)
            .is_err());
    }

    #[test]
    fn startup_cleanup_converges_preparing_expired_pending_and_exchanging_without_provider() {
        let now = Utc::now();

        let preparing_store = EventStore::open_memory().expect("preparing store opens");
        let (preparing, verifier) = prepare_authorization(
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43826/callback".to_string(),
            now,
        )
        .expect("preparing material builds");
        preparing_store
            .insert_preparing_connector_authorization(&preparing, now)
            .expect("preparing receipt persists");
        let mut preparing_credentials = FakeConnectorCredentialStore::default();
        preparing_credentials
            .put_at(&preparing.verifier_handle, verifier)
            .expect("verifier writes after receipt");
        let preparing_runtime = ConnectorRuntime::new(preparing_credentials);
        assert_eq!(
            recover_due_connector_authorizations(&preparing_store, &preparing_runtime, now, 64)
                .expect("preparing cleanup runs"),
            1
        );
        let preparing = preparing_store
            .connector_authorization_session(preparing.id)
            .expect("preparing cleanup persists");
        assert_eq!(preparing.status, ConnectorAuthorizationStatus::Cancelled);
        assert!(!preparing.cleanup_required);

        let pending_store = EventStore::open_memory().expect("pending store opens");
        let mut pending_credentials = FakeConnectorCredentialStore::default();
        let pending = begin_persisted_authorization(
            &pending_store,
            &mut pending_credentials,
            &FakeOAuth,
            vec![ConnectorCapability::CalendarListEvents],
            "http://127.0.0.1:43827/callback".to_string(),
            now,
        )
        .expect("pending authorization begins");
        let pending_token = pending_store
            .issue_connector_authorization_action(pending.id, now)
            .expect("pending action issues");
        let pending_runtime = ConnectorRuntime::new(pending_credentials);
        assert_eq!(
            recover_due_connector_authorizations(
                &pending_store,
                &pending_runtime,
                pending.expires_at,
                64
            )
            .expect("expired pending cleanup runs"),
            1
        );
        let cleaned_pending = pending_store
            .connector_authorization_session(pending.id)
            .expect("pending cleanup persists");
        assert_eq!(
            cleaned_pending.status,
            ConnectorAuthorizationStatus::Cancelled
        );
        assert!(!cleaned_pending.cleanup_required);
        assert!(pending_store
            .claim_connector_authorization_action(pending.id, &pending_token, pending.expires_at)
            .is_err());

        let exchanging_store = EventStore::open_memory().expect("exchanging store opens");
        let mut exchanging_credentials = FakeConnectorCredentialStore::default();
        let exchanging = begin_persisted_authorization(
            &exchanging_store,
            &mut exchanging_credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43828/callback".to_string(),
            now,
        )
        .expect("exchanging authorization begins");
        let token = exchanging_store
            .issue_connector_authorization_action(exchanging.id, now)
            .expect("exchanging action issues");
        let exchange_claim = exchanging_store
            .claim_connector_authorization_action(exchanging.id, &token, now)
            .expect("authorization enters exchanging");
        let (mut stale_session, stale_claim_id, stale_claim_expires_at) =
            exchange_claim.into_parts();
        let exchanging_runtime = ConnectorRuntime::new(exchanging_credentials);
        assert_eq!(
            recover_due_connector_authorizations(&exchanging_store, &exchanging_runtime, now, 64)
                .expect("live exchange is left alone"),
            0
        );
        assert_eq!(
            exchanging_store
                .connector_authorization_session(exchanging.id)
                .expect("live exchange remains")
                .status,
            ConnectorAuthorizationStatus::Exchanging
        );
        assert_eq!(
            recover_due_connector_authorizations(
                &exchanging_store,
                &exchanging_runtime,
                now + Duration::minutes(6),
                64,
            )
            .expect("expired exchange cleanup runs"),
            1
        );
        let cleaned_exchanging = exchanging_store
            .connector_authorization_session(exchanging.id)
            .expect("exchanging cleanup persists");
        assert_eq!(
            cleaned_exchanging.status,
            ConnectorAuthorizationStatus::RepairRequired
        );
        assert!(!cleaned_exchanging.cleanup_required);
        stale_session.status = ConnectorAuthorizationStatus::Completed;
        stale_session.consumed_at = Some(now + Duration::minutes(6));
        stale_session.revision += 1;
        let stale_account = ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: stale_session.provider_id.clone(),
            display_name: "Stale exchange account".to_string(),
            tenant_ref: None,
            credential_handle: stale_session.result_credential_handle.clone(),
            granted_capabilities: stale_session.requested_capabilities.clone(),
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        };
        assert!(exchanging_store
            .finish_connector_authorization_with_account(
                &stale_session,
                &stale_account,
                stale_claim_id,
                stale_claim_expires_at,
                now + Duration::minutes(6),
            )
            .is_err());
        assert!(exchanging_store
            .list_connector_accounts()
            .expect("accounts list")
            .is_empty());
    }

    #[test]
    fn malformed_authorization_cleanup_row_does_not_starve_later_healthy_row() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("oauth-cleanup-non-starvation.sqlite3");
        let store = EventStore::open(&path).expect("store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let malformed = begin_persisted_authorization(
            &store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43831/callback".to_string(),
            now - Duration::minutes(30),
        )
        .expect("first authorization persists");
        let healthy = begin_persisted_authorization(
            &store,
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::CalendarListEvents],
            "http://127.0.0.1:43832/callback".to_string(),
            now - Duration::minutes(20),
        )
        .expect("second authorization persists");
        rusqlite::Connection::open(&path)
            .expect("tamper connection opens")
            .execute(
                "UPDATE connector_authorization_sessions SET session_json = 'malformed' WHERE id = ?1",
                rusqlite::params![malformed.id.to_string()],
            )
            .expect("oldest due row tampers");

        let runtime = ConnectorRuntime::new(credentials);
        assert_eq!(
            recover_due_connector_authorizations(&store, &runtime, now, 64)
                .expect("bounded cleanup isolates malformed row"),
            1
        );
        let healthy = store
            .connector_authorization_session(healthy.id)
            .expect("healthy row reloads");
        assert_eq!(healthy.status, ConnectorAuthorizationStatus::Cancelled);
        assert!(!healthy.cleanup_required);
        assert!(runtime.delete_authorization_handles(&healthy).is_ok());
    }

    #[test]
    fn legacy_authorization_rows_backfill_status_and_new_saga_fields_idempotently() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("legacy-oauth.sqlite3");
        let now = Utc::now();
        let mut credentials = FakeConnectorCredentialStore::default();
        let pending = begin_authorization(
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43829/callback".to_string(),
            now,
        )
        .expect("pending session builds");
        let mut completed = begin_authorization(
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::CalendarListEvents],
            "http://127.0.0.1:43830/callback".to_string(),
            now,
        )
        .expect("completed session builds");
        completed.consumed_at = Some(now);
        completed.status = ConnectorAuthorizationStatus::Completed;
        let legacy_json = |session: &ConnectorAuthorizationSession| {
            let mut value = serde_json::to_value(session).expect("session serializes");
            let object = value.as_object_mut().expect("session is object");
            object.remove("status");
            object.remove("revision");
            object.remove("cleanup_required");
            object.remove("cleanup_completed_at");
            value.to_string()
        };
        {
            let connection = rusqlite::Connection::open(&path).expect("legacy db opens");
            connection
                .execute_batch(
                    r#"CREATE TABLE connector_authorization_sessions (
                         id TEXT PRIMARY KEY NOT NULL,
                         session_json TEXT NOT NULL,
                         expires_at TEXT NOT NULL,
                         consumed_at TEXT,
                         updated_at TEXT NOT NULL
                       );"#,
                )
                .expect("legacy table creates");
            for session in [&pending, &completed] {
                connection
                    .execute(
                        r#"INSERT INTO connector_authorization_sessions
                           (id, session_json, expires_at, consumed_at, updated_at)
                           VALUES (?1, ?2, ?3, ?4, ?5)"#,
                        rusqlite::params![
                            session.id.to_string(),
                            legacy_json(session),
                            session.expires_at.to_rfc3339(),
                            session.consumed_at.map(|value| value.to_rfc3339()),
                            now.to_rfc3339(),
                        ],
                    )
                    .expect("legacy row inserts");
            }
        }
        for _ in 0..2 {
            let store = EventStore::open(&path).expect("legacy db migrates idempotently");
            assert_eq!(
                store
                    .connector_authorization_session(pending.id)
                    .expect("pending migrates")
                    .status,
                ConnectorAuthorizationStatus::Pending
            );
            assert_eq!(
                store
                    .connector_authorization_session(completed.id)
                    .expect("completed migrates")
                    .status,
                ConnectorAuthorizationStatus::Completed
            );
        }
        let connection = rusqlite::Connection::open(&path).expect("migrated db opens");
        let columns = connection
            .prepare("PRAGMA table_info(connector_authorization_sessions)")
            .expect("column query prepares")
            .query_map([], |row| row.get::<_, String>(1))
            .expect("columns query")
            .collect::<Result<Vec<_>, _>>()
            .expect("columns collect");
        for column in [
            "exchange_claim_id",
            "exchange_claim_expires_at",
            "cleanup_claim_id",
            "cleanup_claim_expires_at",
        ] {
            assert!(columns.iter().any(|candidate| candidate == column));
        }
        let ownership_count: i64 = connection
            .query_row(
                r#"SELECT COUNT(*) FROM connector_authorization_sessions
                   WHERE exchange_claim_id IS NOT NULL
                      OR exchange_claim_expires_at IS NOT NULL
                      OR cleanup_claim_id IS NOT NULL
                      OR cleanup_claim_expires_at IS NOT NULL"#,
                [],
                |row| row.get(0),
            )
            .expect("legacy ownership remains empty");
        assert_eq!(ownership_count, 0);
    }

    #[test]
    fn interrupted_exchange_cleans_preallocated_credentials_and_requires_repair() {
        let event_store = EventStore::open_memory().expect("event store opens");
        let mut credentials = FakeConnectorCredentialStore::default();
        let now = Utc::now();
        let session = begin_authorization(
            &mut credentials,
            &FakeOAuth,
            vec![ConnectorCapability::MailSearch],
            "http://127.0.0.1:43821/callback".to_string(),
            now,
        )
        .expect("authorization starts");
        event_store
            .upsert_connector_authorization_session(&session)
            .expect("session persists");
        event_store
            .claim_connector_authorization_session(session.id, &session.state, now)
            .expect("exchange claim persists");
        credentials
            .put_at(
                &session.result_credential_handle,
                ConnectorSecret::new("orphaned-token".to_string()).expect("secret is valid"),
            )
            .expect("simulated token write succeeds");
        assert!(
            recover_interrupted_authorization(&event_store, &mut credentials, session.id, now)
                .is_err()
        );
        recover_interrupted_authorization(
            &event_store,
            &mut credentials,
            session.id,
            now + Duration::minutes(6),
        )
        .expect("expired interrupted exchange recovers");
        assert!(!credentials.contains(&session.result_credential_handle));
        assert!(!credentials.contains(&session.verifier_handle));
        assert_eq!(
            event_store
                .connector_authorization_session(session.id)
                .expect("session reloads")
                .status,
            ConnectorAuthorizationStatus::RepairRequired
        );
    }
}
