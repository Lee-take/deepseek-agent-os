use std::time::Duration;

use super::{MICROSOFT_GRAPH_BASE, MICROSOFT_PROVIDER_ID};
use crate::kernel::connectors::http::{
    ConnectorAccessTokenResolver, ConnectorHttpAuthContext, ConnectorHttpFailure,
};
use crate::kernel::connectors::landing::{
    cleanup_incomplete_connector_attachment, stage_connector_attachment_with_checkpoint,
    ConnectorAttachmentCleanupFailure, ConnectorAttachmentDownloadPermit,
    ConnectorAttachmentMetadata, LandedConnectorAttachment,
};
use crate::kernel::connectors::{ConnectorAccount, ConnectorCapability, ConnectorHealth};
use crate::kernel::event_store::{ConnectorAttachmentCleanupClaim, EventStore};

pub(crate) struct MicrosoftAttachmentDownloadClient<R> {
    client: reqwest::blocking::Client,
    access_tokens: R,
    base: reqwest::Url,
}

impl<R: ConnectorAccessTokenResolver> MicrosoftAttachmentDownloadClient<R> {
    pub(crate) fn new(access_tokens: R) -> Result<Self, String> {
        let base = reqwest::Url::parse(MICROSOFT_GRAPH_BASE)
            .map_err(|_| "Microsoft attachment transport is unavailable".to_string())?;
        Self::build(access_tokens, base, true)
    }

    fn build(access_tokens: R, base: reqwest::Url, https_only: bool) -> Result<Self, String> {
        if !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err("Microsoft attachment transport is unavailable".to_string());
        }
        let client = reqwest::blocking::Client::builder()
            .user_agent("DS-Agent-Connector/1.0")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .https_only(https_only)
            .build()
            .map_err(|_| "Microsoft attachment transport is unavailable".to_string())?;
        Ok(Self {
            client,
            access_tokens,
            base,
        })
    }

    #[cfg(test)]
    fn new_test_http(access_tokens: R, port: u16) -> Result<Self, String> {
        let base = reqwest::Url::parse(&format!("http://127.0.0.1:{port}/v1.0/"))
            .map_err(|_| "Microsoft attachment transport is unavailable".to_string())?;
        Self::build(access_tokens, base, false)
    }

    pub(crate) fn download_and_land(
        &self,
        event_store: &EventStore,
        permit: ConnectorAttachmentDownloadPermit,
    ) -> Result<LandedConnectorAttachment, String> {
        let landing_id = permit.reservation_id();
        let result = self.download_and_land_inner(event_store, permit);
        if result.is_err() {
            reconcile_failed_download(event_store, landing_id);
        }
        result
    }

    fn download_and_land_inner(
        &self,
        event_store: &EventStore,
        permit: ConnectorAttachmentDownloadPermit,
    ) -> Result<LandedConnectorAttachment, String> {
        let execution = event_store
            .load_connector_attachment_execution(permit.reservation_id())
            .map_err(|_| "Microsoft attachment durable authority is unavailable".to_string())?;
        let account = &execution.account;
        let metadata = &execution.metadata;
        validate_account(account, metadata)?;
        permit.validate(metadata)?;
        permit.validate_workspace(&execution.workspace_identity)?;
        event_store
            .assert_connector_attachment_execution_current(permit.reservation_id())
            .map_err(|_| "Microsoft attachment account changed before download".to_string())?;
        let mut url = self.base.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| "Microsoft attachment request is invalid".to_string())?;
            segments
                .pop_if_empty()
                .push("me")
                .push("messages")
                .push(&metadata.parent_remote_ref)
                .push("attachments")
                .push(&metadata.attachment_remote_ref)
                .push("$value");
        }
        let access_token = self
            .access_tokens
            .resolve(&ConnectorHttpAuthContext::for_account(account))
            .map_err(normalize_http_failure)?;
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token.expose())
            .header(reqwest::header::ACCEPT, &metadata.declared_media_type)
            .send()
            .map_err(|error| {
                if error.is_timeout() {
                    "Microsoft attachment download timed out".to_string()
                } else {
                    "Microsoft attachment download failed".to_string()
                }
            })?;
        if response.status().as_u16() != 200 {
            return Err(match response.status().as_u16() {
                401 | 403 => "Microsoft attachment credential is unavailable",
                404 => "Microsoft attachment is unavailable",
                429 | 500..=599 => "Microsoft attachment service is temporarily unavailable",
                _ => "Microsoft attachment response is invalid",
            }
            .to_string());
        }
        if response
            .content_length()
            .is_some_and(|size| size != metadata.size_bytes)
        {
            return Err("Microsoft attachment size did not match metadata".to_string());
        }
        let response_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .unwrap_or_default();
        if !response_type.eq_ignore_ascii_case(&metadata.declared_media_type) {
            return Err("Microsoft attachment media type did not match metadata".to_string());
        }
        let staged = stage_connector_attachment_with_checkpoint(
            &execution.workspace_root,
            metadata,
            permit,
            response,
            |landing_id, storage_identity| {
                event_store
                    .mark_connector_attachment_staging(
                        landing_id,
                        storage_identity,
                        chrono::Utc::now(),
                    )
                    .map_err(|error| {
                        format!(
                            "connector attachment staging failed: {}",
                            normalized_store_error(&error)
                        )
                    })
            },
        )?;
        event_store
            .assert_connector_attachment_execution_current(staged.receipt().landing_id)
            .map_err(|_| "Microsoft attachment account changed during download".to_string())?;
        event_store
            .mark_connector_attachment_ready(&staged, chrono::Utc::now())
            .map_err(|error| {
                format!(
                    "connector attachment staging failed: {}",
                    normalized_store_error(&error)
                )
            })?;
        let landed = staged.commit()?;
        event_store
            .complete_connector_attachment_landing(&landed, chrono::Utc::now())
            .map_err(|error| {
                format!(
                    "connector attachment completion failed: {}",
                    normalized_store_error(&error)
                )
            })?;
        Ok(landed)
    }
}

fn reconcile_failed_download(event_store: &EventStore, landing_id: uuid::Uuid) {
    let now = chrono::Utc::now();
    let claim = event_store.claim_connector_attachment_cleanup(landing_id, "download_failed", now);
    let Ok(ConnectorAttachmentCleanupClaim::Owned(claim_id)) = claim else {
        return;
    };
    if event_store
        .renew_connector_attachment_recovery_claim(landing_id, claim_id, chrono::Utc::now())
        .is_err()
    {
        return;
    }
    let Ok(candidate) = event_store.connector_attachment_cleanup_candidate(landing_id, claim_id)
    else {
        return;
    };
    match cleanup_incomplete_connector_attachment(&candidate) {
        Ok(()) => {
            let _ = event_store.fail_connector_attachment_after_cleanup(
                landing_id,
                claim_id,
                chrono::Utc::now(),
            );
        }
        Err(ConnectorAttachmentCleanupFailure::Unsafe) => {
            let _ = event_store.mark_connector_attachment_repair_required(
                landing_id,
                claim_id,
                "download_cleanup_identity_conflict",
                chrono::Utc::now(),
            );
        }
        Err(ConnectorAttachmentCleanupFailure::Transient) => {
            let _ = event_store.defer_connector_attachment_cleanup(
                landing_id,
                claim_id,
                chrono::Utc::now(),
            );
        }
    }
}

fn normalized_store_error(_error: &crate::kernel::event_store::EventStoreError) -> &'static str {
    "durable account state changed"
}

fn validate_account(
    account: &ConnectorAccount,
    metadata: &ConnectorAttachmentMetadata,
) -> Result<(), String> {
    if account.id != metadata.account_id
        || account.provider_id != MICROSOFT_PROVIDER_ID
        || metadata.provider_id != MICROSOFT_PROVIDER_ID
        || account.health != ConnectorHealth::Connected
        || !account
            .granted_capabilities
            .contains(&ConnectorCapability::MailReadAttachment)
    {
        return Err("Microsoft attachment permission is unavailable".to_string());
    }
    Ok(())
}

fn normalize_http_failure(failure: ConnectorHttpFailure) -> String {
    match failure {
        ConnectorHttpFailure::CredentialUnavailable => {
            "Microsoft attachment credential is unavailable"
        }
        ConnectorHttpFailure::Timeout => "Microsoft attachment download timed out",
        _ => "Microsoft attachment download failed",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::kernel::connectors::http::ConnectorAccessTokenResolver;
    use crate::kernel::connectors::{ConnectorCredentialHandle, ConnectorSecret};
    use crate::kernel::event_store::EventStore;

    struct TokenResolver;

    impl ConnectorAccessTokenResolver for TokenResolver {
        fn resolve(
            &self,
            _auth: &ConnectorHttpAuthContext,
        ) -> Result<ConnectorSecret, ConnectorHttpFailure> {
            ConnectorSecret::new("access-token-marker".to_string())
                .map_err(|_| ConnectorHttpFailure::CredentialUnavailable)
        }
    }

    fn account() -> ConnectorAccount {
        ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: MICROSOFT_PROVIDER_ID.to_string(),
            display_name: "Microsoft test".to_string(),
            tenant_ref: Some("tenant:test".to_string()),
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailReadAttachment],
            health: ConnectorHealth::Connected,
            connected_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn microsoft_attachment_requires_permit_and_streams_fixed_value_path() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, receiver) = mpsc::channel();
        let bytes = b"%PDF-1.7\nprovider attachment".to_vec();
        let response_bytes = bytes.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0u8; 4096];
                let count = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..count]);
                if request.windows(4).any(|value| value == b"\r\n\r\n") {
                    break;
                }
            }
            sender
                .send(String::from_utf8_lossy(&request).to_string())
                .unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_bytes.len()
            )
            .unwrap();
            stream.write_all(&response_bytes).unwrap();
        });
        let account = account();
        let metadata = ConnectorAttachmentMetadata {
            account_id: account.id,
            provider_id: MICROSOFT_PROVIDER_ID.to_string(),
            parent_remote_ref: "message/one".to_string(),
            attachment_remote_ref: "attachment+one".to_string(),
            file_name: "report.pdf".to_string(),
            declared_media_type: "application/pdf".to_string(),
            size_bytes: bytes.len() as u64,
            contains_macros: false,
            untrusted_evidence: true,
        };
        let store = EventStore::open_memory().unwrap();
        store.upsert_connector_account(&account).unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let (approval, _) = store
            .prepare_connector_attachment_download_approval(&metadata, workspace.path(), None, now)
            .unwrap();
        let scope = approval.request.exact_tool.as_ref().unwrap();
        let permit = store
            .approve_and_reserve_connector_attachment_download(
                approval.request.id,
                approval.projection_revision,
                scope.preview_revision,
                &scope.preview_hash,
                "Approve one attachment".to_string(),
                now,
            )
            .unwrap();
        let client = MicrosoftAttachmentDownloadClient::new_test_http(TokenResolver, port).unwrap();
        let landed = client
            .download_and_land(&store, permit)
            .expect("Microsoft attachment lands");
        assert_eq!(fs::read(landed.path()).unwrap(), bytes);
        let request = receiver.recv().unwrap();
        server.join().unwrap();
        assert!(
            request.starts_with(
                "GET /v1.0/me/messages/message%2Fone/attachments/attachment+one/$value HTTP/1.1"
            ),
            "{request}"
        );
        assert!(
            request.contains("authorization: Bearer access-token-marker")
                || request.contains("Authorization: Bearer access-token-marker")
        );
        assert!(!request.contains("client_secret"));
    }
}
