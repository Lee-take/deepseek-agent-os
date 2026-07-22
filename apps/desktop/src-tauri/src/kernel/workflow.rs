use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

use crate::kernel::capability::{
    run_evidence_folder_ingest, CapabilityInvocation, CapabilityInvocationStatus,
    EvidenceFolderClient, EvidenceFolderRequest,
};
use crate::kernel::models::AccessMode;
use crate::kernel::policy::{
    request_capability_access, CapabilityAccessRequest, CapabilityKind, PolicyDecision,
};

pub const OPERATIONS_BRIEFING_WORKFLOW_ID: &str = "operations.briefing.v1";
const OPERATIONS_BRIEFING_REPAIR_RETRY_BUDGET: usize = 1;
const OPERATIONS_BRIEFING_REPAIR_CONTEXT_MAX_CHARS: usize = 4_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationsBriefingRunStatus {
    PendingApproval,
    DraftReady,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsBriefingRequest {
    pub access_mode: AccessMode,
    pub evidence_folder_path: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsBriefingAnomaly {
    pub area: String,
    pub signal: String,
    pub evidence_ref: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsBriefingAction {
    pub owner: String,
    pub action: String,
    pub due_hint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsBriefingSynthesis {
    pub summary: String,
    pub anomalies: Vec<OperationsBriefingAnomaly>,
    pub action_plan: Vec<OperationsBriefingAction>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsBriefingContextReceipt {
    pub user_intent: String,
    #[serde(default = "default_operations_briefing_loop_mode")]
    pub loop_mode: String,
    #[serde(default = "default_operations_briefing_workflow_policy")]
    pub workflow_policy: String,
    pub selected_evidence: Vec<String>,
    pub selected_memories: Vec<String>,
    pub model_route: String,
    pub thinking_level: String,
    pub token_cache_state: String,
    pub validation_results: Vec<String>,
    pub intentional_omissions: Vec<String>,
}

fn default_operations_briefing_loop_mode() -> String {
    "workflow_pack_run".to_string()
}

fn default_operations_briefing_workflow_policy() -> String {
    "Operations Briefing v1: FileRead evidence folder only; raw file bodies omitted; deterministic validators first; bounded model repair retry budget 1."
        .to_string()
}

pub trait OperationsBriefingSynthesizer {
    fn synthesize_briefing(
        &self,
        manifest_excerpt: &str,
        evidence_ref: Option<&str>,
    ) -> Result<OperationsBriefingSynthesis, String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsBriefingRun {
    pub id: Uuid,
    pub workflow_id: String,
    pub status: OperationsBriefingRunStatus,
    #[serde(default)]
    pub archived_from_package: bool,
    pub evidence_folder_path: Option<String>,
    pub evidence_invocation_id: Option<Uuid>,
    pub title: String,
    pub summary: String,
    pub anomalies: Vec<OperationsBriefingAnomaly>,
    pub action_plan: Vec<OperationsBriefingAction>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub context_receipt: OperationsBriefingContextReceipt,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsBriefingOutcome {
    pub access_request: CapabilityAccessRequest,
    pub evidence_invocation: CapabilityInvocation,
    pub run: OperationsBriefingRun,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OperationsBriefingRepairResult {
    reason: String,
    succeeded: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsBriefingTemplateSeedRequest {
    pub access_mode: AccessMode,
    pub evidence_folder_path: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsBriefingTemplateSeedResult {
    pub target_dir: String,
    pub written_files: Vec<String>,
    pub skipped_files: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowTemplateFile {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowTemplatePackage {
    pub id: String,
    pub workflow_id: String,
    pub title: String,
    pub description: String,
    pub files: Vec<WorkflowTemplateFile>,
}

impl WorkflowTemplatePackage {
    pub fn new(
        id: String,
        workflow_id: String,
        title: String,
        description: String,
        files: Vec<WorkflowTemplateFile>,
    ) -> Result<Self, String> {
        let id = id.trim().to_string();
        let workflow_id = workflow_id.trim().to_string();
        let title = title.trim().to_string();
        if id.is_empty() {
            return Err("workflow template package id is required".to_string());
        }
        if workflow_id.is_empty() {
            return Err("workflow id is required".to_string());
        }
        if title.is_empty() {
            return Err("workflow template title is required".to_string());
        }

        Ok(Self {
            id,
            workflow_id,
            title,
            description: description.trim().to_string(),
            files,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationsBriefingEvidenceTemplate {
    pub file_name: &'static str,
    pub content: &'static str,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationsBriefingTemplateSeedOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
    pub seed_result: Option<OperationsBriefingTemplateSeedResult>,
}

pub trait OperationsBriefingTemplateSeeder {
    fn seed_templates(
        &self,
        target_dir: &str,
    ) -> Result<OperationsBriefingTemplateSeedResult, String>;
}

pub const OPERATIONS_BRIEFING_EVIDENCE_TEMPLATES: &[OperationsBriefingEvidenceTemplate] = &[
    OperationsBriefingEvidenceTemplate {
        file_name: "revenue.md",
        content: include_str!(
            "../../../../../docs/templates/operations-briefing-evidence/revenue.md"
        ),
    },
    OperationsBriefingEvidenceTemplate {
        file_name: "guest-experience.md",
        content: include_str!(
            "../../../../../docs/templates/operations-briefing-evidence/guest-experience.md"
        ),
    },
    OperationsBriefingEvidenceTemplate {
        file_name: "risk-and-compliance.md",
        content: include_str!(
            "../../../../../docs/templates/operations-briefing-evidence/risk-and-compliance.md"
        ),
    },
    OperationsBriefingEvidenceTemplate {
        file_name: "action-followups.md",
        content: include_str!(
            "../../../../../docs/templates/operations-briefing-evidence/action-followups.md"
        ),
    },
];

pub fn operations_briefing_workflow_template_package() -> WorkflowTemplatePackage {
    WorkflowTemplatePackage::new(
        "operations.briefing.templates.v1".to_string(),
        OPERATIONS_BRIEFING_WORKFLOW_ID.to_string(),
        "Operations Briefing Evidence Templates".to_string(),
        "Blank operator templates for the Operations Briefing workflow.".to_string(),
        OPERATIONS_BRIEFING_EVIDENCE_TEMPLATES
            .iter()
            .map(|template| WorkflowTemplateFile {
                path: template.file_name.to_string(),
                content: template.content.to_string(),
            })
            .collect(),
    )
    .expect("compiled operations briefing templates are valid")
}

pub struct LocalOperationsBriefingTemplateSeeder;

impl OperationsBriefingTemplateSeeder for LocalOperationsBriefingTemplateSeeder {
    fn seed_templates(
        &self,
        target_dir: &str,
    ) -> Result<OperationsBriefingTemplateSeedResult, String> {
        let target_dir_path = Path::new(target_dir);
        std::fs::create_dir_all(target_dir_path)
            .map_err(|error| format!("template target directory could not be created: {error}"))?;

        let mut written_files = Vec::new();
        let mut skipped_files = Vec::new();
        for template in OPERATIONS_BRIEFING_EVIDENCE_TEMPLATES {
            let file_name = normalize_template_file_name(template.file_name)?;
            let output_path = target_dir_path.join(&file_name);
            if output_path.exists() {
                skipped_files.push(file_name);
                continue;
            }

            std::fs::write(&output_path, template.content)
                .map_err(|error| format!("template file could not be written: {error}"))?;
            written_files.push(file_name);
        }

        Ok(OperationsBriefingTemplateSeedResult {
            target_dir: target_dir_path.to_string_lossy().to_string(),
            written_files,
            skipped_files,
        })
    }
}

pub fn run_operations_briefing(
    request: OperationsBriefingRequest,
    client: &impl EvidenceFolderClient,
) -> Result<OperationsBriefingOutcome, String> {
    run_operations_briefing_internal(request, client, None)
}

pub fn run_operations_briefing_with_synthesizer(
    request: OperationsBriefingRequest,
    client: &impl EvidenceFolderClient,
    synthesizer: &impl OperationsBriefingSynthesizer,
) -> Result<OperationsBriefingOutcome, String> {
    run_operations_briefing_internal(request, client, Some(synthesizer))
}

pub fn run_operations_briefing_template_seed(
    request: OperationsBriefingTemplateSeedRequest,
    seeder: &impl OperationsBriefingTemplateSeeder,
) -> Result<OperationsBriefingTemplateSeedOutcome, String> {
    let evidence_folder_path = normalize_template_seed_folder_path(&request.evidence_folder_path)?;
    let started_at = Instant::now();
    let access_request = request_capability_access(request.access_mode, CapabilityKind::FileWrite)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(OperationsBriefingTemplateSeedOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileWrite,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(evidence_folder_path.clone()),
                evidence_ref: Some(evidence_folder_path.clone()),
                requested_url: None,
                evidence_url: None,
                title: Some("Operations briefing evidence template seed request".to_string()),
                excerpt: Some("Seed Operations Briefing evidence templates.".to_string()),
                warnings: vec![
                    "evidence template seeding requires FileWrite approval in this access mode"
                        .to_string(),
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
            seed_result: None,
        });
    }

    match seeder.seed_templates(&evidence_folder_path) {
        Ok(seed_result) => Ok(OperationsBriefingTemplateSeedOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileWrite,
                status: CapabilityInvocationStatus::Succeeded,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(evidence_folder_path.clone()),
                evidence_ref: Some(evidence_folder_path),
                requested_url: None,
                evidence_url: None,
                title: Some("Operations briefing evidence templates seeded".to_string()),
                excerpt: Some(format!(
                    "Wrote {} templates; skipped {} existing templates.",
                    seed_result.written_files.len(),
                    seed_result.skipped_files.len()
                )),
                warnings: if seed_result.skipped_files.is_empty() {
                    Vec::new()
                } else {
                    vec![format!(
                        "Skipped existing templates: {}",
                        seed_result.skipped_files.join(", ")
                    )]
                },
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
            seed_result: Some(seed_result),
        }),
        Err(error) => Ok(OperationsBriefingTemplateSeedOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileWrite,
                status: CapabilityInvocationStatus::Failed,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(evidence_folder_path.clone()),
                evidence_ref: Some(evidence_folder_path),
                requested_url: None,
                evidence_url: None,
                title: Some("Operations briefing evidence template seeding failed".to_string()),
                excerpt: Some(
                    "Operations Briefing evidence templates were not written.".to_string(),
                ),
                warnings: vec![error],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
            seed_result: None,
        }),
    }
}

fn run_operations_briefing_internal(
    request: OperationsBriefingRequest,
    client: &impl EvidenceFolderClient,
    synthesizer: Option<&dyn OperationsBriefingSynthesizer>,
) -> Result<OperationsBriefingOutcome, String> {
    let evidence_outcome = run_evidence_folder_ingest(
        EvidenceFolderRequest {
            access_mode: request.access_mode,
            folder_path: request.evidence_folder_path,
            approval_granted: request.approval_granted,
        },
        client,
    )?;
    let evidence_invocation = evidence_outcome.invocation;
    let evidence_ref = evidence_invocation.evidence_ref.clone();
    let manifest_excerpt = evidence_invocation
        .excerpt
        .clone()
        .unwrap_or_else(|| "No evidence manifest is available yet.".to_string());

    let (status, synthesis, repair_result) = match evidence_invocation.status {
        CapabilityInvocationStatus::PendingApproval => (
            OperationsBriefingRunStatus::PendingApproval,
            OperationsBriefingSynthesis {
                summary: "Waiting for FileRead approval before scanning the evidence folder."
                    .to_string(),
                anomalies: Vec::new(),
                action_plan: Vec::new(),
                warnings: evidence_invocation.warnings.clone(),
            },
            None,
        ),
        CapabilityInvocationStatus::Failed => (
            OperationsBriefingRunStatus::Failed,
            OperationsBriefingSynthesis {
                summary:
                    "Evidence folder ingestion failed before the briefing draft could be prepared."
                        .to_string(),
                anomalies: Vec::new(),
                action_plan: Vec::new(),
                warnings: evidence_invocation.warnings.clone(),
            },
            None,
        ),
        CapabilityInvocationStatus::Succeeded => {
            let deterministic = deterministic_operations_briefing_synthesis(
                &manifest_excerpt,
                evidence_ref.as_deref(),
            );
            let (synthesis, repair_result) = if let Some(synthesizer) = synthesizer {
                model_operations_briefing_synthesis(
                    synthesizer,
                    &manifest_excerpt,
                    evidence_ref.as_deref(),
                    deterministic,
                )
            } else {
                (deterministic, None)
            };

            (
                OperationsBriefingRunStatus::DraftReady,
                synthesis,
                repair_result,
            )
        }
    };
    let context_receipt = operations_briefing_context_receipt(
        &evidence_invocation,
        status,
        &synthesis,
        synthesizer.is_some(),
        repair_result.as_ref(),
    );

    Ok(OperationsBriefingOutcome {
        access_request: evidence_outcome.access_request,
        run: OperationsBriefingRun {
            id: Uuid::new_v4(),
            workflow_id: OPERATIONS_BRIEFING_WORKFLOW_ID.to_string(),
            status,
            archived_from_package: false,
            evidence_folder_path: evidence_ref,
            evidence_invocation_id: Some(evidence_invocation.id),
            title: "Operations Briefing Draft".to_string(),
            summary: synthesis.summary,
            anomalies: synthesis.anomalies,
            action_plan: synthesis.action_plan,
            warnings: synthesis.warnings,
            context_receipt,
            created_at: Utc::now(),
        },
        evidence_invocation,
    })
}

fn model_operations_briefing_synthesis(
    synthesizer: &dyn OperationsBriefingSynthesizer,
    manifest_excerpt: &str,
    evidence_ref: Option<&str>,
    deterministic: OperationsBriefingSynthesis,
) -> (
    OperationsBriefingSynthesis,
    Option<OperationsBriefingRepairResult>,
) {
    match synthesizer.synthesize_briefing(manifest_excerpt, evidence_ref) {
        Ok(synthesis) => (synthesis, None),
        Err(error) => {
            let repair_excerpt =
                operations_briefing_repair_manifest_excerpt(manifest_excerpt, &error);
            let mut last_repair_error = None;
            for _ in 0..OPERATIONS_BRIEFING_REPAIR_RETRY_BUDGET {
                match synthesizer.synthesize_briefing(&repair_excerpt, evidence_ref) {
                    Ok(mut synthesis) => {
                        synthesis.warnings.push(format!(
                            "bounded repair loop retried model synthesis after failure: {error}"
                        ));
                        return (
                            synthesis,
                            Some(OperationsBriefingRepairResult {
                                reason: error,
                                succeeded: true,
                            }),
                        );
                    }
                    Err(repair_error) => {
                        last_repair_error = Some(repair_error);
                    }
                }
            }

            let mut synthesis = deterministic;
            synthesis
                .warnings
                .push(format!("model-backed synthesis failed: {error}"));
            if let Some(repair_error) = last_repair_error {
                synthesis
                    .warnings
                    .push(format!("bounded repair loop failed: {repair_error}"));
            }
            (
                synthesis,
                Some(OperationsBriefingRepairResult {
                    reason: error,
                    succeeded: false,
                }),
            )
        }
    }
}

fn operations_briefing_repair_manifest_excerpt(
    manifest_excerpt: &str,
    validation_failure: &str,
) -> String {
    let compact_manifest = truncate_chars(
        manifest_excerpt,
        OPERATIONS_BRIEFING_REPAIR_CONTEXT_MAX_CHARS,
    );
    format!(
        "Repair failed Operations Briefing synthesis.\nValidation failure: {validation_failure}\nEvidence manifest excerpt:\n{compact_manifest}"
    )
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if index >= max_chars {
            output.push_str("\n[truncated]");
            return output;
        }
        output.push(character);
    }
    output
}

fn operations_briefing_context_receipt(
    evidence_invocation: &CapabilityInvocation,
    status: OperationsBriefingRunStatus,
    synthesis: &OperationsBriefingSynthesis,
    model_synthesis_requested: bool,
    repair_result: Option<&OperationsBriefingRepairResult>,
) -> OperationsBriefingContextReceipt {
    OperationsBriefingContextReceipt {
        user_intent: "Draft an Operations Briefing from selected local evidence.".to_string(),
        loop_mode: "workflow_pack_run".to_string(),
        workflow_policy:
            "Operations Briefing v1: FileRead evidence folder only; raw file bodies omitted; deterministic validators first; bounded model repair retry budget 1."
                .to_string(),
        selected_evidence: operations_briefing_selected_evidence(evidence_invocation),
        selected_memories: Vec::new(),
        model_route: if model_synthesis_requested {
            "model-backed synthesis requested".to_string()
        } else {
            "local deterministic synthesis".to_string()
        },
        thinking_level: "not recorded by workflow kernel".to_string(),
        token_cache_state: "no model request recorded".to_string(),
        validation_results: operations_briefing_validation_results(
            evidence_invocation,
            status,
            synthesis,
            repair_result,
        ),
        intentional_omissions: operations_briefing_intentional_omissions(
            status,
            model_synthesis_requested,
            repair_result,
        ),
    }
}

fn operations_briefing_selected_evidence(
    evidence_invocation: &CapabilityInvocation,
) -> Vec<String> {
    if let Some(excerpt) = evidence_invocation.excerpt.as_ref() {
        return vec![excerpt.clone()];
    }

    match evidence_invocation.status {
        CapabilityInvocationStatus::PendingApproval => {
            vec![
                "Evidence folder was not scanned because FileRead approval is pending.".to_string(),
            ]
        }
        CapabilityInvocationStatus::Failed => {
            vec!["Evidence folder ingestion failed before evidence was selected.".to_string()]
        }
        CapabilityInvocationStatus::Succeeded => {
            vec!["Evidence folder scan succeeded without a manifest excerpt.".to_string()]
        }
    }
}

fn operations_briefing_validation_results(
    evidence_invocation: &CapabilityInvocation,
    status: OperationsBriefingRunStatus,
    synthesis: &OperationsBriefingSynthesis,
    repair_result: Option<&OperationsBriefingRepairResult>,
) -> Vec<String> {
    let mut results = Vec::new();
    match evidence_invocation.status {
        CapabilityInvocationStatus::PendingApproval => {
            results.push("evidence folder ingestion pending user approval".to_string());
        }
        CapabilityInvocationStatus::Failed => {
            results.push("evidence folder ingestion failed".to_string());
        }
        CapabilityInvocationStatus::Succeeded => {
            results.push("evidence folder ingestion succeeded".to_string());
        }
    }

    match status {
        OperationsBriefingRunStatus::PendingApproval => {
            results.push("briefing draft not generated before approval".to_string());
        }
        OperationsBriefingRunStatus::Failed => {
            results.push("briefing draft not generated after evidence failure".to_string());
        }
        OperationsBriefingRunStatus::DraftReady => {
            results.push("briefing summary generated".to_string());
            results.push(format!(
                "{} anomaly leads generated",
                synthesis.anomalies.len()
            ));
            results.push(format!(
                "{} action items generated",
                synthesis.action_plan.len()
            ));
        }
    }

    if !synthesis.warnings.is_empty() {
        results.push(format!("{} warnings recorded", synthesis.warnings.len()));
    }

    if let Some(repair_result) = repair_result {
        results.push("bounded repair loop retried model synthesis once".to_string());
        if repair_result.succeeded {
            results.push("bounded repair loop succeeded".to_string());
        } else {
            results.push("bounded repair loop failed".to_string());
        }
    }

    results
}

fn operations_briefing_intentional_omissions(
    status: OperationsBriefingRunStatus,
    model_synthesis_requested: bool,
    repair_result: Option<&OperationsBriefingRepairResult>,
) -> Vec<String> {
    let mut omissions = vec![
        "Raw evidence file bodies are not stored in the context receipt.".to_string(),
        "Memory records are not selected by this Operations Briefing preview run.".to_string(),
    ];

    if repair_result.is_none() {
        omissions.push(
            "No repair loop was run unless a validation failure required a retry.".to_string(),
        );
    }

    if !model_synthesis_requested {
        omissions.push("Model-backed synthesis was not requested for this run.".to_string());
    }

    if status != OperationsBriefingRunStatus::DraftReady {
        omissions.push("Model synthesis was omitted because evidence was not ready.".to_string());
    }

    omissions
}

fn deterministic_operations_briefing_synthesis(
    manifest_excerpt: &str,
    evidence_ref: Option<&str>,
) -> OperationsBriefingSynthesis {
    OperationsBriefingSynthesis {
        summary: format!(
            "Draft ready from evidence folder manifest: {manifest_excerpt}. Model-backed synthesis and export are still pending."
        ),
        anomalies: vec![OperationsBriefingAnomaly {
            area: "Evidence review".to_string(),
            signal: "Review the accepted text files for revenue, service, risk, and owner follow-up signals.".to_string(),
            evidence_ref: evidence_ref.map(str::to_string),
        }],
        action_plan: vec![OperationsBriefingAction {
            owner: "Operations owner".to_string(),
            action:
                "Confirm the evidence set, then run model-backed synthesis for the management brief."
                    .to_string(),
            due_hint: "Next briefing cycle".to_string(),
        }],
        warnings: Vec::new(),
    }
}

pub fn render_operations_briefing_report(run: &OperationsBriefingRun) -> String {
    let mut report = String::new();
    let _ = writeln!(report, "# {}", run.title);
    let _ = writeln!(report);
    let _ = writeln!(report, "- Run ID: {}", run.id);
    let _ = writeln!(report, "- Workflow: {}", run.workflow_id);
    let _ = writeln!(
        report,
        "- Status: {}",
        operations_briefing_status_label(run.status)
    );
    let _ = writeln!(report, "- Created: {}", run.created_at.to_rfc3339());
    if run.archived_from_package {
        let _ = writeln!(report, "- Source: archived work package");
    }
    if let Some(evidence_folder_path) = &run.evidence_folder_path {
        let _ = writeln!(report, "- Evidence: {evidence_folder_path}");
    }
    if let Some(evidence_invocation_id) = run.evidence_invocation_id {
        let _ = writeln!(report, "- Evidence invocation: {evidence_invocation_id}");
    }

    write_operations_briefing_context_receipt_markdown(&mut report, &run.context_receipt);

    let _ = writeln!(report);
    let _ = writeln!(report, "## Summary");
    let _ = writeln!(report);
    let _ = writeln!(report, "{}", run.summary);

    let _ = writeln!(report);
    let _ = writeln!(report, "## Anomalies");
    let _ = writeln!(report);
    if run.anomalies.is_empty() {
        let _ = writeln!(report, "No anomalies recorded.");
    } else {
        for anomaly in &run.anomalies {
            let _ = writeln!(report, "- **{}**: {}", anomaly.area, anomaly.signal);
            if let Some(evidence_ref) = &anomaly.evidence_ref {
                let _ = writeln!(report, "  Evidence: {evidence_ref}");
            }
        }
    }

    let _ = writeln!(report);
    let _ = writeln!(report, "## Action Plan");
    let _ = writeln!(report);
    if run.action_plan.is_empty() {
        let _ = writeln!(report, "No action items recorded.");
    } else {
        for action in &run.action_plan {
            let _ = writeln!(
                report,
                "- **{}**: {} _(due: {})_",
                action.owner, action.action, action.due_hint
            );
        }
    }

    let _ = writeln!(report);
    let _ = writeln!(report, "## Warnings");
    let _ = writeln!(report);
    if run.warnings.is_empty() {
        let _ = writeln!(report, "No warnings recorded.");
    } else {
        for warning in &run.warnings {
            let _ = writeln!(report, "- {warning}");
        }
    }

    report
}

pub fn operations_briefing_report_file_name(run: &OperationsBriefingRun) -> String {
    format!("operations-briefing-{}.md", run.id)
}

pub fn render_operations_briefing_html_report(run: &OperationsBriefingRun) -> String {
    let mut html = String::new();
    let _ = writeln!(html, "<!doctype html>");
    let _ = writeln!(html, "<html lang=\"en\">");
    let _ = writeln!(html, "<head>");
    let _ = writeln!(html, "<meta charset=\"utf-8\">");
    let _ = writeln!(
        html,
        "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">"
    );
    let _ = writeln!(html, "<title>{}</title>", escape_html_text(&run.title));
    let _ = writeln!(
        html,
        "<style>body{{font-family:Arial,sans-serif;line-height:1.5;margin:40px;max-width:960px;color:#172033;background:#fff}}h1{{font-size:28px;margin-bottom:8px}}h2{{font-size:18px;margin-top:28px;border-bottom:1px solid #d8dee8;padding-bottom:6px}}.meta{{color:#526071;font-size:13px}}li{{margin:8px 0}}.warning{{color:#7a4a00}}</style>"
    );
    let _ = writeln!(html, "</head>");
    let _ = writeln!(html, "<body>");
    let _ = writeln!(html, "<h1>{}</h1>", escape_html_text(&run.title));
    let _ = writeln!(html, "<section class=\"meta\">");
    let _ = writeln!(html, "<p>Run ID: {}</p>", run.id);
    let _ = writeln!(
        html,
        "<p>Workflow: {}</p>",
        escape_html_text(&run.workflow_id)
    );
    let _ = writeln!(
        html,
        "<p>Status: {}</p>",
        operations_briefing_status_label(run.status)
    );
    let _ = writeln!(html, "<p>Created: {}</p>", run.created_at.to_rfc3339());
    if run.archived_from_package {
        let _ = writeln!(html, "<p>Source: archived work package</p>");
    }
    if let Some(evidence_folder_path) = &run.evidence_folder_path {
        let _ = writeln!(
            html,
            "<p>Evidence: {}</p>",
            escape_html_text(evidence_folder_path)
        );
    }
    if let Some(evidence_invocation_id) = run.evidence_invocation_id {
        let _ = writeln!(html, "<p>Evidence invocation: {evidence_invocation_id}</p>");
    }
    let _ = writeln!(html, "</section>");

    write_operations_briefing_context_receipt_html(&mut html, &run.context_receipt);

    let _ = writeln!(html, "<h2>Summary</h2>");
    let _ = writeln!(html, "<p>{}</p>", escape_html_text(&run.summary));

    let _ = writeln!(html, "<h2>Anomalies</h2>");
    if run.anomalies.is_empty() {
        let _ = writeln!(html, "<p>No anomalies recorded.</p>");
    } else {
        let _ = writeln!(html, "<ul>");
        for anomaly in &run.anomalies {
            let evidence = anomaly
                .evidence_ref
                .as_ref()
                .map(|evidence_ref| format!(" Evidence: {}", escape_html_text(evidence_ref)))
                .unwrap_or_default();
            let _ = writeln!(
                html,
                "<li><strong>{}</strong>: {}{}</li>",
                escape_html_text(&anomaly.area),
                escape_html_text(&anomaly.signal),
                evidence
            );
        }
        let _ = writeln!(html, "</ul>");
    }

    let _ = writeln!(html, "<h2>Action Plan</h2>");
    if run.action_plan.is_empty() {
        let _ = writeln!(html, "<p>No action items recorded.</p>");
    } else {
        let _ = writeln!(html, "<ul>");
        for action in &run.action_plan {
            let _ = writeln!(
                html,
                "<li><strong>{}</strong>: {} <em>due: {}</em></li>",
                escape_html_text(&action.owner),
                escape_html_text(&action.action),
                escape_html_text(&action.due_hint)
            );
        }
        let _ = writeln!(html, "</ul>");
    }

    let _ = writeln!(html, "<h2>Warnings</h2>");
    if run.warnings.is_empty() {
        let _ = writeln!(html, "<p>No warnings recorded.</p>");
    } else {
        let _ = writeln!(html, "<ul>");
        for warning in &run.warnings {
            let _ = writeln!(
                html,
                "<li class=\"warning\">{}</li>",
                escape_html_text(warning)
            );
        }
        let _ = writeln!(html, "</ul>");
    }

    let _ = writeln!(html, "</body>");
    let _ = writeln!(html, "</html>");
    html
}

fn write_operations_briefing_context_receipt_markdown(
    report: &mut String,
    receipt: &OperationsBriefingContextReceipt,
) {
    let _ = writeln!(report);
    let _ = writeln!(report, "## Context Receipt");
    let _ = writeln!(report);
    let _ = writeln!(report, "- User intent: {}", receipt.user_intent);
    let _ = writeln!(report, "- Loop mode: {}", receipt.loop_mode);
    let _ = writeln!(report, "- Workflow policy: {}", receipt.workflow_policy);
    let _ = writeln!(report, "- Model route: {}", receipt.model_route);
    let _ = writeln!(report, "- Thinking level: {}", receipt.thinking_level);
    let _ = writeln!(report, "- Token / cache: {}", receipt.token_cache_state);
    write_markdown_list(report, "Selected evidence", &receipt.selected_evidence);
    write_markdown_list(report, "Selected memories", &receipt.selected_memories);
    write_markdown_list(report, "Validation", &receipt.validation_results);
    write_markdown_list(
        report,
        "Intentional omissions",
        &receipt.intentional_omissions,
    );
}

fn write_markdown_list(report: &mut String, title: &str, items: &[String]) {
    let _ = writeln!(report);
    let _ = writeln!(report, "### {title}");
    if items.is_empty() {
        let _ = writeln!(report, "- None");
        return;
    }
    for item in items {
        let _ = writeln!(report, "- {item}");
    }
}

fn write_operations_briefing_context_receipt_html(
    html: &mut String,
    receipt: &OperationsBriefingContextReceipt,
) {
    let _ = writeln!(html, "<h2>Context Receipt</h2>");
    let _ = writeln!(html, "<dl>");
    write_html_definition(html, "User intent", &receipt.user_intent);
    write_html_definition(html, "Loop mode", &receipt.loop_mode);
    write_html_definition(html, "Workflow policy", &receipt.workflow_policy);
    write_html_definition(html, "Model route", &receipt.model_route);
    write_html_definition(html, "Thinking level", &receipt.thinking_level);
    write_html_definition(html, "Token / cache", &receipt.token_cache_state);
    let _ = writeln!(html, "</dl>");
    write_html_list(html, "Selected evidence", &receipt.selected_evidence);
    write_html_list(html, "Selected memories", &receipt.selected_memories);
    write_html_list(html, "Validation", &receipt.validation_results);
    write_html_list(
        html,
        "Intentional omissions",
        &receipt.intentional_omissions,
    );
}

fn write_html_definition(html: &mut String, label: &str, value: &str) {
    let _ = writeln!(
        html,
        "<dt>{}</dt><dd>{}</dd>",
        escape_html_text(label),
        escape_html_text(value)
    );
}

fn write_html_list(html: &mut String, title: &str, items: &[String]) {
    let _ = writeln!(html, "<h3>{}</h3>", escape_html_text(title));
    if items.is_empty() {
        let _ = writeln!(html, "<p>None</p>");
        return;
    }
    let _ = writeln!(html, "<ul>");
    for item in items {
        let _ = writeln!(html, "<li>{}</li>", escape_html_text(item));
    }
    let _ = writeln!(html, "</ul>");
}

pub fn operations_briefing_html_report_file_name(run: &OperationsBriefingRun) -> String {
    format!("operations-briefing-{}.html", run.id)
}

pub fn render_operations_briefing_pdf_report(run: &OperationsBriefingRun) -> Vec<u8> {
    let lines = operations_briefing_pdf_lines(run);
    let pages = paginate_pdf_report(lines);
    build_simple_pdf_document(&pages)
}

pub fn operations_briefing_pdf_report_file_name(run: &OperationsBriefingRun) -> String {
    format!("operations-briefing-{}.pdf", run.id)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PdfReportFont {
    Regular,
    Bold,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PdfReportLine {
    text: String,
    font: PdfReportFont,
    size: u8,
}

fn operations_briefing_pdf_lines(run: &OperationsBriefingRun) -> Vec<PdfReportLine> {
    let mut lines = Vec::new();
    push_pdf_line(&mut lines, &run.title, PdfReportFont::Bold, 18);
    push_pdf_line(
        &mut lines,
        &format!("Run ID: {}", run.id),
        PdfReportFont::Regular,
        10,
    );
    push_pdf_line(
        &mut lines,
        &format!("Workflow: {}", run.workflow_id),
        PdfReportFont::Regular,
        10,
    );
    push_pdf_line(
        &mut lines,
        &format!("Status: {}", operations_briefing_status_label(run.status)),
        PdfReportFont::Regular,
        10,
    );
    push_pdf_line(
        &mut lines,
        &format!("Created: {}", run.created_at.to_rfc3339()),
        PdfReportFont::Regular,
        10,
    );
    if run.archived_from_package {
        push_pdf_line(
            &mut lines,
            "Source: archived work package",
            PdfReportFont::Regular,
            10,
        );
    }
    if let Some(evidence_folder_path) = &run.evidence_folder_path {
        push_pdf_wrapped(
            &mut lines,
            "Evidence: ",
            evidence_folder_path,
            PdfReportFont::Regular,
            10,
        );
    }
    if let Some(evidence_invocation_id) = run.evidence_invocation_id {
        push_pdf_line(
            &mut lines,
            &format!("Evidence invocation: {evidence_invocation_id}"),
            PdfReportFont::Regular,
            10,
        );
    }

    push_pdf_blank(&mut lines);
    push_pdf_section(&mut lines, "Summary");
    push_pdf_wrapped(&mut lines, "", &run.summary, PdfReportFont::Regular, 11);

    push_pdf_blank(&mut lines);
    push_pdf_section(&mut lines, "Anomalies");
    if run.anomalies.is_empty() {
        push_pdf_line(
            &mut lines,
            "No anomalies recorded.",
            PdfReportFont::Regular,
            11,
        );
    } else {
        for anomaly in &run.anomalies {
            let evidence = anomaly
                .evidence_ref
                .as_ref()
                .map(|evidence_ref| format!(" Evidence: {evidence_ref}"))
                .unwrap_or_default();
            push_pdf_wrapped(
                &mut lines,
                "- ",
                &format!("{}: {}{}", anomaly.area, anomaly.signal, evidence),
                PdfReportFont::Regular,
                11,
            );
        }
    }

    push_pdf_blank(&mut lines);
    push_pdf_section(&mut lines, "Action Plan");
    if run.action_plan.is_empty() {
        push_pdf_line(
            &mut lines,
            "No action items recorded.",
            PdfReportFont::Regular,
            11,
        );
    } else {
        for action in &run.action_plan {
            push_pdf_wrapped(
                &mut lines,
                "- ",
                &format!(
                    "{}: {} (due: {})",
                    action.owner, action.action, action.due_hint
                ),
                PdfReportFont::Regular,
                11,
            );
        }
    }

    push_pdf_blank(&mut lines);
    push_pdf_section(&mut lines, "Warnings");
    if run.warnings.is_empty() {
        push_pdf_line(
            &mut lines,
            "No warnings recorded.",
            PdfReportFont::Regular,
            11,
        );
    } else {
        for warning in &run.warnings {
            push_pdf_wrapped(&mut lines, "- ", warning, PdfReportFont::Regular, 11);
        }
    }

    lines
}

fn push_pdf_section(lines: &mut Vec<PdfReportLine>, text: &str) {
    push_pdf_line(lines, text, PdfReportFont::Bold, 13);
}

fn push_pdf_blank(lines: &mut Vec<PdfReportLine>) {
    lines.push(PdfReportLine {
        text: String::new(),
        font: PdfReportFont::Regular,
        size: 8,
    });
}

fn push_pdf_line(lines: &mut Vec<PdfReportLine>, text: &str, font: PdfReportFont, size: u8) {
    lines.push(PdfReportLine {
        text: sanitize_pdf_text(text),
        font,
        size,
    });
}

fn push_pdf_wrapped(
    lines: &mut Vec<PdfReportLine>,
    prefix: &str,
    text: &str,
    font: PdfReportFont,
    size: u8,
) {
    let max_chars = 92usize.saturating_sub(prefix.len());
    let text = sanitize_pdf_text(text);
    let mut current = String::new();

    for word in text.split_whitespace() {
        let separator_len = usize::from(!current.is_empty());
        if !current.is_empty() && current.len() + separator_len + word.len() > max_chars {
            push_pdf_line(lines, &format!("{prefix}{current}"), font, size);
            current.clear();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }

    if current.is_empty() {
        push_pdf_line(lines, prefix.trim_end(), font, size);
    } else {
        push_pdf_line(lines, &format!("{prefix}{current}"), font, size);
    }
}

fn paginate_pdf_report(lines: Vec<PdfReportLine>) -> Vec<Vec<PdfReportLine>> {
    const MAX_LINES_PER_PAGE: usize = 48;

    let mut pages = Vec::new();
    let mut current_page = Vec::new();
    for line in lines {
        if current_page.len() >= MAX_LINES_PER_PAGE {
            pages.push(current_page);
            current_page = Vec::new();
        }
        current_page.push(line);
    }
    if !current_page.is_empty() {
        pages.push(current_page);
    }
    if pages.is_empty() {
        pages.push(Vec::new());
    }
    pages
}

fn build_simple_pdf_document(pages: &[Vec<PdfReportLine>]) -> Vec<u8> {
    let mut objects = Vec::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());

    let kids = (0..pages.len())
        .map(|index| format!("{} 0 R", 5 + index * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!(
        "<< /Type /Pages /Kids [{kids}] /Count {} >>",
        pages.len()
    ));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_string());

    for (index, page_lines) in pages.iter().enumerate() {
        let page_object_id = 5 + index * 2;
        let content_object_id = page_object_id + 1;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {content_object_id} 0 R >>"
        ));

        let stream = build_pdf_page_stream(page_lines, index + 1, pages.len());
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}endstream",
            stream.len(),
            stream
        ));
    }

    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        let _ = writeln!(pdf, "{} 0 obj", index + 1);
        let _ = writeln!(pdf, "{object}");
        let _ = writeln!(pdf, "endobj");
    }

    let xref_offset = pdf.len();
    let _ = writeln!(pdf, "xref");
    let _ = writeln!(pdf, "0 {}", objects.len() + 1);
    let _ = writeln!(pdf, "0000000000 65535 f ");
    for offset in offsets {
        let _ = writeln!(pdf, "{offset:010} 00000 n ");
    }
    let _ = writeln!(pdf, "trailer");
    let _ = writeln!(pdf, "<< /Size {} /Root 1 0 R >>", objects.len() + 1);
    let _ = writeln!(pdf, "startxref");
    let _ = writeln!(pdf, "{xref_offset}");
    let _ = writeln!(pdf, "%%EOF");
    pdf.into_bytes()
}

fn build_pdf_page_stream(
    page_lines: &[PdfReportLine],
    page_number: usize,
    page_count: usize,
) -> String {
    let mut stream = String::new();
    let mut y = 740.0f32;
    for line in page_lines {
        if line.text.is_empty() {
            y -= 10.0;
            continue;
        }
        let font_name = match line.font {
            PdfReportFont::Regular => "F1",
            PdfReportFont::Bold => "F2",
        };
        let escaped_text = escape_pdf_literal(&line.text);
        let _ = writeln!(
            stream,
            "BT /{font_name} {} Tf 1 0 0 1 54 {:.2} Tm ({escaped_text}) Tj ET",
            line.size, y
        );
        y -= if line.size >= 18 {
            24.0
        } else if line.size >= 13 {
            18.0
        } else {
            15.0
        };
    }

    let footer = format!("Page {page_number} of {page_count}");
    let _ = writeln!(
        stream,
        "BT /F1 9 Tf 1 0 0 1 54 36 Tm ({}) Tj ET",
        escape_pdf_literal(&footer)
    );
    stream
}

fn sanitize_pdf_text(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '\r' | '\n' | '\t' => ' ',
            character if character.is_ascii() && !character.is_control() => character,
            _ => '?',
        })
        .collect()
}

fn escape_pdf_literal(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '(' => "\\(".chars().collect::<Vec<_>>(),
            ')' => "\\)".chars().collect::<Vec<_>>(),
            _ => vec![character],
        })
        .collect()
}

fn operations_briefing_status_label(status: OperationsBriefingRunStatus) -> &'static str {
    match status {
        OperationsBriefingRunStatus::PendingApproval => "pending approval",
        OperationsBriefingRunStatus::DraftReady => "draft ready",
        OperationsBriefingRunStatus::Failed => "failed",
    }
}

fn escape_html_text(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&#39;".to_string(),
            _ => character.to_string(),
        })
        .collect()
}

fn normalize_template_seed_folder_path(value: &str) -> Result<String, String> {
    let normalized = value.trim().to_string();
    if normalized.is_empty() {
        return Err("evidence folder path is required".to_string());
    }

    Ok(normalized)
}

fn normalize_template_file_name(file_name: &str) -> Result<String, String> {
    let file_name = file_name.trim();
    if file_name.is_empty() {
        return Err("template file name is required".to_string());
    }
    let path = Path::new(file_name);
    if path.components().count() != 1
        || path.file_name().and_then(|value| value.to_str()) != Some(file_name)
    {
        return Err("template file name must not include directories".to_string());
    }

    Ok(file_name.to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use crate::kernel::capability::{
        CapabilityInvocationStatus, EvidenceFolderClient, EvidenceFolderFile,
    };
    use crate::kernel::models::AccessMode;
    use crate::kernel::workflow::{
        operations_briefing_html_report_file_name, operations_briefing_pdf_report_file_name,
        operations_briefing_workflow_template_package, render_operations_briefing_html_report,
        render_operations_briefing_pdf_report, render_operations_briefing_report,
        run_operations_briefing, run_operations_briefing_template_seed,
        run_operations_briefing_with_synthesizer, LocalOperationsBriefingTemplateSeeder,
        OperationsBriefingAction, OperationsBriefingAnomaly, OperationsBriefingContextReceipt,
        OperationsBriefingRequest, OperationsBriefingRun, OperationsBriefingRunStatus,
        OperationsBriefingSynthesis, OperationsBriefingSynthesizer,
        OperationsBriefingTemplateSeedRequest, OperationsBriefingTemplateSeedResult,
        OperationsBriefingTemplateSeeder, OPERATIONS_BRIEFING_WORKFLOW_ID,
    };

    struct FakeEvidenceFolderClient {
        calls: Cell<u32>,
    }

    struct FakeOperationsBriefingSynthesizer {
        failing: bool,
        calls: Cell<u32>,
    }

    struct RepairableOperationsBriefingSynthesizer {
        calls: Cell<u32>,
        repair_prompt_seen: Cell<bool>,
    }

    struct FakeOperationsBriefingTemplateSeeder {
        calls: Cell<u32>,
    }

    impl FakeEvidenceFolderClient {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
            }
        }
    }

    impl FakeOperationsBriefingTemplateSeeder {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
            }
        }
    }

    impl EvidenceFolderClient for FakeEvidenceFolderClient {
        fn read_text_files(&self, folder_path: &str) -> Result<Vec<EvidenceFolderFile>, String> {
            self.calls.set(self.calls.get() + 1);
            Ok(vec![
                EvidenceFolderFile {
                    path: format!("{folder_path}\\revenue.md"),
                    title: "revenue.md".to_string(),
                    text: "Room revenue improved by 6 percent.".to_string(),
                    bytes: 35,
                    encoding: "utf-8".to_string(),
                },
                EvidenceFolderFile {
                    path: format!("{folder_path}/complaints.txt"),
                    title: "complaints.txt".to_string(),
                    text: "Guest complaints increased in the west wing.".to_string(),
                    bytes: 48,
                    encoding: "utf-8".to_string(),
                },
            ])
        }
    }

    impl OperationsBriefingSynthesizer for FakeOperationsBriefingSynthesizer {
        fn synthesize_briefing(
            &self,
            manifest_excerpt: &str,
            evidence_ref: Option<&str>,
        ) -> Result<OperationsBriefingSynthesis, String> {
            let next_call = self.calls.get() + 1;
            self.calls.set(next_call);
            if self.failing {
                return Err(if next_call == 1 {
                    "model synthesis unavailable"
                } else {
                    "repair synthesis unavailable"
                }
                .to_string());
            }

            Ok(OperationsBriefingSynthesis {
                summary: format!("Model summary from {manifest_excerpt}"),
                anomalies: vec![OperationsBriefingAnomaly {
                    area: "Guest experience".to_string(),
                    signal: "Guest complaints increased in the west wing.".to_string(),
                    evidence_ref: evidence_ref.map(str::to_string),
                }],
                action_plan: vec![OperationsBriefingAction {
                    owner: "Rooms".to_string(),
                    action: "Inspect west wing complaint drivers.".to_string(),
                    due_hint: "48 hours".to_string(),
                }],
                warnings: Vec::new(),
            })
        }
    }

    impl RepairableOperationsBriefingSynthesizer {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
                repair_prompt_seen: Cell::new(false),
            }
        }
    }

    impl OperationsBriefingSynthesizer for RepairableOperationsBriefingSynthesizer {
        fn synthesize_briefing(
            &self,
            manifest_excerpt: &str,
            evidence_ref: Option<&str>,
        ) -> Result<OperationsBriefingSynthesis, String> {
            let next_call = self.calls.get() + 1;
            self.calls.set(next_call);
            if next_call == 1 {
                return Err("model returned non-json briefing".to_string());
            }

            self.repair_prompt_seen.set(
                manifest_excerpt.contains("Repair failed Operations Briefing synthesis.")
                    && manifest_excerpt
                        .contains("Validation failure: model returned non-json briefing")
                    && manifest_excerpt.contains("Evidence manifest excerpt:"),
            );

            Ok(OperationsBriefingSynthesis {
                summary: "Repaired model summary from compact context.".to_string(),
                anomalies: vec![OperationsBriefingAnomaly {
                    area: "Evidence review".to_string(),
                    signal: "Repair retry used the evidence manifest only.".to_string(),
                    evidence_ref: evidence_ref.map(str::to_string),
                }],
                action_plan: vec![OperationsBriefingAction {
                    owner: "Operations".to_string(),
                    action: "Review the repaired briefing before sharing.".to_string(),
                    due_hint: "Next briefing cycle".to_string(),
                }],
                warnings: Vec::new(),
            })
        }
    }

    impl OperationsBriefingTemplateSeeder for FakeOperationsBriefingTemplateSeeder {
        fn seed_templates(
            &self,
            target_dir: &str,
        ) -> Result<OperationsBriefingTemplateSeedResult, String> {
            self.calls.set(self.calls.get() + 1);
            Ok(OperationsBriefingTemplateSeedResult {
                target_dir: target_dir.to_string(),
                written_files: vec!["revenue.md".to_string()],
                skipped_files: Vec::new(),
            })
        }
    }

    #[test]
    fn operations_briefing_generates_draft_from_evidence_manifest() {
        let client = FakeEvidenceFolderClient::new();
        let outcome = run_operations_briefing(
            OperationsBriefingRequest {
                access_mode: AccessMode::AskOnRisk,
                evidence_folder_path: "fixtures/evidence".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("operations briefing succeeds");

        assert_eq!(outcome.run.status, OperationsBriefingRunStatus::DraftReady);
        assert_eq!(
            outcome.evidence_invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.run.evidence_folder_path.as_deref(),
            Some("fixtures/evidence")
        );
        assert_eq!(
            outcome.run.evidence_invocation_id,
            Some(outcome.evidence_invocation.id)
        );
        assert!(outcome.run.summary.contains("2 text files"));
        assert_eq!(outcome.run.anomalies.len(), 1);
        assert_eq!(outcome.run.action_plan.len(), 1);
        assert_eq!(client.calls.get(), 1);
    }

    #[test]
    fn operations_briefing_records_context_receipt_for_draft_run() {
        let client = FakeEvidenceFolderClient::new();
        let outcome = run_operations_briefing(
            OperationsBriefingRequest {
                access_mode: AccessMode::AskOnRisk,
                evidence_folder_path: "fixtures/evidence".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("operations briefing succeeds");

        let run_json = serde_json::to_value(&outcome.run).expect("run serializes");
        let receipt = run_json
            .get("context_receipt")
            .expect("context receipt is serialized");

        assert_eq!(
            receipt["selected_evidence"][0],
            "2 text files, 83 bytes: revenue.md (utf-8 text, 35 bytes), complaints.txt (utf-8 text, 48 bytes)"
        );
        assert_eq!(receipt["loop_mode"], "workflow_pack_run");
        assert_eq!(
            receipt["workflow_policy"],
            "Operations Briefing v1: FileRead evidence folder only; raw file bodies omitted; deterministic validators first; bounded model repair retry budget 1."
        );
        assert_eq!(
            receipt["selected_memories"]
                .as_array()
                .expect("selected memories is an array")
                .len(),
            0
        );
        assert_eq!(receipt["model_route"], "local deterministic synthesis");
        assert!(receipt["validation_results"]
            .as_array()
            .expect("validation results is an array")
            .iter()
            .any(|result| result == "evidence folder ingestion succeeded"));
        assert!(receipt["intentional_omissions"]
            .as_array()
            .expect("intentional omissions is an array")
            .iter()
            .any(|omission| omission
                == "Raw evidence file bodies are not stored in the context receipt."));
    }

    #[test]
    fn operations_briefing_uses_model_synthesis_after_evidence_manifest_succeeds() {
        let client = FakeEvidenceFolderClient::new();
        let synthesizer = FakeOperationsBriefingSynthesizer {
            failing: false,
            calls: Cell::new(0),
        };
        let outcome = run_operations_briefing_with_synthesizer(
            OperationsBriefingRequest {
                access_mode: AccessMode::AskOnRisk,
                evidence_folder_path: "fixtures/evidence".to_string(),
                approval_granted: false,
            },
            &client,
            &synthesizer,
        )
        .expect("operations briefing succeeds");

        assert_eq!(outcome.run.status, OperationsBriefingRunStatus::DraftReady);
        assert!(outcome.run.summary.starts_with("Model summary"));
        assert_eq!(outcome.run.anomalies[0].area, "Guest experience");
        assert_eq!(outcome.run.action_plan[0].owner, "Rooms");
        assert!(outcome.run.warnings.is_empty());
        assert_eq!(synthesizer.calls.get(), 1);
    }

    #[test]
    fn operations_briefing_keeps_deterministic_draft_when_model_synthesis_fails() {
        let client = FakeEvidenceFolderClient::new();
        let synthesizer = FakeOperationsBriefingSynthesizer {
            failing: true,
            calls: Cell::new(0),
        };
        let outcome = run_operations_briefing_with_synthesizer(
            OperationsBriefingRequest {
                access_mode: AccessMode::AskOnRisk,
                evidence_folder_path: "fixtures/evidence".to_string(),
                approval_granted: false,
            },
            &client,
            &synthesizer,
        )
        .expect("operations briefing falls back");

        assert_eq!(outcome.run.status, OperationsBriefingRunStatus::DraftReady);
        assert!(outcome
            .run
            .summary
            .contains("Draft ready from evidence folder manifest"));
        assert_eq!(synthesizer.calls.get(), 2);
        assert_eq!(
            outcome.run.warnings,
            vec![
                "model-backed synthesis failed: model synthesis unavailable",
                "bounded repair loop failed: repair synthesis unavailable",
            ]
        );
    }

    #[test]
    fn operations_briefing_repairs_failed_model_synthesis_with_bounded_retry() {
        let client = FakeEvidenceFolderClient::new();
        let synthesizer = RepairableOperationsBriefingSynthesizer::new();
        let outcome = run_operations_briefing_with_synthesizer(
            OperationsBriefingRequest {
                access_mode: AccessMode::AskOnRisk,
                evidence_folder_path: "fixtures/evidence".to_string(),
                approval_granted: false,
            },
            &client,
            &synthesizer,
        )
        .expect("operations briefing repairs model synthesis");

        assert_eq!(synthesizer.calls.get(), 2);
        assert!(synthesizer.repair_prompt_seen.get());
        assert_eq!(outcome.run.status, OperationsBriefingRunStatus::DraftReady);
        assert_eq!(
            outcome.run.summary,
            "Repaired model summary from compact context."
        );
        assert_eq!(
            outcome.run.anomalies[0].evidence_ref.as_deref(),
            Some("fixtures/evidence")
        );
        assert!(outcome
            .run
            .warnings
            .iter()
            .any(|warning| warning
                .contains("bounded repair loop retried model synthesis after failure")));
        assert!(outcome
            .run
            .context_receipt
            .validation_results
            .iter()
            .any(|result| result == "bounded repair loop retried model synthesis once"));
        assert!(outcome
            .run
            .context_receipt
            .validation_results
            .iter()
            .any(|result| result == "bounded repair loop succeeded"));
        assert!(!outcome
            .run
            .context_receipt
            .intentional_omissions
            .iter()
            .any(|omission| omission.contains("No repair loop was run")));
    }

    #[test]
    fn operations_briefing_waits_for_evidence_approval_without_scanning() {
        let client = FakeEvidenceFolderClient::new();
        let outcome = run_operations_briefing(
            OperationsBriefingRequest {
                access_mode: AccessMode::AskEveryStep,
                evidence_folder_path: "fixtures/evidence".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("operations briefing returns pending result");

        assert_eq!(
            outcome.run.status,
            OperationsBriefingRunStatus::PendingApproval
        );
        assert_eq!(
            outcome.evidence_invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(outcome.run.anomalies.len(), 0);
        assert_eq!(outcome.run.action_plan.len(), 0);
        assert_eq!(client.calls.get(), 0);
    }

    #[test]
    fn operations_briefing_report_renders_markdown_with_traceable_sections() {
        let run = OperationsBriefingRun {
            id: uuid::Uuid::new_v4(),
            workflow_id: OPERATIONS_BRIEFING_WORKFLOW_ID.to_string(),
            status: OperationsBriefingRunStatus::DraftReady,
            archived_from_package: false,
            evidence_folder_path: Some("fixtures/evidence".to_string()),
            evidence_invocation_id: Some(uuid::Uuid::new_v4()),
            title: "Operations Briefing Draft".to_string(),
            summary: "Revenue improved while west wing complaints increased.".to_string(),
            anomalies: vec![OperationsBriefingAnomaly {
                area: "Guest experience".to_string(),
                signal: "West wing complaints increased.".to_string(),
                evidence_ref: Some("fixtures/evidence".to_string()),
            }],
            action_plan: vec![OperationsBriefingAction {
                owner: "Rooms".to_string(),
                action: "Inspect west wing service recovery drivers.".to_string(),
                due_hint: "48 hours".to_string(),
            }],
            warnings: vec!["model-backed synthesis failed: timeout".to_string()],
            context_receipt: OperationsBriefingContextReceipt {
                user_intent: "Draft an Operations Briefing from selected local evidence."
                    .to_string(),
                loop_mode: "workflow_pack_run".to_string(),
                workflow_policy: "Operations Briefing v1: FileRead evidence folder only."
                    .to_string(),
                selected_evidence: vec!["2 text files selected".to_string()],
                selected_memories: Vec::new(),
                model_route: "DeepSeek / Pro".to_string(),
                thinking_level: "standard".to_string(),
                token_cache_state: "deepseek-v4: cache miss; 100 tokens".to_string(),
                validation_results: vec!["evidence folder ingestion succeeded".to_string()],
                intentional_omissions: vec![
                    "Raw evidence file bodies are not stored in the context receipt.".to_string(),
                ],
            },
            created_at: chrono::Utc::now(),
        };

        let report = render_operations_briefing_report(&run);

        assert!(report.starts_with("# Operations Briefing Draft"));
        assert!(report.contains("## Summary"));
        assert!(report.contains("Revenue improved"));
        assert!(report.contains("## Anomalies"));
        assert!(report.contains("Guest experience"));
        assert!(report.contains("## Action Plan"));
        assert!(report.contains("Rooms"));
        assert!(report.contains("## Warnings"));
        assert!(report.contains("model-backed synthesis failed"));
        assert!(report.contains("fixtures/evidence"));
        assert!(report.contains("## Context Receipt"));
        assert!(report.contains("- Loop mode: workflow_pack_run"));
        assert!(report
            .contains("- Workflow policy: Operations Briefing v1: FileRead evidence folder only."));
        assert!(report.contains("- Model route: DeepSeek / Pro"));
        assert!(report.contains("- Token / cache: deepseek-v4: cache miss; 100 tokens"));
        assert!(report.contains("- evidence folder ingestion succeeded"));
    }

    #[test]
    fn operations_briefing_html_report_escapes_content_and_renders_sections() {
        let run = OperationsBriefingRun {
            id: uuid::Uuid::new_v4(),
            workflow_id: OPERATIONS_BRIEFING_WORKFLOW_ID.to_string(),
            status: OperationsBriefingRunStatus::DraftReady,
            archived_from_package: false,
            evidence_folder_path: Some("fixtures/<evidence>".to_string()),
            evidence_invocation_id: None,
            title: "Operations <Briefing>".to_string(),
            summary: "Revenue <script>alert(1)</script> improved.".to_string(),
            anomalies: vec![OperationsBriefingAnomaly {
                area: "Guest & experience".to_string(),
                signal: "Complaints < increased.".to_string(),
                evidence_ref: Some("fixtures/<evidence>".to_string()),
            }],
            action_plan: vec![OperationsBriefingAction {
                owner: "Rooms".to_string(),
                action: "Inspect <west wing>.".to_string(),
                due_hint: "48 hours".to_string(),
            }],
            warnings: vec!["Use <caution>.".to_string()],
            context_receipt: OperationsBriefingContextReceipt {
                user_intent: "Draft <briefing>.".to_string(),
                loop_mode: "workflow_pack_run".to_string(),
                workflow_policy: "Policy keeps <raw evidence> out of receipts.".to_string(),
                selected_evidence: vec!["2 <files> selected".to_string()],
                selected_memories: Vec::new(),
                model_route: "DeepSeek / Pro".to_string(),
                thinking_level: "standard".to_string(),
                token_cache_state: "cache <miss>".to_string(),
                validation_results: vec!["validation <passed>".to_string()],
                intentional_omissions: vec!["raw <files> omitted".to_string()],
            },
            created_at: chrono::Utc::now(),
        };

        let html = render_operations_briefing_html_report(&run);

        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<h1>Operations &lt;Briefing&gt;</h1>"));
        assert!(html.contains("<h2>Summary</h2>"));
        assert!(html.contains("Revenue &lt;script&gt;alert(1)&lt;/script&gt; improved."));
        assert!(html.contains("Guest &amp; experience"));
        assert!(html.contains("fixtures/&lt;evidence&gt;"));
        assert!(html.contains("<h2>Action Plan</h2>"));
        assert!(html.contains("<h2>Warnings</h2>"));
        assert!(html.contains("<h2>Context Receipt</h2>"));
        assert!(html.contains("workflow_pack_run"));
        assert!(html.contains("Policy keeps &lt;raw evidence&gt; out of receipts."));
        assert!(html.contains("validation &lt;passed&gt;"));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn operations_briefing_html_report_file_name_uses_html_extension() {
        let run = OperationsBriefingRun {
            id: uuid::Uuid::new_v4(),
            workflow_id: OPERATIONS_BRIEFING_WORKFLOW_ID.to_string(),
            status: OperationsBriefingRunStatus::DraftReady,
            archived_from_package: false,
            evidence_folder_path: None,
            evidence_invocation_id: None,
            title: "Operations Briefing".to_string(),
            summary: "Revenue improved.".to_string(),
            anomalies: Vec::new(),
            action_plan: Vec::new(),
            warnings: Vec::new(),
            context_receipt: Default::default(),
            created_at: chrono::Utc::now(),
        };

        assert_eq!(
            operations_briefing_html_report_file_name(&run),
            format!("operations-briefing-{}.html", run.id)
        );
    }

    #[test]
    fn operations_briefing_pdf_report_renders_valid_pdf_bytes() {
        let run = OperationsBriefingRun {
            id: uuid::Uuid::new_v4(),
            workflow_id: OPERATIONS_BRIEFING_WORKFLOW_ID.to_string(),
            status: OperationsBriefingRunStatus::DraftReady,
            archived_from_package: false,
            evidence_folder_path: Some("fixtures/evidence".to_string()),
            evidence_invocation_id: None,
            title: "Operations Briefing".to_string(),
            summary: "Revenue improved and service follow-up is ready.".to_string(),
            anomalies: vec![OperationsBriefingAnomaly {
                area: "Guest experience".to_string(),
                signal: "Complaints increased.".to_string(),
                evidence_ref: Some("fixtures/evidence".to_string()),
            }],
            action_plan: vec![OperationsBriefingAction {
                owner: "Rooms".to_string(),
                action: "Inspect west wing service recovery drivers.".to_string(),
                due_hint: "48 hours".to_string(),
            }],
            warnings: vec!["model-backed synthesis failed: timeout".to_string()],
            context_receipt: Default::default(),
            created_at: chrono::Utc::now(),
        };

        let pdf = render_operations_briefing_pdf_report(&run);

        assert!(pdf.starts_with(b"%PDF-1.4\n"));
        assert!(pdf.ends_with(b"%%EOF\n"));
        assert!(pdf
            .windows("Operations Briefing".len())
            .any(|window| { window == "Operations Briefing".as_bytes() }));
        assert_eq!(
            operations_briefing_pdf_report_file_name(&run),
            format!("operations-briefing-{}.pdf", run.id)
        );
    }

    #[test]
    fn operations_briefing_template_seed_waits_for_filewrite_approval_without_writing() {
        let seeder = FakeOperationsBriefingTemplateSeeder::new();

        let outcome = run_operations_briefing_template_seed(
            OperationsBriefingTemplateSeedRequest {
                access_mode: AccessMode::AskOnRisk,
                evidence_folder_path: "fixtures/evidence".to_string(),
                approval_granted: false,
            },
            &seeder,
        )
        .expect("template seed returns pending");

        assert_eq!(seeder.calls.get(), 0);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("fixtures/evidence")
        );
    }

    #[test]
    fn operations_briefing_template_seed_writes_templates_without_overwriting_existing_files() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let revenue_path = temp_dir.path().join("revenue.md");
        std::fs::write(&revenue_path, "custom revenue notes").expect("write custom revenue");
        let seeder = LocalOperationsBriefingTemplateSeeder;

        let result = seeder
            .seed_templates(temp_dir.path().to_string_lossy().as_ref())
            .expect("templates seed");

        assert!(result.skipped_files.contains(&"revenue.md".to_string()));
        assert!(result
            .written_files
            .contains(&"guest-experience.md".to_string()));
        assert_eq!(
            std::fs::read_to_string(revenue_path).expect("revenue preserved"),
            "custom revenue notes"
        );
        assert!(
            std::fs::read_to_string(temp_dir.path().join("guest-experience.md"))
                .expect("guest template written")
                .contains("# Guest Experience Evidence")
        );
    }

    #[test]
    fn operations_briefing_template_package_describes_blank_operator_templates() {
        let package = operations_briefing_workflow_template_package();

        assert_eq!(
            package.description,
            "Blank operator templates for the Operations Briefing workflow."
        );
    }

    #[test]
    fn archive_replay_legacy_run_json_defaults_to_local_run() {
        let run: OperationsBriefingRun = serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "workflow_id": OPERATIONS_BRIEFING_WORKFLOW_ID,
            "status": "draft_ready",
            "evidence_folder_path": "fixtures/evidence",
            "evidence_invocation_id": uuid::Uuid::new_v4(),
            "title": "Operations Briefing Draft",
            "summary": "Legacy run payload.",
            "anomalies": [],
            "action_plan": [],
            "warnings": [],
            "created_at": chrono::Utc::now()
        }))
        .expect("legacy run deserializes");

        assert!(!run.archived_from_package);
    }
}
