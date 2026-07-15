use base64::{engine::general_purpose, Engine as _};
use chrono::{Duration, Utc};
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::kernel::connectors::contract::{
    validate_calendar_read_contract, validate_calendar_sync_contract, validate_mail_read_contract,
    validate_mail_sync_contract, validate_mutation_contract, validate_provider_contract,
};
use crate::kernel::connectors::http::{json_response, ScriptedConnectorHttpTransport};
use crate::kernel::connectors::{
    ConnectorCredentialHandle, ConnectorHealth, ConnectorMutationEnvelope,
};
use crate::kernel::models::AccessMode;

fn account(capabilities: Vec<ConnectorCapability>) -> ConnectorAccount {
    let now = Utc::now();
    ConnectorAccount {
        id: Uuid::new_v4(),
        provider_id: GOOGLE_PROVIDER_ID.to_string(),
        display_name: "Google fixture".to_string(),
        tenant_ref: None,
        credential_handle: ConnectorCredentialHandle::new(),
        granted_capabilities: capabilities,
        health: ConnectorHealth::Connected,
        connected_at: now,
        updated_at: now,
    }
}

fn gmail_message(id: &str, thread_id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "threadId": thread_id,
        "internalDate": "1784073600000",
        "snippet": "Untrusted bounded snippet",
        "payload": {
            "mimeType": "text/plain",
            "filename": "",
            "headers": [
                {"name": "From", "value": "Sender <sender@example.com>"},
                {"name": "To", "value": "receiver@example.com"},
                {"name": "Subject", "value": "Review"}
            ],
            "body": {"size": 12},
            "parts": []
        }
    })
}

fn gmail_draft(id: &str, message_id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "message": {
            "id": "draft-message-1",
            "threadId": "draft-thread-1",
            "internalDate": "1784073600000",
            "snippet": "Reviewed draft",
            "payload": {
                "mimeType": "text/plain",
                "filename": "",
                "headers": [
                    {"name": "Message-ID", "value": message_id},
                    {"name": "From", "value": "owner@example.com"},
                    {"name": "To", "value": "receiver@example.com"},
                    {"name": "Subject", "value": "Reviewed draft"}
                ],
                "body": {"size": 12},
                "parts": []
            }
        }
    })
}

fn calendar_event() -> CalendarMutationEvent {
    let starts_at = DateTime::parse_from_rfc3339("2026-07-15T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    CalendarMutationEvent {
        title: "Reviewed Google event".to_string(),
        description: Some("Exact reviewed description".to_string()),
        location: Some("Room G".to_string()),
        starts_at,
        ends_at: starts_at + Duration::hours(1),
        timezone: "Asia/Shanghai".to_string(),
        attendees: Vec::new(),
        notify_attendees: false,
    }
}

fn mail_content() -> ConnectorMailDraftContent {
    ConnectorMailDraftContent {
        to: vec![MailAddress {
            display_name: Some("Receiver".to_string()),
            address: "receiver@example.com".to_string(),
        }],
        cc: Vec::new(),
        bcc: Vec::new(),
        subject: "Reviewed local draft".to_string(),
        body_text: "Exact reviewed body".to_string(),
        in_reply_to: None,
        thread_ref: None,
    }
}

fn mutation_invocation(
    account: &ConnectorAccount,
    intent: ConnectorMutationIntent,
    idempotency_key: &str,
) -> ConnectorInvocation {
    let intent_hash = intent.hash().unwrap();
    let target_ref = intent.target_ref().to_string();
    let automation_run_id = Uuid::new_v4();
    ConnectorInvocation {
        id: Uuid::new_v4(),
        provider_id: GOOGLE_PROVIDER_ID.to_string(),
        account_id: account.id,
        account_generation: Some(0),
        capability: intent.capability(),
        automation_run_id: Some(automation_run_id),
        tool_invocation_id: Some(Uuid::new_v4()),
        request_fingerprint: format!("sha256:{}", Uuid::new_v4()),
        idempotency_key: idempotency_key.to_string(),
        mutation: Some(ConnectorMutationEnvelope {
            provider_id: GOOGLE_PROVIDER_ID.to_string(),
            account_id: account.id,
            account_generation: Some(0),
            capability: intent.capability(),
            target_ref,
            preview_hash: "sha256:reviewed-provider-preview".to_string(),
            intent_hash: Some(intent_hash),
            intent: Some(intent),
            idempotency_key: idempotency_key.to_string(),
            automation_run_id,
            agent_run_id: None,
            access_mode: AccessMode::FullAccess,
        }),
        status: ConnectorInvocationStatus::Running,
        evidence: Vec::new(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[test]
fn google_oauth_url_uses_exact_offline_pkce_scope_boundary() {
    struct NoExchange;
    impl GoogleCodeExchange for NoExchange {
        fn exchange_code(
            &self,
            _code: &str,
            _verifier: &ConnectorSecret,
            _redirect_uri: &str,
            _requested_scopes: &[String],
        ) -> Result<ConnectorOAuthExchange, String> {
            Err("unused".to_string())
        }
    }
    let provider = GoogleOAuthProvider::new(NoExchange);
    assert_eq!(provider.base_scopes(), &["openid", "email"]);
    assert_eq!(
        provider.scopes_for(ConnectorCapability::MailSearch),
        Some(&["https://www.googleapis.com/auth/gmail.readonly"] as &[&str])
    );

    let now = Utc::now();
    let mut session = ConnectorAuthorizationSession {
        id: Uuid::new_v4(),
        provider_id: GOOGLE_PROVIDER_ID.to_string(),
        requested_capabilities: vec![ConnectorCapability::MailSearch],
        requested_scopes: vec![
            "email".to_string(),
            "https://www.googleapis.com/auth/gmail.readonly".to_string(),
            "openid".to_string(),
        ],
        redirect_uri: "http://127.0.0.1:43123/callback".to_string(),
        state: "a".repeat(32),
        pkce_challenge: "A".repeat(43),
        pkce_method: "S256".to_string(),
        verifier_handle: ConnectorCredentialHandle::new(),
        result_credential_handle: ConnectorCredentialHandle::new(),
        status: ConnectorAuthorizationStatus::Pending,
        expires_at: now + Duration::minutes(5),
        consumed_at: None,
        revision: 0,
        cleanup_required: false,
        cleanup_completed_at: None,
    };
    let url = authorization_url(
        "1234567890-example.apps.googleusercontent.com",
        &session,
        now,
    )
    .unwrap();
    assert!(url.starts_with(GOOGLE_AUTHORIZATION_ENDPOINT));
    assert!(url.contains("access_type=offline"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("include_granted_scopes=false"));
    assert!(!url.contains("client_secret"));

    session
        .requested_scopes
        .push("scope.escalation".to_string());
    assert!(authorization_url(
        "1234567890-example.apps.googleusercontent.com",
        &session,
        now,
    )
    .is_err());
}

#[test]
fn google_mail_and_calendar_reads_are_typed_bounded_and_fixed_origin() {
    let transport = ScriptedConnectorHttpTransport::new(vec![
        Ok(json_response(200, json!({"threads": [{"id": "thread-1"}]}))),
        Ok(json_response(
            200,
            json!({"id": "thread-1", "messages": [gmail_message("message-1", "thread-1")]}),
        )),
        Ok(json_response(
            200,
            json!({
                "items": [{
                    "id": "event-1",
                    "status": "confirmed",
                    "summary": "Untrusted event",
                    "start": {"dateTime": "2026-07-15T10:00:00Z", "timeZone": "UTC"},
                    "end": {"dateTime": "2026-07-15T11:00:00Z", "timeZone": "UTC"},
                    "attendees": [{"email": "guest@example.com", "responseStatus": "accepted"}]
                }]
            }),
        )),
    ]);
    let adapter = GoogleWorkspaceAdapter::new(transport);
    let account = account(vec![
        ConnectorCapability::MailSearch,
        ConnectorCapability::CalendarListEvents,
    ]);
    let mail = adapter
        .search_mail(
            &account,
            &MailSearchRequest::new("from:sender@example.com".to_string(), 5).unwrap(),
        )
        .unwrap();
    assert_eq!(mail.len(), 1);
    assert!(mail.items()[0].messages[0].untrusted_evidence);
    let starts_at = DateTime::parse_from_rfc3339("2026-07-15T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let calendar = adapter
        .list_events(
            &account,
            &CalendarListRequest::new(starts_at, starts_at + Duration::days(1), 5).unwrap(),
        )
        .unwrap();
    assert_eq!(calendar.len(), 1);
    assert!(calendar.items()[0].untrusted_evidence);
}

#[test]
fn google_sync_uses_history_and_sync_tokens_without_leaking_provider_bodies() {
    let transport = ScriptedConnectorHttpTransport::new(vec![
        Ok(json_response(
            200,
            json!({"messages": [{"id": "message-1"}]}),
        )),
        Ok(json_response(200, gmail_message("message-1", "thread-1"))),
        Ok(json_response(
            200,
            json!({"emailAddress": "owner@example.com", "historyId": "9001"}),
        )),
        Ok(json_response(
            200,
            json!({
                "items": [],
                "nextSyncToken": "calendar-sync-secret"
            }),
        )),
    ]);
    let adapter = GoogleWorkspaceAdapter::new(transport);
    let account = account(vec![
        ConnectorCapability::MailSyncInbox,
        ConnectorCapability::CalendarSyncEvents,
    ]);
    let mail = adapter
        .sync_mail_page(&account, &MailSyncRequest::inbox(5).unwrap(), None)
        .unwrap();
    assert_eq!(mail.changes().len(), 1);
    assert!(matches!(
        mail.continuation(),
        ConnectorSyncContinuation::Delta(_)
    ));

    let starts_at = Utc::now();
    let calendar = adapter
        .sync_calendar_page(
            &account,
            &CalendarSyncRequest::new(starts_at, starts_at + Duration::days(1), 5).unwrap(),
            None,
        )
        .unwrap();
    assert!(matches!(
        calendar.continuation(),
        ConnectorSyncContinuation::Delta(_)
    ));
}

#[test]
fn google_provider_contract_covers_reads_sync_and_exact_reconciled_mutations() {
    let account = account(GOOGLE_CAPABILITIES.to_vec());
    let mail_send = mutation_invocation(
        &account,
        ConnectorMutationIntent::MailSendDraft {
            draft_ref: "draft-1".to_string(),
            internet_message_id: "<reviewed-google@example.com>".to_string(),
            content: None,
        },
        "google:mail:once",
    );
    let calendar_create = mutation_invocation(
        &account,
        ConnectorMutationIntent::CalendarCreateEvent {
            calendar_ref: "primary".to_string(),
            event: calendar_event(),
        },
        "google:create:once",
    );
    let calendar_update = mutation_invocation(
        &account,
        ConnectorMutationIntent::CalendarUpdateEvent {
            calendar_ref: "primary".to_string(),
            event_ref: "event-update-1".to_string(),
            expected_etag: "etag-before".to_string(),
            event: calendar_event(),
        },
        "google:update:once",
    );
    let calendar_cancel = mutation_invocation(
        &account,
        ConnectorMutationIntent::CalendarCancelEvent {
            calendar_ref: "primary".to_string(),
            event_ref: "event-cancel-1".to_string(),
            expected_etag: "etag-cancel".to_string(),
        },
        "google:cancel:once",
    );
    let create_hash = calendar_create
        .mutation
        .as_ref()
        .and_then(|mutation| mutation.intent_hash.clone())
        .unwrap();
    let update_hash = calendar_update
        .mutation
        .as_ref()
        .and_then(|mutation| mutation.intent_hash.clone())
        .unwrap();
    let created_event_id = deterministic_google_event_id("google:create:once").unwrap();
    let transport = ScriptedConnectorHttpTransport::new(vec![
        Ok(json_response(200, json!({"threads": [{"id": "thread-1"}]}))),
        Ok(json_response(
            200,
            json!({"id": "thread-1", "messages": [gmail_message("message-1", "thread-1")]}),
        )),
        Ok(json_response(
            200,
            json!({"id": "thread-1", "messages": [gmail_message("message-1", "thread-1")]}),
        )),
        Ok(json_response(200, json!({"items": []}))),
        Ok(json_response(200, json!({"messages": []}))),
        Ok(json_response(
            200,
            json!({"emailAddress": "owner@example.com", "historyId": "9001"}),
        )),
        Ok(json_response(
            200,
            json!({"items": [], "nextSyncToken": "calendar-sync"}),
        )),
        Ok(json_response(
            200,
            gmail_draft("draft-1", "<reviewed-google@example.com>"),
        )),
        Ok(json_response(200, json!({"id": "sent-message-1"}))),
        Ok(json_response(
            200,
            json!({"messages": [{"id": "sent-message-1"}]}),
        )),
        Ok(json_response(200, json!({"id": created_event_id}))),
        Ok(json_response(
            200,
            json!({
                "id": created_event_id,
                "extendedProperties": {"private": {"dsAgentIntentHash": create_hash}}
            }),
        )),
        Ok(json_response(200, json!({"id": "event-update-1"}))),
        Ok(json_response(
            200,
            json!({
                "id": "event-update-1",
                "etag": "etag-after",
                "extendedProperties": {"private": {"dsAgentIntentHash": update_hash}}
            }),
        )),
        Ok(json_response(204, json!({}))),
        Ok(json_response(404, json!({"error": {"code": 404}}))),
    ]);
    let adapter = GoogleWorkspaceAdapter::new(transport);
    let mut coverage = validate_provider_contract(&adapter, &account).unwrap();
    let mail_search = MailSearchRequest::new("from:sender@example.com".to_string(), 5).unwrap();
    let thread_read = MailThreadRequest::new("thread-1".to_string(), 5).unwrap();
    validate_mail_read_contract(
        &adapter,
        &account,
        &mail_search,
        &thread_read,
        &mut coverage,
    )
    .unwrap();
    let starts_at = DateTime::parse_from_rfc3339("2026-07-15T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let calendar_read =
        CalendarListRequest::new(starts_at, starts_at + Duration::days(1), 5).unwrap();
    validate_calendar_read_contract(&adapter, &account, &calendar_read, &mut coverage).unwrap();
    validate_mail_sync_contract(
        &adapter,
        &account,
        &MailSyncRequest::inbox(5).unwrap(),
        &mut coverage,
    )
    .unwrap();
    validate_calendar_sync_contract(
        &adapter,
        &account,
        &CalendarSyncRequest::new(starts_at, starts_at + Duration::days(1), 5).unwrap(),
        &mut coverage,
    )
    .unwrap();
    for invocation in [
        &mail_send,
        &calendar_create,
        &calendar_update,
        &calendar_cancel,
    ] {
        validate_mutation_contract(&adapter, &adapter, &account, invocation, &mut coverage)
            .unwrap();
    }
    coverage.finish().unwrap();
}

#[test]
fn google_calendar_timeout_reconciles_by_deterministic_id_without_replaying_create() {
    let account = account(GOOGLE_CAPABILITIES.to_vec());
    let mut invocation = mutation_invocation(
        &account,
        ConnectorMutationIntent::CalendarCreateEvent {
            calendar_ref: "primary".to_string(),
            event: calendar_event(),
        },
        "google:timeout:create:once",
    );
    let intent_hash = invocation
        .mutation
        .as_ref()
        .and_then(|mutation| mutation.intent_hash.clone())
        .unwrap();
    let event_id = deterministic_google_event_id(&invocation.idempotency_key).unwrap();
    let transport = ScriptedConnectorHttpTransport::new(vec![
        Err(ConnectorHttpFailure::Timeout),
        Ok(json_response(
            200,
            json!({
                "id": event_id,
                "extendedProperties": {"private": {"dsAgentIntentHash": intent_hash}}
            }),
        )),
    ]);
    let adapter = GoogleWorkspaceAdapter::new(transport);
    assert_eq!(
        adapter.apply_mutation(&account, &invocation).unwrap(),
        ConnectorMutationApplyOutcome::ReconciliationRequired
    );
    invocation.status = ConnectorInvocationStatus::ReconciliationRequired;
    assert!(matches!(
        adapter.reconcile_mutation(&account, &invocation).unwrap(),
        ConnectorReconciliationOutcome::Applied(_)
    ));
    let requests = adapter.transport.take_requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == ConnectorHttpMethod::Post)
            .count(),
        1
    );
    assert_eq!(requests.len(), 2);
}

#[test]
fn google_direct_mail_timeout_sends_once_then_reconciles_read_only() {
    let account = account(GOOGLE_CAPABILITIES.to_vec());
    let mut invocation = mutation_invocation(
        &account,
        ConnectorMutationIntent::MailSendDraft {
            draft_ref: "local-draft-1".to_string(),
            internet_message_id: "<local-google@example.com>".to_string(),
            content: Some(mail_content()),
        },
        "google:direct-mail:once",
    );
    let transport = Arc::new(ScriptedConnectorHttpTransport::new(vec![
        Err(ConnectorHttpFailure::Timeout),
        Ok(json_response(200, json!({"messages": [{"id": "sent-1"}]}))),
    ]));
    let adapter = GoogleWorkspaceAdapter::new(Arc::clone(&transport));
    assert_eq!(
        adapter.apply_mutation(&account, &invocation).unwrap(),
        ConnectorMutationApplyOutcome::ReconciliationRequired
    );
    invocation.status = ConnectorInvocationStatus::ReconciliationRequired;
    assert!(matches!(
        adapter.reconcile_mutation(&account, &invocation).unwrap(),
        ConnectorReconciliationOutcome::Applied(_)
    ));
    let requests = transport.take_requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, ConnectorHttpMethod::Post);
    assert!(requests[0]
        .url
        .ends_with("/gmail/v1/users/me/messages/send"));
    let body = requests[0].body.as_ref().unwrap();
    assert_eq!(body.content_type(), "application/json");
    let value: serde_json::Value = serde_json::from_slice(body.bytes()).unwrap();
    let decoded = general_purpose::URL_SAFE_NO_PAD
        .decode(value["raw"].as_str().unwrap())
        .unwrap();
    let decoded = String::from_utf8(decoded).unwrap();
    assert!(decoded.contains("Message-ID: <local-google@example.com>"));
    assert!(decoded.contains("Exact reviewed body"));
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == ConnectorHttpMethod::Post)
            .count(),
        1
    );
}

#[test]
fn google_mail_snapshot_and_calendar_pagination_replace_tokens_and_keep_page_bounds() {
    let mail_transport = Arc::new(ScriptedConnectorHttpTransport::new(vec![
        Ok(json_response(
            200,
            json!({"messages": [{"id": "message-1"}], "nextPageToken": "mail-page-2"}),
        )),
        Ok(json_response(200, gmail_message("message-1", "thread-1"))),
        Ok(json_response(
            200,
            json!({"messages": [{"id": "message-2"}]}),
        )),
        Ok(json_response(200, gmail_message("message-2", "thread-2"))),
        Ok(json_response(
            200,
            json!({"emailAddress": "owner@example.com", "historyId": "9002"}),
        )),
    ]));
    let adapter = GoogleWorkspaceAdapter::new(Arc::clone(&mail_transport));
    let mail_account = account(vec![ConnectorCapability::MailSyncInbox]);
    let request = MailSyncRequest::inbox(2).unwrap();
    let first = adapter
        .sync_mail_page(&mail_account, &request, None)
        .unwrap();
    let second = match first.continuation() {
        ConnectorSyncContinuation::Next(continuation) => adapter
            .sync_mail_page(&mail_account, &request, Some(continuation))
            .unwrap(),
        ConnectorSyncContinuation::Delta(_) => panic!("snapshot must continue"),
    };
    assert!(matches!(
        second.continuation(),
        ConnectorSyncContinuation::Delta(_)
    ));
    let mail_requests = mail_transport.take_requests();
    let second_page = mail_requests
        .iter()
        .filter_map(|request| reqwest::Url::parse(&request.url).ok())
        .find(|url| {
            url.path() == "/gmail/v1/users/me/messages"
                && url.query_pairs().any(|(key, _)| key == "pageToken")
        })
        .unwrap();
    assert_eq!(
        second_page
            .query_pairs()
            .filter(|(key, _)| key == "pageToken")
            .count(),
        1
    );
    assert!(second_page
        .query_pairs()
        .any(|(key, value)| key == "maxResults" && value == "2"));

    let calendar_transport = Arc::new(ScriptedConnectorHttpTransport::new(vec![
        Ok(json_response(
            200,
            json!({"items": [], "nextPageToken": "cal-page-2"}),
        )),
        Ok(json_response(
            200,
            json!({"items": [], "nextSyncToken": "cal-sync"}),
        )),
    ]));
    let adapter = GoogleWorkspaceAdapter::new(Arc::clone(&calendar_transport));
    let calendar_account = account(vec![ConnectorCapability::CalendarSyncEvents]);
    let starts_at = Utc::now();
    let request = CalendarSyncRequest::new(starts_at, starts_at + Duration::days(1), 2).unwrap();
    let first = adapter
        .sync_calendar_page(&calendar_account, &request, None)
        .unwrap();
    let second = match first.continuation() {
        ConnectorSyncContinuation::Next(continuation) => adapter
            .sync_calendar_page(&calendar_account, &request, Some(continuation))
            .unwrap(),
        ConnectorSyncContinuation::Delta(_) => panic!("calendar snapshot must continue"),
    };
    let delta = match second.continuation() {
        ConnectorSyncContinuation::Delta(continuation) => continuation,
        ConnectorSyncContinuation::Next(_) => panic!("calendar snapshot must finish"),
    };
    let delta = reqwest::Url::parse(delta.expose()).unwrap();
    assert!(delta
        .query_pairs()
        .any(|(key, value)| key == "maxResults" && value == "2"));
    let calendar_requests = calendar_transport.take_requests();
    let second_page = reqwest::Url::parse(&calendar_requests[1].url).unwrap();
    assert_eq!(
        second_page
            .query_pairs()
            .filter(|(key, _)| key == "pageToken")
            .count(),
        1
    );
}
