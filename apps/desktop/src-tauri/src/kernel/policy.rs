use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::kernel::models::AccessMode;

pub const TOOL_APPROVAL_PREVIEW_REVISION: u32 = 1;

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
    ConnectorAttachmentRead,
    ConnectorWrite,
    DriveRead,
    DriveWrite,
    TerminalRead,
    TerminalWrite,
    ComputerScreenshot,
    ComputerControl,
    AppUpdateCheck,
    AppUpdateDownload,
    AppUpdateInstall,
    SkillUse,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityFamily {
    File,
    Network,
    Browser,
    Email,
    Drive,
    Terminal,
    ComputerUse,
    AppUpdate,
    Skill,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAccessStatus {
    AutoApproved,
    PendingApproval,
    Approved,
    Rejected,
    Denied,
}

#[derive(Clone, Copy, Debug, Deserialize, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityGrantState {
    #[default]
    NotGranted,
    Reusable,
    OneShotAvailable,
    OneShotConsumed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityDescriptor {
    pub family: CapabilityFamily,
    pub capability: CapabilityKind,
    pub title: String,
    pub summary: String,
    pub risk_level: RiskLevel,
    pub default_scope: String,
    pub experimental: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExactToolApprovalScope {
    pub tool_id: String,
    pub request_fingerprint: String,
    pub preview: String,
    #[serde(default)]
    pub preview_revision: u32,
    #[serde(default)]
    pub preview_hash: String,
}

pub fn exact_tool_preview_hash(preview_revision: u32, preview: &str) -> String {
    let preview = preview.trim();
    let mut digest = Sha256::new();
    digest.update(b"ds-agent.tool-approval-preview.v1\0");
    digest.update(preview_revision.to_be_bytes());
    digest.update((preview.len() as u64).to_be_bytes());
    digest.update(preview.as_bytes());
    format!("{:x}", digest.finalize())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityAccessRequest {
    pub id: Uuid,
    pub access_mode: AccessMode,
    pub family: CapabilityFamily,
    pub capability: CapabilityKind,
    pub title: String,
    pub summary: String,
    pub risk_level: RiskLevel,
    pub decision: PolicyDecision,
    pub status: CapabilityAccessStatus,
    pub reason: String,
    #[serde(default)]
    pub exact_tool: Option<ExactToolApprovalScope>,
    pub created_at: DateTime<Utc>,
}

impl CapabilityAccessRequest {
    pub fn bind_exact_tool(
        &mut self,
        tool_id: impl Into<String>,
        request_fingerprint: impl Into<String>,
        preview: impl Into<String>,
    ) -> Result<(), String> {
        let tool_id = tool_id.into();
        let request_fingerprint = request_fingerprint.into();
        let preview = preview.into();
        if tool_id.trim().is_empty()
            || tool_id.len() > 128
            || request_fingerprint.len() != 64
            || !request_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || preview.trim().is_empty()
            || preview.len() > 512
        {
            return Err("exact tool approval scope is invalid".to_string());
        }
        self.exact_tool = Some(ExactToolApprovalScope {
            tool_id,
            request_fingerprint,
            preview: preview.trim().to_string(),
            preview_revision: TOOL_APPROVAL_PREVIEW_REVISION,
            preview_hash: exact_tool_preview_hash(TOOL_APPROVAL_PREVIEW_REVISION, &preview),
        });
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PermissionResolution {
    pub id: Uuid,
    pub request_id: Uuid,
    pub approved: bool,
    pub note: String,
    #[serde(default)]
    pub expected_request_revision: Option<u64>,
    #[serde(default)]
    pub exact_preview_revision: Option<u32>,
    #[serde(default)]
    pub exact_preview_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl PermissionResolution {
    pub fn new(request_id: Uuid, approved: bool, note: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            request_id,
            approved,
            note: note.trim().to_string(),
            expected_request_revision: None,
            exact_preview_revision: None,
            exact_preview_hash: None,
            created_at: Utc::now(),
        }
    }

    pub fn new_exact(
        request_id: Uuid,
        approved: bool,
        note: String,
        expected_request_revision: u64,
        scope: &ExactToolApprovalScope,
    ) -> Result<Self, String> {
        if scope.preview_revision == 0
            || scope.preview_hash != exact_tool_preview_hash(scope.preview_revision, &scope.preview)
        {
            return Err("exact tool preview evidence is invalid".to_string());
        }
        Ok(Self {
            id: Uuid::new_v4(),
            request_id,
            approved,
            note: note.trim().to_string(),
            expected_request_revision: Some(expected_request_revision),
            exact_preview_revision: Some(scope.preview_revision),
            exact_preview_hash: Some(scope.preview_hash.clone()),
            created_at: Utc::now(),
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityAccessRecord {
    pub request: CapabilityAccessRequest,
    pub resolution: Option<PermissionResolution>,
    pub effective_status: CapabilityAccessStatus,
    #[serde(default)]
    pub projection_revision: u64,
    #[serde(default)]
    pub grant_state: CapabilityGrantState,
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

pub fn builtin_capability_catalog() -> Vec<CapabilityDescriptor> {
    [
        descriptor(
            CapabilityFamily::File,
            CapabilityKind::FileRead,
            "Read local files",
            "Read selected files inside the current workspace.",
            "workspace",
            false,
        ),
        descriptor(
            CapabilityFamily::File,
            CapabilityKind::FileWrite,
            "Write local files",
            "Create or modify drafts and exported artifacts in approved folders.",
            "workspace",
            false,
        ),
        descriptor(
            CapabilityFamily::Network,
            CapabilityKind::NetworkSearch,
            "Network search",
            "Search the public web and collect source links for review.",
            "internet_search",
            false,
        ),
        descriptor(
            CapabilityFamily::Browser,
            CapabilityKind::BrowserBrowse,
            "Browser browsing",
            "Open and inspect web pages through the browser capability.",
            "web_browse",
            false,
        ),
        descriptor(
            CapabilityFamily::Browser,
            CapabilityKind::BrowserSubmit,
            "Browser form submission",
            "Fill or submit forms only after policy review.",
            "form_submission",
            false,
        ),
        descriptor(
            CapabilityFamily::Email,
            CapabilityKind::EmailRead,
            "Email read",
            "Read selected mailbox threads for task evidence.",
            "email_read",
            false,
        ),
        descriptor(
            CapabilityFamily::Email,
            CapabilityKind::EmailDraft,
            "Email draft",
            "Prepare draft replies without sending them.",
            "email_draft",
            false,
        ),
        descriptor(
            CapabilityFamily::Email,
            CapabilityKind::EmailSend,
            "Email send",
            "Send approved outbound email from a connected mailbox.",
            "email_send",
            false,
        ),
        descriptor(
            CapabilityFamily::Email,
            CapabilityKind::ConnectorAttachmentRead,
            "Connected attachment download",
            "Download one approved untrusted attachment into the managed workspace.",
            "single_connector_attachment",
            true,
        ),
        descriptor(
            CapabilityFamily::Email,
            CapabilityKind::ConnectorWrite,
            "Connected account change",
            "Apply one approved external account mutation.",
            "single_external_change",
            true,
        ),
        descriptor(
            CapabilityFamily::Drive,
            CapabilityKind::DriveRead,
            "Drive read",
            "Read selected cloud-drive files and folders.",
            "drive_read",
            false,
        ),
        descriptor(
            CapabilityFamily::Drive,
            CapabilityKind::DriveWrite,
            "Drive write",
            "Upload or export approved artifacts to cloud drive.",
            "drive_write",
            false,
        ),
        descriptor(
            CapabilityFamily::Terminal,
            CapabilityKind::TerminalRead,
            "Terminal read",
            "Run read-only diagnostics and collect command output.",
            "terminal_read",
            false,
        ),
        descriptor(
            CapabilityFamily::Terminal,
            CapabilityKind::TerminalWrite,
            "Terminal write",
            "Run commands that can mutate files or machine state.",
            "terminal_write",
            false,
        ),
        descriptor(
            CapabilityFamily::ComputerUse,
            CapabilityKind::ComputerScreenshot,
            "Computer screenshot",
            "Capture or inspect the visible desktop for context.",
            "computer_screenshot",
            true,
        ),
        descriptor(
            CapabilityFamily::ComputerUse,
            CapabilityKind::ComputerControl,
            "Computer control",
            "Use mouse or keyboard actions with per-step approval.",
            "computer_control",
            true,
        ),
        descriptor(
            CapabilityFamily::AppUpdate,
            CapabilityKind::AppUpdateCheck,
            "Check for DS Agent updates",
            "Read trusted release metadata and compare it with the installed version.",
            "trusted_release_metadata",
            false,
        ),
        descriptor(
            CapabilityFamily::AppUpdate,
            CapabilityKind::AppUpdateDownload,
            "Download a DS Agent update",
            "Download a trusted Windows installer into the isolated update directory.",
            "app_update_directory",
            false,
        ),
        descriptor(
            CapabilityFamily::AppUpdate,
            CapabilityKind::AppUpdateInstall,
            "Install a DS Agent update",
            "Launch a verified installer and restart DS Agent after explicit approval.",
            "verified_update_installer",
            false,
        ),
        descriptor(
            CapabilityFamily::Skill,
            CapabilityKind::SkillUse,
            "Use an installed declarative skill",
            "Load a trusted, enabled, hash-verified declarative skill entry as planning evidence.",
            "installed_skill_catalog",
            false,
        ),
    ]
    .to_vec()
}

pub fn capability_descriptor(capability: CapabilityKind) -> Option<CapabilityDescriptor> {
    builtin_capability_catalog()
        .into_iter()
        .find(|descriptor| descriptor.capability == capability)
}

pub fn request_capability_access(
    access_mode: AccessMode,
    capability: CapabilityKind,
) -> Result<CapabilityAccessRequest, String> {
    let descriptor = capability_descriptor(capability)
        .ok_or_else(|| "capability is not declared in the built-in catalog".to_string())?;
    let risk_level = descriptor.risk_level;
    let decision = decide(access_mode, capability);
    let status = match decision {
        PolicyDecision::Allow => CapabilityAccessStatus::AutoApproved,
        PolicyDecision::Ask => CapabilityAccessStatus::PendingApproval,
        PolicyDecision::Deny => CapabilityAccessStatus::Denied,
    };

    Ok(CapabilityAccessRequest {
        id: Uuid::new_v4(),
        access_mode,
        family: descriptor.family,
        capability,
        title: descriptor.title,
        summary: descriptor.summary,
        risk_level,
        decision,
        status,
        reason: decision_reason(access_mode, capability, risk_level, decision).to_string(),
        exact_tool: None,
        created_at: Utc::now(),
    })
}

fn descriptor(
    family: CapabilityFamily,
    capability: CapabilityKind,
    title: &str,
    summary: &str,
    default_scope: &str,
    experimental: bool,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        family,
        capability,
        title: title.to_string(),
        summary: summary.to_string(),
        risk_level: capability_risk(capability),
        default_scope: default_scope.to_string(),
        experimental,
    }
}

pub fn capability_risk(capability: CapabilityKind) -> RiskLevel {
    match capability {
        CapabilityKind::FileRead
        | CapabilityKind::NetworkSearch
        | CapabilityKind::BrowserBrowse
        | CapabilityKind::EmailDraft
        | CapabilityKind::DriveRead
        | CapabilityKind::TerminalRead
        | CapabilityKind::AppUpdateCheck
        | CapabilityKind::SkillUse => RiskLevel::Low,
        CapabilityKind::EmailRead | CapabilityKind::ComputerScreenshot => RiskLevel::Medium,
        CapabilityKind::FileWrite
        | CapabilityKind::BrowserSubmit
        | CapabilityKind::DriveWrite
        | CapabilityKind::TerminalWrite
        | CapabilityKind::AppUpdateDownload => RiskLevel::High,
        CapabilityKind::EmailSend
        | CapabilityKind::ConnectorAttachmentRead
        | CapabilityKind::ConnectorWrite
        | CapabilityKind::ComputerControl
        | CapabilityKind::AppUpdateInstall => RiskLevel::Critical,
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
            CapabilityKind::EmailSend
            | CapabilityKind::ConnectorAttachmentRead
            | CapabilityKind::ConnectorWrite
            | CapabilityKind::ComputerControl
            | CapabilityKind::AppUpdateInstall => PolicyDecision::Ask,
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
            CapabilityKind::EmailSend
            | CapabilityKind::ConnectorAttachmentRead
            | CapabilityKind::ConnectorWrite
            | CapabilityKind::ComputerControl
            | CapabilityKind::AppUpdateInstall,
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
    use super::{
        builtin_capability_catalog, request_capability_access, CapabilityAccessStatus,
        CapabilityFamily, CapabilityKind, PermissionAuditEntry, PolicyDecision, RiskLevel,
    };
    use crate::kernel::models::AccessMode;

    #[test]
    fn builtin_catalog_declares_phase_two_connector_families() {
        let catalog = builtin_capability_catalog();

        assert!(catalog
            .iter()
            .any(|capability| capability.family == CapabilityFamily::Browser
                && capability.capability == CapabilityKind::BrowserBrowse));
        assert!(catalog
            .iter()
            .any(|capability| capability.family == CapabilityFamily::Email
                && capability.capability == CapabilityKind::EmailRead));
        assert!(catalog
            .iter()
            .any(|capability| capability.family == CapabilityFamily::Drive
                && capability.capability == CapabilityKind::DriveRead));
        assert!(catalog.iter().any(|capability| capability.family
            == CapabilityFamily::ComputerUse
            && capability.capability == CapabilityKind::ComputerScreenshot));
        assert!(catalog.iter().any(|capability| capability.family
            == CapabilityFamily::ComputerUse
            && capability.capability == CapabilityKind::ComputerControl
            && capability.experimental));
    }

    #[test]
    fn low_risk_access_request_is_auto_approved_when_policy_allows() {
        let request = request_capability_access(AccessMode::AskOnRisk, CapabilityKind::DriveRead)
            .expect("drive read is a declared capability");

        assert_eq!(request.capability, CapabilityKind::DriveRead);
        assert_eq!(request.risk_level, RiskLevel::Low);
        assert_eq!(request.decision, PolicyDecision::Allow);
        assert_eq!(request.status, CapabilityAccessStatus::AutoApproved);
        assert!(request.reason.contains("low risk"));
        assert!(request.created_at <= chrono::Utc::now());
    }

    #[test]
    fn critical_access_request_waits_for_user_even_in_full_access() {
        let request =
            request_capability_access(AccessMode::FullAccess, CapabilityKind::ComputerControl)
                .expect("computer control is declared but approval gated");

        assert_eq!(request.capability, CapabilityKind::ComputerControl);
        assert_eq!(request.risk_level, RiskLevel::Critical);
        assert_eq!(request.decision, PolicyDecision::Ask);
        assert_eq!(request.status, CapabilityAccessStatus::PendingApproval);
        assert!(request.reason.contains("critical"));
    }

    #[test]
    fn access_request_keeps_descriptor_context_for_ui() {
        let request =
            request_capability_access(AccessMode::LimitedAuto, CapabilityKind::BrowserSubmit)
                .expect("browser submit is declared");

        assert_eq!(request.family, CapabilityFamily::Browser);
        assert_eq!(request.title, "Browser form submission");
        assert!(!request.summary.is_empty());
    }

    #[test]
    fn ask_every_step_always_asks() {
        assert_eq!(
            request_capability_access(AccessMode::AskEveryStep, CapabilityKind::FileRead)
                .expect("file read is declared")
                .decision,
            PolicyDecision::Ask
        );
        assert_eq!(
            request_capability_access(AccessMode::AskEveryStep, CapabilityKind::EmailSend)
                .expect("email send is declared")
                .decision,
            PolicyDecision::Ask
        );
    }

    #[test]
    fn ask_on_risk_allows_low_risk_only() {
        assert_eq!(
            request_capability_access(AccessMode::AskOnRisk, CapabilityKind::FileRead)
                .expect("file read is declared")
                .decision,
            PolicyDecision::Allow
        );
        assert_eq!(
            request_capability_access(AccessMode::AskOnRisk, CapabilityKind::BrowserBrowse)
                .expect("browser browse is declared")
                .decision,
            PolicyDecision::Allow
        );
        assert_eq!(
            request_capability_access(AccessMode::AskOnRisk, CapabilityKind::FileWrite)
                .expect("file write is declared")
                .decision,
            PolicyDecision::Ask
        );
        assert_eq!(
            request_capability_access(AccessMode::AskOnRisk, CapabilityKind::EmailSend)
                .expect("email send is declared")
                .decision,
            PolicyDecision::Ask
        );
    }

    #[test]
    fn ask_on_risk_requires_approval_for_screen_pixel_capture() {
        let request =
            request_capability_access(AccessMode::AskOnRisk, CapabilityKind::ComputerScreenshot)
                .expect("computer screenshot is declared");

        assert_eq!(request.risk_level, RiskLevel::Medium);
        assert_eq!(request.decision, PolicyDecision::Ask);
        assert_eq!(request.status, CapabilityAccessStatus::PendingApproval);
        assert!(request.reason.contains("risky"));
    }

    #[test]
    fn limited_auto_allows_medium_but_asks_high_and_critical() {
        assert_eq!(
            request_capability_access(AccessMode::LimitedAuto, CapabilityKind::BrowserBrowse)
                .expect("browser browse is declared")
                .decision,
            PolicyDecision::Allow
        );
        assert_eq!(
            request_capability_access(AccessMode::LimitedAuto, CapabilityKind::FileWrite)
                .expect("file write is declared")
                .decision,
            PolicyDecision::Ask
        );
        assert_eq!(
            request_capability_access(AccessMode::LimitedAuto, CapabilityKind::EmailSend)
                .expect("email send is declared")
                .decision,
            PolicyDecision::Ask
        );
    }

    #[test]
    fn full_access_still_asks_for_email_send_and_computer_control() {
        assert_eq!(
            request_capability_access(AccessMode::FullAccess, CapabilityKind::FileWrite)
                .expect("file write is declared")
                .decision,
            PolicyDecision::Allow
        );
        assert_eq!(
            request_capability_access(AccessMode::FullAccess, CapabilityKind::EmailSend)
                .expect("email send is declared")
                .decision,
            PolicyDecision::Ask
        );
        assert_eq!(
            request_capability_access(AccessMode::FullAccess, CapabilityKind::ComputerControl)
                .expect("computer control is declared")
                .decision,
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
