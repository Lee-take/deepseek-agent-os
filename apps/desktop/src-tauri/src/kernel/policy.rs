use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::kernel::models::AccessMode;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    FileRead,
    FileWrite,
    NetworkSearch,
    BrowserBrowse,
    BrowserSubmit,
    EmailRead,
    EmailDraft,
    EmailSend,
    DriveRead,
    DriveWrite,
    TerminalRead,
    TerminalWrite,
    ComputerScreenshot,
    ComputerControl,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecision {
    Allow,
    Ask,
    Deny,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PermissionAuditEntry {
    pub id: Uuid,
    pub access_mode: AccessMode,
    pub capability: CapabilityKind,
    pub risk_level: RiskLevel,
    pub decision: PolicyDecision,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

impl PermissionAuditEntry {
    pub fn evaluate(access_mode: AccessMode, capability: CapabilityKind) -> Self {
        let risk_level = capability_risk(capability);
        let decision = decide(access_mode, capability);

        Self {
            id: Uuid::new_v4(),
            access_mode,
            capability,
            risk_level,
            decision,
            reason: decision_reason(access_mode, capability, risk_level, decision).to_string(),
            created_at: Utc::now(),
        }
    }
}

pub fn capability_risk(capability: CapabilityKind) -> RiskLevel {
    match capability {
        CapabilityKind::FileRead
        | CapabilityKind::NetworkSearch
        | CapabilityKind::EmailDraft
        | CapabilityKind::DriveRead
        | CapabilityKind::TerminalRead
        | CapabilityKind::ComputerScreenshot => RiskLevel::Low,
        CapabilityKind::BrowserBrowse | CapabilityKind::EmailRead => RiskLevel::Medium,
        CapabilityKind::FileWrite
        | CapabilityKind::BrowserSubmit
        | CapabilityKind::DriveWrite
        | CapabilityKind::TerminalWrite => RiskLevel::High,
        CapabilityKind::EmailSend | CapabilityKind::ComputerControl => RiskLevel::Critical,
    }
}

pub fn decide(access_mode: AccessMode, capability: CapabilityKind) -> PolicyDecision {
    match access_mode {
        AccessMode::AskEveryStep => PolicyDecision::Ask,
        AccessMode::AskOnRisk => match capability_risk(capability) {
            RiskLevel::Low => PolicyDecision::Allow,
            RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical => PolicyDecision::Ask,
        },
        AccessMode::LimitedAuto => match capability_risk(capability) {
            RiskLevel::Low | RiskLevel::Medium => PolicyDecision::Allow,
            RiskLevel::High | RiskLevel::Critical => PolicyDecision::Ask,
        },
        AccessMode::FullAccess => match capability {
            CapabilityKind::EmailSend | CapabilityKind::ComputerControl => PolicyDecision::Ask,
            _ => PolicyDecision::Allow,
        },
    }
}

fn decision_reason(
    access_mode: AccessMode,
    capability: CapabilityKind,
    risk_level: RiskLevel,
    decision: PolicyDecision,
) -> &'static str {
    match (access_mode, capability, risk_level, decision) {
        (
            _,
            CapabilityKind::EmailSend | CapabilityKind::ComputerControl,
            RiskLevel::Critical,
            _,
        ) => "critical capability requires explicit approval",
        (AccessMode::AskEveryStep, _, _, PolicyDecision::Ask) => {
            "ask_every_step requires approval before every capability"
        }
        (AccessMode::AskOnRisk, _, RiskLevel::Low, PolicyDecision::Allow) => {
            "ask_on_risk allows low risk capability"
        }
        (AccessMode::AskOnRisk, _, _, PolicyDecision::Ask) => {
            "ask_on_risk requires approval for risky capability"
        }
        (AccessMode::LimitedAuto, _, RiskLevel::Low | RiskLevel::Medium, PolicyDecision::Allow) => {
            "limited_auto allows low and medium risk capability"
        }
        (AccessMode::LimitedAuto, _, RiskLevel::High, PolicyDecision::Ask) => {
            "limited_auto requires approval for high risk capability"
        }
        (AccessMode::FullAccess, _, _, PolicyDecision::Allow) => {
            "full_access allows non-critical capability"
        }
        _ => "policy decision recorded",
    }
}

#[cfg(test)]
mod tests {
    use super::{decide, CapabilityKind, PermissionAuditEntry, PolicyDecision, RiskLevel};
    use crate::kernel::models::AccessMode;

    #[test]
    fn ask_every_step_always_asks() {
        assert_eq!(
            decide(AccessMode::AskEveryStep, CapabilityKind::FileRead),
            PolicyDecision::Ask
        );
        assert_eq!(
            decide(AccessMode::AskEveryStep, CapabilityKind::EmailSend),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn ask_on_risk_allows_low_risk_only() {
        assert_eq!(
            decide(AccessMode::AskOnRisk, CapabilityKind::FileRead),
            PolicyDecision::Allow
        );
        assert_eq!(
            decide(AccessMode::AskOnRisk, CapabilityKind::BrowserBrowse),
            PolicyDecision::Ask
        );
        assert_eq!(
            decide(AccessMode::AskOnRisk, CapabilityKind::FileWrite),
            PolicyDecision::Ask
        );
        assert_eq!(
            decide(AccessMode::AskOnRisk, CapabilityKind::EmailSend),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn limited_auto_allows_medium_but_asks_high_and_critical() {
        assert_eq!(
            decide(AccessMode::LimitedAuto, CapabilityKind::BrowserBrowse),
            PolicyDecision::Allow
        );
        assert_eq!(
            decide(AccessMode::LimitedAuto, CapabilityKind::FileWrite),
            PolicyDecision::Ask
        );
        assert_eq!(
            decide(AccessMode::LimitedAuto, CapabilityKind::EmailSend),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn full_access_still_asks_for_email_send_and_computer_control() {
        assert_eq!(
            decide(AccessMode::FullAccess, CapabilityKind::FileWrite),
            PolicyDecision::Allow
        );
        assert_eq!(
            decide(AccessMode::FullAccess, CapabilityKind::EmailSend),
            PolicyDecision::Ask
        );
        assert_eq!(
            decide(AccessMode::FullAccess, CapabilityKind::ComputerControl),
            PolicyDecision::Ask
        );
    }

    #[test]
    fn permission_audit_entry_captures_policy_decision_context() {
        let entry =
            PermissionAuditEntry::evaluate(AccessMode::FullAccess, CapabilityKind::EmailSend);

        assert_eq!(entry.access_mode, AccessMode::FullAccess);
        assert_eq!(entry.capability, CapabilityKind::EmailSend);
        assert_eq!(entry.risk_level, RiskLevel::Critical);
        assert_eq!(entry.decision, PolicyDecision::Ask);
        assert!(entry.reason.contains("critical"));
        assert!(entry.created_at <= chrono::Utc::now());
    }
}
