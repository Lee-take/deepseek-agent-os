use chrono::Utc;
use serde::Serialize;
use tauri::State;
use uuid::Uuid;

use crate::commands::{AgentChatActionProposal, AppState};
use crate::kernel::automation::{ReviewQueueItemStatus, ReviewQueueItemView};
use crate::kernel::connectors::mutation::{ConnectorMailDraftContent, ConnectorMutationIntent};
use crate::kernel::connectors::{
    ConnectorAccount, ConnectorCapability, ConnectorHealth, ConnectorInvocationStatus,
    ConnectorMutationApplyOutcome,
};
use crate::kernel::event_store::{ConnectedWorkReviewView, EventStore};

const CONNECTED_MAIL_REVIEW_ACTION: &str = "connected_mail_review";
const CONNECTED_CALENDAR_REVIEW_ACTION: &str = "connected_calendar_review";

pub(crate) fn is_connected_work_agent_action(action_type: &str) -> bool {
    matches!(
        action_type,
        CONNECTED_MAIL_REVIEW_ACTION | CONNECTED_CALENDAR_REVIEW_ACTION
    )
}

pub(crate) fn dispatch_connected_work_agent_action(
    store: &EventStore,
    action: &mut AgentChatActionProposal,
    source_run_id: Option<Uuid>,
) -> Result<(), String> {
    if !is_connected_work_agent_action(&action.action_type) {
        return Ok(());
    }
    let Some(source_run_id) = source_run_id else {
        mark_connected_work_action_failed(
            action,
            "This connected-account review could not be attached to a durable local run. Nothing was sent or changed.",
        );
        return Ok(());
    };
    let Some(content) = action
        .content
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        mark_connected_work_action_failed(
            action,
            "The proposed connected-account change is incomplete. Nothing was sent or changed.",
        );
        return Ok(());
    };

    let (capability, mail, calendar) = match action.action_type.as_str() {
        CONNECTED_MAIL_REVIEW_ACTION => {
            let draft = match serde_json::from_str::<ConnectorMailDraftContent>(content) {
                Ok(draft) => draft,
                Err(_) => {
                    mark_connected_work_action_failed(
                        action,
                        "The proposed email could not be validated. Nothing was sent.",
                    );
                    return Ok(());
                }
            };
            if draft.validate().is_err() {
                mark_connected_work_action_failed(
                    action,
                    "The proposed email could not be validated. Nothing was sent.",
                );
                return Ok(());
            }
            (ConnectorCapability::MailSendDraft, Some(draft), None)
        }
        CONNECTED_CALENDAR_REVIEW_ACTION => {
            let intent = match serde_json::from_str::<ConnectorMutationIntent>(content) {
                Ok(intent) => intent,
                Err(_) => {
                    mark_connected_work_action_failed(
                        action,
                        "The proposed calendar change could not be validated. Nothing was changed.",
                    );
                    return Ok(());
                }
            };
            if matches!(&intent, ConnectorMutationIntent::MailSendDraft { .. })
                || intent.validate().is_err()
            {
                mark_connected_work_action_failed(
                    action,
                    "The proposed calendar change could not be validated. Nothing was changed.",
                );
                return Ok(());
            }
            (intent.capability(), None, Some(intent))
        }
        _ => return Ok(()),
    };

    let Some(account) = select_connected_work_account(store, capability, action.target.as_deref())?
    else {
        let message = if store
            .list_connector_accounts()
            .map_err(store_error)?
            .into_iter()
            .any(|account| {
                account.health == ConnectorHealth::Connected
                    && account.granted_capabilities.contains(&capability)
            }) {
            "Choose one connected account by its display name, then try again. Nothing was sent or changed."
        } else {
            "Connect an account that permits this exact change, then try again. Nothing was sent or changed."
        };
        action.execution_state = "waiting_prerequisite".to_string();
        action.dispatch_note = Some(message.to_string());
        action.blocked_reason = None;
        return Ok(());
    };

    let review = if let Some(draft) = mail {
        store
            .prepare_foreground_connected_mail_review(source_run_id, &account, draft, Utc::now())
            .map_err(store_error)?
    } else if let Some(intent) = calendar {
        store
            .prepare_foreground_connected_calendar_review(
                source_run_id,
                &account,
                intent,
                Utc::now(),
            )
            .map_err(store_error)?
    } else {
        return Err("connected work action had no validated local review payload".to_string());
    };

    action.execution_state = "succeeded".to_string();
    action.dispatch_note = Some(match review {
        ConnectedWorkReviewView::Mail { .. } => {
            "Email draft is ready for exact review; nothing has been sent.".to_string()
        }
        ConnectedWorkReviewView::Calendar { .. } => {
            "Calendar change is ready for exact review; nothing has been changed.".to_string()
        }
    });
    action.blocked_reason = None;
    action.content = None;
    Ok(())
}

fn select_connected_work_account(
    store: &EventStore,
    capability: ConnectorCapability,
    requested_display_name: Option<&str>,
) -> Result<Option<ConnectorAccount>, String> {
    let mut eligible = store
        .list_connector_accounts()
        .map_err(store_error)?
        .into_iter()
        .filter(|account| {
            account.health == ConnectorHealth::Connected
                && account.granted_capabilities.contains(&capability)
        })
        .collect::<Vec<_>>();
    if eligible.len() == 1 {
        return Ok(eligible.pop());
    }
    let Some(requested) = requested_display_name
        .map(str::trim)
        .filter(|requested| !requested.is_empty())
    else {
        return Ok(None);
    };
    let mut matches = eligible
        .into_iter()
        .filter(|account| account.display_name.eq_ignore_ascii_case(requested))
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        Ok(matches.pop())
    } else {
        Ok(None)
    }
}

fn mark_connected_work_action_failed(action: &mut AgentChatActionProposal, message: &str) {
    action.execution_state = "failed".to_string();
    action.dispatch_note = Some(message.to_string());
    action.blocked_reason = Some(message.to_string());
    action.content = None;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ConnectedWorkExecutionView {
    pub review_id: Uuid,
    pub invocation_status: ConnectorInvocationStatus,
    pub effect_state: ConnectedWorkEffectState,
    pub evidence_ref: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectedWorkEffectState {
    KnownApplied,
    EffectUnknown,
}

fn store_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
pub fn list_connected_work_reviews(
    state: State<'_, AppState>,
) -> Result<Vec<ConnectedWorkReviewView>, String> {
    list_connected_work_reviews_with_state(&state)
}

fn list_connected_work_reviews_with_state(
    state: &AppState,
) -> Result<Vec<ConnectedWorkReviewView>, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store.list_connected_work_reviews().map_err(store_error)
}

#[tauri::command]
pub fn request_connected_work_approval(
    review_id: Uuid,
    action_revision: String,
    state: State<'_, AppState>,
) -> Result<ConnectedWorkReviewView, String> {
    request_connected_work_approval_with_state(&state, review_id, &action_revision)
}

fn request_connected_work_approval_with_state(
    state: &AppState,
    review_id: Uuid,
    action_revision: &str,
) -> Result<ConnectedWorkReviewView, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    let review = store.review_queue_item(review_id).map_err(store_error)?;
    review
        .validate_action_revision(action_revision)
        .map_err(store_error)?;
    match review.status {
        ReviewQueueItemStatus::PendingReview => {
            store
                .prepare_connector_calendar_proposal_approval(
                    review_id,
                    action_revision,
                    Utc::now(),
                )
                .map_err(store_error)?;
        }
        ReviewQueueItemStatus::PendingApproval => {
            store
                .connected_work_invocation_for_review(review_id, action_revision)
                .map_err(store_error)?;
        }
        ReviewQueueItemStatus::Accepted | ReviewQueueItemStatus::Rejected => {
            return Err("connected work review is already resolved".to_string());
        }
    }
    store.connected_work_review(review_id).map_err(store_error)
}

#[tauri::command]
pub fn reject_connected_work_review(
    review_id: Uuid,
    action_revision: String,
    state: State<'_, AppState>,
) -> Result<ReviewQueueItemView, String> {
    let store = state.event_store();
    let store = store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .resolve_review_queue_item(review_id, &action_revision, false, Utc::now())
        .map(|review| review.public_view())
        .map_err(store_error)
}

#[tauri::command]
pub fn approve_and_run_connected_work_review(
    review_id: Uuid,
    action_revision: String,
    state: State<'_, AppState>,
) -> Result<ConnectedWorkExecutionView, String> {
    approve_and_run_connected_work_review_with_state(&state, review_id, &action_revision)
}

fn approve_and_run_connected_work_review_with_state(
    state: &AppState,
    review_id: Uuid,
    action_revision: &str,
) -> Result<ConnectedWorkExecutionView, String> {
    let registry = state.connector_mutations();
    if !registry.execution_enabled() {
        return Err(
            "Connected-account changes are not enabled in this build; nothing was sent or changed."
                .to_string(),
        );
    }
    let store_handle = state.event_store();
    let (pending, account) = {
        let store = store_handle
            .lock()
            .map_err(|_| "event store lock failed".to_string())?;
        let pending = store
            .connected_work_invocation_for_review(review_id, action_revision)
            .map_err(store_error)?;
        if !registry.supports(&pending.provider_id, pending.capability) {
            return Err(
                "This connected-account change is unavailable; nothing was sent or changed."
                    .to_string(),
            );
        }
        let account = store
            .list_connector_accounts()
            .map_err(store_error)?
            .into_iter()
            .find(|account| account.id == pending.account_id)
            .ok_or_else(|| "connected account was not found".to_string())?;
        (pending, account)
    };

    if pending.status == ConnectorInvocationStatus::Running {
        let store = store_handle
            .lock()
            .map_err(|_| "event store lock failed".to_string())?;
        let uncertain = store
            .mark_connector_invocation_reconciliation_required(pending.id, Utc::now())
            .map_err(store_error)?;
        return Ok(ConnectedWorkExecutionView {
            review_id,
            invocation_status: uncertain.status,
            effect_state: ConnectedWorkEffectState::EffectUnknown,
            evidence_ref: None,
        });
    }

    let running = {
        let store = store_handle
            .lock()
            .map_err(|_| "event store lock failed".to_string())?;
        store
            .approve_and_start_connected_work_review(
                review_id,
                action_revision,
                "Approved from the exact connected-work review.".to_string(),
                Utc::now(),
            )
            .map_err(store_error)?
    };
    let provider = registry.provider(&running.provider_id).ok_or_else(|| {
        "This connected-account change is unavailable; nothing was sent or changed.".to_string()
    })?;
    let outcome = provider.apply_mutation(&account, &running);
    let store = store_handle
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    match outcome {
        Ok(ConnectorMutationApplyOutcome::Applied(receipt)) => {
            match store.complete_connector_invocation(running.id, receipt, Utc::now()) {
                Ok(completed) => Ok(ConnectedWorkExecutionView {
                    review_id,
                    invocation_status: completed.status,
                    effect_state: ConnectedWorkEffectState::KnownApplied,
                    evidence_ref: completed
                        .evidence
                        .first()
                        .map(|evidence| evidence.remote_object_ref.clone()),
                }),
                Err(completion_error) => {
                    let current = store
                        .connector_invocation(running.id)
                        .map_err(store_error)?;
                    if current.status == ConnectorInvocationStatus::Succeeded {
                        return Ok(ConnectedWorkExecutionView {
                            review_id,
                            invocation_status: current.status,
                            effect_state: ConnectedWorkEffectState::KnownApplied,
                            evidence_ref: current
                                .evidence
                                .first()
                                .map(|evidence| evidence.remote_object_ref.clone()),
                        });
                    }
                    if current.status != ConnectorInvocationStatus::Running {
                        return Err(store_error(completion_error));
                    }
                    let uncertain = store
                        .mark_connector_invocation_reconciliation_required(running.id, Utc::now())
                        .map_err(store_error)?;
                    Ok(ConnectedWorkExecutionView {
                        review_id,
                        invocation_status: uncertain.status,
                        effect_state: ConnectedWorkEffectState::EffectUnknown,
                        evidence_ref: None,
                    })
                }
            }
        }
        Ok(ConnectorMutationApplyOutcome::ReconciliationRequired) | Err(_) => {
            let uncertain = store
                .mark_connector_invocation_reconciliation_required(running.id, Utc::now())
                .map_err(store_error)?;
            Ok(ConnectedWorkExecutionView {
                review_id,
                invocation_status: uncertain.status,
                effect_state: ConnectedWorkEffectState::EffectUnknown,
                evidence_ref: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use chrono::Duration;

    use super::*;
    use crate::kernel::automation::{AutomationDefinition, AutomationDefinitionStatus};
    use crate::kernel::connectors::domain::MailAddress;
    use crate::kernel::connectors::mutation::{CalendarMutationEvent, ConnectorMutationIntent};
    use crate::kernel::connectors::runtime_registry::ConnectorMutationRegistry;
    use crate::kernel::connectors::{
        ConnectorAccount, ConnectorCapability, ConnectorCredentialHandle, ConnectorHealth,
        ConnectorMutationProvider, ConnectorProvider, FakeConnectorProvider,
    };
    use crate::kernel::event_store::EventStore;

    struct SingleMutationRegistry {
        provider: Arc<FakeConnectorProvider>,
    }

    impl ConnectorMutationRegistry for SingleMutationRegistry {
        fn provider(&self, provider_key: &str) -> Option<&dyn ConnectorMutationProvider> {
            (provider_key == self.provider.provider_id())
                .then_some(self.provider.as_ref() as &dyn ConnectorMutationProvider)
        }

        fn execution_enabled(&self) -> bool {
            true
        }
    }

    fn app_state(store: EventStore, vault_root: &Path) -> AppState {
        #[cfg(windows)]
        {
            AppState::new(store, vault_root).unwrap()
        }
        #[cfg(not(windows))]
        {
            let _ = vault_root;
            AppState::new(store).unwrap()
        }
    }

    fn account(now: chrono::DateTime<Utc>) -> ConnectorAccount {
        account_with_capabilities(now, vec![ConnectorCapability::CalendarCreateEvent])
    }

    fn account_with_capabilities(
        now: chrono::DateTime<Utc>,
        granted_capabilities: Vec<ConnectorCapability>,
    ) -> ConnectorAccount {
        ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "fake".to_string(),
            display_name: "Offline fake account".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities,
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        }
    }

    fn connected_work_action(action_type: &str, content: String) -> AgentChatActionProposal {
        AgentChatActionProposal {
            action_type: action_type.to_string(),
            title: None,
            reason: None,
            risk: Some("medium".to_string()),
            requires_confirmation: false,
            target: None,
            target_location: None,
            destination: None,
            preferred_browser: None,
            content: Some(content),
            capability: None,
            policy_decision: None,
            execution_state: "proposed".to_string(),
            dispatch_note: None,
            permission_request_id: None,
            capability_invocation_id: None,
            workflow_run_id: None,
            blocked_reason: None,
        }
    }

    fn seed_calendar_review(
        store: &EventStore,
        account: &ConnectorAccount,
        now: chrono::DateTime<Utc>,
    ) -> (Uuid, Uuid) {
        store.upsert_connector_account(account).unwrap();
        let definition = AutomationDefinition::once(
            "Prepare a calendar proposal for review.".to_string(),
            "Asia/Shanghai".to_string(),
            now,
        )
        .unwrap();
        store.upsert_automation_definition(&definition).unwrap();
        let (run, agent_run) = store
            .enqueue_manual_automation_agent_run(
                definition.id,
                Uuid::new_v4(),
                now,
                "connected-work-command-test".to_string(),
            )
            .unwrap();
        store
            .claim_agent_run(agent_run.id, "connected-work-test-worker".to_string(), 60)
            .unwrap();
        let intent = ConnectorMutationIntent::CalendarCreateEvent {
            calendar_ref: "primary".to_string(),
            event: CalendarMutationEvent {
                title: "Private command boundary event".to_string(),
                description: Some("Only the foreground review may read this".to_string()),
                location: None,
                starts_at: now + Duration::hours(1),
                ends_at: now + Duration::hours(2),
                timezone: "Asia/Shanghai".to_string(),
                attendees: vec![MailAddress {
                    display_name: None,
                    address: "reviewer@example.com".to_string(),
                }],
                notify_attendees: false,
            },
        };
        let (proposal, review) = store
            .create_connector_calendar_proposal(account, intent, run.id, agent_run.id, now)
            .unwrap();
        (proposal.id, review.id)
    }

    fn review_action(view: &ConnectedWorkReviewView) -> String {
        match view {
            ConnectedWorkReviewView::Mail { review, .. }
            | ConnectedWorkReviewView::Calendar { review, .. } => review.action_revision.clone(),
        }
    }

    #[test]
    fn thin_command_boundary_runs_one_exact_fake_calendar_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let account = account(now);
        let store = EventStore::open_memory().unwrap();
        let (proposal_id, review_id) = seed_calendar_review(&store, &account, now);
        let provider = Arc::new(FakeConnectorProvider::default());
        let state = app_state(store, &temp.path().join("vault"))
            .with_connector_mutation_registry_for_test(Arc::new(SingleMutationRegistry {
                provider: Arc::clone(&provider),
            }));

        let before = list_connected_work_reviews_with_state(&state).unwrap();
        assert_eq!(before.len(), 1);
        let pending = request_connected_work_approval_with_state(
            &state,
            review_id,
            &review_action(&before[0]),
        )
        .unwrap();
        let result = approve_and_run_connected_work_review_with_state(
            &state,
            review_id,
            &review_action(&pending),
        )
        .unwrap();
        assert_eq!(result.effect_state, ConnectedWorkEffectState::KnownApplied);
        assert_eq!(
            result.invocation_status,
            ConnectorInvocationStatus::Succeeded
        );
        assert!(result.evidence_ref.is_some());
        assert_eq!(provider.applied_count(), 1);

        let store = state.event_store();
        let store = store.lock().unwrap();
        let proposal = store.connector_calendar_proposal(proposal_id).unwrap();
        assert_eq!(
            proposal.status,
            crate::kernel::connectors::draft::ConnectorCalendarProposalStatus::Consumed
        );
        assert_eq!(
            store.review_queue_item(review_id).unwrap().status,
            ReviewQueueItemStatus::Accepted
        );
        assert!(store.list_connected_work_reviews().unwrap().is_empty());
    }

    #[test]
    fn production_empty_registry_preserves_pending_approval_without_external_effect() {
        let temp = tempfile::tempdir().unwrap();
        let now = Utc::now();
        let account = account(now);
        let store = EventStore::open_memory().unwrap();
        let (_, review_id) = seed_calendar_review(&store, &account, now);
        let state = app_state(store, &temp.path().join("vault"));
        let before = list_connected_work_reviews_with_state(&state).unwrap();
        let pending = request_connected_work_approval_with_state(
            &state,
            review_id,
            &review_action(&before[0]),
        )
        .unwrap();

        let error = approve_and_run_connected_work_review_with_state(
            &state,
            review_id,
            &review_action(&pending),
        )
        .unwrap_err();
        assert!(error.contains("not enabled"));
        let store = state.event_store();
        let store = store.lock().unwrap();
        assert_eq!(
            store.review_queue_item(review_id).unwrap().status,
            ReviewQueueItemStatus::PendingApproval
        );
        assert_eq!(
            store
                .list_connector_invocations()
                .unwrap()
                .first()
                .unwrap()
                .status,
            ConnectorInvocationStatus::PendingApproval
        );
        assert!(store
            .list_capability_access_records()
            .unwrap()
            .iter()
            .all(|record| record.resolution.is_none()));
    }

    #[test]
    fn chat_mail_action_creates_one_private_review_without_provider_effect() {
        let now = Utc::now();
        let account = account_with_capabilities(now, vec![ConnectorCapability::MailSendDraft]);
        let store = EventStore::open_memory().unwrap();
        store.upsert_connector_account(&account).unwrap();
        let content = ConnectorMailDraftContent {
            to: vec![MailAddress {
                display_name: Some("Private recipient".to_string()),
                address: "private@example.com".to_string(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Private foreground subject".to_string(),
            body_text: "Private foreground body".to_string(),
            in_reply_to: None,
            thread_ref: None,
        };
        let encoded = serde_json::to_string(&content).unwrap();
        let source_run_id = Uuid::new_v4();
        let mut first = connected_work_action(CONNECTED_MAIL_REVIEW_ACTION, encoded.clone());
        dispatch_connected_work_agent_action(&store, &mut first, Some(source_run_id)).unwrap();
        assert_eq!(first.execution_state, "succeeded");
        assert!(first.content.is_none());

        let mut replay = connected_work_action(CONNECTED_MAIL_REVIEW_ACTION, encoded);
        dispatch_connected_work_agent_action(&store, &mut replay, Some(source_run_id)).unwrap();
        assert_eq!(replay.execution_state, "succeeded");
        assert_eq!(store.list_connected_work_reviews().unwrap().len(), 1);
        assert_eq!(store.list_automation_runs().unwrap().len(), 1);
        assert!(store
            .list_automation_definitions()
            .unwrap()
            .iter()
            .all(|definition| definition.status == AutomationDefinitionStatus::Deleted));
        assert!(store.list_recent(100).unwrap().iter().all(|event| !event
            .payload_json
            .contains("Private foreground subject")
            && !event.payload_json.contains("Private foreground body")));
        let review = store.list_connected_work_reviews().unwrap().pop().unwrap();
        assert!(matches!(review, ConnectedWorkReviewView::Mail { .. }));
        assert_eq!(
            store.list_connector_invocations().unwrap()[0].status,
            ConnectorInvocationStatus::PendingApproval
        );
    }

    #[test]
    fn chat_calendar_action_waits_for_review_and_never_executes_provider() {
        let now = Utc::now();
        let account = account(now);
        let store = EventStore::open_memory().unwrap();
        store.upsert_connector_account(&account).unwrap();
        let intent = ConnectorMutationIntent::CalendarCreateEvent {
            calendar_ref: "primary".to_string(),
            event: CalendarMutationEvent {
                title: "Private chat calendar title".to_string(),
                description: None,
                location: None,
                starts_at: now + Duration::hours(1),
                ends_at: now + Duration::hours(2),
                timezone: "Asia/Shanghai".to_string(),
                attendees: Vec::new(),
                notify_attendees: false,
            },
        };
        let mut action = connected_work_action(
            CONNECTED_CALENDAR_REVIEW_ACTION,
            serde_json::to_string(&intent).unwrap(),
        );
        dispatch_connected_work_agent_action(&store, &mut action, Some(Uuid::new_v4())).unwrap();
        assert_eq!(action.execution_state, "succeeded");
        assert!(store.list_connector_invocations().unwrap().is_empty());
        let review = store.list_connected_work_reviews().unwrap().pop().unwrap();
        let ConnectedWorkReviewView::Calendar { review, intent, .. } = review else {
            panic!("calendar action should create a calendar review");
        };
        assert_eq!(review.status, ReviewQueueItemStatus::PendingReview);
        assert_eq!(
            intent,
            ConnectorMutationIntent::CalendarCreateEvent {
                calendar_ref: "primary".to_string(),
                event: CalendarMutationEvent {
                    title: "Private chat calendar title".to_string(),
                    description: None,
                    location: None,
                    starts_at: now + Duration::hours(1),
                    ends_at: now + Duration::hours(2),
                    timezone: "Asia/Shanghai".to_string(),
                    attendees: Vec::new(),
                    notify_attendees: false,
                },
            }
        );
    }
}
