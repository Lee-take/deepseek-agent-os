use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::domain::MailAddress;
use super::ConnectorCapability;

const MAX_REMOTE_REF_CHARS: usize = 1024;
const MAX_MESSAGE_ID_CHARS: usize = 998;
const MAX_EVENT_TEXT_CHARS: usize = 16 * 1024;
const MAX_EVENT_TITLE_CHARS: usize = 512;
const MAX_EVENT_LOCATION_CHARS: usize = 2 * 1024;
const MAX_TIMEZONE_CHARS: usize = 128;
const MAX_ETAG_CHARS: usize = 1024;
const MAX_ATTENDEES: usize = 100;

/// Private, deterministic provider input. The Tool request carries only its hash;
/// this value is persisted inside the Kernel invocation projection and is never a
/// model-facing or frontend-facing DTO.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ConnectorMutationIntent {
    MailSendDraft {
        draft_ref: String,
        internet_message_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<ConnectorMailDraftContent>,
    },
    CalendarCreateEvent {
        calendar_ref: String,
        event: CalendarMutationEvent,
    },
    CalendarUpdateEvent {
        calendar_ref: String,
        event_ref: String,
        expected_etag: String,
        event: CalendarMutationEvent,
    },
    CalendarCancelEvent {
        calendar_ref: String,
        event_ref: String,
        expected_etag: String,
    },
}

impl std::fmt::Debug for ConnectorMutationIntent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectorMutationIntent")
            .field("capability", &self.capability())
            .field("target_ref", &"[redacted]")
            .field("content", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct CalendarMutationEvent {
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub timezone: String,
    pub attendees: Vec<MailAddress>,
    pub notify_attendees: bool,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorMailDraftContent {
    pub to: Vec<MailAddress>,
    pub cc: Vec<MailAddress>,
    pub bcc: Vec<MailAddress>,
    pub subject: String,
    pub body_text: String,
    pub in_reply_to: Option<String>,
    pub thread_ref: Option<String>,
}

impl std::fmt::Debug for ConnectorMailDraftContent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectorMailDraftContent")
            .field(
                "recipient_count",
                &(self.to.len() + self.cc.len() + self.bcc.len()),
            )
            .field("content", &"[redacted]")
            .finish()
    }
}

impl std::fmt::Debug for CalendarMutationEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CalendarMutationEvent")
            .field("starts_at", &self.starts_at)
            .field("ends_at", &self.ends_at)
            .field("attendee_count", &self.attendees.len())
            .field("notify_attendees", &self.notify_attendees)
            .field("content", &"[redacted]")
            .finish()
    }
}

impl ConnectorMutationIntent {
    pub fn capability(&self) -> ConnectorCapability {
        match self {
            Self::MailSendDraft { .. } => ConnectorCapability::MailSendDraft,
            Self::CalendarCreateEvent { .. } => ConnectorCapability::CalendarCreateEvent,
            Self::CalendarUpdateEvent { .. } => ConnectorCapability::CalendarUpdateEvent,
            Self::CalendarCancelEvent { .. } => ConnectorCapability::CalendarCancelEvent,
        }
    }

    pub fn target_ref(&self) -> &str {
        match self {
            Self::MailSendDraft { draft_ref, .. } => draft_ref,
            Self::CalendarCreateEvent { calendar_ref, .. } => calendar_ref,
            Self::CalendarUpdateEvent { event_ref, .. }
            | Self::CalendarCancelEvent { event_ref, .. } => event_ref,
        }
    }

    pub fn mail_content(&self) -> Option<&ConnectorMailDraftContent> {
        match self {
            Self::MailSendDraft { content, .. } => content.as_ref(),
            _ => None,
        }
    }

    pub fn hash(&self) -> Result<String, String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|_| "connector mutation intent could not be encoded".to_string())?;
        Ok(format!("intent1:{}", hex::encode(Sha256::digest(encoded))))
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::MailSendDraft {
                draft_ref,
                internet_message_id,
                content,
            } => {
                bounded_required(draft_ref, MAX_REMOTE_REF_CHARS, "mail draft reference")?;
                bounded_required(
                    internet_message_id,
                    MAX_MESSAGE_ID_CHARS,
                    "mail internet message id",
                )?;
                if !internet_message_id.starts_with('<')
                    || !internet_message_id.ends_with('>')
                    || !internet_message_id.contains('@')
                    || contains_line_break(internet_message_id)
                {
                    return Err("mail internet message id is invalid".to_string());
                }
                if let Some(content) = content {
                    content.validate()?;
                }
            }
            Self::CalendarCreateEvent {
                calendar_ref,
                event,
            } => {
                bounded_required(calendar_ref, MAX_REMOTE_REF_CHARS, "calendar reference")?;
                event.validate()?;
            }
            Self::CalendarUpdateEvent {
                calendar_ref,
                event_ref,
                expected_etag,
                event,
            } => {
                bounded_required(calendar_ref, MAX_REMOTE_REF_CHARS, "calendar reference")?;
                bounded_required(event_ref, MAX_REMOTE_REF_CHARS, "calendar event reference")?;
                bounded_required(expected_etag, MAX_ETAG_CHARS, "calendar event etag")?;
                event.validate()?;
            }
            Self::CalendarCancelEvent {
                calendar_ref,
                event_ref,
                expected_etag,
            } => {
                bounded_required(calendar_ref, MAX_REMOTE_REF_CHARS, "calendar reference")?;
                bounded_required(event_ref, MAX_REMOTE_REF_CHARS, "calendar event reference")?;
                bounded_required(expected_etag, MAX_ETAG_CHARS, "calendar event etag")?;
            }
        }
        Ok(())
    }
}

impl ConnectorMailDraftContent {
    pub fn validate(&self) -> Result<(), String> {
        let recipient_count = self.to.len() + self.cc.len() + self.bcc.len();
        if recipient_count == 0 || recipient_count > 100 {
            return Err("mail draft recipient budget is invalid".to_string());
        }
        for address in self.to.iter().chain(self.cc.iter()).chain(self.bcc.iter()) {
            bounded_required(&address.address, 512, "mail draft recipient")?;
            if !address.address.contains('@') {
                return Err("mail draft recipient is invalid".to_string());
            }
            bounded_optional(
                address.display_name.as_deref(),
                256,
                "mail draft recipient display name",
            )?;
            if address
                .display_name
                .as_ref()
                .is_some_and(|value| contains_line_break(value))
            {
                return Err("mail draft recipient display name is invalid".to_string());
            }
        }
        if self.subject.chars().count() > 998 || contains_line_break(&self.subject) {
            return Err("mail draft subject is invalid".to_string());
        }
        if self.body_text.len() > 64 * 1024 || self.body_text.contains('\0') {
            return Err("mail draft body budget exceeded".to_string());
        }
        if self.in_reply_to.as_ref().is_some_and(|value| {
            value.len() > MAX_MESSAGE_ID_CHARS
                || contains_line_break(value)
                || !value.starts_with('<')
                || !value.ends_with('>')
        }) {
            return Err("mail draft reply reference is invalid".to_string());
        }
        if self.thread_ref.as_ref().is_some_and(|value| {
            value.trim().is_empty()
                || value.len() > MAX_REMOTE_REF_CHARS
                || contains_line_break(value)
        }) {
            return Err("mail draft thread reference is invalid".to_string());
        }
        Ok(())
    }
}

pub fn build_rfc5322_message(
    internet_message_id: &str,
    content: &ConnectorMailDraftContent,
) -> Result<Vec<u8>, String> {
    ConnectorMutationIntent::MailSendDraft {
        draft_ref: "local-draft-validation".to_string(),
        internet_message_id: internet_message_id.to_string(),
        content: Some(content.clone()),
    }
    .validate()?;
    let mut headers = vec![
        "MIME-Version: 1.0".to_string(),
        format!("Message-ID: {internet_message_id}"),
        format!("To: {}", format_addresses(&content.to)),
    ];
    if !content.cc.is_empty() {
        headers.push(format!("Cc: {}", format_addresses(&content.cc)));
    }
    if !content.bcc.is_empty() {
        headers.push(format!("Bcc: {}", format_addresses(&content.bcc)));
    }
    headers.push(format!("Subject: {}", encode_header(&content.subject)));
    if let Some(in_reply_to) = &content.in_reply_to {
        headers.push(format!("In-Reply-To: {in_reply_to}"));
        headers.push(format!("References: {in_reply_to}"));
    }
    headers.push("Content-Type: text/plain; charset=UTF-8".to_string());
    headers.push("Content-Transfer-Encoding: 8bit".to_string());
    let normalized_body = content
        .body_text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n");
    let message = format!("{}\r\n\r\n{}", headers.join("\r\n"), normalized_body);
    if message.len() > 128 * 1024 {
        return Err("mail MIME payload budget exceeded".to_string());
    }
    Ok(message.into_bytes())
}

fn format_addresses(addresses: &[MailAddress]) -> String {
    addresses
        .iter()
        .map(|address| match &address.display_name {
            Some(display_name) if !display_name.trim().is_empty() => {
                format!("{} <{}>", encode_header(display_name), address.address)
            }
            _ => address.address.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn encode_header(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| byte.is_ascii() && !byte.is_ascii_control())
    {
        value.to_string()
    } else {
        format!("=?UTF-8?B?{}?=", general_purpose::STANDARD.encode(value))
    }
}

impl CalendarMutationEvent {
    pub fn validate(&self) -> Result<(), String> {
        bounded_required(&self.title, MAX_EVENT_TITLE_CHARS, "calendar event title")?;
        bounded_optional(
            self.description.as_deref(),
            MAX_EVENT_TEXT_CHARS,
            "calendar event description",
        )?;
        bounded_optional(
            self.location.as_deref(),
            MAX_EVENT_LOCATION_CHARS,
            "calendar event location",
        )?;
        bounded_required(
            &self.timezone,
            MAX_TIMEZONE_CHARS,
            "calendar event timezone",
        )?;
        if self.ends_at <= self.starts_at {
            return Err("calendar event must end after it starts".to_string());
        }
        if self.ends_at.signed_duration_since(self.starts_at) > chrono::Duration::days(366) {
            return Err("calendar event duration cannot exceed 366 days".to_string());
        }
        if self.attendees.len() > MAX_ATTENDEES {
            return Err("calendar attendee budget exceeded".to_string());
        }
        for attendee in &self.attendees {
            bounded_required(&attendee.address, 512, "calendar attendee address")?;
            if !attendee.address.contains('@') || contains_line_break(&attendee.address) {
                return Err("calendar attendee address is invalid".to_string());
            }
            bounded_optional(
                attendee.display_name.as_deref(),
                256,
                "calendar attendee display name",
            )?;
        }
        Ok(())
    }
}

pub fn deterministic_google_event_id(idempotency_key: &str) -> Result<String, String> {
    bounded_required(idempotency_key, 1024, "connector idempotency key")?;
    let digest = Sha256::digest(idempotency_key.as_bytes());
    // Google event ids use base32hex (0-9, a-v); a SHA-256 prefix is already
    // valid base32hex and gives a stable collision-resistant 128-bit id.
    Ok(hex::encode(&digest[..16]))
}

fn bounded_required(value: &str, maximum: usize, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().count() > maximum || contains_line_break(value) {
        return Err(format!("{field} is invalid"));
    }
    Ok(())
}

fn bounded_optional(value: Option<&str>, maximum: usize, field: &str) -> Result<(), String> {
    if value.is_some_and(|value| value.chars().count() > maximum) {
        return Err(format!("{field} budget exceeded"));
    }
    Ok(())
}

fn contains_line_break(value: &str) -> bool {
    value.contains(['\r', '\n'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calendar_event() -> CalendarMutationEvent {
        let starts_at = Utc::now();
        CalendarMutationEvent {
            title: "Reviewed event".to_string(),
            description: Some("Private provider payload".to_string()),
            location: None,
            starts_at,
            ends_at: starts_at + chrono::Duration::hours(1),
            timezone: "Asia/Shanghai".to_string(),
            attendees: vec![MailAddress {
                display_name: Some("Reviewer".to_string()),
                address: "reviewer@example.com".to_string(),
            }],
            notify_attendees: false,
        }
    }

    #[test]
    fn mutation_intents_are_bounded_hashed_and_debug_redacted() {
        let intent = ConnectorMutationIntent::CalendarCreateEvent {
            calendar_ref: "primary".to_string(),
            event: calendar_event(),
        };
        let hash = intent.hash().unwrap();
        assert_eq!(hash.len(), 72);
        assert!(hash.starts_with("intent1:"));
        let debug = format!("{intent:?}");
        assert!(!debug.contains("Private provider payload"));
        assert!(!debug.contains("reviewer@example.com"));

        let mut changed = intent.clone();
        let ConnectorMutationIntent::CalendarCreateEvent { event, .. } = &mut changed else {
            unreachable!()
        };
        event.title.push('!');
        assert_ne!(hash, changed.hash().unwrap());
    }

    #[test]
    fn mutation_intents_reject_header_injection_and_unbounded_content() {
        assert!(ConnectorMutationIntent::MailSendDraft {
            draft_ref: "draft:1".to_string(),
            internet_message_id: "<safe@example.com>\r\nBcc: attacker@example.com".to_string(),
            content: None,
        }
        .validate()
        .is_err());
        let mut event = calendar_event();
        event.description = Some("x".repeat(MAX_EVENT_TEXT_CHARS + 1));
        assert!(ConnectorMutationIntent::CalendarCreateEvent {
            calendar_ref: "primary".to_string(),
            event,
        }
        .validate()
        .is_err());
    }

    #[test]
    fn google_event_id_is_stable_and_base32hex_compatible() {
        let first = deterministic_google_event_id("automation:run:event:once").unwrap();
        let second = deterministic_google_event_id("automation:run:event:once").unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 32);
        assert!(first
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    }

    #[test]
    fn rfc5322_builder_is_bounded_injection_safe_and_unicode_aware() {
        let content = ConnectorMailDraftContent {
            to: vec![MailAddress {
                display_name: Some("审核人".to_string()),
                address: "reviewer@example.com".to_string(),
            }],
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: "请审核".to_string(),
            body_text: "第一行\n第二行".to_string(),
            in_reply_to: Some("<parent@example.com>".to_string()),
            thread_ref: Some("thread-1".to_string()),
        };
        let mime = build_rfc5322_message("<draft@example.com>", &content).unwrap();
        let mime = String::from_utf8(mime).unwrap();
        assert!(mime.contains("Message-ID: <draft@example.com>\r\n"));
        assert!(mime.contains("Subject: =?UTF-8?B?"));
        assert!(mime.ends_with("第一行\r\n第二行"));
        assert!(!mime.contains("\nBcc: attacker"));
    }
}
