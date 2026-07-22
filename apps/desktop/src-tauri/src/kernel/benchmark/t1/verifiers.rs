use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use image::ImageFormat;
use quick_xml::{events::Event, Reader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::kernel::artifact_render::ACTUAL_RENDERER_VERSION;
use crate::kernel::artifacts::preview_manifest_hash;

use super::super::{
    BenchmarkEvidenceReceipt, BenchmarkRunResult, BenchmarkTaskSpec, BenchmarkVerifierResult,
    BenchmarkVerifierStatus,
};
use super::fixtures::{T1GeneratedFixture, T1GeneratedFixtureSet};
use super::{
    ACTUAL_RENDER_VERIFIER_ID, BRIEF_OUTPUT_PATH, FIXTURE_SET_ID, ONE_PAGE_PPTX_VERIFIER_ID,
    PROVENANCE_VERIFIER_ID, RECONCILIATION_OUTPUT_PATH, RECONCILIATION_XLSX_VERIFIER_ID,
    RESULT_RECEIPT_VERIFIER_ID, SOURCE_MANIFEST_VERIFIER_ID,
};

const SOURCE_MANIFEST_VERSION: &str = "t1.source-manifest/v1";
const PROVENANCE_VERSION: &str = "t1.provenance-manifest/v1";
const ACTUAL_RENDER_RECEIPT_VERSION: &str = "t1.actual-render-receipt/v1";
const RESULT_RECEIPT_VERSION: &str = "t1.result-receipt/v1";
const MAX_OPC_PARTS: usize = 128;
const MAX_OPC_BYTES: u64 = 8 * 1024 * 1024;
const NUMERIC_TOLERANCE: f64 = 0.000_001;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1SourceManifestEntry {
    pub fixture_id: String,
    pub relative_path: String,
    pub media_type: String,
    pub bytes: u64,
    pub sha256: String,
    pub generator_id: String,
    pub source_label: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1SourceManifest {
    pub version: String,
    pub fixture_set_id: String,
    pub entries: Vec<T1SourceManifestEntry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1SourceFactLocator {
    pub fixture_id: String,
    pub relative_path: String,
    pub source_sha256: String,
    pub locator: String,
    pub extracted_value: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1DerivedFactProvenance {
    pub operands: Vec<String>,
    pub algorithm_id: String,
    pub formula: String,
    pub recomputed_value: String,
    pub tolerance: f64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1FactProvenance {
    pub fact_id: String,
    pub value: String,
    pub source: Option<T1SourceFactLocator>,
    pub derivation: Option<T1DerivedFactProvenance>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1ProvenanceManifest {
    pub version: String,
    pub fixture_set_id: String,
    pub source_manifest_sha256: String,
    pub facts: Vec<T1FactProvenance>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct T1CandidateArtifact {
    pub relative_path: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1PreviewReceipt {
    pub relative_path: String,
    pub bytes: u64,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub edge_clipping: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1RenderArtifactReceipt {
    pub output_relative_path: String,
    pub output_sha256: String,
    pub renderer_version: String,
    pub rendered_unit_count: u32,
    pub preview_manifest_sha256: String,
    pub previews: Vec<T1PreviewReceipt>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1ActualRenderReceipt {
    pub version: String,
    pub artifacts: Vec<T1RenderArtifactReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct T1RenderEvidence {
    pub receipt: T1ActualRenderReceipt,
    pub preview_bytes: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1OutputReceipt {
    pub relative_path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1ResultReceipt {
    pub version: String,
    pub run_id: String,
    pub task_id: String,
    pub task_revision: u32,
    pub task_spec_fingerprint: String,
    pub outputs: Vec<T1OutputReceipt>,
    pub key_figures: BTreeMap<String, String>,
    pub anomalies: Vec<String>,
    pub evidence_refs: Vec<String>,
}

pub fn build_source_manifest(fixtures: &T1GeneratedFixtureSet) -> T1SourceManifest {
    let entries = fixtures
        .files
        .iter()
        .filter_map(|file| {
            fixtures
                .manifest
                .files
                .iter()
                .find(|entry| entry.fixture_id == file.fixture_id)
                .map(|entry| T1SourceManifestEntry {
                    fixture_id: file.fixture_id.clone(),
                    relative_path: file.relative_path.clone(),
                    media_type: file.media_type.clone(),
                    bytes: file.bytes.len() as u64,
                    sha256: sha256(&file.bytes),
                    generator_id: fixtures.manifest.generator_id.clone(),
                    source_label: entry.source_label.clone(),
                })
        })
        .collect();
    T1SourceManifest {
        version: SOURCE_MANIFEST_VERSION.to_string(),
        fixture_set_id: fixtures.manifest.fixture_set_id.clone(),
        entries,
    }
}

pub fn build_provenance_manifest(
    fixtures: &T1GeneratedFixtureSet,
) -> Result<T1ProvenanceManifest, String> {
    let source_manifest = build_source_manifest(fixtures);
    validate_source_manifest(fixtures, &source_manifest)?;
    Ok(T1ProvenanceManifest {
        version: PROVENANCE_VERSION.to_string(),
        fixture_set_id: fixtures.manifest.fixture_set_id.clone(),
        source_manifest_sha256: canonical_hash(&source_manifest)?,
        facts: expected_facts(fixtures)?,
    })
}

pub fn verify_source_manifest(
    fixtures: &T1GeneratedFixtureSet,
    manifest: &T1SourceManifest,
) -> BenchmarkVerifierResult {
    verifier_result(
        "source-manifest",
        SOURCE_MANIFEST_VERIFIER_ID,
        "source_manifest",
        "benchmark-evidence:t1-source-manifest",
        canonical_hash(manifest).ok(),
        validate_source_manifest(fixtures, manifest),
    )
}

pub fn verify_provenance(
    fixtures: &T1GeneratedFixtureSet,
    source_manifest: &T1SourceManifest,
    provenance: &T1ProvenanceManifest,
) -> BenchmarkVerifierResult {
    verifier_result(
        "fact-provenance",
        PROVENANCE_VERIFIER_ID,
        "fact_provenance",
        "benchmark-evidence:t1-fact-provenance",
        canonical_hash(provenance).ok(),
        validate_provenance(fixtures, source_manifest, provenance),
    )
}

pub fn verify_reconciliation_xlsx(
    fixtures: &T1GeneratedFixtureSet,
    source_manifest: &T1SourceManifest,
    provenance: &T1ProvenanceManifest,
    candidate: &T1CandidateArtifact,
) -> BenchmarkVerifierResult {
    let hash = sha256(&candidate.bytes);
    verifier_result(
        "reconciliation-xlsx",
        RECONCILIATION_XLSX_VERIFIER_ID,
        "reconciliation_xlsx",
        RECONCILIATION_OUTPUT_PATH,
        Some(hash),
        validate_reconciliation_xlsx(fixtures, source_manifest, provenance, candidate),
    )
}

pub fn verify_one_page_pptx(
    fixtures: &T1GeneratedFixtureSet,
    candidate: &T1CandidateArtifact,
) -> BenchmarkVerifierResult {
    let hash = sha256(&candidate.bytes);
    verifier_result(
        "one-page-brief",
        ONE_PAGE_PPTX_VERIFIER_ID,
        "one_page_pptx",
        BRIEF_OUTPUT_PATH,
        Some(hash),
        validate_one_page_pptx(fixtures, candidate),
    )
}

pub fn verify_actual_render(
    reconciliation: &T1CandidateArtifact,
    brief: &T1CandidateArtifact,
    evidence: &T1RenderEvidence,
) -> BenchmarkVerifierResult {
    verifier_result(
        "actual-render",
        ACTUAL_RENDER_VERIFIER_ID,
        "actual_render_receipt",
        "benchmark-evidence:t1-actual-render",
        canonical_hash(&evidence.receipt).ok(),
        validate_actual_render(reconciliation, brief, evidence),
    )
}

pub fn verify_result_receipt(
    task_spec: &BenchmarkTaskSpec,
    run_result: &BenchmarkRunResult,
    fixtures: &T1GeneratedFixtureSet,
    reconciliation: &T1CandidateArtifact,
    brief: &T1CandidateArtifact,
    receipt: &T1ResultReceipt,
) -> BenchmarkVerifierResult {
    verifier_result(
        "result-receipt",
        RESULT_RECEIPT_VERIFIER_ID,
        "result_receipt",
        "benchmark-evidence:t1-result-receipt",
        canonical_hash(receipt).ok(),
        validate_result_receipt(
            task_spec,
            run_result,
            fixtures,
            reconciliation,
            brief,
            receipt,
        ),
    )
}

fn verifier_result(
    done_when_id: &str,
    verifier_id: &str,
    evidence_kind: &str,
    evidence_ref: &str,
    evidence_sha256: Option<String>,
    validation: Result<(), String>,
) -> BenchmarkVerifierResult {
    let (status, summary) = match validation {
        Ok(()) => (
            BenchmarkVerifierStatus::Passed,
            "Deterministic T1 verification passed".to_string(),
        ),
        Err(_) => (
            BenchmarkVerifierStatus::Failed,
            "Deterministic T1 verification failed closed".to_string(),
        ),
    };
    BenchmarkVerifierResult {
        done_when_id: done_when_id.to_string(),
        verifier_id: verifier_id.to_string(),
        status,
        summary,
        evidence: vec![BenchmarkEvidenceReceipt {
            kind: evidence_kind.to_string(),
            relative_or_opaque_ref: evidence_ref.to_string(),
            sha256: evidence_sha256,
            summary: "Secret-safe deterministic T1 verifier receipt".to_string(),
        }],
    }
}

fn validate_source_manifest(
    fixtures: &T1GeneratedFixtureSet,
    manifest: &T1SourceManifest,
) -> Result<(), String> {
    if manifest.version != SOURCE_MANIFEST_VERSION
        || manifest.fixture_set_id != FIXTURE_SET_ID
        || fixtures.manifest.fixture_set_id != FIXTURE_SET_ID
        || manifest.entries.len() != 3
        || fixtures.files.len() != 3
        || fixtures.manifest.files.len() != 3
    {
        return Err("T1 source manifest identity is invalid".to_string());
    }
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for expected in &fixtures.manifest.files {
        let matches = manifest
            .entries
            .iter()
            .filter(|entry| entry.fixture_id == expected.fixture_id)
            .collect::<Vec<_>>();
        if matches.len() != 1 || !ids.insert(expected.fixture_id.as_str()) {
            return Err("T1 source manifest fixture binding is not unique".to_string());
        }
        let entry = matches[0];
        super::super::validate_relative_path("T1 source path", &entry.relative_path)?;
        if !paths.insert(entry.relative_path.as_str()) {
            return Err("T1 source manifest contains a duplicate path".to_string());
        }
        let files = fixtures
            .files
            .iter()
            .filter(|file| file.fixture_id == expected.fixture_id)
            .collect::<Vec<_>>();
        if files.len() != 1 {
            return Err("T1 source fixture is missing or duplicated".to_string());
        }
        let file = files[0];
        if entry.relative_path != expected.relative_path
            || entry.relative_path != file.relative_path
            || entry.media_type != expected.media_type
            || entry.media_type != file.media_type
            || entry.bytes != expected.bytes
            || entry.bytes != file.bytes.len() as u64
            || entry.sha256 != expected.sha256
            || entry.sha256 != sha256(&file.bytes)
            || entry.generator_id != fixtures.manifest.generator_id
            || entry.source_label != expected.source_label
        {
            return Err("T1 source manifest entry does not match fixture bytes".to_string());
        }
    }
    if manifest
        .entries
        .iter()
        .any(|entry| !ids.contains(entry.fixture_id.as_str()))
    {
        return Err("T1 source manifest contains an extra input".to_string());
    }
    Ok(())
}

fn validate_provenance(
    fixtures: &T1GeneratedFixtureSet,
    source_manifest: &T1SourceManifest,
    provenance: &T1ProvenanceManifest,
) -> Result<(), String> {
    validate_source_manifest(fixtures, source_manifest)?;
    if provenance.version != PROVENANCE_VERSION
        || provenance.fixture_set_id != FIXTURE_SET_ID
        || provenance.source_manifest_sha256 != canonical_hash(source_manifest)?
    {
        return Err("T1 provenance identity is invalid".to_string());
    }
    let expected = expected_facts(fixtures)?;
    if provenance.facts.len() != expected.len() {
        return Err("T1 provenance fact count is incomplete".to_string());
    }
    let mut ids = BTreeSet::new();
    for expected_fact in expected {
        let matches = provenance
            .facts
            .iter()
            .filter(|fact| fact.fact_id == expected_fact.fact_id)
            .collect::<Vec<_>>();
        if matches.len() != 1 || !ids.insert(expected_fact.fact_id.clone()) {
            return Err("T1 provenance fact binding is not unique".to_string());
        }
        let actual = matches[0];
        super::super::validate_slug("T1 fact_id", &actual.fact_id, 96)?;
        match (&expected_fact.source, &expected_fact.derivation) {
            (Some(expected_source), None) => {
                if actual.source.as_ref() != Some(expected_source)
                    || actual.derivation.is_some()
                    || actual.value != expected_fact.value
                    || expected_source.extracted_value != expected_fact.value
                {
                    return Err("T1 source fact provenance is invalid".to_string());
                }
            }
            (None, Some(expected_derivation)) => {
                let actual_derivation = actual
                    .derivation
                    .as_ref()
                    .ok_or_else(|| "T1 derived fact provenance is missing".to_string())?;
                if actual.source.is_some()
                    || actual_derivation.operands != expected_derivation.operands
                    || actual_derivation.algorithm_id != expected_derivation.algorithm_id
                    || actual_derivation.formula != expected_derivation.formula
                    || !actual_derivation.tolerance.is_finite()
                    || actual_derivation.tolerance < 0.0
                    || actual_derivation.tolerance > 0.01
                {
                    return Err("T1 derived fact provenance contract is invalid".to_string());
                }
                let expected_value = parse_number(&expected_derivation.recomputed_value)?;
                if !numbers_match(&actual.value, expected_value, actual_derivation.tolerance)?
                    || !numbers_match(
                        &actual_derivation.recomputed_value,
                        expected_value,
                        actual_derivation.tolerance,
                    )?
                {
                    return Err("T1 derived fact was not independently recomputed".to_string());
                }
            }
            _ => return Err("T1 provenance fact kind is invalid".to_string()),
        }
    }
    if provenance
        .facts
        .iter()
        .any(|fact| !ids.contains(&fact.fact_id))
    {
        return Err("T1 provenance contains an extra fact".to_string());
    }
    Ok(())
}

fn validate_reconciliation_xlsx(
    fixtures: &T1GeneratedFixtureSet,
    source_manifest: &T1SourceManifest,
    provenance: &T1ProvenanceManifest,
    candidate: &T1CandidateArtifact,
) -> Result<(), String> {
    if candidate.relative_path != RECONCILIATION_OUTPUT_PATH {
        return Err("T1 reconciliation path is invalid".to_string());
    }
    validate_provenance(fixtures, source_manifest, provenance)?;
    let package = OpcPackage::read(&candidate.bytes)?;
    package.validate_main(
        "xl/workbook.xml",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
    )?;
    for required in [
        "xl/workbook.xml",
        "xl/_rels/workbook.xml.rels",
        "xl/worksheets/sheet1.xml",
        "docProps/core.xml",
    ] {
        package.required(required)?;
    }
    package.validate_content_type(
        "xl/worksheets/sheet1.xml",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
    )?;
    package.validate_relationship(
        "xl/_rels/workbook.xml.rels",
        "/worksheet",
        "worksheets/sheet1.xml",
        1,
    )?;
    let sheet = package.text("xl/worksheets/sheet1.xml")?;
    for marker in ["#REF!", "#DIV/0!", "#VALUE!", "#N/A", "\u{fffd}"] {
        if sheet.contains(marker) {
            return Err("T1 reconciliation contains a formula or encoding error".to_string());
        }
    }
    let cells = parse_worksheet_cells(sheet)?;
    let facts = expected_facts(fixtures)?;
    let fact_map = facts
        .iter()
        .map(|fact| (fact.fact_id.as_str(), fact))
        .collect::<BTreeMap<_, _>>();
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
    for (row, fact_id) in source_rows {
        let fact = fact_map
            .get(fact_id)
            .ok_or_else(|| "T1 reconciliation source fact is unavailable".to_string())?;
        require_text(&cells, &format!("A{row}"), fact_id)?;
        require_value(&cells, &format!("B{row}"), &fact.value, NUMERIC_TOLERANCE)?;
        let locator = fact
            .source
            .as_ref()
            .ok_or_else(|| "T1 reconciliation source locator is unavailable".to_string())?;
        require_text(&cells, &format!("C{row}"), &locator.locator)?;
    }
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
    for (row, fact_id, formula) in derived_rows {
        let fact = fact_map
            .get(fact_id)
            .ok_or_else(|| "T1 reconciliation derived fact is unavailable".to_string())?;
        require_text(&cells, &format!("A{row}"), fact_id)?;
        let cell = cells
            .get(&format!("B{row}"))
            .ok_or_else(|| "T1 reconciliation formula cell is missing".to_string())?;
        if cell.formula.as_deref() != Some(formula) {
            return Err("T1 reconciliation formula changed".to_string());
        }
        require_value(&cells, &format!("B{row}"), &fact.value, NUMERIC_TOLERANCE)?;
        let derivation = fact
            .derivation
            .as_ref()
            .ok_or_else(|| "T1 reconciliation derivation is unavailable".to_string())?;
        require_text(
            &cells,
            &format!("C{row}"),
            &format!("derived:{}", derivation.algorithm_id),
        )?;
    }
    Ok(())
}

fn validate_one_page_pptx(
    fixtures: &T1GeneratedFixtureSet,
    candidate: &T1CandidateArtifact,
) -> Result<(), String> {
    if candidate.relative_path != BRIEF_OUTPUT_PATH {
        return Err("T1 brief path is invalid".to_string());
    }
    let package = OpcPackage::read(&candidate.bytes)?;
    package.validate_main(
        "ppt/presentation.xml",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
    )?;
    for required in [
        "ppt/presentation.xml",
        "ppt/_rels/presentation.xml.rels",
        "ppt/slides/slide1.xml",
        "ppt/slides/_rels/slide1.xml.rels",
        "ppt/slideLayouts/slideLayout1.xml",
        "docProps/core.xml",
    ] {
        package.required(required)?;
    }
    package.validate_content_type(
        "ppt/slides/slide1.xml",
        "application/vnd.openxmlformats-officedocument.presentationml.slide+xml",
    )?;
    package.validate_relationship(
        "ppt/_rels/presentation.xml.rels",
        "/slide",
        "slides/slide1.xml",
        1,
    )?;
    package.validate_relationship(
        "ppt/slides/_rels/slide1.xml.rels",
        "/slideLayout",
        "../slideLayouts/slideLayout1.xml",
        1,
    )?;
    let slide_parts = package
        .parts
        .keys()
        .filter(|name| {
            name.starts_with("ppt/slides/slide")
                && name.ends_with(".xml")
                && !name.contains("/_rels/")
        })
        .count();
    let presentation = package.text("ppt/presentation.xml")?;
    if slide_parts != 1 || presentation.matches("<p:sldId ").count() != 1 {
        return Err("T1 monthly brief must contain exactly one slide".to_string());
    }
    let slide_xml = package.text("ppt/slides/slide1.xml")?;
    if slide_xml.contains('\u{fffd}') {
        return Err("T1 monthly brief contains replacement characters".to_string());
    }
    let text = xml_visible_text(slide_xml)?;
    if text.trim().is_empty() {
        return Err("T1 monthly brief is blank".to_string());
    }
    let facts = fact_map(expected_facts(fixtures)?);
    let required_text = [
        fact_value(&facts, "period")?,
        format_integer_grouped(fact_number(&facts, "total_revenue_cny")? as i64),
        format_integer_grouped(fact_number(&facts, "budget_variance_cny")? as i64),
        format!(
            "{:.2}%",
            fact_number(&facts, "budget_variance_rate")? * 100.0
        ),
        format!("{:.2}%", fact_number(&facts, "occupancy_rate")? * 100.0),
        format!(
            "{:.2} percentage points",
            fact_number(&facts, "occupancy_variance_percentage_points")?
        ),
        "早餐排队".to_string(),
        "电梯停运".to_string(),
        "防火门逾期".to_string(),
    ];
    if required_text
        .iter()
        .any(|required| !text.contains(required))
    {
        return Err("T1 monthly brief is missing a required figure or anomaly".to_string());
    }
    let has_source_heading = text.contains("来源") || text.contains("Sources");
    let source_names = [
        "01-monthly-revenue.xlsx",
        "02-operations-notes.docx",
        "03-risk-summary.pdf",
    ];
    if !has_source_heading || source_names.iter().any(|name| !text.contains(name)) {
        return Err("T1 monthly brief source footnote is incomplete".to_string());
    }
    Ok(())
}

fn validate_actual_render(
    reconciliation: &T1CandidateArtifact,
    brief: &T1CandidateArtifact,
    evidence: &T1RenderEvidence,
) -> Result<(), String> {
    if evidence.receipt.version != ACTUAL_RENDER_RECEIPT_VERSION
        || evidence.receipt.artifacts.len() != 2
    {
        return Err("T1 actual-render receipt identity is invalid".to_string());
    }
    let outputs = [reconciliation, brief];
    let mut seen_outputs = BTreeSet::new();
    let mut seen_previews = BTreeSet::new();
    for output in outputs {
        let matches = evidence
            .receipt
            .artifacts
            .iter()
            .filter(|artifact| artifact.output_relative_path == output.relative_path)
            .collect::<Vec<_>>();
        if matches.len() != 1 || !seen_outputs.insert(output.relative_path.as_str()) {
            return Err("T1 actual-render output binding is not unique".to_string());
        }
        let artifact = matches[0];
        super::super::validate_relative_path(
            "T1 rendered output path",
            &artifact.output_relative_path,
        )?;
        if artifact.output_sha256 != sha256(&output.bytes)
            || artifact.renderer_version != ACTUAL_RENDERER_VERSION
            || artifact.rendered_unit_count != 1
            || artifact.previews.len() != 1
        {
            return Err("T1 actual-render artifact receipt is invalid".to_string());
        }
        let mut ordered_preview_bytes = Vec::new();
        for preview in &artifact.previews {
            super::super::validate_relative_path("T1 preview path", &preview.relative_path)?;
            if !preview.relative_path.ends_with(".png")
                || !seen_previews.insert(preview.relative_path.as_str())
                || preview.edge_clipping
            {
                return Err("T1 preview receipt path or clipping state is invalid".to_string());
            }
            let bytes = evidence
                .preview_bytes
                .get(&preview.relative_path)
                .ok_or_else(|| "T1 preview bytes are missing".to_string())?;
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                || preview.bytes != bytes.len() as u64
                || preview.sha256 != sha256(bytes)
            {
                return Err("T1 preview identity is invalid".to_string());
            }
            let image = image::load_from_memory_with_format(bytes, ImageFormat::Png)
                .map_err(|_| "T1 preview PNG is invalid".to_string())?
                .to_luma8();
            if preview.width != image.width()
                || preview.height != image.height()
                || image.width() < 100
                || image.height() < 100
                || image.pixels().filter(|pixel| pixel.0[0] < 250).count() <= 25
                || has_edge_clipping(&image)
            {
                return Err("T1 preview is blank, clipped, or dimensionally invalid".to_string());
            }
            ordered_preview_bytes.push(bytes.clone());
        }
        if artifact.preview_manifest_sha256 != preview_manifest_hash(&ordered_preview_bytes) {
            return Err("T1 preview manifest hash is invalid".to_string());
        }
    }
    if evidence.preview_bytes.len() != seen_previews.len()
        || evidence
            .receipt
            .artifacts
            .iter()
            .any(|artifact| !seen_outputs.contains(artifact.output_relative_path.as_str()))
    {
        return Err("T1 actual-render receipt contains extra evidence".to_string());
    }
    Ok(())
}

fn validate_result_receipt(
    task_spec: &BenchmarkTaskSpec,
    run_result: &BenchmarkRunResult,
    fixtures: &T1GeneratedFixtureSet,
    reconciliation: &T1CandidateArtifact,
    brief: &T1CandidateArtifact,
    receipt: &T1ResultReceipt,
) -> Result<(), String> {
    task_spec.validate()?;
    run_result.validate(task_spec)?;
    if receipt.version != RESULT_RECEIPT_VERSION
        || receipt.run_id != run_result.run_id
        || receipt.task_id != task_spec.task_id
        || receipt.task_revision != task_spec.task_revision
        || receipt.task_spec_fingerprint != task_spec.fingerprint()?
        || receipt.outputs.len() != 2
        || run_result.verifier_results.len() != task_spec.done_when.len()
    {
        return Err("T1 result receipt identity is invalid".to_string());
    }
    let mut output_paths = BTreeSet::new();
    for output in [reconciliation, brief] {
        let matches = receipt
            .outputs
            .iter()
            .filter(|entry| entry.relative_path == output.relative_path)
            .collect::<Vec<_>>();
        if matches.len() != 1 || !output_paths.insert(output.relative_path.as_str()) {
            return Err("T1 result receipt output binding is not unique".to_string());
        }
        super::super::validate_relative_path("T1 receipt output path", &matches[0].relative_path)?;
        if matches[0].sha256 != sha256(&output.bytes) {
            return Err("T1 result receipt output hash is invalid".to_string());
        }
    }
    if receipt
        .outputs
        .iter()
        .any(|entry| !output_paths.contains(entry.relative_path.as_str()))
    {
        return Err("T1 result receipt contains an extra output".to_string());
    }
    let facts = fact_map(expected_facts(fixtures)?);
    let expected_key_figures = BTreeMap::from([
        ("period".to_string(), fact_value(&facts, "period")?),
        (
            "total_revenue_cny".to_string(),
            fact_value(&facts, "total_revenue_cny")?,
        ),
        (
            "budget_variance_cny".to_string(),
            fact_value(&facts, "budget_variance_cny")?,
        ),
        (
            "budget_variance_rate".to_string(),
            fact_value(&facts, "budget_variance_rate")?,
        ),
        (
            "prior_variance_cny".to_string(),
            fact_value(&facts, "prior_variance_cny")?,
        ),
        (
            "prior_variance_rate".to_string(),
            fact_value(&facts, "prior_variance_rate")?,
        ),
        (
            "occupancy_rate".to_string(),
            fact_value(&facts, "occupancy_rate")?,
        ),
        (
            "occupancy_variance_percentage_points".to_string(),
            fact_value(&facts, "occupancy_variance_percentage_points")?,
        ),
    ]);
    if receipt.key_figures != expected_key_figures {
        return Err("T1 result receipt key figures are incomplete or incorrect".to_string());
    }
    let expected_anomalies = vec![
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
    ];
    if receipt.anomalies != expected_anomalies {
        return Err("T1 result receipt anomalies are incomplete or incorrect".to_string());
    }
    for value in receipt
        .key_figures
        .keys()
        .chain(receipt.key_figures.values())
        .chain(receipt.anomalies.iter())
        .chain(receipt.evidence_refs.iter())
    {
        super::super::validate_secret_safe_text("T1 result receipt text", value, 512)?;
    }
    let mut expected_refs = BTreeSet::new();
    for condition in &task_spec.done_when {
        let matches = run_result
            .verifier_results
            .iter()
            .filter(|result| result.done_when_id == condition.done_when_id)
            .collect::<Vec<_>>();
        if matches.len() != 1
            || matches[0].verifier_id != condition.verifier_id
            || matches[0].status != BenchmarkVerifierStatus::Passed
            || matches[0].evidence.is_empty()
        {
            return Err("T1 result receipt verifier binding is incomplete".to_string());
        }
        for evidence in &matches[0].evidence {
            super::super::validate_evidence_reference(&evidence.relative_or_opaque_ref)?;
            if !expected_refs.insert(evidence.relative_or_opaque_ref.clone()) {
                return Err("T1 result receipt evidence reference is duplicated".to_string());
            }
        }
    }
    let receipt_refs = receipt
        .evidence_refs
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if receipt_refs.len() != receipt.evidence_refs.len() || receipt_refs != expected_refs {
        return Err("T1 result receipt evidence references are incomplete".to_string());
    }
    Ok(())
}

fn expected_facts(fixtures: &T1GeneratedFixtureSet) -> Result<Vec<T1FactProvenance>, String> {
    let xlsx = fixtures
        .file("monthly-revenue-xlsx")
        .ok_or_else(|| "T1 revenue fixture is missing".to_string())?;
    let docx = fixtures
        .file("operations-notes-docx")
        .ok_or_else(|| "T1 operations fixture is missing".to_string())?;
    let pdf = fixtures
        .file("risk-summary-pdf")
        .ok_or_else(|| "T1 risk fixture is missing".to_string())?;
    let revenue = extract_xlsx_facts(xlsx)?;
    let operations = extract_docx_facts(docx)?;
    let risks = extract_pdf_facts(pdf)?;
    let period = revenue_value(&revenue, "period")?;
    if operations.get("period").map(|value| value.0.as_str()) != Some(period.as_str())
        || risks.get("period").map(|value| value.0.as_str()) != Some(period.as_str())
    {
        return Err("T1 fixture periods conflict".to_string());
    }
    let mut facts = Vec::new();
    let revenue_keys = [
        "period",
        "available_room_nights",
        "sold_room_nights",
        "reported_occupancy_rate",
        "rooms_revenue_cny",
        "reported_adr_cny",
        "food_beverage_revenue_cny",
        "other_revenue_cny",
        "reported_total_revenue_cny",
        "budget_total_revenue_cny",
        "prior_period_total_revenue_cny",
        "budget_occupancy_rate",
    ];
    for key in revenue_keys {
        let (value, locator) = revenue
            .get(key)
            .ok_or_else(|| format!("T1 revenue fact {key} is missing"))?;
        facts.push(source_fact(xlsx, key, value, locator));
    }
    let operations_keys = [
        "breakfast_queue_complaints",
        "overdue_invoice_corrections_over_48h",
        "group_leads_deferred_to_july",
    ];
    for key in operations_keys {
        let (value, locator) = operations
            .get(key)
            .ok_or_else(|| format!("T1 operations fact {key} is missing"))?;
        facts.push(source_fact(docx, key, value, locator));
    }
    let risk_keys = [
        "elevator_2_unplanned_outages",
        "overdue_fire_door_closing_checks",
        "temporary_food_staff_retraining_incomplete",
    ];
    for key in risk_keys {
        let (value, locator) = risks
            .get(key)
            .ok_or_else(|| format!("T1 risk fact {key} is missing"))?;
        facts.push(source_fact(pdf, key, value, locator));
    }

    let available = revenue_number(&revenue, "available_room_nights")?;
    let sold = revenue_number(&revenue, "sold_room_nights")?;
    let rooms = revenue_number(&revenue, "rooms_revenue_cny")?;
    let food = revenue_number(&revenue, "food_beverage_revenue_cny")?;
    let other = revenue_number(&revenue, "other_revenue_cny")?;
    let budget = revenue_number(&revenue, "budget_total_revenue_cny")?;
    let prior = revenue_number(&revenue, "prior_period_total_revenue_cny")?;
    let budget_occupancy = revenue_number(&revenue, "budget_occupancy_rate")?;
    if available <= 0.0 || sold <= 0.0 || budget == 0.0 || prior == 0.0 {
        return Err("T1 derived fact denominator is invalid".to_string());
    }
    let occupancy = sold / available;
    let adr = rooms / sold;
    let revpar = rooms / available;
    let total = rooms + food + other;
    let budget_variance = total - budget;
    let prior_variance = total - prior;
    let derived = [
        (
            "occupancy_rate",
            occupancy,
            vec!["sold_room_nights", "available_room_nights"],
            "t1.derive-occupancy/v1",
            "sold_room_nights / available_room_nights",
        ),
        (
            "adr_cny",
            adr,
            vec!["rooms_revenue_cny", "sold_room_nights"],
            "t1.derive-adr/v1",
            "rooms_revenue_cny / sold_room_nights",
        ),
        (
            "revpar_cny",
            revpar,
            vec!["rooms_revenue_cny", "available_room_nights"],
            "t1.derive-revpar/v1",
            "rooms_revenue_cny / available_room_nights",
        ),
        (
            "total_revenue_cny",
            total,
            vec![
                "rooms_revenue_cny",
                "food_beverage_revenue_cny",
                "other_revenue_cny",
            ],
            "t1.derive-total-revenue/v1",
            "rooms_revenue_cny + food_beverage_revenue_cny + other_revenue_cny",
        ),
        (
            "budget_variance_cny",
            budget_variance,
            vec!["total_revenue_cny", "budget_total_revenue_cny"],
            "t1.derive-budget-variance/v1",
            "total_revenue_cny - budget_total_revenue_cny",
        ),
        (
            "budget_variance_rate",
            budget_variance / budget,
            vec!["budget_variance_cny", "budget_total_revenue_cny"],
            "t1.derive-budget-variance-rate/v1",
            "budget_variance_cny / budget_total_revenue_cny",
        ),
        (
            "prior_variance_cny",
            prior_variance,
            vec!["total_revenue_cny", "prior_period_total_revenue_cny"],
            "t1.derive-prior-variance/v1",
            "total_revenue_cny - prior_period_total_revenue_cny",
        ),
        (
            "prior_variance_rate",
            prior_variance / prior,
            vec!["prior_variance_cny", "prior_period_total_revenue_cny"],
            "t1.derive-prior-variance-rate/v1",
            "prior_variance_cny / prior_period_total_revenue_cny",
        ),
        (
            "occupancy_variance_percentage_points",
            (occupancy - budget_occupancy) * 100.0,
            vec!["occupancy_rate", "budget_occupancy_rate"],
            "t1.derive-occupancy-variance/v1",
            "(occupancy_rate - budget_occupancy_rate) * 100",
        ),
    ];
    for (fact_id, value, operands, algorithm_id, formula) in derived {
        facts.push(derived_fact(
            fact_id,
            value,
            operands,
            algorithm_id,
            formula,
        ));
    }
    let map = fact_map(facts.clone());
    for (reported, computed) in [
        ("reported_occupancy_rate", "occupancy_rate"),
        ("reported_adr_cny", "adr_cny"),
        ("reported_total_revenue_cny", "total_revenue_cny"),
    ] {
        if (fact_number(&map, reported)? - fact_number(&map, computed)?).abs() > NUMERIC_TOLERANCE {
            return Err("T1 reported and independently recomputed facts conflict".to_string());
        }
    }
    Ok(facts)
}

fn source_fact(
    fixture: &T1GeneratedFixture,
    fact_id: &str,
    value: &str,
    locator: &str,
) -> T1FactProvenance {
    T1FactProvenance {
        fact_id: fact_id.to_string(),
        value: value.to_string(),
        source: Some(T1SourceFactLocator {
            fixture_id: fixture.fixture_id.clone(),
            relative_path: fixture.relative_path.clone(),
            source_sha256: sha256(&fixture.bytes),
            locator: locator.to_string(),
            extracted_value: value.to_string(),
        }),
        derivation: None,
    }
}

fn derived_fact(
    fact_id: &str,
    value: f64,
    operands: Vec<&str>,
    algorithm_id: &str,
    formula: &str,
) -> T1FactProvenance {
    let value = canonical_number(value);
    T1FactProvenance {
        fact_id: fact_id.to_string(),
        value: value.clone(),
        source: None,
        derivation: Some(T1DerivedFactProvenance {
            operands: operands.into_iter().map(str::to_string).collect(),
            algorithm_id: algorithm_id.to_string(),
            formula: formula.to_string(),
            recomputed_value: value,
            tolerance: NUMERIC_TOLERANCE,
        }),
    }
}

fn extract_xlsx_facts(
    fixture: &T1GeneratedFixture,
) -> Result<BTreeMap<String, (String, String)>, String> {
    let package = OpcPackage::read(&fixture.bytes)?;
    package.validate_main(
        "xl/workbook.xml",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
    )?;
    package.validate_content_type(
        "xl/worksheets/sheet1.xml",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml",
    )?;
    package.validate_relationship(
        "xl/_rels/workbook.xml.rels",
        "/worksheet",
        "worksheets/sheet1.xml",
        1,
    )?;
    let cells = parse_worksheet_cells(package.text("xl/worksheets/sheet1.xml")?)?;
    let mut facts = BTreeMap::new();
    for row in 1..=13 {
        let key = cell_text(&cells, &format!("A{row}"))?;
        let value = cell_text(&cells, &format!("B{row}"))?;
        if facts
            .insert(
                key,
                (value, format!("xlsx:xl/worksheets/sheet1.xml#B{row}")),
            )
            .is_some()
        {
            return Err("T1 XLSX source contains a duplicate fact".to_string());
        }
    }
    if !facts
        .get("synthetic_notice")
        .is_some_and(|value| value.0.contains("synthetic benchmark data only"))
    {
        return Err("T1 XLSX synthetic notice is missing".to_string());
    }
    Ok(facts)
}

fn extract_docx_facts(
    fixture: &T1GeneratedFixture,
) -> Result<BTreeMap<String, (String, String)>, String> {
    let package = OpcPackage::read(&fixture.bytes)?;
    package.validate_main(
        "word/document.xml",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
    )?;
    let paragraphs = xml_paragraphs(package.text("word/document.xml")?, b"p")?;
    let mut facts = BTreeMap::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        if let Some((key, value)) = paragraph.split_once('=') {
            if facts
                .insert(
                    key.to_string(),
                    (
                        value.to_string(),
                        format!("docx:word/document.xml#p{}", index + 1),
                    ),
                )
                .is_some()
            {
                return Err("T1 DOCX source contains a duplicate fact".to_string());
            }
        }
    }
    if !facts
        .get("synthetic_notice")
        .is_some_and(|value| value.0.contains("synthetic benchmark data only"))
    {
        return Err("T1 DOCX synthetic notice is missing".to_string());
    }
    Ok(facts)
}

fn extract_pdf_facts(
    fixture: &T1GeneratedFixture,
) -> Result<BTreeMap<String, (String, String)>, String> {
    let text = std::str::from_utf8(&fixture.bytes)
        .map_err(|_| "T1 PDF source is not deterministic UTF-8".to_string())?;
    if !text.starts_with("%PDF-1.4") || !text.ends_with("%%EOF\n") || !text.contains("xref") {
        return Err("T1 PDF source structure is invalid".to_string());
    }
    let keys = [
        "period",
        "synthetic_notice",
        "elevator_2_unplanned_outages",
        "overdue_fire_door_closing_checks",
        "temporary_food_staff_retraining_incomplete",
    ];
    let mut facts = BTreeMap::new();
    for key in keys {
        let prefix = format!("({key}=");
        let start = text
            .find(&prefix)
            .ok_or_else(|| format!("T1 PDF source fact {key} is missing"))?
            + prefix.len();
        let end = text[start..]
            .find(") Tj")
            .ok_or_else(|| "T1 PDF text token is invalid".to_string())?
            + start;
        facts.insert(
            key.to_string(),
            (
                text[start..end].to_string(),
                format!("pdf:text-token:{key}"),
            ),
        );
    }
    if !facts
        .get("synthetic_notice")
        .is_some_and(|value| value.0.contains("synthetic benchmark data only"))
    {
        return Err("T1 PDF synthetic notice is missing".to_string());
    }
    Ok(facts)
}

#[derive(Clone, Debug, Default)]
struct WorksheetCell {
    value: String,
    formula: Option<String>,
    inline_text: String,
}

fn parse_worksheet_cells(xml: &str) -> Result<BTreeMap<String, WorksheetCell>, String> {
    validate_xml(xml.as_bytes())?;
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut cells = BTreeMap::new();
    let mut current_ref = None;
    let mut current = WorksheetCell::default();
    let mut capture = None::<u8>;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.local_name().as_ref() == b"c" => {
                current_ref = Some(required_attribute(&event, b"r")?);
                current = WorksheetCell::default();
            }
            Ok(Event::Start(event)) if event.local_name().as_ref() == b"v" => capture = Some(b'v'),
            Ok(Event::Start(event)) if event.local_name().as_ref() == b"f" => capture = Some(b'f'),
            Ok(Event::Start(event)) if event.local_name().as_ref() == b"t" => capture = Some(b't'),
            Ok(Event::Text(text)) => {
                let decoded = decode_text(&text)?;
                match capture {
                    Some(b'v') => current.value.push_str(&decoded),
                    Some(b'f') => current
                        .formula
                        .get_or_insert_with(String::new)
                        .push_str(&decoded),
                    Some(b't') => current.inline_text.push_str(&decoded),
                    _ => {}
                }
            }
            Ok(Event::End(event)) if matches!(event.local_name().as_ref(), b"v" | b"f" | b"t") => {
                capture = None;
            }
            Ok(Event::End(event)) if event.local_name().as_ref() == b"c" => {
                let reference = current_ref
                    .take()
                    .ok_or_else(|| "T1 worksheet cell reference is missing".to_string())?;
                if cells.insert(reference, current.clone()).is_some() {
                    return Err("T1 worksheet contains a duplicate cell".to_string());
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err("T1 worksheet XML is invalid".to_string()),
        }
    }
    Ok(cells)
}

fn xml_paragraphs(xml: &str, paragraph_name: &[u8]) -> Result<Vec<String>, String> {
    validate_xml(xml.as_bytes())?;
    let mut reader = Reader::from_str(xml);
    let mut paragraphs = Vec::new();
    let mut in_paragraph = false;
    let mut current = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if event.local_name().as_ref() == paragraph_name => {
                in_paragraph = true;
                current.clear();
            }
            Ok(Event::Text(text)) if in_paragraph => current.push_str(&decode_text(&text)?),
            Ok(Event::End(event)) if event.local_name().as_ref() == paragraph_name => {
                in_paragraph = false;
                paragraphs.push(current.clone());
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err("T1 document XML is invalid".to_string()),
        }
    }
    Ok(paragraphs)
}

fn xml_visible_text(xml: &str) -> Result<String, String> {
    validate_xml(xml.as_bytes())?;
    let mut reader = Reader::from_str(xml);
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(value)) => {
                let value = decode_text(&value)?;
                if !value.trim().is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&value);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err("T1 visible XML text is invalid".to_string()),
        }
    }
    Ok(text)
}

fn decode_text(text: &quick_xml::events::BytesText<'_>) -> Result<String, String> {
    let decoded = text
        .decode()
        .map_err(|_| "T1 XML text encoding is invalid".to_string())?;
    quick_xml::escape::unescape(&decoded)
        .map(|value| value.into_owned())
        .map_err(|_| "T1 XML text escaping is invalid".to_string())
}

fn required_attribute(
    event: &quick_xml::events::BytesStart<'_>,
    key: &[u8],
) -> Result<String, String> {
    for attribute in event.attributes().with_checks(true) {
        let attribute = attribute.map_err(|_| "T1 XML attribute is invalid".to_string())?;
        if attribute.key.local_name().as_ref() == key {
            return std::str::from_utf8(attribute.value.as_ref())
                .map(str::to_string)
                .map_err(|_| "T1 XML attribute encoding is invalid".to_string());
        }
    }
    Err("T1 XML required attribute is missing".to_string())
}

struct OpcPackage {
    parts: BTreeMap<String, Vec<u8>>,
}

impl OpcPackage {
    fn read(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() || bytes.len() as u64 > MAX_OPC_BYTES {
            return Err("T1 OPC package size is invalid".to_string());
        }
        let mut archive = ZipArchive::new(Cursor::new(bytes))
            .map_err(|_| "T1 OPC package cannot be opened".to_string())?;
        if archive.is_empty() || archive.len() > MAX_OPC_PARTS {
            return Err("T1 OPC part count is invalid".to_string());
        }
        let mut parts = BTreeMap::new();
        let mut expanded = 0_u64;
        for index in 0..archive.len() {
            let mut file = archive
                .by_index(index)
                .map_err(|_| "T1 OPC part cannot be opened".to_string())?;
            if file.is_dir() {
                return Err("T1 OPC package contains a directory entry".to_string());
            }
            let name = file.name().to_string();
            super::super::validate_relative_path("T1 OPC part path", &name)?;
            expanded = expanded
                .checked_add(file.size())
                .ok_or_else(|| "T1 OPC expanded size overflow".to_string())?;
            if expanded > MAX_OPC_BYTES {
                return Err("T1 OPC expanded size is invalid".to_string());
            }
            let mut part = Vec::new();
            file.read_to_end(&mut part)
                .map_err(|_| "T1 OPC part cannot be read".to_string())?;
            if (name.ends_with(".xml") || name.ends_with(".rels")) && validate_xml(&part).is_err() {
                return Err("T1 OPC XML part is invalid".to_string());
            }
            if parts.insert(name, part).is_some() {
                return Err("T1 OPC package contains a duplicate part".to_string());
            }
        }
        let package = Self { parts };
        package.required("[Content_Types].xml")?;
        package.required("_rels/.rels")?;
        package.reject_external_relationships()?;
        Ok(package)
    }

    fn required(&self, name: &str) -> Result<&[u8], String> {
        self.parts
            .get(name)
            .map(Vec::as_slice)
            .ok_or_else(|| "T1 OPC package is missing a required part".to_string())
    }

    fn text(&self, name: &str) -> Result<&str, String> {
        std::str::from_utf8(self.required(name)?)
            .map_err(|_| "T1 OPC text part is invalid UTF-8".to_string())
    }

    fn validate_main(&self, main_part: &str, content_type: &str) -> Result<(), String> {
        self.validate_content_type(main_part, content_type)?;
        self.validate_relationship("_rels/.rels", "/officeDocument", main_part, 1)?;
        self.required(main_part)?;
        Ok(())
    }

    fn validate_content_type(&self, part: &str, content_type: &str) -> Result<(), String> {
        let xml = self.text("[Content_Types].xml")?;
        let mut reader = Reader::from_str(xml);
        let mut matches = 0;
        loop {
            match reader.read_event() {
                Ok(Event::Start(event)) | Ok(Event::Empty(event))
                    if event
                        .local_name()
                        .as_ref()
                        .eq_ignore_ascii_case(b"override") =>
                {
                    let attributes = xml_attributes(&event)?;
                    if attributes.get("partname").map(String::as_str)
                        == Some(format!("/{part}").as_str())
                        && attributes.get("contenttype").map(String::as_str) == Some(content_type)
                    {
                        matches += 1;
                    }
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => return Err("T1 OPC content types are invalid".to_string()),
            }
        }
        if matches != 1 {
            return Err("T1 OPC content type binding is invalid".to_string());
        }
        Ok(())
    }

    fn validate_relationship(
        &self,
        relationship_part: &str,
        relationship_type_suffix: &str,
        target: &str,
        expected_count: usize,
    ) -> Result<(), String> {
        let xml = self.text(relationship_part)?;
        let mut reader = Reader::from_str(xml);
        let mut matches = 0;
        loop {
            match reader.read_event() {
                Ok(Event::Start(event)) | Ok(Event::Empty(event))
                    if event
                        .local_name()
                        .as_ref()
                        .eq_ignore_ascii_case(b"relationship") =>
                {
                    let attributes = xml_attributes(&event)?;
                    let relationship_type = attributes
                        .get("type")
                        .ok_or_else(|| "T1 OPC relationship type is missing".to_string())?;
                    let actual_target = attributes
                        .get("target")
                        .ok_or_else(|| "T1 OPC relationship target is missing".to_string())?;
                    if attributes
                        .get("targetmode")
                        .is_some_and(|mode| mode.eq_ignore_ascii_case("external"))
                    {
                        return Err("T1 OPC external relationship is blocked".to_string());
                    }
                    if relationship_type.ends_with(relationship_type_suffix)
                        && actual_target == target
                    {
                        matches += 1;
                    }
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => return Err("T1 OPC relationships are invalid".to_string()),
            }
        }
        if matches != expected_count {
            return Err("T1 OPC relationship binding is invalid".to_string());
        }
        Ok(())
    }

    fn reject_external_relationships(&self) -> Result<(), String> {
        for (name, bytes) in &self.parts {
            if !name.ends_with(".rels") {
                continue;
            }
            let lower = String::from_utf8_lossy(bytes).to_ascii_lowercase();
            if lower.contains("targetmode=\"external\"")
                || lower.contains("target=\"file:")
                || lower.contains("target=\"http:")
                || lower.contains("target=\"https:")
                || lower.contains("target=\"\\\\")
            {
                return Err("T1 OPC external relationship is blocked".to_string());
            }
        }
        Ok(())
    }
}

fn xml_attributes(
    event: &quick_xml::events::BytesStart<'_>,
) -> Result<BTreeMap<String, String>, String> {
    let mut attributes = BTreeMap::new();
    for attribute in event.attributes().with_checks(true) {
        let attribute = attribute.map_err(|_| "T1 XML attribute is invalid".to_string())?;
        if attribute.value.contains(&b'&') {
            return Err("T1 XML attribute escaping is unsupported".to_string());
        }
        let key = std::str::from_utf8(attribute.key.local_name().as_ref())
            .map_err(|_| "T1 XML attribute name is invalid".to_string())?
            .to_ascii_lowercase();
        let value = std::str::from_utf8(attribute.value.as_ref())
            .map_err(|_| "T1 XML attribute value is invalid".to_string())?
            .to_string();
        if attributes.insert(key, value).is_some() {
            return Err("T1 XML attribute is duplicated".to_string());
        }
    }
    Ok(attributes)
}

fn validate_xml(bytes: &[u8]) -> Result<(), String> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    loop {
        match reader.read_event() {
            Ok(Event::DocType(_)) | Ok(Event::PI(_)) => {
                return Err("T1 XML declaration is unsafe".to_string())
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err("T1 XML is malformed".to_string()),
        }
    }
    Ok(())
}

fn cell_text(cells: &BTreeMap<String, WorksheetCell>, reference: &str) -> Result<String, String> {
    let cell = cells
        .get(reference)
        .ok_or_else(|| "T1 worksheet required cell is missing".to_string())?;
    if !cell.inline_text.is_empty() {
        Ok(cell.inline_text.clone())
    } else if !cell.value.is_empty() {
        Ok(cell.value.clone())
    } else {
        Err("T1 worksheet required cell is empty".to_string())
    }
}

fn require_text(
    cells: &BTreeMap<String, WorksheetCell>,
    reference: &str,
    expected: &str,
) -> Result<(), String> {
    if cell_text(cells, reference)? != expected {
        return Err("T1 worksheet text cell is incorrect".to_string());
    }
    Ok(())
}

fn require_value(
    cells: &BTreeMap<String, WorksheetCell>,
    reference: &str,
    expected: &str,
    tolerance: f64,
) -> Result<(), String> {
    let actual = cell_text(cells, reference)?;
    match (actual.parse::<f64>(), expected.parse::<f64>()) {
        (Ok(actual), Ok(expected)) if (actual - expected).abs() <= tolerance => Ok(()),
        (Err(_), Err(_)) if actual == expected => Ok(()),
        _ => Err("T1 worksheet value is incorrect".to_string()),
    }
}

fn has_edge_clipping(image: &image::GrayImage) -> bool {
    let margin_x = (image.width() / 100).max(1);
    let margin_y = (image.height() / 100).max(1);
    let edge_dark_pixels = image
        .enumerate_pixels()
        .filter(|(x, y, pixel)| {
            pixel.0[0] < 220
                && (*x < margin_x
                    || *x >= image.width() - margin_x
                    || *y < margin_y
                    || *y >= image.height() - margin_y)
        })
        .count();
    let edge_area = usize::try_from(2 * margin_x * image.height() + 2 * margin_y * image.width())
        .unwrap_or(usize::MAX);
    edge_dark_pixels > 25 && edge_dark_pixels.saturating_mul(20) > edge_area
}

fn revenue_value(facts: &BTreeMap<String, (String, String)>, key: &str) -> Result<String, String> {
    facts
        .get(key)
        .map(|value| value.0.clone())
        .ok_or_else(|| format!("T1 source value {key} is missing"))
}

fn revenue_number(facts: &BTreeMap<String, (String, String)>, key: &str) -> Result<f64, String> {
    parse_number(&revenue_value(facts, key)?)
}

fn fact_map(facts: Vec<T1FactProvenance>) -> BTreeMap<String, T1FactProvenance> {
    facts
        .into_iter()
        .map(|fact| (fact.fact_id.clone(), fact))
        .collect()
}

fn fact_value(facts: &BTreeMap<String, T1FactProvenance>, fact_id: &str) -> Result<String, String> {
    facts
        .get(fact_id)
        .map(|fact| fact.value.clone())
        .ok_or_else(|| format!("T1 fact {fact_id} is missing"))
}

fn fact_number(facts: &BTreeMap<String, T1FactProvenance>, fact_id: &str) -> Result<f64, String> {
    parse_number(&fact_value(facts, fact_id)?)
}

fn parse_number(value: &str) -> Result<f64, String> {
    let value = value
        .parse::<f64>()
        .map_err(|_| "T1 numeric fact is invalid".to_string())?;
    if !value.is_finite() {
        return Err("T1 numeric fact is not finite".to_string());
    }
    Ok(value)
}

fn numbers_match(value: &str, expected: f64, tolerance: f64) -> Result<bool, String> {
    Ok((parse_number(value)? - expected).abs() <= tolerance)
}

fn canonical_number(value: f64) -> String {
    if value.fract().abs() < 0.000_000_1 {
        format!("{value:.0}")
    } else {
        format!("{value:.6}")
    }
}

fn format_integer_grouped(value: i64) -> String {
    let negative = value < 0;
    let digits = value.unsigned_abs().to_string();
    let mut grouped = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(character);
    }
    if negative {
        format!("-{grouped}")
    } else {
        grouped
    }
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn canonical_hash<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_vec(value)
        .map(|bytes| sha256(&bytes))
        .map_err(|error| format!("T1 receipt could not be serialized: {error}"))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use image::{DynamicImage, GrayImage, Luma};

    use crate::kernel::benchmark::{
        classify_benchmark_run, BenchmarkDeepSeekUsage, BenchmarkExternalEffectState,
        BenchmarkInteractions, BenchmarkOutcomeClass, BenchmarkRunResult, BenchmarkSubject,
        BenchmarkTerminalState, BenchmarkVerifierStatus, BENCHMARK_RUN_RESULT_VERSION,
    };
    use crate::kernel::models::AccessMode;

    use super::super::fixtures::{generate_fixture_set, write_deterministic_zip, xml_escape};
    use super::super::task_spec;
    use super::*;

    #[derive(Default)]
    struct XlsxMutation {
        value_overrides: BTreeMap<String, String>,
        formula_overrides: BTreeMap<String, String>,
        provenance_overrides: BTreeMap<String, String>,
        omit_sheet: bool,
        malformed_sheet: bool,
    }

    #[derive(Default)]
    struct PptxMutation {
        slides: usize,
        omit_footnote: bool,
        wrong_total: bool,
        blank: bool,
        replacement_character: bool,
    }

    fn valid_inputs() -> (
        T1GeneratedFixtureSet,
        T1SourceManifest,
        T1ProvenanceManifest,
        T1CandidateArtifact,
        T1CandidateArtifact,
    ) {
        let fixtures = generate_fixture_set().unwrap();
        let source = build_source_manifest(&fixtures);
        let provenance = build_provenance_manifest(&fixtures).unwrap();
        let reconciliation = build_test_reconciliation(&fixtures, &XlsxMutation::default());
        let brief = build_test_brief(&fixtures, &PptxMutation::default());
        (fixtures, source, provenance, reconciliation, brief)
    }

    #[test]
    fn task_contract_and_all_six_positive_verifiers_pass() {
        let (fixtures, source, provenance, reconciliation, brief) = valid_inputs();
        let spec = task_spec().unwrap();
        assert_eq!(spec.fixtures.len(), 3);
        assert_eq!(spec.done_when.len(), 6);

        let source_result = verify_source_manifest(&fixtures, &source);
        let provenance_result = verify_provenance(&fixtures, &source, &provenance);
        let xlsx_result =
            verify_reconciliation_xlsx(&fixtures, &source, &provenance, &reconciliation);
        let pptx_result = verify_one_page_pptx(&fixtures, &brief);
        let render = valid_render_evidence(&reconciliation, &brief);
        let render_result = verify_actual_render(&reconciliation, &brief, &render);
        for result in [
            source_result,
            provenance_result,
            xlsx_result,
            pptx_result,
            render_result,
        ] {
            assert_eq!(result.status, BenchmarkVerifierStatus::Passed);
            assert!(!result.evidence.is_empty());
        }

        let run = valid_run_result(&spec);
        let receipt = valid_result_receipt(&spec, &run, &fixtures, &reconciliation, &brief);
        let receipt_result =
            verify_result_receipt(&spec, &run, &fixtures, &reconciliation, &brief, &receipt);
        assert_eq!(receipt_result.status, BenchmarkVerifierStatus::Passed);
    }

    #[test]
    fn source_manifest_rejects_missing_extra_and_tampered_inputs() {
        let fixtures = generate_fixture_set().unwrap();
        let source = build_source_manifest(&fixtures);
        assert_eq!(
            verify_source_manifest(&fixtures, &source).status,
            BenchmarkVerifierStatus::Passed
        );

        let mut missing = fixtures.clone();
        missing.files.pop();
        assert_eq!(
            verify_source_manifest(&missing, &source).status,
            BenchmarkVerifierStatus::Failed
        );

        let mut extra = fixtures.clone();
        extra.files.push(T1GeneratedFixture {
            fixture_id: "fake-input".to_string(),
            relative_path: "inputs/fake.xlsx".to_string(),
            media_type: "application/octet-stream".to_string(),
            bytes: vec![1, 2, 3],
        });
        assert_eq!(
            verify_source_manifest(&extra, &source).status,
            BenchmarkVerifierStatus::Failed
        );

        let mut tampered = fixtures;
        tampered.files[0].bytes.push(0);
        assert_eq!(
            verify_source_manifest(&tampered, &source).status,
            BenchmarkVerifierStatus::Failed
        );

        let fixtures = generate_fixture_set().unwrap();
        let mut directory = source;
        directory.entries[0].relative_path = "inputs/".to_string();
        assert_eq!(
            verify_source_manifest(&fixtures, &directory).status,
            BenchmarkVerifierStatus::Failed
        );
    }

    #[test]
    fn provenance_rejects_wrong_locator_hash_derivation_and_tolerance() {
        let (fixtures, source, provenance, _, _) = valid_inputs();
        let mutations = [
            Box::new(|manifest: &mut T1ProvenanceManifest| {
                manifest.facts[0].source.as_mut().unwrap().locator =
                    "xlsx:xl/worksheets/sheet1.xml#Z99".to_string();
            }) as Box<dyn Fn(&mut T1ProvenanceManifest)>,
            Box::new(|manifest: &mut T1ProvenanceManifest| {
                manifest.facts[0].source.as_mut().unwrap().source_sha256 = "a".repeat(64);
            }),
            Box::new(|manifest: &mut T1ProvenanceManifest| {
                let derived = manifest
                    .facts
                    .iter_mut()
                    .find(|fact| fact.fact_id == "revpar_cny")
                    .unwrap();
                derived.derivation.as_mut().unwrap().recomputed_value = "999".to_string();
            }),
            Box::new(|manifest: &mut T1ProvenanceManifest| {
                let derived = manifest
                    .facts
                    .iter_mut()
                    .find(|fact| fact.fact_id == "occupancy_rate")
                    .unwrap();
                derived.derivation.as_mut().unwrap().tolerance = f64::INFINITY;
            }),
        ];
        for mutate in mutations {
            let mut changed = provenance.clone();
            mutate(&mut changed);
            assert_eq!(
                verify_provenance(&fixtures, &source, &changed).status,
                BenchmarkVerifierStatus::Failed
            );
        }
    }

    #[test]
    fn reconciliation_rejects_wrong_constants_formula_errors_and_tampering() {
        let (fixtures, source, provenance, _, _) = valid_inputs();
        let mut mutations = Vec::new();

        let mut wrong_constant = XlsxMutation::default();
        wrong_constant
            .value_overrides
            .insert("B3".to_string(), "2999".to_string());
        mutations.push(wrong_constant);

        let mut formula_error = XlsxMutation::default();
        formula_error
            .formula_overrides
            .insert("B20".to_string(), "#REF!".to_string());
        mutations.push(formula_error);

        let mut wrong_total = XlsxMutation::default();
        wrong_total
            .value_overrides
            .insert("B23".to_string(), "1702401".to_string());
        mutations.push(wrong_total);

        let mut wrong_variance = XlsxMutation::default();
        wrong_variance
            .formula_overrides
            .insert("B24".to_string(), "B23-B12".to_string());
        mutations.push(wrong_variance);

        let mut wrong_provenance = XlsxMutation::default();
        wrong_provenance.provenance_overrides.insert(
            "C3".to_string(),
            "xlsx:xl/worksheets/sheet1.xml#Z99".to_string(),
        );
        mutations.push(wrong_provenance);

        let mut static_table = XlsxMutation::default();
        static_table
            .formula_overrides
            .insert("B20".to_string(), String::new());
        mutations.push(static_table);

        let mut missing_part = XlsxMutation::default();
        missing_part.omit_sheet = true;
        mutations.push(missing_part);

        let mut malformed = XlsxMutation::default();
        malformed.malformed_sheet = true;
        mutations.push(malformed);

        for mutation in mutations {
            let candidate = build_test_reconciliation(&fixtures, &mutation);
            assert_eq!(
                verify_reconciliation_xlsx(&fixtures, &source, &provenance, &candidate).status,
                BenchmarkVerifierStatus::Failed
            );
        }
    }

    #[test]
    fn one_page_brief_rejects_two_pages_missing_footnote_wrong_number_blank_and_replacement() {
        let fixtures = generate_fixture_set().unwrap();
        let mutations = [
            PptxMutation {
                slides: 2,
                ..Default::default()
            },
            PptxMutation {
                omit_footnote: true,
                ..Default::default()
            },
            PptxMutation {
                wrong_total: true,
                ..Default::default()
            },
            PptxMutation {
                blank: true,
                ..Default::default()
            },
            PptxMutation {
                replacement_character: true,
                ..Default::default()
            },
        ];
        for mutation in mutations {
            let candidate = build_test_brief(&fixtures, &mutation);
            assert_eq!(
                verify_one_page_pptx(&fixtures, &candidate).status,
                BenchmarkVerifierStatus::Failed
            );
        }
    }

    #[test]
    fn actual_render_rejects_output_preview_and_manifest_hash_drift() {
        let (_, _, _, reconciliation, brief) = valid_inputs();
        let valid = valid_render_evidence(&reconciliation, &brief);
        assert_eq!(
            verify_actual_render(&reconciliation, &brief, &valid).status,
            BenchmarkVerifierStatus::Passed
        );

        let mut output_hash = valid.clone();
        output_hash.receipt.artifacts[0].output_sha256 = "a".repeat(64);
        assert_eq!(
            verify_actual_render(&reconciliation, &brief, &output_hash).status,
            BenchmarkVerifierStatus::Failed
        );

        let mut preview_hash = valid.clone();
        preview_hash.receipt.artifacts[0].previews[0].sha256 = "b".repeat(64);
        assert_eq!(
            verify_actual_render(&reconciliation, &brief, &preview_hash).status,
            BenchmarkVerifierStatus::Failed
        );

        let mut manifest_hash = valid;
        manifest_hash.receipt.artifacts[1].preview_manifest_sha256 = "c".repeat(64);
        assert_eq!(
            verify_actual_render(&reconciliation, &brief, &manifest_hash).status,
            BenchmarkVerifierStatus::Failed
        );

        let mut clipped = valid_render_evidence(&reconciliation, &brief);
        clipped.receipt.artifacts[0].previews[0].edge_clipping = true;
        assert_eq!(
            verify_actual_render(&reconciliation, &brief, &clipped).status,
            BenchmarkVerifierStatus::Failed
        );
    }

    #[test]
    fn result_receipt_rejects_missing_evidence_and_binding_drift() {
        let (fixtures, _, _, reconciliation, brief) = valid_inputs();
        let spec = task_spec().unwrap();
        let run = valid_run_result(&spec);
        let receipt = valid_result_receipt(&spec, &run, &fixtures, &reconciliation, &brief);

        let mut no_evidence = run.clone();
        no_evidence.verifier_results[0].evidence.clear();
        assert_eq!(
            verify_result_receipt(
                &spec,
                &no_evidence,
                &fixtures,
                &reconciliation,
                &brief,
                &receipt
            )
            .status,
            BenchmarkVerifierStatus::Failed
        );

        let mut wrong_verifier = run.clone();
        wrong_verifier.verifier_results[0].verifier_id = "t1.wrong/v1".to_string();
        assert_eq!(
            verify_result_receipt(
                &spec,
                &wrong_verifier,
                &fixtures,
                &reconciliation,
                &brief,
                &receipt
            )
            .status,
            BenchmarkVerifierStatus::Failed
        );

        let mut wrong_done_when = run.clone();
        wrong_done_when.verifier_results[0].done_when_id = "wrong-binding".to_string();
        assert_eq!(
            verify_result_receipt(
                &spec,
                &wrong_done_when,
                &fixtures,
                &reconciliation,
                &brief,
                &receipt
            )
            .status,
            BenchmarkVerifierStatus::Failed
        );

        let mut wrong_version = receipt;
        wrong_version.version = "t1.result-receipt/v2".to_string();
        assert_eq!(
            verify_result_receipt(
                &spec,
                &run,
                &fixtures,
                &reconciliation,
                &brief,
                &wrong_version
            )
            .status,
            BenchmarkVerifierStatus::Failed
        );
    }

    #[test]
    fn result_receipt_rejects_unsafe_paths_secrets_and_provider_text() {
        let (fixtures, _, _, reconciliation, brief) = valid_inputs();
        let spec = task_spec().unwrap();
        let run = valid_run_result(&spec);
        let receipt = valid_result_receipt(&spec, &run, &fixtures, &reconciliation, &brief);
        for unsafe_reference in [
            "C:/private/evidence.json",
            "\\\\server\\share\\evidence.json",
            "file://evidence.json",
            "../evidence.json",
        ] {
            let mut changed = run.clone();
            changed.verifier_results[0].evidence[0].relative_or_opaque_ref =
                unsafe_reference.to_string();
            assert_eq!(
                verify_result_receipt(
                    &spec,
                    &changed,
                    &fixtures,
                    &reconciliation,
                    &brief,
                    &receipt
                )
                .status,
                BenchmarkVerifierStatus::Failed
            );
        }
        let secret_markers = vec![
            "Bearer abcdefghijklmnop".to_string(),
            format!("{}{}", "sk-", "abcdefghijklmnop"),
            "test-secret marker".to_string(),
            "provider raw body".to_string(),
        ];
        for marker in secret_markers {
            let mut changed = run.clone();
            changed.verifier_results[0].evidence[0].summary = marker;
            assert_eq!(
                verify_result_receipt(
                    &spec,
                    &changed,
                    &fixtures,
                    &reconciliation,
                    &brief,
                    &receipt
                )
                .status,
                BenchmarkVerifierStatus::Failed
            );
        }
    }

    #[test]
    fn every_required_not_run_error_or_failed_status_breaks_completion_gate() {
        let spec = task_spec().unwrap();
        for index in 0..spec.done_when.len() {
            for status in [
                BenchmarkVerifierStatus::NotRun,
                BenchmarkVerifierStatus::Error,
                BenchmarkVerifierStatus::Failed,
            ] {
                let mut run = valid_run_result(&spec);
                run.verifier_results[index].status = status;
                let classification = classify_benchmark_run(&spec, &run).unwrap();
                assert!(!classification.verifier_gate_passed);
                assert!(classification.false_completion);
                assert_eq!(classification.outcome_class, BenchmarkOutcomeClass::F);
            }
        }
    }

    fn build_test_reconciliation(
        fixtures: &T1GeneratedFixtureSet,
        mutation: &XlsxMutation,
    ) -> T1CandidateArtifact {
        let facts = fact_map(expected_facts(fixtures).unwrap());
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
            let fact = &facts[fact_id];
            let value_ref = format!("B{row}");
            let provenance_ref = format!("C{row}");
            let value = mutation
                .value_overrides
                .get(&value_ref)
                .cloned()
                .unwrap_or_else(|| fact.value.clone());
            let locator = mutation
                .provenance_overrides
                .get(&provenance_ref)
                .cloned()
                .unwrap_or_else(|| fact.source.as_ref().unwrap().locator.clone());
            rows.push(format!(
                "<row r=\"{row}\">{}{}{}</row>",
                inline_cell(&format!("A{row}"), fact_id),
                value_cell(&value_ref, &value, None),
                inline_cell(&provenance_ref, &locator)
            ));
        }
        for (row, fact_id, formula) in derived_rows {
            let fact = &facts[fact_id];
            let value_ref = format!("B{row}");
            let provenance_ref = format!("C{row}");
            let value = mutation
                .value_overrides
                .get(&value_ref)
                .cloned()
                .unwrap_or_else(|| fact.value.clone());
            let formula = mutation
                .formula_overrides
                .get(&value_ref)
                .cloned()
                .unwrap_or_else(|| formula.to_string());
            let algorithm = &fact.derivation.as_ref().unwrap().algorithm_id;
            let provenance = mutation
                .provenance_overrides
                .get(&provenance_ref)
                .cloned()
                .unwrap_or_else(|| format!("derived:{algorithm}"));
            rows.push(format!(
                "<row r=\"{row}\">{}{}{}</row>",
                inline_cell(&format!("A{row}"), fact_id),
                value_cell(&value_ref, &value, Some(&formula)),
                inline_cell(&provenance_ref, &provenance)
            ));
        }
        let sheet = if mutation.malformed_sheet {
            "<worksheet><broken>".to_string()
        } else {
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><dimension ref=\"A1:C28\"/><sheetData>{}</sheetData></worksheet>",
                rows.join("")
            )
        };
        let content_types = br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#;
        let root_rels = root_relationships("xl/workbook.xml");
        let core = core_properties();
        let workbook = br#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Reconciliation" sheetId="1" r:id="rId1"/></sheets><calcPr calcMode="auto"/></workbook>"#;
        let workbook_rels = br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let mut parts = vec![
            ("[Content_Types].xml".to_string(), content_types.to_vec()),
            ("_rels/.rels".to_string(), root_rels.into_bytes()),
            ("docProps/core.xml".to_string(), core.into_bytes()),
            ("xl/workbook.xml".to_string(), workbook.to_vec()),
            (
                "xl/_rels/workbook.xml.rels".to_string(),
                workbook_rels.to_vec(),
            ),
        ];
        if !mutation.omit_sheet {
            parts.push(("xl/worksheets/sheet1.xml".to_string(), sheet.into_bytes()));
        }
        T1CandidateArtifact {
            relative_path: RECONCILIATION_OUTPUT_PATH.to_string(),
            bytes: zip_owned(parts),
        }
    }

    fn build_test_brief(
        fixtures: &T1GeneratedFixtureSet,
        mutation: &PptxMutation,
    ) -> T1CandidateArtifact {
        let facts = fact_map(expected_facts(fixtures).unwrap());
        let slide_count = if mutation.slides == 0 {
            1
        } else {
            mutation.slides
        };
        let total = if mutation.wrong_total {
            "1,702,401".to_string()
        } else {
            format_integer_grouped(fact_number(&facts, "total_revenue_cny").unwrap() as i64)
        };
        let mut text = if mutation.blank {
            Vec::new()
        } else {
            vec![
                "2026-06 月度经营简报".to_string(),
                format!("总收入 {total}"),
                format!(
                    "预算差异 {} / {:.2}%",
                    format_integer_grouped(
                        fact_number(&facts, "budget_variance_cny").unwrap() as i64
                    ),
                    fact_number(&facts, "budget_variance_rate").unwrap() * 100.0
                ),
                format!(
                    "入住率 {:.2}% / {:.2} percentage points",
                    fact_number(&facts, "occupancy_rate").unwrap() * 100.0,
                    fact_number(&facts, "occupancy_variance_percentage_points").unwrap()
                ),
                format!(
                    "早餐排队 {} 起",
                    fact_value(&facts, "breakfast_queue_complaints").unwrap()
                ),
                format!(
                    "电梯停运 {} 次",
                    fact_value(&facts, "elevator_2_unplanned_outages").unwrap()
                ),
                format!(
                    "防火门逾期 {} 项",
                    fact_value(&facts, "overdue_fire_door_closing_checks").unwrap()
                ),
            ]
        };
        if !mutation.blank && !mutation.omit_footnote {
            text.push(
                "来源: 01-monthly-revenue.xlsx; 02-operations-notes.docx; 03-risk-summary.pdf"
                    .to_string(),
            );
        }
        if mutation.replacement_character {
            text.push("bad \u{fffd} text".to_string());
        }
        let slide_xml = slide_xml(&text);
        let slide_ids = (1..=slide_count)
            .map(|index| format!("<p:sldId id=\"{}\" r:id=\"rId{index}\"/>", 255 + index))
            .collect::<Vec<_>>()
            .join("");
        let presentation = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><p:presentation xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx=\"12192000\" cy=\"6858000\"/></p:presentation>"
        );
        let presentation_rels = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{}</Relationships>",
            (1..=slide_count)
                .map(|index| format!("<Relationship Id=\"rId{index}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{index}.xml\"/>"))
                .collect::<Vec<_>>()
                .join("")
        );
        let slide_overrides = (1..=slide_count)
            .map(|index| format!("<Override PartName=\"/ppt/slides/slide{index}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"))
            .collect::<Vec<_>>()
            .join("");
        let content_types = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/><Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>{slide_overrides}<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>"
        );
        let root_rels = root_relationships("ppt/presentation.xml");
        let mut parts = vec![
            ("[Content_Types].xml".to_string(), content_types.into_bytes()),
            ("_rels/.rels".to_string(), root_rels.into_bytes()),
            ("docProps/core.xml".to_string(), core_properties().into_bytes()),
            ("ppt/presentation.xml".to_string(), presentation.into_bytes()),
            (
                "ppt/_rels/presentation.xml.rels".to_string(),
                presentation_rels.into_bytes(),
            ),
            (
                "ppt/slideLayouts/slideLayout1.xml".to_string(),
                br#"<?xml version="1.0" encoding="UTF-8"?><p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree/></p:cSld></p:sldLayout>"#.to_vec(),
            ),
        ];
        for index in 1..=slide_count {
            parts.push((
                format!("ppt/slides/slide{index}.xml"),
                slide_xml.as_bytes().to_vec(),
            ));
            parts.push((
                format!("ppt/slides/_rels/slide{index}.xml.rels"),
                br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"#.to_vec(),
            ));
        }
        T1CandidateArtifact {
            relative_path: BRIEF_OUTPUT_PATH.to_string(),
            bytes: zip_owned(parts),
        }
    }

    fn valid_render_evidence(
        reconciliation: &T1CandidateArtifact,
        brief: &T1CandidateArtifact,
    ) -> T1RenderEvidence {
        let mut preview_bytes = BTreeMap::new();
        let artifacts = [reconciliation, brief]
            .iter()
            .enumerate()
            .map(|(index, output)| {
                let path = format!("previews/t1-output-{}-1.png", index + 1);
                let png = test_png();
                preview_bytes.insert(path.clone(), png.clone());
                T1RenderArtifactReceipt {
                    output_relative_path: output.relative_path.clone(),
                    output_sha256: sha256(&output.bytes),
                    renderer_version: ACTUAL_RENDERER_VERSION.to_string(),
                    rendered_unit_count: 1,
                    preview_manifest_sha256: preview_manifest_hash(&[png.clone()]),
                    previews: vec![T1PreviewReceipt {
                        relative_path: path,
                        bytes: png.len() as u64,
                        sha256: sha256(&png),
                        width: 160,
                        height: 120,
                        edge_clipping: false,
                    }],
                }
            })
            .collect();
        T1RenderEvidence {
            receipt: T1ActualRenderReceipt {
                version: ACTUAL_RENDER_RECEIPT_VERSION.to_string(),
                artifacts,
            },
            preview_bytes,
        }
    }

    fn valid_run_result(spec: &BenchmarkTaskSpec) -> BenchmarkRunResult {
        BenchmarkRunResult {
            version: BENCHMARK_RUN_RESULT_VERSION.to_string(),
            run_id: "t1-run-0001".to_string(),
            task_id: spec.task_id.clone(),
            task_revision: spec.task_revision,
            task_spec_fingerprint: spec.fingerprint().unwrap(),
            repetition_index: 1,
            subject: BenchmarkSubject {
                app_version: "1.0.2".to_string(),
                source_commit: "c".repeat(40),
                release_tag: Some("v1.0.2".to_string()),
                environment_profile: "windows-synthetic".to_string(),
                clean_state_id: "clean-state-01".to_string(),
                access_mode: AccessMode::AskOnRisk,
            },
            started_at: Utc.with_ymd_and_hms(2026, 7, 16, 8, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 7, 16, 8, 0, 1).unwrap(),
            elapsed_ms: 1_000,
            terminal_state: BenchmarkTerminalState::ClaimedComplete,
            external_effect_state: BenchmarkExternalEffectState::None,
            outcome_class: BenchmarkOutcomeClass::A,
            interactions: BenchmarkInteractions {
                clarification_count: 0,
                authorization_count: 1,
                manual_interventions: Vec::new(),
            },
            deepseek_usage: vec![BenchmarkDeepSeekUsage {
                model: "deepseek-v4-flash".to_string(),
                api_call_count: 1,
                prompt_tokens: Some(100),
                completion_tokens: Some(50),
                total_tokens: Some(150),
                estimated_cost_micro_usd: Some(25),
            }],
            verifier_results: spec
                .done_when
                .iter()
                .map(|condition| BenchmarkVerifierResult {
                    done_when_id: condition.done_when_id.clone(),
                    verifier_id: condition.verifier_id.clone(),
                    status: BenchmarkVerifierStatus::Passed,
                    summary: "Synthetic deterministic verifier passed".to_string(),
                    evidence: vec![BenchmarkEvidenceReceipt {
                        kind: condition.required_evidence_kinds[0].clone(),
                        relative_or_opaque_ref: format!(
                            "benchmark-evidence:{}",
                            condition.done_when_id
                        ),
                        sha256: Some("d".repeat(64)),
                        summary: "Synthetic deterministic evidence receipt".to_string(),
                    }],
                })
                .collect(),
            guardrail_violations: Vec::new(),
            failure_stage: None,
            failure_code: None,
        }
    }

    fn valid_result_receipt(
        spec: &BenchmarkTaskSpec,
        run: &BenchmarkRunResult,
        fixtures: &T1GeneratedFixtureSet,
        reconciliation: &T1CandidateArtifact,
        brief: &T1CandidateArtifact,
    ) -> T1ResultReceipt {
        let facts = fact_map(expected_facts(fixtures).unwrap());
        T1ResultReceipt {
            version: RESULT_RECEIPT_VERSION.to_string(),
            run_id: run.run_id.clone(),
            task_id: spec.task_id.clone(),
            task_revision: spec.task_revision,
            task_spec_fingerprint: spec.fingerprint().unwrap(),
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
                ("period".to_string(), fact_value(&facts, "period").unwrap()),
                (
                    "total_revenue_cny".to_string(),
                    fact_value(&facts, "total_revenue_cny").unwrap(),
                ),
                (
                    "budget_variance_cny".to_string(),
                    fact_value(&facts, "budget_variance_cny").unwrap(),
                ),
                (
                    "budget_variance_rate".to_string(),
                    fact_value(&facts, "budget_variance_rate").unwrap(),
                ),
                (
                    "prior_variance_cny".to_string(),
                    fact_value(&facts, "prior_variance_cny").unwrap(),
                ),
                (
                    "prior_variance_rate".to_string(),
                    fact_value(&facts, "prior_variance_rate").unwrap(),
                ),
                (
                    "occupancy_rate".to_string(),
                    fact_value(&facts, "occupancy_rate").unwrap(),
                ),
                (
                    "occupancy_variance_percentage_points".to_string(),
                    fact_value(&facts, "occupancy_variance_percentage_points").unwrap(),
                ),
            ]),
            anomalies: vec![
                format!(
                    "breakfast_queue_complaints={}",
                    fact_value(&facts, "breakfast_queue_complaints").unwrap()
                ),
                format!(
                    "elevator_2_unplanned_outages={}",
                    fact_value(&facts, "elevator_2_unplanned_outages").unwrap()
                ),
                format!(
                    "overdue_fire_door_closing_checks={}",
                    fact_value(&facts, "overdue_fire_door_closing_checks").unwrap()
                ),
            ],
            evidence_refs: run
                .verifier_results
                .iter()
                .flat_map(|result| {
                    result
                        .evidence
                        .iter()
                        .map(|evidence| evidence.relative_or_opaque_ref.clone())
                })
                .collect(),
        }
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

    fn zip_owned(parts: Vec<(String, Vec<u8>)>) -> Vec<u8> {
        let refs = parts
            .iter()
            .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
            .collect::<Vec<_>>();
        write_deterministic_zip(&refs).unwrap()
    }

    fn test_png() -> Vec<u8> {
        let mut image = GrayImage::from_pixel(160, 120, Luma([255]));
        for y in 30..70 {
            for x in 25..130 {
                image.put_pixel(x, y, Luma([80]));
            }
        }
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
}
