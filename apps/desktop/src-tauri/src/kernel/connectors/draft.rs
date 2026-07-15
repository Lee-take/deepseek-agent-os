use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::mutation::{ConnectorMailDraftContent, ConnectorMutationIntent};
use super::ConnectorCapability;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorMailDraftStatus {
    Editing,
    Frozen,
    Consumed,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorMailDraft {
    pub id: Uuid,
    pub provider_id: String,
    pub account_id: Uuid,
    pub account_generation: u64,
    pub content: ConnectorMailDraftContent,
    pub internet_message_id: String,
    pub status: ConnectorMailDraftStatus,
    pub revision: u64,
    pub consumed_by_invocation_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl std::fmt::Debug for ConnectorMailDraft {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectorMailDraft")
            .field("id", &self.id)
            .field("provider_id", &self.provider_id)
            .field("account_id", &self.account_id)
            .field("account_generation", &self.account_generation)
            .field("status", &self.status)
            .field("revision", &self.revision)
            .field("content", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConnectorMailDraftView {
    pub id: Uuid,
    pub provider_id: String,
    pub account_id: Uuid,
    pub account_generation: u64,
    pub status: ConnectorMailDraftStatus,
    pub revision: u64,
    pub action_revision: String,
    pub recipient_count: usize,
    pub body_chars: usize,
    pub has_reply_reference: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorCalendarProposalStatus {
    PendingReview,
    Frozen,
    Consumed,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorCalendarProposal {
    pub id: Uuid,
    pub provider_id: String,
    pub account_id: Uuid,
    pub account_generation: u64,
    pub intent: ConnectorMutationIntent,
    pub status: ConnectorCalendarProposalStatus,
    pub revision: u64,
    pub consumed_by_invocation_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl std::fmt::Debug for ConnectorCalendarProposal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectorCalendarProposal")
            .field("id", &self.id)
            .field("provider_id", &self.provider_id)
            .field("account_id", &self.account_id)
            .field("account_generation", &self.account_generation)
            .field("capability", &self.intent.capability())
            .field("status", &self.status)
            .field("revision", &self.revision)
            .field("content", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConnectorCalendarProposalView {
    pub id: Uuid,
    pub provider_id: String,
    pub account_id: Uuid,
    pub account_generation: u64,
    pub capability: ConnectorCapability,
    pub status: ConnectorCalendarProposalStatus,
    pub revision: u64,
    pub action_revision: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConnectorMailDraft {
    pub fn new(
        provider_id: String,
        account_id: Uuid,
        account_generation: u64,
        content: ConnectorMailDraftContent,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        let provider_id = provider_id.trim().to_string();
        if provider_id.is_empty() || provider_id.len() > 128 || provider_id.contains(['\r', '\n']) {
            return Err("connector mail draft provider is invalid".to_string());
        }
        content.validate()?;
        let id = Uuid::new_v4();
        Ok(Self {
            id,
            provider_id,
            account_id,
            account_generation,
            content,
            internet_message_id: format!("<{id}@ds-agent.local>"),
            status: ConnectorMailDraftStatus::Editing,
            revision: 0,
            consumed_by_invocation_id: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.provider_id.trim().is_empty()
            || self.provider_id.len() > 128
            || self.provider_id.contains(['\r', '\n'])
            || self.internet_message_id != format!("<{}@ds-agent.local>", self.id)
        {
            return Err("connector mail draft identity is invalid".to_string());
        }
        self.content.validate()?;
        match self.status {
            ConnectorMailDraftStatus::Editing | ConnectorMailDraftStatus::Frozen
                if self.consumed_by_invocation_id.is_none() => {}
            ConnectorMailDraftStatus::Consumed if self.consumed_by_invocation_id.is_some() => {}
            _ => return Err("connector mail draft lifecycle is invalid".to_string()),
        }
        Ok(())
    }

    pub fn content_hash(&self) -> String {
        let encoded = serde_json::to_vec(&self.content)
            .expect("validated connector draft content is serializable");
        format!("sha256:{}", hex::encode(Sha256::digest(encoded)))
    }

    pub fn action_revision(&self) -> String {
        let canonical = serde_json::to_vec(&(
            "ds-agent.connector-mail-draft-action.v1",
            self.id,
            &self.provider_id,
            self.account_id,
            self.account_generation,
            self.status,
            self.revision,
            self.content_hash(),
        ))
        .expect("connector draft action fields are serializable");
        format!("draft1:{}", hex::encode(Sha256::digest(canonical)))
    }

    pub fn public_view(&self) -> ConnectorMailDraftView {
        ConnectorMailDraftView {
            id: self.id,
            provider_id: self.provider_id.clone(),
            account_id: self.account_id,
            account_generation: self.account_generation,
            status: self.status,
            revision: self.revision,
            action_revision: self.action_revision(),
            recipient_count: self.content.to.len() + self.content.cc.len() + self.content.bcc.len(),
            body_chars: self.content.body_text.chars().count(),
            has_reply_reference: self.content.in_reply_to.is_some(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub fn update(
        &mut self,
        expected_action_revision: &str,
        content: ConnectorMailDraftContent,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.require_action(expected_action_revision)?;
        if self.status != ConnectorMailDraftStatus::Editing {
            return Err("only an editing connector mail draft can change".to_string());
        }
        content.validate()?;
        self.content = content;
        self.advance(now)?;
        Ok(())
    }

    pub fn freeze(
        &mut self,
        expected_action_revision: &str,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.require_action(expected_action_revision)?;
        if self.status != ConnectorMailDraftStatus::Editing {
            return Err("connector mail draft is not editable".to_string());
        }
        self.status = ConnectorMailDraftStatus::Frozen;
        self.advance(now)
    }

    pub fn mutation_intent(&self) -> Result<ConnectorMutationIntent, String> {
        self.validate()?;
        if self.status != ConnectorMailDraftStatus::Frozen {
            return Err("connector mail draft must be frozen before review".to_string());
        }
        Ok(ConnectorMutationIntent::MailSendDraft {
            draft_ref: format!("local-draft:{}", self.id),
            internet_message_id: self.internet_message_id.clone(),
            content: Some(self.content.clone()),
        })
    }

    pub fn consume(&mut self, invocation_id: Uuid, now: DateTime<Utc>) -> Result<(), String> {
        if self.status == ConnectorMailDraftStatus::Consumed
            && self.consumed_by_invocation_id == Some(invocation_id)
        {
            return Ok(());
        }
        if self.status != ConnectorMailDraftStatus::Frozen
            || self.consumed_by_invocation_id.is_some()
        {
            return Err("connector mail draft cannot be consumed".to_string());
        }
        self.status = ConnectorMailDraftStatus::Consumed;
        self.consumed_by_invocation_id = Some(invocation_id);
        self.advance(now)
    }

    fn require_action(&self, value: &str) -> Result<(), String> {
        if value != self.action_revision() {
            return Err("connector mail draft action is stale".to_string());
        }
        Ok(())
    }

    fn advance(&mut self, now: DateTime<Utc>) -> Result<(), String> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "connector mail draft revision is exhausted".to_string())?;
        self.updated_at = now;
        Ok(())
    }
}

impl ConnectorCalendarProposal {
    pub fn new(
        provider_id: String,
        account_id: Uuid,
        account_generation: u64,
        intent: ConnectorMutationIntent,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        let provider_id = provider_id.trim().to_string();
        if provider_id.is_empty()
            || provider_id.len() > 128
            || provider_id.contains(['\r', '\n'])
            || !matches!(
                intent.capability(),
                ConnectorCapability::CalendarCreateEvent
                    | ConnectorCapability::CalendarUpdateEvent
                    | ConnectorCapability::CalendarCancelEvent
            )
        {
            return Err("connector calendar proposal identity is invalid".to_string());
        }
        intent.validate()?;
        Ok(Self {
            id: Uuid::new_v4(),
            provider_id,
            account_id,
            account_generation,
            intent,
            status: ConnectorCalendarProposalStatus::PendingReview,
            revision: 0,
            consumed_by_invocation_id: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.provider_id.trim().is_empty()
            || self.provider_id.len() > 128
            || self.provider_id.contains(['\r', '\n'])
            || !matches!(
                self.intent.capability(),
                ConnectorCapability::CalendarCreateEvent
                    | ConnectorCapability::CalendarUpdateEvent
                    | ConnectorCapability::CalendarCancelEvent
            )
        {
            return Err("connector calendar proposal identity is invalid".to_string());
        }
        self.intent.validate()?;
        match self.status {
            ConnectorCalendarProposalStatus::PendingReview
            | ConnectorCalendarProposalStatus::Frozen
                if self.consumed_by_invocation_id.is_none() => {}
            ConnectorCalendarProposalStatus::Consumed
                if self.consumed_by_invocation_id.is_some() => {}
            _ => return Err("connector calendar proposal lifecycle is invalid".to_string()),
        }
        Ok(())
    }

    pub fn action_revision(&self) -> String {
        let canonical = serde_json::to_vec(&(
            "ds-agent.connector-calendar-proposal-action.v1",
            self.id,
            &self.provider_id,
            self.account_id,
            self.account_generation,
            self.status,
            self.revision,
            self.intent.hash().expect("validated proposal hashes"),
        ))
        .expect("connector calendar proposal action fields are serializable");
        format!("calendar1:{}", hex::encode(Sha256::digest(canonical)))
    }

    pub fn public_view(&self) -> ConnectorCalendarProposalView {
        ConnectorCalendarProposalView {
            id: self.id,
            provider_id: self.provider_id.clone(),
            account_id: self.account_id,
            account_generation: self.account_generation,
            capability: self.intent.capability(),
            status: self.status,
            revision: self.revision,
            action_revision: self.action_revision(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub fn update(
        &mut self,
        expected_action_revision: &str,
        intent: ConnectorMutationIntent,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.require_action(expected_action_revision)?;
        if self.status != ConnectorCalendarProposalStatus::PendingReview
            || !matches!(
                intent.capability(),
                ConnectorCapability::CalendarCreateEvent
                    | ConnectorCapability::CalendarUpdateEvent
                    | ConnectorCapability::CalendarCancelEvent
            )
        {
            return Err("connector calendar proposal cannot change".to_string());
        }
        intent.validate()?;
        self.intent = intent;
        self.advance(now)
    }

    pub fn freeze(
        &mut self,
        expected_action_revision: &str,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.require_action(expected_action_revision)?;
        if self.status != ConnectorCalendarProposalStatus::PendingReview {
            return Err("connector calendar proposal is not pending review".to_string());
        }
        self.status = ConnectorCalendarProposalStatus::Frozen;
        self.advance(now)
    }

    pub fn mutation_intent(&self) -> Result<ConnectorMutationIntent, String> {
        self.validate()?;
        if self.status != ConnectorCalendarProposalStatus::Frozen {
            return Err("connector calendar proposal must be frozen first".to_string());
        }
        Ok(self.intent.clone())
    }

    pub fn consume(&mut self, invocation_id: Uuid, now: DateTime<Utc>) -> Result<(), String> {
        if self.status == ConnectorCalendarProposalStatus::Consumed
            && self.consumed_by_invocation_id == Some(invocation_id)
        {
            return Ok(());
        }
        if self.status != ConnectorCalendarProposalStatus::Frozen
            || self.consumed_by_invocation_id.is_some()
        {
            return Err("connector calendar proposal cannot be consumed".to_string());
        }
        self.status = ConnectorCalendarProposalStatus::Consumed;
        self.consumed_by_invocation_id = Some(invocation_id);
        self.advance(now)
    }

    fn require_action(&self, value: &str) -> Result<(), String> {
        if value != self.action_revision() {
            return Err("connector calendar proposal action is stale".to_string());
        }
        Ok(())
    }

    fn advance(&mut self, now: DateTime<Utc>) -> Result<(), String> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "connector calendar proposal revision is exhausted".to_string())?;
        self.updated_at = now;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::connectors::domain::MailAddress;

    fn content(body: &str) -> ConnectorMailDraftContent {
        ConnectorMailDraftContent {
            to: vec![MailAddress {
                display_name: Some("Recipient".to_string()),
                address: "recipient@example.com".to_string(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "Private subject".to_string(),
            body_text: body.to_string(),
            in_reply_to: None,
            thread_ref: None,
        }
    }

    #[test]
    fn exact_revision_freezes_private_content_and_prevents_stale_edits() {
        let now = Utc::now();
        let mut draft = ConnectorMailDraft::new(
            "google".to_string(),
            Uuid::new_v4(),
            3,
            content("Private body"),
            now,
        )
        .unwrap();
        let stale = draft.action_revision();
        draft
            .update(&stale, content("Updated private body"), now)
            .unwrap();
        assert!(draft.freeze(&stale, now).is_err());
        let exact = draft.action_revision();
        draft.freeze(&exact, now).unwrap();
        let intent = draft.mutation_intent().unwrap();
        assert_eq!(intent.target_ref(), format!("local-draft:{}", draft.id));
        assert!(!format!("{draft:?}").contains("Updated private body"));
        let view = serde_json::to_string(&draft.public_view()).unwrap();
        assert!(!view.contains("Updated private body"));
        assert!(!view.contains("Private subject"));
    }
}
