use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use quick_xml::{events::Event, Reader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::{write::FileOptions, ZipArchive};

use super::tool_runtime::{
    AgentToolExecutor, ToolEvidence, ToolExecutionOutput, ToolExecutionPlan,
    ToolVerificationResult, T1_RECONCILIATION_TOOL_ID,
};

pub const T1_RECONCILIATION_ARTIFACT_ID: &str = "t1-reconciliation-xlsx";
pub const T1_SOURCE_MANIFEST_EVIDENCE_KIND: &str = "t1_source_manifest";
pub const T1_PROVENANCE_EVIDENCE_KIND: &str = "t1_fact_provenance";
pub const T1_RECONCILIATION_EVIDENCE_KIND: &str = "t1_reconciliation_xlsx";

const SOURCE_MANIFEST_VERSION: &str = "ds-agent.t1-source-manifest/v1";
const PROVENANCE_VERSION: &str = "ds-agent.t1-provenance-manifest/v1";
const ARTIFACT_RECEIPT_VERSION: &str = "ds-agent.t1-reconciliation-artifact/v1";
const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_OPC_PARTS: usize = 128;
const MAX_OPC_BYTES: u64 = 8 * 1024 * 1024;
const NUMERIC_TOLERANCE: f64 = 0.000_001;
const XLSX_MEDIA_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const DOCX_MEDIA_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const PDF_MEDIA_TYPE: &str = "application/pdf";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1ReconciliationRequest {
    pub source_directory: String,
    pub output_relative_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1SourceManifestEntry {
    pub source_id: String,
    pub relative_path: String,
    pub media_type: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1SourceManifest {
    pub version: String,
    pub source_set_id: String,
    pub entries: Vec<T1SourceManifestEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1SourceFactLocator {
    pub source_id: String,
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
    pub source_set_id: String,
    pub source_manifest_sha256: String,
    pub facts: Vec<T1FactProvenance>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1ReconciliationArtifactReceipt {
    pub version: String,
    pub artifact_id: String,
    pub relative_path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1ReconciliationOutcome {
    pub source_manifest: T1SourceManifest,
    pub provenance: T1ProvenanceManifest,
    pub artifact: T1ReconciliationArtifactReceipt,
    pub key_figures: BTreeMap<String, String>,
    pub completion_evidence: Vec<ToolEvidence>,
}

#[derive(Clone, Debug)]
struct SourceDocument {
    entry: T1SourceManifestEntry,
    bytes: Vec<u8>,
}

pub struct T1ReconciliationAgentToolExecutor<'a> {
    workspace_root: &'a Path,
}

impl<'a> T1ReconciliationAgentToolExecutor<'a> {
    pub fn new(workspace_root: &'a Path) -> Self {
        Self { workspace_root }
    }
}

impl AgentToolExecutor for T1ReconciliationAgentToolExecutor<'_> {
    fn execute(&self, plan: &ToolExecutionPlan) -> Result<ToolExecutionOutput, String> {
        if plan.contract.id != T1_RECONCILIATION_TOOL_ID {
            return Err(format!(
                "T1 reconciliation executor cannot execute `{}`",
                plan.contract.id
            ));
        }
        let request = serde_json::from_value::<T1ReconciliationRequest>(plan.request.input.clone())
            .map_err(|error| {
                format!("operations.reconcile_excel input could not be decoded: {error}")
            })?;
        let outcome = run_t1_reconciliation(self.workspace_root, &request)?;
        let source_count = outcome.source_manifest.entries.len();
        let fact_count = outcome.provenance.facts.len();
        let artifact_bytes = outcome.artifact.bytes;
        let evidence = outcome.completion_evidence.clone();
        Ok(ToolExecutionOutput {
            output: serde_json::to_value(&outcome).map_err(|error| {
                format!("T1 reconciliation output could not be serialized: {error}")
            })?,
            evidence,
            verification: ToolVerificationResult::passed(format!(
                "operations.reconcile_excel re-read {source_count} exact sources, reconciled {fact_count} facts, and verified {artifact_bytes} XLSX bytes"
            )),
        })
    }
}

pub fn run_t1_reconciliation(
    workspace_root: &Path,
    request: &T1ReconciliationRequest,
) -> Result<T1ReconciliationOutcome, String> {
    let workspace = canonical_workspace(workspace_root)?;
    let sources = scan_sources(&workspace, &request.source_directory)?;
    let source_manifest = build_source_manifest(&sources)?;
    let provenance = build_provenance(&source_manifest, &sources)?;
    let workbook = build_reconciliation_workbook(&provenance)?;
    verify_workbook(&source_manifest, &provenance, &workbook)?;

    let (output_path, output_relative_path) =
        resolve_new_output(&workspace, &request.output_relative_path)?;
    write_new_artifact(&output_path, &workbook)?;
    let artifact = T1ReconciliationArtifactReceipt {
        version: ARTIFACT_RECEIPT_VERSION.to_string(),
        artifact_id: T1_RECONCILIATION_ARTIFACT_ID.to_string(),
        relative_path: output_relative_path,
        bytes: workbook.len() as u64,
        sha256: sha256(&workbook),
    };

    match verify_persisted_t1_reconciliation(
        &workspace,
        request,
        &source_manifest,
        &provenance,
        &artifact,
    ) {
        Ok(completion_evidence) => Ok(T1ReconciliationOutcome {
            key_figures: key_figures(&provenance)?,
            source_manifest,
            provenance,
            artifact,
            completion_evidence,
        }),
        Err(error) => {
            remove_artifact_if_unchanged(&output_path, &artifact);
            Err(error)
        }
    }
}

pub fn verify_persisted_t1_reconciliation(
    workspace_root: &Path,
    request: &T1ReconciliationRequest,
    expected_sources: &T1SourceManifest,
    expected_provenance: &T1ProvenanceManifest,
    expected_artifact: &T1ReconciliationArtifactReceipt,
) -> Result<Vec<ToolEvidence>, String> {
    let workspace = canonical_workspace(workspace_root)?;
    let sources = scan_sources(&workspace, &request.source_directory)?;
    let source_manifest = build_source_manifest(&sources)?;
    if &source_manifest != expected_sources {
        return Err("T1 source identity changed before completion verification".to_string());
    }
    let provenance = build_provenance(&source_manifest, &sources)?;
    if &provenance != expected_provenance {
        return Err("T1 provenance changed before completion verification".to_string());
    }

    let (output_path, output_relative_path) =
        resolve_existing_output(&workspace, &request.output_relative_path)?;
    if expected_artifact.version != ARTIFACT_RECEIPT_VERSION
        || expected_artifact.artifact_id != T1_RECONCILIATION_ARTIFACT_ID
        || expected_artifact.relative_path != output_relative_path
    {
        return Err("T1 artifact identity receipt is invalid".to_string());
    }
    let bytes = read_bounded_file(&output_path, MAX_OPC_BYTES as usize)?;
    if expected_artifact.bytes != bytes.len() as u64 || expected_artifact.sha256 != sha256(&bytes) {
        return Err("T1 artifact bytes do not match the completion receipt".to_string());
    }
    verify_t1_reconciliation_artifact(&source_manifest, &provenance, expected_artifact, &bytes)
}

pub fn verify_existing_t1_reconciliation(
    workspace_root: &Path,
    source_directory: &str,
    expected: &T1ReconciliationOutcome,
) -> Result<T1ReconciliationOutcome, String> {
    let request = T1ReconciliationRequest {
        source_directory: source_directory.to_string(),
        output_relative_path: expected.artifact.relative_path.clone(),
    };
    let completion_evidence = verify_persisted_t1_reconciliation(
        workspace_root,
        &request,
        &expected.source_manifest,
        &expected.provenance,
        &expected.artifact,
    )?;
    if completion_evidence != expected.completion_evidence {
        return Err(
            "T1 reconciliation completion evidence changed before PPT generation".to_string(),
        );
    }
    if key_figures(&expected.provenance)? != expected.key_figures {
        return Err("T1 reconciliation key figures changed before PPT generation".to_string());
    }
    Ok(expected.clone())
}

pub fn verify_t1_reconciliation_artifact(
    source_manifest: &T1SourceManifest,
    provenance: &T1ProvenanceManifest,
    artifact: &T1ReconciliationArtifactReceipt,
    bytes: &[u8],
) -> Result<Vec<ToolEvidence>, String> {
    if artifact.version != ARTIFACT_RECEIPT_VERSION
        || artifact.artifact_id != T1_RECONCILIATION_ARTIFACT_ID
        || artifact.bytes != bytes.len() as u64
        || artifact.sha256 != sha256(bytes)
    {
        return Err("T1 artifact identity receipt is invalid".to_string());
    }
    verify_workbook(source_manifest, provenance, bytes)?;
    completion_evidence(source_manifest, provenance, artifact)
}

fn canonical_workspace(workspace_root: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(workspace_root)
        .map_err(|error| format!("T1 workspace is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("T1 workspace must be a real directory".to_string());
    }
    workspace_root
        .canonicalize()
        .map_err(|error| format!("T1 workspace could not be resolved: {error}"))
}

fn validated_relative_path(value: &str, label: &str) -> Result<(PathBuf, String), String> {
    let normalized = value.trim().replace('\\', "/");
    let normalized = normalized.trim_matches('/');
    if normalized.is_empty() {
        return Err(format!("{label} is required"));
    }
    let path = Path::new(normalized);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("{label} must stay inside the authorized workspace"));
    }
    Ok((path.to_path_buf(), normalized.to_string()))
}

fn scan_sources(workspace: &Path, relative_directory: &str) -> Result<Vec<SourceDocument>, String> {
    let (relative, _) = validated_relative_path(relative_directory, "T1 source directory")?;
    let candidate = workspace.join(relative);
    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|error| format!("T1 source directory is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("T1 source directory must be a real directory".to_string());
    }
    let directory = candidate
        .canonicalize()
        .map_err(|error| format!("T1 source directory could not be resolved: {error}"))?;
    if !directory.starts_with(workspace) {
        return Err("T1 source directory escaped the authorized workspace".to_string());
    }

    let mut paths = fs::read_dir(&directory)
        .map_err(|error| format!("T1 source directory could not be scanned: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("T1 source directory entry could not be read: {error}"))?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| source_role(path).is_some())
        .collect::<Vec<_>>();
    paths.sort();
    if paths.len() != 3 {
        return Err("T1 requires exactly one XLSX, one DOCX, and one PDF source".to_string());
    }

    let mut roles = BTreeSet::new();
    let mut documents = Vec::with_capacity(paths.len());
    for path in paths {
        let (source_id, media_type) =
            source_role(&path).ok_or_else(|| "T1 source type is unsupported".to_string())?;
        if !roles.insert(source_id) {
            return Err("T1 source role is duplicated".to_string());
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("T1 source metadata is unavailable: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("T1 source must be a real file".to_string());
        }
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("T1 source could not be resolved: {error}"))?;
        if !canonical.starts_with(workspace) {
            return Err("T1 source escaped the authorized workspace".to_string());
        }
        let bytes = read_bounded_file(&canonical, MAX_SOURCE_BYTES)?;
        let relative_path = canonical
            .strip_prefix(workspace)
            .map_err(|_| "T1 source relative path is unavailable".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        documents.push(SourceDocument {
            entry: T1SourceManifestEntry {
                source_id: source_id.to_string(),
                relative_path,
                media_type: media_type.to_string(),
                bytes: bytes.len() as u64,
                sha256: sha256(&bytes),
            },
            bytes,
        });
    }
    if roles != BTreeSet::from(["excel", "word", "pdf"]) {
        return Err("T1 source roles are incomplete".to_string());
    }
    Ok(documents)
}

fn source_role(path: &Path) -> Option<(&'static str, &'static str)> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "xlsx" => Some(("excel", XLSX_MEDIA_TYPE)),
        "docx" => Some(("word", DOCX_MEDIA_TYPE)),
        "pdf" => Some(("pdf", PDF_MEDIA_TYPE)),
        _ => None,
    }
}

fn read_bounded_file(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let mut file =
        File::open(path).map_err(|error| format!("T1 file could not be opened: {error}"))?;
    let mut bytes = Vec::new();
    Read::take(&mut file, maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("T1 file could not be read: {error}"))?;
    if bytes.is_empty() || bytes.len() > maximum {
        return Err("T1 file size is invalid".to_string());
    }
    Ok(bytes)
}

fn resolve_new_output(workspace: &Path, value: &str) -> Result<(PathBuf, String), String> {
    let (relative, normalized) = validated_relative_path(value, "T1 output path")?;
    if !normalized.to_ascii_lowercase().ends_with(".xlsx") {
        return Err("T1 output path must end in .xlsx".to_string());
    }
    let output = workspace.join(relative);
    if output.exists() {
        return Err("T1 output already exists; overwrite is blocked".to_string());
    }
    let parent = output
        .parent()
        .ok_or_else(|| "T1 output parent is invalid".to_string())?;
    let metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("T1 output parent is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("T1 output parent must be an authorized real directory".to_string());
    }
    let parent = parent
        .canonicalize()
        .map_err(|error| format!("T1 output parent could not be resolved: {error}"))?;
    if !parent.starts_with(workspace) {
        return Err("T1 output escaped the authorized workspace".to_string());
    }
    let file_name = output
        .file_name()
        .ok_or_else(|| "T1 output file name is invalid".to_string())?;
    Ok((parent.join(file_name), normalized))
}

fn resolve_existing_output(workspace: &Path, value: &str) -> Result<(PathBuf, String), String> {
    let (relative, normalized) = validated_relative_path(value, "T1 output path")?;
    if !normalized.to_ascii_lowercase().ends_with(".xlsx") {
        return Err("T1 output path must end in .xlsx".to_string());
    }
    let candidate = workspace.join(relative);
    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|error| format!("T1 output is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("T1 output must be a real file".to_string());
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|error| format!("T1 output could not be resolved: {error}"))?;
    if !canonical.starts_with(workspace) {
        return Err("T1 output escaped the authorized workspace".to_string());
    }
    Ok((canonical, normalized))
}

fn write_new_artifact(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "T1 output parent is invalid".to_string())?;
    let staged = parent.join(format!(".t1-reconciliation-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(|error| format!("T1 staged output could not be created: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("T1 staged output could not be written: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("T1 staged output could not be synchronized: {error}"))?;
        fs::rename(&staged, path)
            .map_err(|error| format!("T1 output could not be committed: {error}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(staged);
    }
    result
}

fn remove_artifact_if_unchanged(path: &Path, artifact: &T1ReconciliationArtifactReceipt) {
    let matches = fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| !metadata.file_type().is_symlink() && metadata.is_file())
        .and_then(|_| read_bounded_file(path, MAX_OPC_BYTES as usize).ok())
        .is_some_and(|bytes| {
            artifact.bytes == bytes.len() as u64 && artifact.sha256 == sha256(&bytes)
        });
    if matches {
        let _ = fs::remove_file(path);
    }
}

fn build_source_manifest(sources: &[SourceDocument]) -> Result<T1SourceManifest, String> {
    let entries = sources
        .iter()
        .map(|source| source.entry.clone())
        .collect::<Vec<_>>();
    let source_set_id = canonical_hash(&entries)?;
    Ok(T1SourceManifest {
        version: SOURCE_MANIFEST_VERSION.to_string(),
        source_set_id,
        entries,
    })
}

fn build_provenance(
    source_manifest: &T1SourceManifest,
    sources: &[SourceDocument],
) -> Result<T1ProvenanceManifest, String> {
    if source_manifest.version != SOURCE_MANIFEST_VERSION
        || source_manifest.entries
            != sources
                .iter()
                .map(|source| source.entry.clone())
                .collect::<Vec<_>>()
        || source_manifest.source_set_id != canonical_hash(&source_manifest.entries)?
    {
        return Err("T1 source manifest identity is invalid".to_string());
    }
    let excel = source_by_id(sources, "excel")?;
    let word = source_by_id(sources, "word")?;
    let pdf = source_by_id(sources, "pdf")?;
    let revenue = extract_xlsx_facts(&excel.bytes)?;
    let operations = extract_docx_facts(&word.bytes)?;
    let risks = extract_pdf_facts(&pdf.bytes)?;
    let period = fact_text(&revenue, "period")?;
    if operations.get("period").map(|value| value.0.as_str()) != Some(period.as_str())
        || risks.get("period").map(|value| value.0.as_str()) != Some(period.as_str())
    {
        return Err("T1 source periods conflict".to_string());
    }

    let mut facts = Vec::new();
    for key in [
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
    ] {
        facts.push(source_fact(excel, key, &revenue)?);
    }
    for key in [
        "breakfast_queue_complaints",
        "overdue_invoice_corrections_over_48h",
        "group_leads_deferred_to_july",
    ] {
        facts.push(source_fact(word, key, &operations)?);
    }
    for key in [
        "elevator_2_unplanned_outages",
        "overdue_fire_door_closing_checks",
        "temporary_food_staff_retraining_incomplete",
    ] {
        facts.push(source_fact(pdf, key, &risks)?);
    }

    let available = fact_number(&revenue, "available_room_nights")?;
    let sold = fact_number(&revenue, "sold_room_nights")?;
    let rooms = fact_number(&revenue, "rooms_revenue_cny")?;
    let food = fact_number(&revenue, "food_beverage_revenue_cny")?;
    let other = fact_number(&revenue, "other_revenue_cny")?;
    let budget = fact_number(&revenue, "budget_total_revenue_cny")?;
    let prior = fact_number(&revenue, "prior_period_total_revenue_cny")?;
    let budget_occupancy = fact_number(&revenue, "budget_occupancy_rate")?;
    if available <= 0.0 || sold <= 0.0 || budget == 0.0 || prior == 0.0 {
        return Err("T1 derived fact denominator is invalid".to_string());
    }
    let occupancy = sold / available;
    let adr = rooms / sold;
    let revpar = rooms / available;
    let total = rooms + food + other;
    let budget_variance = total - budget;
    let prior_variance = total - prior;
    for (fact_id, value, operands, algorithm_id, formula) in [
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
    ] {
        facts.push(derived_fact(
            fact_id,
            value,
            operands,
            algorithm_id,
            formula,
        ));
    }
    let by_id = facts
        .iter()
        .map(|fact| (fact.fact_id.as_str(), fact))
        .collect::<BTreeMap<_, _>>();
    for (reported, computed) in [
        ("reported_occupancy_rate", "occupancy_rate"),
        ("reported_adr_cny", "adr_cny"),
        ("reported_total_revenue_cny", "total_revenue_cny"),
    ] {
        if (provenance_number(&by_id, reported)? - provenance_number(&by_id, computed)?).abs()
            > NUMERIC_TOLERANCE
        {
            return Err(format!(
                "T1 numeric conflict: {reported} does not match {computed}"
            ));
        }
    }
    Ok(T1ProvenanceManifest {
        version: PROVENANCE_VERSION.to_string(),
        source_set_id: source_manifest.source_set_id.clone(),
        source_manifest_sha256: canonical_hash(source_manifest)?,
        facts,
    })
}

fn source_by_id<'a>(
    sources: &'a [SourceDocument],
    source_id: &str,
) -> Result<&'a SourceDocument, String> {
    let matches = sources
        .iter()
        .filter(|source| source.entry.source_id == source_id)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(format!("T1 source {source_id} is missing or duplicated"));
    }
    Ok(matches[0])
}

fn source_fact(
    source: &SourceDocument,
    fact_id: &str,
    extracted: &BTreeMap<String, (String, String)>,
) -> Result<T1FactProvenance, String> {
    let (value, locator) = extracted
        .get(fact_id)
        .ok_or_else(|| format!("T1 source fact {fact_id} is missing"))?;
    Ok(T1FactProvenance {
        fact_id: fact_id.to_string(),
        value: value.clone(),
        source: Some(T1SourceFactLocator {
            source_id: source.entry.source_id.clone(),
            relative_path: source.entry.relative_path.clone(),
            source_sha256: source.entry.sha256.clone(),
            locator: locator.clone(),
            extracted_value: value.clone(),
        }),
        derivation: None,
    })
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

fn extract_xlsx_facts(bytes: &[u8]) -> Result<BTreeMap<String, (String, String)>, String> {
    let package = OpcPackage::read(bytes)?;
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
    Ok(facts)
}

fn extract_docx_facts(bytes: &[u8]) -> Result<BTreeMap<String, (String, String)>, String> {
    let package = OpcPackage::read(bytes)?;
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
    Ok(facts)
}

fn extract_pdf_facts(bytes: &[u8]) -> Result<BTreeMap<String, (String, String)>, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "T1 PDF source is not deterministic UTF-8".to_string())?;
    if !text.starts_with("%PDF-1.4") || !text.ends_with("%%EOF\n") || !text.contains("xref") {
        return Err("T1 PDF source structure is invalid".to_string());
    }
    let mut facts = BTreeMap::new();
    for key in [
        "period",
        "elevator_2_unplanned_outages",
        "overdue_fire_door_closing_checks",
        "temporary_food_staff_retraining_incomplete",
    ] {
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
    Ok(facts)
}

fn build_reconciliation_workbook(provenance: &T1ProvenanceManifest) -> Result<Vec<u8>, String> {
    let facts = provenance
        .facts
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
        "<row r=\"1\">{}{}{}{}{}{}</row>",
        inline_cell("A1", "fact_id"),
        inline_cell("B1", "value"),
        inline_cell("C1", "source_or_kind"),
        inline_cell("D1", "hash_or_algorithm"),
        inline_cell("E1", "locator_or_operands"),
        inline_cell("F1", "provenance_fingerprint")
    )];
    for (row, fact_id) in source_rows {
        let fact = facts
            .get(fact_id)
            .ok_or_else(|| format!("missing T1 fact {fact_id}"))?;
        let source = fact
            .source
            .as_ref()
            .ok_or_else(|| format!("missing T1 source provenance {fact_id}"))?;
        rows.push(format!(
            "<row r=\"{row}\">{}{}{}{}{}{}</row>",
            inline_cell(&format!("A{row}"), fact_id),
            value_cell(&format!("B{row}"), &fact.value, None),
            inline_cell(&format!("C{row}"), &source.relative_path),
            inline_cell(&format!("D{row}"), &source.source_sha256),
            inline_cell(&format!("E{row}"), &source.locator),
            inline_cell(&format!("F{row}"), &canonical_hash(fact)?)
        ));
    }
    for (row, fact_id, formula) in derived_rows {
        let fact = facts
            .get(fact_id)
            .ok_or_else(|| format!("missing T1 fact {fact_id}"))?;
        let derivation = fact
            .derivation
            .as_ref()
            .ok_or_else(|| format!("missing T1 derivation {fact_id}"))?;
        rows.push(format!(
            "<row r=\"{row}\">{}{}{}{}{}{}</row>",
            inline_cell(&format!("A{row}"), fact_id),
            value_cell(&format!("B{row}"), &fact.value, Some(formula)),
            inline_cell(&format!("C{row}"), "derived"),
            inline_cell(&format!("D{row}"), &derivation.algorithm_id),
            inline_cell(&format!("E{row}"), &derivation.operands.join(",")),
            inline_cell(&format!("F{row}"), &canonical_hash(fact)?)
        ));
    }
    let sheet = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><dimension ref=\"A1:F28\"/><sheetData>{}</sheetData></worksheet>",
        rows.join("")
    );
    write_zip(BTreeMap::from([
        ("[Content_Types].xml".to_string(), b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>".to_vec()),
        ("_rels/.rels".to_string(), root_relationships("xl/workbook.xml").into_bytes()),
        ("docProps/core.xml".to_string(), b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\"/>".to_vec()),
        ("xl/workbook.xml".to_string(), b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><sheets><sheet name=\"Reconciliation\" sheetId=\"1\" r:id=\"rId1\"/></sheets><calcPr calcMode=\"auto\"/></workbook>".to_vec()),
        ("xl/_rels/workbook.xml.rels".to_string(), b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/></Relationships>".to_vec()),
        ("xl/worksheets/sheet1.xml".to_string(), sheet.into_bytes()),
    ]))
}

fn verify_workbook(
    source_manifest: &T1SourceManifest,
    provenance: &T1ProvenanceManifest,
    bytes: &[u8],
) -> Result<(), String> {
    if source_manifest.version != SOURCE_MANIFEST_VERSION
        || source_manifest.source_set_id != canonical_hash(&source_manifest.entries)?
        || provenance.version != PROVENANCE_VERSION
        || provenance.source_set_id != source_manifest.source_set_id
        || provenance.source_manifest_sha256 != canonical_hash(source_manifest)?
    {
        return Err("T1 provenance is not bound to the exact source manifest".to_string());
    }
    let package = OpcPackage::read(bytes)?;
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
    let sheet = package.text("xl/worksheets/sheet1.xml")?;
    for marker in ["#REF!", "#DIV/0!", "#VALUE!", "#N/A", "\u{fffd}"] {
        if sheet.contains(marker) {
            return Err("T1 reconciliation contains a formula or encoding error".to_string());
        }
    }
    let cells = parse_worksheet_cells(sheet)?;
    let facts = provenance
        .facts
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
        let fact = facts
            .get(fact_id)
            .ok_or_else(|| format!("T1 fact {fact_id} is missing"))?;
        let source = fact
            .source
            .as_ref()
            .ok_or_else(|| format!("T1 source provenance {fact_id} is missing"))?;
        require_text(&cells, &format!("A{row}"), fact_id)?;
        require_value(&cells, &format!("B{row}"), &fact.value)?;
        require_text(&cells, &format!("C{row}"), &source.relative_path)?;
        require_text(&cells, &format!("D{row}"), &source.source_sha256)?;
        require_text(&cells, &format!("E{row}"), &source.locator)?;
        require_text(&cells, &format!("F{row}"), &canonical_hash(fact)?)?;
    }
    for (row, fact_id, formula) in [
        (20, "occupancy_rate", "B4/B3"),
        (21, "adr_cny", "B6/B4"),
        (22, "revpar_cny", "B6/B3"),
        (23, "total_revenue_cny", "SUM(B6,B8,B9)"),
        (24, "budget_variance_cny", "B23-B11"),
        (25, "budget_variance_rate", "B24/B11"),
        (26, "prior_variance_cny", "B23-B12"),
        (27, "prior_variance_rate", "B26/B12"),
        (28, "occupancy_variance_percentage_points", "(B20-B13)*100"),
    ] {
        let fact = facts
            .get(fact_id)
            .ok_or_else(|| format!("T1 fact {fact_id} is missing"))?;
        let derivation = fact
            .derivation
            .as_ref()
            .ok_or_else(|| format!("T1 derivation {fact_id} is missing"))?;
        require_text(&cells, &format!("A{row}"), fact_id)?;
        let cell = cells
            .get(&format!("B{row}"))
            .ok_or_else(|| "T1 reconciliation formula cell is missing".to_string())?;
        if cell.formula.as_deref() != Some(formula) {
            return Err(format!("T1 reconciliation formula changed for {fact_id}"));
        }
        require_value(&cells, &format!("B{row}"), &fact.value)?;
        require_text(&cells, &format!("C{row}"), "derived")?;
        require_text(&cells, &format!("D{row}"), &derivation.algorithm_id)?;
        require_text(&cells, &format!("E{row}"), &derivation.operands.join(","))?;
        require_text(&cells, &format!("F{row}"), &canonical_hash(fact)?)?;
    }
    Ok(())
}

fn completion_evidence(
    source_manifest: &T1SourceManifest,
    provenance: &T1ProvenanceManifest,
    artifact: &T1ReconciliationArtifactReceipt,
) -> Result<Vec<ToolEvidence>, String> {
    Ok(vec![
        ToolEvidence {
            kind: T1_SOURCE_MANIFEST_EVIDENCE_KIND.to_string(),
            reference: format!("evidence:t1-source-manifest:{}", canonical_hash(source_manifest)?),
            summary: "Exact T1 source paths, sizes, and SHA-256 identities were re-read inside the authorized workspace.".to_string(),
        },
        ToolEvidence {
            kind: T1_PROVENANCE_EVIDENCE_KIND.to_string(),
            reference: format!("evidence:t1-provenance:{}", canonical_hash(provenance)?),
            summary: "Every source and derived T1 fact was traced and independently reconciled without a numeric conflict.".to_string(),
        },
        ToolEvidence {
            kind: T1_RECONCILIATION_EVIDENCE_KIND.to_string(),
            reference: artifact.artifact_id.clone(),
            summary: format!("Formula-backed XLSX re-read passed with artifact SHA-256 {}.", artifact.sha256),
        },
    ])
}

fn key_figures(provenance: &T1ProvenanceManifest) -> Result<BTreeMap<String, String>, String> {
    let facts = provenance
        .facts
        .iter()
        .map(|fact| (fact.fact_id.as_str(), fact.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    [
        "period",
        "total_revenue_cny",
        "budget_variance_cny",
        "budget_variance_rate",
        "prior_variance_cny",
        "prior_variance_rate",
        "occupancy_rate",
        "occupancy_variance_percentage_points",
    ]
    .into_iter()
    .map(|fact_id| {
        facts
            .get(fact_id)
            .map(|value| (fact_id.to_string(), (*value).to_string()))
            .ok_or_else(|| format!("T1 key figure {fact_id} is missing"))
    })
    .collect()
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
                capture = None
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
            let name = file.name().replace('\\', "/");
            validated_relative_path(&name, "T1 OPC part path")?;
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
        let mut reader = Reader::from_str(self.text("[Content_Types].xml")?);
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
        part: &str,
        type_suffix: &str,
        target: &str,
        expected_count: usize,
    ) -> Result<(), String> {
        let mut reader = Reader::from_str(self.text(part)?);
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
                    if attributes
                        .get("targetmode")
                        .is_some_and(|mode| mode.eq_ignore_ascii_case("external"))
                    {
                        return Err("T1 OPC external relationship is blocked".to_string());
                    }
                    if attributes
                        .get("type")
                        .is_some_and(|value| value.ends_with(type_suffix))
                        && attributes.get("target").map(String::as_str) == Some(target)
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
            if name.ends_with(".rels") {
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
        return Err(format!("T1 worksheet text cell {reference} is incorrect"));
    }
    Ok(())
}

fn require_value(
    cells: &BTreeMap<String, WorksheetCell>,
    reference: &str,
    expected: &str,
) -> Result<(), String> {
    let actual = cell_text(cells, reference)?;
    match (actual.parse::<f64>(), expected.parse::<f64>()) {
        (Ok(actual), Ok(expected)) if (actual - expected).abs() <= NUMERIC_TOLERANCE => Ok(()),
        (Err(_), Err(_)) if actual == expected => Ok(()),
        _ => Err(format!("T1 worksheet value cell {reference} is incorrect")),
    }
}

fn fact_text(facts: &BTreeMap<String, (String, String)>, key: &str) -> Result<String, String> {
    facts
        .get(key)
        .map(|value| value.0.clone())
        .ok_or_else(|| format!("T1 source value {key} is missing"))
}

fn fact_number(facts: &BTreeMap<String, (String, String)>, key: &str) -> Result<f64, String> {
    parse_number(&fact_text(facts, key)?)
}

fn provenance_number(
    facts: &BTreeMap<&str, &T1FactProvenance>,
    fact_id: &str,
) -> Result<f64, String> {
    parse_number(
        &facts
            .get(fact_id)
            .ok_or_else(|| format!("T1 fact {fact_id} is missing"))?
            .value,
    )
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

fn canonical_number(value: f64) -> String {
    if value.fract().abs() < 0.000_000_1 {
        format!("{value:.0}")
    } else {
        format!("{value:.6}")
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

fn root_relationships(target: &str) -> String {
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"{target}\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>")
}

fn write_zip(parts: BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (path, bytes) in parts {
        zip.start_file(&path, options)
            .map_err(|error| format!("T1 zip part {path} could not be started: {error}"))?;
        zip.write_all(&bytes)
            .map_err(|error| format!("T1 zip part {path} could not be written: {error}"))?;
    }
    zip.finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| format!("T1 zip package could not be finished: {error}"))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
    use super::*;
    use crate::kernel::benchmark::t1::fixtures::generate_fixture_set;
    use crate::kernel::models::AccessMode;
    use crate::kernel::tool_runtime::{
        prepare_tool_execution, ToolExecutionRequest, ToolExecutionStatus, ToolInvocationRecord,
    };

    fn fixture_workspace() -> (tempfile::TempDir, T1ReconciliationRequest) {
        let workspace = tempfile::tempdir().expect("workspace");
        let fixtures = generate_fixture_set().expect("fixtures");
        for fixture in fixtures.files {
            let path = workspace.path().join(fixture.relative_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, fixture.bytes).unwrap();
        }
        fs::create_dir(workspace.path().join("outputs")).unwrap();
        (
            workspace,
            T1ReconciliationRequest {
                source_directory: "inputs".to_string(),
                output_relative_path: "outputs/t1-reconciliation.xlsx".to_string(),
            },
        )
    }

    #[test]
    fn reconciliation_binds_sources_key_numbers_artifact_and_completion_evidence() {
        let (workspace, request) = fixture_workspace();
        let outcome = run_t1_reconciliation(workspace.path(), &request).expect("T1 reconciles");
        assert_eq!(outcome.source_manifest.entries.len(), 3);
        assert_eq!(outcome.provenance.facts.len(), 27);
        assert_eq!(outcome.key_figures.len(), 8);
        assert_eq!(outcome.completion_evidence.len(), 3);
        assert_eq!(
            outcome.completion_evidence[2].reference,
            T1_RECONCILIATION_ARTIFACT_ID
        );
        assert!(workspace
            .path()
            .join(&outcome.artifact.relative_path)
            .is_file());
        assert_eq!(
            verify_persisted_t1_reconciliation(
                workspace.path(),
                &request,
                &outcome.source_manifest,
                &outcome.provenance,
                &outcome.artifact,
            )
            .unwrap(),
            outcome.completion_evidence
        );
    }

    #[test]
    fn tool_executor_returns_contract_validated_kernel_completion_evidence() {
        let (workspace, request) = fixture_workspace();
        let plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: T1_RECONCILIATION_TOOL_ID.to_string(),
            input: serde_json::to_value(request).unwrap(),
            access_mode: AccessMode::FullAccess,
            run_id: Some(Uuid::new_v4()),
        })
        .unwrap();
        let executor = T1ReconciliationAgentToolExecutor::new(workspace.path());
        let output = executor.execute(&plan).unwrap();
        let invocation = ToolInvocationRecord::succeeded(
            &plan,
            output.output,
            output.evidence,
            output.verification,
            None,
            1,
        )
        .unwrap();

        assert_eq!(invocation.status, ToolExecutionStatus::Succeeded);
        assert_eq!(invocation.evidence.len(), 3);
        assert!(invocation.verification.passed);
    }

    #[test]
    fn reconciliation_detects_injected_numeric_conflict_before_writing() {
        let (workspace, request) = fixture_workspace();
        let path = workspace.path().join("inputs/01-monthly-revenue.xlsx");
        mutate_zip_part(&path, "xl/worksheets/sheet1.xml", |xml| {
            xml.replace(
                "<c r=\"B9\"><v>1702400</v></c>",
                "<c r=\"B9\"><v>1702401</v></c>",
            )
        });
        let error = run_t1_reconciliation(workspace.path(), &request).unwrap_err();
        assert!(error.contains("numeric conflict"));
        assert!(!workspace
            .path()
            .join(&request.output_relative_path)
            .exists());
    }

    #[test]
    fn reconciliation_rejects_damaged_formula_without_completion_evidence() {
        let (workspace, request) = fixture_workspace();
        let outcome = run_t1_reconciliation(workspace.path(), &request).unwrap();
        let path = workspace.path().join(&outcome.artifact.relative_path);
        let mut bytes = fs::read(path).unwrap();
        bytes = mutate_zip_bytes(bytes, "xl/worksheets/sheet1.xml", |xml| {
            xml.replace("<f>SUM(B6,B8,B9)</f>", "<f>SUM(B6,B8)</f>")
        });
        let altered = T1ReconciliationArtifactReceipt {
            bytes: bytes.len() as u64,
            sha256: sha256(&bytes),
            ..outcome.artifact.clone()
        };
        let error = verify_t1_reconciliation_artifact(
            &outcome.source_manifest,
            &outcome.provenance,
            &altered,
            &bytes,
        )
        .unwrap_err();
        assert!(error.contains("formula changed"));
    }

    #[test]
    fn reconciliation_rejects_path_escape_and_artifact_identity_drift() {
        let (workspace, mut request) = fixture_workspace();
        request.output_relative_path = "../outside.xlsx".to_string();
        assert!(run_t1_reconciliation(workspace.path(), &request)
            .unwrap_err()
            .contains("authorized workspace"));

        request.output_relative_path = "missing/t1-reconciliation.xlsx".to_string();
        assert!(run_t1_reconciliation(workspace.path(), &request)
            .unwrap_err()
            .contains("output parent is unavailable"));
        assert!(!workspace.path().join("missing").exists());

        request.output_relative_path = "outputs/t1-reconciliation.xlsx".to_string();
        let outcome = run_t1_reconciliation(workspace.path(), &request).unwrap();
        let mut drifted = outcome.artifact.clone();
        drifted.sha256 = "0".repeat(64);
        assert!(verify_persisted_t1_reconciliation(
            workspace.path(),
            &request,
            &outcome.source_manifest,
            &outcome.provenance,
            &drifted,
        )
        .unwrap_err()
        .contains("completion receipt"));
    }

    fn mutate_zip_part(path: &Path, part: &str, mutate: impl FnOnce(&str) -> String) {
        let bytes = fs::read(path).unwrap();
        fs::write(path, mutate_zip_bytes(bytes, part, mutate)).unwrap();
    }

    fn mutate_zip_bytes(
        bytes: Vec<u8>,
        part: &str,
        mutate: impl FnOnce(&str) -> String,
    ) -> Vec<u8> {
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut parts = BTreeMap::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let mut content = Vec::new();
            file.read_to_end(&mut content).unwrap();
            parts.insert(file.name().to_string(), content);
        }
        let text = String::from_utf8(parts.remove(part).unwrap()).unwrap();
        parts.insert(part.to_string(), mutate(&text).into_bytes());
        write_zip(parts).unwrap()
    }
}
