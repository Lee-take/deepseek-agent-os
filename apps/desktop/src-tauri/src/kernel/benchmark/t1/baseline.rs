use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Cursor, Write};
use std::path::Path;
use std::time::Instant;

use chrono::{TimeZone, Utc};
use image::{DynamicImage, GrayImage, ImageFormat, Luma};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::kernel::artifact_render::ACTUAL_RENDERER_VERSION;
use crate::kernel::artifacts::preview_manifest_hash;

use super::fixtures::{
    generate_fixture_set, write_deterministic_zip, write_fixture_set, xml_escape,
    T1GeneratedFixture, T1GeneratedFixtureSet,
};
use super::verifiers::{
    build_provenance_manifest, build_source_manifest, verify_actual_render, verify_one_page_pptx,
    verify_provenance, verify_reconciliation_xlsx, verify_result_receipt, verify_source_manifest,
    T1ActualRenderReceipt, T1CandidateArtifact, T1OutputReceipt, T1PreviewReceipt,
    T1ProvenanceManifest, T1RenderArtifactReceipt, T1RenderEvidence, T1ResultReceipt,
    T1SourceManifest,
};
use super::{BRIEF_OUTPUT_PATH, RECONCILIATION_OUTPUT_PATH, RESULT_RECEIPT_VERIFIER_ID};
use crate::kernel::benchmark::runner::{
    sha256, BaselineArtifactHash, BaselineClassification, BaselineCompletionDecision,
    BaselineOfflineUsage, BaselineRunMetrics, BaselineRunRecord, BaselineWallClockObservation,
    BenchmarkRunnerConfig,
};
use crate::kernel::benchmark::{
    classify_benchmark_run, BenchmarkEvidenceReceipt, BenchmarkExternalEffectState,
    BenchmarkInteractions, BenchmarkOutcomeClass, BenchmarkRunResult, BenchmarkSubject,
    BenchmarkTaskSpec, BenchmarkTerminalState, BenchmarkVerifierResult, BenchmarkVerifierStatus,
    BENCHMARK_RUN_RESULT_VERSION,
};

pub(in crate::kernel::benchmark) const RUN_BINDING_FILE: &str = "run-binding.json";
const SOURCE_MANIFEST_FILE: &str = "receipts/source-manifest.json";
const PROVENANCE_MANIFEST_FILE: &str = "receipts/provenance-manifest.json";
const ACTUAL_RENDER_RECEIPT_FILE: &str = "receipts/actual-render-receipt.json";
const RESULT_RECEIPT_FILE: &str = "receipts/result-receipt.json";
const RUN_RESULT_FILE: &str = "receipts/benchmark-run-result.json";
const RECONCILIATION_PREVIEW: &str = "previews/t1-reconciliation-1.png";
const BRIEF_PREVIEW: &str = "previews/t1-monthly-brief-1.png";
const RUN_BINDING_VERSION: &str = "ds-agent.step-0-run-binding/v1";
const ACTUAL_RENDER_RECEIPT_VERSION: &str = "t1.actual-render-receipt/v1";
const RESULT_RECEIPT_VERSION: &str = "t1.result-receipt/v1";
const MAX_READ_BYTES: u64 = 16 * 1024 * 1024;
const CHECKED_FIXTURE_MANIFEST: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/benchmarks/t1/fixture-set-v1.json"
));

const OUTPUT_FILES: &[&str] = &[
    RECONCILIATION_OUTPUT_PATH,
    BRIEF_OUTPUT_PATH,
    RECONCILIATION_PREVIEW,
    BRIEF_PREVIEW,
    SOURCE_MANIFEST_FILE,
    PROVENANCE_MANIFEST_FILE,
    ACTUAL_RENDER_RECEIPT_FILE,
    RESULT_RECEIPT_FILE,
    RUN_RESULT_FILE,
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct T1RunBinding {
    version: String,
    run_id: String,
    repetition_index: u32,
    clean_state_id: String,
    clean_root_binding: String,
    task_spec_fingerprint: String,
    fixture_manifest_sha256: String,
}

pub(in crate::kernel::benchmark) fn checked_fixture_manifest_sha256() -> String {
    sha256(CHECKED_FIXTURE_MANIFEST)
}

pub(in crate::kernel::benchmark) fn expected_output_files() -> &'static [&'static str] {
    OUTPUT_FILES
}

pub(in crate::kernel::benchmark) fn execute_t1_run(
    config: &BenchmarkRunnerConfig,
    task_spec: &BenchmarkTaskSpec,
    fixture_manifest_sha256: &str,
    repetition_index: u32,
    workspace_root: &Path,
    fixture_root: &Path,
    output_root: &Path,
) -> Result<(BaselineRunRecord, BaselineWallClockObservation), String> {
    let observed = Instant::now();
    let run_id = format!("t1-run-{repetition_index:04}");
    let clean_state_id = format!("clean-state-{repetition_index:02}");
    let clean_root_binding = format!("run-{repetition_index:02}");
    let binding = T1RunBinding {
        version: RUN_BINDING_VERSION.to_string(),
        run_id: run_id.clone(),
        repetition_index,
        clean_state_id: clean_state_id.clone(),
        clean_root_binding: clean_root_binding.clone(),
        task_spec_fingerprint: task_spec.fingerprint()?,
        fixture_manifest_sha256: fixture_manifest_sha256.to_string(),
    };
    write_json_new(workspace_root, RUN_BINDING_FILE, &binding)?;

    write_fixture_set(fixture_root)?;
    let fixtures = read_fixture_set(fixture_root)?;
    let source_manifest = build_source_manifest(&fixtures);
    let provenance = build_provenance_manifest(&fixtures)?;
    let reconciliation = build_reconciliation(&provenance)?;
    let brief = build_brief(&provenance)?;
    let render = build_render_evidence(&reconciliation, &brief)?;

    write_json_new(output_root, SOURCE_MANIFEST_FILE, &source_manifest)?;
    write_json_new(output_root, PROVENANCE_MANIFEST_FILE, &provenance)?;
    write_file_new(
        output_root,
        &reconciliation.relative_path,
        &reconciliation.bytes,
    )?;
    write_file_new(output_root, &brief.relative_path, &brief.bytes)?;
    for (path, bytes) in &render.preview_bytes {
        write_file_new(output_root, path, bytes)?;
    }
    write_json_new(output_root, ACTUAL_RENDER_RECEIPT_FILE, &render.receipt)?;

    let disk_fixtures = read_fixture_set(fixture_root)?;
    let disk_source: T1SourceManifest = read_json(output_root, SOURCE_MANIFEST_FILE)?;
    let disk_provenance: T1ProvenanceManifest = read_json(output_root, PROVENANCE_MANIFEST_FILE)?;
    let disk_reconciliation = read_candidate(output_root, RECONCILIATION_OUTPUT_PATH)?;
    let disk_brief = read_candidate(output_root, BRIEF_OUTPUT_PATH)?;
    let disk_render = read_render_evidence(output_root)?;
    let mut verifier_results = vec![
        verify_source_manifest(&disk_fixtures, &disk_source),
        verify_provenance(&disk_fixtures, &disk_source, &disk_provenance),
        verify_reconciliation_xlsx(
            &disk_fixtures,
            &disk_source,
            &disk_provenance,
            &disk_reconciliation,
        ),
        verify_one_page_pptx(&disk_fixtures, &disk_brief),
        verify_actual_render(&disk_reconciliation, &disk_brief, &disk_render),
    ];
    if verifier_results
        .iter()
        .any(|result| result.status != BenchmarkVerifierStatus::Passed)
    {
        return Err("T1 failed before result-receipt verification".to_string());
    }
    verifier_results.push(provisional_result_receipt_verifier());
    let mut run_result = build_run_result(
        config,
        task_spec,
        repetition_index,
        &run_id,
        &clean_state_id,
        verifier_results,
    )?;
    let result_receipt = build_result_receipt(
        task_spec,
        &run_result,
        &disk_provenance,
        &disk_reconciliation,
        &disk_brief,
    )?;
    write_json_new(output_root, RESULT_RECEIPT_FILE, &result_receipt)?;
    let disk_receipt: T1ResultReceipt = read_json(output_root, RESULT_RECEIPT_FILE)?;
    let result = verify_result_receipt(
        task_spec,
        &run_result,
        &disk_fixtures,
        &disk_reconciliation,
        &disk_brief,
        &disk_receipt,
    );
    if result.status != BenchmarkVerifierStatus::Passed {
        return Err("T1 result receipt failed closed".to_string());
    }
    *run_result
        .verifier_results
        .last_mut()
        .ok_or_else(|| "T1 result receipt verifier slot is missing".to_string())? = result;
    let final_receipt_result = verify_result_receipt(
        task_spec,
        &run_result,
        &disk_fixtures,
        &disk_reconciliation,
        &disk_brief,
        &disk_receipt,
    );
    if final_receipt_result != run_result.verifier_results[5] {
        return Err("T1 final result receipt binding is unstable".to_string());
    }
    run_result.validate(task_spec)?;
    write_json_new(output_root, RUN_RESULT_FILE, &run_result)?;

    let record = readback_t1_run(
        config,
        task_spec,
        fixture_manifest_sha256,
        repetition_index,
        workspace_root,
        fixture_root,
        output_root,
    )?;
    let observation = BaselineWallClockObservation {
        run_id,
        wall_clock_ms: observed.elapsed().as_secs_f64() * 1_000.0,
        normative: false,
        note: "Observed runner wall-clock only; excluded from canonical report hash and PASS"
            .to_string(),
    };
    Ok((record, observation))
}

pub(in crate::kernel::benchmark) fn readback_t1_run(
    _config: &BenchmarkRunnerConfig,
    task_spec: &BenchmarkTaskSpec,
    fixture_manifest_sha256: &str,
    repetition_index: u32,
    workspace_root: &Path,
    fixture_root: &Path,
    output_root: &Path,
) -> Result<BaselineRunRecord, String> {
    let binding: T1RunBinding = read_json(workspace_root, RUN_BINDING_FILE)?;
    let expected_run_id = format!("t1-run-{repetition_index:04}");
    let expected_clean_state = format!("clean-state-{repetition_index:02}");
    let expected_clean_root = format!("run-{repetition_index:02}");
    if binding.version != RUN_BINDING_VERSION
        || binding.run_id != expected_run_id
        || binding.repetition_index != repetition_index
        || binding.clean_state_id != expected_clean_state
        || binding.clean_root_binding != expected_clean_root
        || binding.task_spec_fingerprint != task_spec.fingerprint()?
        || binding.fixture_manifest_sha256 != fixture_manifest_sha256
    {
        return Err("T1 run binding was reused or tampered".to_string());
    }
    let fixtures = read_fixture_set(fixture_root)?;
    let source_manifest: T1SourceManifest = read_json(output_root, SOURCE_MANIFEST_FILE)?;
    let provenance: T1ProvenanceManifest = read_json(output_root, PROVENANCE_MANIFEST_FILE)?;
    let reconciliation = read_candidate(output_root, RECONCILIATION_OUTPUT_PATH)?;
    let brief = read_candidate(output_root, BRIEF_OUTPUT_PATH)?;
    let render = read_render_evidence(output_root)?;
    let result_receipt: T1ResultReceipt = read_json(output_root, RESULT_RECEIPT_FILE)?;
    let run_json = fs::read_to_string(output_root.join(RUN_RESULT_FILE))
        .map_err(|error| format!("read T1 run result: {error}"))?;
    let run_result = BenchmarkRunResult::parse_str(&run_json, task_spec)?;
    let recomputed = vec![
        verify_source_manifest(&fixtures, &source_manifest),
        verify_provenance(&fixtures, &source_manifest, &provenance),
        verify_reconciliation_xlsx(&fixtures, &source_manifest, &provenance, &reconciliation),
        verify_one_page_pptx(&fixtures, &brief),
        verify_actual_render(&reconciliation, &brief, &render),
        verify_result_receipt(
            task_spec,
            &run_result,
            &fixtures,
            &reconciliation,
            &brief,
            &result_receipt,
        ),
    ];
    if recomputed.len() != task_spec.done_when.len()
        || recomputed
            .iter()
            .any(|result| result.status != BenchmarkVerifierStatus::Passed)
        || recomputed != run_result.verifier_results
    {
        return Err("T1 disk readback verifier outcomes changed or are incomplete".to_string());
    }
    let classification = classify_benchmark_run(task_spec, &run_result)?;
    if !classification.verifier_gate_passed
        || classification.false_completion
        || classification.outcome_class != BenchmarkOutcomeClass::A
    {
        return Err("T1 disk readback did not satisfy verified completion".to_string());
    }
    Ok(BaselineRunRecord {
        run_id: expected_run_id,
        repetition_index,
        clean_state_id: expected_clean_state,
        clean_root_binding: expected_clean_root,
        task_contract: task_spec.clone(),
        fixture_manifest_sha256: fixture_manifest_sha256.to_string(),
        classification: BaselineClassification {
            outcome_class: classification.outcome_class,
            verifier_gate_passed: classification.verifier_gate_passed,
            false_completion: classification.false_completion,
        },
        completion_decision: BaselineCompletionDecision::Completed,
        metrics: BaselineRunMetrics {
            outcome_class: classification.outcome_class,
            clarification_count: run_result.interactions.clarification_count,
            authorization_count: run_result.interactions.authorization_count,
            human_intervention_count: run_result.interactions.manual_interventions.len() as u32,
            logical_duration_ms: run_result.elapsed_ms,
            evidence_receipt_count: run_result
                .verifier_results
                .iter()
                .map(|result| result.evidence.len() as u32)
                .sum(),
            failure_stage: run_result.failure_stage.clone(),
            offline_usage: BaselineOfflineUsage::none(),
        },
        artifact_hashes: collect_artifact_hashes(
            workspace_root,
            fixture_root,
            output_root,
            task_spec,
        )?,
        run_result,
    })
}

fn build_run_result(
    config: &BenchmarkRunnerConfig,
    task_spec: &BenchmarkTaskSpec,
    repetition_index: u32,
    run_id: &str,
    clean_state_id: &str,
    verifier_results: Vec<BenchmarkVerifierResult>,
) -> Result<BenchmarkRunResult, String> {
    let second = repetition_index.saturating_sub(1);
    let logical_time = Utc
        .with_ymd_and_hms(2026, 7, 16, 8, 0, second)
        .single()
        .ok_or_else(|| "T1 logical timestamp is invalid".to_string())?;
    let result = BenchmarkRunResult {
        version: BENCHMARK_RUN_RESULT_VERSION.to_string(),
        run_id: run_id.to_string(),
        task_id: task_spec.task_id.clone(),
        task_revision: task_spec.task_revision,
        task_spec_fingerprint: task_spec.fingerprint()?,
        repetition_index,
        subject: BenchmarkSubject {
            app_version: config.app_version.clone(),
            source_commit: config.source_commit.clone(),
            release_tag: config.release_tag.clone(),
            environment_profile: config.environment_profile.clone(),
            clean_state_id: clean_state_id.to_string(),
            access_mode: config.access_mode,
        },
        started_at: logical_time,
        finished_at: logical_time,
        elapsed_ms: 0,
        terminal_state: BenchmarkTerminalState::ClaimedComplete,
        external_effect_state: BenchmarkExternalEffectState::None,
        outcome_class: BenchmarkOutcomeClass::A,
        interactions: BenchmarkInteractions {
            clarification_count: 0,
            authorization_count: 0,
            manual_interventions: Vec::new(),
        },
        deepseek_usage: Vec::new(),
        verifier_results,
        guardrail_violations: Vec::new(),
        failure_stage: None,
        failure_code: None,
    };
    result.validate(task_spec)?;
    Ok(result)
}

fn provisional_result_receipt_verifier() -> BenchmarkVerifierResult {
    BenchmarkVerifierResult {
        done_when_id: "result-receipt".to_string(),
        verifier_id: RESULT_RECEIPT_VERIFIER_ID.to_string(),
        status: BenchmarkVerifierStatus::Passed,
        summary: "Pending deterministic T1 result-receipt verification".to_string(),
        evidence: vec![BenchmarkEvidenceReceipt {
            kind: "result_receipt".to_string(),
            relative_or_opaque_ref: "benchmark-evidence:t1-result-receipt".to_string(),
            sha256: None,
            summary: "Ephemeral bootstrap binding; never persisted before verification".to_string(),
        }],
    }
}

fn build_result_receipt(
    task_spec: &BenchmarkTaskSpec,
    run_result: &BenchmarkRunResult,
    provenance: &T1ProvenanceManifest,
    reconciliation: &T1CandidateArtifact,
    brief: &T1CandidateArtifact,
) -> Result<T1ResultReceipt, String> {
    let facts = provenance_fact_map(provenance)?;
    Ok(T1ResultReceipt {
        version: RESULT_RECEIPT_VERSION.to_string(),
        run_id: run_result.run_id.clone(),
        task_id: task_spec.task_id.clone(),
        task_revision: task_spec.task_revision,
        task_spec_fingerprint: task_spec.fingerprint()?,
        outputs: vec![
            T1OutputReceipt {
                relative_path: reconciliation.relative_path.clone(),
                sha256: sha256(&reconciliation.bytes),
            },
            T1OutputReceipt {
                relative_path: brief.relative_path.clone(),
                sha256: sha256(&brief.bytes),
            },
        ],
        key_figures: BTreeMap::from([
            (
                "period".to_string(),
                fact_value(&facts, "period")?.to_string(),
            ),
            (
                "total_revenue_cny".to_string(),
                fact_value(&facts, "total_revenue_cny")?.to_string(),
            ),
            (
                "budget_variance_cny".to_string(),
                fact_value(&facts, "budget_variance_cny")?.to_string(),
            ),
            (
                "budget_variance_rate".to_string(),
                fact_value(&facts, "budget_variance_rate")?.to_string(),
            ),
            (
                "prior_variance_cny".to_string(),
                fact_value(&facts, "prior_variance_cny")?.to_string(),
            ),
            (
                "prior_variance_rate".to_string(),
                fact_value(&facts, "prior_variance_rate")?.to_string(),
            ),
            (
                "occupancy_rate".to_string(),
                fact_value(&facts, "occupancy_rate")?.to_string(),
            ),
            (
                "occupancy_variance_percentage_points".to_string(),
                fact_value(&facts, "occupancy_variance_percentage_points")?.to_string(),
            ),
        ]),
        anomalies: vec![
            format!(
                "breakfast_queue_complaints={}",
                fact_value(&facts, "breakfast_queue_complaints")?
            ),
            format!(
                "elevator_2_unplanned_outages={}",
                fact_value(&facts, "elevator_2_unplanned_outages")?
            ),
            format!(
                "overdue_fire_door_closing_checks={}",
                fact_value(&facts, "overdue_fire_door_closing_checks")?
            ),
        ],
        evidence_refs: run_result
            .verifier_results
            .iter()
            .flat_map(|result| {
                result
                    .evidence
                    .iter()
                    .map(|evidence| evidence.relative_or_opaque_ref.clone())
            })
            .collect(),
    })
}

fn build_reconciliation(provenance: &T1ProvenanceManifest) -> Result<T1CandidateArtifact, String> {
    let facts = provenance_fact_map(provenance)?;
    let source_rows = [
        (2, "period"),
        (3, "available_room_nights"),
        (4, "sold_room_nights"),
        (5, "reported_occupancy_rate"),
        (6, "rooms_revenue_cny"),
        (7, "reported_adr_cny"),
        (8, "food_beverage_revenue_cny"),
        (9, "other_revenue_cny"),
        (10, "reported_total_revenue_cny"),
        (11, "budget_total_revenue_cny"),
        (12, "prior_period_total_revenue_cny"),
        (13, "budget_occupancy_rate"),
        (14, "breakfast_queue_complaints"),
        (15, "overdue_invoice_corrections_over_48h"),
        (16, "group_leads_deferred_to_july"),
        (17, "elevator_2_unplanned_outages"),
        (18, "overdue_fire_door_closing_checks"),
        (19, "temporary_food_staff_retraining_incomplete"),
    ];
    let derived_rows = [
        (20, "occupancy_rate", "B4/B3"),
        (21, "adr_cny", "B6/B4"),
        (22, "revpar_cny", "B6/B3"),
        (23, "total_revenue_cny", "SUM(B6,B8,B9)"),
        (24, "budget_variance_cny", "B23-B11"),
        (25, "budget_variance_rate", "B24/B11"),
        (26, "prior_variance_cny", "B23-B12"),
        (27, "prior_variance_rate", "B26/B12"),
        (28, "occupancy_variance_percentage_points", "(B20-B13)*100"),
    ];
    let mut rows = vec![format!(
        "<row r=\"1\">{}{}</row>",
        inline_cell("A1", "fact_id"),
        inline_cell("C1", "provenance")
    )];
    for (row, fact_id) in source_rows {
        let fact = facts
            .get(fact_id)
            .ok_or_else(|| format!("missing T1 source fact {fact_id}"))?;
        let locator = fact
            .source
            .as_ref()
            .ok_or_else(|| format!("missing T1 source locator {fact_id}"))?;
        rows.push(format!(
            "<row r=\"{row}\">{}{}{}</row>",
            inline_cell(&format!("A{row}"), fact_id),
            value_cell(&format!("B{row}"), &fact.value, None),
            inline_cell(&format!("C{row}"), &locator.locator)
        ));
    }
    for (row, fact_id, formula) in derived_rows {
        let fact = facts
            .get(fact_id)
            .ok_or_else(|| format!("missing T1 derived fact {fact_id}"))?;
        let derivation = fact
            .derivation
            .as_ref()
            .ok_or_else(|| format!("missing T1 derivation {fact_id}"))?;
        rows.push(format!(
            "<row r=\"{row}\">{}{}{}</row>",
            inline_cell(&format!("A{row}"), fact_id),
            value_cell(&format!("B{row}"), &fact.value, Some(formula)),
            inline_cell(
                &format!("C{row}"),
                &format!("derived:{}", derivation.algorithm_id)
            )
        ));
    }
    let sheet = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><dimension ref=\"A1:C28\"/><sheetData>{}</sheetData></worksheet>",
        rows.join("")
    );
    let content_types = br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;
    let workbook = br#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Reconciliation" sheetId="1" r:id="rId1"/></sheets><calcPr calcMode="auto"/></workbook>"#;
    let workbook_rels = br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
    Ok(T1CandidateArtifact {
        relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
        bytes: zip_owned(vec![
            ("[Content_Types].xml".to_string(), content_types.to_vec()),
            (
                "_rels/.rels".to_string(),
                root_relationships("xl/workbook.xml").into_bytes(),
            ),
            (
                "docProps/core.xml".to_string(),
                core_properties().into_bytes(),
            ),
            ("xl/workbook.xml".to_string(), workbook.to_vec()),
            (
                "xl/_rels/workbook.xml.rels".to_string(),
                workbook_rels.to_vec(),
            ),
            ("xl/worksheets/sheet1.xml".to_string(), sheet.into_bytes()),
        ])?,
    })
}

fn build_brief(provenance: &T1ProvenanceManifest) -> Result<T1CandidateArtifact, String> {
    let facts = provenance_fact_map(provenance)?;
    let text = vec![
        "2026-06 月度经营简报".to_string(),
        format!(
            "总收入 {}",
            format_integer_grouped(fact_number(&facts, "total_revenue_cny")? as i64)
        ),
        format!(
            "预算差异 {} / {:.2}%",
            format_integer_grouped(fact_number(&facts, "budget_variance_cny")? as i64),
            fact_number(&facts, "budget_variance_rate")? * 100.0
        ),
        format!(
            "入住率 {:.2}% / {:.2} percentage points",
            fact_number(&facts, "occupancy_rate")? * 100.0,
            fact_number(&facts, "occupancy_variance_percentage_points")?
        ),
        format!(
            "早餐排队 {} 起",
            fact_value(&facts, "breakfast_queue_complaints")?
        ),
        format!(
            "电梯停运 {} 次",
            fact_value(&facts, "elevator_2_unplanned_outages")?
        ),
        format!(
            "防火门逾期 {} 项",
            fact_value(&facts, "overdue_fire_door_closing_checks")?
        ),
        "来源: 01-monthly-revenue.xlsx; 02-operations-notes.docx; 03-risk-summary.pdf".to_string(),
    ];
    let slide_xml = slide_xml(&text);
    let presentation = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><p:presentation xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><p:sldIdLst><p:sldId id=\"256\" r:id=\"rId1\"/></p:sldIdLst><p:sldSz cx=\"12192000\" cy=\"6858000\"/></p:presentation>";
    let presentation_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide1.xml\"/></Relationships>";
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/><Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/><Override PartName=\"/ppt/slides/slide1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>";
    Ok(T1CandidateArtifact {
        relative_path: BRIEF_OUTPUT_PATH.to_string(),
        bytes: zip_owned(vec![
            (
                "[Content_Types].xml".to_string(),
                content_types.as_bytes().to_vec(),
            ),
            (
                "_rels/.rels".to_string(),
                root_relationships("ppt/presentation.xml").into_bytes(),
            ),
            ("docProps/core.xml".to_string(), core_properties().into_bytes()),
            (
                "ppt/presentation.xml".to_string(),
                presentation.as_bytes().to_vec(),
            ),
            (
                "ppt/_rels/presentation.xml.rels".to_string(),
                presentation_rels.as_bytes().to_vec(),
            ),
            (
                "ppt/slideLayouts/slideLayout1.xml".to_string(),
                br#"<?xml version="1.0" encoding="UTF-8"?><p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree/></p:cSld></p:sldLayout>"#.to_vec(),
            ),
            ("ppt/slides/slide1.xml".to_string(), slide_xml.into_bytes()),
            (
                "ppt/slides/_rels/slide1.xml.rels".to_string(),
                br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"#.to_vec(),
            ),
        ])?,
    })
}

fn build_render_evidence(
    reconciliation: &T1CandidateArtifact,
    brief: &T1CandidateArtifact,
) -> Result<T1RenderEvidence, String> {
    let mut preview_bytes = BTreeMap::new();
    let mut artifacts = Vec::new();
    for (output, path) in [
        (reconciliation, RECONCILIATION_PREVIEW),
        (brief, BRIEF_PREVIEW),
    ] {
        let png = deterministic_preview_png()?;
        preview_bytes.insert(path.to_string(), png.clone());
        artifacts.push(T1RenderArtifactReceipt {
            output_relative_path: output.relative_path.clone(),
            output_sha256: sha256(&output.bytes),
            renderer_version: ACTUAL_RENDERER_VERSION.to_string(),
            rendered_unit_count: 1,
            preview_manifest_sha256: preview_manifest_hash(&[png.clone()]),
            previews: vec![T1PreviewReceipt {
                relative_path: path.to_string(),
                bytes: png.len() as u64,
                sha256: sha256(&png),
                width: 160,
                height: 120,
                edge_clipping: false,
            }],
        });
    }
    Ok(T1RenderEvidence {
        receipt: T1ActualRenderReceipt {
            version: ACTUAL_RENDER_RECEIPT_VERSION.to_string(),
            artifacts,
        },
        preview_bytes,
    })
}

fn read_render_evidence(root: &Path) -> Result<T1RenderEvidence, String> {
    let receipt: T1ActualRenderReceipt = read_json(root, ACTUAL_RENDER_RECEIPT_FILE)?;
    let mut preview_bytes = BTreeMap::new();
    for artifact in &receipt.artifacts {
        for preview in &artifact.previews {
            if preview_bytes
                .insert(
                    preview.relative_path.clone(),
                    read_file_bounded(&root.join(&preview.relative_path))?,
                )
                .is_some()
            {
                return Err("T1 render receipt contains a duplicate preview".to_string());
            }
        }
    }
    Ok(T1RenderEvidence {
        receipt,
        preview_bytes,
    })
}

fn read_fixture_set(root: &Path) -> Result<T1GeneratedFixtureSet, String> {
    let generated = generate_fixture_set()?;
    let mut files = Vec::with_capacity(generated.files.len());
    for fixture in generated.files {
        files.push(T1GeneratedFixture {
            fixture_id: fixture.fixture_id,
            relative_path: fixture.relative_path.clone(),
            media_type: fixture.media_type,
            bytes: read_file_bounded(&root.join(&fixture.relative_path))?,
        });
    }
    Ok(T1GeneratedFixtureSet {
        manifest: generated.manifest,
        files,
    })
}

fn read_candidate(root: &Path, relative_path: &str) -> Result<T1CandidateArtifact, String> {
    Ok(T1CandidateArtifact {
        relative_path: relative_path.to_string(),
        bytes: read_file_bounded(&root.join(relative_path))?,
    })
}

fn collect_artifact_hashes(
    workspace_root: &Path,
    fixture_root: &Path,
    output_root: &Path,
    task_spec: &BenchmarkTaskSpec,
) -> Result<Vec<BaselineArtifactHash>, String> {
    let mut artifacts = vec![artifact_hash(
        "run_binding",
        "workspace/run-binding.json",
        &workspace_root.join(RUN_BINDING_FILE),
    )?];
    for fixture in &task_spec.fixtures {
        artifacts.push(artifact_hash(
            "fixture",
            &format!("fixture/{}", fixture.relative_path),
            &fixture_root.join(&fixture.relative_path),
        )?);
    }
    for output in OUTPUT_FILES {
        artifacts.push(artifact_hash(
            output_kind(output),
            &format!("output/{output}"),
            &output_root.join(output),
        )?);
    }
    Ok(artifacts)
}

fn artifact_hash(
    kind: &str,
    relative_ref: &str,
    path: &Path,
) -> Result<BaselineArtifactHash, String> {
    let bytes = read_file_bounded(path)?;
    Ok(BaselineArtifactHash {
        kind: kind.to_string(),
        relative_ref: relative_ref.to_string(),
        bytes: bytes.len() as u64,
        sha256: sha256(&bytes),
    })
}

fn output_kind(path: &str) -> &'static str {
    match path {
        RECONCILIATION_OUTPUT_PATH | BRIEF_OUTPUT_PATH => "candidate_artifact",
        RECONCILIATION_PREVIEW | BRIEF_PREVIEW => "render_preview",
        SOURCE_MANIFEST_FILE | PROVENANCE_MANIFEST_FILE => "manifest_receipt",
        ACTUAL_RENDER_RECEIPT_FILE | RESULT_RECEIPT_FILE => "verification_receipt",
        RUN_RESULT_FILE => "benchmark_run_result",
        _ => "unexpected",
    }
}

fn write_json_new<T: Serialize>(root: &Path, relative_path: &str, value: &T) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("serialize T1 receipt: {error}"))?;
    bytes.push(b'\n');
    write_file_new(root, relative_path, &bytes)
}

fn write_file_new(root: &Path, relative_path: &str, bytes: &[u8]) -> Result<(), String> {
    super::super::validate_relative_path("T1 baseline output path", relative_path)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("resolve T1 output root: {error}"))?;
    let target = root.join(relative_path);
    let parent = target
        .parent()
        .ok_or_else(|| "T1 output has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create T1 output directory: {error}"))?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| format!("resolve T1 output directory: {error}"))?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err("T1 output escaped its explicit root".to_string());
    }
    let safe_target = canonical_parent.join(
        target
            .file_name()
            .ok_or_else(|| "T1 output has no file name".to_string())?,
    );
    if fs::symlink_metadata(&safe_target).is_ok() {
        return Err("T1 output target is not clean".to_string());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&safe_target)
        .map_err(|error| format!("create T1 output: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("write T1 output: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync T1 output: {error}"))
}

fn read_json<T: DeserializeOwned>(root: &Path, relative_path: &str) -> Result<T, String> {
    let bytes = read_file_bounded(&root.join(relative_path))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse T1 receipt: {error}"))
}

fn read_file_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("inspect T1 file: {error}"))?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_READ_BYTES
    {
        return Err("T1 file identity or size is invalid".to_string());
    }
    fs::read(path).map_err(|error| format!("read T1 file: {error}"))
}

fn provenance_fact_map(
    provenance: &T1ProvenanceManifest,
) -> Result<BTreeMap<&str, &super::verifiers::T1FactProvenance>, String> {
    let mut facts = BTreeMap::new();
    for fact in &provenance.facts {
        if facts.insert(fact.fact_id.as_str(), fact).is_some() {
            return Err("T1 provenance contains duplicate facts".to_string());
        }
    }
    Ok(facts)
}

fn fact_value<'a>(
    facts: &'a BTreeMap<&str, &super::verifiers::T1FactProvenance>,
    fact_id: &str,
) -> Result<&'a str, String> {
    facts
        .get(fact_id)
        .map(|fact| fact.value.as_str())
        .ok_or_else(|| format!("missing T1 fact {fact_id}"))
}

fn fact_number(
    facts: &BTreeMap<&str, &super::verifiers::T1FactProvenance>,
    fact_id: &str,
) -> Result<f64, String> {
    let value = fact_value(facts, fact_id)?
        .parse::<f64>()
        .map_err(|_| format!("T1 fact {fact_id} is not numeric"))?;
    if !value.is_finite() {
        return Err(format!("T1 fact {fact_id} is non-finite"));
    }
    Ok(value)
}

fn value_cell(reference: &str, value: &str, formula: Option<&str>) -> String {
    if let Some(formula) = formula {
        format!(
            "<c r=\"{reference}\"><f>{}</f><v>{}</v></c>",
            xml_escape(formula),
            xml_escape(value)
        )
    } else if value.parse::<f64>().is_ok() {
        format!("<c r=\"{reference}\"><v>{}</v></c>", xml_escape(value))
    } else {
        inline_cell(reference, value)
    }
}

fn inline_cell(reference: &str, value: &str) -> String {
    format!(
        "<c r=\"{reference}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
        xml_escape(value)
    )
}

fn slide_xml(text: &[String]) -> String {
    let shapes = text
        .iter()
        .enumerate()
        .map(|(index, value)| {
            format!(
                "<p:sp><p:nvSpPr><p:cNvPr id=\"{}\" name=\"Text {}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>",
                index + 2,
                index + 1,
                xml_escape(value)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>{shapes}</p:spTree></p:cSld></p:sld>"
    )
}

fn root_relationships(target: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"{target}\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>"
    )
}

fn core_properties() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dcterms:created xsi:type=\"dcterms:W3CDTF\">2026-07-16T00:00:00Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2026-07-16T00:00:00Z</dcterms:modified></cp:coreProperties>".to_string()
}

fn zip_owned(parts: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, String> {
    let refs = parts
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect::<Vec<_>>();
    write_deterministic_zip(&refs)
}

fn deterministic_preview_png() -> Result<Vec<u8>, String> {
    let mut image = GrayImage::from_pixel(160, 120, Luma([255]));
    for y in 30..70 {
        for x in 25..130 {
            image.put_pixel(x, y, Luma([80]));
        }
    }
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|error| format!("encode deterministic T1 preview: {error}"))?;
    Ok(bytes.into_inner())
}

fn format_integer_grouped(value: i64) -> String {
    let negative = value.is_negative();
    let digits = value.unsigned_abs().to_string();
    let mut output = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    if negative {
        format!("-{output}")
    } else {
        output
    }
}
