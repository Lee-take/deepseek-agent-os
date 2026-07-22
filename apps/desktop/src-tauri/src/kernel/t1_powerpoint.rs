use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use quick_xml::{events::Event, Reader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::{write::FileOptions, ZipArchive};

use super::artifact_render::render_artifact_file;
use super::artifacts::{
    preview_manifest_hash, ArtifactEngine, ArtifactFormat, ArtifactGenerationRequest,
    ArtifactInput, ArtifactPhase, ArtifactTemplate, MAX_ARTIFACT_REVISIONS,
};
use super::office::{build_office_artifact, OfficeApp, OfficeCreateSpec, OfficeSlideSpec};
use super::t1_reconciliation::{verify_existing_t1_reconciliation, T1ReconciliationOutcome};
use super::tool_runtime::{
    AgentToolExecutor, ToolEvidence, ToolExecutionOutput, ToolExecutionPlan,
    ToolVerificationResult, T1_POWERPOINT_TOOL_ID,
};

pub const T1_POWERPOINT_ARTIFACT_ID: &str = "t1-monthly-brief-pptx";
pub const T1_POWERPOINT_EVIDENCE_KIND: &str = "one_page_pptx";
pub const T1_POWERPOINT_RENDER_EVIDENCE_KIND: &str = "actual_render_receipt";
pub const T1_POWERPOINT_REVISION_EVIDENCE_KIND: &str = "office_revision_receipt";

const ARTIFACT_RECEIPT_VERSION: &str = "ds-agent.t1-powerpoint-artifact/v1";
const RENDER_RECEIPT_VERSION: &str = "ds-agent.t1-powerpoint-render/v1";
const REVISION_RECEIPT_VERSION: &str = "ds-agent.t1-powerpoint-revision/v1";
const MAX_PPTX_BYTES: usize = 16 * 1024 * 1024;
const MAX_OPC_PARTS: usize = 128;
const FIXED_CORE_TIMESTAMP: &str = "2000-01-01T00:00:00Z";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1PowerPointRequest {
    pub source_directory: String,
    pub reconciliation: T1ReconciliationOutcome,
    pub output_relative_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1PowerPointArtifactReceipt {
    pub version: String,
    pub artifact_id: String,
    pub original_relative_path: String,
    pub delivered_relative_path: String,
    pub bytes: u64,
    pub sha256: String,
    pub artifact_revision: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1PowerPointRenderReceipt {
    pub version: String,
    pub artifact_sha256: String,
    pub renderer_version: String,
    pub rendered_page_count: u32,
    pub preview_manifest_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1PowerPointRevisionReceipt {
    pub version: String,
    pub original_relative_path: String,
    pub delivered_relative_path: String,
    pub revision_attempts: u32,
    pub revision_paths: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct T1PowerPointOutcome {
    pub reconciliation_artifact_sha256: String,
    pub artifact: T1PowerPointArtifactReceipt,
    pub render: T1PowerPointRenderReceipt,
    pub revision: T1PowerPointRevisionReceipt,
    pub key_figures: BTreeMap<String, String>,
    pub anomalies: BTreeMap<String, String>,
    pub completion_evidence: Vec<ToolEvidence>,
}

pub struct T1PowerPointRender {
    pub pages: Vec<Vec<u8>>,
    pub renderer_version: String,
}

pub trait T1PowerPointRenderer {
    fn render(&self, path: &Path) -> Result<T1PowerPointRender, String>;
}

pub struct LocalT1PowerPointRenderer;

impl T1PowerPointRenderer for LocalT1PowerPointRenderer {
    fn render(&self, path: &Path) -> Result<T1PowerPointRender, String> {
        let render = render_artifact_file(ArtifactFormat::PowerPoint, path)?;
        Ok(T1PowerPointRender {
            pages: render.pages,
            renderer_version: render.renderer_version.to_string(),
        })
    }
}

pub struct T1PowerPointAgentToolExecutor<'a> {
    workspace_root: &'a Path,
    renderer: &'a dyn T1PowerPointRenderer,
}

impl<'a> T1PowerPointAgentToolExecutor<'a> {
    pub fn new(workspace_root: &'a Path, renderer: &'a dyn T1PowerPointRenderer) -> Self {
        Self {
            workspace_root,
            renderer,
        }
    }
}

impl AgentToolExecutor for T1PowerPointAgentToolExecutor<'_> {
    fn execute(&self, plan: &ToolExecutionPlan) -> Result<ToolExecutionOutput, String> {
        if plan.contract.id != T1_POWERPOINT_TOOL_ID {
            return Err(format!(
                "T1 PowerPoint executor cannot execute `{}`",
                plan.contract.id
            ));
        }
        let request = serde_json::from_value::<T1PowerPointRequest>(plan.request.input.clone())
            .map_err(|error| {
                format!("operations.generate_powerpoint input could not be decoded: {error}")
            })?;
        let outcome = run_t1_powerpoint(self.workspace_root, &request, self.renderer)?;
        let evidence = outcome.completion_evidence.clone();
        let bytes = outcome.artifact.bytes;
        let revisions = outcome.revision.revision_attempts;
        Ok(ToolExecutionOutput {
            output: serde_json::to_value(&outcome).map_err(|error| {
                format!("T1 PowerPoint output could not be serialized: {error}")
            })?,
            evidence,
            verification: ToolVerificationResult::passed(format!(
                "operations.generate_powerpoint reverified the exact C4A receipt, persisted and locally rendered one PPTX page ({bytes} bytes), and completed after {revisions} bounded revision(s)"
            )),
        })
    }
}

pub fn run_t1_powerpoint(
    workspace_root: &Path,
    request: &T1PowerPointRequest,
    renderer: &dyn T1PowerPointRenderer,
) -> Result<T1PowerPointOutcome, String> {
    let workspace = canonical_workspace(workspace_root)?;
    let reconciliation = verify_existing_t1_reconciliation(
        &workspace,
        &request.source_directory,
        &request.reconciliation,
    )?;
    let anomalies = anomaly_projection(&reconciliation)?;
    let (original_path, original_relative_path) =
        resolve_new_pptx(&workspace, &request.output_relative_path)?;
    let template = ArtifactTemplate::new(
        "t1-monthly-brief".to_string(),
        1,
        "T1 verified monthly brief".to_string(),
        vec![ArtifactFormat::PowerPoint],
        "one-page-verified-office".to_string(),
    );
    let initial_spec = powerpoint_spec(&original_relative_path, &reconciliation, &anomalies, 0)?;
    let generation = ArtifactEngine::generate_with_template(
        &ArtifactGenerationRequest {
            request_id: Uuid::new_v4(),
            input: ArtifactInput::Office { spec: initial_spec },
            template: template.reference.clone(),
            approved_storage_ref: format!("artifact-storage:{T1_POWERPOINT_ARTIFACT_ID}"),
        },
        &template,
        Utc::now(),
    )?;
    let mut record = generation.record;
    let frozen_input_fingerprint = record.input_fingerprint.clone();
    let mut bytes = canonicalize_pptx(generation.bytes)?;
    record.artifact_hash = sha256(&bytes);
    let mut delivered_path = original_path;
    let mut delivered_relative_path = original_relative_path.clone();
    let mut created = Vec::new();
    let mut revision_paths = Vec::new();

    let result = (|| {
        write_new_artifact(&workspace, &delivered_path, &bytes)?;
        created.push((delivered_path.clone(), sha256(&bytes)));

        loop {
            verify_powerpoint(
                &bytes,
                &reconciliation,
                &anomalies,
                &delivered_relative_path,
            )?;
            ArtifactEngine::check_structure(&mut record, &bytes, Utc::now())?;
            let render = renderer
                .render(&delivered_path)
                .map_err(|error| format!("T1 local Office renderer failed: {error}"))?;
            if render.pages.len() != 1 {
                return Err(
                    "T1 PowerPoint actual renderer must return exactly one page".to_string()
                );
            }
            let preview_hash = preview_manifest_hash(&render.pages);
            match ArtifactEngine::check_actual_visual(
                &mut record,
                &render.pages,
                &render.renderer_version,
                format!("artifact-preview:t1-one-page:{preview_hash}"),
                Utc::now(),
            ) {
                Ok(()) => {
                    record.complete(Utc::now())?;
                    if record.phase != ArtifactPhase::Completed {
                        return Err(
                            "T1 PowerPoint artifact did not reach completed state".to_string()
                        );
                    }
                    let persisted = read_bounded_file(&delivered_path, MAX_PPTX_BYTES)?;
                    if persisted != bytes || record.artifact_hash != sha256(&persisted) {
                        return Err(
                            "T1 PowerPoint bytes changed after local render verification"
                                .to_string(),
                        );
                    }
                    let artifact = T1PowerPointArtifactReceipt {
                        version: ARTIFACT_RECEIPT_VERSION.to_string(),
                        artifact_id: T1_POWERPOINT_ARTIFACT_ID.to_string(),
                        original_relative_path: original_relative_path.clone(),
                        delivered_relative_path: delivered_relative_path.clone(),
                        bytes: bytes.len() as u64,
                        sha256: record.artifact_hash.clone(),
                        artifact_revision: record.artifact_revision,
                    };
                    let render_receipt = T1PowerPointRenderReceipt {
                        version: RENDER_RECEIPT_VERSION.to_string(),
                        artifact_sha256: record.artifact_hash.clone(),
                        renderer_version: render.renderer_version,
                        rendered_page_count: 1,
                        preview_manifest_sha256: preview_hash,
                    };
                    let revision = T1PowerPointRevisionReceipt {
                        version: REVISION_RECEIPT_VERSION.to_string(),
                        original_relative_path: original_relative_path.clone(),
                        delivered_relative_path: delivered_relative_path.clone(),
                        revision_attempts: record.revision_attempts,
                        revision_paths: revision_paths.clone(),
                    };
                    let completion_evidence =
                        completion_evidence(&artifact, &render_receipt, &revision)?;
                    return Ok(T1PowerPointOutcome {
                        reconciliation_artifact_sha256: reconciliation.artifact.sha256.clone(),
                        artifact,
                        render: render_receipt,
                        revision,
                        key_figures: reconciliation.key_figures.clone(),
                        anomalies: anomalies.clone(),
                        completion_evidence,
                    });
                }
                Err(error) => {
                    if record.phase != ArtifactPhase::RevisionRequired {
                        return Err(format!(
                            "T1 PowerPoint actual visual verification failed: {error}"
                        ));
                    }
                    record.request_revision(Utc::now())?;
                    let attempt = record.revision_attempts;
                    if attempt > MAX_ARTIFACT_REVISIONS {
                        return Err("T1 PowerPoint revision limit was exceeded".to_string());
                    }
                    let (revision_path, revision_relative_path) =
                        revision_sibling(&workspace, &original_relative_path, attempt)?;
                    let spec = powerpoint_spec(
                        &revision_relative_path,
                        &reconciliation,
                        &anomalies,
                        attempt,
                    )?;
                    bytes = canonicalize_pptx(build_office_artifact(&spec)?)?;
                    record.replace_revision(
                        &bytes,
                        frozen_input_fingerprint.clone(),
                        Utc::now(),
                    )?;
                    write_new_artifact(&workspace, &revision_path, &bytes)?;
                    created.push((revision_path.clone(), sha256(&bytes)));
                    revision_paths.push(revision_relative_path.clone());
                    delivered_path = revision_path;
                    delivered_relative_path = revision_relative_path;
                }
            }
        }
    })();

    if result.is_err() {
        remove_created_if_unchanged(&created);
    }
    result
}

fn powerpoint_spec(
    relative_path: &str,
    reconciliation: &T1ReconciliationOutcome,
    anomalies: &BTreeMap<String, String>,
    revision: u32,
) -> Result<OfficeCreateSpec, String> {
    let value = |key: &str| {
        reconciliation
            .key_figures
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| format!("T1 PowerPoint key figure {key} is missing"))
    };
    let anomaly = |key: &str| {
        anomalies
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| format!("T1 PowerPoint anomaly {key} is missing"))
    };
    let sources = reconciliation
        .source_manifest
        .entries
        .iter()
        .map(|entry| entry.relative_path.as_str())
        .collect::<Vec<_>>();
    if sources.len() != 3 {
        return Err("T1 PowerPoint requires the exact three-source manifest".to_string());
    }
    let period = value("period")?;
    let body = match revision {
        0 => format!(
            "Period: {period}\nRevenue: CNY {} | Budget variance: CNY {} ({:.2}%)\nPrior-period variance: CNY {} ({:.2}%)\nOccupancy: {:.2}% | Budget gap: {} pp\nGuest/service: breakfast queue {} | invoice corrections >48h {} | July-deferred leads {}\nFacilities/people: elevator outages {} | overdue fire-door checks {} | retraining incomplete {}\nSources:\n{}\n{}\n{}",
            value("total_revenue_cny")?,
            value("budget_variance_cny")?,
            percentage(value("budget_variance_rate")?)?,
            value("prior_variance_cny")?,
            percentage(value("prior_variance_rate")?)?,
            percentage(value("occupancy_rate")?)?,
            value("occupancy_variance_percentage_points")?,
            anomaly("breakfast_queue_complaints")?,
            anomaly("overdue_invoice_corrections_over_48h")?,
            anomaly("group_leads_deferred_to_july")?,
            anomaly("elevator_2_unplanned_outages")?,
            anomaly("overdue_fire_door_closing_checks")?,
            anomaly("temporary_food_staff_retraining_incomplete")?,
            sources[0],
            sources[1],
            sources[2],
        ),
        _ => format!(
            "{period} | Revenue CNY {} | Budget CNY {} ({:.2}%) | Prior CNY {} ({:.2}%)\nOccupancy {:.2}% | Budget gap {} pp\nFlags: breakfast {} | invoices {} | July leads {} | elevator {} | fire doors {} | retraining {}\nSource 1: {}\nSource 2: {}\nSource 3: {}",
            value("total_revenue_cny")?,
            value("budget_variance_cny")?,
            percentage(value("budget_variance_rate")?)?,
            value("prior_variance_cny")?,
            percentage(value("prior_variance_rate")?)?,
            percentage(value("occupancy_rate")?)?,
            value("occupancy_variance_percentage_points")?,
            anomaly("breakfast_queue_complaints")?,
            anomaly("overdue_invoice_corrections_over_48h")?,
            anomaly("group_leads_deferred_to_july")?,
            anomaly("elevator_2_unplanned_outages")?,
            anomaly("overdue_fire_door_closing_checks")?,
            anomaly("temporary_food_staff_retraining_incomplete")?,
            sources[0],
            sources[1],
            sources[2],
        ),
    };
    Ok(OfficeCreateSpec {
        app: OfficeApp::PowerPoint,
        path: relative_path.to_string(),
        title: format!("Verified T1 monthly operating brief — {period}"),
        body: String::new(),
        rows: Vec::new(),
        slides: vec![OfficeSlideSpec {
            title: format!("Verified T1 monthly operating brief — {period}"),
            body,
        }],
    })
}

fn percentage(value: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .map(|value| value * 100.0)
        .map_err(|_| "T1 PowerPoint percentage value is invalid".to_string())
}

fn anomaly_projection(
    reconciliation: &T1ReconciliationOutcome,
) -> Result<BTreeMap<String, String>, String> {
    let facts = reconciliation
        .provenance
        .facts
        .iter()
        .map(|fact| (fact.fact_id.as_str(), fact.value.as_str()))
        .collect::<BTreeMap<_, _>>();
    [
        "breakfast_queue_complaints",
        "overdue_invoice_corrections_over_48h",
        "group_leads_deferred_to_july",
        "elevator_2_unplanned_outages",
        "overdue_fire_door_closing_checks",
        "temporary_food_staff_retraining_incomplete",
    ]
    .into_iter()
    .map(|fact_id| {
        facts
            .get(fact_id)
            .map(|value| (fact_id.to_string(), (*value).to_string()))
            .ok_or_else(|| format!("T1 PowerPoint anomaly {fact_id} is missing"))
    })
    .collect()
}

fn canonical_workspace(workspace_root: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(workspace_root)
        .map_err(|error| format!("T1 PowerPoint workspace is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("T1 PowerPoint workspace must be a real directory".to_string());
    }
    workspace_root
        .canonicalize()
        .map_err(|error| format!("T1 PowerPoint workspace could not be resolved: {error}"))
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

fn resolve_new_pptx(workspace: &Path, value: &str) -> Result<(PathBuf, String), String> {
    let (relative, normalized) = validated_relative_path(value, "T1 PowerPoint output path")?;
    if !normalized.to_ascii_lowercase().ends_with(".pptx") {
        return Err("T1 PowerPoint output path must end in .pptx".to_string());
    }
    let output = workspace.join(relative);
    validate_new_output(workspace, &output)?;
    Ok((output, normalized))
}

fn revision_sibling(
    workspace: &Path,
    original_relative_path: &str,
    revision: u32,
) -> Result<(PathBuf, String), String> {
    if revision == 0 || revision > MAX_ARTIFACT_REVISIONS {
        return Err("T1 PowerPoint revision number is invalid".to_string());
    }
    let original = Path::new(original_relative_path);
    let stem = original
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "T1 PowerPoint output name is invalid".to_string())?;
    let sibling = original.with_file_name(format!("{stem}.revision-{revision}.pptx"));
    let normalized = sibling.to_string_lossy().replace('\\', "/");
    let output = workspace.join(&sibling);
    validate_new_output(workspace, &output)?;
    Ok((output, normalized))
}

fn validate_new_output(workspace: &Path, output: &Path) -> Result<(), String> {
    if output.exists() {
        return Err("T1 PowerPoint output already exists; overwrite is blocked".to_string());
    }
    let parent = output
        .parent()
        .ok_or_else(|| "T1 PowerPoint output parent is invalid".to_string())?;
    let metadata = fs::symlink_metadata(parent)
        .map_err(|error| format!("T1 PowerPoint output parent is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("T1 PowerPoint output parent must be an authorized real directory".to_string());
    }
    let parent = parent
        .canonicalize()
        .map_err(|error| format!("T1 PowerPoint output parent could not be resolved: {error}"))?;
    if !parent.starts_with(workspace) {
        return Err("T1 PowerPoint output escaped the authorized workspace".to_string());
    }
    Ok(())
}

fn write_new_artifact(workspace: &Path, path: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_PPTX_BYTES {
        return Err("T1 PowerPoint artifact size is invalid".to_string());
    }
    validate_new_output(workspace, path)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("T1 PowerPoint artifact could not be created: {error}"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("T1 PowerPoint artifact could not be persisted: {error}"))?;
    drop(file);
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("T1 PowerPoint artifact metadata is unavailable: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("T1 PowerPoint artifact must be a real file".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("T1 PowerPoint artifact could not be resolved: {error}"))?;
    if !canonical.starts_with(workspace) || read_bounded_file(&canonical, MAX_PPTX_BYTES)? != bytes
    {
        return Err("T1 PowerPoint persisted bytes failed identity verification".to_string());
    }
    Ok(())
}

fn read_bounded_file(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("T1 PowerPoint artifact could not be opened: {error}"))?;
    let mut bytes = Vec::new();
    Read::take(&mut file, maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("T1 PowerPoint artifact could not be read: {error}"))?;
    if bytes.is_empty() || bytes.len() > maximum {
        return Err("T1 PowerPoint artifact size is invalid".to_string());
    }
    Ok(bytes)
}

fn remove_created_if_unchanged(created: &[(PathBuf, String)]) {
    for (path, expected_hash) in created.iter().rev() {
        let should_remove = fs::symlink_metadata(path)
            .ok()
            .is_some_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            && read_bounded_file(path, MAX_PPTX_BYTES)
                .ok()
                .is_some_and(|bytes| sha256(&bytes) == *expected_hash);
        if should_remove {
            let _ = fs::remove_file(path);
        }
    }
}

fn canonicalize_pptx(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "T1 PowerPoint OPC package cannot be opened".to_string())?;
    if archive.is_empty() || archive.len() > MAX_OPC_PARTS {
        return Err("T1 PowerPoint OPC part count is invalid".to_string());
    }
    let mut parts = BTreeMap::new();
    let mut expanded = 0usize;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|_| "T1 PowerPoint OPC part cannot be opened".to_string())?;
        if file.is_dir() {
            return Err("T1 PowerPoint OPC package contains a directory entry".to_string());
        }
        let name = file.name().replace('\\', "/");
        validated_relative_path(&name, "T1 PowerPoint OPC part path")?;
        let mut part = Vec::new();
        file.read_to_end(&mut part)
            .map_err(|_| "T1 PowerPoint OPC part cannot be read".to_string())?;
        expanded = expanded
            .checked_add(part.len())
            .ok_or_else(|| "T1 PowerPoint OPC expanded size overflow".to_string())?;
        if expanded > MAX_PPTX_BYTES {
            return Err("T1 PowerPoint OPC expanded size is invalid".to_string());
        }
        if name == "docProps/core.xml" {
            part = canonical_core_properties(&part)?;
        }
        if parts.insert(name, part).is_some() {
            return Err("T1 PowerPoint OPC package contains a duplicate part".to_string());
        }
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (path, part) in parts {
        zip.start_file(&path, options)
            .map_err(|error| format!("T1 PowerPoint OPC part {path} could not start: {error}"))?;
        zip.write_all(&part)
            .map_err(|error| format!("T1 PowerPoint OPC part {path} could not write: {error}"))?;
    }
    zip.finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| format!("T1 PowerPoint OPC package could not finish: {error}"))
}

fn canonical_core_properties(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut xml = std::str::from_utf8(bytes)
        .map_err(|_| "T1 PowerPoint core properties are invalid UTF-8".to_string())?
        .to_string();
    for tag in ["dcterms:created", "dcterms:modified"] {
        let start_marker = format!("<{tag} xsi:type=\"dcterms:W3CDTF\">");
        let start = xml
            .find(&start_marker)
            .ok_or_else(|| "T1 PowerPoint core timestamp is missing".to_string())?
            + start_marker.len();
        let end_marker = format!("</{tag}>");
        let end = xml[start..]
            .find(&end_marker)
            .ok_or_else(|| "T1 PowerPoint core timestamp is invalid".to_string())?
            + start;
        xml.replace_range(start..end, FIXED_CORE_TIMESTAMP);
    }
    Ok(xml.into_bytes())
}

fn verify_powerpoint(
    bytes: &[u8],
    reconciliation: &T1ReconciliationOutcome,
    anomalies: &BTreeMap<String, String>,
    expected_path: &str,
) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_PPTX_BYTES || !expected_path.ends_with(".pptx") {
        return Err("T1 PowerPoint artifact identity is invalid".to_string());
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "T1 PowerPoint OPC package cannot be opened".to_string())?;
    if archive.is_empty() || archive.len() > MAX_OPC_PARTS {
        return Err("T1 PowerPoint OPC part count is invalid".to_string());
    }
    let mut parts = BTreeMap::new();
    let mut expanded = 0usize;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|_| "T1 PowerPoint OPC part cannot be opened".to_string())?;
        if file.is_dir() {
            return Err("T1 PowerPoint OPC package contains a directory entry".to_string());
        }
        let name = file.name().replace('\\', "/");
        validated_relative_path(&name, "T1 PowerPoint OPC part path")?;
        if name.to_ascii_lowercase().ends_with(".bin")
            || name.to_ascii_lowercase().contains("vbaproject")
        {
            return Err("T1 PowerPoint macro content is blocked".to_string());
        }
        let mut part = Vec::new();
        file.read_to_end(&mut part)
            .map_err(|_| "T1 PowerPoint OPC part cannot be read".to_string())?;
        expanded = expanded
            .checked_add(part.len())
            .ok_or_else(|| "T1 PowerPoint OPC expanded size overflow".to_string())?;
        if expanded > MAX_PPTX_BYTES {
            return Err("T1 PowerPoint OPC expanded size is invalid".to_string());
        }
        if (name.ends_with(".xml") || name.ends_with(".rels"))
            && (validate_xml(&part).is_err() || String::from_utf8_lossy(&part).contains('\u{fffd}'))
        {
            return Err("T1 PowerPoint OPC XML is invalid".to_string());
        }
        if name.ends_with(".rels") {
            let lower = String::from_utf8_lossy(&part).to_ascii_lowercase();
            if lower.contains("targetmode=\"external\"")
                || lower.contains("target=\"http:")
                || lower.contains("target=\"https:")
                || lower.contains("target=\"file:")
                || lower.contains("target=\"\\\\")
            {
                return Err("T1 PowerPoint external relationship is blocked".to_string());
            }
        }
        if parts.insert(name, part).is_some() {
            return Err("T1 PowerPoint OPC package contains a duplicate part".to_string());
        }
    }
    for required in [
        "[Content_Types].xml",
        "_rels/.rels",
        "ppt/presentation.xml",
        "ppt/_rels/presentation.xml.rels",
        "ppt/slides/slide1.xml",
    ] {
        if !parts.contains_key(required) {
            return Err("T1 PowerPoint OPC package is missing a required part".to_string());
        }
    }
    let slide_parts = parts
        .keys()
        .filter(|name| {
            name.strip_prefix("ppt/slides/slide")
                .and_then(|name| name.strip_suffix(".xml"))
                .is_some_and(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
        })
        .count();
    if slide_parts != 1 {
        return Err("T1 PowerPoint must contain exactly one slide".to_string());
    }
    let slide_xml = std::str::from_utf8(&parts["ppt/slides/slide1.xml"])
        .map_err(|_| "T1 PowerPoint slide text is invalid UTF-8".to_string())?;
    let slide_text = xml_text(slide_xml)?;
    let key_value = |key: &str| {
        reconciliation
            .key_figures
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| format!("T1 PowerPoint key figure {key} is missing"))
    };
    let mut required_text = vec![
        key_value("period")?.to_string(),
        key_value("total_revenue_cny")?.to_string(),
        key_value("budget_variance_cny")?.to_string(),
        format!("{:.2}", percentage(key_value("budget_variance_rate")?)?),
        key_value("prior_variance_cny")?.to_string(),
        format!("{:.2}", percentage(key_value("prior_variance_rate")?)?),
        format!("{:.2}", percentage(key_value("occupancy_rate")?)?),
        key_value("occupancy_variance_percentage_points")?.to_string(),
    ];
    required_text.extend(anomalies.values().cloned());
    required_text.extend(
        reconciliation
            .source_manifest
            .entries
            .iter()
            .map(|entry| entry.relative_path.clone()),
    );
    if required_text
        .into_iter()
        .any(|required| !slide_text.contains(&required))
    {
        return Err("T1 PowerPoint slide is missing verified source content".to_string());
    }
    Ok(())
}

fn validate_xml(bytes: &[u8]) -> Result<(), String> {
    let mut reader = Reader::from_reader(bytes);
    loop {
        match reader.read_event() {
            Ok(Event::Eof) => return Ok(()),
            Ok(_) => {}
            Err(_) => return Err("T1 PowerPoint XML is invalid".to_string()),
        }
    }
}

fn xml_text(xml: &str) -> Result<String, String> {
    let mut reader = Reader::from_str(xml);
    let mut text = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Text(value)) => {
                let decoded = value
                    .decode()
                    .map_err(|_| "T1 PowerPoint text encoding is invalid".to_string())?;
                let unescaped = quick_xml::escape::unescape(&decoded)
                    .map_err(|_| "T1 PowerPoint text escaping is invalid".to_string())?;
                text.push_str(&unescaped);
                text.push('\n');
            }
            Ok(Event::Eof) => return Ok(text),
            Ok(_) => {}
            Err(_) => return Err("T1 PowerPoint slide XML is invalid".to_string()),
        }
    }
}

fn completion_evidence(
    artifact: &T1PowerPointArtifactReceipt,
    render: &T1PowerPointRenderReceipt,
    revision: &T1PowerPointRevisionReceipt,
) -> Result<Vec<ToolEvidence>, String> {
    Ok(vec![
        ToolEvidence {
            kind: T1_POWERPOINT_EVIDENCE_KIND.to_string(),
            reference: T1_POWERPOINT_ARTIFACT_ID.to_string(),
            summary: format!(
                "One-page PPTX persisted and re-read with artifact SHA-256 {}.",
                artifact.sha256
            ),
        },
        ToolEvidence {
            kind: T1_POWERPOINT_RENDER_EVIDENCE_KIND.to_string(),
            reference: format!("evidence:t1-office-render:{}", canonical_hash(render)?),
            summary: format!(
                "Microsoft Office actual render verified {} non-blank page with preview manifest SHA-256 {}.",
                render.rendered_page_count, render.preview_manifest_sha256
            ),
        },
        ToolEvidence {
            kind: T1_POWERPOINT_REVISION_EVIDENCE_KIND.to_string(),
            reference: format!("evidence:t1-office-revision:{}", canonical_hash(revision)?),
            summary: format!(
                "Bounded sibling-only revision workflow completed after {} revision(s).",
                revision.revision_attempts
            ),
        },
    ])
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn canonical_hash<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_vec(value)
        .map(|bytes| sha256(&bytes))
        .map_err(|error| format!("T1 PowerPoint receipt could not be serialized: {error}"))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use image::{DynamicImage, GrayImage, ImageFormat, Luma};

    use super::*;
    use crate::kernel::benchmark::t1::fixtures::generate_fixture_set;
    use crate::kernel::models::AccessMode;
    use crate::kernel::t1_reconciliation::{run_t1_reconciliation, T1ReconciliationRequest};
    use crate::kernel::tool_runtime::{
        prepare_tool_execution, ToolExecutionRequest, ToolExecutionStatus, ToolInvocationRecord,
    };

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
                renderer_version: "fixture-office-renderer/v1".to_string(),
            })
        }
    }

    fn fixture_request() -> (tempfile::TempDir, T1PowerPointRequest) {
        let workspace = tempfile::tempdir().expect("workspace");
        let fixtures = generate_fixture_set().expect("fixtures");
        for fixture in fixtures.files {
            let path = workspace.path().join(fixture.relative_path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, fixture.bytes).unwrap();
        }
        fs::create_dir(workspace.path().join("outputs")).unwrap();
        let reconciliation = run_t1_reconciliation(
            workspace.path(),
            &T1ReconciliationRequest {
                source_directory: "inputs".to_string(),
                output_relative_path: "outputs/t1-reconciliation.xlsx".to_string(),
            },
        )
        .expect("reconciliation");
        (
            workspace,
            T1PowerPointRequest {
                source_directory: "inputs".to_string(),
                reconciliation,
                output_relative_path: "outputs/t1-monthly-brief.pptx".to_string(),
            },
        )
    }

    fn blank_preview() -> Vec<u8> {
        png_preview(false)
    }

    fn valid_preview() -> Vec<u8> {
        png_preview(true)
    }

    fn png_preview(with_content: bool) -> Vec<u8> {
        let mut image = GrayImage::from_pixel(320, 180, Luma([255]));
        if with_content {
            for y in 60..120 {
                for x in 80..240 {
                    image.put_pixel(x, y, Luma([32]));
                }
            }
        }
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageLuma8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .unwrap();
        cursor.into_inner()
    }

    fn renderer(renders: Vec<Result<Vec<Vec<u8>>, String>>) -> FixtureRenderer {
        FixtureRenderer {
            renders: RefCell::new(renders.into()),
        }
    }

    #[test]
    fn powerpoint_executor_binds_c4a_and_returns_contract_validated_completion() {
        let (workspace, request) = fixture_request();
        let plan = prepare_tool_execution(&ToolExecutionRequest {
            tool_id: T1_POWERPOINT_TOOL_ID.to_string(),
            input: serde_json::to_value(&request).unwrap(),
            access_mode: AccessMode::FullAccess,
            run_id: Some(Uuid::new_v4()),
        })
        .unwrap();
        let fixture_renderer = renderer(vec![Ok(vec![valid_preview()])]);
        let executor = T1PowerPointAgentToolExecutor::new(workspace.path(), &fixture_renderer);
        let output = executor.execute(&plan).unwrap();
        let outcome: T1PowerPointOutcome = serde_json::from_value(output.output.clone()).unwrap();
        let mut incomplete_evidence = output.evidence.clone();
        incomplete_evidence.retain(|item| item.kind != T1_POWERPOINT_RENDER_EVIDENCE_KIND);
        assert!(ToolInvocationRecord::succeeded(
            &plan,
            output.output.clone(),
            incomplete_evidence,
            output.verification.clone(),
            None,
            1,
        )
        .is_err());
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
        assert_eq!(outcome.render.rendered_page_count, 1);
        assert_eq!(outcome.revision.revision_attempts, 0);
        assert_eq!(
            outcome.reconciliation_artifact_sha256,
            request.reconciliation.artifact.sha256
        );
        assert!(workspace
            .path()
            .join(outcome.artifact.delivered_relative_path)
            .is_file());
    }

    #[test]
    fn visual_failure_creates_a_new_sibling_and_preserves_original() {
        let (workspace, request) = fixture_request();
        let fixture_renderer = renderer(vec![Ok(vec![blank_preview()]), Ok(vec![valid_preview()])]);
        let outcome = run_t1_powerpoint(workspace.path(), &request, &fixture_renderer).unwrap();
        let original = workspace
            .path()
            .join(&outcome.artifact.original_relative_path);
        let delivered = workspace
            .path()
            .join(&outcome.artifact.delivered_relative_path);

        assert_eq!(outcome.revision.revision_attempts, 1);
        assert!(outcome
            .artifact
            .delivered_relative_path
            .ends_with(".revision-1.pptx"));
        assert!(original.is_file());
        assert!(delivered.is_file());
        assert_ne!(fs::read(original).unwrap(), fs::read(delivered).unwrap());
    }

    #[test]
    fn exhausted_visual_revisions_fail_and_remove_only_created_pptx_files() {
        let (workspace, request) = fixture_request();
        let fixture_renderer = renderer(vec![
            Ok(vec![blank_preview()]),
            Ok(vec![blank_preview()]),
            Ok(vec![blank_preview()]),
            Ok(vec![blank_preview()]),
        ]);
        let error = run_t1_powerpoint(workspace.path(), &request, &fixture_renderer).unwrap_err();
        let pptx_paths = fs::read_dir(workspace.path().join("outputs"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "pptx")
            })
            .collect::<Vec<_>>();

        assert!(error.contains("revision limit"));
        assert!(pptx_paths.is_empty());
        assert!(workspace
            .path()
            .join(&request.reconciliation.artifact.relative_path)
            .is_file());
    }

    #[test]
    fn source_or_reconciliation_drift_fails_without_leaving_a_pptx() {
        let (workspace, request) = fixture_request();
        fs::write(
            workspace.path().join("inputs/03-operations-notes.pdf"),
            b"changed",
        )
        .unwrap();
        let fixture_renderer = renderer(vec![Ok(vec![valid_preview()])]);
        run_t1_powerpoint(workspace.path(), &request, &fixture_renderer).unwrap_err();
        assert!(!workspace
            .path()
            .join(&request.output_relative_path)
            .exists());
    }

    #[test]
    fn output_escape_overwrite_and_renderer_failure_are_fail_closed() {
        let (workspace, mut request) = fixture_request();
        let fixture_renderer = renderer(vec![Ok(vec![valid_preview()])]);
        request.output_relative_path = "../escaped.pptx".to_string();
        assert!(run_t1_powerpoint(workspace.path(), &request, &fixture_renderer).is_err());

        request.output_relative_path = "outputs/existing.pptx".to_string();
        fs::write(
            workspace.path().join(&request.output_relative_path),
            b"owned",
        )
        .unwrap();
        assert!(run_t1_powerpoint(workspace.path(), &request, &fixture_renderer).is_err());
        assert_eq!(
            fs::read(workspace.path().join(&request.output_relative_path)).unwrap(),
            b"owned"
        );

        request.output_relative_path = "outputs/render-failure.pptx".to_string();
        let failing_renderer = renderer(vec![Err("Office unavailable".to_string())]);
        assert!(run_t1_powerpoint(workspace.path(), &request, &failing_renderer).is_err());
        assert!(!workspace
            .path()
            .join(&request.output_relative_path)
            .exists());
    }

    #[test]
    fn pptx_bytes_are_deterministic_for_the_same_verified_input_and_revision() {
        let (workspace_a, request_a) = fixture_request();
        let (workspace_b, request_b) = fixture_request();
        let renderer_a = renderer(vec![Ok(vec![valid_preview()])]);
        let renderer_b = renderer(vec![Ok(vec![valid_preview()])]);
        let outcome_a = run_t1_powerpoint(workspace_a.path(), &request_a, &renderer_a).unwrap();
        let outcome_b = run_t1_powerpoint(workspace_b.path(), &request_b, &renderer_b).unwrap();

        assert_eq!(outcome_a.artifact.sha256, outcome_b.artifact.sha256);
        assert_eq!(outcome_a.artifact.bytes, outcome_b.artifact.bytes);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires installed Microsoft Office and pdftoppm"]
    fn live_office_render_verifies_the_generated_one_page_pptx() {
        let (workspace, request) = fixture_request();
        let outcome = run_t1_powerpoint(workspace.path(), &request, &LocalT1PowerPointRenderer)
            .expect("PowerPoint actual render");
        assert_eq!(outcome.render.rendered_page_count, 1);
    }
}
