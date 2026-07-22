use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    EventStore, EventStoreError, EventStoreResult, AGENT_RUN_TRANSITIONED_EVENT,
    CAPABILITY_ACCESS_REQUESTED_EVENT, TOOL_INVOCATION_RECORDED_EVENT,
};
use crate::kernel::agent_run::{AgentRunFinish, AgentRunStatus, AgentRunTransition};
use crate::kernel::automation::{
    AutomationDefinition, AutomationDefinitionStatus, AutomationRunStatus, ReviewQueueItem,
    ReviewQueueItemStatus, ReviewQueueItemView,
};
use crate::kernel::connectors::draft::{
    ConnectorCalendarProposal, ConnectorCalendarProposalStatus, ConnectorMailDraft,
    ConnectorMailDraftStatus,
};
use crate::kernel::connectors::mutation::{ConnectorMailDraftContent, ConnectorMutationIntent};
use crate::kernel::connectors::{
    ConnectorAccount, ConnectorCapability, ConnectorHealth, ConnectorInvocation,
    ConnectorInvocationStatus,
};
use crate::kernel::models::{AccessMode, KernelEvent};
use crate::kernel::policy::{
    request_capability_access, CapabilityAccessRequest, CapabilityKind, PolicyDecision,
};
use crate::kernel::tool_runtime::{
    prepare_tool_execution, tool_approval_preview, tool_request_fingerprint, ToolExecutionRequest,
    ToolExecutionStatus, ToolInvocationRecord, CONNECTOR_MUTATE_TOOL_ID,
};

#[derive(Clone)]
pub struct ConnectorMailDraftReviewPreparation {
    pub draft: ConnectorMailDraft,
    pub access_request: CapabilityAccessRequest,
    pub tool_invocation: ToolInvocationRecord,
    pub review_item: ReviewQueueItem,
    pub connector_invocation: ConnectorInvocation,
}

#[derive(Clone)]
pub struct ConnectorCalendarProposalReviewPreparation {
    pub proposal: ConnectorCalendarProposal,
    pub access_request: CapabilityAccessRequest,
    pub tool_invocation: ToolInvocationRecord,
    pub review_item: ReviewQueueItem,
    pub connector_invocation: ConnectorInvocation,
}

/// Explicit local review DTO. It contains private draft content only for the
/// foreground desktop review surface and must never enter Kernel events, model
/// context, work-package exports, logs, or Debug output.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ConnectedWorkReviewView {
    Mail {
        review: ReviewQueueItemView,
        account_display_name: String,
        draft: crate::kernel::connectors::draft::ConnectorMailDraftView,
        content: ConnectorMailDraftContent,
        invocation_status: ConnectorInvocationStatus,
    },
    Calendar {
        review: ReviewQueueItemView,
        account_display_name: String,
        proposal: crate::kernel::connectors::draft::ConnectorCalendarProposalView,
        intent: ConnectorMutationIntent,
        invocation_status: Option<ConnectorInvocationStatus>,
    },
}

const FOREGROUND_CONNECTED_MAIL_GOAL: &str = "Prepare an exact connected-account email for review.";
const FOREGROUND_CONNECTED_CALENDAR_GOAL: &str =
    "Prepare an exact connected-account calendar change for review.";
const MAX_FOREGROUND_CONNECTED_WORK_ATTEMPTS: u32 = 8;

enum ForegroundConnectedWorkReservation {
    Replay(Box<ConnectedWorkReviewView>),
    New {
        definition_id: Uuid,
        manual_invocation_id: Uuid,
    },
}

pub(super) fn migrate(store: &EventStore) -> EventStoreResult<()> {
    store.conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS connector_mail_drafts (
            id TEXT PRIMARY KEY NOT NULL,
            provider_id TEXT NOT NULL,
            account_id TEXT NOT NULL,
            account_generation INTEGER NOT NULL,
            draft_json TEXT NOT NULL,
            status TEXT NOT NULL,
            revision INTEGER NOT NULL,
            consumed_by_invocation_id TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_connector_mail_drafts_account
            ON connector_mail_drafts (account_id, account_generation, status, updated_at);

        CREATE TABLE IF NOT EXISTS connector_mail_draft_reviews (
            draft_id TEXT PRIMARY KEY NOT NULL,
            draft_action_revision TEXT NOT NULL,
            automation_run_id TEXT NOT NULL UNIQUE,
            agent_run_id TEXT,
            access_request_id TEXT NOT NULL UNIQUE,
            tool_invocation_id TEXT NOT NULL UNIQUE,
            review_item_id TEXT NOT NULL UNIQUE,
            connector_invocation_id TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS connector_calendar_proposals (
            id TEXT PRIMARY KEY NOT NULL,
            provider_id TEXT NOT NULL,
            account_id TEXT NOT NULL,
            account_generation INTEGER NOT NULL,
            proposal_json TEXT NOT NULL,
            status TEXT NOT NULL,
            revision INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_connector_calendar_proposals_account
            ON connector_calendar_proposals
               (account_id, account_generation, status, updated_at);

        CREATE TABLE IF NOT EXISTS connector_calendar_proposal_reviews (
            proposal_id TEXT PRIMARY KEY NOT NULL,
            automation_run_id TEXT NOT NULL UNIQUE,
            agent_run_id TEXT NOT NULL,
            review_item_id TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL
        );
        "#,
    )?;
    for (column, migration) in [
        (
            "proposal_action_revision",
            "ALTER TABLE connector_calendar_proposal_reviews ADD COLUMN proposal_action_revision TEXT",
        ),
        (
            "access_request_id",
            "ALTER TABLE connector_calendar_proposal_reviews ADD COLUMN access_request_id TEXT",
        ),
        (
            "tool_invocation_id",
            "ALTER TABLE connector_calendar_proposal_reviews ADD COLUMN tool_invocation_id TEXT",
        ),
        (
            "connector_invocation_id",
            "ALTER TABLE connector_calendar_proposal_reviews ADD COLUMN connector_invocation_id TEXT",
        ),
    ] {
        super::ensure_sqlite_column(
            &store.conn,
            "connector_calendar_proposal_reviews",
            column,
            migration,
        )?;
    }
    store.conn.execute_batch(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_connector_calendar_review_access
          ON connector_calendar_proposal_reviews (access_request_id)
          WHERE access_request_id IS NOT NULL;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_connector_calendar_review_tool
          ON connector_calendar_proposal_reviews (tool_invocation_id)
          WHERE tool_invocation_id IS NOT NULL;
        CREATE UNIQUE INDEX IF NOT EXISTS idx_connector_calendar_review_invocation
          ON connector_calendar_proposal_reviews (connector_invocation_id)
          WHERE connector_invocation_id IS NOT NULL;
        "#,
    )?;
    Ok(())
}

impl EventStore {
    pub fn create_connector_calendar_proposal(
        &self,
        account: &ConnectorAccount,
        intent: crate::kernel::connectors::mutation::ConnectorMutationIntent,
        automation_run_id: Uuid,
        agent_run_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<(ConnectorCalendarProposal, ReviewQueueItem)> {
        let capability = intent.capability();
        if account.health != ConnectorHealth::Connected
            || !matches!(
                capability,
                ConnectorCapability::CalendarCreateEvent
                    | ConnectorCapability::CalendarUpdateEvent
                    | ConnectorCapability::CalendarCancelEvent
            )
            || !account.granted_capabilities.contains(&capability)
        {
            return Err(invalid(
                "connector account cannot create this calendar proposal",
            ));
        }
        let generation = self.connector_account_sync_generation(account)?;
        let proposal = ConnectorCalendarProposal::new(
            account.provider_id.clone(),
            account.id,
            generation,
            intent,
            now,
        )
        .map_err(invalid)?;
        let mut automation_run = self.automation_run(automation_run_id)?;
        if automation_run.agent_run_id != Some(agent_run_id)
            || automation_run.review_queue_item_id.is_some()
            || !self
                .list_agent_run_records()?
                .into_iter()
                .any(|record| record.id == agent_run_id && record.status == AgentRunStatus::Running)
        {
            return Err(invalid(
                "automation run cannot create this calendar proposal review",
            ));
        }
        let review_item = ReviewQueueItem {
            id: Uuid::new_v4(),
            automation_run_id,
            agent_run_id: Some(agent_run_id),
            tool_invocation_id: None,
            status: ReviewQueueItemStatus::PendingReview,
            preview_fingerprint: Some(proposal.intent.hash().map_err(invalid)?),
            revision: 0,
            title: "Review this calendar proposal".to_string(),
            evidence_ref: Some(format!("local-calendar-proposal:{}", proposal.id)),
            created_at: now,
            updated_at: now,
        };
        automation_run.review_queue_item_id = Some(review_item.id);
        automation_run.updated_at = now;
        let transition = AgentRunTransition::new(
            agent_run_id,
            AgentRunStatus::WaitingForPrerequisite,
            "Calendar proposal is ready for local review before any external change.".to_string(),
            None,
        )
        .map_err(invalid)?;
        let transition_event = KernelEvent::new(AGENT_RUN_TRANSITIONED_EVENT, &transition)?;
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let inserted = transaction.execute(
            r#"INSERT INTO connector_calendar_proposals
               (id, provider_id, account_id, account_generation, proposal_json,
                status, revision, created_at, updated_at)
               SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9
               WHERE EXISTS (
                 SELECT 1 FROM connector_accounts AS account
                 JOIN connector_account_generations AS generation
                   ON generation.account_id = account.id
                 WHERE account.id = ?3 AND account.provider_id = ?2
                   AND account.health = ?10 AND generation.generation = ?4
               )"#,
            params![
                proposal.id.to_string(),
                proposal.provider_id,
                proposal.account_id.to_string(),
                i64::try_from(proposal.account_generation)
                    .map_err(|_| invalid("connector calendar generation is too large"))?,
                serde_json::to_string(&proposal)?,
                calendar_status_text(proposal.status),
                i64::try_from(proposal.revision)
                    .map_err(|_| invalid("connector calendar revision is too large"))?,
                timestamp(proposal.created_at),
                timestamp(proposal.updated_at),
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
        )?;
        if inserted != 1 {
            return Err(invalid(
                "connector calendar account changed during proposal creation",
            ));
        }
        transaction.execute(
            r#"INSERT INTO review_queue_items
               (id, automation_run_id, item_json, status, revision, updated_at)
               VALUES (?1, ?2, ?3, ?4, 0, ?5)"#,
            params![
                review_item.id.to_string(),
                automation_run_id.to_string(),
                serde_json::to_string(&review_item)?,
                serde_json::to_string(&review_item.status)?,
                timestamp(review_item.updated_at),
            ],
        )?;
        transaction.execute(
            "UPDATE automation_runs SET run_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![
                automation_run_id.to_string(),
                serde_json::to_string(&automation_run)?,
                timestamp(automation_run.updated_at),
            ],
        )?;
        transaction.execute(
            r#"INSERT INTO connector_calendar_proposal_reviews
               (proposal_id, automation_run_id, agent_run_id, review_item_id, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5)"#,
            params![
                proposal.id.to_string(),
                automation_run_id.to_string(),
                agent_run_id.to_string(),
                review_item.id.to_string(),
                timestamp(now),
            ],
        )?;
        Self::insert_kernel_event(&transaction, &transition_event)?;
        transaction.commit()?;
        Ok((proposal, review_item))
    }

    pub fn connector_calendar_proposal(
        &self,
        id: Uuid,
    ) -> EventStoreResult<ConnectorCalendarProposal> {
        let json = self
            .conn
            .query_row(
                "SELECT proposal_json FROM connector_calendar_proposals WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| invalid("connector calendar proposal was not found"))?;
        let proposal: ConnectorCalendarProposal = serde_json::from_str(&json)?;
        proposal.validate().map_err(invalid)?;
        Ok(proposal)
    }

    pub fn prepare_connector_calendar_proposal_approval(
        &self,
        review_id: Uuid,
        expected_review_action_revision: &str,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorCalendarProposalReviewPreparation> {
        let binding = self
            .connector_calendar_proposal_review_binding(review_id)?
            .ok_or_else(|| invalid("connected calendar review was not found"))?;
        let mut review_item = self.review_queue_item(review_id)?;
        review_item
            .validate_action_revision(expected_review_action_revision)
            .map_err(invalid)?;
        if binding.prepared() {
            if review_item.status != ReviewQueueItemStatus::PendingApproval {
                return Err(invalid(
                    "connected calendar review is no longer awaiting approval",
                ));
            }
            return self.load_connector_calendar_proposal_review_preparation(binding);
        }
        if review_item.status != ReviewQueueItemStatus::PendingReview
            || review_item.automation_run_id != binding.automation_run_id
            || review_item.agent_run_id != Some(binding.agent_run_id)
        {
            return Err(invalid("connected calendar review is stale or unavailable"));
        }
        let mut proposal = self.connector_calendar_proposal(binding.proposal_id)?;
        if proposal.status != ConnectorCalendarProposalStatus::PendingReview {
            return Err(invalid(
                "connector calendar proposal is not pending local review",
            ));
        }
        let previous_proposal_status = proposal.status;
        let previous_proposal_revision = proposal.revision;
        let previous_review_revision = review_item.revision;
        proposal
            .freeze(&proposal.action_revision(), now)
            .map_err(invalid)?;
        let account = self
            .list_connector_accounts()?
            .into_iter()
            .find(|account| account.id == proposal.account_id)
            .ok_or_else(|| invalid("connector calendar account was not found"))?;
        let capability = proposal.intent.capability();
        if account.provider_id != proposal.provider_id
            || account.health != ConnectorHealth::Connected
            || !account.granted_capabilities.contains(&capability)
            || self.connector_account_sync_generation(&account)? != proposal.account_generation
        {
            return Err(invalid(
                "connector calendar account changed before exact approval",
            ));
        }
        let automation_run = self.automation_run(binding.automation_run_id)?;
        if automation_run.agent_run_id != Some(binding.agent_run_id)
            || automation_run.review_queue_item_id != Some(review_id)
            || !self.list_agent_run_records()?.into_iter().any(|record| {
                record.id == binding.agent_run_id
                    && record.status == AgentRunStatus::WaitingForPrerequisite
            })
        {
            return Err(invalid(
                "calendar proposal owner is not waiting for local review",
            ));
        }

        let intent = proposal.mutation_intent().map_err(invalid)?;
        let intent_hash = intent.hash().map_err(invalid)?;
        let request = ToolExecutionRequest {
            tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            input: serde_json::json!({
                "provider_id": proposal.provider_id,
                "account_id": proposal.account_id.to_string(),
                "account_generation": proposal.account_generation,
                "capability": capability.contract_name(),
                "target_ref": intent.target_ref(),
                "preview_hash": intent_hash,
                "intent_hash": intent_hash,
                "idempotency_key": format!(
                    "connector-calendar-proposal:{}:{}",
                    proposal.id, proposal.revision
                ),
                "automation_run_id": binding.automation_run_id.to_string(),
            }),
            access_mode: AccessMode::FullAccess,
            run_id: Some(binding.agent_run_id),
        };
        let plan = prepare_tool_execution(&request).map_err(invalid)?;
        if plan.policy_decision != PolicyDecision::Ask {
            return Err(invalid(
                "connector calendar mutation must require exact approval",
            ));
        }
        let mut access_request =
            request_capability_access(AccessMode::FullAccess, CapabilityKind::ConnectorWrite)
                .map_err(invalid)?;
        access_request
            .bind_exact_tool(
                CONNECTOR_MUTATE_TOOL_ID,
                tool_request_fingerprint(&request),
                tool_approval_preview(&request),
            )
            .map_err(invalid)?;
        let tool_invocation =
            ToolInvocationRecord::waiting_for_confirmation(&plan, access_request.id);
        review_item
            .edit(
                "Review this exact calendar change".to_string(),
                Some(tool_invocation.request_fingerprint.clone()),
                now,
            )
            .and_then(|_| {
                review_item.request_approval(
                    tool_invocation.id,
                    tool_invocation.request_fingerprint.clone(),
                    now,
                )
            })
            .map_err(invalid)?;
        let connector_invocation =
            ConnectorInvocation::from_tool_request(&request, &tool_invocation)
                .and_then(|invocation| invocation.bind_intent(intent))
                .map_err(invalid)?;
        let transition = AgentRunTransition::new(
            binding.agent_run_id,
            AgentRunStatus::WaitingForConfirmation,
            "Exact connected-account calendar change is frozen and waiting for approval."
                .to_string(),
            Some(tool_invocation.id),
        )
        .map_err(invalid)?;
        let access_event = KernelEvent::new(CAPABILITY_ACCESS_REQUESTED_EVENT, &access_request)?;
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool_invocation)?;
        let agent_event = KernelEvent::new(AGENT_RUN_TRANSITIONED_EVENT, &transition)?;
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let proposal_updated = transaction.execute(
            r#"UPDATE connector_calendar_proposals
               SET proposal_json = ?2, status = ?3, revision = ?4, updated_at = ?5
               WHERE id = ?1 AND status = ?6 AND revision = ?7
                 AND EXISTS (
                   SELECT 1 FROM connector_accounts AS account
                   JOIN connector_account_generations AS generation
                     ON generation.account_id = account.id
                   WHERE account.id = connector_calendar_proposals.account_id
                     AND account.provider_id = connector_calendar_proposals.provider_id
                     AND account.health = ?8
                     AND generation.generation = connector_calendar_proposals.account_generation
                 )"#,
            params![
                proposal.id.to_string(),
                serde_json::to_string(&proposal)?,
                calendar_status_text(proposal.status),
                i64::try_from(proposal.revision)
                    .map_err(|_| invalid("connector calendar revision is too large"))?,
                timestamp(proposal.updated_at),
                calendar_status_text(previous_proposal_status),
                i64::try_from(previous_proposal_revision)
                    .map_err(|_| invalid("connector calendar revision is too large"))?,
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
        )?;
        if proposal_updated != 1 {
            return Err(invalid(
                "connector calendar proposal changed during exact review",
            ));
        }
        let review_updated = transaction.execute(
            r#"UPDATE review_queue_items
               SET item_json = ?2, status = ?3, revision = ?4, updated_at = ?5
               WHERE id = ?1 AND revision = ?6"#,
            params![
                review_item.id.to_string(),
                serde_json::to_string(&review_item)?,
                serde_json::to_string(&review_item.status)?,
                i64::from(review_item.revision),
                timestamp(review_item.updated_at),
                i64::from(previous_review_revision),
            ],
        )?;
        if review_updated != 1 {
            return Err(invalid(
                "connector calendar review changed during exact preparation",
            ));
        }
        for event in [&access_event, &tool_event, &agent_event] {
            Self::insert_kernel_event(&transaction, event)?;
        }
        let invocation_inserted = transaction.execute(
            r#"INSERT INTO connector_invocations
               (id, account_id, account_generation, idempotency_key,
                invocation_json, status, updated_at)
               SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7
               WHERE EXISTS (
                 SELECT 1 FROM connector_accounts AS account
                 JOIN connector_account_generations AS generation
                   ON generation.account_id = account.id
                 WHERE account.id = ?2 AND account.provider_id = ?8
                   AND account.health = ?9 AND generation.generation = ?3
               )"#,
            params![
                connector_invocation.id.to_string(),
                connector_invocation.account_id.to_string(),
                i64::try_from(proposal.account_generation)
                    .map_err(|_| invalid("connector calendar generation is too large"))?,
                connector_invocation.idempotency_key,
                serde_json::to_string(&connector_invocation)?,
                serde_json::to_string(&connector_invocation.status)?,
                timestamp(connector_invocation.updated_at),
                connector_invocation.provider_id,
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
        )?;
        if invocation_inserted != 1 {
            return Err(invalid(
                "connector calendar account changed during exact preparation",
            ));
        }
        let binding_updated = transaction.execute(
            r#"UPDATE connector_calendar_proposal_reviews
               SET proposal_action_revision = ?2, access_request_id = ?3,
                   tool_invocation_id = ?4, connector_invocation_id = ?5
               WHERE proposal_id = ?1 AND access_request_id IS NULL
                 AND tool_invocation_id IS NULL AND connector_invocation_id IS NULL"#,
            params![
                proposal.id.to_string(),
                proposal.action_revision(),
                access_request.id.to_string(),
                tool_invocation.id.to_string(),
                connector_invocation.id.to_string(),
            ],
        )?;
        if binding_updated != 1 {
            return Err(invalid("connector calendar approval binding changed"));
        }
        transaction.commit()?;
        Ok(ConnectorCalendarProposalReviewPreparation {
            proposal,
            access_request,
            tool_invocation,
            review_item,
            connector_invocation,
        })
    }

    pub fn create_connector_mail_draft(
        &self,
        account: &ConnectorAccount,
        content: ConnectorMailDraftContent,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorMailDraft> {
        if account.health != ConnectorHealth::Connected
            || !account
                .granted_capabilities
                .contains(&ConnectorCapability::MailSendDraft)
        {
            return Err(invalid("connector account cannot create mail drafts"));
        }
        let generation = self.connector_account_sync_generation(account)?;
        let draft = ConnectorMailDraft::new(
            account.provider_id.clone(),
            account.id,
            generation,
            content,
            now,
        )
        .map_err(invalid)?;
        self.conn.execute(
            r#"INSERT INTO connector_mail_drafts
               (id, provider_id, account_id, account_generation, draft_json, status, revision,
                consumed_by_invocation_id, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9)"#,
            params![
                draft.id.to_string(),
                draft.provider_id,
                draft.account_id.to_string(),
                i64::try_from(draft.account_generation)
                    .map_err(|_| invalid("connector mail draft generation is too large"))?,
                serde_json::to_string(&draft)?,
                status_text(draft.status),
                i64::try_from(draft.revision)
                    .map_err(|_| invalid("connector mail draft revision is too large"))?,
                timestamp(draft.created_at),
                timestamp(draft.updated_at),
            ],
        )?;
        Ok(draft)
    }

    pub fn connector_mail_draft(&self, id: Uuid) -> EventStoreResult<ConnectorMailDraft> {
        let json = self
            .conn
            .query_row(
                "SELECT draft_json FROM connector_mail_drafts WHERE id = ?1",
                params![id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| invalid("connector mail draft was not found"))?;
        let draft: ConnectorMailDraft = serde_json::from_str(&json)?;
        draft.validate().map_err(invalid)?;
        Ok(draft)
    }

    pub fn update_connector_mail_draft(
        &self,
        id: Uuid,
        expected_action_revision: &str,
        content: ConnectorMailDraftContent,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorMailDraft> {
        let mut draft = self.connector_mail_draft(id)?;
        let expected_status = draft.status;
        let expected_revision = draft.revision;
        draft
            .update(expected_action_revision, content, now)
            .map_err(invalid)?;
        self.save_connector_mail_draft(&draft, expected_status, expected_revision)?;
        Ok(draft)
    }

    pub fn freeze_connector_mail_draft(
        &self,
        id: Uuid,
        expected_action_revision: &str,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorMailDraft> {
        let mut draft = self.connector_mail_draft(id)?;
        let expected_status = draft.status;
        let expected_revision = draft.revision;
        draft
            .freeze(expected_action_revision, now)
            .map_err(invalid)?;
        self.save_connector_mail_draft(&draft, expected_status, expected_revision)?;
        Ok(draft)
    }

    pub fn consume_connector_mail_draft(
        &self,
        id: Uuid,
        invocation_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorMailDraft> {
        let mut draft = self.connector_mail_draft(id)?;
        if draft.status == ConnectorMailDraftStatus::Consumed
            && draft.consumed_by_invocation_id == Some(invocation_id)
        {
            return Ok(draft);
        }
        let invocation = self.connector_invocation(invocation_id)?;
        let expected_target = format!("local-draft:{}", draft.id);
        let actual_target = invocation
            .mutation_intent()
            .ok()
            .map(|intent| intent.target_ref());
        if invocation.status != ConnectorInvocationStatus::Succeeded
            || invocation.provider_id != draft.provider_id
            || invocation.account_id != draft.account_id
            || invocation.account_generation != Some(draft.account_generation)
            || actual_target != Some(expected_target.as_str())
        {
            return Err(invalid(
                "connector mail draft completion does not match the successful invocation",
            ));
        }
        let expected_status = draft.status;
        let expected_revision = draft.revision;
        draft.consume(invocation_id, now).map_err(invalid)?;
        self.save_connector_mail_draft(&draft, expected_status, expected_revision)?;
        Ok(draft)
    }

    pub fn consume_connector_calendar_proposal(
        &self,
        id: Uuid,
        invocation_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorCalendarProposal> {
        let mut proposal = self.connector_calendar_proposal(id)?;
        if proposal.status == ConnectorCalendarProposalStatus::Consumed
            && proposal.consumed_by_invocation_id == Some(invocation_id)
        {
            return Ok(proposal);
        }
        let invocation = self.connector_invocation(invocation_id)?;
        if invocation.status != ConnectorInvocationStatus::Succeeded
            || invocation.provider_id != proposal.provider_id
            || invocation.account_id != proposal.account_id
            || invocation.account_generation != Some(proposal.account_generation)
            || invocation.mutation_intent().ok() != Some(&proposal.intent)
        {
            return Err(invalid(
                "connector calendar proposal completion does not match the successful invocation",
            ));
        }
        let expected_status = proposal.status;
        let expected_revision = proposal.revision;
        proposal.consume(invocation_id, now).map_err(invalid)?;
        self.save_connector_calendar_proposal(&proposal, expected_status, expected_revision)?;
        Ok(proposal)
    }

    pub fn list_connected_work_reviews(&self) -> EventStoreResult<Vec<ConnectedWorkReviewView>> {
        let ids = {
            let mut statement = self.conn.prepare(
                r#"SELECT review_item_id FROM connector_mail_draft_reviews
                   UNION ALL
                   SELECT review_item_id FROM connector_calendar_proposal_reviews"#,
            )?;
            let values = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            values
                .into_iter()
                .map(|value| Uuid::parse_str(&value).map_err(Into::into))
                .collect::<EventStoreResult<Vec<_>>>()?
        };
        let mut reviews = ids
            .into_iter()
            .filter_map(|id| match self.review_queue_item(id) {
                Ok(item)
                    if matches!(
                        item.status,
                        ReviewQueueItemStatus::PendingReview
                            | ReviewQueueItemStatus::PendingApproval
                    ) =>
                {
                    Some(self.connected_work_review(id))
                }
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<EventStoreResult<Vec<_>>>()?;
        reviews.sort_by_key(|review| match review {
            ConnectedWorkReviewView::Mail { review, .. }
            | ConnectedWorkReviewView::Calendar { review, .. } => review.updated_at,
        });
        Ok(reviews)
    }

    pub fn connected_work_review(
        &self,
        review_id: Uuid,
    ) -> EventStoreResult<ConnectedWorkReviewView> {
        let review = self.review_queue_item(review_id)?;
        if let Some(binding) = self.connector_mail_draft_review_binding_by_review(review_id)? {
            let draft = self.connector_mail_draft(binding.draft_id)?;
            let account_display_name = self.connected_work_account_display_name(
                draft.account_id,
                &draft.provider_id,
                draft.account_generation,
            )?;
            let invocation = self.connector_invocation(binding.connector_invocation_id)?;
            return Ok(ConnectedWorkReviewView::Mail {
                review: review.public_view(),
                account_display_name,
                draft: draft.public_view(),
                content: draft.content,
                invocation_status: invocation.status,
            });
        }
        let binding = self
            .connector_calendar_proposal_review_binding(review_id)?
            .ok_or_else(|| invalid("connected work review was not found"))?;
        let proposal = self.connector_calendar_proposal(binding.proposal_id)?;
        let account_display_name = self.connected_work_account_display_name(
            proposal.account_id,
            &proposal.provider_id,
            proposal.account_generation,
        )?;
        let invocation_status = binding
            .connector_invocation_id
            .map(|id| {
                self.connector_invocation(id)
                    .map(|invocation| invocation.status)
            })
            .transpose()?;
        Ok(ConnectedWorkReviewView::Calendar {
            review: review.public_view(),
            account_display_name,
            proposal: proposal.public_view(),
            intent: proposal.intent,
            invocation_status,
        })
    }

    pub fn prepare_foreground_connected_mail_review(
        &self,
        source_run_id: Uuid,
        account: &ConnectorAccount,
        content: ConnectorMailDraftContent,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectedWorkReviewView> {
        content.validate().map_err(invalid)?;
        let payload = serde_json::to_vec(&content)?;
        let reservation =
            self.foreground_connected_work_reservation(source_run_id, "mail", &payload, now)?;
        let ForegroundConnectedWorkReservation::New {
            definition_id,
            manual_invocation_id,
        } = reservation
        else {
            let ForegroundConnectedWorkReservation::Replay(review) = reservation else {
                unreachable!()
            };
            return Ok(*review);
        };
        let result = (|| {
            let (run, agent_run) = self.start_foreground_connected_work_run(
                definition_id,
                manual_invocation_id,
                FOREGROUND_CONNECTED_MAIL_GOAL,
                now,
            )?;
            let editing = self.create_connector_mail_draft(account, content, now)?;
            let frozen =
                self.freeze_connector_mail_draft(editing.id, &editing.action_revision(), now)?;
            let prepared = self.prepare_connector_mail_draft_review(
                frozen.id,
                &frozen.action_revision(),
                run.id,
                Some(agent_run.id),
                now,
            )?;
            self.finish_foreground_connected_work_preparation(definition_id, now)?;
            self.connected_work_review(prepared.review_item.id)
        })();
        if result.is_err() {
            let _ = self.fail_foreground_connected_work_preparation(definition_id, now);
        }
        result
    }

    pub fn prepare_foreground_connected_calendar_review(
        &self,
        source_run_id: Uuid,
        account: &ConnectorAccount,
        intent: ConnectorMutationIntent,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectedWorkReviewView> {
        intent.validate().map_err(invalid)?;
        let payload = serde_json::to_vec(&intent)?;
        let reservation =
            self.foreground_connected_work_reservation(source_run_id, "calendar", &payload, now)?;
        let ForegroundConnectedWorkReservation::New {
            definition_id,
            manual_invocation_id,
        } = reservation
        else {
            let ForegroundConnectedWorkReservation::Replay(review) = reservation else {
                unreachable!()
            };
            return Ok(*review);
        };
        let result = (|| {
            let (run, agent_run) = self.start_foreground_connected_work_run(
                definition_id,
                manual_invocation_id,
                FOREGROUND_CONNECTED_CALENDAR_GOAL,
                now,
            )?;
            let (_, review) = self.create_connector_calendar_proposal(
                account,
                intent,
                run.id,
                agent_run.id,
                now,
            )?;
            self.finish_foreground_connected_work_preparation(definition_id, now)?;
            self.connected_work_review(review.id)
        })();
        if result.is_err() {
            let _ = self.fail_foreground_connected_work_preparation(definition_id, now);
        }
        result
    }

    fn foreground_connected_work_reservation(
        &self,
        source_run_id: Uuid,
        kind: &str,
        payload: &[u8],
        now: DateTime<Utc>,
    ) -> EventStoreResult<ForegroundConnectedWorkReservation> {
        let definitions = self.list_automation_definitions()?;
        let runs = self.list_automation_runs()?;
        let agent_runs = self.list_agent_run_records()?;
        for attempt in 0..MAX_FOREGROUND_CONNECTED_WORK_ATTEMPTS {
            let (definition_id, manual_invocation_id) =
                foreground_connected_work_ids(source_run_id, kind, payload, attempt);
            let definition = definitions
                .iter()
                .find(|definition| definition.id == definition_id);
            let trigger = format!("manual:{manual_invocation_id}");
            let run = runs.iter().find(|run| {
                run.definition_id == definition_id && run.trigger_window_key == trigger
            });
            if let Some(run) = run {
                if let Some(review_id) = run.review_queue_item_id {
                    self.finish_foreground_connected_work_preparation(definition_id, now)?;
                    return self
                        .connected_work_review(review_id)
                        .map(Box::new)
                        .map(ForegroundConnectedWorkReservation::Replay);
                }
                let agent_terminal = run.agent_run_id.is_some_and(|agent_run_id| {
                    agent_runs.iter().any(|agent_run| {
                        agent_run.id == agent_run_id
                            && matches!(
                                agent_run.status,
                                AgentRunStatus::Completed
                                    | AgentRunStatus::Failed
                                    | AgentRunStatus::Cancelled
                            )
                    })
                });
                if matches!(
                    run.status,
                    AutomationRunStatus::Completed
                        | AutomationRunStatus::Failed
                        | AutomationRunStatus::Cancelled
                ) || agent_terminal
                {
                    continue;
                }
                return Err(invalid(
                    "foreground connected work preparation is already in progress",
                ));
            }
            match definition {
                Some(definition) if definition.status == AutomationDefinitionStatus::Deleted => {
                    continue;
                }
                Some(_) => {
                    return Err(invalid(
                        "foreground connected work preparation is already in progress",
                    ));
                }
                None => {
                    return Ok(ForegroundConnectedWorkReservation::New {
                        definition_id,
                        manual_invocation_id,
                    });
                }
            }
        }
        Err(invalid(
            "foreground connected work exhausted its safe preparation attempts",
        ))
    }

    fn start_foreground_connected_work_run(
        &self,
        definition_id: Uuid,
        manual_invocation_id: Uuid,
        goal: &str,
        now: DateTime<Utc>,
    ) -> EventStoreResult<(
        crate::kernel::automation::AutomationRun,
        crate::kernel::agent_run::AgentRunStart,
    )> {
        if self
            .list_automation_definitions()?
            .into_iter()
            .any(|definition| definition.id == definition_id)
        {
            return Err(invalid(
                "foreground connected work definition exists without its durable review",
            ));
        }
        let definition = AutomationDefinition::once_with_id(
            definition_id,
            goal.to_string(),
            "UTC".to_string(),
            now,
            now,
        )
        .map_err(invalid)?;
        self.upsert_automation_definition(&definition)?;
        let (run, agent_run) = self.enqueue_manual_automation_agent_run(
            definition_id,
            manual_invocation_id,
            now,
            format!("connected-work:{definition_id}"),
        )?;
        self.claim_agent_run(agent_run.id, "foreground-connected-work".to_string(), 60)?;
        Ok((run, agent_run))
    }

    fn finish_foreground_connected_work_preparation(
        &self,
        definition_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        let definition = self.automation_definition(definition_id)?;
        if definition.status != AutomationDefinitionStatus::Deleted {
            self.set_automation_definition_status(
                definition_id,
                AutomationDefinitionStatus::Deleted,
                now,
            )?;
        }
        self.reconcile_automation_agent_runs(now)?;
        Ok(())
    }

    fn fail_foreground_connected_work_preparation(
        &self,
        definition_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<bool> {
        let run = self
            .list_automation_runs()?
            .into_iter()
            .find(|run| run.definition_id == definition_id);
        if run
            .as_ref()
            .and_then(|run| run.review_queue_item_id)
            .is_some()
        {
            self.finish_foreground_connected_work_preparation(definition_id, now)?;
            return Ok(false);
        }
        let mut changed = false;
        if let Some(agent_run_id) = run.as_ref().and_then(|run| run.agent_run_id) {
            if let Some(agent_run) = self
                .list_agent_run_records()?
                .into_iter()
                .find(|agent_run| agent_run.id == agent_run_id)
            {
                if !matches!(
                    agent_run.status,
                    AgentRunStatus::Completed | AgentRunStatus::Failed | AgentRunStatus::Cancelled
                ) {
                    let finish = AgentRunFinish::new(
                        agent_run_id,
                        AgentRunStatus::Failed,
                        None,
                        Some(
                            "Foreground connected work was interrupted before review; no external effect was attempted."
                                .to_string(),
                        ),
                    )
                    .map_err(invalid)?;
                    self.append_agent_run_finish(&finish)?;
                    changed = true;
                }
            }
        }
        let definition = self.automation_definition(definition_id)?;
        if definition.status != AutomationDefinitionStatus::Deleted {
            self.set_automation_definition_status(
                definition_id,
                AutomationDefinitionStatus::Deleted,
                now,
            )?;
            changed = true;
        }
        self.reconcile_automation_agent_runs(now)?;
        Ok(changed)
    }

    pub(crate) fn reconcile_interrupted_foreground_connected_work_preparations(
        &self,
        now: DateTime<Utc>,
    ) -> EventStoreResult<usize> {
        let definitions = self
            .list_automation_definitions()?
            .into_iter()
            .filter(|definition| {
                matches!(
                    definition.goal.as_str(),
                    FOREGROUND_CONNECTED_MAIL_GOAL | FOREGROUND_CONNECTED_CALENDAR_GOAL
                )
            })
            .collect::<Vec<_>>();
        let runs = self.list_automation_runs()?;
        let mut repaired = 0_usize;
        for definition in definitions {
            let matching_runs = runs
                .iter()
                .filter(|run| run.definition_id == definition.id)
                .collect::<Vec<_>>();
            if matching_runs.is_empty() {
                if definition.status != AutomationDefinitionStatus::Deleted {
                    self.set_automation_definition_status(
                        definition.id,
                        AutomationDefinitionStatus::Deleted,
                        now,
                    )?;
                    repaired += 1;
                }
                continue;
            }
            for run in matching_runs {
                if run.review_queue_item_id.is_some() {
                    let was_enabled = definition.status != AutomationDefinitionStatus::Deleted;
                    self.finish_foreground_connected_work_preparation(definition.id, now)?;
                    repaired += usize::from(was_enabled);
                } else {
                    repaired += usize::from(
                        self.fail_foreground_connected_work_preparation(definition.id, now)?,
                    );
                }
            }
        }
        Ok(repaired)
    }

    pub fn connected_work_invocation_for_review(
        &self,
        review_id: Uuid,
        expected_review_action_revision: &str,
    ) -> EventStoreResult<ConnectorInvocation> {
        let review = self.review_queue_item(review_id)?;
        review
            .validate_action_revision(expected_review_action_revision)
            .map_err(invalid)?;
        if review.status != ReviewQueueItemStatus::PendingApproval {
            return Err(invalid(
                "connected work review is not waiting for exact approval",
            ));
        }
        let invocation_id =
            if let Some(binding) = self.connector_mail_draft_review_binding_by_review(review_id)? {
                binding.connector_invocation_id
            } else {
                self.connector_calendar_proposal_review_binding(review_id)?
                    .and_then(|binding| binding.connector_invocation_id)
                    .ok_or_else(|| invalid("connected work review has no exact invocation"))?
            };
        let invocation = self.connector_invocation(invocation_id)?;
        if invocation.tool_invocation_id != review.tool_invocation_id
            || review.preview_fingerprint.as_deref()
                != Some(invocation.request_fingerprint.as_str())
        {
            return Err(invalid("connected work exact review binding changed"));
        }
        Ok(invocation)
    }

    pub fn approve_and_start_connected_work_review(
        &self,
        review_id: Uuid,
        expected_review_action_revision: &str,
        note: String,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorInvocation> {
        let invocation =
            self.connected_work_invocation_for_review(review_id, expected_review_action_revision)?;
        if invocation.status == ConnectorInvocationStatus::Running {
            return Ok(invocation);
        }
        if invocation.status != ConnectorInvocationStatus::PendingApproval {
            return Err(invalid(
                "connected work invocation is no longer awaiting approval",
            ));
        }
        let tool_id = invocation
            .tool_invocation_id
            .ok_or_else(|| invalid("connected work invocation has no exact tool"))?;
        let tool = self
            .list_tool_invocations()?
            .into_iter()
            .find(|tool| tool.id == tool_id)
            .ok_or_else(|| invalid("connected work exact tool was not found"))?;
        let request_id = tool
            .approval_request_id
            .ok_or_else(|| invalid("connected work exact approval was not found"))?;
        let record = self.capability_access_record_by_id(request_id)?;
        if record.effective_status == crate::kernel::policy::CapabilityAccessStatus::PendingApproval
            && record.resolution.is_none()
        {
            let scope =
                record.request.exact_tool.as_ref().ok_or_else(|| {
                    invalid("connected work exact approval has no preview evidence")
                })?;
            return self.resolve_and_start_connector_invocation(
                invocation.id,
                note,
                record.projection_revision,
                scope.preview_revision,
                scope.preview_hash.clone(),
                now,
            );
        }
        self.start_approved_connector_invocation(invocation.id, now)
    }

    pub fn consume_connected_work_review(
        &self,
        review_id: Uuid,
        invocation_id: Uuid,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if let Some(binding) = self.connector_mail_draft_review_binding_by_review(review_id)? {
            self.consume_connector_mail_draft(binding.draft_id, invocation_id, now)?;
            return Ok(());
        }
        let binding = self
            .connector_calendar_proposal_review_binding(review_id)?
            .ok_or_else(|| invalid("connected work review was not found"))?;
        self.consume_connector_calendar_proposal(binding.proposal_id, invocation_id, now)?;
        Ok(())
    }

    pub(crate) fn reconcile_completed_connected_work_projections(
        &self,
        now: DateTime<Utc>,
    ) -> EventStoreResult<usize> {
        const PAGE_SIZE: usize = 64;
        let succeeded = serde_json::to_string(&ConnectorInvocationStatus::Succeeded)?;
        let mail_consumed = serde_json::to_string(&ConnectorMailDraftStatus::Consumed)?;
        let calendar_consumed = serde_json::to_string(&ConnectorCalendarProposalStatus::Consumed)?;
        let mut cursor = String::new();
        let mut repaired = 0_usize;

        loop {
            let candidates = {
                let mut statement = self.conn.prepare(
                    r#"SELECT candidate_key, invocation_id
                       FROM (
                         SELECT 'mail:' || draft.id AS candidate_key,
                                binding.connector_invocation_id AS invocation_id
                         FROM connector_mail_drafts AS draft
                         JOIN connector_mail_draft_reviews AS binding
                           ON binding.draft_id = draft.id
                         JOIN connector_invocations AS invocation
                           ON invocation.id = binding.connector_invocation_id
                         WHERE invocation.status = ?1 AND draft.status <> ?2
                         UNION ALL
                         SELECT 'calendar:' || proposal.id AS candidate_key,
                                binding.connector_invocation_id AS invocation_id
                         FROM connector_calendar_proposals AS proposal
                         JOIN connector_calendar_proposal_reviews AS binding
                           ON binding.proposal_id = proposal.id
                         JOIN connector_invocations AS invocation
                           ON invocation.id = binding.connector_invocation_id
                         WHERE invocation.status = ?1 AND proposal.status <> ?3
                       )
                       WHERE candidate_key > ?4
                       ORDER BY candidate_key ASC
                       LIMIT ?5"#,
                )?;
                let rows = statement
                    .query_map(
                        params![
                            succeeded,
                            mail_consumed,
                            calendar_consumed,
                            cursor,
                            i64::try_from(PAGE_SIZE).unwrap_or(i64::MAX),
                        ],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            if candidates.is_empty() {
                break;
            }
            cursor = candidates
                .last()
                .map(|(candidate_key, _)| candidate_key.clone())
                .unwrap_or_default();

            for (_, invocation_id) in candidates.iter() {
                let Ok(invocation_id) = Uuid::parse_str(invocation_id) else {
                    continue;
                };
                let transaction = rusqlite::Transaction::new_unchecked(
                    &self.conn,
                    TransactionBehavior::Immediate,
                )?;
                let invocation_json = transaction
                    .query_row(
                        "SELECT invocation_json FROM connector_invocations WHERE id = ?1",
                        params![invocation_id.to_string()],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                let Some(invocation_json) = invocation_json else {
                    transaction.commit()?;
                    continue;
                };
                let invocation = match serde_json::from_str(&invocation_json) {
                    Ok(invocation) => invocation,
                    Err(_) => continue,
                };
                match Self::consume_completed_connected_work_projection(
                    &transaction,
                    &invocation,
                    now,
                ) {
                    Ok(consumed) => {
                        transaction.commit()?;
                        repaired += usize::from(consumed);
                    }
                    Err(EventStoreError::Sqlite(error)) => {
                        return Err(EventStoreError::Sqlite(error));
                    }
                    Err(
                        EventStoreError::Timestamp(_)
                        | EventStoreError::Uuid(_)
                        | EventStoreError::Json(_)
                        | EventStoreError::NotFound(_)
                        | EventStoreError::InvalidState(_),
                    ) => {}
                }
            }
            if candidates.len() < PAGE_SIZE {
                break;
            }
        }
        Ok(repaired)
    }

    pub fn prepare_connector_mail_draft_review(
        &self,
        id: Uuid,
        expected_action_revision: &str,
        automation_run_id: Uuid,
        agent_run_id: Option<Uuid>,
        now: DateTime<Utc>,
    ) -> EventStoreResult<ConnectorMailDraftReviewPreparation> {
        if let Some(existing) = self.connector_mail_draft_review_binding(id)? {
            if existing.draft_action_revision != expected_action_revision
                || existing.automation_run_id != automation_run_id
                || existing.agent_run_id != agent_run_id
            {
                return Err(invalid(
                    "connector mail draft already has a different frozen review",
                ));
            }
            return self.load_connector_mail_draft_review_preparation(existing);
        }

        let draft = self.connector_mail_draft(id)?;
        if draft.status != ConnectorMailDraftStatus::Frozen
            || draft.action_revision() != expected_action_revision
        {
            return Err(invalid(
                "connector mail draft review is stale or not frozen",
            ));
        }
        let account = self
            .list_connector_accounts()?
            .into_iter()
            .find(|account| account.id == draft.account_id)
            .ok_or_else(|| invalid("connector mail draft account was not found"))?;
        if account.provider_id != draft.provider_id
            || account.health != ConnectorHealth::Connected
            || !account
                .granted_capabilities
                .contains(&ConnectorCapability::MailSendDraft)
            || self.connector_account_sync_generation(&account)? != draft.account_generation
        {
            return Err(invalid(
                "connector mail draft account changed before frozen review",
            ));
        }
        let mut automation_run = self.automation_run(automation_run_id)?;
        if automation_run.agent_run_id != agent_run_id
            || automation_run.review_queue_item_id.is_some()
        {
            return Err(invalid(
                "automation run cannot bind this connector mail review",
            ));
        }
        if let Some(agent_run_id) = agent_run_id {
            let running = self.list_agent_run_records()?.into_iter().any(|record| {
                record.id == agent_run_id && record.status == AgentRunStatus::Running
            });
            if !running {
                return Err(invalid(
                    "automation agent run must be running before it can wait for review",
                ));
            }
        }

        let intent = draft.mutation_intent().map_err(invalid)?;
        let intent_hash = intent.hash().map_err(invalid)?;
        let request = ToolExecutionRequest {
            tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            input: serde_json::json!({
                "provider_id": draft.provider_id,
                "account_id": draft.account_id.to_string(),
                "account_generation": draft.account_generation,
                "capability": ConnectorCapability::MailSendDraft.contract_name(),
                "target_ref": intent.target_ref(),
                "preview_hash": draft.content_hash(),
                "intent_hash": intent_hash,
                "idempotency_key": format!("connector-mail-draft:{}:{}", draft.id, draft.revision),
                "automation_run_id": automation_run_id.to_string(),
            }),
            access_mode: AccessMode::FullAccess,
            run_id: agent_run_id,
        };
        let plan = prepare_tool_execution(&request).map_err(invalid)?;
        if plan.policy_decision != PolicyDecision::Ask {
            return Err(invalid(
                "connector mail mutation must require exact approval",
            ));
        }
        let mut access_request =
            request_capability_access(AccessMode::FullAccess, CapabilityKind::ConnectorWrite)
                .map_err(invalid)?;
        access_request
            .bind_exact_tool(
                CONNECTOR_MUTATE_TOOL_ID,
                tool_request_fingerprint(&request),
                tool_approval_preview(&request),
            )
            .map_err(invalid)?;
        let tool_invocation =
            ToolInvocationRecord::waiting_for_confirmation(&plan, access_request.id);
        let mut review_item = ReviewQueueItem {
            id: Uuid::new_v4(),
            automation_run_id,
            agent_run_id,
            tool_invocation_id: None,
            status: ReviewQueueItemStatus::PendingReview,
            preview_fingerprint: Some(tool_invocation.request_fingerprint.clone()),
            revision: 0,
            title: "Review this exact email before sending".to_string(),
            evidence_ref: Some(format!("local-draft:{}", draft.id)),
            created_at: now,
            updated_at: now,
        };
        review_item
            .request_approval(
                tool_invocation.id,
                tool_invocation.request_fingerprint.clone(),
                now,
            )
            .map_err(invalid)?;
        let connector_invocation =
            ConnectorInvocation::from_tool_request(&request, &tool_invocation)
                .and_then(|invocation| invocation.bind_intent(intent))
                .map_err(invalid)?;
        if connector_invocation.status != ConnectorInvocationStatus::PendingApproval
            || tool_invocation.status != ToolExecutionStatus::WaitingForConfirmation
        {
            return Err(invalid("connector mail review preparation is invalid"));
        }
        automation_run.review_queue_item_id = Some(review_item.id);
        automation_run.updated_at = now;
        let agent_transition = agent_run_id
            .map(|agent_run_id| {
                AgentRunTransition::new(
                    agent_run_id,
                    AgentRunStatus::WaitingForConfirmation,
                    "Exact connected-account email is frozen and waiting for approval.".to_string(),
                    Some(tool_invocation.id),
                )
                .map_err(invalid)
            })
            .transpose()?;

        let access_event = KernelEvent::new(CAPABILITY_ACCESS_REQUESTED_EVENT, &access_request)?;
        let tool_event = KernelEvent::new(TOOL_INVOCATION_RECORDED_EVENT, &tool_invocation)?;
        let agent_event = agent_transition
            .as_ref()
            .map(|transition| KernelEvent::new(AGENT_RUN_TRANSITIONED_EVENT, transition))
            .transpose()?;
        let transaction =
            rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        Self::insert_kernel_event(&transaction, &access_event)?;
        Self::insert_kernel_event(&transaction, &tool_event)?;
        if let Some(agent_event) = &agent_event {
            Self::insert_kernel_event(&transaction, agent_event)?;
        }
        transaction.execute(
            r#"INSERT INTO review_queue_items
               (id, automation_run_id, item_json, status, revision, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
            params![
                review_item.id.to_string(),
                automation_run_id.to_string(),
                serde_json::to_string(&review_item)?,
                serde_json::to_string(&review_item.status)?,
                i64::from(review_item.revision),
                timestamp(review_item.updated_at),
            ],
        )?;
        transaction.execute(
            "UPDATE automation_runs SET run_json = ?2, updated_at = ?3 WHERE id = ?1",
            params![
                automation_run_id.to_string(),
                serde_json::to_string(&automation_run)?,
                timestamp(automation_run.updated_at),
            ],
        )?;
        let inserted = transaction.execute(
            r#"INSERT INTO connector_invocations
               (id, account_id, account_generation, idempotency_key,
                invocation_json, status, updated_at)
               SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7
               WHERE EXISTS (
                 SELECT 1 FROM connector_accounts AS account
                 JOIN connector_account_generations AS generation
                   ON generation.account_id = account.id
                 WHERE account.id = ?2 AND account.provider_id = ?8
                   AND account.health = ?9 AND generation.generation = ?3
               )"#,
            params![
                connector_invocation.id.to_string(),
                connector_invocation.account_id.to_string(),
                i64::try_from(draft.account_generation)
                    .map_err(|_| invalid("connector mail draft generation is too large"))?,
                connector_invocation.idempotency_key,
                serde_json::to_string(&connector_invocation)?,
                serde_json::to_string(&connector_invocation.status)?,
                timestamp(connector_invocation.updated_at),
                connector_invocation.provider_id,
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
        )?;
        if inserted != 1 {
            return Err(invalid(
                "connector mail draft account changed during review preparation",
            ));
        }
        transaction.execute(
            r#"INSERT INTO connector_mail_draft_reviews
               (draft_id, draft_action_revision, automation_run_id, agent_run_id,
                access_request_id, tool_invocation_id, review_item_id,
                connector_invocation_id, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
            params![
                draft.id.to_string(),
                expected_action_revision,
                automation_run_id.to_string(),
                agent_run_id.map(|value| value.to_string()),
                access_request.id.to_string(),
                tool_invocation.id.to_string(),
                review_item.id.to_string(),
                connector_invocation.id.to_string(),
                timestamp(now),
            ],
        )?;
        transaction.commit()?;

        Ok(ConnectorMailDraftReviewPreparation {
            draft,
            access_request,
            tool_invocation,
            review_item,
            connector_invocation,
        })
    }

    fn connector_mail_draft_review_binding(
        &self,
        draft_id: Uuid,
    ) -> EventStoreResult<Option<ConnectorMailDraftReviewBinding>> {
        self.conn
            .query_row(
                r#"SELECT draft_action_revision, automation_run_id, agent_run_id,
                          access_request_id, tool_invocation_id, review_item_id,
                          connector_invocation_id
                   FROM connector_mail_draft_reviews WHERE draft_id = ?1"#,
                params![draft_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?
            .map(|row| ConnectorMailDraftReviewBinding::parse(draft_id, row))
            .transpose()
    }

    fn connector_mail_draft_review_binding_by_review(
        &self,
        review_id: Uuid,
    ) -> EventStoreResult<Option<ConnectorMailDraftReviewBinding>> {
        self.conn
            .query_row(
                r#"SELECT draft_id, draft_action_revision, automation_run_id, agent_run_id,
                          access_request_id, tool_invocation_id, connector_invocation_id
                   FROM connector_mail_draft_reviews WHERE review_item_id = ?1"#,
                params![review_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?
            .map(|row| {
                Ok(ConnectorMailDraftReviewBinding {
                    draft_id: Uuid::parse_str(&row.0)?,
                    draft_action_revision: row.1,
                    automation_run_id: Uuid::parse_str(&row.2)?,
                    agent_run_id: row.3.map(|value| Uuid::parse_str(&value)).transpose()?,
                    access_request_id: Uuid::parse_str(&row.4)?,
                    tool_invocation_id: Uuid::parse_str(&row.5)?,
                    review_item_id: review_id,
                    connector_invocation_id: Uuid::parse_str(&row.6)?,
                })
            })
            .transpose()
    }

    fn connector_calendar_proposal_review_binding(
        &self,
        review_id: Uuid,
    ) -> EventStoreResult<Option<ConnectorCalendarProposalReviewBinding>> {
        self.conn
            .query_row(
                r#"SELECT proposal_id, automation_run_id, agent_run_id,
                          proposal_action_revision, access_request_id,
                          tool_invocation_id, connector_invocation_id
                   FROM connector_calendar_proposal_reviews WHERE review_item_id = ?1"#,
                params![review_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                },
            )
            .optional()?
            .map(|row| {
                Ok(ConnectorCalendarProposalReviewBinding {
                    proposal_id: Uuid::parse_str(&row.0)?,
                    automation_run_id: Uuid::parse_str(&row.1)?,
                    agent_run_id: Uuid::parse_str(&row.2)?,
                    review_item_id: review_id,
                    proposal_action_revision: row.3,
                    access_request_id: row.4.map(|value| Uuid::parse_str(&value)).transpose()?,
                    tool_invocation_id: row.5.map(|value| Uuid::parse_str(&value)).transpose()?,
                    connector_invocation_id: row
                        .6
                        .map(|value| Uuid::parse_str(&value))
                        .transpose()?,
                })
            })
            .transpose()
    }

    fn load_connector_mail_draft_review_preparation(
        &self,
        binding: ConnectorMailDraftReviewBinding,
    ) -> EventStoreResult<ConnectorMailDraftReviewPreparation> {
        let access_request = self
            .list_capability_access_requests()?
            .into_iter()
            .find(|request| request.id == binding.access_request_id)
            .ok_or_else(|| invalid("connector mail draft approval projection is missing"))?;
        let tool_invocation = self
            .list_tool_invocations()?
            .into_iter()
            .find(|tool| tool.id == binding.tool_invocation_id)
            .ok_or_else(|| invalid("connector mail draft tool projection is missing"))?;
        Ok(ConnectorMailDraftReviewPreparation {
            draft: self.connector_mail_draft(binding.draft_id)?,
            access_request,
            tool_invocation,
            review_item: self.review_queue_item(binding.review_item_id)?,
            connector_invocation: self.connector_invocation(binding.connector_invocation_id)?,
        })
    }

    fn load_connector_calendar_proposal_review_preparation(
        &self,
        binding: ConnectorCalendarProposalReviewBinding,
    ) -> EventStoreResult<ConnectorCalendarProposalReviewPreparation> {
        let access_request_id = binding
            .access_request_id
            .ok_or_else(|| invalid("connector calendar approval binding is incomplete"))?;
        let tool_invocation_id = binding
            .tool_invocation_id
            .ok_or_else(|| invalid("connector calendar tool binding is incomplete"))?;
        let connector_invocation_id = binding
            .connector_invocation_id
            .ok_or_else(|| invalid("connector calendar invocation binding is incomplete"))?;
        let access_request = self
            .list_capability_access_requests()?
            .into_iter()
            .find(|request| request.id == access_request_id)
            .ok_or_else(|| invalid("connector calendar approval projection is missing"))?;
        let tool_invocation = self
            .list_tool_invocations()?
            .into_iter()
            .find(|tool| tool.id == tool_invocation_id)
            .ok_or_else(|| invalid("connector calendar tool projection is missing"))?;
        Ok(ConnectorCalendarProposalReviewPreparation {
            proposal: self.connector_calendar_proposal(binding.proposal_id)?,
            access_request,
            tool_invocation,
            review_item: self.review_queue_item(binding.review_item_id)?,
            connector_invocation: self.connector_invocation(connector_invocation_id)?,
        })
    }

    fn connected_work_account_display_name(
        &self,
        account_id: Uuid,
        provider_id: &str,
        generation: u64,
    ) -> EventStoreResult<String> {
        let account = self
            .list_connector_accounts()?
            .into_iter()
            .find(|account| account.id == account_id)
            .ok_or_else(|| invalid("connected work account was not found"))?;
        if account.provider_id != provider_id
            || account.health != ConnectorHealth::Connected
            || self.connector_account_sync_generation(&account)? != generation
        {
            return Err(invalid("connected work account binding changed"));
        }
        Ok(account.display_name)
    }

    fn save_connector_calendar_proposal(
        &self,
        proposal: &ConnectorCalendarProposal,
        expected_status: ConnectorCalendarProposalStatus,
        expected_revision: u64,
    ) -> EventStoreResult<()> {
        proposal.validate().map_err(invalid)?;
        let changed = self.conn.execute(
            r#"UPDATE connector_calendar_proposals
               SET proposal_json = ?1, status = ?2, revision = ?3, updated_at = ?4
               WHERE id = ?5 AND status = ?6 AND revision = ?7
                 AND EXISTS (
                   SELECT 1 FROM connector_accounts AS account
                   JOIN connector_account_generations AS generation
                     ON generation.account_id = account.id
                   WHERE account.id = connector_calendar_proposals.account_id
                     AND account.provider_id = connector_calendar_proposals.provider_id
                     AND account.health = ?8
                     AND generation.generation = connector_calendar_proposals.account_generation
                 )"#,
            params![
                serde_json::to_string(proposal)?,
                calendar_status_text(proposal.status),
                i64::try_from(proposal.revision)
                    .map_err(|_| invalid("connector calendar revision is too large"))?,
                timestamp(proposal.updated_at),
                proposal.id.to_string(),
                calendar_status_text(expected_status),
                i64::try_from(expected_revision)
                    .map_err(|_| invalid("connector calendar revision is too large"))?,
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
        )?;
        if changed != 1 {
            return Err(invalid(
                "connector calendar proposal changed or its account binding expired",
            ));
        }
        Ok(())
    }

    fn save_connector_mail_draft(
        &self,
        draft: &ConnectorMailDraft,
        expected_status: ConnectorMailDraftStatus,
        expected_revision: u64,
    ) -> EventStoreResult<()> {
        draft.validate().map_err(invalid)?;
        let changed = self.conn.execute(
            r#"UPDATE connector_mail_drafts
               SET draft_json = ?1, status = ?2, revision = ?3,
                   consumed_by_invocation_id = ?4, updated_at = ?5
               WHERE id = ?6 AND status = ?7 AND revision = ?8
                 AND EXISTS (
                   SELECT 1 FROM connector_accounts AS account
                   JOIN connector_account_generations AS generation
                     ON generation.account_id = account.id
                   WHERE account.id = connector_mail_drafts.account_id
                     AND account.provider_id = connector_mail_drafts.provider_id
                     AND account.health = ?9
                     AND generation.generation = connector_mail_drafts.account_generation
                 )"#,
            params![
                serde_json::to_string(draft)?,
                status_text(draft.status),
                i64::try_from(draft.revision)
                    .map_err(|_| invalid("connector mail draft revision is too large"))?,
                draft
                    .consumed_by_invocation_id
                    .map(|value| value.to_string()),
                timestamp(draft.updated_at),
                draft.id.to_string(),
                status_text(expected_status),
                i64::try_from(expected_revision)
                    .map_err(|_| invalid("connector mail draft revision is too large"))?,
                serde_json::to_string(&ConnectorHealth::Connected)?,
            ],
        )?;
        if changed != 1 {
            return Err(invalid(
                "connector mail draft changed or its account binding expired",
            ));
        }
        Ok(())
    }
}

struct ConnectorMailDraftReviewBinding {
    draft_id: Uuid,
    draft_action_revision: String,
    automation_run_id: Uuid,
    agent_run_id: Option<Uuid>,
    access_request_id: Uuid,
    tool_invocation_id: Uuid,
    review_item_id: Uuid,
    connector_invocation_id: Uuid,
}

impl ConnectorMailDraftReviewBinding {
    fn parse(
        draft_id: Uuid,
        row: (
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            String,
        ),
    ) -> EventStoreResult<Self> {
        Ok(Self {
            draft_id,
            draft_action_revision: row.0,
            automation_run_id: Uuid::parse_str(&row.1)?,
            agent_run_id: row.2.map(|value| Uuid::parse_str(&value)).transpose()?,
            access_request_id: Uuid::parse_str(&row.3)?,
            tool_invocation_id: Uuid::parse_str(&row.4)?,
            review_item_id: Uuid::parse_str(&row.5)?,
            connector_invocation_id: Uuid::parse_str(&row.6)?,
        })
    }
}

struct ConnectorCalendarProposalReviewBinding {
    proposal_id: Uuid,
    automation_run_id: Uuid,
    agent_run_id: Uuid,
    review_item_id: Uuid,
    proposal_action_revision: Option<String>,
    access_request_id: Option<Uuid>,
    tool_invocation_id: Option<Uuid>,
    connector_invocation_id: Option<Uuid>,
}

impl ConnectorCalendarProposalReviewBinding {
    fn prepared(&self) -> bool {
        self.proposal_action_revision.is_some()
            && self.access_request_id.is_some()
            && self.tool_invocation_id.is_some()
            && self.connector_invocation_id.is_some()
    }
}

fn foreground_connected_work_ids(
    source_run_id: Uuid,
    kind: &str,
    payload: &[u8],
    attempt: u32,
) -> (Uuid, Uuid) {
    fn derived_uuid(
        source_run_id: Uuid,
        kind: &str,
        payload: &[u8],
        attempt: u32,
        label: &str,
    ) -> Uuid {
        let mut digest = Sha256::new();
        digest.update(b"ds-agent.foreground-connected-work.v1\0");
        digest.update(label.as_bytes());
        digest.update(b"\0");
        digest.update(source_run_id.as_bytes());
        digest.update(b"\0");
        digest.update(kind.as_bytes());
        digest.update(b"\0");
        digest.update(payload);
        if attempt > 0 {
            digest.update(b"\0attempt\0");
            digest.update(attempt.to_be_bytes());
        }
        let digest = digest.finalize();
        let mut bytes = [0_u8; 16];
        bytes.copy_from_slice(&digest[..16]);
        bytes[6] = (bytes[6] & 0x0f) | 0x80;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Uuid::from_bytes(bytes)
    }

    (
        derived_uuid(source_run_id, kind, payload, attempt, "definition"),
        derived_uuid(source_run_id, kind, payload, attempt, "manual-invocation"),
    )
}

fn status_text(status: ConnectorMailDraftStatus) -> String {
    serde_json::to_string(&status).expect("connector mail draft status is serializable")
}

fn calendar_status_text(status: ConnectorCalendarProposalStatus) -> String {
    serde_json::to_string(&status).expect("connector calendar proposal status is serializable")
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn invalid(message: impl Into<String>) -> EventStoreError {
    EventStoreError::InvalidState(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::automation::AutomationDefinition;
    use crate::kernel::connectors::domain::MailAddress;
    use crate::kernel::connectors::mutation::{CalendarMutationEvent, ConnectorMutationIntent};
    use crate::kernel::connectors::{
        ConnectorCredentialHandle, ConnectorMutationApplyOutcome, ConnectorMutationProvider,
        ConnectorMutationReconciler, ConnectorReconciliationOutcome, FakeConnectorProvider,
    };

    fn account(now: DateTime<Utc>) -> ConnectorAccount {
        ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "google".to_string(),
            display_name: "Offline Google fixture".to_string(),
            tenant_ref: None,
            credential_handle: ConnectorCredentialHandle::new(),
            granted_capabilities: vec![ConnectorCapability::MailSendDraft],
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        }
    }

    fn content(body: &str) -> ConnectorMailDraftContent {
        ConnectorMailDraftContent {
            to: vec![MailAddress {
                display_name: None,
                address: "recipient@example.com".to_string(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Never in events".to_string(),
            body_text: body.to_string(),
            in_reply_to: None,
            thread_ref: None,
        }
    }

    #[test]
    fn private_connector_draft_survives_restart_with_exact_cas_and_no_kernel_event() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("connector-draft.sqlite3");
        let now = Utc::now();
        let account = account(now);
        let (id, stale, exact);
        {
            let store = EventStore::open(&path).unwrap();
            store.upsert_connector_account(&account).unwrap();
            let draft = store
                .create_connector_mail_draft(&account, content("Private body"), now)
                .unwrap();
            id = draft.id;
            stale = draft.action_revision();
            let updated = store
                .update_connector_mail_draft(id, &stale, content("Updated private body"), now)
                .unwrap();
            exact = updated.action_revision();
            assert!(store.freeze_connector_mail_draft(id, &stale, now).is_err());
            assert_eq!(
                store
                    .conn
                    .query_row("SELECT COUNT(*) FROM kernel_events", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        let store = EventStore::open(&path).unwrap();
        let frozen = store.freeze_connector_mail_draft(id, &exact, now).unwrap();
        assert_eq!(frozen.status, ConnectorMailDraftStatus::Frozen);
        assert_eq!(frozen.content.body_text, "Updated private body".to_string());
        assert!(frozen.mutation_intent().is_ok());
    }

    #[test]
    fn frozen_local_draft_prepares_one_atomic_exact_automation_review_without_content_events() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("connector-draft-review.sqlite3");
        let now = Utc::now();
        let account = account(now);
        let (draft_id, action_revision, automation_run_id, agent_run_id, invocation_id);
        {
            let store = EventStore::open(&path).unwrap();
            store.upsert_connector_account(&account).unwrap();
            let definition = AutomationDefinition::once(
                "Prepare a reviewed email without sending it automatically.".to_string(),
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
                    "connector-draft-review".to_string(),
                )
                .unwrap();
            store
                .claim_agent_run(agent_run.id, "review-worker".to_string(), 60)
                .unwrap();
            automation_run_id = run.id;
            agent_run_id = agent_run.id;
            let editing = store
                .create_connector_mail_draft(&account, content("Private exact body"), now)
                .unwrap();
            draft_id = editing.id;
            let frozen = store
                .freeze_connector_mail_draft(editing.id, &editing.action_revision(), now)
                .unwrap();
            action_revision = frozen.action_revision();
            let prepared = store
                .prepare_connector_mail_draft_review(
                    frozen.id,
                    &action_revision,
                    run.id,
                    Some(agent_run.id),
                    now,
                )
                .unwrap();
            invocation_id = prepared.connector_invocation.id;
            assert!(prepared.access_request.exact_tool.is_some());
            assert_eq!(
                prepared.review_item.status,
                ReviewQueueItemStatus::PendingApproval
            );
            assert_eq!(
                prepared.connector_invocation.status,
                ConnectorInvocationStatus::PendingApproval
            );
            assert_eq!(
                prepared
                    .connector_invocation
                    .mutation_intent()
                    .unwrap()
                    .mail_content()
                    .unwrap()
                    .body_text,
                "Private exact body"
            );
            assert_eq!(store.reconcile_automation_agent_runs(now).unwrap(), 1);
            assert_eq!(
                store.automation_run(run.id).unwrap().status,
                AutomationRunStatus::WaitingApproval
            );
            let mut statement = store
                .conn
                .prepare("SELECT payload_json FROM kernel_events")
                .unwrap();
            let payloads = statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(payloads.iter().all(|payload| {
                !payload.contains("Private exact body") && !payload.contains("Never in events")
            }));
        }
        let store = EventStore::open(&path).unwrap();
        let replay = store
            .prepare_connector_mail_draft_review(
                draft_id,
                &action_revision,
                automation_run_id,
                Some(agent_run_id),
                now,
            )
            .unwrap();
        assert_eq!(replay.connector_invocation.id, invocation_id);
        assert_eq!(store.list_connector_invocations().unwrap().len(), 1);
        assert_eq!(store.list_review_queue_items().unwrap().len(), 1);
    }

    #[test]
    fn local_draft_automation_timeout_recovers_once_and_consumes_only_after_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("connector-draft-e2e.sqlite3");
        let now = Utc::now();
        let mut account = account(now);
        account.provider_id = "fake".to_string();
        let provider = FakeConnectorProvider::default();
        let remote = provider.remote_state();
        let (draft_id, invocation_id);
        {
            let store = EventStore::open(&path).unwrap();
            store.upsert_connector_account(&account).unwrap();
            let definition = AutomationDefinition::once(
                "Prepare an exact local reply and wait for approval before sending.".to_string(),
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
                    "connector-draft-e2e".to_string(),
                )
                .unwrap();
            store
                .claim_agent_run(agent_run.id, "e2e-worker".to_string(), 60)
                .unwrap();
            let editing = store
                .create_connector_mail_draft(&account, content("Approved private reply"), now)
                .unwrap();
            draft_id = editing.id;
            let frozen = store
                .freeze_connector_mail_draft(editing.id, &editing.action_revision(), now)
                .unwrap();
            let prepared = store
                .prepare_connector_mail_draft_review(
                    frozen.id,
                    &frozen.action_revision(),
                    run.id,
                    Some(agent_run.id),
                    now,
                )
                .unwrap();
            invocation_id = prepared.connector_invocation.id;
            let scope = prepared.access_request.exact_tool.as_ref().unwrap();
            store
                .resolve_connector_mutation_access_request(
                    prepared.access_request.id,
                    true,
                    "Approved exact local draft".to_string(),
                    0,
                    scope.preview_revision,
                    &scope.preview_hash,
                )
                .unwrap();
            let running = store
                .start_approved_connector_invocation(invocation_id, now)
                .unwrap();
            provider.timeout_after_next_apply();
            assert_eq!(
                provider.apply_mutation(&account, &running).unwrap(),
                ConnectorMutationApplyOutcome::ReconciliationRequired
            );
            store
                .mark_connector_invocation_reconciliation_required(invocation_id, now)
                .unwrap();
            assert!(store
                .consume_connector_mail_draft(draft_id, invocation_id, now)
                .is_err());
        }

        let provider = FakeConnectorProvider::with_remote_state(remote);
        let store = EventStore::open(&path).unwrap();
        let mut claims = store
            .claim_due_connector_reconciliations(Utc::now(), 1)
            .unwrap();
        let claim = claims.pop().unwrap();
        let ConnectorReconciliationOutcome::Applied(receipt) = provider
            .reconcile_mutation(&account, claim.invocation())
            .unwrap()
        else {
            panic!("fake side effect must be found read-only after restart");
        };
        store
            .complete_claimed_connector_reconciliation(&claim, receipt, Utc::now())
            .unwrap();
        let consumed = store
            .consume_connector_mail_draft(draft_id, invocation_id, Utc::now())
            .unwrap();
        assert_eq!(consumed.status, ConnectorMailDraftStatus::Consumed);
        assert_eq!(consumed.consumed_by_invocation_id, Some(invocation_id));
        assert_eq!(provider.applied_count(), 1);
        assert_eq!(
            store
                .consume_connector_mail_draft(draft_id, invocation_id, Utc::now())
                .unwrap()
                .revision,
            consumed.revision
        );
    }

    #[test]
    fn automation_calendar_proposal_is_private_durable_and_waits_for_review_without_effect() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("connector-calendar-proposal.sqlite3");
        let now = Utc::now();
        let mut account = account(now);
        account.granted_capabilities = vec![ConnectorCapability::CalendarCreateEvent];
        let proposal_id;
        {
            let store = EventStore::open(&path).unwrap();
            store.upsert_connector_account(&account).unwrap();
            let definition = AutomationDefinition::once(
                "Prepare a meeting proposal for local review.".to_string(),
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
                    "calendar-proposal".to_string(),
                )
                .unwrap();
            store
                .claim_agent_run(agent_run.id, "calendar-worker".to_string(), 60)
                .unwrap();
            let intent = ConnectorMutationIntent::CalendarCreateEvent {
                calendar_ref: "primary".to_string(),
                event: CalendarMutationEvent {
                    title: "Private meeting title".to_string(),
                    description: Some("Private meeting description".to_string()),
                    location: Some("Private room".to_string()),
                    starts_at: now + chrono::Duration::hours(1),
                    ends_at: now + chrono::Duration::hours(2),
                    timezone: "Asia/Shanghai".to_string(),
                    attendees: Vec::new(),
                    notify_attendees: false,
                },
            };
            let (proposal, review) = store
                .create_connector_calendar_proposal(&account, intent, run.id, agent_run.id, now)
                .unwrap();
            proposal_id = proposal.id;
            assert_eq!(
                proposal.status,
                ConnectorCalendarProposalStatus::PendingReview
            );
            assert_eq!(review.status, ReviewQueueItemStatus::PendingReview);
            assert_eq!(store.reconcile_automation_agent_runs(now).unwrap(), 1);
            assert_eq!(
                store.automation_run(run.id).unwrap().status,
                AutomationRunStatus::WaitingReview
            );
            assert_eq!(store.list_connector_invocations().unwrap().len(), 0);
            let events = store.list_recent(100).unwrap();
            assert!(events.iter().all(|event| {
                !event.payload_json.contains("Private meeting title")
                    && !event.payload_json.contains("Private meeting description")
                    && !event.payload_json.contains("Private room")
            }));
        }
        let store = EventStore::open(&path).unwrap();
        let proposal = store.connector_calendar_proposal(proposal_id).unwrap();
        assert_eq!(
            proposal.intent.capability(),
            ConnectorCapability::CalendarCreateEvent
        );
        assert!(!format!("{proposal:?}").contains("Private meeting title"));
        assert!(!serde_json::to_string(&proposal.public_view())
            .unwrap()
            .contains("Private meeting title"));
    }

    #[test]
    fn calendar_review_freezes_exact_approval_and_consumes_only_after_provider_evidence() {
        let now = Utc::now();
        let mut account = account(now);
        account.provider_id = "fake".to_string();
        account.granted_capabilities = vec![ConnectorCapability::CalendarCreateEvent];
        let store = EventStore::open_memory().unwrap();
        store.upsert_connector_account(&account).unwrap();
        let definition = AutomationDefinition::once(
            "Prepare a reviewed calendar change.".to_string(),
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
                "calendar-review-e2e".to_string(),
            )
            .unwrap();
        store
            .claim_agent_run(agent_run.id, "calendar-review-worker".to_string(), 60)
            .unwrap();
        let intent = ConnectorMutationIntent::CalendarCreateEvent {
            calendar_ref: "primary".to_string(),
            event: CalendarMutationEvent {
                title: "Exact private calendar title".to_string(),
                description: Some("Exact private calendar description".to_string()),
                location: Some("Exact private room".to_string()),
                starts_at: now + chrono::Duration::hours(1),
                ends_at: now + chrono::Duration::hours(2),
                timezone: "Asia/Shanghai".to_string(),
                attendees: Vec::new(),
                notify_attendees: false,
            },
        };
        let (proposal, review) = store
            .create_connector_calendar_proposal(&account, intent, run.id, agent_run.id, now)
            .unwrap();
        let views = store.list_connected_work_reviews().unwrap();
        assert_eq!(views.len(), 1);
        let rendered = serde_json::to_string(&views).unwrap();
        assert!(rendered.contains("Exact private calendar title"));
        assert!(store
            .list_recent(100)
            .unwrap()
            .iter()
            .all(|event| !event.payload_json.contains("Exact private calendar title")));

        let prepared = store
            .prepare_connector_calendar_proposal_approval(review.id, &review.action_revision(), now)
            .unwrap();
        assert_eq!(
            prepared.proposal.status,
            ConnectorCalendarProposalStatus::Frozen
        );
        assert_eq!(
            prepared.review_item.status,
            ReviewQueueItemStatus::PendingApproval
        );
        assert!(prepared.access_request.exact_tool.is_some());
        assert_eq!(
            prepared.connector_invocation.mutation_intent().unwrap(),
            &proposal.intent
        );
        let replay = store
            .prepare_connector_calendar_proposal_approval(
                review.id,
                &prepared.review_item.action_revision(),
                now,
            )
            .unwrap();
        assert_eq!(
            replay.connector_invocation.id,
            prepared.connector_invocation.id
        );

        let provider = FakeConnectorProvider::default();
        let running = store
            .approve_and_start_connected_work_review(
                review.id,
                &prepared.review_item.action_revision(),
                "Approved exact fake calendar change".to_string(),
                now,
            )
            .unwrap();
        let ConnectorMutationApplyOutcome::Applied(receipt) =
            provider.apply_mutation(&account, &running).unwrap()
        else {
            panic!("fake provider should apply the exact calendar change");
        };
        let frozen = store.connector_calendar_proposal(proposal.id).unwrap();
        store
            .conn
            .execute_batch(
                r#"
                CREATE TRIGGER phase6_fail_connected_work_completion
                BEFORE UPDATE OF status ON connector_calendar_proposals
                WHEN NEW.status = '"consumed"'
                BEGIN
                  SELECT RAISE(ABORT, 'phase6 connected-work completion fault');
                END;
                "#,
            )
            .unwrap();
        assert!(store
            .complete_connector_invocation(running.id, receipt.clone(), now)
            .is_err());
        assert_eq!(
            store.connector_invocation(running.id).unwrap().status,
            ConnectorInvocationStatus::Running
        );
        assert_eq!(
            store
                .connector_calendar_proposal(proposal.id)
                .unwrap()
                .status,
            ConnectorCalendarProposalStatus::Frozen
        );
        assert_eq!(
            store.review_queue_item(review.id).unwrap().status,
            ReviewQueueItemStatus::PendingApproval
        );
        assert_eq!(
            store
                .tool_invocation_by_id(running.tool_invocation_id.unwrap())
                .unwrap()
                .status,
            ToolExecutionStatus::Running
        );
        store
            .conn
            .execute_batch("DROP TRIGGER phase6_fail_connected_work_completion")
            .unwrap();
        let completed = store
            .complete_connector_invocation(running.id, receipt, now)
            .unwrap();
        let consumed = store.connector_calendar_proposal(proposal.id).unwrap();
        assert_eq!(consumed.status, ConnectorCalendarProposalStatus::Consumed);
        assert_eq!(consumed.consumed_by_invocation_id, Some(completed.id));
        store
            .consume_connected_work_review(review.id, completed.id, now)
            .unwrap();
        let consumed = store.connector_calendar_proposal(proposal.id).unwrap();
        assert_eq!(consumed.status, ConnectorCalendarProposalStatus::Consumed);
        assert_eq!(consumed.consumed_by_invocation_id, Some(completed.id));
        assert_eq!(provider.applied_count(), 1);
        assert!(store.list_connected_work_reviews().unwrap().is_empty());

        store
            .conn
            .execute(
                r#"UPDATE connector_calendar_proposals
                   SET proposal_json = ?2, status = ?3, revision = ?4, updated_at = ?5
                   WHERE id = ?1"#,
                params![
                    proposal.id.to_string(),
                    serde_json::to_string(&frozen).unwrap(),
                    serde_json::to_string(&frozen.status).unwrap(),
                    frozen.revision,
                    timestamp(now),
                ],
            )
            .unwrap();
        let malformed_invocation_id = Uuid::nil();
        let malformed_proposal_id = Uuid::from_u128(1);
        store
            .conn
            .execute(
                r#"INSERT INTO connector_invocations
                   (id, account_id, account_generation, idempotency_key, invocation_json,
                    status, updated_at)
                   VALUES (?1, ?2, ?3, ?4, '{malformed', ?5, ?6)"#,
                params![
                    malformed_invocation_id.to_string(),
                    account.id.to_string(),
                    i64::try_from(frozen.account_generation).unwrap(),
                    "phase6-malformed-startup-repair",
                    serde_json::to_string(&ConnectorInvocationStatus::Succeeded).unwrap(),
                    timestamp(now),
                ],
            )
            .unwrap();
        store
            .conn
            .execute(
                r#"INSERT INTO connector_calendar_proposals
                   (id, provider_id, account_id, account_generation, proposal_json, status,
                    revision, created_at, updated_at)
                   VALUES (?1, ?2, ?3, ?4, '{malformed', ?5, 0, ?6, ?6)"#,
                params![
                    malformed_proposal_id.to_string(),
                    account.provider_id,
                    account.id.to_string(),
                    i64::try_from(frozen.account_generation).unwrap(),
                    serde_json::to_string(&ConnectorCalendarProposalStatus::Frozen).unwrap(),
                    timestamp(now),
                ],
            )
            .unwrap();
        store
            .conn
            .execute(
                r#"INSERT INTO connector_calendar_proposal_reviews
                   (proposal_id, automation_run_id, agent_run_id, review_item_id,
                    connector_invocation_id, created_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
                params![
                    malformed_proposal_id.to_string(),
                    Uuid::from_u128(2).to_string(),
                    Uuid::from_u128(3).to_string(),
                    Uuid::from_u128(4).to_string(),
                    malformed_invocation_id.to_string(),
                    timestamp(now),
                ],
            )
            .unwrap();

        assert_eq!(
            store
                .reconcile_completed_connected_work_projections(now)
                .unwrap(),
            1
        );
        let repaired = store.connector_calendar_proposal(proposal.id).unwrap();
        assert_eq!(repaired.status, ConnectorCalendarProposalStatus::Consumed);
        assert_eq!(repaired.consumed_by_invocation_id, Some(completed.id));
        assert_eq!(
            store
                .reconcile_completed_connected_work_projections(now)
                .unwrap(),
            0
        );
        assert_eq!(provider.applied_count(), 1);
    }

    #[test]
    fn connected_work_start_fault_rolls_back_resolution_and_one_shot_consumption() {
        let now = Utc::now();
        let account = account(now);
        let store = EventStore::open_memory().unwrap();
        store.upsert_connector_account(&account).unwrap();
        let view = store
            .prepare_foreground_connected_mail_review(
                Uuid::new_v4(),
                &account,
                content("Atomic approval body"),
                now,
            )
            .unwrap();
        let ConnectedWorkReviewView::Mail { review, .. } = view else {
            panic!("foreground mail must create a mail review");
        };
        let pending = store
            .connected_work_invocation_for_review(review.id, &review.action_revision)
            .unwrap();
        let request_id = store
            .tool_invocation_by_id(pending.tool_invocation_id.unwrap())
            .unwrap()
            .approval_request_id
            .unwrap();

        store
            .conn
            .execute_batch(
                r#"
                CREATE TRIGGER phase6_fail_connected_work_start
                BEFORE UPDATE OF status ON connector_invocations
                WHEN NEW.status = '"running"'
                BEGIN
                  SELECT RAISE(ABORT, 'phase6 connected-work start fault');
                END;
                "#,
            )
            .unwrap();
        assert!(store
            .approve_and_start_connected_work_review(
                review.id,
                &review.action_revision,
                "Atomic exact approval".to_string(),
                now,
            )
            .is_err());

        let approval = store.capability_access_record_by_id(request_id).unwrap();
        assert_eq!(
            approval.effective_status,
            crate::kernel::policy::CapabilityAccessStatus::PendingApproval
        );
        assert!(approval.resolution.is_none());
        assert_eq!(
            store.connector_invocation(pending.id).unwrap().status,
            ConnectorInvocationStatus::PendingApproval
        );
        assert_eq!(
            store
                .tool_invocation_by_id(pending.tool_invocation_id.unwrap())
                .unwrap()
                .status,
            ToolExecutionStatus::WaitingForConfirmation
        );
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM connector_approval_consumptions WHERE request_id = ?1",
                    params![request_id.to_string()],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );

        store
            .conn
            .execute_batch("DROP TRIGGER phase6_fail_connected_work_start")
            .unwrap();
        let running = store
            .approve_and_start_connected_work_review(
                review.id,
                &review.action_revision,
                "Atomic exact approval".to_string(),
                now,
            )
            .unwrap();
        assert_eq!(running.status, ConnectorInvocationStatus::Running);
        let approval = store.capability_access_record_by_id(request_id).unwrap();
        assert_eq!(
            approval.effective_status,
            crate::kernel::policy::CapabilityAccessStatus::Approved
        );
        assert!(approval.resolution.is_some());
    }

    #[test]
    fn foreground_connected_work_fault_is_terminalized_and_retried_without_external_effect() {
        let now = Utc::now();
        let account = account(now);
        let source_run_id = Uuid::new_v4();
        let private_content = content("Private retry body");
        let store = EventStore::open_memory().unwrap();
        store.upsert_connector_account(&account).unwrap();
        store
            .conn
            .execute_batch(
                r#"
                CREATE TRIGGER phase6_fail_foreground_draft_insert
                BEFORE INSERT ON connector_mail_drafts
                BEGIN
                  SELECT RAISE(ABORT, 'phase6 foreground preparation fault');
                END;
                "#,
            )
            .unwrap();

        assert!(store
            .prepare_foreground_connected_mail_review(
                source_run_id,
                &account,
                private_content.clone(),
                now,
            )
            .is_err());
        store
            .conn
            .execute_batch("DROP TRIGGER phase6_fail_foreground_draft_insert")
            .unwrap();
        let first_definitions = store
            .list_automation_definitions()
            .unwrap()
            .into_iter()
            .filter(|definition| definition.goal == FOREGROUND_CONNECTED_MAIL_GOAL)
            .collect::<Vec<_>>();
        assert_eq!(first_definitions.len(), 1);
        assert_eq!(
            first_definitions[0].status,
            AutomationDefinitionStatus::Deleted
        );
        let first_runs = store.list_automation_runs().unwrap();
        assert_eq!(first_runs.len(), 1);
        assert_eq!(first_runs[0].status, AutomationRunStatus::Failed);
        assert!(store.list_connector_invocations().unwrap().is_empty());

        let review = store
            .prepare_foreground_connected_mail_review(
                source_run_id,
                &account,
                private_content.clone(),
                now,
            )
            .unwrap();
        let ConnectedWorkReviewView::Mail { review, .. } = review else {
            panic!("retry must create the private mail review");
        };
        let runs = store.list_automation_runs().unwrap();
        assert_eq!(runs.len(), 2);
        assert!(runs
            .iter()
            .any(|run| run.status == AutomationRunStatus::Failed));
        let successful_run = runs
            .iter()
            .find(|run| run.review_queue_item_id == Some(review.id))
            .unwrap();
        assert_eq!(successful_run.status, AutomationRunStatus::WaitingApproval);
        assert_eq!(store.list_connector_invocations().unwrap().len(), 1);

        store
            .set_automation_definition_status(
                successful_run.definition_id,
                AutomationDefinitionStatus::Enabled,
                now,
            )
            .unwrap();
        let replay = store
            .prepare_foreground_connected_mail_review(source_run_id, &account, private_content, now)
            .unwrap();
        let ConnectedWorkReviewView::Mail {
            review: replay_review,
            ..
        } = replay
        else {
            panic!("retry must replay the same private mail review");
        };
        assert_eq!(replay_review.id, review.id);
        assert_eq!(
            store
                .automation_definition(successful_run.definition_id)
                .unwrap()
                .status,
            AutomationDefinitionStatus::Deleted
        );
    }

    #[test]
    fn startup_repair_terminalizes_interrupted_foreground_work_before_retry() {
        let now = Utc::now();
        let account = account(now);
        let source_run_id = Uuid::new_v4();
        let private_content = content("Private restart body");
        let payload = serde_json::to_vec(&private_content).unwrap();
        let (definition_id, manual_invocation_id) =
            foreground_connected_work_ids(source_run_id, "mail", &payload, 0);
        let store = EventStore::open_memory().unwrap();
        store.upsert_connector_account(&account).unwrap();
        let (interrupted_run, _) = store
            .start_foreground_connected_work_run(
                definition_id,
                manual_invocation_id,
                FOREGROUND_CONNECTED_MAIL_GOAL,
                now,
            )
            .unwrap();
        assert!(store
            .prepare_foreground_connected_mail_review(
                source_run_id,
                &account,
                private_content.clone(),
                now,
            )
            .is_err());

        assert_eq!(
            store
                .reconcile_interrupted_foreground_connected_work_preparations(now)
                .unwrap(),
            1
        );
        assert_eq!(
            store.automation_run(interrupted_run.id).unwrap().status,
            AutomationRunStatus::Failed
        );
        assert_eq!(
            store.automation_definition(definition_id).unwrap().status,
            AutomationDefinitionStatus::Deleted
        );
        let review = store
            .prepare_foreground_connected_mail_review(source_run_id, &account, private_content, now)
            .unwrap();
        assert!(matches!(review, ConnectedWorkReviewView::Mail { .. }));
        assert_eq!(store.list_automation_runs().unwrap().len(), 2);
        assert_eq!(
            store
                .reconcile_interrupted_foreground_connected_work_preparations(now)
                .unwrap(),
            0
        );
    }

    #[test]
    fn private_connected_work_marker_stays_out_of_events_lifecycle_and_model_context() {
        let now = Utc::now();
        let account = account(now);
        let marker = format!("DS_AGENT_PRIVATE_{}", Uuid::new_v4().simple());
        let private_path = format!("C:/Users/private/{marker}/connector-vault");
        let private_content = content(&format!("{marker} {private_path}"));
        let store = EventStore::open_memory().unwrap();
        store.upsert_connector_account(&account).unwrap();

        let local_review = store
            .prepare_foreground_connected_mail_review(
                Uuid::new_v4(),
                &account,
                private_content,
                now,
            )
            .unwrap();
        let local_review_json = serde_json::to_string(&local_review).unwrap();
        assert!(local_review_json.contains(&marker));
        assert!(local_review_json.contains(&private_path));

        let public_outputs = [
            serde_json::to_string(&store.list_recent(500).unwrap()).unwrap(),
            serde_json::to_string(&store.list_tool_invocations().unwrap()).unwrap(),
            serde_json::to_string(&store.list_agent_run_records().unwrap()).unwrap(),
            serde_json::to_string(&store.task_lifecycle_snapshot().unwrap()).unwrap(),
            crate::kernel::connectors::connector_context_summary(std::slice::from_ref(&account)),
        ];
        for (index, output) in public_outputs.into_iter().enumerate() {
            assert!(
                !output.contains(&marker),
                "public output {index} leaked marker"
            );
            assert!(
                !output.contains(&private_path),
                "public output {index} leaked private path"
            );
        }
        let invocation_debug = format!("{:?}", store.list_connector_invocations().unwrap());
        assert!(!invocation_debug.contains(&marker));
        assert!(!invocation_debug.contains(&private_path));
    }
}
