use crate::kernel::computer_use::{
    computer_use_backend_status_for_strategy_with_codex_bridge_config, ComputerUseBackendStatus,
};
use crate::kernel::deepseek_credential::DeepSeekReadinessProjection;
use crate::kernel::local_directory::LocalDirectoryReadinessStatus;
use crate::kernel::models::{FoundationState, MemoryCandidate, TaskRecord};
use crate::kernel::network_search::{
    network_search_route_status_for_strategy, NetworkSearchRouteStatus,
};
use crate::kernel::tool_strategy::{
    current_runtime_platform, model_driven_tool_strategy_with_native_network_search_bridge,
    ModelDrivenToolStrategy,
};
use crate::kernel::workflow::{
    operations_briefing_workflow_template_package, OperationsBriefingRun, WorkflowTemplatePackage,
};

pub const WORK_PACKAGE_VERSION: &str = "deepseek-agent-os.work-package.v1";
const REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE: &str = "redacted source-machine evidence handle";

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum WorkPackageError {
    #[error("invalid work package json: {0}")]
    InvalidJson(String),

    #[error("unsupported work package version: {0}")]
    UnsupportedVersion(String),
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackage {
    pub version: String,
    pub exported_at: chrono::DateTime<chrono::Utc>,
    pub foundation_state: FoundationState,
    #[serde(default)]
    pub tool_readiness: WorkPackageToolReadiness,
    pub task_records: Vec<TaskRecord>,
    #[serde(default)]
    pub memory_candidates: Vec<MemoryCandidate>,
    #[serde(default)]
    pub operations_briefing_runs: Vec<OperationsBriefingRun>,
    #[serde(default)]
    pub workflow_templates: Vec<WorkflowTemplatePackage>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageToolReadiness {
    #[serde(
        default,
        deserialize_with = "deserialize_work_package_deepseek_readiness"
    )]
    pub deepseek: DeepSeekReadinessProjection,
    pub network_search: NetworkSearchRouteStatus,
    pub computer_use: ComputerUseBackendStatus,
    #[serde(default)]
    pub local_directories: LocalDirectoryReadinessStatus,
    #[serde(default)]
    pub tool_strategy: ModelDrivenToolStrategy,
}

fn deserialize_work_package_deepseek_readiness<'de, D>(
    deserializer: D,
) -> Result<DeepSeekReadinessProjection, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
    if value.get("source").is_none()
        || value.get("verification").is_none()
        || value.get("code").is_none()
    {
        return Ok(DeepSeekReadinessProjection::default());
    }
    let projection = serde_json::from_value::<DeepSeekReadinessProjection>(value)
        .map_err(serde::de::Error::custom)?;
    let coherent_ready = projection.verification
        == crate::kernel::deepseek_credential::DeepSeekVerificationState::Verified
        && projection.code == crate::kernel::deepseek_credential::DeepSeekReadinessCode::Ready;
    if projection.chat_completion_ready != coherent_ready {
        return Ok(DeepSeekReadinessProjection::default());
    }
    Ok(projection)
}

impl Default for WorkPackageToolReadiness {
    fn default() -> Self {
        let deepseek = DeepSeekReadinessProjection::default();
        let foundation_state = FoundationState::default();
        let tool_strategy = model_driven_tool_strategy_with_native_network_search_bridge(
            foundation_state.large_model_provider,
            foundation_state.network_search_source_model,
            current_runtime_platform(),
            false,
        );
        Self {
            network_search: network_search_route_status_for_strategy(
                &tool_strategy,
                deepseek.chat_completion_ready,
            ),
            deepseek,
            computer_use: computer_use_backend_status_for_strategy_with_codex_bridge_config(
                &tool_strategy,
                None,
                None,
            ),
            local_directories: LocalDirectoryReadinessStatus::default(),
            tool_strategy,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageImportSummary {
    pub imported: usize,
    pub skipped: usize,
    pub memory_candidates: WorkPackageMemoryCandidateImportSummary,
    pub operations_briefing_runs: WorkPackageOperationsBriefingImportSummary,
    pub workflow_templates: WorkPackageWorkflowTemplateImportSummary,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageMemoryCandidateImportSummary {
    pub imported: usize,
    pub skipped: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageOperationsBriefingImportSummary {
    pub imported: usize,
    pub skipped: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageWorkflowTemplateImportSummary {
    pub imported: usize,
    pub skipped: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageTaskImportPreview {
    pub total: usize,
    pub new: usize,
    pub skipped: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageOperationsBriefingImportPreview {
    pub total: usize,
    pub new: usize,
    pub skipped: usize,
    pub replay_supported: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageWorkflowTemplateImportPreview {
    pub total: usize,
    pub new: usize,
    pub skipped: usize,
    pub import_supported: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageMemoryCandidateImportPreview {
    pub total: usize,
    pub new: usize,
    pub skipped: usize,
    pub review_supported: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct WorkPackageImportPreview {
    pub task_records: WorkPackageTaskImportPreview,
    pub memory_candidates: WorkPackageMemoryCandidateImportPreview,
    pub operations_briefing_runs: WorkPackageOperationsBriefingImportPreview,
    pub workflow_templates: WorkPackageWorkflowTemplateImportPreview,
}

#[cfg(test)]
pub fn export_work_package(
    foundation_state: FoundationState,
    task_records: Vec<TaskRecord>,
    memory_candidates: Vec<MemoryCandidate>,
    operations_briefing_runs: Vec<OperationsBriefingRun>,
) -> WorkPackage {
    export_work_package_with_tool_readiness(
        foundation_state,
        task_records,
        memory_candidates,
        operations_briefing_runs,
        WorkPackageToolReadiness::default(),
    )
}

pub fn export_work_package_with_tool_readiness(
    foundation_state: FoundationState,
    task_records: Vec<TaskRecord>,
    memory_candidates: Vec<MemoryCandidate>,
    operations_briefing_runs: Vec<OperationsBriefingRun>,
    tool_readiness: WorkPackageToolReadiness,
) -> WorkPackage {
    let operations_briefing_runs = operations_briefing_runs
        .into_iter()
        .map(redact_operations_briefing_run_for_package_export)
        .collect();

    WorkPackage {
        version: WORK_PACKAGE_VERSION.to_string(),
        exported_at: chrono::Utc::now(),
        foundation_state,
        tool_readiness,
        task_records,
        memory_candidates,
        operations_briefing_runs,
        workflow_templates: vec![operations_briefing_workflow_template_package()],
    }
}

pub(crate) fn redact_operations_briefing_run_for_package_export(
    mut run: OperationsBriefingRun,
) -> OperationsBriefingRun {
    let evidence_folder_path = run.evidence_folder_path.clone();
    let evidence_folder_path = evidence_folder_path.as_deref();
    run.title = redact_source_machine_evidence_text(&run.title, evidence_folder_path);
    run.summary = redact_source_machine_evidence_text(&run.summary, evidence_folder_path);
    for anomaly in &mut run.anomalies {
        anomaly.area = redact_source_machine_evidence_text(&anomaly.area, evidence_folder_path);
        anomaly.signal = redact_source_machine_evidence_text(&anomaly.signal, evidence_folder_path);
        if anomaly.evidence_ref.as_deref().is_some_and(|evidence_ref| {
            should_redact_source_machine_evidence_ref(evidence_ref, evidence_folder_path)
        }) {
            anomaly.evidence_ref = Some(REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE.to_string());
        }
    }
    for action in &mut run.action_plan {
        action.owner = redact_source_machine_evidence_text(&action.owner, evidence_folder_path);
        action.action = redact_source_machine_evidence_text(&action.action, evidence_folder_path);
        action.due_hint =
            redact_source_machine_evidence_text(&action.due_hint, evidence_folder_path);
    }
    run.warnings = run
        .warnings
        .into_iter()
        .map(|warning| redact_source_machine_evidence_text(&warning, evidence_folder_path))
        .collect();
    run.context_receipt.user_intent =
        redact_source_machine_evidence_text(&run.context_receipt.user_intent, evidence_folder_path);
    run.context_receipt.loop_mode =
        redact_source_machine_evidence_text(&run.context_receipt.loop_mode, evidence_folder_path);
    run.context_receipt.workflow_policy = redact_source_machine_evidence_text(
        &run.context_receipt.workflow_policy,
        evidence_folder_path,
    );
    run.context_receipt.selected_evidence = run
        .context_receipt
        .selected_evidence
        .into_iter()
        .map(|evidence| redact_source_machine_evidence_text(&evidence, evidence_folder_path))
        .collect();
    run.context_receipt.selected_memories = run
        .context_receipt
        .selected_memories
        .into_iter()
        .map(|memory| redact_source_machine_evidence_text(&memory, evidence_folder_path))
        .collect();
    run.context_receipt.model_route =
        redact_source_machine_evidence_text(&run.context_receipt.model_route, evidence_folder_path);
    run.context_receipt.thinking_level = redact_source_machine_evidence_text(
        &run.context_receipt.thinking_level,
        evidence_folder_path,
    );
    run.context_receipt.token_cache_state = redact_source_machine_evidence_text(
        &run.context_receipt.token_cache_state,
        evidence_folder_path,
    );
    run.context_receipt.validation_results = run
        .context_receipt
        .validation_results
        .into_iter()
        .map(|result| redact_source_machine_evidence_text(&result, evidence_folder_path))
        .collect();
    run.context_receipt.intentional_omissions = run
        .context_receipt
        .intentional_omissions
        .into_iter()
        .map(|omission| redact_source_machine_evidence_text(&omission, evidence_folder_path))
        .collect();
    run.evidence_folder_path = None;
    run.evidence_invocation_id = None;
    run
}

fn redact_source_machine_evidence_text(value: &str, evidence_folder_path: Option<&str>) -> String {
    let Some(evidence_folder_path) = evidence_folder_path else {
        return value.to_string();
    };
    if !is_source_machine_path_handle(evidence_folder_path) {
        return value.to_string();
    }

    source_machine_path_variants(evidence_folder_path)
        .into_iter()
        .fold(value.to_string(), |redacted, path| {
            redacted.replace(&path, REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE)
        })
}

fn should_redact_source_machine_evidence_ref(
    evidence_ref: &str,
    evidence_folder_path: Option<&str>,
) -> bool {
    let Some(evidence_folder_path) = evidence_folder_path else {
        return false;
    };
    if !is_source_machine_path_handle(evidence_folder_path) {
        return false;
    }

    let evidence_ref = normalize_path_handle_for_redaction(evidence_ref);
    let evidence_folder_path = normalize_path_handle_for_redaction(evidence_folder_path);
    !evidence_folder_path.is_empty() && evidence_ref.contains(&evidence_folder_path)
}

fn source_machine_path_variants(value: &str) -> Vec<String> {
    let trimmed = value.trim().trim_end_matches(['\\', '/']).to_string();
    let slash_normalized = trimmed.replace('\\', "/");
    let backslash_normalized = trimmed.replace('/', "\\");
    let mut variants = Vec::new();
    for variant in [trimmed, slash_normalized, backslash_normalized] {
        if !variant.is_empty() && !variants.contains(&variant) {
            variants.push(variant);
        }
    }
    variants
}

fn is_source_machine_path_handle(value: &str) -> bool {
    let value = value.trim();
    let mut chars = value.chars();
    let first = chars.next();
    let second = chars.next();
    let third = chars.next();
    matches!((first, second, third), (Some(c), Some(':'), Some('\\' | '/')) if c.is_ascii_alphabetic())
        || value.starts_with("\\\\")
        || value.starts_with("//")
        || value.starts_with('/')
        || value.starts_with('~')
}

fn normalize_path_handle_for_redaction(value: &str) -> String {
    value
        .trim()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_lowercase()
}

pub fn parse_work_package_json(package_json: &str) -> Result<WorkPackage, WorkPackageError> {
    let package = serde_json::from_str::<WorkPackage>(package_json)
        .map_err(|error| WorkPackageError::InvalidJson(error.to_string()))?;

    if package.version != WORK_PACKAGE_VERSION {
        return Err(WorkPackageError::UnsupportedVersion(package.version));
    }

    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::{
        export_work_package, export_work_package_with_tool_readiness, parse_work_package_json,
        WorkPackageError, WorkPackageToolReadiness, REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE,
    };
    use crate::kernel::deepseek::{DEEPSEEK_FLASH_MODEL, DEEPSEEK_PRO_MODEL};
    use crate::kernel::deepseek_credential::{
        DeepSeekCredentialSource, DeepSeekReadinessCode, DeepSeekReadinessProjection,
        DeepSeekVerificationState,
    };
    use crate::kernel::local_directory::{
        local_directory_readiness_from_state, LocalDirectorySettings, LocalDirectoryState,
    };
    use crate::kernel::models::{
        FoundationState, MemoryCandidate, MemoryCandidateSource, MemoryLifecycle, MemoryScope,
        MemorySensitivity, MemoryType, TaskRecord,
    };
    use crate::kernel::workflow::{
        OperationsBriefingAction, OperationsBriefingAnomaly, OperationsBriefingRun,
        OperationsBriefingRunStatus, OPERATIONS_BRIEFING_WORKFLOW_ID,
    };

    fn sample_operations_briefing_run() -> OperationsBriefingRun {
        OperationsBriefingRun {
            id: uuid::Uuid::new_v4(),
            workflow_id: OPERATIONS_BRIEFING_WORKFLOW_ID.to_string(),
            status: OperationsBriefingRunStatus::DraftReady,
            archived_from_package: false,
            evidence_folder_path: Some("fixtures/evidence".to_string()),
            evidence_invocation_id: Some(uuid::Uuid::new_v4()),
            title: "Operations Briefing Draft".to_string(),
            summary: "Draft ready from evidence manifest.".to_string(),
            anomalies: vec![OperationsBriefingAnomaly {
                area: "Revenue".to_string(),
                signal: "Room revenue improved by 6 percent.".to_string(),
                evidence_ref: Some("fixtures/evidence".to_string()),
            }],
            action_plan: vec![OperationsBriefingAction {
                owner: "Operations owner".to_string(),
                action: "Confirm owner follow-up items.".to_string(),
                due_hint: "Next briefing cycle".to_string(),
            }],
            warnings: vec!["One evidence file was skipped.".to_string()],
            context_receipt: Default::default(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn exports_versioned_work_package_with_task_records() {
        let record = TaskRecord::new(
            "Prepare sales briefing".to_string(),
            "Summarize inbox and drive evidence for Monday review.".to_string(),
        )
        .expect("record is valid");

        let package = export_work_package(
            FoundationState::default(),
            vec![record.clone()],
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(package.version, "deepseek-agent-os.work-package.v1");
        assert!(package.exported_at <= chrono::Utc::now());
        assert_eq!(package.foundation_state, FoundationState::default());
        assert_eq!(package.task_records, vec![record]);
        assert!(package.operations_briefing_runs.is_empty());
    }

    #[test]
    fn operations_export_package_includes_briefing_runs() {
        let run = sample_operations_briefing_run();

        let package = export_work_package(
            FoundationState::default(),
            Vec::new(),
            Vec::new(),
            vec![run.clone()],
        );

        assert_eq!(package.operations_briefing_runs.len(), 1);
        let exported_run = &package.operations_briefing_runs[0];
        assert_eq!(exported_run.id, run.id);
        assert_eq!(exported_run.workflow_id, run.workflow_id);
        assert_eq!(exported_run.status, run.status);
        assert_eq!(exported_run.title, run.title);
        assert_eq!(exported_run.summary, run.summary);
        assert_eq!(exported_run.anomalies, run.anomalies);
        assert_eq!(exported_run.action_plan, run.action_plan);
        assert_eq!(exported_run.warnings, run.warnings);
        assert_eq!(exported_run.created_at, run.created_at);
        assert_eq!(exported_run.evidence_folder_path, None);
        assert_eq!(exported_run.evidence_invocation_id, None);
    }

    #[test]
    fn operations_export_package_redacts_local_evidence_handles() {
        let mut run = sample_operations_briefing_run();
        run.evidence_folder_path = Some("D:\\operator\\private-evidence".to_string());
        run.evidence_invocation_id = Some(uuid::Uuid::new_v4());
        run.anomalies[0].evidence_ref = Some("source file: revenue.md".to_string());

        let package = export_work_package(
            FoundationState::default(),
            Vec::new(),
            Vec::new(),
            vec![run],
        );
        let package_json = serde_json::to_string(&package).expect("package serializes");
        let exported_run = &package.operations_briefing_runs[0];

        assert_eq!(exported_run.evidence_folder_path, None);
        assert_eq!(exported_run.evidence_invocation_id, None);
        assert_eq!(
            exported_run.anomalies[0].evidence_ref.as_deref(),
            Some("source file: revenue.md")
        );
        assert!(!package_json.contains("private-evidence"));
        assert!(!package_json.contains(r"D:\\operator"));
    }

    #[test]
    fn operations_export_package_redacts_local_anomaly_evidence_refs() {
        let local_evidence_path = "D:\\operator\\private-evidence".to_string();
        let mut run = sample_operations_briefing_run();
        run.evidence_folder_path = Some(local_evidence_path.clone());
        run.anomalies[0].evidence_ref = Some(format!("{local_evidence_path}\\revenue.md"));
        run.anomalies.push(OperationsBriefingAnomaly {
            area: "Service".to_string(),
            signal: "Guest comments need review.".to_string(),
            evidence_ref: Some("source file: guest-experience.md".to_string()),
        });

        let package = export_work_package(
            FoundationState::default(),
            Vec::new(),
            Vec::new(),
            vec![run],
        );
        let package_json = serde_json::to_string(&package).expect("package serializes");
        let exported_run = &package.operations_briefing_runs[0];

        assert_eq!(
            exported_run.anomalies[0].evidence_ref.as_deref(),
            Some("redacted source-machine evidence handle")
        );
        assert_eq!(
            exported_run.anomalies[1].evidence_ref.as_deref(),
            Some("source file: guest-experience.md")
        );
        assert!(!package_json.contains("private-evidence"));
        assert!(!package_json.contains(r"D:\\operator"));
    }

    #[test]
    fn operations_export_package_redacts_local_evidence_path_mentions() {
        let local_evidence_path = "D:\\operator\\private-evidence".to_string();
        let mut run = sample_operations_briefing_run();
        run.evidence_folder_path = Some(local_evidence_path.clone());
        run.summary = format!("Draft used files from {local_evidence_path}.");
        run.anomalies[0].signal = format!("Revenue check references {local_evidence_path}.");
        run.action_plan[0].action = format!("Review exported notes from {local_evidence_path}.");
        run.action_plan[0].due_hint = format!("Before archiving {local_evidence_path}.");
        run.warnings = vec![format!("Skipped binary file under {local_evidence_path}.")];
        run.context_receipt.user_intent =
            format!("Draft briefing from evidence under {local_evidence_path}.");
        run.context_receipt.selected_evidence = vec![format!(
            "2 text files from {local_evidence_path}: revenue.md, risk.md"
        )];
        run.context_receipt.validation_results =
            vec![format!("Validated manifest from {local_evidence_path}.")];
        run.context_receipt.intentional_omissions = vec![format!(
            "Raw file bodies from {local_evidence_path} were omitted."
        )];

        let package = export_work_package(
            FoundationState::default(),
            Vec::new(),
            Vec::new(),
            vec![run],
        );
        let package_json = serde_json::to_string(&package).expect("package serializes");
        let exported_run = &package.operations_briefing_runs[0];

        assert!(exported_run
            .summary
            .contains(REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE));
        assert!(exported_run.anomalies[0]
            .signal
            .contains(REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE));
        assert!(exported_run.action_plan[0]
            .action
            .contains(REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE));
        assert!(exported_run.action_plan[0]
            .due_hint
            .contains(REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE));
        assert!(exported_run.warnings[0].contains(REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE));
        assert!(exported_run
            .context_receipt
            .user_intent
            .contains(REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE));
        assert!(exported_run.context_receipt.selected_evidence[0]
            .contains(REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE));
        assert!(exported_run.context_receipt.validation_results[0]
            .contains(REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE));
        assert!(exported_run.context_receipt.intentional_omissions[0]
            .contains(REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE));
        assert!(!package_json.contains("private-evidence"));
        assert!(!package_json.contains(r"D:\\operator"));
    }

    #[test]
    fn workflow_template_export_package_includes_default_operations_templates() {
        let package = export_work_package(
            FoundationState::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(package.workflow_templates.len(), 1);
        assert_eq!(
            package.workflow_templates[0].workflow_id,
            OPERATIONS_BRIEFING_WORKFLOW_ID
        );
        assert_eq!(package.workflow_templates[0].files.len(), 4);
    }

    #[test]
    fn imported_memory_candidate_export_package_includes_candidates() {
        let candidate = MemoryCandidate::new_with_metadata(
            "Review-safe project rule".to_string(),
            "Imported package candidates must stay pending until accepted locally.".to_string(),
            MemoryCandidateSource::Manual,
            None,
            "Package export should preserve review candidates.".to_string(),
            MemoryType::WorkflowRule,
            MemoryScope::Project,
            MemorySensitivity::Normal,
            MemoryLifecycle::Active,
        )
        .expect("candidate is valid");

        let package = export_work_package(
            FoundationState::default(),
            Vec::new(),
            vec![candidate.clone()],
            Vec::new(),
        );

        assert_eq!(package.memory_candidates, vec![candidate]);
    }

    #[test]
    fn imported_memory_candidate_legacy_package_defaults_candidates() {
        let package_json = serde_json::json!({
            "version": "deepseek-agent-os.work-package.v1",
            "exported_at": chrono::Utc::now(),
            "foundation_state": FoundationState::default(),
            "task_records": [],
            "operations_briefing_runs": []
        })
        .to_string();

        let package = parse_work_package_json(&package_json).expect("legacy package parses");

        assert!(package.memory_candidates.is_empty());
    }

    #[test]
    fn tool_readiness_export_package_stays_secret_safe() {
        let deepseek_status = DeepSeekReadinessProjection {
            source: DeepSeekCredentialSource::Environment,
            configured: true,
            verification: DeepSeekVerificationState::NotChecked,
            code: DeepSeekReadinessCode::NotChecked,
            chat_completion_ready: false,
            balance_available: None,
            flash_model: DEEPSEEK_FLASH_MODEL.to_string(),
            pro_model: DEEPSEEK_PRO_MODEL.to_string(),
            flash_available: None,
            pro_available: None,
            retryable: false,
            last_verified_at: None,
            message_key: "onboarding.deepseek.not_checked".to_string(),
        };
        let package = export_work_package_with_tool_readiness(
            FoundationState::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            WorkPackageToolReadiness {
                deepseek: deepseek_status,
                ..WorkPackageToolReadiness::default()
            },
        );

        let package_json = serde_json::to_string(&package).expect("package serializes");

        assert!(package.tool_readiness.deepseek.configured);
        assert!(package_json.contains("\"source\":\"environment\""));
        assert!(!package_json.contains("DEEPSEEK_API_KEY"));
        assert!(package_json.contains("pending_user_confirmation"));
        assert!(package_json.contains("\"local_directories\""));
        if cfg!(target_os = "macos") {
            assert!(package_json.contains("local_macos_screen_capture"));
        } else {
            assert!(package_json.contains("local_windows_screen_capture"));
        }
        assert!(!package_json.contains("test-secret-token"));
    }

    #[test]
    fn tool_readiness_local_directories_reports_setup_without_serializing_paths() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_dir = temp_dir.path().join("workspace");
        let evidence_dir = temp_dir.path().join("evidence");
        let export_dir = temp_dir.path().join("exports");
        std::fs::create_dir_all(&workspace_dir).expect("workspace dir");
        std::fs::create_dir_all(&evidence_dir).expect("evidence dir");
        std::fs::create_dir_all(&export_dir).expect("export dir");
        let state = LocalDirectoryState {
            app_data_dir: "C:\\Users\\alice\\AppData\\Roaming\\deepseek-agent-os".to_string(),
            settings_file:
                "C:\\Users\\alice\\AppData\\Roaming\\deepseek-agent-os\\local-directories.json"
                    .to_string(),
            settings: Some(
                LocalDirectorySettings::new(
                    workspace_dir.to_string_lossy().to_string(),
                    evidence_dir.to_string_lossy().to_string(),
                    export_dir.to_string_lossy().to_string(),
                )
                .expect("settings validate"),
            ),
            needs_setup: false,
        };
        let package = export_work_package_with_tool_readiness(
            FoundationState::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            WorkPackageToolReadiness {
                local_directories: local_directory_readiness_from_state(&state),
                ..WorkPackageToolReadiness::default()
            },
        );

        let package_json = serde_json::to_string(&package).expect("package serializes");

        assert!(!package.tool_readiness.local_directories.needs_setup);
        assert!(
            package
                .tool_readiness
                .local_directories
                .workspace_configured
        );
        assert!(package.tool_readiness.local_directories.evidence_configured);
        assert!(package.tool_readiness.local_directories.export_configured);
        assert!(package.tool_readiness.local_directories.paths_redacted);
        assert!(!package_json.contains(&temp_dir.path().to_string_lossy().to_string()));
        assert!(!package_json.contains("alice"));
    }

    #[test]
    fn tool_readiness_legacy_package_json_defaults_readiness() {
        let package_json = serde_json::json!({
            "version": "deepseek-agent-os.work-package.v1",
            "exported_at": chrono::Utc::now(),
            "foundation_state": FoundationState::default(),
            "task_records": [],
            "memory_candidates": [],
            "operations_briefing_runs": []
        })
        .to_string();

        let package = parse_work_package_json(&package_json).expect("legacy package parses");

        assert_eq!(
            package.tool_readiness.deepseek,
            DeepSeekReadinessProjection::default()
        );
        assert_eq!(package.tool_readiness, WorkPackageToolReadiness::default());
    }

    #[test]
    fn tool_readiness_legacy_deepseek_shape_cannot_import_false_ready_authority() {
        let mut tool_readiness = serde_json::to_value(WorkPackageToolReadiness::default())
            .expect("readiness serializes");
        tool_readiness["deepseek"] = serde_json::json!({
            "base_url": "https://api.deepseek.com",
            "chat_completions_url": "https://api.deepseek.com/chat/completions",
            "api_key_env_var": "DEEPSEEK_API_KEY",
            "api_key_configured": true,
            "chat_completion_ready": true,
            "flash_model": "deepseek-v4-flash",
            "pro_model": "deepseek-v4-pro"
        });
        let package_json = serde_json::json!({
            "version": "deepseek-agent-os.work-package.v1",
            "exported_at": chrono::Utc::now(),
            "foundation_state": FoundationState::default(),
            "tool_readiness": tool_readiness,
            "task_records": [],
            "memory_candidates": [],
            "operations_briefing_runs": []
        })
        .to_string();

        let package = parse_work_package_json(&package_json).expect("legacy package parses");

        assert_eq!(
            package.tool_readiness.deepseek,
            DeepSeekReadinessProjection::default()
        );
        assert!(!package.tool_readiness.deepseek.chat_completion_ready);
    }

    #[test]
    fn operations_export_legacy_package_json_defaults_briefing_runs() {
        let package_json = serde_json::json!({
            "version": "deepseek-agent-os.work-package.v1",
            "exported_at": chrono::Utc::now(),
            "foundation_state": FoundationState::default(),
            "task_records": []
        })
        .to_string();

        let package = parse_work_package_json(&package_json).expect("legacy package parses");

        assert!(package.operations_briefing_runs.is_empty());
        assert!(package.workflow_templates.is_empty());
    }

    #[test]
    fn rejects_work_package_with_unknown_version() {
        let mut package = export_work_package(
            FoundationState::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        package.version = "deepseek-agent-os.work-package.v0".to_string();
        let package_json = serde_json::to_string(&package).expect("package serializes");

        let error = parse_work_package_json(&package_json).expect_err("version should fail");

        assert_eq!(
            error,
            WorkPackageError::UnsupportedVersion("deepseek-agent-os.work-package.v0".to_string())
        );
    }
}
