use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::kernel::models::AccessMode;
use crate::kernel::policy::{decide, CapabilityKind, PolicyDecision, RiskLevel};

pub const APP_UPDATE_CHECK_TOOL_ID: &str = "app_update.check";
pub const APP_UPDATE_DOWNLOAD_TOOL_ID: &str = "app_update.download";
pub const APP_UPDATE_INSTALL_TOOL_ID: &str = "app_update.install";
pub const BROWSER_BROWSE_TOOL_ID: &str = "browser.browse";
pub const BROWSER_OPEN_TOOL_ID: &str = "browser.open";
pub const COMPUTER_CONTROL_TOOL_ID: &str = "computer.control";
pub const COMPUTER_SCREENSHOT_TOOL_ID: &str = "computer.screenshot";
pub const CONNECTOR_MUTATE_TOOL_ID: &str = "connector.mutate";
pub const CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID: &str = "connector.attachment.download";
pub const FILESYSTEM_MUTATE_TOOL_ID: &str = "filesystem.mutate";
pub const FILE_READ_TOOL_ID: &str = "file.read";
pub const FILE_WRITE_TOOL_ID: &str = "file.write";
pub const NETWORK_SEARCH_TOOL_ID: &str = "network.search";
pub const OFFICE_CREATE_TOOL_ID: &str = "office.create";
pub const OFFICE_OPEN_TOOL_ID: &str = "office.open";
pub const OFFICE_UPDATE_TOOL_ID: &str = "office.update";
pub const OPERATIONS_BRIEFING_TOOL_ID: &str = "operations.briefing";
pub const T1_POWERPOINT_TOOL_ID: &str = "operations.generate_powerpoint";
pub const T1_RECONCILIATION_TOOL_ID: &str = "operations.reconcile_excel";
pub const SKILL_ACTIVATE_TOOL_ID: &str = "skill.activate";
pub const TERMINAL_READ_TOOL_ID: &str = "terminal.read";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolValueType {
    String,
    Boolean,
    Number,
    Object,
    Array,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolFieldSchema {
    pub name: String,
    pub value_type: ToolValueType,
    pub nullable: bool,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolObjectSchema {
    pub properties: Vec<ToolFieldSchema>,
    pub required: Vec<String>,
    pub allow_additional: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPathScope {
    None,
    Workspace,
    LocalFilesystem,
    AppEvidenceDirectory,
    AppUpdateDirectory,
    InstalledSkillStore,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResourceAccess {
    Read,
    Write,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolResourceRequirement {
    pub key: String,
    pub access: ToolResourceAccess,
    pub lease_seconds: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolConstraints {
    pub allowed_network_hosts: Vec<String>,
    pub path_scope: ToolPathScope,
    pub mutates_machine_state: bool,
    pub protected_path_policy: String,
    pub resource: Option<ToolResourceRequirement>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolVerificationContract {
    pub recipe_id: String,
    pub description: String,
    pub required_evidence_kinds: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolContract {
    pub id: String,
    pub version: String,
    pub title: String,
    pub description: String,
    pub capability: CapabilityKind,
    pub risk_level: RiskLevel,
    pub executor_id: String,
    pub input_schema: ToolObjectSchema,
    pub output_schema: ToolObjectSchema,
    pub constraints: ToolConstraints,
    pub verification: ToolVerificationContract,
    pub recovery_hint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolExecutionRequest {
    pub tool_id: String,
    pub input: Value,
    pub access_mode: AccessMode,
    pub run_id: Option<Uuid>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolExecutionPlan {
    pub invocation_id: Uuid,
    pub request: ToolExecutionRequest,
    pub contract: ToolContract,
    pub policy_decision: PolicyDecision,
    pub prepared_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionStatus {
    WaitingForConfirmation,
    Running,
    Succeeded,
    Failed,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolEvidence {
    pub kind: String,
    pub reference: String,
    pub summary: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolVerificationResult {
    pub passed: bool,
    pub summary: String,
    pub checked_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolExecutionOutput {
    pub output: Value,
    pub evidence: Vec<ToolEvidence>,
    pub verification: ToolVerificationResult,
}

pub trait AgentToolExecutor {
    fn execute(&self, plan: &ToolExecutionPlan) -> Result<ToolExecutionOutput, String>;
}

impl ToolVerificationResult {
    pub fn passed(summary: impl Into<String>) -> Self {
        Self {
            passed: true,
            summary: summary.into(),
            checked_at: Utc::now(),
        }
    }

    pub fn failed(summary: impl Into<String>) -> Self {
        Self {
            passed: false,
            summary: summary.into(),
            checked_at: Utc::now(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolInvocationRecord {
    pub id: Uuid,
    pub run_id: Option<Uuid>,
    pub tool_id: String,
    pub tool_version: String,
    pub capability: CapabilityKind,
    pub status: ToolExecutionStatus,
    pub policy_decision: PolicyDecision,
    pub approval_request_id: Option<Uuid>,
    pub input_summary: String,
    #[serde(default)]
    pub request_fingerprint: String,
    pub output: Option<Value>,
    pub evidence: Vec<ToolEvidence>,
    pub verification: ToolVerificationResult,
    pub error: Option<String>,
    pub recovery_hint: String,
    pub elapsed_ms: u128,
    pub created_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl ToolInvocationRecord {
    pub fn waiting_for_confirmation(plan: &ToolExecutionPlan, approval_request_id: Uuid) -> Self {
        Self::unfinished(
            plan,
            ToolExecutionStatus::WaitingForConfirmation,
            Some(approval_request_id),
            "waiting for local permission approval",
        )
    }

    pub fn running(plan: &ToolExecutionPlan, approval_request_id: Option<Uuid>) -> Self {
        Self::unfinished(
            plan,
            ToolExecutionStatus::Running,
            approval_request_id,
            "execution is in progress",
        )
    }

    pub fn failed(
        plan: &ToolExecutionPlan,
        error: impl Into<String>,
        approval_request_id: Option<Uuid>,
        elapsed_ms: u128,
    ) -> Self {
        let error = error.into();
        Self {
            id: plan.invocation_id,
            run_id: plan.request.run_id,
            tool_id: plan.contract.id.clone(),
            tool_version: plan.contract.version.clone(),
            capability: plan.contract.capability,
            status: ToolExecutionStatus::Failed,
            policy_decision: plan.policy_decision,
            approval_request_id,
            input_summary: summarize_input(&plan.request.input),
            request_fingerprint: tool_request_fingerprint(&plan.request),
            output: None,
            evidence: Vec::new(),
            verification: ToolVerificationResult::failed(format!(
                "tool execution failed before verification: {error}"
            )),
            error: Some(error),
            recovery_hint: plan.contract.recovery_hint.clone(),
            elapsed_ms,
            created_at: plan.prepared_at,
            finished_at: Some(Utc::now()),
        }
    }

    pub fn blocked(plan: &ToolExecutionPlan, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            id: plan.invocation_id,
            run_id: plan.request.run_id,
            tool_id: plan.contract.id.clone(),
            tool_version: plan.contract.version.clone(),
            capability: plan.contract.capability,
            status: ToolExecutionStatus::Blocked,
            policy_decision: plan.policy_decision,
            approval_request_id: None,
            input_summary: summarize_input(&plan.request.input),
            request_fingerprint: tool_request_fingerprint(&plan.request),
            output: None,
            evidence: Vec::new(),
            verification: ToolVerificationResult::failed(format!(
                "tool execution was blocked: {reason}"
            )),
            error: Some(reason),
            recovery_hint: plan.contract.recovery_hint.clone(),
            elapsed_ms: 0,
            created_at: plan.prepared_at,
            finished_at: Some(Utc::now()),
        }
    }

    pub fn succeeded(
        plan: &ToolExecutionPlan,
        output: Value,
        evidence: Vec<ToolEvidence>,
        verification: ToolVerificationResult,
        approval_request_id: Option<Uuid>,
        elapsed_ms: u128,
    ) -> Result<Self, String> {
        validate_object_schema(&plan.contract.output_schema, &output, "tool output")?;
        if !verification.passed {
            return Err("tool execution cannot succeed before verification passes".to_string());
        }
        for required_kind in &plan.contract.verification.required_evidence_kinds {
            if !evidence.iter().any(|item| item.kind == *required_kind) {
                return Err(format!(
                    "tool execution is missing required evidence kind `{required_kind}`"
                ));
            }
        }

        Ok(Self {
            id: plan.invocation_id,
            run_id: plan.request.run_id,
            tool_id: plan.contract.id.clone(),
            tool_version: plan.contract.version.clone(),
            capability: plan.contract.capability,
            status: ToolExecutionStatus::Succeeded,
            policy_decision: plan.policy_decision,
            approval_request_id,
            input_summary: summarize_input(&plan.request.input),
            request_fingerprint: tool_request_fingerprint(&plan.request),
            output: Some(output),
            evidence,
            verification,
            error: None,
            recovery_hint: plan.contract.recovery_hint.clone(),
            elapsed_ms,
            created_at: plan.prepared_at,
            finished_at: Some(Utc::now()),
        })
    }

    fn unfinished(
        plan: &ToolExecutionPlan,
        status: ToolExecutionStatus,
        approval_request_id: Option<Uuid>,
        verification_summary: &str,
    ) -> Self {
        Self {
            id: plan.invocation_id,
            run_id: plan.request.run_id,
            tool_id: plan.contract.id.clone(),
            tool_version: plan.contract.version.clone(),
            capability: plan.contract.capability,
            status,
            policy_decision: plan.policy_decision,
            approval_request_id,
            input_summary: summarize_input(&plan.request.input),
            request_fingerprint: tool_request_fingerprint(&plan.request),
            output: None,
            evidence: Vec::new(),
            verification: ToolVerificationResult::failed(verification_summary),
            error: None,
            recovery_hint: plan.contract.recovery_hint.clone(),
            elapsed_ms: 0,
            created_at: plan.prepared_at,
            finished_at: None,
        }
    }
}

pub fn builtin_tool_catalog() -> Vec<ToolContract> {
    vec![
        ToolContract {
            id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Apply one approved connected-account change".to_string(),
            description: "Apply one frozen, idempotent external mutation after exact approval."
                .to_string(),
            capability: CapabilityKind::ConnectorWrite,
            risk_level: RiskLevel::Critical,
            executor_id: "kernel.connector.mutate.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("provider_id", ToolValueType::String, "Connector provider id."),
                    field("account_id", ToolValueType::String, "Connector account id."),
                    field(
                        "account_generation",
                        ToolValueType::Number,
                        "Frozen durable account generation.",
                    ),
                    field("capability", ToolValueType::String, "Normalized capability."),
                    field("target_ref", ToolValueType::String, "Normalized remote target."),
                    field("preview_hash", ToolValueType::String, "Frozen preview hash."),
                    field(
                        "intent_hash",
                        ToolValueType::String,
                        "Hash of private typed provider input.",
                    ),
                    field("idempotency_key", ToolValueType::String, "One-side-effect key."),
                    field("automation_run_id", ToolValueType::String, "Owning automation run."),
                ],
                &[
                    "provider_id",
                    "account_id",
                    "account_generation",
                    "capability",
                    "target_ref",
                    "preview_hash",
                    "idempotency_key",
                    "automation_run_id",
                ],
            ),
            output_schema: object_schema(
                vec![
                    field("remote_object_ref", ToolValueType::String, "Verified remote object."),
                    field("outcome", ToolValueType::String, "Normalized mutation outcome."),
                ],
                &["remote_object_ref", "outcome"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::None,
                mutates_machine_state: true,
                protected_path_policy: "connected account, exact approval, and idempotency key are mandatory".to_string(),
                resource: None,
            },
            verification: ToolVerificationContract {
                recipe_id: "connector.remote-mutation.v1".to_string(),
                description: "Require provider evidence and remote-state verification before success.".to_string(),
                required_evidence_kinds: vec!["connector_remote_state".to_string()],
            },
            recovery_hint: "Reconcile remote state before retrying; never replay an uncertain external mutation.".to_string(),
        },
        ToolContract {
            id: CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Download one approved connected attachment".to_string(),
            description: "Download one exact connected-account attachment into the managed workspace as untrusted evidence."
                .to_string(),
            capability: CapabilityKind::ConnectorAttachmentRead,
            risk_level: RiskLevel::Critical,
            executor_id: "kernel.connector.attachment.download.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("provider_id", ToolValueType::String, "Connector provider id."),
                    field("account_id", ToolValueType::String, "Connected account id."),
                    field("parent_remote_ref", ToolValueType::String, "Private parent object reference."),
                    field("attachment_remote_ref", ToolValueType::String, "Private attachment reference."),
                    field("file_name", ToolValueType::String, "Approved original filename."),
                    field("media_type", ToolValueType::String, "Approved media type."),
                    field("size_bytes", ToolValueType::Number, "Approved byte size."),
                    field("account_generation", ToolValueType::Number, "Durable account generation."),
                    field("workspace_identity", ToolValueType::String, "Hashed managed workspace identity."),
                ],
                &[
                    "provider_id",
                    "account_id",
                    "parent_remote_ref",
                    "attachment_remote_ref",
                    "file_name",
                    "media_type",
                    "size_bytes",
                    "account_generation",
                    "workspace_identity",
                ],
            ),
            output_schema: object_schema(
                vec![
                    field("landing_ref", ToolValueType::String, "Managed relative landing reference."),
                    field("media_type", ToolValueType::String, "Verified media type."),
                    field("byte_size", ToolValueType::Number, "Verified byte size."),
                    field("sha256", ToolValueType::String, "Verified SHA-256."),
                ],
                &["landing_ref", "media_type", "byte_size", "sha256"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: vec!["graph.microsoft.com".to_string()],
                path_scope: ToolPathScope::Workspace,
                mutates_machine_state: true,
                protected_path_policy: "exact one-shot approval, connected account generation, and fixed managed landing directory are mandatory"
                    .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "connector-attachment://managed-landing".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 2 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "connector.attachment.sha256-type-size-generation.v1".to_string(),
                description: "Require hash, type, size, workspace identity, and account generation verification before success."
                    .to_string(),
                required_evidence_kinds: vec!["connector_attachment".to_string()],
            },
            recovery_hint: "Recover or clean the durable landing reservation before requesting another one-shot download."
                .to_string(),
        },
        ToolContract {
            id: FILE_READ_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Read a sandboxed UTF-8 text file".to_string(),
            description:
                "Read bounded UTF-8 text from an allowed local or workspace-relative path."
                    .to_string(),
            capability: CapabilityKind::FileRead,
            risk_level: RiskLevel::Low,
            executor_id: "builtin.file.read.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("path", ToolValueType::String, "Local text file path."),
                    field(
                        "summary",
                        ToolValueType::String,
                        "Human-readable purpose for reading the file.",
                    ),
                ],
                &["path", "summary"],
            ),
            output_schema: object_schema(
                vec![
                    field("path", ToolValueType::String, "Sandbox-verified file path."),
                    field("title", ToolValueType::String, "File title."),
                    field("text", ToolValueType::String, "Bounded UTF-8 file text."),
                    field("bytes", ToolValueType::Number, "UTF-8 bytes read."),
                    field("encoding", ToolValueType::String, "Text encoding."),
                ],
                &["path", "title", "text", "bytes", "encoding"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::LocalFilesystem,
                mutates_machine_state: false,
                protected_path_policy:
                    "deny-first local read sandbox; protected paths and secret files always win"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "local_filesystem://mutation".to_string(),
                    access: ToolResourceAccess::Read,
                    lease_seconds: 10 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "file.read.utf8.v1".to_string(),
                description:
                    "Require the executor to verify path identity, UTF-8 encoding, byte count, and file-content evidence."
                        .to_string(),
                required_evidence_kinds: vec!["file_content".to_string()],
            },
            recovery_hint:
                "Choose an unprotected UTF-8 text file within the size limit and retry the smallest read."
                    .to_string(),
        },
        ToolContract {
            id: FILE_WRITE_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Write a UTF-8 workspace file".to_string(),
            description:
                "Write validated UTF-8 content inside the configured workspace sandbox."
                    .to_string(),
            capability: CapabilityKind::FileWrite,
            risk_level: RiskLevel::High,
            executor_id: "builtin.file.write.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("path", ToolValueType::String, "Workspace-relative output path."),
                    field("summary", ToolValueType::String, "Human-readable write purpose."),
                    field("content", ToolValueType::String, "Exact UTF-8 file content."),
                ],
                &["path", "summary", "content"],
            ),
            output_schema: object_schema(
                vec![
                    field("path", ToolValueType::String, "Sandbox-verified written path."),
                    field("bytes", ToolValueType::Number, "UTF-8 bytes written."),
                    field("encoding", ToolValueType::String, "Written text encoding."),
                ],
                &["path", "bytes", "encoding"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::Workspace,
                mutates_machine_state: true,
                protected_path_policy:
                    "deny-first workspace sandbox; protected paths and secret files always win"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "local_filesystem://mutation".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 30 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "file.write.utf8.v1".to_string(),
                description:
                    "Require the executor to report the exact path, UTF-8 byte count, and written-file evidence."
                        .to_string(),
                required_evidence_kinds: vec!["written_file".to_string()],
            },
            recovery_hint:
                "Choose an unprotected workspace path, review local permission, and retry the smallest write."
                    .to_string(),
        },
        ToolContract {
            id: OFFICE_CREATE_TOOL_ID.to_string(),
            version: "1.1.0".to_string(),
            title: "Create a verified Office artifact".to_string(),
            description:
                "Create one DOCX, XLSX, or PPTX artifact inside the managed workspace or desktop output boundary, including at most three DS Agent validation revisions as named siblings without overwriting the approved original."
                    .to_string(),
            capability: CapabilityKind::FileWrite,
            risk_level: RiskLevel::High,
            executor_id: "builtin.office.create.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("app", ToolValueType::String, "word, excel, or power_point."),
                    field("path", ToolValueType::String, "Managed Office output path."),
                    field("title", ToolValueType::String, "Artifact title."),
                    field("body", ToolValueType::String, "Primary artifact body."),
                    field("rows", ToolValueType::Array, "Structured spreadsheet rows."),
                    field("slides", ToolValueType::Array, "Structured presentation slides."),
                ],
                &["app", "path", "title", "body", "rows", "slides"],
            ),
            output_schema: object_schema(
                vec![
                    field("path", ToolValueType::String, "Verified artifact path."),
                    field("bytes", ToolValueType::Number, "Binary artifact bytes written."),
                    field("app", ToolValueType::String, "Office application kind."),
                    field(
                        "artifact_kind",
                        ToolValueType::String,
                        "Verified Office artifact kind.",
                    ),
                ],
                &["path", "bytes", "app", "artifact_kind"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::Workspace,
                mutates_machine_state: true,
                protected_path_policy:
                    "exact approved target plus at most three same-parent .revision-N siblings; deny-first workspace and managed desktop output sandbox; protected paths always win"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "local_filesystem://mutation".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 30 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "office.create.binary_artifact.v1".to_string(),
                description:
                    "Require matching path and app metadata, positive binary size, and Office artifact evidence."
                        .to_string(),
                required_evidence_kinds: vec!["office_artifact".to_string()],
            },
            recovery_hint:
                "Choose an unprotected managed output path, review local permission, and retry one artifact creation."
                    .to_string(),
        },
        ToolContract {
            id: OFFICE_OPEN_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Open a verified Office artifact".to_string(),
            description:
                "Open one existing DOCX, XLSX, or PPTX artifact through the local Office application or default launcher."
                    .to_string(),
            capability: CapabilityKind::FileRead,
            risk_level: RiskLevel::Medium,
            executor_id: "builtin.office.open.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("path", ToolValueType::String, "Managed Office artifact path."),
                    field(
                        "preferred_app",
                        ToolValueType::String,
                        "word, excel, or power_point.",
                    ),
                ],
                &["path", "preferred_app"],
            ),
            output_schema: object_schema(
                vec![
                    field("path", ToolValueType::String, "Verified opened artifact path."),
                    field("app", ToolValueType::String, "Office application kind."),
                    field(
                        "opener_label",
                        ToolValueType::String,
                        "Verified local launcher label.",
                    ),
                    field(
                        "fallback_note",
                        ToolValueType::String,
                        "Fallback launcher note, blank when no fallback was needed.",
                    ),
                ],
                &["path", "app", "opener_label", "fallback_note"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::Workspace,
                mutates_machine_state: true,
                protected_path_policy:
                    "deny-first managed Office read sandbox; protected paths always win and foreground launch is serialized"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "computer://foreground_desktop".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 2 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "office.open.launch_receipt.v1".to_string(),
                description:
                    "Require matching path and app metadata plus a non-empty local launcher receipt."
                        .to_string(),
                required_evidence_kinds: vec!["office_open_receipt".to_string()],
            },
            recovery_hint:
                "Choose an existing unprotected Office artifact, close conflicting foreground automation, and retry one launch."
                    .to_string(),
        },
        ToolContract {
            id: OFFICE_UPDATE_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Update a verified Office artifact".to_string(),
            description:
                "Update one existing DOCX, XLSX, or PPTX artifact inside the managed workspace or desktop boundary."
                    .to_string(),
            capability: CapabilityKind::FileWrite,
            risk_level: RiskLevel::High,
            executor_id: "builtin.office.update.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("app", ToolValueType::String, "word, excel, or power_point."),
                    field("path", ToolValueType::String, "Managed Office artifact path."),
                    field("body", ToolValueType::String, "Text content to append."),
                    field("rows", ToolValueType::Array, "Spreadsheet rows to append."),
                    field("slides", ToolValueType::Array, "Presentation slides to append."),
                ],
                &["app", "path", "body", "rows", "slides"],
            ),
            output_schema: object_schema(
                vec![
                    field("path", ToolValueType::String, "Verified updated artifact path."),
                    field("bytes", ToolValueType::Number, "Binary artifact bytes after update."),
                    field("app", ToolValueType::String, "Office application kind."),
                    field(
                        "artifact_kind",
                        ToolValueType::String,
                        "Verified Office artifact kind.",
                    ),
                    field("summary", ToolValueType::String, "Verified update summary."),
                ],
                &["path", "bytes", "app", "artifact_kind", "summary"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::Workspace,
                mutates_machine_state: true,
                protected_path_policy:
                    "deny-first workspace and managed desktop update sandbox; protected paths always win"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "local_filesystem://mutation".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 30 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "office.update.binary_artifact.v1".to_string(),
                description:
                    "Require matching path and app metadata, positive binary size, update summary, and Office update evidence."
                        .to_string(),
                required_evidence_kinds: vec!["office_artifact_update".to_string()],
            },
            recovery_hint:
                "Choose an existing unprotected managed artifact, review local permission, and retry one update."
                    .to_string(),
        },
        ToolContract {
            id: T1_RECONCILIATION_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Reconcile verified T1 sources into Excel".to_string(),
            description:
                "Scan one authorized workspace folder for the exact T1 XLSX, DOCX, and PDF source set; reject period, numeric, formula, path, or identity conflicts; and create one re-read formula-backed Excel reconciliation artifact."
                    .to_string(),
            capability: CapabilityKind::FileWrite,
            risk_level: RiskLevel::High,
            executor_id: "kernel.operations.reconcile_excel.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field(
                        "source_directory",
                        ToolValueType::String,
                        "Workspace-relative directory containing exactly one XLSX, DOCX, and PDF source.",
                    ),
                    field(
                        "output_relative_path",
                        ToolValueType::String,
                        "Workspace-relative new XLSX output path inside an existing authorized directory.",
                    ),
                ],
                &["source_directory", "output_relative_path"],
            ),
            output_schema: object_schema(
                vec![
                    field(
                        "source_manifest",
                        ToolValueType::Object,
                        "Exact source paths, byte counts, media types, and SHA-256 identities.",
                    ),
                    field(
                        "provenance",
                        ToolValueType::Object,
                        "Source and derived fact provenance with independent reconciliation.",
                    ),
                    field(
                        "artifact",
                        ToolValueType::Object,
                        "Verified XLSX artifact identity receipt.",
                    ),
                    field(
                        "key_figures",
                        ToolValueType::Object,
                        "Reconciled key-number projection.",
                    ),
                    field(
                        "completion_evidence",
                        ToolValueType::Array,
                        "Kernel-issued evidence created only after persisted re-read verification.",
                    ),
                ],
                &[
                    "source_manifest",
                    "provenance",
                    "artifact",
                    "key_figures",
                    "completion_evidence",
                ],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::Workspace,
                mutates_machine_state: true,
                protected_path_policy:
                    "exact workspace-relative source directory and new XLSX output; no overwrite, link traversal, protected path, or boundary escape"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "local_filesystem://mutation".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 30 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "operations.reconcile_excel.t1.v1".to_string(),
                description:
                    "Require exact source identity, per-fact provenance, independent numeric reconciliation, formula verification, persisted artifact identity, and post-write re-read evidence."
                        .to_string(),
                required_evidence_kinds: vec![
                    "t1_source_manifest".to_string(),
                    "t1_fact_provenance".to_string(),
                    "t1_reconciliation_xlsx".to_string(),
                ],
            },
            recovery_hint:
                "Restore the exact three-source T1 set or choose a new authorized XLSX path, then retry one reconciliation without overwriting an existing artifact."
                    .to_string(),
        },
        ToolContract {
            id: T1_POWERPOINT_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Generate and locally verify the T1 PowerPoint brief".to_string(),
            description:
                "Re-read the exact completed T1 reconciliation receipt, create one new workspace-relative PPTX, render it through local Microsoft Office, and permit at most three non-overwriting sibling revisions before Kernel completion."
                    .to_string(),
            capability: CapabilityKind::FileWrite,
            risk_level: RiskLevel::High,
            executor_id: "kernel.operations.generate_powerpoint.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field(
                        "source_directory",
                        ToolValueType::String,
                        "Workspace-relative directory containing the exact C4A source set.",
                    ),
                    field(
                        "reconciliation",
                        ToolValueType::Object,
                        "Complete Kernel-issued C4A reconciliation outcome and evidence receipts.",
                    ),
                    field(
                        "output_relative_path",
                        ToolValueType::String,
                        "Workspace-relative new PPTX output path inside an existing authorized directory.",
                    ),
                ],
                &["source_directory", "reconciliation", "output_relative_path"],
            ),
            output_schema: object_schema(
                vec![
                    field(
                        "reconciliation_artifact_sha256",
                        ToolValueType::String,
                        "Exact reverified C4A reconciliation artifact identity.",
                    ),
                    field(
                        "artifact",
                        ToolValueType::Object,
                        "Persisted PPTX artifact identity and delivered revision receipt.",
                    ),
                    field(
                        "render",
                        ToolValueType::Object,
                        "Actual local Office render and preview identity receipt.",
                    ),
                    field(
                        "revision",
                        ToolValueType::Object,
                        "Bounded sibling-only revision history.",
                    ),
                    field(
                        "key_figures",
                        ToolValueType::Object,
                        "Reverified C4A key-number projection used in the slide.",
                    ),
                    field(
                        "anomalies",
                        ToolValueType::Object,
                        "Reverified operational anomaly projection used in the slide.",
                    ),
                    field(
                        "completion_evidence",
                        ToolValueType::Array,
                        "Kernel-issued evidence created only after persisted PPTX and actual-render verification.",
                    ),
                ],
                &[
                    "reconciliation_artifact_sha256",
                    "artifact",
                    "render",
                    "revision",
                    "key_figures",
                    "anomalies",
                    "completion_evidence",
                ],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::Workspace,
                mutates_machine_state: true,
                protected_path_policy:
                    "exact C4A receipt and new workspace-relative PPTX; no overwrite, link traversal, protected path, boundary escape, external relationship, macro, or non-sibling revision"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "local_filesystem://mutation".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 30 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "operations.generate_powerpoint.t1.v1".to_string(),
                description:
                    "Require exact C4A source and XLSX identity, a deterministic one-page PPTX, persisted artifact identity, actual local Office render evidence, and bounded sibling-only revision evidence."
                        .to_string(),
                required_evidence_kinds: vec![
                    "one_page_pptx".to_string(),
                    "actual_render_receipt".to_string(),
                    "office_revision_receipt".to_string(),
                ],
            },
            recovery_hint:
                "Restore the exact C4A sources and XLSX, choose a new authorized PPTX path, or restore local Office rendering, then retry without overwriting any artifact."
                    .to_string(),
        },
        ToolContract {
            id: OPERATIONS_BRIEFING_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Build a verified operations briefing".to_string(),
            description:
                "Read one bounded evidence folder and produce a persisted, traceable Operations Briefing draft."
                    .to_string(),
            capability: CapabilityKind::FileRead,
            risk_level: RiskLevel::Low,
            executor_id: "builtin.operations.briefing.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field(
                        "evidence_folder_path",
                        ToolValueType::String,
                        "Local or workspace-relative evidence folder.",
                    ),
                    field("summary", ToolValueType::String, "Human-readable workflow purpose."),
                ],
                &["evidence_folder_path", "summary"],
            ),
            output_schema: object_schema(
                vec![
                    field("workflow_run_id", ToolValueType::String, "Persisted workflow run ID."),
                    field("status", ToolValueType::String, "Verified workflow status."),
                    field(
                        "evidence_folder_path",
                        ToolValueType::String,
                        "Verified evidence folder reference.",
                    ),
                    field("summary", ToolValueType::String, "Generated briefing summary."),
                    field("run", ToolValueType::Object, "Complete Operations Briefing run record."),
                ],
                &["workflow_run_id", "status", "evidence_folder_path", "summary", "run"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::LocalFilesystem,
                mutates_machine_state: false,
                protected_path_policy:
                    "deny-first bounded evidence-folder read; protected paths and secret files always win"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "local_filesystem://mutation".to_string(),
                    access: ToolResourceAccess::Read,
                    lease_seconds: 30 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "operations.briefing.draft.v1".to_string(),
                description:
                    "Require a draft-ready workflow run, evidence-folder identity, context receipt, and persisted workflow evidence."
                        .to_string(),
                required_evidence_kinds: vec!["operations_briefing_draft".to_string()],
            },
            recovery_hint:
                "Choose an unprotected bounded evidence folder, review FileRead permission, and retry one briefing run."
                    .to_string(),
        },
        ToolContract {
            id: FILESYSTEM_MUTATE_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Mutate a local filesystem path".to_string(),
            description: "Create, update, rename, or delete a local file or directory through the deny-first sandbox."
                .to_string(),
            capability: CapabilityKind::FileWrite,
            risk_level: RiskLevel::High,
            executor_id: "builtin.filesystem.mutate.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field(
                        "operation",
                        ToolValueType::String,
                        "One supported file or directory mutation operation.",
                    ),
                    field("path", ToolValueType::String, "Absolute source or target path."),
                    nullable_field(
                        "destination",
                        ToolValueType::String,
                        "Absolute rename destination when required.",
                    ),
                    nullable_field(
                        "content",
                        ToolValueType::String,
                        "Exact UTF-8 content for create_file or update_file.",
                    ),
                    field(
                        "summary",
                        ToolValueType::String,
                        "Human-readable purpose for the mutation.",
                    ),
                ],
                &["operation", "path", "summary"],
            ),
            output_schema: object_schema(
                vec![
                    field("operation", ToolValueType::String, "Completed operation."),
                    field("path", ToolValueType::String, "Verified source or target path."),
                    nullable_field(
                        "destination",
                        ToolValueType::String,
                        "Verified rename destination when applicable.",
                    ),
                    field("bytes", ToolValueType::Number, "Content bytes affected."),
                    field("summary", ToolValueType::String, "Executor result summary."),
                ],
                &["operation", "path", "bytes", "summary"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::LocalFilesystem,
                mutates_machine_state: true,
                protected_path_policy:
                    "deny-first local filesystem sandbox; protected paths and secret files always win"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "local_filesystem://mutation".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 30 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "filesystem.mutate.state.v1".to_string(),
                description: "Verify the requested postcondition on disk and preserve filesystem-state evidence."
                    .to_string(),
                required_evidence_kinds: vec!["filesystem_state".to_string()],
            },
            recovery_hint:
                "Choose an unprotected local path, review the exact operation and destination, then retry the smallest mutation."
                    .to_string(),
        },
        ToolContract {
            id: TERMINAL_READ_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Run an allowlisted read-only terminal inspection".to_string(),
            description: "Execute one deterministic read-only inspection or sandboxed local directory listing."
                .to_string(),
            capability: CapabilityKind::TerminalRead,
            risk_level: RiskLevel::Low,
            executor_id: "builtin.terminal.read.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field(
                        "command",
                        ToolValueType::String,
                        "Allowlisted command or DS Agent directory-listing request.",
                    ),
                    field(
                        "summary",
                        ToolValueType::String,
                        "Human-readable purpose for the inspection.",
                    ),
                ],
                &["command", "summary"],
            ),
            output_schema: object_schema(
                vec![
                    field("command", ToolValueType::String, "Normalized command."),
                    field("stdout", ToolValueType::String, "Bounded standard output."),
                    field("stderr", ToolValueType::String, "Bounded standard error."),
                    field("exit_code", ToolValueType::Number, "Process exit code."),
                ],
                &["command", "stdout", "stderr", "exit_code"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::LocalFilesystem,
                mutates_machine_state: false,
                protected_path_policy:
                    "strict read-only command allowlist; directory listings use the deny-first local path sandbox"
                        .to_string(),
                resource: None,
            },
            verification: ToolVerificationContract {
                recipe_id: "terminal.read.exit_output.v1".to_string(),
                description:
                    "Require the normalized command, a zero exit code, bounded output, and terminal-output evidence."
                        .to_string(),
                required_evidence_kinds: vec!["terminal_output".to_string()],
            },
            recovery_hint:
                "Use one allowlisted inspection or choose an unprotected directory, then retry without shell composition."
                    .to_string(),
        },
        ToolContract {
            id: BROWSER_OPEN_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Open a website in the local browser".to_string(),
            description:
                "Launch one validated HTTP(S) URL in Chrome when explicitly requested or in the system default browser."
                    .to_string(),
            capability: CapabilityKind::BrowserBrowse,
            risk_level: RiskLevel::Medium,
            executor_id: "builtin.browser.open.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("url", ToolValueType::String, "Validated HTTP(S) URL."),
                    field(
                        "preferred_browser",
                        ToolValueType::String,
                        "default or chrome.",
                    ),
                    field("summary", ToolValueType::String, "Human-readable launch purpose."),
                ],
                &["url", "preferred_browser", "summary"],
            ),
            output_schema: object_schema(
                vec![
                    field("url", ToolValueType::String, "Opened HTTP(S) URL."),
                    field(
                        "browser_label",
                        ToolValueType::String,
                        "Verified local browser label.",
                    ),
                    field(
                        "fallback_note",
                        ToolValueType::String,
                        "Fallback note, blank when no fallback was needed.",
                    ),
                ],
                &["url", "browser_label", "fallback_note"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::None,
                mutates_machine_state: true,
                protected_path_policy:
                    "allow only HTTP(S) browser launches; local foreground launch is serialized"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "computer://foreground_desktop".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 2 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "browser.open.launch_receipt.v1".to_string(),
                description:
                    "Require URL identity, a non-empty local browser label, and a launch receipt."
                        .to_string(),
                required_evidence_kinds: vec!["browser_open_receipt".to_string()],
            },
            recovery_hint:
                "Close conflicting foreground automation, verify the browser installation, and retry one HTTP(S) launch."
                    .to_string(),
        },
        ToolContract {
            id: BROWSER_BROWSE_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Read a public web page".to_string(),
            description:
                "Fetch one public HTTP(S) page through the deny-first network sandbox."
                    .to_string(),
            capability: CapabilityKind::BrowserBrowse,
            risk_level: RiskLevel::Low,
            executor_id: "builtin.browser.browse.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("url", ToolValueType::String, "Public HTTP(S) page URL."),
                    field(
                        "summary",
                        ToolValueType::String,
                        "Human-readable purpose for reading the page.",
                    ),
                ],
                &["url", "summary"],
            ),
            output_schema: object_schema(
                vec![
                    field("requested_url", ToolValueType::String, "Requested public page URL."),
                    field("final_url", ToolValueType::String, "Verified final public page URL."),
                    field("title", ToolValueType::String, "Page title."),
                    field("text", ToolValueType::String, "Bounded page text."),
                ],
                &["requested_url", "final_url", "title", "text"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: vec!["public_http".to_string()],
                path_scope: ToolPathScope::None,
                mutates_machine_state: false,
                protected_path_policy:
                    "deny loopback, private, link-local, metadata, credential-bearing, and non-HTTP(S) targets before and after redirects"
                        .to_string(),
                resource: None,
            },
            verification: ToolVerificationContract {
                recipe_id: "browser.browse.public_page.v1".to_string(),
                description:
                    "Require a public final URL, bounded non-empty page text, and browser-page evidence."
                        .to_string(),
                required_evidence_kinds: vec!["browser_page".to_string()],
            },
            recovery_hint:
                "Choose a public HTTP(S) page without embedded credentials and retry the smallest read."
                    .to_string(),
        },
        ToolContract {
            id: NETWORK_SEARCH_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Search the public web with source links".to_string(),
            description:
                "Run a bounded source-backed public-web search through the network sandbox."
                    .to_string(),
            capability: CapabilityKind::NetworkSearch,
            risk_level: RiskLevel::Low,
            executor_id: "builtin.network.search.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("query", ToolValueType::String, "Public-web search query."),
                    field("scope", ToolValueType::String, "Search scope."),
                    field(
                        "summary",
                        ToolValueType::String,
                        "Human-readable purpose for the search.",
                    ),
                ],
                &["query", "scope", "summary"],
            ),
            output_schema: object_schema(
                vec![
                    field("provider", ToolValueType::String, "Source adapter label."),
                    field("query", ToolValueType::String, "Normalized query."),
                    field("scope", ToolValueType::String, "Normalized scope."),
                    field("search_url", ToolValueType::String, "Verified public search URL."),
                    field("items", ToolValueType::Array, "Verified public source links."),
                ],
                &["provider", "query", "scope", "search_url", "items"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: vec![
                    "duckduckgo.com".to_string(),
                    "public_result_urls".to_string(),
                ],
                path_scope: ToolPathScope::None,
                mutates_machine_state: false,
                protected_path_policy:
                    "fixed public search adapter; source URLs must pass the deny-first public network policy"
                        .to_string(),
                resource: None,
            },
            verification: ToolVerificationContract {
                recipe_id: "network.search.source_links.v1".to_string(),
                description:
                    "Require normalized query metadata and at least one verified public source link."
                        .to_string(),
                required_evidence_kinds: vec!["source_links".to_string()],
            },
            recovery_hint:
                "Refine the public-web query or scope and retry; never accept private or unverifiable source URLs."
                    .to_string(),
        },
        ToolContract {
            id: COMPUTER_SCREENSHOT_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Capture the visible desktop as evidence".to_string(),
            description:
                "Inspect the current primary display and persist bounded PNG evidence through the selected local or loopback bridge route."
                    .to_string(),
            capability: CapabilityKind::ComputerScreenshot,
            risk_level: RiskLevel::Medium,
            executor_id: "builtin.computer.screenshot.v1".to_string(),
            input_schema: object_schema(
                vec![field(
                    "summary",
                    ToolValueType::String,
                    "Human-readable purpose for inspecting the desktop.",
                )],
                &["summary"],
            ),
            output_schema: object_schema(
                vec![
                    field("display_label", ToolValueType::String, "Captured display label."),
                    field(
                        "evidence_ref",
                        ToolValueType::String,
                        "Durable local screenshot evidence reference.",
                    ),
                    field("width", ToolValueType::Number, "Captured pixel width."),
                    field("height", ToolValueType::Number, "Captured pixel height."),
                    field("captured_at", ToolValueType::String, "Capture timestamp."),
                ],
                &["display_label", "evidence_ref", "width", "height", "captured_at"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: vec!["configured_loopback_bridge".to_string()],
                path_scope: ToolPathScope::AppEvidenceDirectory,
                mutates_machine_state: false,
                protected_path_policy:
                    "screen pixels remain approval-gated; PNG evidence is written only through the configured evidence directory"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "computer://foreground_desktop".to_string(),
                    access: ToolResourceAccess::Read,
                    lease_seconds: 5 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "computer.screenshot.image_evidence.v1".to_string(),
                description:
                    "Require non-empty display metadata, positive dimensions, a capture timestamp, and durable screenshot evidence."
                        .to_string(),
                required_evidence_kinds: vec!["screenshot_image".to_string()],
            },
            recovery_hint:
                "Verify screen-capture permission or the local bridge, then retry one screenshot before planning further controls."
                    .to_string(),
        },
        ToolContract {
            id: COMPUTER_CONTROL_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Execute one structured desktop input action".to_string(),
            description:
                "Execute exactly one validated mouse or keyboard action after one-shot approval and local unlock."
                    .to_string(),
            capability: CapabilityKind::ComputerControl,
            risk_level: RiskLevel::Critical,
            executor_id: "builtin.computer.control.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("target", ToolValueType::String, "Target app or window."),
                    field(
                        "action",
                        ToolValueType::String,
                        "One structured click, move, type, press, hotkey, or scroll action.",
                    ),
                    field(
                        "summary",
                        ToolValueType::String,
                        "Human-readable purpose for the input action.",
                    ),
                ],
                &["target", "action", "summary"],
            ),
            output_schema: object_schema(
                vec![
                    field("target", ToolValueType::String, "Validated target app or window."),
                    field("action", ToolValueType::String, "Validated structured action."),
                    field(
                        "summary",
                        ToolValueType::String,
                        "Local executor acknowledgement; not a claim that the wider task is complete.",
                    ),
                ],
                &["target", "action", "summary"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: vec!["configured_loopback_bridge".to_string()],
                path_scope: ToolPathScope::None,
                mutates_machine_state: true,
                protected_path_policy:
                    "critical one-shot approval and a short-lived local unlock are mandatory; one structured input action per invocation"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "computer://foreground_desktop".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 2 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "computer.control.execution_receipt.v1".to_string(),
                description:
                    "Verify the structured action identity and executor acknowledgement; use a subsequent screenshot to verify visible task state."
                        .to_string(),
                required_evidence_kinds: vec!["computer_control_receipt".to_string()],
            },
            recovery_hint:
                "Unlock Computer Use locally, approve this exact action, retry once, then capture a screenshot to verify visible state."
                    .to_string(),
        },
        ToolContract {
            id: APP_UPDATE_CHECK_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Check for DS Agent updates".to_string(),
            description: "Compare the installed version with trusted GitHub release metadata."
                .to_string(),
            capability: CapabilityKind::AppUpdateCheck,
            risk_level: RiskLevel::Low,
            executor_id: "builtin.app_update.check.v1".to_string(),
            input_schema: empty_object_schema(),
            output_schema: object_schema(
                vec![
                    field("current_version", ToolValueType::String, "Installed version."),
                    nullable_field(
                        "latest_version",
                        ToolValueType::String,
                        "Latest release version.",
                    ),
                    field("update_available", ToolValueType::Boolean, "Whether an update is available."),
                    nullable_field("asset_name", ToolValueType::String, "Trusted installer asset name."),
                    nullable_field("release_url", ToolValueType::String, "Release page URL."),
                    nullable_field("message", ToolValueType::String, "Optional update status message."),
                ],
                &["current_version", "update_available"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: vec!["api.github.com".to_string()],
                path_scope: ToolPathScope::None,
                mutates_machine_state: false,
                protected_path_policy: "read trusted release metadata only".to_string(),
                resource: None,
            },
            verification: ToolVerificationContract {
                recipe_id: "app_update.release_status.v1".to_string(),
                description: "Require parsed release metadata and a non-empty current version."
                    .to_string(),
                required_evidence_kinds: vec!["release_status".to_string()],
            },
            recovery_hint: "Retry after checking network access and the trusted release source."
                .to_string(),
        },
        ToolContract {
            id: APP_UPDATE_DOWNLOAD_TOOL_ID.to_string(),
            version: "2.0.0".to_string(),
            title: "Download a DS Agent update".to_string(),
            description:
                "Stream a trusted Windows installer into receipt-bound managed storage."
                    .to_string(),
            capability: CapabilityKind::AppUpdateDownload,
            risk_level: RiskLevel::High,
            executor_id: "builtin.app_update.download.v2".to_string(),
            input_schema: empty_object_schema(),
            output_schema: object_schema(
                vec![
                    field("latest_version", ToolValueType::String, "Downloaded version."),
                    field("asset_name", ToolValueType::String, "Trusted release asset name."),
                    field("download_receipt", ToolValueType::String, "Opaque one-shot download receipt."),
                    field("sha256", ToolValueType::String, "Streamed installer SHA-256."),
                    field("byte_size", ToolValueType::Number, "Bounded streamed byte count."),
                ],
                &["latest_version", "asset_name", "download_receipt", "sha256", "byte_size"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: vec![
                    "api.github.com".to_string(),
                    "github.com".to_string(),
                    "release-assets.githubusercontent.com".to_string(),
                ],
                path_scope: ToolPathScope::AppUpdateDirectory,
                mutates_machine_state: false,
                protected_path_policy: "write only inside the isolated app update directory"
                    .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "app_update://installer".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 30 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "app_update.downloaded_installer.v2".to_string(),
                description:
                    "Require bounded streaming, managed atomic landing, hash and file identity."
                        .to_string(),
                required_evidence_kinds: vec!["downloaded_installer".to_string()],
            },
            recovery_hint: "Retry the download; do not install assets that fail source or path validation."
                .to_string(),
        },
        ToolContract {
            id: APP_UPDATE_INSTALL_TOOL_ID.to_string(),
            version: "2.0.0".to_string(),
            title: "Install a DS Agent update".to_string(),
            description: "Schedule a verified installer and restart DS Agent after approval."
                .to_string(),
            capability: CapabilityKind::AppUpdateInstall,
            risk_level: RiskLevel::Critical,
            executor_id: "builtin.app_update.install.v2".to_string(),
            input_schema: object_schema(
                vec![field(
                    "download_receipt",
                    ToolValueType::String,
                    "Opaque one-shot receipt returned by app_update.download.",
                )],
                &["download_receipt"],
            ),
            output_schema: object_schema(
                vec![field("restart_scheduled", ToolValueType::Boolean, "Whether restart is scheduled.")],
                &["restart_scheduled"],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::AppUpdateDirectory,
                mutates_machine_state: true,
                protected_path_policy:
                    "consume one exact receipt and revalidate managed identity, size and hash"
                        .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "app_update://installer".to_string(),
                    access: ToolResourceAccess::Write,
                    lease_seconds: 30 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "app_update.install_schedule.v1".to_string(),
                description: "Require a verified installer path and a successfully scheduled restart."
                    .to_string(),
                required_evidence_kinds: vec!["install_schedule".to_string()],
            },
            recovery_hint: "If scheduling fails, keep the verified installer and retry after reviewing the audit error."
                .to_string(),
        },
        ToolContract {
            id: SKILL_ACTIVATE_TOOL_ID.to_string(),
            version: "1.0.0".to_string(),
            title: "Activate an installed declarative skill".to_string(),
            description: "Load a trusted, enabled, hash-verified declarative skill entry as evidence for the bounded agent loop."
                .to_string(),
            capability: CapabilityKind::SkillUse,
            risk_level: RiskLevel::Low,
            executor_id: "builtin.skill.activate.v1".to_string(),
            input_schema: object_schema(
                vec![
                    field("skill_id", ToolValueType::String, "Installed skill identifier."),
                    field(
                        "input_summary",
                        ToolValueType::String,
                        "User task context to apply to the skill.",
                    ),
                ],
                &["skill_id", "input_summary"],
            ),
            output_schema: object_schema(
                vec![
                    field("skill_id", ToolValueType::String, "Activated skill identifier."),
                    field("skill_name", ToolValueType::String, "Activated skill name."),
                    field("skill_version", ToolValueType::String, "Activated skill version."),
                    field("entry_kind", ToolValueType::String, "Declarative entry kind."),
                    field("entry_path", ToolValueType::String, "Package entry path."),
                    field("entry_sha256", ToolValueType::String, "Verified entry SHA-256."),
                    field("input_summary", ToolValueType::String, "Bound user task context."),
                    field("instructions", ToolValueType::String, "Verified declarative entry content."),
                    field("capability_summary", ToolValueType::String, "Declared capability summary."),
                    field("permission_summary", ToolValueType::String, "Declared permission summary."),
                ],
                &[
                    "skill_id",
                    "skill_name",
                    "skill_version",
                    "entry_kind",
                    "entry_path",
                    "entry_sha256",
                    "input_summary",
                    "instructions",
                    "capability_summary",
                    "permission_summary",
                ],
            ),
            constraints: ToolConstraints {
                allowed_network_hosts: Vec::new(),
                path_scope: ToolPathScope::InstalledSkillStore,
                mutates_machine_state: false,
                protected_path_policy: "read only the installed declarative entry retained by the audited skill store"
                    .to_string(),
                resource: Some(ToolResourceRequirement {
                    key: "skill://installed_catalog".to_string(),
                    access: ToolResourceAccess::Read,
                    lease_seconds: 5 * 60,
                }),
            },
            verification: ToolVerificationContract {
                recipe_id: "skill.activate.entry_sha256.v1".to_string(),
                description: "Require enabled trust state, retained entry content, and a matching SHA-256 before returning skill context."
                    .to_string(),
                required_evidence_kinds: vec!["skill_context".to_string()],
            },
            recovery_hint: "Install the declarative ZIP package again, review trust and enablement, then retry activation."
                .to_string(),
        },
    ]
}

pub fn prepare_tool_execution(request: &ToolExecutionRequest) -> Result<ToolExecutionPlan, String> {
    let tool_id = request.tool_id.trim();
    let contract = builtin_tool_catalog()
        .into_iter()
        .find(|contract| contract.id == tool_id)
        .ok_or_else(|| format!("unknown or disabled tool `{tool_id}`"))?;
    validate_tool_input(&contract, &request.input)?;
    validate_tool_semantics(&contract, &request.input)?;

    Ok(ToolExecutionPlan {
        invocation_id: Uuid::new_v4(),
        request: request.clone(),
        policy_decision: decide(request.access_mode, contract.capability),
        contract,
        prepared_at: Utc::now(),
    })
}

pub fn validate_tool_input(contract: &ToolContract, input: &Value) -> Result<(), String> {
    validate_object_schema(&contract.input_schema, input, "tool input")
}

fn validate_tool_semantics(contract: &ToolContract, input: &Value) -> Result<(), String> {
    if contract.id == CONNECTOR_MUTATE_TOOL_ID && input["account_generation"].as_u64().is_none() {
        return Err(
            "connector.mutate input field `account_generation` must be an unsigned integer"
                .to_string(),
        );
    }
    if contract.id == CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID {
        for field in [
            "provider_id",
            "account_id",
            "parent_remote_ref",
            "attachment_remote_ref",
            "file_name",
            "media_type",
            "workspace_identity",
        ] {
            let value = input[field].as_str().map(str::trim).unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "connector.attachment.download input field `{field}` cannot be blank"
                ));
            }
        }
        let provider_id = input["provider_id"].as_str().unwrap_or_default();
        let account_id = input["account_id"].as_str().unwrap_or_default();
        let parent_ref = input["parent_remote_ref"].as_str().unwrap_or_default();
        let attachment_ref = input["attachment_remote_ref"].as_str().unwrap_or_default();
        let file_name = input["file_name"].as_str().unwrap_or_default().trim();
        let media_type = input["media_type"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let workspace_identity = input["workspace_identity"].as_str().unwrap_or_default();
        let workspace_identity_hash = workspace_identity
            .strip_prefix("v2:")
            .unwrap_or(workspace_identity);
        let size_bytes = input["size_bytes"].as_u64().unwrap_or_default();
        if provider_id.len() > 64
            || Uuid::parse_str(account_id).is_err()
            || parent_ref.len() > 1024
            || attachment_ref.len() > 1024
            || file_name.len() > 255
            || file_name == "."
            || file_name == ".."
            || file_name.contains(['/', '\\', ':'])
            || file_name.chars().any(char::is_control)
            || size_bytes == 0
            || size_bytes > 20 * 1024 * 1024
            || input["account_generation"].as_u64().is_none()
            || workspace_identity_hash.len() != 64
            || !workspace_identity_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("connector.attachment.download input is unsafe".to_string());
        }
        let extension = std::path::Path::new(file_name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let supported = matches!(
            (extension.as_str(), media_type.as_str()),
            ("pdf", "application/pdf")
                | ("png", "image/png")
                | ("jpg" | "jpeg", "image/jpeg")
                | ("txt", "text/plain")
                | ("csv", "text/csv")
                | ("json", "application/json")
                | (
                    "docx",
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                )
                | (
                    "xlsx",
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                )
                | (
                    "pptx",
                    "application/vnd.openxmlformats-officedocument.presentationml.presentation"
                )
        );
        if !supported {
            return Err("connector.attachment.download file type is not supported".to_string());
        }
    }
    if contract.id == FILE_WRITE_TOOL_ID {
        for field in ["path", "summary", "content"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!("file.write input field `{field}` cannot be blank"));
            }
        }
    }
    if contract.id == OFFICE_CREATE_TOOL_ID {
        for field in ["app", "path", "title"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "office.create input field `{field}` cannot be blank"
                ));
            }
        }
        let app = input["app"].as_str().unwrap_or_default();
        if !matches!(app, "word" | "excel" | "power_point") {
            return Err("office.create app must be word, excel, or power_point".to_string());
        }
    }
    if contract.id == OFFICE_UPDATE_TOOL_ID {
        for field in ["app", "path"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "office.update input field `{field}` cannot be blank"
                ));
            }
        }
        let app = input["app"].as_str().unwrap_or_default();
        if !matches!(app, "word" | "excel" | "power_point") {
            return Err("office.update app must be word, excel, or power_point".to_string());
        }
        let body_is_blank = input["body"]
            .as_str()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty();
        let rows_are_empty = input["rows"].as_array().map(Vec::is_empty).unwrap_or(true);
        let slides_are_empty = input["slides"]
            .as_array()
            .map(Vec::is_empty)
            .unwrap_or(true);
        if body_is_blank && rows_are_empty && slides_are_empty {
            return Err("office.update requires body, rows, or slides content".to_string());
        }
    }
    if contract.id == OFFICE_OPEN_TOOL_ID {
        for field in ["path", "preferred_app"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!("office.open input field `{field}` cannot be blank"));
            }
        }
        let app = input["preferred_app"].as_str().unwrap_or_default();
        if !matches!(app, "word" | "excel" | "power_point") {
            return Err(
                "office.open preferred_app must be word, excel, or power_point".to_string(),
            );
        }
    }
    if contract.id == OPERATIONS_BRIEFING_TOOL_ID {
        for field in ["evidence_folder_path", "summary"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "operations.briefing input field `{field}` cannot be blank"
                ));
            }
        }
    }
    if contract.id == T1_RECONCILIATION_TOOL_ID {
        for field in ["source_directory", "output_relative_path"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "operations.reconcile_excel input field `{field}` cannot be blank"
                ));
            }
        }
        if !input["output_relative_path"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .ends_with(".xlsx")
        {
            return Err(
                "operations.reconcile_excel output_relative_path must end in .xlsx".to_string(),
            );
        }
    }
    if contract.id == T1_POWERPOINT_TOOL_ID {
        for field in ["source_directory", "output_relative_path"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "operations.generate_powerpoint input field `{field}` cannot be blank"
                ));
            }
        }
        if !input.get("reconciliation").is_some_and(Value::is_object) {
            return Err(
                "operations.generate_powerpoint reconciliation must be an object".to_string(),
            );
        }
        if !input["output_relative_path"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .ends_with(".pptx")
        {
            return Err(
                "operations.generate_powerpoint output_relative_path must end in .pptx".to_string(),
            );
        }
    }
    if contract.id == FILESYSTEM_MUTATE_TOOL_ID {
        validate_filesystem_mutation_semantics(input)?;
    }
    if contract.id == FILE_READ_TOOL_ID {
        for field in ["path", "summary"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!("file.read input field `{field}` cannot be blank"));
            }
        }
    }
    if contract.id == TERMINAL_READ_TOOL_ID {
        for field in ["command", "summary"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "terminal.read input field `{field}` cannot be blank"
                ));
            }
        }
    }
    if contract.id == BROWSER_BROWSE_TOOL_ID {
        for field in ["url", "summary"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "browser.browse input field `{field}` cannot be blank"
                ));
            }
        }
    }
    if contract.id == BROWSER_OPEN_TOOL_ID {
        for field in ["url", "preferred_browser", "summary"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "browser.open input field `{field}` cannot be blank"
                ));
            }
        }
        let url = input["url"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err("browser.open url must use http or https".to_string());
        }
        let preferred_browser = input["preferred_browser"].as_str().unwrap_or_default();
        if !matches!(preferred_browser, "default" | "chrome") {
            return Err("browser.open preferred_browser must be default or chrome".to_string());
        }
    }
    if contract.id == NETWORK_SEARCH_TOOL_ID {
        for field in ["query", "scope", "summary"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "network.search input field `{field}` cannot be blank"
                ));
            }
        }
    }
    if contract.id == COMPUTER_SCREENSHOT_TOOL_ID {
        let summary = input
            .get("summary")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if summary.is_empty() {
            return Err("computer.screenshot input field `summary` cannot be blank".to_string());
        }
    }
    if contract.id == COMPUTER_CONTROL_TOOL_ID {
        for field in ["target", "action", "summary"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "computer.control input field `{field}` cannot be blank"
                ));
            }
        }
    }
    if contract.id == SKILL_ACTIVATE_TOOL_ID {
        for field in ["skill_id", "input_summary"] {
            let value = input
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default();
            if value.is_empty() {
                return Err(format!(
                    "skill.activate input field `{field}` cannot be blank"
                ));
            }
        }
        let skill_id = input["skill_id"]
            .as_str()
            .ok_or_else(|| "skill.activate requires a string skill_id".to_string())?;
        Uuid::parse_str(skill_id.trim())
            .map_err(|_| "skill.activate input field `skill_id` must be a UUID".to_string())?;
    }
    Ok(())
}

fn validate_filesystem_mutation_semantics(input: &Value) -> Result<(), String> {
    let string = |field: &str| {
        input
            .get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
    };
    let operation = string("operation");
    let path = string("path");
    let summary = string("summary");
    let has_destination = !string("destination").is_empty();
    let has_content = !string("content").is_empty();
    if path.is_empty() {
        return Err("filesystem.mutate input field `path` cannot be blank".to_string());
    }
    if summary.is_empty() {
        return Err("filesystem.mutate input field `summary` cannot be blank".to_string());
    }

    match operation {
        "create_file" | "update_file" => {
            if !has_content {
                return Err(format!(
                    "filesystem.mutate operation `{operation}` requires non-blank content"
                ));
            }
            if has_destination {
                return Err(format!(
                    "filesystem.mutate operation `{operation}` does not accept a destination"
                ));
            }
        }
        "rename_file" | "rename_directory" => {
            if !has_destination {
                return Err(format!(
                    "filesystem.mutate operation `{operation}` requires a destination"
                ));
            }
            if has_content {
                return Err(format!(
                    "filesystem.mutate operation `{operation}` does not accept content"
                ));
            }
        }
        "delete_file" | "create_directory" | "delete_directory" => {
            if has_destination || has_content {
                return Err(format!(
                    "filesystem.mutate operation `{operation}` accepts only path and summary"
                ));
            }
        }
        _ => {
            return Err(format!(
                "filesystem.mutate operation `{operation}` is unsupported"
            ));
        }
    }
    Ok(())
}

fn validate_object_schema(
    schema: &ToolObjectSchema,
    value: &Value,
    label: &str,
) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} must be a JSON object"))?;
    for required in &schema.required {
        if !object.contains_key(required) {
            return Err(format!("{label} is missing required field `{required}`"));
        }
    }
    for (name, field_value) in object {
        let Some(field) = schema.properties.iter().find(|field| field.name == *name) else {
            if schema.allow_additional {
                continue;
            }
            return Err(format!("{label} contains unsupported field `{name}`"));
        };
        if !value_matches_type(field_value, field.value_type) {
            if field.nullable && field_value.is_null() {
                continue;
            }
            return Err(format!(
                "{label} field `{name}` must be {}",
                value_type_name(field.value_type)
            ));
        }
    }
    Ok(())
}

fn value_matches_type(value: &Value, value_type: ToolValueType) -> bool {
    match value_type {
        ToolValueType::String => value.is_string(),
        ToolValueType::Boolean => value.is_boolean(),
        ToolValueType::Number => value.is_number(),
        ToolValueType::Object => value.is_object(),
        ToolValueType::Array => value.is_array(),
    }
}

fn value_type_name(value_type: ToolValueType) -> &'static str {
    match value_type {
        ToolValueType::String => "a string",
        ToolValueType::Boolean => "a boolean",
        ToolValueType::Number => "a number",
        ToolValueType::Object => "an object",
        ToolValueType::Array => "an array",
    }
}

fn empty_object_schema() -> ToolObjectSchema {
    object_schema(Vec::new(), &[])
}

fn object_schema(properties: Vec<ToolFieldSchema>, required: &[&str]) -> ToolObjectSchema {
    ToolObjectSchema {
        properties,
        required: required.iter().map(|value| (*value).to_string()).collect(),
        allow_additional: false,
    }
}

fn field(name: &str, value_type: ToolValueType, description: &str) -> ToolFieldSchema {
    ToolFieldSchema {
        name: name.to_string(),
        value_type,
        nullable: false,
        description: description.to_string(),
    }
}

fn nullable_field(name: &str, value_type: ToolValueType, description: &str) -> ToolFieldSchema {
    ToolFieldSchema {
        name: name.to_string(),
        value_type,
        nullable: true,
        description: description.to_string(),
    }
}

fn summarize_input(input: &Value) -> String {
    let Some(object) = input.as_object() else {
        return "invalid input".to_string();
    };
    if object.is_empty() {
        return "no input fields".to_string();
    }
    let mut fields = object.keys().cloned().collect::<Vec<_>>();
    fields.sort();
    format!("fields: {}", fields.join(", "))
}

pub fn tool_request_fingerprint(request: &ToolExecutionRequest) -> String {
    let payload = serde_json::json!({
        "tool_id": request.tool_id.trim(),
        "input": canonical_json_value(&request.input),
        "access_mode": request.access_mode,
        "run_id": request.run_id,
    });
    let bytes = serde_json::to_vec(&payload).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn tool_approval_preview(request: &ToolExecutionRequest) -> String {
    if request.tool_id == CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID {
        let provider = request.input["provider_id"]
            .as_str()
            .map(str::trim)
            .unwrap_or("connector");
        let account = request.input["account_id"]
            .as_str()
            .map(str::trim)
            .unwrap_or("unknown account");
        let file_name = request.input["file_name"]
            .as_str()
            .map(str::trim)
            .unwrap_or("attachment");
        let media_type = request.input["media_type"]
            .as_str()
            .map(str::trim)
            .unwrap_or("unknown type");
        let size_bytes = request.input["size_bytes"].as_u64().unwrap_or_default();
        let workspace_identity = request.input["workspace_identity"]
            .as_str()
            .unwrap_or_default();
        let workspace_short = workspace_identity.get(..12).unwrap_or(workspace_identity);
        return format!(
            "Download `{file_name}` ({size_bytes} bytes, {media_type}) from {provider} account {account} as untrusted evidence into managed workspace {workspace_short}/connector-downloads. The file will not be opened automatically."
        );
    }
    format!("Approve this exact `{}` tool request.", request.tool_id)
}

fn canonical_json_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut fields = object.iter().collect::<Vec<_>>();
            fields.sort_by_key(|(left, _)| *left);
            Value::Object(
                fields
                    .into_iter()
                    .map(|(name, value)| (name.clone(), canonical_json_value(value)))
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json_value).collect()),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        builtin_tool_catalog, prepare_tool_execution, tool_request_fingerprint,
        validate_tool_input, ToolEvidence, ToolExecutionRequest, ToolExecutionStatus,
        ToolInvocationRecord, ToolPathScope, ToolResourceAccess, ToolVerificationResult,
        APP_UPDATE_CHECK_TOOL_ID, APP_UPDATE_DOWNLOAD_TOOL_ID, APP_UPDATE_INSTALL_TOOL_ID,
        BROWSER_OPEN_TOOL_ID, CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID, CONNECTOR_MUTATE_TOOL_ID,
        FILESYSTEM_MUTATE_TOOL_ID, FILE_READ_TOOL_ID, FILE_WRITE_TOOL_ID, OFFICE_CREATE_TOOL_ID,
        OFFICE_OPEN_TOOL_ID, OFFICE_UPDATE_TOOL_ID, OPERATIONS_BRIEFING_TOOL_ID,
        SKILL_ACTIVATE_TOOL_ID, T1_POWERPOINT_TOOL_ID, T1_RECONCILIATION_TOOL_ID,
        TERMINAL_READ_TOOL_ID,
    };
    use crate::kernel::models::AccessMode;
    use crate::kernel::policy::{CapabilityKind, PolicyDecision, RiskLevel};

    #[test]
    fn builtin_catalog_declares_versioned_app_update_toolchain() {
        let catalog = builtin_tool_catalog();
        let check = catalog
            .iter()
            .find(|contract| contract.id == APP_UPDATE_CHECK_TOOL_ID)
            .expect("app update check contract");
        let download = catalog
            .iter()
            .find(|contract| contract.id == APP_UPDATE_DOWNLOAD_TOOL_ID)
            .expect("app update download contract");
        let install = catalog
            .iter()
            .find(|contract| contract.id == APP_UPDATE_INSTALL_TOOL_ID)
            .expect("app update install contract");

        assert_eq!(check.version, "1.0.0");
        assert_eq!(check.capability, CapabilityKind::AppUpdateCheck);
        assert_eq!(check.risk_level, RiskLevel::Low);
        assert!(check
            .constraints
            .allowed_network_hosts
            .contains(&"api.github.com".to_string()));

        assert_eq!(download.capability, CapabilityKind::AppUpdateDownload);
        assert_eq!(
            download.constraints.path_scope,
            ToolPathScope::AppUpdateDirectory
        );
        assert!(download
            .verification
            .required_evidence_kinds
            .contains(&"downloaded_installer".to_string()));

        assert_eq!(install.capability, CapabilityKind::AppUpdateInstall);
        assert_eq!(install.risk_level, RiskLevel::Critical);
        assert!(install.constraints.mutates_machine_state);
        assert!(install
            .input_schema
            .required
            .contains(&"download_receipt".to_string()));
    }

    #[test]
    fn connector_attachment_contract_is_critical_exact_and_semantically_bounded() {
        let account_id = Uuid::new_v4();
        let request = ToolExecutionRequest {
            tool_id: CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID.to_string(),
            input: json!({
                "provider_id": "microsoft",
                "account_id": account_id,
                "parent_remote_ref": "message-one",
                "attachment_remote_ref": "attachment-one",
                "file_name": "report.pdf",
                "media_type": "application/pdf",
                "size_bytes": 4096,
                "account_generation": 3,
                "workspace_identity": "a".repeat(64),
            }),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        };
        let plan = prepare_tool_execution(&request).expect("attachment plan validates");
        assert_eq!(
            plan.contract.capability,
            CapabilityKind::ConnectorAttachmentRead
        );
        assert_eq!(plan.contract.risk_level, RiskLevel::Critical);
        assert_eq!(plan.policy_decision, PolicyDecision::Ask);
        assert_eq!(
            plan.contract.constraints.path_scope,
            ToolPathScope::Workspace
        );
        assert!(plan.contract.constraints.mutates_machine_state);

        let mut oversized = request;
        oversized.input["size_bytes"] = json!(20 * 1024 * 1024 + 1);
        assert!(prepare_tool_execution(&oversized).is_err());
    }

    #[test]
    fn builtin_catalog_declares_file_write_as_a_sandboxed_resource_tool() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == FILE_WRITE_TOOL_ID)
            .expect("file write contract");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::FileWrite);
        assert_eq!(contract.risk_level, RiskLevel::High);
        assert_eq!(contract.constraints.path_scope, ToolPathScope::Workspace);
        assert!(contract.constraints.mutates_machine_state);
        let resource = contract.constraints.resource.expect("write resource");
        assert_eq!(resource.key, "local_filesystem://mutation");
        assert_eq!(resource.access, ToolResourceAccess::Write);
        assert!(contract
            .verification
            .required_evidence_kinds
            .contains(&"written_file".to_string()));
    }

    #[test]
    fn builtin_catalog_declares_office_create_as_verified_binary_workspace_write() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == OFFICE_CREATE_TOOL_ID)
            .expect("office create contract");

        assert_eq!(contract.version, "1.1.0");
        assert!(contract.description.contains("at most three"));
        assert_eq!(contract.capability, CapabilityKind::FileWrite);
        assert_eq!(contract.risk_level, RiskLevel::High);
        assert_eq!(contract.constraints.path_scope, ToolPathScope::Workspace);
        assert!(contract.constraints.mutates_machine_state);
        let resource = contract.constraints.resource.expect("write resource");
        assert_eq!(resource.key, "local_filesystem://mutation");
        assert_eq!(resource.access, ToolResourceAccess::Write);
        assert!(contract
            .verification
            .required_evidence_kinds
            .contains(&"office_artifact".to_string()));
    }

    #[test]
    fn builtin_catalog_declares_office_update_as_verified_binary_workspace_write() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == OFFICE_UPDATE_TOOL_ID)
            .expect("office update contract");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::FileWrite);
        assert_eq!(contract.risk_level, RiskLevel::High);
        assert_eq!(contract.constraints.path_scope, ToolPathScope::Workspace);
        assert!(contract.constraints.mutates_machine_state);
        let resource = contract.constraints.resource.expect("write resource");
        assert_eq!(resource.key, "local_filesystem://mutation");
        assert_eq!(resource.access, ToolResourceAccess::Write);
        assert!(contract
            .verification
            .required_evidence_kinds
            .contains(&"office_artifact_update".to_string()));
    }

    #[test]
    fn builtin_catalog_declares_t1_reconciliation_as_verified_workspace_write() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == T1_RECONCILIATION_TOOL_ID)
            .expect("T1 reconciliation contract");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::FileWrite);
        assert_eq!(contract.risk_level, RiskLevel::High);
        assert_eq!(contract.constraints.path_scope, ToolPathScope::Workspace);
        assert!(contract.constraints.mutates_machine_state);
        let resource = contract.constraints.resource.expect("write resource");
        assert_eq!(resource.key, "local_filesystem://mutation");
        assert_eq!(resource.access, ToolResourceAccess::Write);
        assert_eq!(
            contract.verification.required_evidence_kinds,
            [
                "t1_source_manifest",
                "t1_fact_provenance",
                "t1_reconciliation_xlsx",
            ]
        );
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: T1_RECONCILIATION_TOOL_ID.to_string(),
            input: json!({
                "source_directory": "inputs",
                "output_relative_path": "outputs/reconciliation.txt",
            }),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());
    }

    #[test]
    fn builtin_catalog_declares_t1_powerpoint_as_verified_workspace_write() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == T1_POWERPOINT_TOOL_ID)
            .expect("T1 PowerPoint contract");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::FileWrite);
        assert_eq!(contract.risk_level, RiskLevel::High);
        assert_eq!(contract.constraints.path_scope, ToolPathScope::Workspace);
        assert!(contract.constraints.mutates_machine_state);
        let resource = contract.constraints.resource.expect("write resource");
        assert_eq!(resource.key, "local_filesystem://mutation");
        assert_eq!(resource.access, ToolResourceAccess::Write);
        assert_eq!(
            contract.verification.required_evidence_kinds,
            [
                "one_page_pptx",
                "actual_render_receipt",
                "office_revision_receipt",
            ]
        );
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: T1_POWERPOINT_TOOL_ID.to_string(),
            input: json!({
                "source_directory": "inputs",
                "reconciliation": {},
                "output_relative_path": "outputs/brief.txt",
            }),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());
    }

    #[test]
    fn builtin_catalog_declares_office_open_as_verified_foreground_launch() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == OFFICE_OPEN_TOOL_ID)
            .expect("office open contract");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::FileRead);
        assert_eq!(contract.risk_level, RiskLevel::Medium);
        assert_eq!(contract.constraints.path_scope, ToolPathScope::Workspace);
        assert!(contract.constraints.mutates_machine_state);
        let resource = contract.constraints.resource.expect("foreground resource");
        assert_eq!(resource.key, "computer://foreground_desktop");
        assert_eq!(resource.access, ToolResourceAccess::Write);
        assert!(contract
            .verification
            .required_evidence_kinds
            .contains(&"office_open_receipt".to_string()));
    }

    #[test]
    fn builtin_catalog_declares_browser_open_as_verified_foreground_launch() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == BROWSER_OPEN_TOOL_ID)
            .expect("browser open contract");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::BrowserBrowse);
        assert_eq!(contract.risk_level, RiskLevel::Medium);
        assert_eq!(contract.constraints.path_scope, ToolPathScope::None);
        assert!(contract.constraints.mutates_machine_state);
        let resource = contract.constraints.resource.expect("foreground resource");
        assert_eq!(resource.key, "computer://foreground_desktop");
        assert_eq!(resource.access, ToolResourceAccess::Write);
        assert!(contract
            .verification
            .required_evidence_kinds
            .contains(&"browser_open_receipt".to_string()));
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: BROWSER_OPEN_TOOL_ID.to_string(),
            input: json!({
                "url": "file:///C:/Windows/System32/config/SAM",
                "preferred_browser": "default",
                "summary": "Reject a non-web target."
            }),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());
    }

    #[test]
    fn builtin_catalog_declares_operations_briefing_as_verified_workflow_read() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == OPERATIONS_BRIEFING_TOOL_ID)
            .expect("operations briefing contract");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::FileRead);
        assert_eq!(contract.risk_level, RiskLevel::Low);
        assert_eq!(
            contract.constraints.path_scope,
            ToolPathScope::LocalFilesystem
        );
        assert!(!contract.constraints.mutates_machine_state);
        let resource = contract
            .constraints
            .resource
            .expect("filesystem read resource");
        assert_eq!(resource.key, "local_filesystem://mutation");
        assert_eq!(resource.access, ToolResourceAccess::Read);
        assert!(contract
            .verification
            .required_evidence_kinds
            .contains(&"operations_briefing_draft".to_string()));
    }

    #[test]
    fn file_write_tool_rejects_blank_missing_and_unknown_input_fields() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == FILE_WRITE_TOOL_ID)
            .expect("file write contract");

        assert!(validate_tool_input(&contract, &json!({"path": "reports/a.md"})).is_err());
        assert!(validate_tool_input(
            &contract,
            &json!({"path": "reports/a.md", "summary": "Write", "content": "A", "raw": true})
        )
        .is_err());
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: FILE_WRITE_TOOL_ID.to_string(),
            input: json!({"path": " ", "summary": "Write", "content": "A"}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: FILE_WRITE_TOOL_ID.to_string(),
            input: json!({"path": "reports/a.md", "summary": "Write", "content": " "}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());
    }

    #[test]
    fn file_write_tool_uses_capability_policy_instead_of_model_claims() {
        let ask = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: FILE_WRITE_TOOL_ID.to_string(),
            input: json!({"path": "reports/a.md", "summary": "Write", "content": "A"}),
            access_mode: AccessMode::AskOnRisk,
            run_id: None,
        })
        .expect("ask plan");
        let trusted = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: FILE_WRITE_TOOL_ID.to_string(),
            input: json!({"path": "reports/a.md", "summary": "Write", "content": "A"}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .expect("trusted plan");

        assert_eq!(ask.policy_decision, PolicyDecision::Ask);
        assert_eq!(trusted.policy_decision, PolicyDecision::Allow);
    }

    #[test]
    fn builtin_catalog_declares_file_read_as_sandboxed_verified_resource_reader() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == FILE_READ_TOOL_ID)
            .expect("file read contract");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::FileRead);
        assert_eq!(contract.risk_level, RiskLevel::Low);
        assert_eq!(
            contract.constraints.path_scope,
            ToolPathScope::LocalFilesystem
        );
        assert!(!contract.constraints.mutates_machine_state);
        let resource = contract.constraints.resource.expect("read resource");
        assert_eq!(resource.key, "local_filesystem://mutation");
        assert_eq!(resource.access, ToolResourceAccess::Read);
        assert!(contract
            .verification
            .required_evidence_kinds
            .contains(&"file_content".to_string()));
    }

    #[test]
    fn file_read_tool_rejects_blank_missing_and_unknown_input_fields() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == FILE_READ_TOOL_ID)
            .expect("file read contract");

        assert!(validate_tool_input(&contract, &json!({"path": "notes/a.md"})).is_err());
        assert!(validate_tool_input(
            &contract,
            &json!({"path": "notes/a.md", "summary": "Read", "raw": true})
        )
        .is_err());
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: FILE_READ_TOOL_ID.to_string(),
            input: json!({"path": " ", "summary": "Read"}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());
    }

    #[test]
    fn builtin_catalog_declares_filesystem_mutation_as_a_sandboxed_resource_tool() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == FILESYSTEM_MUTATE_TOOL_ID)
            .expect("filesystem mutation contract");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::FileWrite);
        assert_eq!(contract.risk_level, RiskLevel::High);
        assert_eq!(
            contract.constraints.path_scope,
            ToolPathScope::LocalFilesystem
        );
        assert!(contract.constraints.mutates_machine_state);
        let resource = contract
            .constraints
            .resource
            .expect("filesystem write resource");
        assert_eq!(resource.key, "local_filesystem://mutation");
        assert_eq!(resource.access, ToolResourceAccess::Write);
        assert!(contract
            .verification
            .required_evidence_kinds
            .contains(&"filesystem_state".to_string()));
    }

    #[test]
    fn filesystem_mutation_tool_rejects_invalid_operation_specific_inputs() {
        let base = |input| ToolExecutionRequest {
            tool_id: FILESYSTEM_MUTATE_TOOL_ID.to_string(),
            input,
            access_mode: AccessMode::FullAccess,
            run_id: None,
        };

        assert!(prepare_tool_execution(&base(json!({
            "operation": "run_shell",
            "path": "C:/Temp/a.txt",
            "summary": "Unsupported"
        })))
        .is_err());
        assert!(prepare_tool_execution(&base(json!({
            "operation": "rename_file",
            "path": "C:/Temp/a.txt",
            "summary": "Missing destination"
        })))
        .is_err());
        assert!(prepare_tool_execution(&base(json!({
            "operation": "create_file",
            "path": "C:/Temp/a.txt",
            "summary": "Missing content"
        })))
        .is_err());
        assert!(prepare_tool_execution(&base(json!({
            "operation": "delete_file",
            "path": " ",
            "summary": "Blank path"
        })))
        .is_err());
        assert!(prepare_tool_execution(&base(json!({
            "operation": "rename_directory",
            "path": "C:/Temp/a",
            "destination": "C:/Temp/b",
            "summary": "Rename"
        })))
        .is_ok());
    }

    #[test]
    fn builtin_catalog_declares_terminal_read_as_allowlisted_verified_tool() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == TERMINAL_READ_TOOL_ID)
            .expect("terminal read contract");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::TerminalRead);
        assert_eq!(contract.risk_level, RiskLevel::Low);
        assert_eq!(
            contract.constraints.path_scope,
            ToolPathScope::LocalFilesystem
        );
        assert!(!contract.constraints.mutates_machine_state);
        assert!(contract.constraints.resource.is_none());
        assert!(contract
            .verification
            .required_evidence_kinds
            .contains(&"terminal_output".to_string()));
    }

    #[test]
    fn terminal_read_tool_rejects_blank_missing_and_unknown_input_fields() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == TERMINAL_READ_TOOL_ID)
            .expect("terminal read contract");

        assert!(validate_tool_input(&contract, &json!({"command": "pwd"})).is_err());
        assert!(validate_tool_input(
            &contract,
            &json!({"command": "pwd", "summary": "Inspect", "shell": "powershell"})
        )
        .is_err());
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: TERMINAL_READ_TOOL_ID.to_string(),
            input: json!({"command": " ", "summary": "Inspect"}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());
    }

    #[test]
    fn builtin_catalog_declares_public_network_read_tools() {
        let catalog = builtin_tool_catalog();
        let browser = catalog
            .iter()
            .find(|contract| contract.id == "browser.browse")
            .expect("browser browse contract");
        let search = catalog
            .iter()
            .find(|contract| contract.id == "network.search")
            .expect("network search contract");

        assert_eq!(browser.version, "1.0.0");
        assert_eq!(browser.capability, CapabilityKind::BrowserBrowse);
        assert_eq!(browser.risk_level, RiskLevel::Low);
        assert_eq!(browser.constraints.path_scope, ToolPathScope::None);
        assert!(!browser.constraints.mutates_machine_state);
        assert!(browser.constraints.resource.is_none());
        assert!(browser
            .constraints
            .allowed_network_hosts
            .contains(&"public_http".to_string()));
        assert!(browser
            .verification
            .required_evidence_kinds
            .contains(&"browser_page".to_string()));

        assert_eq!(search.version, "1.0.0");
        assert_eq!(search.capability, CapabilityKind::NetworkSearch);
        assert_eq!(search.risk_level, RiskLevel::Low);
        assert_eq!(search.constraints.path_scope, ToolPathScope::None);
        assert!(!search.constraints.mutates_machine_state);
        assert!(search.constraints.resource.is_none());
        assert!(search
            .constraints
            .allowed_network_hosts
            .contains(&"duckduckgo.com".to_string()));
        assert!(search
            .verification
            .required_evidence_kinds
            .contains(&"source_links".to_string()));
    }

    #[test]
    fn public_network_read_tools_reject_blank_missing_and_unknown_input_fields() {
        let catalog = builtin_tool_catalog();
        let browser = catalog
            .iter()
            .find(|contract| contract.id == "browser.browse")
            .expect("browser browse contract");
        let search = catalog
            .iter()
            .find(|contract| contract.id == "network.search")
            .expect("network search contract");

        assert!(validate_tool_input(browser, &json!({"url": "https://example.com"})).is_err());
        assert!(validate_tool_input(
            browser,
            &json!({"url": "https://example.com", "summary": "Read", "raw": true})
        )
        .is_err());
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: "browser.browse".to_string(),
            input: json!({"url": " ", "summary": "Read"}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());

        assert!(validate_tool_input(search, &json!({"query": "DS Agent"})).is_err());
        assert!(validate_tool_input(
            search,
            &json!({"query": "DS Agent", "scope": "public web", "raw": true})
        )
        .is_err());
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: "network.search".to_string(),
            input: json!({"query": " ", "scope": "public web", "summary": "Research"}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: "network.search".to_string(),
            input: json!({"query": "DS Agent", "scope": " ", "summary": "Research"}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());
    }

    #[test]
    fn builtin_catalog_declares_computer_use_tools_with_shared_desktop_resource() {
        let catalog = builtin_tool_catalog();
        let screenshot = catalog
            .iter()
            .find(|contract| contract.id == "computer.screenshot")
            .expect("computer screenshot contract");
        let control = catalog
            .iter()
            .find(|contract| contract.id == "computer.control")
            .expect("computer control contract");

        assert_eq!(screenshot.version, "1.0.0");
        assert_eq!(screenshot.capability, CapabilityKind::ComputerScreenshot);
        assert_eq!(screenshot.risk_level, RiskLevel::Medium);
        assert_eq!(
            screenshot.constraints.path_scope,
            ToolPathScope::AppEvidenceDirectory
        );
        assert!(!screenshot.constraints.mutates_machine_state);
        let screenshot_resource = screenshot
            .constraints
            .resource
            .as_ref()
            .expect("desktop read resource");
        assert_eq!(screenshot_resource.key, "computer://foreground_desktop");
        assert_eq!(screenshot_resource.access, ToolResourceAccess::Read);
        assert!(screenshot
            .verification
            .required_evidence_kinds
            .contains(&"screenshot_image".to_string()));

        assert_eq!(control.version, "1.0.0");
        assert_eq!(control.capability, CapabilityKind::ComputerControl);
        assert_eq!(control.risk_level, RiskLevel::Critical);
        assert_eq!(control.constraints.path_scope, ToolPathScope::None);
        assert!(control.constraints.mutates_machine_state);
        let control_resource = control
            .constraints
            .resource
            .as_ref()
            .expect("desktop write resource");
        assert_eq!(control_resource.key, "computer://foreground_desktop");
        assert_eq!(control_resource.access, ToolResourceAccess::Write);
        assert!(control
            .verification
            .required_evidence_kinds
            .contains(&"computer_control_receipt".to_string()));
    }

    #[test]
    fn builtin_catalog_declares_hash_verified_skill_activation() {
        let contract = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == SKILL_ACTIVATE_TOOL_ID)
            .expect("skill activation contract exists");

        assert_eq!(contract.version, "1.0.0");
        assert_eq!(contract.capability, CapabilityKind::SkillUse);
        assert_eq!(contract.risk_level, RiskLevel::Low);
        assert_eq!(
            contract.constraints.path_scope,
            ToolPathScope::InstalledSkillStore
        );
        assert!(!contract.constraints.mutates_machine_state);
        assert_eq!(
            contract.verification.required_evidence_kinds,
            vec!["skill_context".to_string()]
        );

        let plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: SKILL_ACTIVATE_TOOL_ID.to_string(),
            input: serde_json::json!({
                "skill_id": Uuid::new_v4().to_string(),
                "input_summary": "Apply the installed workflow to the active task."
            }),
            access_mode: AccessMode::AskOnRisk,
            run_id: None,
        })
        .expect("valid skill activation prepares");
        assert_eq!(plan.policy_decision, PolicyDecision::Allow);

        let error = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: SKILL_ACTIVATE_TOOL_ID.to_string(),
            input: serde_json::json!({
                "skill_id": "not-a-uuid",
                "input_summary": "Apply the skill."
            }),
            access_mode: AccessMode::AskOnRisk,
            run_id: None,
        })
        .expect_err("invalid skill id is rejected");
        assert!(error.contains("UUID"));
    }

    #[test]
    fn computer_use_tools_reject_invalid_input_and_keep_control_confirmation_mandatory() {
        let catalog = builtin_tool_catalog();
        let screenshot = catalog
            .iter()
            .find(|contract| contract.id == "computer.screenshot")
            .expect("computer screenshot contract");
        let control = catalog
            .iter()
            .find(|contract| contract.id == "computer.control")
            .expect("computer control contract");

        assert!(validate_tool_input(screenshot, &json!({})).is_err());
        assert!(validate_tool_input(
            screenshot,
            &json!({"summary": "Inspect", "display": "primary"})
        )
        .is_err());
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: "computer.screenshot".to_string(),
            input: json!({"summary": " "}),
            access_mode: AccessMode::LimitedAuto,
            run_id: None,
        })
        .is_err());

        assert!(
            validate_tool_input(control, &json!({"target": "Word", "action": "click:10,20"}))
                .is_err()
        );
        assert!(validate_tool_input(
            control,
            &json!({
                "target": "Word",
                "action": "click:10,20",
                "summary": "Click",
                "shell": true
            })
        )
        .is_err());
        assert!(prepare_tool_execution(&ToolExecutionRequest {
            tool_id: "computer.control".to_string(),
            input: json!({"target": "Word", "action": " ", "summary": "Click"}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .is_err());

        let screenshot_plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: "computer.screenshot".to_string(),
            input: json!({"summary": "Inspect the current desktop"}),
            access_mode: AccessMode::LimitedAuto,
            run_id: None,
        })
        .expect("limited-auto screenshot plan");
        let control_plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: "computer.control".to_string(),
            input: json!({
                "target": "Word",
                "action": "click:10,20",
                "summary": "Click the selected control"
            }),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .expect("full-access control plan");

        assert_eq!(screenshot_plan.policy_decision, PolicyDecision::Allow);
        assert_eq!(control_plan.policy_decision, PolicyDecision::Ask);
    }

    #[test]
    fn tool_input_validation_rejects_missing_or_unknown_install_fields() {
        let install = builtin_tool_catalog()
            .into_iter()
            .find(|contract| contract.id == APP_UPDATE_INSTALL_TOOL_ID)
            .expect("app update install contract");

        assert!(validate_tool_input(&install, &json!({})).is_err());
        assert!(validate_tool_input(&install, &json!({"download_receipt": 42})).is_err());
        assert!(validate_tool_input(
            &install,
            &json!({"download_receipt": "dsur1.123456781234123412341234567890ab.3", "approve": true})
        )
        .is_err());
        assert!(validate_tool_input(
            &install,
            &json!({"download_receipt": "dsur1.123456781234123412341234567890ab.3"})
        )
        .is_ok());
    }

    #[test]
    fn tool_policy_keeps_update_install_confirmation_mandatory() {
        let check = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: APP_UPDATE_CHECK_TOOL_ID.to_string(),
            input: json!({}),
            access_mode: AccessMode::AskOnRisk,
            run_id: None,
        })
        .expect("check plan");
        let download = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: APP_UPDATE_DOWNLOAD_TOOL_ID.to_string(),
            input: json!({}),
            access_mode: AccessMode::AskOnRisk,
            run_id: None,
        })
        .expect("download plan");
        let install = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: APP_UPDATE_INSTALL_TOOL_ID.to_string(),
            input: json!({"download_receipt": "dsur1.123456781234123412341234567890ab.3"}),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .expect("install plan");

        assert_eq!(check.policy_decision, PolicyDecision::Allow);
        assert_eq!(download.policy_decision, PolicyDecision::Ask);
        assert_eq!(install.policy_decision, PolicyDecision::Ask);
    }

    #[test]
    fn connector_mutation_exact_fingerprint_and_confirmation_are_mandatory() {
        let input = json!({
            "provider_id": "fake",
            "account_id": Uuid::new_v4().to_string(),
            "account_generation": 0,
            "capability": "mail_send_draft",
            "target_ref": "draft:42",
            "preview_hash": "sha256:preview-a",
            "idempotency_key": "send:42:once",
            "automation_run_id": Uuid::new_v4().to_string()
        });
        let request = ToolExecutionRequest {
            tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            input: input.clone(),
            access_mode: AccessMode::FullAccess,
            run_id: Some(Uuid::new_v4()),
        };
        let plan = prepare_tool_execution(&request).expect("connector mutation plan");
        assert_eq!(plan.policy_decision, PolicyDecision::Ask);

        let original = tool_request_fingerprint(&request);
        let mut changed = request;
        changed.input["preview_hash"] = json!("sha256:preview-b");
        assert_ne!(original, tool_request_fingerprint(&changed));
        changed.input["preview_hash"] = input["preview_hash"].clone();
        changed.input["account_generation"] = json!(1);
        assert_ne!(original, tool_request_fingerprint(&changed));
    }

    #[test]
    fn successful_tool_invocation_requires_verified_evidence() {
        let plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: APP_UPDATE_CHECK_TOOL_ID.to_string(),
            input: json!({}),
            access_mode: AccessMode::AskOnRisk,
            run_id: None,
        })
        .expect("check plan");

        let unverified = ToolInvocationRecord::succeeded(
            &plan,
            json!({"current_version": "0.1.2", "update_available": false}),
            Vec::new(),
            ToolVerificationResult::failed("release status was not verified"),
            None,
            12,
        );
        assert!(unverified.is_err());

        let verified = ToolInvocationRecord::succeeded(
            &plan,
            json!({"current_version": "0.1.2", "update_available": false}),
            vec![ToolEvidence {
                kind: "release_status".to_string(),
                reference: "github:Lee-take/deepseek-agent-os/releases".to_string(),
                summary: "Current and latest release versions were compared.".to_string(),
            }],
            ToolVerificationResult::passed("release status contains the current version"),
            None,
            12,
        )
        .expect("verified invocation");

        assert_eq!(verified.status, ToolExecutionStatus::Succeeded);
        assert!(verified.verification.passed);
        assert_eq!(verified.evidence.len(), 1);
    }

    #[test]
    fn tool_request_fingerprint_is_canonical_input_bound_and_secret_safe() {
        let request = |input| ToolExecutionRequest {
            tool_id: FILE_READ_TOOL_ID.to_string(),
            input,
            access_mode: AccessMode::AskEveryStep,
            run_id: None,
        };
        let first = tool_request_fingerprint(&request(json!({
            "path": "C:/Evidence/a.md",
            "summary": "private-purpose-token"
        })));
        let reordered = tool_request_fingerprint(&request(json!({
            "summary": "private-purpose-token",
            "path": "C:/Evidence/a.md"
        })));
        let different = tool_request_fingerprint(&request(json!({
            "path": "C:/Evidence/b.md",
            "summary": "private-purpose-token"
        })));

        assert_eq!(first, reordered);
        assert_ne!(first, different);
        assert_eq!(first.len(), 64);
        assert!(!first.contains("private-purpose-token"));
        assert!(first.chars().all(|character| character.is_ascii_hexdigit()));
    }
}
