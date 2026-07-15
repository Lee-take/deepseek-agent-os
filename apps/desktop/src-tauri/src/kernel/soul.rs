use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentSoulProfileUpdateProposal {
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub clear_fields: Vec<String>,
    #[serde(default, alias = "evidence")]
    pub current_message_evidence: String,
    #[serde(default)]
    pub confirmation_context: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentSoulProfileUpdateReceipt {
    pub update_id: Uuid,
    pub status: String,
    pub summary: String,
    pub changed_fields: Vec<String>,
    pub undo_available: bool,
    pub applied_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentSoulProfileUpdateAudit {
    pub id: Uuid,
    pub source_run_id: Option<Uuid>,
    pub current_message_sha256: String,
    pub previous_content: String,
    pub updated_content: String,
    pub previous_content_sha256: String,
    pub updated_content_sha256: String,
    pub changed_fields: Vec<String>,
    pub applied_at: DateTime<Utc>,
}

impl AgentSoulProfileUpdateAudit {
    pub fn new(
        source_run_id: Option<Uuid>,
        current_user_message: &str,
        previous_content: String,
        updated_content: String,
        changed_fields: Vec<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            source_run_id,
            current_message_sha256: soul_sha256(current_user_message),
            previous_content_sha256: soul_sha256(&previous_content),
            updated_content_sha256: soul_sha256(&updated_content),
            previous_content,
            updated_content,
            changed_fields,
            applied_at: Utc::now(),
        }
    }
}

pub fn soul_sha256(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}
