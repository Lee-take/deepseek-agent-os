use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const MAX_REMOTE_REF_CHARS: usize = 1024;
const MAX_ADDRESS_CHARS: usize = 512;
const MAX_DISPLAY_NAME_CHARS: usize = 256;
const MAX_SUBJECT_CHARS: usize = 512;
const MAX_BODY_SUMMARY_CHARS: usize = 2000;
const MAX_RECIPIENTS: usize = 50;
const MAX_ATTACHMENTS: usize = 100;
const MAX_ATTENDEES: usize = 100;
const MAX_URL_CHARS: usize = 2048;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MailAddress {
    pub display_name: Option<String>,
    pub address: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MailAttachmentRef {
    pub remote_ref: String,
    pub file_name: String,
    pub media_type: String,
    pub size_bytes: u64,
    pub contains_macros: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MailMessage {
    pub remote_ref: String,
    pub thread_ref: String,
    pub from: MailAddress,
    pub to: Vec<MailAddress>,
    pub subject: String,
    pub received_at: DateTime<Utc>,
    pub bounded_body_summary: Option<String>,
    pub attachments: Vec<MailAttachmentRef>,
    #[serde(default)]
    pub has_attachments: bool,
    pub untrusted_evidence: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MailThread {
    pub remote_ref: String,
    pub messages: Vec<MailMessage>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CalendarAttendee {
    pub address: MailAddress,
    pub response: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CalendarEvent {
    pub remote_ref: String,
    pub calendar_ref: String,
    pub title: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub timezone: String,
    pub attendees: Vec<CalendarAttendee>,
    pub meeting_url: Option<String>,
    pub recurrence: Option<String>,
    pub untrusted_evidence: bool,
}

impl MailMessage {
    pub fn validate(&self) -> Result<(), String> {
        required_bounded(
            &self.remote_ref,
            MAX_REMOTE_REF_CHARS,
            "mail remote reference",
        )?;
        required_bounded(
            &self.thread_ref,
            MAX_REMOTE_REF_CHARS,
            "mail thread reference",
        )?;
        validate_address(&self.from)?;
        if self.to.len() > MAX_RECIPIENTS {
            return Err("mail recipient budget exceeded".to_string());
        }
        for address in &self.to {
            validate_address(address)?;
        }
        bounded_optional(&self.subject, MAX_SUBJECT_CHARS, "mail subject")?;
        if self
            .bounded_body_summary
            .as_ref()
            .is_some_and(|value| value.chars().count() > MAX_BODY_SUMMARY_CHARS)
        {
            return Err("mail body summary budget exceeded".to_string());
        }
        if !self.untrusted_evidence {
            return Err("mail content must remain untrusted evidence".to_string());
        }
        if self.attachments.len() > MAX_ATTACHMENTS {
            return Err("mail attachment metadata budget exceeded".to_string());
        }
        for attachment in &self.attachments {
            required_bounded(
                &attachment.remote_ref,
                MAX_REMOTE_REF_CHARS,
                "attachment remote reference",
            )?;
            required_bounded(&attachment.file_name, 512, "attachment file name")?;
            required_bounded(&attachment.media_type, 255, "attachment media type")?;
        }
        if !self.has_attachments && !self.attachments.is_empty() {
            return Err("mail attachment metadata is inconsistent".to_string());
        }
        Ok(())
    }
}

impl MailThread {
    pub fn validate(&self) -> Result<(), String> {
        required_bounded(
            &self.remote_ref,
            MAX_REMOTE_REF_CHARS,
            "mail thread reference",
        )?;
        if self.messages.is_empty() {
            return Err("mail thread requires at least one message".to_string());
        }
        for message in &self.messages {
            message.validate()?;
            if message.thread_ref != self.remote_ref {
                return Err("mail message does not belong to its normalized thread".to_string());
            }
        }
        Ok(())
    }
}

impl CalendarEvent {
    pub fn validate(&self) -> Result<(), String> {
        required_bounded(
            &self.remote_ref,
            MAX_REMOTE_REF_CHARS,
            "calendar event remote reference",
        )?;
        required_bounded(
            &self.calendar_ref,
            MAX_REMOTE_REF_CHARS,
            "calendar reference",
        )?;
        required_bounded(&self.timezone, 128, "calendar timezone")?;
        bounded_optional(&self.title, MAX_SUBJECT_CHARS, "calendar title")?;
        if self.ends_at <= self.starts_at {
            return Err("calendar event must end after it starts".to_string());
        }
        if !self.untrusted_evidence {
            return Err("calendar content must remain untrusted evidence".to_string());
        }
        if self.attendees.len() > MAX_ATTENDEES {
            return Err("calendar attendee budget exceeded".to_string());
        }
        for attendee in &self.attendees {
            validate_address(&attendee.address)?;
            if attendee
                .response
                .as_ref()
                .is_some_and(|value| value.chars().count() > 128)
            {
                return Err("calendar attendee response budget exceeded".to_string());
            }
        }
        if self
            .meeting_url
            .as_ref()
            .is_some_and(|value| value.chars().count() > MAX_URL_CHARS)
        {
            return Err("calendar meeting URL budget exceeded".to_string());
        }
        if self
            .recurrence
            .as_ref()
            .is_some_and(|value| value.chars().count() > 512)
        {
            return Err("calendar recurrence budget exceeded".to_string());
        }
        Ok(())
    }
}

fn validate_address(address: &MailAddress) -> Result<(), String> {
    required_bounded(&address.address, MAX_ADDRESS_CHARS, "mail address")?;
    if address
        .display_name
        .as_ref()
        .is_some_and(|value| value.chars().count() > MAX_DISPLAY_NAME_CHARS)
    {
        return Err("mail display name budget exceeded".to_string());
    }
    if !address.address.contains('@') {
        return Err("mail address is invalid".to_string());
    }
    Ok(())
}

fn required_bounded(value: &str, maximum: usize, field: &str) -> Result<(), String> {
    required(value, field)?;
    if value.chars().count() > maximum {
        return Err(format!("{field} budget exceeded"));
    }
    Ok(())
}

fn bounded_optional(value: &str, maximum: usize, field: &str) -> Result<(), String> {
    if value.chars().count() > maximum {
        return Err(format!("{field} budget exceeded"));
    }
    Ok(())
}

fn required(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} is required"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_mail_and_calendar_require_untrusted_evidence_boundary() {
        let now = Utc::now();
        let address = MailAddress {
            display_name: Some("Person".to_string()),
            address: "person@example.com".to_string(),
        };
        let mut message = MailMessage {
            remote_ref: "message:1".to_string(),
            thread_ref: "thread:1".to_string(),
            from: address.clone(),
            to: vec![address.clone()],
            subject: "Untrusted subject".to_string(),
            received_at: now,
            bounded_body_summary: Some("Untrusted evidence summary.".to_string()),
            attachments: vec![],
            has_attachments: false,
            untrusted_evidence: true,
        };
        message.validate().expect("mail is valid");
        message.untrusted_evidence = false;
        assert!(message.validate().is_err());

        let event = CalendarEvent {
            remote_ref: "event:1".to_string(),
            calendar_ref: "calendar:1".to_string(),
            title: "Review".to_string(),
            starts_at: now,
            ends_at: now + chrono::Duration::hours(1),
            timezone: "Asia/Shanghai".to_string(),
            attendees: vec![CalendarAttendee {
                address,
                response: None,
            }],
            meeting_url: None,
            recurrence: None,
            untrusted_evidence: true,
        };
        event.validate().expect("calendar event is valid");
    }
}
