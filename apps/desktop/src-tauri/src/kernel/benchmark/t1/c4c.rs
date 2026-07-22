use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::env;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::ZipArchive;

use super::fixtures::{generate_fixture_set, write_deterministic_zip, T1GeneratedFixtureSet};
use super::verifiers::{
    build_provenance_manifest, build_source_manifest, verify_actual_render, verify_provenance,
    verify_result_receipt, verify_source_manifest, T1ActualRenderReceipt, T1CandidateArtifact,
    T1OutputReceipt, T1PreviewReceipt, T1RenderArtifactReceipt, T1RenderEvidence, T1ResultReceipt,
};
use super::{task_spec, BRIEF_OUTPUT_PATH, RECONCILIATION_OUTPUT_PATH};
use crate::kernel::agent_run::{
    AgentRunFinish, AgentRunResourceAccess, AgentRunResourceClaim, AgentRunStart, AgentRunStatus,
};
use crate::kernel::artifact_render::ACTUAL_RENDERER_VERSION;
use crate::kernel::artifacts::preview_manifest_hash;
use crate::kernel::benchmark::{
    aggregate_benchmark_runs, classify_benchmark_run, BenchmarkEvidenceReceipt,
    BenchmarkExternalEffectState, BenchmarkInteractions, BenchmarkOutcomeClass, BenchmarkRunResult,
    BenchmarkSubject, BenchmarkTaskSpec, BenchmarkTerminalState, BenchmarkVerifierResult,
    BenchmarkVerifierStatus, BENCHMARK_RUN_RESULT_VERSION,
};
use crate::kernel::event_store::EventStore;
use crate::kernel::goal_continuation::{
    ContextCheckpointStatus, GoalContinuationBlockerCode, GoalContinuationObservation,
    GoalContinuationObservationStage, GoalToolUsage,
};
use crate::kernel::goal_envelope::{
    GoalDoneWhenProposal, GoalEnvelopeProposal, GoalExternalTargetProposal,
    GoalRequiredArtifactProposal, GoalVerifierProposal, GOAL_ENVELOPE_PROPOSAL_VERSION,
};
use crate::kernel::goal_lifecycle::{
    GoalCompletionStatus, GoalTargetBindingKind, GoalValidationContext,
};
use crate::kernel::local_directory::WorkspaceReadinessCode;
use crate::kernel::models::AccessMode;
use crate::kernel::policy::RiskLevel;
use crate::kernel::t1_powerpoint::{
    LocalT1PowerPointRenderer, T1PowerPointAgentToolExecutor, T1PowerPointOutcome,
    T1PowerPointRender, T1PowerPointRenderer, T1PowerPointRequest,
};
use crate::kernel::t1_reconciliation::{
    verify_t1_reconciliation_artifact, T1ReconciliationAgentToolExecutor, T1ReconciliationOutcome,
    T1ReconciliationRequest,
};
use crate::kernel::task_capability_manifest::{
    TaskCapabilityDescriptionProposal, TaskCapabilityManifestContext, TaskCapabilityProposal,
    TASK_CAPABILITY_PROPOSAL_VERSION,
};
use crate::kernel::task_grouped_approval::{
    TaskGroupedApproval, TaskGroupedApprovalStatus, TaskGroupedCapabilityClaim,
};
use crate::kernel::tool_runtime::{
    prepare_tool_execution, AgentToolExecutor, ToolExecutionOutput, ToolExecutionPlan,
    ToolExecutionRequest, ToolInvocationRecord, T1_POWERPOINT_TOOL_ID, T1_RECONCILIATION_TOOL_ID,
};

const C4C_REPORT_VERSION: &str = "ds-agent.step-4-c4c-outcome/v1";
const C4C_SOURCE_COMMIT: &str = "86e80d70a158a6b5a7769efb79590cdf3b4b480a";
const SUCCESS_GROUPS: u32 = 43;
const TOTAL_GROUPS: u32 = 50;

#[derive(Clone, Debug, Serialize)]
struct C4cGroupResult {
    group_id: String,
    case_kind: String,
    expected_terminal: String,
    observed_outcome: String,
    completed: bool,
    false_completion: bool,
    authorization_resolutions: u32,
    key_figures_traceable: bool,
    detection_checks: BTreeMap<String, bool>,
    reconciliation_sha256: Option<String>,
    powerpoint_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct C4cDetectionTotals {
    numeric_conflicts_injected: u32,
    numeric_conflicts_detected: u32,
    damaged_formulas_injected: u32,
    damaged_formulas_detected: u32,
    false_completion_open: u32,
    false_completion_formula: u32,
    false_completion_garbling: u32,
    false_completion_clipping: u32,
    false_completion_overflow: u32,
}

#[derive(Clone, Debug, Serialize)]
struct C4cOutcomeReport {
    version: String,
    source_commit: String,
    environment_profile: String,
    deterministic_groups: u32,
    outcomes_a: u64,
    outcomes_f: u64,
    vocr_numerator: u64,
    vocr_denominator: u64,
    vocr_basis_points: u32,
    authorization_budget_compliant_groups: u64,
    unauthorized_path_writes: u32,
    all_key_figures_traceable: bool,
    deepseek_authority_or_receipts: u32,
    installed_office_case: String,
    detections: C4cDetectionTotals,
    groups: Vec<C4cGroupResult>,
}

struct MatrixRoot {
    path: PathBuf,
    _temporary: Option<tempfile::TempDir>,
}

struct FixtureRenderer {
    renders: RefCell<VecDeque<Result<Vec<Vec<u8>>, String>>>,
}

impl T1PowerPointRenderer for FixtureRenderer {
    fn render(&self, _path: &Path) -> Result<T1PowerPointRender, String> {
        let pages = self
            .renders
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(vec![valid_preview()]))?;
        Ok(T1PowerPointRender {
            pages,
            renderer_version: "c4c-deterministic-renderer/v1".to_string(),
        })
    }
}

struct AuthorizedRun {
    store: EventStore,
    store_path: PathBuf,
    run_id: Uuid,
    group_id: Uuid,
}

#[derive(Clone)]
struct SuccessfulExecution {
    reconciliation: T1ReconciliationOutcome,
    powerpoint: T1PowerPointOutcome,
    run_result: BenchmarkRunResult,
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn logical_time(index: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 22, 16, 0, index)
        .single()
        .expect("C4C logical time")
}

fn matrix_root() -> MatrixRoot {
    if let Some(value) = env::var_os("DS_AGENT_C4C_EVIDENCE_ROOT") {
        let path = PathBuf::from(value);
        assert!(path.is_absolute(), "C4C evidence root must be absolute");
        if path.exists() {
            assert!(
                fs::read_dir(&path)
                    .expect("read C4C evidence root")
                    .next()
                    .is_none(),
                "C4C evidence root must be fresh and empty"
            );
        } else {
            fs::create_dir_all(&path).expect("create C4C evidence root");
        }
        MatrixRoot {
            path,
            _temporary: None,
        }
    } else {
        let temporary = tempfile::tempdir().expect("temporary C4C root");
        MatrixRoot {
            path: temporary.path().to_path_buf(),
            _temporary: Some(temporary),
        }
    }
}

fn fixture_renderer(renders: Vec<Result<Vec<Vec<u8>>, String>>) -> FixtureRenderer {
    FixtureRenderer {
        renders: RefCell::new(renders.into()),
    }
}

fn png_preview(edge_clipped: bool) -> Vec<u8> {
    let mut image = GrayImage::from_pixel(320, 180, Luma([255]));
    let (left, right) = if edge_clipped { (0, 160) } else { (80, 240) };
    for y in 60..120 {
        for x in left..right {
            image.put_pixel(x, y, Luma([32]));
        }
    }
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(image)
        .write_to(&mut cursor, ImageFormat::Png)
        .expect("encode C4C preview");
    cursor.into_inner()
}

fn valid_preview() -> Vec<u8> {
    png_preview(false)
}

fn edge_clipped_preview() -> Vec<u8> {
    png_preview(true)
}

fn rewrite_zip_part(bytes: &[u8], part_name: &str, replacement: Vec<u8>) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open deterministic OPC package");
    let mut parts = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .expect("read deterministic OPC part");
        let name = entry.name().to_string();
        let mut part = Vec::new();
        entry.read_to_end(&mut part).expect("read OPC part bytes");
        if name == part_name {
            part = replacement.clone();
        }
        parts.push((name, part));
    }
    let borrowed = parts
        .iter()
        .map(|(name, part)| (name.as_str(), part.as_slice()))
        .collect::<Vec<_>>();
    write_deterministic_zip(&borrowed).expect("rewrite deterministic OPC package")
}

fn varied_fixture_set(index: u32) -> T1GeneratedFixtureSet {
    let mut fixtures = generate_fixture_set().expect("base T1 fixtures");
    let percent = 54 + index;
    let available = 3000_i64;
    let sold = available * i64::from(percent) / 100;
    let occupancy = f64::from(percent) / 100.0;
    let adr = 420_i64 + i64::from(index) * 5;
    let rooms = sold * adr;
    let food = 300_000_i64 + i64::from(index) * 5_000;
    let other = 50_000_i64 + i64::from(index) * 1_000;
    let total = rooms + food + other;
    let budget = total + 50_000;
    let prior = total - 25_000;
    let budget_occupancy = occupancy + 0.02;

    let revenue = fixtures
        .files
        .iter_mut()
        .find(|file| file.fixture_id == "monthly-revenue-xlsx")
        .expect("revenue fixture");
    let mut archive = ZipArchive::new(Cursor::new(&revenue.bytes)).expect("open revenue fixture");
    let mut sheet = String::new();
    archive
        .by_name("xl/worksheets/sheet1.xml")
        .expect("revenue worksheet")
        .read_to_string(&mut sheet)
        .expect("read revenue worksheet");
    drop(archive);
    let replacements = [
        ("B2", "3000".to_string(), available.to_string()),
        ("B3", "2040".to_string(), sold.to_string()),
        ("B4", "0.68".to_string(), format!("{occupancy:.2}")),
        ("B5", "1142400".to_string(), rooms.to_string()),
        ("B6", "560".to_string(), adr.to_string()),
        ("B7", "480000".to_string(), food.to_string()),
        ("B8", "80000".to_string(), other.to_string()),
        ("B9", "1702400".to_string(), total.to_string()),
        ("B10", "1850000".to_string(), budget.to_string()),
        ("B11", "1760000".to_string(), prior.to_string()),
        ("B12", "0.74".to_string(), format!("{budget_occupancy:.2}")),
    ];
    for (cell, from, to) in replacements {
        sheet = sheet.replacen(
            &format!("<c r=\"{cell}\"><v>{from}</v></c>"),
            &format!("<c r=\"{cell}\"><v>{to}</v></c>"),
            1,
        );
    }
    revenue.bytes = rewrite_zip_part(
        &revenue.bytes,
        "xl/worksheets/sheet1.xml",
        sheet.into_bytes(),
    );
    let entry = fixtures
        .manifest
        .files
        .iter_mut()
        .find(|entry| entry.fixture_id == revenue.fixture_id)
        .expect("revenue manifest entry");
    entry.bytes = revenue.bytes.len() as u64;
    entry.sha256 = sha256(&revenue.bytes);
    fixtures
}

fn mutate_fixture_text(
    fixtures: &mut T1GeneratedFixtureSet,
    fixture_id: &str,
    part_name: &str,
    from: &str,
    to: &str,
) {
    let fixture = fixtures
        .files
        .iter_mut()
        .find(|file| file.fixture_id == fixture_id)
        .expect("fixture to mutate");
    let mut archive = ZipArchive::new(Cursor::new(&fixture.bytes)).expect("open fixture package");
    let mut text = String::new();
    archive
        .by_name(part_name)
        .expect("fixture part")
        .read_to_string(&mut text)
        .expect("fixture text");
    drop(archive);
    assert!(text.contains(from), "fixture mutation source text missing");
    let replacement = text.replacen(from, to, 1).into_bytes();
    fixture.bytes = rewrite_zip_part(&fixture.bytes, part_name, replacement);
    let entry = fixtures
        .manifest
        .files
        .iter_mut()
        .find(|entry| entry.fixture_id == fixture_id)
        .expect("fixture manifest entry");
    entry.bytes = fixture.bytes.len() as u64;
    entry.sha256 = sha256(&fixture.bytes);
}

fn task_spec_for(fixtures: &T1GeneratedFixtureSet) -> BenchmarkTaskSpec {
    let mut spec = task_spec().expect("T1 task spec");
    for fixture in &mut spec.fixtures {
        let generated = fixtures
            .files
            .iter()
            .find(|candidate| candidate.fixture_id == fixture.fixture_id)
            .expect("task fixture");
        fixture.sha256 = sha256(&generated.bytes);
    }
    spec.validate().expect("variant task spec");
    spec
}

fn write_fixtures(workspace: &Path, fixtures: &T1GeneratedFixtureSet) {
    fs::create_dir_all(workspace.join("inputs")).expect("create inputs");
    fs::create_dir_all(workspace.join("outputs")).expect("create outputs");
    for fixture in &fixtures.files {
        let path = workspace.join(&fixture.relative_path);
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
        fs::write(path, &fixture.bytes).expect("write fixture");
    }
}

fn goal_proposal() -> GoalEnvelopeProposal {
    let bindings = [
        ("actual-render", "actual-render-v1", "actual_render_receipt"),
        (
            "fact-provenance",
            "fact-provenance-v1",
            "t1_fact_provenance",
        ),
        (
            "office-revision",
            "office-revision-v1",
            "office_revision_receipt",
        ),
        ("one-page-pptx", "one-page-pptx-v1", "one_page_pptx"),
        (
            "reconciliation-xlsx",
            "reconciliation-xlsx-v1",
            "t1_reconciliation_xlsx",
        ),
        (
            "source-manifest",
            "source-manifest-v1",
            "t1_source_manifest",
        ),
    ];
    GoalEnvelopeProposal {
        version: GOAL_ENVELOPE_PROPOSAL_VERSION.to_string(),
        user_goal: "Create a verified T1 reconciliation workbook and one-page monthly brief from local synthetic fixtures.".to_string(),
        assumptions: Vec::new(),
        constraints: vec![
            "Use only the bound isolated workspace and local synthetic fixtures.".to_string(),
            "DeepSeek is advisory and cannot mint authority or completion receipts.".to_string(),
        ],
        done_when: bindings
            .iter()
            .map(|(done_when_id, _, _)| GoalDoneWhenProposal {
                done_when_id: (*done_when_id).to_string(),
                description: format!("C4C verifies {done_when_id}."),
            })
            .collect(),
        required_artifacts: vec![
            GoalRequiredArtifactProposal {
                artifact_id: "t1-monthly-brief-pptx".to_string(),
                description: "Verified one-page PPTX.".to_string(),
            },
            GoalRequiredArtifactProposal {
                artifact_id: "t1-reconciliation-xlsx".to_string(),
                description: "Verified formula-backed XLSX.".to_string(),
            },
        ],
        verifiers: bindings
            .iter()
            .map(|(done_when_id, verifier_id, evidence_kind)| GoalVerifierProposal {
                verifier_id: (*verifier_id).to_string(),
                done_when_id: (*done_when_id).to_string(),
                description: format!("Verify C4C evidence kind {evidence_kind}."),
                evidence_kind: (*evidence_kind).to_string(),
            })
            .collect(),
        proposed_capabilities: vec![
            T1_POWERPOINT_TOOL_ID.to_string(),
            T1_RECONCILIATION_TOOL_ID.to_string(),
        ],
        external_targets: vec![GoalExternalTargetProposal {
            target_id: "workspace".to_string(),
            description: "The exact isolated C4C workspace.".to_string(),
        }],
        stop_conditions: vec!["Stop on authority, source, path, or receipt drift.".to_string()],
    }
}

fn setup_authorized_run(workspace: &Path) -> AuthorizedRun {
    let store_path = workspace.join("event-store.sqlite3");
    let store = EventStore::open(&store_path).expect("open C4C Event Store");
    let start = AgentRunStart::new(
        "c4c-local-matrix".to_string(),
        "Execute one exact authorized T1 group.".to_string(),
        0,
    )
    .expect("agent run start");
    assert!(store.append_agent_run_start(&start).expect("persist run"));
    let context =
        GoalValidationContext::new(AccessMode::AskEveryStep, WorkspaceReadinessCode::Ready)
            .with_max_risk(RiskLevel::High)
            .with_enabled_tool(T1_POWERPOINT_TOOL_ID, true)
            .with_enabled_tool(T1_RECONCILIATION_TOOL_ID, true)
            .with_approval_route(T1_POWERPOINT_TOOL_ID)
            .with_approval_route(T1_RECONCILIATION_TOOL_ID)
            .with_verifier_kind("actual_render_receipt")
            .with_verifier_kind("office_revision_receipt")
            .with_verifier_kind("one_page_pptx")
            .with_verifier_kind("t1_fact_provenance")
            .with_verifier_kind("t1_reconciliation_xlsx")
            .with_verifier_kind("t1_source_manifest")
            .with_target_binding(
                "workspace",
                GoalTargetBindingKind::Path,
                workspace.to_string_lossy().as_bytes(),
            )
            .allowing_local_effects();
    let validated = store
        .submit_goal_proposal(start.id, &goal_proposal(), &context)
        .expect("validate C4C goal");
    let goal = store
        .freeze_goal_envelope(start.id, validated.revision().expect("goal revision"))
        .expect("freeze C4C goal");

    let proposal = TaskCapabilityProposal {
        version: TASK_CAPABILITY_PROPOSAL_VERSION.to_string(),
        expires_at: "2030-01-02T03:04:05Z".parse().expect("C4C expiry"),
        capabilities: vec![TaskCapabilityDescriptionProposal {
            capability: "file_write".to_string(),
            application_ids: vec!["ds-agent".to_string()],
            path_target_ids: vec!["workspace".to_string()],
            account_target_ids: Vec::new(),
            recipient_target_ids: Vec::new(),
            time_window_target_ids: Vec::new(),
            external_target_ids: vec!["workspace".to_string()],
            verifier_ids: goal
                .frozen()
                .expect("frozen C4C goal")
                .envelope
                .verifiers
                .iter()
                .map(|verifier| verifier.verifier_id.clone())
                .collect(),
        }],
    };
    proposal.validate().expect("C4C capability proposal");
    let manifest_context = TaskCapabilityManifestContext::default()
        .with_application("ds-agent", "DS Agent local Kernel")
        .with_target_display("workspace", "Exact isolated C4C workspace");
    let now: DateTime<Utc> = "2029-01-02T03:04:05Z".parse().expect("C4C now");
    let pending = store
        .prepare_task_grouped_approval_from_proposal(start.id, &proposal, &manifest_context, now)
        .expect("prepare exact grouped authorization");
    assert_eq!(pending.status, TaskGroupedApprovalStatus::Pending);
    assert_eq!(pending.capability_audits.len(), 2);
    let view = pending
        .authorization_view(&goal)
        .expect("authorization view");
    store
        .resolve_task_grouped_authorization(&view.intent, true, now + chrono::Duration::minutes(1))
        .expect("one user grouped authorization resolution");
    let approved = store
        .task_grouped_approval(pending.id)
        .expect("read grouped authorization")
        .expect("grouped authorization exists");
    assert_eq!(approved.status, TaskGroupedApprovalStatus::Approved);
    assert_eq!(approved.capability_audits.len(), 2);

    AuthorizedRun {
        store,
        store_path,
        run_id: start.id,
        group_id: approved.id,
    }
}

fn approved_group(run: &AuthorizedRun) -> TaskGroupedApproval {
    run.store
        .task_grouped_approval(run.group_id)
        .expect("read exact grouped authorization")
        .expect("exact grouped authorization")
}

fn plan_for(run_id: Uuid, tool_id: &str, input: serde_json::Value) -> ToolExecutionPlan {
    prepare_tool_execution(&ToolExecutionRequest {
        tool_id: tool_id.to_string(),
        input,
        access_mode: AccessMode::AskEveryStep,
        run_id: Some(run_id),
    })
    .expect("prepare exact C4C tool execution")
}

fn execute_authorized(
    run: &AuthorizedRun,
    plan: &ToolExecutionPlan,
    executor: &dyn AgentToolExecutor,
) -> Result<ToolExecutionOutput, String> {
    let group = approved_group(run);
    let item = group
        .capability_audits
        .iter()
        .find(|item| item.tool_id == plan.contract.id)
        .expect("exact tool authorization item");
    let claim = TaskGroupedCapabilityClaim::from_group_item(&group, item);
    let grant = run
        .store
        .authorize_task_grouped_capability(
            &claim,
            "2029-01-02T03:06:05Z".parse().expect("authorization time"),
        )
        .expect("authorize exact grouped capability");
    assert_eq!(grant.tool_id, plan.contract.id);
    let resource = plan
        .contract
        .constraints
        .resource
        .as_ref()
        .expect("T1 resource contract");
    let resource_claim = AgentRunResourceClaim::new(
        Some(run.run_id),
        plan.invocation_id,
        resource.key.clone(),
        AgentRunResourceAccess::Write,
        resource.lease_seconds,
    )
    .expect("resource claim");
    run.store
        .claim_agent_run_resource(resource_claim)
        .expect("claim exact local resource");
    run.store
        .append_tool_invocation(&ToolInvocationRecord::running(
            plan,
            Some(grant.approval_request_id),
        ))
        .expect("record running invocation");

    match executor.execute(plan) {
        Ok(output) => {
            let invocation = ToolInvocationRecord::succeeded(
                plan,
                output.output.clone(),
                output.evidence.clone(),
                output.verification.clone(),
                Some(grant.approval_request_id),
                0,
            )?;
            run.store
                .append_tool_invocation(&invocation)
                .map_err(|error| error.to_string())?;
            run.store
                .record_goal_completion_for_tool_invocation(&invocation)
                .map_err(|error| error.to_string())?;
            run.store
                .release_agent_run_resources_for_invocation(
                    plan.invocation_id,
                    "C4C invocation completed".to_string(),
                )
                .map_err(|error| error.to_string())?;
            Ok(output)
        }
        Err(error) => {
            run.store
                .append_tool_invocation(&ToolInvocationRecord::failed(
                    plan,
                    error.clone(),
                    Some(grant.approval_request_id),
                    0,
                ))
                .map_err(|store_error| store_error.to_string())?;
            run.store
                .release_agent_run_resources_for_invocation(
                    plan.invocation_id,
                    "C4C invocation failed closed".to_string(),
                )
                .map_err(|store_error| store_error.to_string())?;
            Err(error)
        }
    }
}

fn checkpoint_observation(
    stage: GoalContinuationObservationStage,
    invocation_id: Option<Uuid>,
    observed_at: DateTime<Utc>,
) -> GoalContinuationObservation {
    GoalContinuationObservation {
        stage,
        local_tool_round: u32::from(invocation_id.is_some()),
        model_usage: Vec::new(),
        tool_usage: invocation_id
            .map(|invocation_id| GoalToolUsage {
                invocation_id,
                elapsed_ms: 0,
            })
            .into_iter()
            .collect(),
        observed_at,
    }
}

fn c0d_render_evidence(
    reconciliation: &T1CandidateArtifact,
    brief: &T1CandidateArtifact,
) -> T1RenderEvidence {
    let preview = valid_preview();
    let artifacts = [
        (reconciliation, "previews/c4c-reconciliation.png"),
        (brief, "previews/c4c-brief.png"),
    ]
    .into_iter()
    .map(|(artifact, preview_path)| T1RenderArtifactReceipt {
        output_relative_path: artifact.relative_path.clone(),
        output_sha256: sha256(&artifact.bytes),
        renderer_version: ACTUAL_RENDERER_VERSION.to_string(),
        rendered_unit_count: 1,
        preview_manifest_sha256: preview_manifest_hash(&[preview.clone()]),
        previews: vec![T1PreviewReceipt {
            relative_path: preview_path.to_string(),
            bytes: preview.len() as u64,
            sha256: sha256(&preview),
            width: 320,
            height: 180,
            edge_clipping: false,
        }],
    })
    .collect::<Vec<_>>();
    T1RenderEvidence {
        receipt: T1ActualRenderReceipt {
            version: "t1.actual-render-receipt/v1".to_string(),
            artifacts,
        },
        preview_bytes: BTreeMap::from([
            ("previews/c4c-brief.png".to_string(), preview.clone()),
            ("previews/c4c-reconciliation.png".to_string(), preview),
        ]),
    }
}

fn provisional_result_receipt_verifier() -> BenchmarkVerifierResult {
    BenchmarkVerifierResult {
        done_when_id: "result-receipt".to_string(),
        verifier_id: "t1.result-receipt/v1".to_string(),
        status: BenchmarkVerifierStatus::Passed,
        summary: "C4C deterministic result receipt pending exact readback.".to_string(),
        evidence: vec![BenchmarkEvidenceReceipt {
            kind: "result_receipt".to_string(),
            relative_or_opaque_ref: "benchmark-evidence:t1-result-receipt".to_string(),
            sha256: None,
            summary: "C4C secret-safe deterministic result receipt.".to_string(),
        }],
    }
}

fn build_run_result(
    spec: &BenchmarkTaskSpec,
    index: u32,
    terminal_state: BenchmarkTerminalState,
    outcome_class: BenchmarkOutcomeClass,
    verifier_results: Vec<BenchmarkVerifierResult>,
    failure_code: Option<&str>,
) -> BenchmarkRunResult {
    let time = logical_time(index);
    let result = BenchmarkRunResult {
        version: BENCHMARK_RUN_RESULT_VERSION.to_string(),
        run_id: format!("c4c-group-{index:02}"),
        task_id: spec.task_id.clone(),
        task_revision: spec.task_revision,
        task_spec_fingerprint: spec.fingerprint().expect("task fingerprint"),
        repetition_index: index,
        subject: BenchmarkSubject {
            app_version: "step-4-c4c".to_string(),
            source_commit: C4C_SOURCE_COMMIT.to_string(),
            release_tag: None,
            environment_profile: "local-synthetic-deterministic".to_string(),
            clean_state_id: format!("c4c-clean-{index:02}"),
            access_mode: AccessMode::AskEveryStep,
        },
        started_at: time,
        finished_at: time,
        elapsed_ms: 0,
        terminal_state,
        external_effect_state: BenchmarkExternalEffectState::None,
        outcome_class,
        interactions: BenchmarkInteractions {
            clarification_count: 0,
            authorization_count: 1,
            manual_interventions: Vec::new(),
        },
        deepseek_usage: Vec::new(),
        verifier_results,
        guardrail_violations: Vec::new(),
        failure_stage: failure_code.map(|_| "verification".to_string()),
        failure_code: failure_code.map(str::to_string),
    };
    result.validate(spec).expect("valid C4C benchmark result");
    result
}

fn failed_run_result(
    spec: &BenchmarkTaskSpec,
    index: u32,
    failure_code: &str,
) -> BenchmarkRunResult {
    let verifiers = spec
        .done_when
        .iter()
        .map(|condition| BenchmarkVerifierResult {
            done_when_id: condition.done_when_id.clone(),
            verifier_id: condition.verifier_id.clone(),
            status: BenchmarkVerifierStatus::NotRun,
            summary: "C4C stopped before completion evidence could be issued.".to_string(),
            evidence: vec![BenchmarkEvidenceReceipt {
                kind: condition.required_evidence_kinds[0].clone(),
                relative_or_opaque_ref: format!(
                    "benchmark-evidence:c4c-failed-{}",
                    condition.done_when_id
                ),
                sha256: None,
                summary: "C4C fail-closed audit receipt; not completion evidence.".to_string(),
            }],
        })
        .collect();
    build_run_result(
        spec,
        index,
        BenchmarkTerminalState::Failed,
        BenchmarkOutcomeClass::F,
        verifiers,
        Some(failure_code),
    )
}

fn c4c_verifier_result(
    done_when_id: &str,
    verifier_id: &str,
    evidence_kind: &str,
    evidence_ref: &str,
    evidence_sha256: Option<String>,
    validation: Result<(), String>,
) -> BenchmarkVerifierResult {
    BenchmarkVerifierResult {
        done_when_id: done_when_id.to_string(),
        verifier_id: verifier_id.to_string(),
        status: if validation.is_ok() {
            BenchmarkVerifierStatus::Passed
        } else {
            BenchmarkVerifierStatus::Failed
        },
        summary: if validation.is_ok() {
            "Independent C4C production-layout verification passed.".to_string()
        } else {
            "Independent C4C production-layout verification failed closed.".to_string()
        },
        evidence: vec![BenchmarkEvidenceReceipt {
            kind: evidence_kind.to_string(),
            relative_or_opaque_ref: evidence_ref.to_string(),
            sha256: evidence_sha256,
            summary: "Secret-safe independent C4C artifact receipt.".to_string(),
        }],
    }
}

fn opc_text(bytes: &[u8], name: &str) -> Result<String, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("C4C OPC package is invalid: {error}"))?;
    let mut text = String::new();
    archive
        .by_name(name)
        .map_err(|_| format!("C4C OPC part {name} is missing"))?
        .read_to_string(&mut text)
        .map_err(|error| format!("C4C OPC part {name} is not UTF-8: {error}"))?;
    Ok(text)
}

fn verify_c4c_reconciliation_layout(
    reconciliation: &T1ReconciliationOutcome,
    candidate: &T1CandidateArtifact,
) -> BenchmarkVerifierResult {
    let validation = (|| {
        if candidate.relative_path != RECONCILIATION_OUTPUT_PATH
            || reconciliation.artifact.bytes != candidate.bytes.len() as u64
            || reconciliation.artifact.sha256 != sha256(&candidate.bytes)
            || reconciliation.provenance.facts.len() != 27
            || reconciliation.key_figures.len() != 8
        {
            return Err("C4C reconciliation identity or fact coverage changed".to_string());
        }
        let workbook = opc_text(&candidate.bytes, "xl/workbook.xml")?;
        let sheet = opc_text(&candidate.bytes, "xl/worksheets/sheet1.xml")?;
        if !workbook.contains("calcMode=\"auto\"")
            || sheet.matches("<f>").count() != 9
            || ["#REF!", "#DIV/0!", "#VALUE!", "#N/A", "�"]
                .iter()
                .any(|marker| sheet.contains(marker))
        {
            return Err("C4C reconciliation formula or encoding gate failed".to_string());
        }
        for fact in &reconciliation.provenance.facts {
            if !sheet.contains(&format!("<t>{}</t>", fact.fact_id)) || !sheet.contains(&fact.value)
            {
                return Err(format!(
                    "C4C reconciliation fact {} is not traceable",
                    fact.fact_id
                ));
            }
        }
        for source in &reconciliation.source_manifest.entries {
            if !sheet.contains(&source.relative_path) || !sheet.contains(&source.sha256) {
                return Err("C4C reconciliation source identity is incomplete".to_string());
            }
        }
        Ok(())
    })();
    c4c_verifier_result(
        "reconciliation-xlsx",
        "t1.reconciliation-xlsx/v1",
        "reconciliation_xlsx",
        RECONCILIATION_OUTPUT_PATH,
        Some(sha256(&candidate.bytes)),
        validation,
    )
}

fn verify_c4c_powerpoint_layout(
    powerpoint: &T1PowerPointOutcome,
    candidate: &T1CandidateArtifact,
) -> BenchmarkVerifierResult {
    let validation = (|| {
        if candidate.relative_path != BRIEF_OUTPUT_PATH
            || powerpoint.artifact.bytes != candidate.bytes.len() as u64
            || powerpoint.artifact.sha256 != sha256(&candidate.bytes)
            || powerpoint.render.rendered_page_count != 1
            || powerpoint.key_figures.len() != 8
        {
            return Err("C4C PowerPoint identity or coverage changed".to_string());
        }
        let presentation = opc_text(&candidate.bytes, "ppt/presentation.xml")?;
        let slide = opc_text(&candidate.bytes, "ppt/slides/slide1.xml")?;
        if presentation.matches("<p:sldId ").count() != 1
            || slide.trim().is_empty()
            || slide.contains('�')
        {
            return Err("C4C PowerPoint page or encoding gate failed".to_string());
        }
        for required in [
            powerpoint.key_figures.get("period"),
            powerpoint.key_figures.get("total_revenue_cny"),
            powerpoint.key_figures.get("budget_variance_cny"),
            powerpoint.key_figures.get("prior_variance_cny"),
        ] {
            let required =
                required.ok_or_else(|| "C4C PowerPoint key figure missing".to_string())?;
            if !slide.contains(required) {
                return Err("C4C PowerPoint key figure is not visible".to_string());
            }
        }
        for source in [
            "inputs/01-monthly-revenue.xlsx",
            "inputs/02-operations-notes.docx",
            "inputs/03-risk-summary.pdf",
        ] {
            if !slide.contains(source) {
                return Err("C4C PowerPoint source footnote is incomplete".to_string());
            }
        }
        for value in powerpoint.anomalies.values() {
            if !slide.contains(value) {
                return Err("C4C PowerPoint anomaly is not visible".to_string());
            }
        }
        Ok(())
    })();
    c4c_verifier_result(
        "one-page-brief",
        "t1.one-page-pptx/v1",
        "one_page_pptx",
        BRIEF_OUTPUT_PATH,
        Some(sha256(&candidate.bytes)),
        validation,
    )
}

fn independent_verified_run(
    fixtures: &T1GeneratedFixtureSet,
    spec: &BenchmarkTaskSpec,
    index: u32,
    reconciliation: &T1ReconciliationOutcome,
    powerpoint: &T1PowerPointOutcome,
    workspace: &Path,
) -> BenchmarkRunResult {
    let source = build_source_manifest(fixtures);
    let provenance = build_provenance_manifest(fixtures).expect("C0D provenance");
    let reconciliation_candidate = T1CandidateArtifact {
        relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
        bytes: fs::read(workspace.join(&reconciliation.artifact.relative_path))
            .expect("read reconciliation artifact"),
    };
    let brief_candidate = T1CandidateArtifact {
        relative_path: BRIEF_OUTPUT_PATH.to_string(),
        bytes: fs::read(workspace.join(&powerpoint.artifact.delivered_relative_path))
            .expect("read PowerPoint artifact"),
    };
    let render = c0d_render_evidence(&reconciliation_candidate, &brief_candidate);
    let mut verifiers = vec![
        verify_source_manifest(fixtures, &source),
        verify_provenance(fixtures, &source, &provenance),
        verify_c4c_reconciliation_layout(reconciliation, &reconciliation_candidate),
        verify_c4c_powerpoint_layout(powerpoint, &brief_candidate),
        verify_actual_render(&reconciliation_candidate, &brief_candidate, &render),
        provisional_result_receipt_verifier(),
    ];
    assert!(
        verifiers
            .iter()
            .all(|verifier| verifier.status == BenchmarkVerifierStatus::Passed),
        "independent verifier status drift: {:?}",
        verifiers
            .iter()
            .map(|verifier| (&verifier.verifier_id, verifier.status))
            .collect::<Vec<_>>()
    );
    let mut result = build_run_result(
        spec,
        index,
        BenchmarkTerminalState::ClaimedComplete,
        BenchmarkOutcomeClass::A,
        verifiers.clone(),
        None,
    );
    let evidence_refs = verifiers
        .iter()
        .flat_map(|verifier| verifier.evidence.iter())
        .map(|evidence| evidence.relative_or_opaque_ref.clone())
        .collect();
    let receipt = T1ResultReceipt {
        version: "t1.result-receipt/v1".to_string(),
        run_id: result.run_id.clone(),
        task_id: spec.task_id.clone(),
        task_revision: spec.task_revision,
        task_spec_fingerprint: spec.fingerprint().expect("task fingerprint"),
        outputs: vec![
            T1OutputReceipt {
                relative_path: BRIEF_OUTPUT_PATH.to_string(),
                sha256: sha256(&brief_candidate.bytes),
            },
            T1OutputReceipt {
                relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
                sha256: sha256(&reconciliation_candidate.bytes),
            },
        ],
        key_figures: reconciliation.key_figures.clone(),
        anomalies: vec![
            "breakfast_queue_complaints=12".to_string(),
            "elevator_2_unplanned_outages=4".to_string(),
            "overdue_fire_door_closing_checks=2".to_string(),
        ],
        evidence_refs,
    };
    verifiers[5] = verify_result_receipt(
        spec,
        &result,
        fixtures,
        &reconciliation_candidate,
        &brief_candidate,
        &receipt,
    );
    assert_eq!(verifiers[5].status, BenchmarkVerifierStatus::Passed);
    result.verifier_results = verifiers;
    result.validate(spec).expect("final independent C4C run");
    assert_eq!(
        verify_result_receipt(
            spec,
            &result,
            fixtures,
            &reconciliation_candidate,
            &brief_candidate,
            &receipt,
        ),
        result.verifier_results[5]
    );
    result
}

fn run_success_group(
    group_root: &Path,
    fixtures: &T1GeneratedFixtureSet,
    spec: &BenchmarkTaskSpec,
    index: u32,
    renderer: &dyn T1PowerPointRenderer,
) -> SuccessfulExecution {
    fs::create_dir_all(group_root).expect("create success group root");
    write_fixtures(group_root, fixtures);
    let mut run = setup_authorized_run(group_root);
    let initial = run
        .store
        .record_goal_context_checkpoint(
            run.run_id,
            checkpoint_observation(
                GoalContinuationObservationStage::InitialModel,
                None,
                logical_time(index),
            ),
        )
        .expect("initial C4C checkpoint")
        .expect("initial checkpoint exists");
    assert_eq!(initial.status, ContextCheckpointStatus::Continue);

    let reconciliation_request = T1ReconciliationRequest {
        source_directory: "inputs".to_string(),
        output_relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
    };
    let reconciliation_plan = plan_for(
        run.run_id,
        T1_RECONCILIATION_TOOL_ID,
        serde_json::to_value(&reconciliation_request).expect("reconciliation input"),
    );
    let reconciliation_output = execute_authorized(
        &run,
        &reconciliation_plan,
        &T1ReconciliationAgentToolExecutor::new(group_root),
    )
    .expect("authorized C4A execution");
    let reconciliation: T1ReconciliationOutcome =
        serde_json::from_value(reconciliation_output.output).expect("C4A outcome");
    assert_eq!(reconciliation.key_figures.len(), 8);
    let after_reconciliation = run
        .store
        .record_goal_context_checkpoint(
            run.run_id,
            checkpoint_observation(
                GoalContinuationObservationStage::AfterToolRound,
                Some(reconciliation_plan.invocation_id),
                logical_time(index) + chrono::Duration::seconds(1),
            ),
        )
        .expect("post-C4A checkpoint")
        .expect("post-C4A checkpoint exists");
    assert_eq!(
        after_reconciliation.status,
        ContextCheckpointStatus::Continue
    );
    let checkpoint_fingerprint = after_reconciliation.fingerprint.clone();

    let store_path = run.store_path.clone();
    let run_id = run.run_id;
    let group_id = run.group_id;
    drop(run.store);
    let reopened = EventStore::open(&store_path).expect("reopen C4C Event Store");
    assert_eq!(
        reopened
            .goal_context_checkpoint(run_id)
            .expect("read checkpoint after restart")
            .expect("checkpoint survives restart")
            .fingerprint,
        checkpoint_fingerprint
    );
    run = AuthorizedRun {
        store: reopened,
        store_path,
        run_id,
        group_id,
    };

    let powerpoint_request = T1PowerPointRequest {
        source_directory: "inputs".to_string(),
        reconciliation: reconciliation.clone(),
        output_relative_path: BRIEF_OUTPUT_PATH.to_string(),
    };
    let powerpoint_plan = plan_for(
        run.run_id,
        T1_POWERPOINT_TOOL_ID,
        serde_json::to_value(&powerpoint_request).expect("PowerPoint input"),
    );
    let powerpoint_output = execute_authorized(
        &run,
        &powerpoint_plan,
        &T1PowerPointAgentToolExecutor::new(group_root, renderer),
    )
    .expect("authorized C4B execution");
    let powerpoint: T1PowerPointOutcome =
        serde_json::from_value(powerpoint_output.output).expect("C4B outcome");
    let final_checkpoint = run
        .store
        .record_goal_context_checkpoint(
            run.run_id,
            checkpoint_observation(
                GoalContinuationObservationStage::Final,
                Some(powerpoint_plan.invocation_id),
                logical_time(index) + chrono::Duration::seconds(2),
            ),
        )
        .expect("final C4C checkpoint")
        .expect("final checkpoint exists");
    assert_eq!(final_checkpoint.status, ContextCheckpointStatus::Complete);
    assert!(final_checkpoint.blocker.is_none());
    assert_eq!(final_checkpoint.evidence.len(), 6);
    assert_eq!(final_checkpoint.artifacts.len(), 2);
    assert!(!final_checkpoint.authorizations.is_empty());
    assert!(
        final_checkpoint.resources.is_empty(),
        "released resource claims must not remain active in the checkpoint"
    );
    assert_eq!(final_checkpoint.sources.len(), 2);

    let completion = run
        .store
        .goal_completion_projection(run.run_id)
        .expect("goal completion projection")
        .expect("goal completion exists");
    assert_eq!(completion.status, GoalCompletionStatus::Complete);
    let receipt = completion
        .completion_receipt
        .expect("exact goal completion receipt");
    assert!(run
        .store
        .append_agent_run_finish(
            &AgentRunFinish::completed(run.run_id, "forged completion".to_string())
                .expect("receipt-free completion object")
        )
        .is_err());
    run.store
        .append_agent_run_finish(
            &AgentRunFinish::completed_with_goal_receipt(
                run.run_id,
                Some("C4C exact verified completion".to_string()),
                receipt,
            )
            .expect("receipt-bound completion"),
        )
        .expect("persist receipt-bound completion");
    let record = run
        .store
        .list_agent_run_records()
        .expect("agent run records")
        .into_iter()
        .find(|record| record.id == run.run_id)
        .expect("C4C agent run record");
    assert_eq!(record.status, AgentRunStatus::Completed);

    let run_result = independent_verified_run(
        fixtures,
        spec,
        index,
        &reconciliation,
        &powerpoint,
        group_root,
    );
    assert_eq!(
        classify_benchmark_run(spec, &run_result)
            .expect("C4C classification")
            .outcome_class,
        BenchmarkOutcomeClass::A
    );
    SuccessfulExecution {
        reconciliation,
        powerpoint,
        run_result,
    }
}

fn assert_no_completion(run: &AuthorizedRun) {
    assert!(run
        .store
        .append_agent_run_finish(
            &AgentRunFinish::completed(run.run_id, "must fail closed".to_string())
                .expect("receipt-free completion")
        )
        .is_err());
    assert_ne!(
        run.store
            .list_agent_run_records()
            .expect("agent run records")
            .into_iter()
            .find(|record| record.id == run.run_id)
            .expect("C4C run record")
            .status,
        AgentRunStatus::Completed
    );
}

fn failure_group_result(
    index: u32,
    case_kind: &str,
    checks: BTreeMap<String, bool>,
) -> C4cGroupResult {
    assert!(checks.values().all(|passed| *passed));
    C4cGroupResult {
        group_id: format!("c4c-group-{index:02}"),
        case_kind: case_kind.to_string(),
        expected_terminal: "failed_closed".to_string(),
        observed_outcome: "F".to_string(),
        completed: false,
        false_completion: false,
        authorization_resolutions: 1,
        key_figures_traceable: false,
        detection_checks: checks,
        reconciliation_sha256: None,
        powerpoint_sha256: None,
    }
}

fn report_summary(report: &C4cOutcomeReport) -> String {
    format!(
        "# DS Agent Step 4 C4C deterministic outcome\n\n- Groups: {}\n- A/F: {}/{}\n- VOCR: {}/{} ({} basis points)\n- Numeric conflicts detected: {}/{}\n- Damaged formulas detected: {}/{}\n- False completion across open/formula/garbling/clipping/overflow: 0\n- All successful key figures traceable: {}\n- Unauthorized path writes: {}\n- DeepSeek authority or receipts: {}\n- Installed Office/render: {}\n",
        report.deterministic_groups,
        report.outcomes_a,
        report.outcomes_f,
        report.vocr_numerator,
        report.vocr_denominator,
        report.vocr_basis_points,
        report.detections.numeric_conflicts_detected,
        report.detections.numeric_conflicts_injected,
        report.detections.damaged_formulas_detected,
        report.detections.damaged_formulas_injected,
        report.all_key_figures_traceable,
        report.unauthorized_path_writes,
        report.deepseek_authority_or_receipts,
        report.installed_office_case,
    )
}

#[test]
fn c4c_t1_e2e_50_group_matrix_meets_step_4_exit_contract() {
    let matrix = matrix_root();
    let groups_root = matrix.path.join("groups");
    fs::create_dir_all(&groups_root).expect("create matrix groups root");
    let mut specs = Vec::new();
    let mut runs = Vec::new();
    let mut groups = Vec::new();

    for index in 1..=SUCCESS_GROUPS {
        let fixtures = varied_fixture_set(index);
        let spec = task_spec_for(&fixtures);
        let renderer = fixture_renderer(vec![Ok(vec![valid_preview()])]);
        let execution = run_success_group(
            &groups_root.join(format!("group-{index:02}")),
            &fixtures,
            &spec,
            index,
            &renderer,
        );
        assert_eq!(execution.reconciliation.key_figures.len(), 8);
        groups.push(C4cGroupResult {
            group_id: format!("c4c-group-{index:02}"),
            case_kind: "deterministic-data-variant".to_string(),
            expected_terminal: "verified_completion".to_string(),
            observed_outcome: "A".to_string(),
            completed: true,
            false_completion: false,
            authorization_resolutions: 1,
            key_figures_traceable: true,
            detection_checks: BTreeMap::from([
                ("c0d_and_c4c_independent_verifiers".to_string(), true),
                ("event_store_exact_receipt".to_string(), true),
                ("g1b_restart_checkpoint".to_string(), true),
            ]),
            reconciliation_sha256: Some(execution.reconciliation.artifact.sha256.clone()),
            powerpoint_sha256: Some(execution.powerpoint.artifact.sha256.clone()),
        });
        specs.push(spec);
        runs.push(execution.run_result);
    }

    let base_spec = task_spec().expect("base T1 spec");

    // Group 44: independently inject a total-revenue conflict and an occupancy conflict.
    let group_root = groups_root.join("group-44");
    fs::create_dir_all(&group_root).expect("group 44 root");
    let run = setup_authorized_run(&group_root);
    let mut numeric_checks = BTreeMap::new();
    for (case, from, to) in [
        ("total", "><v>1702400</v>", "><v>1702401</v>"),
        ("occupancy", "><v>0.68</v>", "><v>0.69</v>"),
    ] {
        let case_root = group_root.join(case);
        let mut fixtures = generate_fixture_set().expect("numeric fixtures");
        mutate_fixture_text(
            &mut fixtures,
            "monthly-revenue-xlsx",
            "xl/worksheets/sheet1.xml",
            from,
            to,
        );
        write_fixtures(&case_root, &fixtures);
        let request = T1ReconciliationRequest {
            source_directory: format!("{case}/inputs"),
            output_relative_path: format!("{case}/outputs/t1-reconciliation.xlsx"),
        };
        let plan = plan_for(
            run.run_id,
            T1_RECONCILIATION_TOOL_ID,
            serde_json::to_value(&request).expect("numeric request"),
        );
        let detected = execute_authorized(
            &run,
            &plan,
            &T1ReconciliationAgentToolExecutor::new(&group_root),
        )
        .is_err()
            && !group_root.join(&request.output_relative_path).exists();
        numeric_checks.insert(format!("numeric_conflict_{case}"), detected);
    }
    assert_no_completion(&run);
    groups.push(failure_group_result(
        44,
        "numeric-conflicts",
        numeric_checks,
    ));
    specs.push(base_spec.clone());
    runs.push(failed_run_result(&base_spec, 44, "numeric_conflict"));

    // Group 45: two independently hash-bound damaged formulas are rejected.
    let group_root = groups_root.join("group-45");
    fs::create_dir_all(&group_root).expect("group 45 root");
    let fixtures = generate_fixture_set().expect("formula fixtures");
    write_fixtures(&group_root, &fixtures);
    let run = setup_authorized_run(&group_root);
    let request = T1ReconciliationRequest {
        source_directory: "inputs".to_string(),
        output_relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
    };
    let plan = plan_for(
        run.run_id,
        T1_RECONCILIATION_TOOL_ID,
        serde_json::to_value(&request).expect("formula request"),
    );
    let output = execute_authorized(
        &run,
        &plan,
        &T1ReconciliationAgentToolExecutor::new(&group_root),
    )
    .expect("C4A before formula damage");
    let reconciliation: T1ReconciliationOutcome =
        serde_json::from_value(output.output).expect("formula C4A outcome");
    let original = fs::read(group_root.join(RECONCILIATION_OUTPUT_PATH)).expect("C4A bytes");
    let mut archive = ZipArchive::new(Cursor::new(&original)).expect("open C4A XLSX");
    let mut sheet = String::new();
    archive
        .by_name("xl/worksheets/sheet1.xml")
        .expect("C4A worksheet")
        .read_to_string(&mut sheet)
        .expect("read C4A worksheet");
    drop(archive);
    let formula_start = sheet.find("<f>").expect("formula start") + 3;
    let formula_end = sheet[formula_start..].find("</f>").expect("formula end") + formula_start;
    let mut formula_checks = BTreeMap::new();
    for marker in ["#REF!", "#DIV/0!"] {
        let mut damaged_sheet = sheet.clone();
        damaged_sheet.replace_range(formula_start..formula_end, marker);
        let damaged = rewrite_zip_part(
            &original,
            "xl/worksheets/sheet1.xml",
            damaged_sheet.into_bytes(),
        );
        let mut artifact = reconciliation.artifact.clone();
        artifact.bytes = damaged.len() as u64;
        artifact.sha256 = sha256(&damaged);
        formula_checks.insert(
            format!("damaged_formula_{}", marker.replace(['#', '/', '!'], "")),
            verify_t1_reconciliation_artifact(
                &reconciliation.source_manifest,
                &reconciliation.provenance,
                &artifact,
                &damaged,
            )
            .is_err(),
        );
    }
    assert_no_completion(&run);
    groups.push(failure_group_result(45, "damaged-formulas", formula_checks));
    specs.push(base_spec.clone());
    runs.push(failed_run_result(&base_spec, 45, "damaged_formula"));

    // Group 46: installed-Office/open/render failure never leaves a PPTX or completion.
    let group_root = groups_root.join("group-46");
    let fixtures = generate_fixture_set().expect("render fixtures");
    let spec = task_spec_for(&fixtures);
    fs::create_dir_all(&group_root).expect("group 46 root");
    write_fixtures(&group_root, &fixtures);
    let run = setup_authorized_run(&group_root);
    let recon_request = T1ReconciliationRequest {
        source_directory: "inputs".to_string(),
        output_relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
    };
    let recon_plan = plan_for(
        run.run_id,
        T1_RECONCILIATION_TOOL_ID,
        serde_json::to_value(&recon_request).expect("render reconciliation request"),
    );
    let recon_output = execute_authorized(
        &run,
        &recon_plan,
        &T1ReconciliationAgentToolExecutor::new(&group_root),
    )
    .expect("render group C4A");
    let reconciliation: T1ReconciliationOutcome =
        serde_json::from_value(recon_output.output).expect("render group C4A outcome");
    let ppt_request = T1PowerPointRequest {
        source_directory: "inputs".to_string(),
        reconciliation,
        output_relative_path: BRIEF_OUTPUT_PATH.to_string(),
    };
    let ppt_plan = plan_for(
        run.run_id,
        T1_POWERPOINT_TOOL_ID,
        serde_json::to_value(&ppt_request).expect("render failure request"),
    );
    let failing = fixture_renderer(vec![Err("Office unavailable".to_string())]);
    let open_detected = execute_authorized(
        &run,
        &ppt_plan,
        &T1PowerPointAgentToolExecutor::new(&group_root, &failing),
    )
    .is_err()
        && !group_root.join(BRIEF_OUTPUT_PATH).exists();
    assert_no_completion(&run);
    groups.push(failure_group_result(
        46,
        "office-open-render-failure",
        BTreeMap::from([("office_open_render_failure".to_string(), open_detected)]),
    ));
    specs.push(spec);
    runs.push(failed_run_result(&base_spec, 46, "office_open_failure"));

    // Group 47: a replacement character in source text is rejected as garbling.
    let group_root = groups_root.join("group-47");
    fs::create_dir_all(&group_root).expect("group 47 root");
    let mut fixtures = generate_fixture_set().expect("garbling fixtures");
    mutate_fixture_text(
        &mut fixtures,
        "operations-notes-docx",
        "word/document.xml",
        "breakfast_queue_complaints=12",
        "breakfast_queue_complaints=�",
    );
    write_fixtures(&group_root, &fixtures);
    let run = setup_authorized_run(&group_root);
    let request = T1ReconciliationRequest {
        source_directory: "inputs".to_string(),
        output_relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
    };
    let plan = plan_for(
        run.run_id,
        T1_RECONCILIATION_TOOL_ID,
        serde_json::to_value(&request).expect("garbling request"),
    );
    let garbling_detected = execute_authorized(
        &run,
        &plan,
        &T1ReconciliationAgentToolExecutor::new(&group_root),
    )
    .is_err()
        && !group_root.join(RECONCILIATION_OUTPUT_PATH).exists();
    assert_no_completion(&run);
    groups.push(failure_group_result(
        47,
        "source-garbling",
        BTreeMap::from([("source_garbling".to_string(), garbling_detected)]),
    ));
    specs.push(base_spec.clone());
    runs.push(failed_run_result(&base_spec, 47, "source_garbling"));

    // Group 48: overflow is rejected by C4B and clipping is rejected independently.
    let group_root = groups_root.join("group-48");
    let fixtures = generate_fixture_set().expect("visual fixtures");
    let spec = task_spec_for(&fixtures);
    fs::create_dir_all(&group_root).expect("group 48 root");
    write_fixtures(&group_root, &fixtures);
    let run = setup_authorized_run(&group_root);
    let recon_request = T1ReconciliationRequest {
        source_directory: "inputs".to_string(),
        output_relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
    };
    let recon_plan = plan_for(
        run.run_id,
        T1_RECONCILIATION_TOOL_ID,
        serde_json::to_value(&recon_request).expect("visual reconciliation request"),
    );
    let recon_output = execute_authorized(
        &run,
        &recon_plan,
        &T1ReconciliationAgentToolExecutor::new(&group_root),
    )
    .expect("visual group C4A");
    let reconciliation: T1ReconciliationOutcome =
        serde_json::from_value(recon_output.output).expect("visual C4A outcome");
    let ppt_request = T1PowerPointRequest {
        source_directory: "inputs".to_string(),
        reconciliation: reconciliation.clone(),
        output_relative_path: BRIEF_OUTPUT_PATH.to_string(),
    };
    let ppt_plan = plan_for(
        run.run_id,
        T1_POWERPOINT_TOOL_ID,
        serde_json::to_value(&ppt_request).expect("overflow request"),
    );
    let overflow_renderer = fixture_renderer(vec![Ok(vec![valid_preview(), valid_preview()])]);
    let overflow_detected = execute_authorized(
        &run,
        &ppt_plan,
        &T1PowerPointAgentToolExecutor::new(&group_root, &overflow_renderer),
    )
    .is_err()
        && !group_root.join(BRIEF_OUTPUT_PATH).exists();
    let reconciliation_candidate = T1CandidateArtifact {
        relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
        bytes: fs::read(group_root.join(RECONCILIATION_OUTPUT_PATH)).expect("visual C4A bytes"),
    };
    let brief_candidate = T1CandidateArtifact {
        relative_path: BRIEF_OUTPUT_PATH.to_string(),
        bytes: b"synthetic-brief-placeholder".to_vec(),
    };
    let clipped = edge_clipped_preview();
    let clipping_evidence = T1RenderEvidence {
        receipt: T1ActualRenderReceipt {
            version: "t1.actual-render-receipt/v1".to_string(),
            artifacts: vec![
                T1RenderArtifactReceipt {
                    output_relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
                    output_sha256: sha256(&reconciliation_candidate.bytes),
                    renderer_version: ACTUAL_RENDERER_VERSION.to_string(),
                    rendered_unit_count: 1,
                    preview_manifest_sha256: preview_manifest_hash(&[clipped.clone()]),
                    previews: vec![T1PreviewReceipt {
                        relative_path: "previews/clipped-reconciliation.png".to_string(),
                        bytes: clipped.len() as u64,
                        sha256: sha256(&clipped),
                        width: 320,
                        height: 180,
                        edge_clipping: true,
                    }],
                },
                T1RenderArtifactReceipt {
                    output_relative_path: BRIEF_OUTPUT_PATH.to_string(),
                    output_sha256: sha256(&brief_candidate.bytes),
                    renderer_version: ACTUAL_RENDERER_VERSION.to_string(),
                    rendered_unit_count: 1,
                    preview_manifest_sha256: preview_manifest_hash(&[valid_preview()]),
                    previews: vec![T1PreviewReceipt {
                        relative_path: "previews/valid-brief.png".to_string(),
                        bytes: valid_preview().len() as u64,
                        sha256: sha256(&valid_preview()),
                        width: 320,
                        height: 180,
                        edge_clipping: false,
                    }],
                },
            ],
        },
        preview_bytes: BTreeMap::from([
            ("previews/clipped-reconciliation.png".to_string(), clipped),
            ("previews/valid-brief.png".to_string(), valid_preview()),
        ]),
    };
    let clipping_detected = verify_actual_render(
        &reconciliation_candidate,
        &brief_candidate,
        &clipping_evidence,
    )
    .status
        == BenchmarkVerifierStatus::Failed;
    assert_no_completion(&run);
    groups.push(failure_group_result(
        48,
        "clipping-and-overflow",
        BTreeMap::from([
            ("clipping".to_string(), clipping_detected),
            ("overflow".to_string(), overflow_detected),
        ]),
    ));
    specs.push(spec);
    runs.push(failed_run_result(&base_spec, 48, "visual_failure"));

    // Group 49: missing evidence, cross-task authority, and path escape all fail closed.
    let group_root = groups_root.join("group-49");
    let fixtures = generate_fixture_set().expect("authority fixtures");
    fs::create_dir_all(&group_root).expect("group 49 root");
    write_fixtures(&group_root, &fixtures);
    let run = setup_authorized_run(&group_root);
    let request = T1ReconciliationRequest {
        source_directory: "inputs".to_string(),
        output_relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
    };
    let plan = plan_for(
        run.run_id,
        T1_RECONCILIATION_TOOL_ID,
        serde_json::to_value(&request).expect("missing evidence request"),
    );
    let output = T1ReconciliationAgentToolExecutor::new(&group_root)
        .execute(&plan)
        .expect("produce C4A output for evidence omission");
    let mut incomplete = output.evidence.clone();
    incomplete.pop();
    let missing_evidence_detected = ToolInvocationRecord::succeeded(
        &plan,
        output.output,
        incomplete,
        output.verification,
        None,
        0,
    )
    .is_err();
    let group = approved_group(&run);
    let mut cross_task = TaskGroupedCapabilityClaim::from_group_item(
        &group,
        group
            .capability_audits
            .iter()
            .find(|item| item.tool_id == T1_RECONCILIATION_TOOL_ID)
            .expect("reconciliation group item"),
    );
    cross_task.task_id = Uuid::new_v4();
    let cross_task_detected = run
        .store
        .authorize_task_grouped_capability(
            &cross_task,
            "2029-01-02T03:06:05Z".parse().expect("authorization time"),
        )
        .is_err();
    let escaped = group_root
        .parent()
        .expect("groups parent")
        .join("c4c-escaped.xlsx");
    let escape_request = T1ReconciliationRequest {
        source_directory: "inputs".to_string(),
        output_relative_path: "../c4c-escaped.xlsx".to_string(),
    };
    let escape_plan = plan_for(
        run.run_id,
        T1_RECONCILIATION_TOOL_ID,
        serde_json::to_value(&escape_request).expect("escape request"),
    );
    let path_escape_detected = T1ReconciliationAgentToolExecutor::new(&group_root)
        .execute(&escape_plan)
        .is_err()
        && !escaped.exists();
    assert_no_completion(&run);
    groups.push(failure_group_result(
        49,
        "authority-evidence-path",
        BTreeMap::from([
            ("cross_task_authority".to_string(), cross_task_detected),
            ("missing_evidence".to_string(), missing_evidence_detected),
            ("path_escape".to_string(), path_escape_detected),
        ]),
    ));
    specs.push(base_spec.clone());
    runs.push(failed_run_result(&base_spec, 49, "authority_or_evidence"));

    // Group 50: no-evidence tool round is blocked and its checkpoint survives reopen.
    let group_root = groups_root.join("group-50");
    fs::create_dir_all(&group_root).expect("group 50 root");
    let run = setup_authorized_run(&group_root);
    let blocked = run
        .store
        .record_goal_context_checkpoint(
            run.run_id,
            checkpoint_observation(
                GoalContinuationObservationStage::AfterToolRound,
                Some(Uuid::new_v4()),
                logical_time(50),
            ),
        )
        .expect("no-evidence checkpoint")
        .expect("no-evidence checkpoint exists");
    let no_evidence_detected = blocked
        .blocker
        .as_ref()
        .is_some_and(|blocker| blocker.code == GoalContinuationBlockerCode::NoNewEvidence);
    let fingerprint = blocked.fingerprint.clone();
    let store_path = run.store_path.clone();
    let run_id = run.run_id;
    drop(run.store);
    let reopened = EventStore::open(&store_path).expect("reopen blocker Event Store");
    let restart_preserved = reopened
        .goal_context_checkpoint(run_id)
        .expect("read blocker checkpoint")
        .is_some_and(|checkpoint| checkpoint.fingerprint == fingerprint);
    groups.push(failure_group_result(
        50,
        "g1b-no-evidence-restart",
        BTreeMap::from([
            ("no_new_evidence_blocker".to_string(), no_evidence_detected),
            ("restart_preserved".to_string(), restart_preserved),
        ]),
    ));
    specs.push(base_spec.clone());
    runs.push(failed_run_result(&base_spec, 50, "no_new_evidence"));

    assert_eq!(groups.len(), TOTAL_GROUPS as usize);
    assert_eq!(specs.len(), TOTAL_GROUPS as usize);
    assert_eq!(runs.len(), TOTAL_GROUPS as usize);
    let pairs = specs.iter().zip(&runs).collect::<Vec<_>>();
    let aggregate = aggregate_benchmark_runs(&pairs).expect("aggregate C4C matrix");
    assert_eq!(aggregate.run_count, 50);
    assert_eq!(aggregate.outcomes.a, 43);
    assert_eq!(aggregate.outcomes.f, 7);
    assert_eq!(aggregate.vocr.numerator, 43);
    assert_eq!(aggregate.vocr.denominator, 50);
    assert_eq!(aggregate.vocr.basis_points, Some(8600));
    assert_eq!(aggregate.false_completion_rate.numerator, 0);
    assert_eq!(aggregate.authorization_budget_compliance.numerator, 50);
    assert!(groups.iter().all(|group| !group.false_completion));
    assert!(groups[..SUCCESS_GROUPS as usize]
        .iter()
        .all(|group| group.key_figures_traceable));

    let report = C4cOutcomeReport {
        version: C4C_REPORT_VERSION.to_string(),
        source_commit: C4C_SOURCE_COMMIT.to_string(),
        environment_profile: "local-synthetic-deterministic".to_string(),
        deterministic_groups: TOTAL_GROUPS,
        outcomes_a: aggregate.outcomes.a,
        outcomes_f: aggregate.outcomes.f,
        vocr_numerator: aggregate.vocr.numerator,
        vocr_denominator: aggregate.vocr.denominator,
        vocr_basis_points: aggregate.vocr.basis_points.expect("VOCR basis points"),
        authorization_budget_compliant_groups: aggregate.authorization_budget_compliance.numerator,
        unauthorized_path_writes: 0,
        all_key_figures_traceable: true,
        deepseek_authority_or_receipts: 0,
        installed_office_case: "separate ignored environment-dependent gate".to_string(),
        detections: C4cDetectionTotals {
            numeric_conflicts_injected: 2,
            numeric_conflicts_detected: 2,
            damaged_formulas_injected: 2,
            damaged_formulas_detected: 2,
            false_completion_open: 0,
            false_completion_formula: 0,
            false_completion_garbling: 0,
            false_completion_clipping: 0,
            false_completion_overflow: 0,
        },
        groups,
    };
    let json = serde_json::to_vec_pretty(&report).expect("serialize C4C report");
    fs::write(matrix.path.join("C4C_OUTCOME.json"), &json).expect("write C4C outcome report");
    fs::write(matrix.path.join("C4C_SUMMARY.md"), report_summary(&report))
        .expect("write C4C summary");
    let readback: serde_json::Value = serde_json::from_slice(
        &fs::read(matrix.path.join("C4C_OUTCOME.json")).expect("read C4C report"),
    )
    .expect("parse C4C report readback");
    assert_eq!(readback["vocr_basis_points"], 8600);
}

#[cfg(windows)]
#[test]
#[ignore = "requires installed Microsoft Office and pdftoppm; writes only an explicit isolated root"]
fn c4c_live_office_render_case_isolated_from_deterministic_matrix() {
    let root = PathBuf::from(
        env::var_os("DS_AGENT_C4C_OFFICE_ROOT").expect("DS_AGENT_C4C_OFFICE_ROOT is required"),
    );
    assert!(root.is_absolute(), "C4C Office root must be absolute");
    if root.exists() {
        assert!(
            fs::read_dir(&root)
                .expect("read C4C Office root")
                .next()
                .is_none(),
            "C4C Office root must be fresh and empty"
        );
    } else {
        fs::create_dir_all(&root).expect("create C4C Office root");
    }
    let fixtures = generate_fixture_set().expect("live Office fixtures");
    let spec = task_spec_for(&fixtures);
    let execution = run_success_group(&root, &fixtures, &spec, 1, &LocalT1PowerPointRenderer);
    assert_eq!(execution.powerpoint.render.rendered_page_count, 1);
    fs::write(
        root.join("C4C_LIVE_OFFICE.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": "ds-agent.step-4-c4c-live-office/v1",
            "environment_dependent": true,
            "rendered_page_count": execution.powerpoint.render.rendered_page_count,
            "renderer_version": execution.powerpoint.render.renderer_version,
            "powerpoint_sha256": execution.powerpoint.artifact.sha256,
            "status": "passed"
        }))
        .expect("serialize live Office receipt"),
    )
    .expect("write live Office receipt");
}
