use std::io::{Cursor, Read};

use chrono::{DateTime, Utc};
use image::ImageFormat;
#[cfg(test)]
use image::{GrayImage, Luma};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::ZipArchive;

#[cfg(test)]
use super::event_store::EventStore;
use super::office::{build_office_artifact, OfficeApp, OfficeCreateSpec};

pub const MAX_ARTIFACT_REVISIONS: u32 = 3;
const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactFormat {
    Word,
    Excel,
    PowerPoint,
    Pdf,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactPhase {
    Generated,
    StructureChecked,
    VisualChecked,
    RevisionRequired,
    RevisionPrepared,
    ReadyForDelivery,
    Completed,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactTemplateRef {
    pub template_id: String,
    pub version: u32,
    pub content_hash: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactTemplate {
    pub reference: ArtifactTemplateRef,
    pub display_name: String,
    pub supported_formats: Vec<ArtifactFormat>,
    pub style_profile: String,
}

impl ArtifactTemplate {
    pub fn new(
        template_id: String,
        version: u32,
        display_name: String,
        supported_formats: Vec<ArtifactFormat>,
        style_profile: String,
    ) -> Self {
        let mut template = Self {
            reference: ArtifactTemplateRef {
                template_id,
                version,
                content_hash: String::new(),
            },
            display_name,
            supported_formats,
            style_profile,
        };
        template.reference.content_hash = template.computed_content_hash();
        template
    }

    fn computed_content_hash(&self) -> String {
        let manifest = serde_json::to_vec(&(
            "ds-agent.artifact-template.v1",
            &self.reference.template_id,
            self.reference.version,
            &self.display_name,
            &self.supported_formats,
            &self.style_profile,
        ))
        .expect("artifact template manifest is serializable");
        sha256(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.reference.validate()?;
        if self.display_name.trim().is_empty()
            || self.display_name.len() > 128
            || self.supported_formats.is_empty()
            || self.supported_formats.len() > 4
            || self.style_profile.trim().is_empty()
            || self.style_profile.len() > 128
            || self.reference.content_hash != self.computed_content_hash()
        {
            return Err("artifact template is invalid".to_string());
        }
        Ok(())
    }
}

impl ArtifactTemplateRef {
    pub fn validate(&self) -> Result<(), String> {
        if self.template_id.is_empty()
            || self.template_id.len() > 128
            || !self
                .template_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            || self.version == 0
            || !valid_sha256(&self.content_hash)
        {
            return Err("artifact template reference is invalid".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArtifactInput {
    Office {
        spec: OfficeCreateSpec,
    },
    Pdf {
        title: String,
        paragraphs: Vec<String>,
    },
}

impl ArtifactInput {
    fn format(&self) -> ArtifactFormat {
        match self {
            Self::Office { spec } => match spec.app {
                OfficeApp::Word => ArtifactFormat::Word,
                OfficeApp::Excel => ArtifactFormat::Excel,
                OfficeApp::PowerPoint => ArtifactFormat::PowerPoint,
            },
            Self::Pdf { .. } => ArtifactFormat::Pdf,
        }
    }

    pub(crate) fn fingerprint_for_template(&self, template_hash: &str) -> Result<String, String> {
        let input_json =
            serde_json::to_vec(self).map_err(|_| "artifact input is invalid".to_string())?;
        Ok(sha256(
            [
                b"ds-agent.artifact-input.v1\0".as_slice(),
                input_json.as_slice(),
                template_hash.as_bytes(),
            ]
            .concat(),
        ))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactGenerationRequest {
    pub request_id: Uuid,
    pub input: ArtifactInput,
    pub template: ArtifactTemplateRef,
    pub approved_storage_ref: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactValidationKind {
    Structure,
    Visual,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactValidationEvidence {
    pub id: Uuid,
    pub kind: ArtifactValidationKind,
    pub artifact_revision: u32,
    pub artifact_hash: String,
    pub input_fingerprint: String,
    pub template_hash: String,
    pub validator_version: String,
    pub passed: bool,
    pub checks: Vec<String>,
    #[serde(default)]
    pub preview_ref: Option<String>,
    #[serde(default)]
    pub rendered_page_count: u32,
    #[serde(default)]
    pub preview_manifest_hash: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactRecord {
    pub id: Uuid,
    pub request_id: Uuid,
    pub format: ArtifactFormat,
    pub phase: ArtifactPhase,
    pub artifact_revision: u32,
    pub artifact_hash: String,
    pub input_fingerprint: String,
    pub input: ArtifactInput,
    pub template: ArtifactTemplateRef,
    pub storage_ref: String,
    pub structure_evidence: Option<ArtifactValidationEvidence>,
    pub visual_evidence: Option<ArtifactValidationEvidence>,
    pub revision_attempts: u32,
    pub safe_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ArtifactDeliveryView {
    pub id: Uuid,
    pub format: ArtifactFormat,
    pub phase: ArtifactPhase,
    pub status_code: String,
    pub structure_checked: bool,
    pub visual_checked: bool,
    pub revision_attempts: u32,
    pub preview_available: bool,
    pub rendered_page_count: u32,
    pub updated_at: DateTime<Utc>,
}

impl ArtifactRecord {
    pub fn public_view(&self) -> ArtifactDeliveryView {
        ArtifactDeliveryView {
            id: self.id,
            format: self.format,
            phase: self.phase,
            status_code: match self.phase {
                ArtifactPhase::Generated => "generated_check_pending",
                ArtifactPhase::StructureChecked => "structure_passed_visual_pending",
                ArtifactPhase::VisualChecked | ArtifactPhase::ReadyForDelivery => {
                    "checks_passed_delivery_pending"
                }
                ArtifactPhase::RevisionRequired | ArtifactPhase::RevisionPrepared => {
                    "revision_in_progress"
                }
                ArtifactPhase::Completed => "completed",
                ArtifactPhase::Failed => "needs_attention",
            }
            .to_string(),
            structure_checked: self.structure_evidence.as_ref().is_some_and(|e| e.passed),
            visual_checked: self.visual_evidence.as_ref().is_some_and(|e| e.passed),
            revision_attempts: self.revision_attempts,
            preview_available: self
                .visual_evidence
                .as_ref()
                .is_some_and(|evidence| evidence.passed && evidence.preview_ref.is_some()),
            rendered_page_count: self
                .visual_evidence
                .as_ref()
                .map(|evidence| evidence.rendered_page_count)
                .unwrap_or(0),
            updated_at: self.updated_at,
        }
    }

    pub fn request_revision(&mut self, now: DateTime<Utc>) -> Result<(), String> {
        if self.phase == ArtifactPhase::Completed {
            return Err("completed artifact cannot be revised".to_string());
        }
        if self.revision_attempts >= MAX_ARTIFACT_REVISIONS {
            self.phase = ArtifactPhase::Failed;
            self.safe_error = Some("automatic revision limit reached".to_string());
            self.updated_at = now;
            return Err("automatic revision limit reached".to_string());
        }
        self.revision_attempts += 1;
        self.phase = ArtifactPhase::RevisionPrepared;
        self.updated_at = now;
        Ok(())
    }

    pub fn replace_revision(
        &mut self,
        bytes: &[u8],
        input_fingerprint: String,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        if self.phase != ArtifactPhase::RevisionPrepared {
            return Err("artifact is not waiting for revision".to_string());
        }
        validate_size(bytes)?;
        self.artifact_revision = self
            .artifact_revision
            .checked_add(1)
            .ok_or_else(|| "artifact revision is exhausted".to_string())?;
        self.artifact_hash = sha256(bytes);
        self.input_fingerprint = input_fingerprint;
        self.structure_evidence = None;
        self.visual_evidence = None;
        self.phase = ArtifactPhase::Generated;
        self.safe_error = None;
        self.updated_at = now;
        Ok(())
    }

    pub fn complete(&mut self, now: DateTime<Utc>) -> Result<(), String> {
        let current = |evidence: &Option<ArtifactValidationEvidence>| {
            evidence.as_ref().is_some_and(|evidence| {
                evidence.passed
                    && evidence.artifact_revision == self.artifact_revision
                    && evidence.artifact_hash == self.artifact_hash
                    && evidence.input_fingerprint == self.input_fingerprint
                    && evidence.template_hash == self.template.content_hash
            })
        };
        if !current(&self.structure_evidence) || !current(&self.visual_evidence) {
            return Err("artifact delivery checks are incomplete".to_string());
        }
        self.phase = ArtifactPhase::Completed;
        self.updated_at = now;
        Ok(())
    }
}

pub struct GeneratedArtifact {
    pub record: ArtifactRecord,
    pub bytes: Vec<u8>,
}

pub struct ArtifactEngine;

impl ArtifactEngine {
    pub fn generate_with_template(
        request: &ArtifactGenerationRequest,
        template: &ArtifactTemplate,
        now: DateTime<Utc>,
    ) -> Result<GeneratedArtifact, String> {
        template.validate()?;
        if request.template != template.reference
            || !template.supported_formats.contains(&request.input.format())
        {
            return Err("artifact template selection changed".to_string());
        }
        Self::generate(request, now)
    }

    pub fn generate(
        request: &ArtifactGenerationRequest,
        now: DateTime<Utc>,
    ) -> Result<GeneratedArtifact, String> {
        request.template.validate()?;
        validate_storage_ref(&request.approved_storage_ref)?;
        let input_fingerprint = request
            .input
            .fingerprint_for_template(&request.template.content_hash)?;
        let bytes = match &request.input {
            ArtifactInput::Office { spec } => build_office_artifact(spec)?,
            ArtifactInput::Pdf { title, paragraphs } => build_text_pdf(title, paragraphs)?,
        };
        validate_size(&bytes)?;
        Ok(GeneratedArtifact {
            record: ArtifactRecord {
                id: Uuid::new_v4(),
                request_id: request.request_id,
                format: request.input.format(),
                phase: ArtifactPhase::Generated,
                artifact_revision: 0,
                artifact_hash: sha256(&bytes),
                input_fingerprint,
                input: request.input.clone(),
                template: request.template.clone(),
                storage_ref: request.approved_storage_ref.clone(),
                structure_evidence: None,
                visual_evidence: None,
                revision_attempts: 0,
                safe_error: None,
                updated_at: now,
            },
            bytes,
        })
    }

    pub fn check_structure(
        record: &mut ArtifactRecord,
        bytes: &[u8],
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let outcome = (|| {
            validate_identity(record, bytes)?;
            Ok(match record.format {
                ArtifactFormat::Word => validate_zip_structure(
                    bytes,
                    &["[Content_Types].xml", "word/document.xml"],
                    "word/document.xml",
                    "<w:t",
                )?,
                ArtifactFormat::Excel => validate_zip_structure(
                    bytes,
                    &[
                        "[Content_Types].xml",
                        "xl/workbook.xml",
                        "xl/worksheets/sheet1.xml",
                    ],
                    "xl/worksheets/sheet1.xml",
                    "<row",
                )?,
                ArtifactFormat::PowerPoint => validate_zip_structure(
                    bytes,
                    &[
                        "[Content_Types].xml",
                        "ppt/presentation.xml",
                        "ppt/slides/slide1.xml",
                    ],
                    "ppt/slides/slide1.xml",
                    "<a:t",
                )?,
                ArtifactFormat::Pdf => validate_pdf_structure(bytes)?,
            })
        })();
        let checks = match outcome {
            Ok(checks) => checks,
            Err(error) => {
                record.structure_evidence = Some(evidence(
                    record,
                    ArtifactValidationKind::Structure,
                    "artifact-structure/v1",
                    false,
                    vec!["structure_check:failed".to_string()],
                    now,
                ));
                record.phase = ArtifactPhase::RevisionRequired;
                record.safe_error = Some("artifact structure needs revision".to_string());
                record.updated_at = now;
                return Err(error);
            }
        };
        record.structure_evidence = Some(evidence(
            record,
            ArtifactValidationKind::Structure,
            "artifact-structure/v1",
            true,
            checks,
            now,
        ));
        record.phase = ArtifactPhase::StructureChecked;
        record.updated_at = now;
        Ok(())
    }

    #[cfg(test)]
    pub fn check_visual(
        record: &mut ArtifactRecord,
        bytes: &[u8],
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        if let Err(error) = validate_identity(record, bytes) {
            record.visual_evidence = Some(evidence(
                record,
                ArtifactValidationKind::Visual,
                "artifact-equivalent-render/v1",
                false,
                vec!["visual_check:failed".to_string()],
                now,
            ));
            record.phase = ArtifactPhase::RevisionRequired;
            record.safe_error = Some("artifact visual check needs revision".to_string());
            record.updated_at = now;
            return Err(error);
        }
        if record.phase != ArtifactPhase::StructureChecked {
            return Err("artifact structure must pass before visual inspection".to_string());
        }
        let render = match render_visual(record.format, bytes) {
            Ok(render) => render,
            Err(error) => {
                record.visual_evidence = Some(evidence(
                    record,
                    ArtifactValidationKind::Visual,
                    "artifact-equivalent-render/v1",
                    false,
                    vec!["visual_check:failed".to_string()],
                    now,
                ));
                record.phase = ArtifactPhase::RevisionRequired;
                record.safe_error = Some("artifact visual check needs revision".to_string());
                record.updated_at = now;
                return Err(error);
            }
        };
        if render.non_blank_units == 0 || render.width == 0 || render.height == 0 {
            record.phase = ArtifactPhase::RevisionRequired;
            return Err("artifact visual inspection found an empty page".to_string());
        }
        record.visual_evidence = Some(evidence(
            record,
            ArtifactValidationKind::Visual,
            "artifact-equivalent-render/v1",
            true,
            vec![
                format!("rendered_canvas:{}x{}", render.width, render.height),
                format!("non_blank_units:{}", render.non_blank_units),
                format!("render_hash:{}", render.render_hash),
            ],
            now,
        ));
        record.phase = ArtifactPhase::ReadyForDelivery;
        record.updated_at = now;
        Ok(())
    }

    pub fn check_actual_visual(
        record: &mut ArtifactRecord,
        pages: &[Vec<u8>],
        renderer_version: &str,
        preview_ref: String,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        if record.phase != ArtifactPhase::StructureChecked
            || pages.is_empty()
            || pages.len() > 64
            || renderer_version.trim().is_empty()
            || !preview_ref.starts_with("artifact-preview:")
            || pages
                .iter()
                .any(|page| !page.starts_with(b"\x89PNG\r\n\x1a\n"))
        {
            return Err("actual visual evidence is invalid".to_string());
        }
        let mut non_blank_pages = 0usize;
        for page in pages {
            let image = match image::load_from_memory_with_format(page, ImageFormat::Png) {
                Ok(image) => image.to_luma8(),
                Err(_) => {
                    return record_actual_visual_failure(
                        record,
                        renderer_version,
                        "preview_decode_failed",
                        now,
                    )
                }
            };
            if image.width() < 100 || image.height() < 100 {
                return record_actual_visual_failure(
                    record,
                    renderer_version,
                    "preview_dimensions_invalid",
                    now,
                );
            }
            let dark_pixels = image.pixels().filter(|pixel| pixel.0[0] < 250).count();
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
            let edge_area =
                usize::try_from(2 * margin_x * image.height() + 2 * margin_y * image.width())
                    .unwrap_or(usize::MAX);
            if edge_dark_pixels > 25 && edge_dark_pixels.saturating_mul(20) > edge_area {
                return record_actual_visual_failure(
                    record,
                    renderer_version,
                    "possible_edge_clipping_detected",
                    now,
                );
            }
            if dark_pixels > 25 {
                non_blank_pages += 1;
            }
        }
        if non_blank_pages != pages.len() {
            return record_actual_visual_failure(
                record,
                renderer_version,
                "blank_page_detected",
                now,
            );
        }
        let preview_manifest_hash = preview_manifest_hash(pages);
        record.visual_evidence = Some(ArtifactValidationEvidence {
            id: Uuid::new_v4(),
            kind: ArtifactValidationKind::Visual,
            artifact_revision: record.artifact_revision,
            artifact_hash: record.artifact_hash.clone(),
            input_fingerprint: record.input_fingerprint.clone(),
            template_hash: record.template.content_hash.clone(),
            validator_version: renderer_version.to_string(),
            passed: true,
            checks: vec![
                "actual_office_or_pdf_render".to_string(),
                format!("rendered_pages:{}", pages.len()),
                format!("non_blank_pages:{non_blank_pages}"),
                format!("preview_manifest_hash:{preview_manifest_hash}"),
            ],
            preview_ref: Some(preview_ref),
            rendered_page_count: u32::try_from(pages.len())
                .map_err(|_| "actual visual page count is invalid".to_string())?,
            preview_manifest_hash: Some(preview_manifest_hash),
            created_at: now,
        });
        record.phase = ArtifactPhase::ReadyForDelivery;
        record.updated_at = now;
        Ok(())
    }
}

fn record_actual_visual_failure(
    record: &mut ArtifactRecord,
    renderer_version: &str,
    check: &str,
    now: DateTime<Utc>,
) -> Result<(), String> {
    record.visual_evidence = Some(ArtifactValidationEvidence {
        id: Uuid::new_v4(),
        kind: ArtifactValidationKind::Visual,
        artifact_revision: record.artifact_revision,
        artifact_hash: record.artifact_hash.clone(),
        input_fingerprint: record.input_fingerprint.clone(),
        template_hash: record.template.content_hash.clone(),
        validator_version: renderer_version.to_string(),
        passed: false,
        checks: vec![check.to_string()],
        preview_ref: None,
        rendered_page_count: 0,
        preview_manifest_hash: None,
        created_at: now,
    });
    record.phase = ArtifactPhase::RevisionRequired;
    record.safe_error = Some("actual_visual_validation_failed".to_string());
    record.updated_at = now;
    Err("actual visual validation failed".to_string())
}

#[cfg(test)]
pub trait ArtifactByteStore: Send + Sync {
    fn read_verified(&self, storage_ref: &str, expected_hash: &str) -> Result<Vec<u8>, String>;
    fn write_revision_if_authorized(
        &self,
        storage_ref: &str,
        expected_hash: &str,
        idempotency_key: &str,
        revised: &[u8],
    ) -> Result<String, String>;
}

#[cfg(test)]
pub struct ArtifactRevision {
    pub bytes: Vec<u8>,
}

#[cfg(test)]
pub struct ArtifactRevisionRequest {
    pub input: ArtifactInput,
    pub format: ArtifactFormat,
    pub artifact_revision: u32,
    pub revision_attempts: u32,
    pub safe_error: Option<String>,
}

#[cfg(test)]
pub trait ArtifactRevisionProvider: Send + Sync {
    fn revise(&self, request: &ArtifactRevisionRequest) -> Result<ArtifactRevision, String>;
}

#[derive(Default, Debug, Eq, PartialEq)]
#[cfg(test)]
pub struct ArtifactWorkerSweep {
    pub inspected: usize,
    pub completed: usize,
    pub waiting_revision: usize,
    pub failed: usize,
}

#[cfg(test)]
pub fn run_artifact_worker(
    store: &EventStore,
    byte_store: &dyn ArtifactByteStore,
    revision_provider: &dyn ArtifactRevisionProvider,
    limit: usize,
    now: DateTime<Utc>,
) -> Result<ArtifactWorkerSweep, String> {
    let due = store
        .recoverable_artifact_records(limit, now)
        .map_err(|_| "artifact recovery state is unavailable".to_string())?;
    let mut sweep = ArtifactWorkerSweep::default();
    for (mut record, mut row_revision) in due {
        sweep.inspected += 1;
        let outcome = (|| -> Result<(), String> {
            for _ in 0..8 {
                let bytes = byte_store.read_verified(&record.storage_ref, &record.artifact_hash)?;
                match record.phase {
                    ArtifactPhase::Generated => {
                        let _ = ArtifactEngine::check_structure(&mut record, &bytes, now);
                    }
                    ArtifactPhase::StructureChecked => {
                        let _ = ArtifactEngine::check_visual(&mut record, &bytes, now);
                    }
                    ArtifactPhase::ReadyForDelivery | ArtifactPhase::VisualChecked => {
                        record.complete(now)?;
                    }
                    ArtifactPhase::RevisionRequired => {
                        if let Err(error) = record.request_revision(now) {
                            row_revision = store
                                .update_artifact_record(&record, row_revision)
                                .map_err(|_| {
                                    "artifact terminal revision state could not be persisted"
                                        .to_string()
                                })?;
                            return Err(error);
                        }
                    }
                    ArtifactPhase::RevisionPrepared => {
                        let revision = revision_provider.revise(&ArtifactRevisionRequest {
                            input: record.input.clone(),
                            format: record.format,
                            artifact_revision: record.artifact_revision,
                            revision_attempts: record.revision_attempts,
                            safe_error: record.safe_error.clone(),
                        })?;
                        validate_size(&revision.bytes)?;
                        let revision_key = sha256(format!(
                            "ds-agent.artifact-revision.v1\0{}\0{}\0{}\0{}\0{}",
                            record.id,
                            record.artifact_revision,
                            record.revision_attempts,
                            record.artifact_hash,
                            record.input_fingerprint,
                        ));
                        let revised_storage_ref = byte_store.write_revision_if_authorized(
                            &record.storage_ref,
                            &record.artifact_hash,
                            &revision_key,
                            &revision.bytes,
                        )?;
                        validate_storage_ref(&revised_storage_ref)?;
                        record.storage_ref = revised_storage_ref;
                        let input_fingerprint = record.input_fingerprint.clone();
                        record.replace_revision(&revision.bytes, input_fingerprint, now)?;
                    }
                    ArtifactPhase::Completed | ArtifactPhase::Failed => break,
                }
                row_revision = store
                    .update_artifact_record(&record, row_revision)
                    .map_err(|_| "artifact state changed during validation".to_string())?;
                if matches!(
                    record.phase,
                    ArtifactPhase::Completed | ArtifactPhase::Failed
                ) {
                    break;
                }
            }
            Ok(())
        })();
        match (outcome, record.phase) {
            (Ok(()), ArtifactPhase::Completed) => sweep.completed += 1,
            (Ok(()), ArtifactPhase::RevisionRequired | ArtifactPhase::RevisionPrepared) => {
                sweep.waiting_revision += 1
            }
            (Err(_), ArtifactPhase::RevisionRequired | ArtifactPhase::RevisionPrepared) => {
                sweep.waiting_revision += 1
            }
            (Err(_), _) | (Ok(()), ArtifactPhase::Failed) => sweep.failed += 1,
            _ => {}
        }
    }
    Ok(sweep)
}

#[cfg(test)]
struct VisualRender {
    width: u32,
    height: u32,
    non_blank_units: usize,
    render_hash: String,
}

#[cfg(test)]
fn render_visual(format: ArtifactFormat, bytes: &[u8]) -> Result<VisualRender, String> {
    let (width, height, content) = match format {
        ArtifactFormat::Word => (816, 1056, zip_text(bytes, "word/document.xml")?),
        ArtifactFormat::Excel => (1280, 720, zip_text(bytes, "xl/worksheets/sheet1.xml")?),
        ArtifactFormat::PowerPoint => (1280, 720, zip_text(bytes, "ppt/slides/slide1.xml")?),
        ArtifactFormat::Pdf => (816, 1056, String::from_utf8_lossy(bytes).into_owned()),
    };
    let visible = visible_render_units(&content);
    let mut canvas = GrayImage::from_pixel(320, 240, Luma([255]));
    for (index, unit) in visible.iter().take(22).enumerate() {
        let y = 12 + u32::try_from(index).unwrap_or(0) * 10;
        let bar_width = u32::try_from(unit.chars().count())
            .unwrap_or(0)
            .clamp(4, 280);
        for x in 12..(12 + bar_width) {
            if x < canvas.width() && y < canvas.height() {
                canvas.put_pixel(x, y, Luma([32]));
            }
        }
    }
    if format == ArtifactFormat::Excel {
        for x in (8..312).step_by(40) {
            for y in 8..232 {
                canvas.put_pixel(x, y, Luma([180]));
            }
        }
        for y in (8..232).step_by(18) {
            for x in 8..312 {
                canvas.put_pixel(x, y, Luma([180]));
            }
        }
    }
    let non_blank_units = canvas.pixels().filter(|pixel| pixel.0[0] < 250).count();
    let render_hash = sha256(canvas.as_raw());
    Ok(VisualRender {
        width,
        height,
        non_blank_units,
        render_hash,
    })
}

fn visible_render_units(content: &str) -> Vec<String> {
    let mut units = Vec::new();
    let mut current = String::new();
    let mut in_tag = false;
    for character in content.chars() {
        match character {
            '<' => {
                if !current.trim().is_empty() {
                    units.push(current.trim().to_string());
                }
                current.clear();
                in_tag = true;
            }
            '>' => in_tag = false,
            '(' if !in_tag => {
                if !current.trim().is_empty() {
                    units.push(current.trim().to_string());
                }
                current.clear();
            }
            ')' if !in_tag => {
                if !current.trim().is_empty() {
                    units.push(current.trim().to_string());
                }
                current.clear();
            }
            value if !in_tag && !value.is_control() => current.push(value),
            _ => {}
        }
    }
    if !current.trim().is_empty() {
        units.push(current.trim().to_string());
    }
    units
}

fn validate_zip_structure(
    bytes: &[u8],
    required: &[&str],
    content_part: &str,
    completion_marker: &str,
) -> Result<Vec<String>, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "artifact package cannot be opened".to_string())?;
    let mut checks = Vec::new();
    for part in required {
        archive
            .by_name(part)
            .map_err(|_| "artifact package is missing a required part".to_string())?;
        checks.push(format!("required_part:{part}"));
    }
    let xml = zip_text(bytes, content_part)?;
    if !xml.contains(completion_marker) || visible_render_units(&xml).is_empty() {
        return Err("artifact required content structure is incomplete".to_string());
    }
    let mut reader = quick_xml::Reader::from_str(&xml);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err("artifact XML structure is damaged".to_string()),
        }
    }
    checks.push("xml_parse:passed".to_string());
    Ok(checks)
}

fn zip_text(bytes: &[u8], name: &str) -> Result<String, String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "artifact package cannot be opened".to_string())?;
    let mut file = archive
        .by_name(name)
        .map_err(|_| "artifact render source is unavailable".to_string())?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|_| "artifact render source is invalid".to_string())?;
    Ok(text)
}

fn validate_pdf_structure(bytes: &[u8]) -> Result<Vec<String>, String> {
    let text = String::from_utf8_lossy(bytes);
    if !bytes.starts_with(b"%PDF-")
        || !text.trim_end().ends_with("%%EOF")
        || !text.contains("xref")
        || !text.contains("/Type /Page")
        || !text.contains(" Tj")
    {
        return Err("PDF structure is damaged".to_string());
    }
    Ok(vec![
        "pdf_header:passed".to_string(),
        "pdf_xref:passed".to_string(),
        "pdf_page_tree:passed".to_string(),
    ])
}

fn evidence(
    record: &ArtifactRecord,
    kind: ArtifactValidationKind,
    version: &str,
    passed: bool,
    checks: Vec<String>,
    now: DateTime<Utc>,
) -> ArtifactValidationEvidence {
    ArtifactValidationEvidence {
        id: Uuid::new_v4(),
        kind,
        artifact_revision: record.artifact_revision,
        artifact_hash: record.artifact_hash.clone(),
        input_fingerprint: record.input_fingerprint.clone(),
        template_hash: record.template.content_hash.clone(),
        validator_version: version.to_string(),
        passed,
        checks,
        preview_ref: None,
        rendered_page_count: 0,
        preview_manifest_hash: None,
        created_at: now,
    }
}

fn validate_identity(record: &ArtifactRecord, bytes: &[u8]) -> Result<(), String> {
    validate_size(bytes)?;
    if sha256(bytes) != record.artifact_hash {
        return Err("artifact content identity changed".to_string());
    }
    record.template.validate()
}

fn validate_size(bytes: &[u8]) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > MAX_ARTIFACT_BYTES {
        return Err("artifact size is invalid".to_string());
    }
    Ok(())
}

fn validate_storage_ref(value: &str) -> Result<(), String> {
    if !value.starts_with("artifact-storage:")
        || value.len() > 160
        || value.chars().any(char::is_control)
    {
        return Err("artifact storage approval is invalid".to_string());
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn sha256(value: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(value.as_ref()))
}

pub(crate) fn preview_manifest_hash(pages: &[Vec<u8>]) -> String {
    let mut manifest = Vec::new();
    manifest.extend_from_slice(&(pages.len() as u64).to_be_bytes());
    for page in pages {
        manifest.extend_from_slice(&(page.len() as u64).to_be_bytes());
        manifest.extend_from_slice(&Sha256::digest(page));
    }
    sha256(manifest)
}

fn build_text_pdf(title: &str, paragraphs: &[String]) -> Result<Vec<u8>, String> {
    if title.trim().is_empty() || paragraphs.is_empty() {
        return Err("PDF content is incomplete".to_string());
    }
    let text = std::iter::once(title)
        .chain(paragraphs.iter().map(String::as_str))
        .map(|line| line.replace(['\\', '(', ')'], " "))
        .collect::<Vec<_>>();
    let mut stream = String::from("BT /F1 16 Tf 72 760 Td ");
    for (index, line) in text.iter().enumerate() {
        if index > 0 {
            stream.push_str("0 -24 Td ");
        }
        stream.push_str(&format!("({line}) Tj "));
    }
    stream.push_str("ET");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", stream.len(), stream),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, object));
    }
    let xref = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1,
    ));
    Ok(pdf.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::office::OfficeSlideSpec;
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn template() -> ArtifactTemplateRef {
        ArtifactTemplateRef {
            template_id: "default.office".to_string(),
            version: 1,
            content_hash: "a".repeat(64),
        }
    }

    fn request(format: ArtifactFormat) -> ArtifactGenerationRequest {
        let input = match format {
            ArtifactFormat::Word => ArtifactInput::Office {
                spec: OfficeCreateSpec {
                    app: OfficeApp::Word,
                    path: "report.docx".to_string(),
                    title: "Report".to_string(),
                    body: "Verified body".to_string(),
                    rows: vec![],
                    slides: vec![],
                },
            },
            ArtifactFormat::Excel => ArtifactInput::Office {
                spec: OfficeCreateSpec {
                    app: OfficeApp::Excel,
                    path: "report.xlsx".to_string(),
                    title: "Report".to_string(),
                    body: String::new(),
                    rows: vec![
                        vec!["Metric".to_string(), "Value".to_string()],
                        vec!["Revenue".to_string(), "100".to_string()],
                    ],
                    slides: vec![],
                },
            },
            ArtifactFormat::PowerPoint => ArtifactInput::Office {
                spec: OfficeCreateSpec {
                    app: OfficeApp::PowerPoint,
                    path: "report.pptx".to_string(),
                    title: "Report".to_string(),
                    body: String::new(),
                    rows: vec![],
                    slides: vec![OfficeSlideSpec {
                        title: "Summary".to_string(),
                        body: "Verified slide".to_string(),
                    }],
                },
            },
            ArtifactFormat::Pdf => ArtifactInput::Pdf {
                title: "Report".to_string(),
                paragraphs: vec!["Verified PDF body".to_string()],
            },
        };
        ArtifactGenerationRequest {
            request_id: Uuid::new_v4(),
            input,
            template: template(),
            approved_storage_ref: format!("artifact-storage:{}", Uuid::new_v4()),
        }
    }

    #[test]
    fn four_formats_generate_structure_render_and_complete() {
        for format in [
            ArtifactFormat::Word,
            ArtifactFormat::Excel,
            ArtifactFormat::PowerPoint,
            ArtifactFormat::Pdf,
        ] {
            let now = Utc::now();
            let mut generated = ArtifactEngine::generate(&request(format), now).unwrap();
            ArtifactEngine::check_structure(&mut generated.record, &generated.bytes, now).unwrap();
            ArtifactEngine::check_visual(&mut generated.record, &generated.bytes, now).unwrap();
            generated.record.complete(now).unwrap();
            assert_eq!(
                generated.record.phase,
                ArtifactPhase::Completed,
                "{format:?}"
            );
            assert!(generated
                .record
                .visual_evidence
                .as_ref()
                .unwrap()
                .checks
                .iter()
                .any(|check| check.starts_with("render_hash:")));
        }
    }

    #[test]
    fn public_delivery_view_redacts_storage_hashes_and_internal_evidence() {
        let generated =
            ArtifactEngine::generate(&request(ArtifactFormat::Word), Utc::now()).unwrap();
        let serialized = serde_json::to_string(&generated.record.public_view()).unwrap();
        for private in [
            generated.record.storage_ref.as_str(),
            generated.record.artifact_hash.as_str(),
            generated.record.input_fingerprint.as_str(),
            generated.record.template.content_hash.as_str(),
            "validator_version",
            "render_hash",
        ] {
            assert!(!serialized.contains(private), "{private}");
        }
    }

    #[test]
    fn corruption_identity_and_unverified_completion_fail_closed() {
        let now = Utc::now();
        let mut generated = ArtifactEngine::generate(&request(ArtifactFormat::Word), now).unwrap();
        assert!(generated.record.complete(now).is_err());
        generated.bytes.truncate(24);
        assert!(
            ArtifactEngine::check_structure(&mut generated.record, &generated.bytes, now,).is_err()
        );
    }

    #[test]
    fn revision_is_bounded_and_invalidates_old_evidence() {
        let now = Utc::now();
        let mut generated = ArtifactEngine::generate(&request(ArtifactFormat::Pdf), now).unwrap();
        ArtifactEngine::check_structure(&mut generated.record, &generated.bytes, now).unwrap();
        ArtifactEngine::check_visual(&mut generated.record, &generated.bytes, now).unwrap();
        for attempt in 0..MAX_ARTIFACT_REVISIONS {
            generated.record.request_revision(now).unwrap();
            let bytes = build_text_pdf("Report", &[format!("revision {attempt}")]).unwrap();
            generated
                .record
                .replace_revision(&bytes, sha256(format!("input-{attempt}")), now)
                .unwrap();
            generated.bytes = bytes;
        }
        assert!(generated.record.request_revision(now).is_err());
        assert_eq!(generated.record.phase, ArtifactPhase::Failed);
        assert!(generated.record.complete(now).is_err());
    }

    struct MemoryArtifactStore {
        bytes: Mutex<HashMap<String, Vec<u8>>>,
        revision_keys: Mutex<HashMap<String, String>>,
        revision_authorized: bool,
    }

    impl ArtifactByteStore for MemoryArtifactStore {
        fn read_verified(&self, storage_ref: &str, expected_hash: &str) -> Result<Vec<u8>, String> {
            let bytes = self
                .bytes
                .lock()
                .unwrap()
                .get(storage_ref)
                .cloned()
                .ok_or_else(|| "artifact bytes unavailable".to_string())?;
            if sha256(&bytes) != expected_hash {
                return Err("artifact bytes changed".to_string());
            }
            Ok(bytes)
        }

        fn write_revision_if_authorized(
            &self,
            _storage_ref: &str,
            _expected_hash: &str,
            idempotency_key: &str,
            revised: &[u8],
        ) -> Result<String, String> {
            if !self.revision_authorized {
                return Err("artifact revision write needs approval".to_string());
            }
            if let Some(storage_ref) = self.revision_keys.lock().unwrap().get(idempotency_key) {
                return Ok(storage_ref.clone());
            }
            let storage_ref = format!("artifact-storage:{}", Uuid::new_v4());
            self.bytes
                .lock()
                .unwrap()
                .insert(storage_ref.clone(), revised.to_vec());
            self.revision_keys
                .lock()
                .unwrap()
                .insert(idempotency_key.to_string(), storage_ref.clone());
            Ok(storage_ref)
        }
    }

    struct FixedRevisionProvider {
        bytes: Vec<u8>,
    }

    impl ArtifactRevisionProvider for FixedRevisionProvider {
        fn revise(&self, _request: &ArtifactRevisionRequest) -> Result<ArtifactRevision, String> {
            Ok(ArtifactRevision {
                bytes: self.bytes.clone(),
            })
        }
    }

    #[test]
    fn durable_worker_completes_all_four_fixture_formats_after_restart() {
        for format in [
            ArtifactFormat::Word,
            ArtifactFormat::Excel,
            ArtifactFormat::PowerPoint,
            ArtifactFormat::Pdf,
        ] {
            let now = Utc::now();
            let generated = ArtifactEngine::generate(&request(format), now).unwrap();
            let storage = MemoryArtifactStore {
                bytes: Mutex::new(HashMap::from([(
                    generated.record.storage_ref.clone(),
                    generated.bytes.clone(),
                )])),
                revision_keys: Mutex::new(HashMap::new()),
                revision_authorized: false,
            };
            let temp = tempfile::tempdir().unwrap();
            let database = temp.path().join("artifact-worker.sqlite3");
            {
                let store = EventStore::open(&database).unwrap();
                store.insert_artifact_record(&generated.record).unwrap();
            }
            let reopened = EventStore::open(&database).unwrap();
            let sweep = run_artifact_worker(
                &reopened,
                &storage,
                &FixedRevisionProvider {
                    bytes: generated.bytes.clone(),
                },
                4,
                now,
            )
            .unwrap();
            assert_eq!(sweep.completed, 1, "{format:?}");
            assert_eq!(
                reopened
                    .artifact_record(generated.record.id)
                    .unwrap()
                    .0
                    .phase,
                ArtifactPhase::Completed
            );
        }
    }

    #[test]
    fn revision_provider_cannot_overwrite_without_storage_authority() {
        let now = Utc::now();
        let valid = ArtifactEngine::generate(&request(ArtifactFormat::Word), now).unwrap();
        let mut damaged_record = valid.record.clone();
        let damaged = b"damaged office package".to_vec();
        damaged_record.artifact_hash = sha256(&damaged);
        let storage = MemoryArtifactStore {
            bytes: Mutex::new(HashMap::from([(
                damaged_record.storage_ref.clone(),
                damaged,
            )])),
            revision_keys: Mutex::new(HashMap::new()),
            revision_authorized: false,
        };
        let store = EventStore::open_memory().unwrap();
        store.insert_artifact_record(&damaged_record).unwrap();
        let sweep = run_artifact_worker(
            &store,
            &storage,
            &FixedRevisionProvider { bytes: valid.bytes },
            1,
            now,
        )
        .unwrap();
        assert_eq!(sweep.completed, 0);
        assert_eq!(sweep.waiting_revision, 1);
        assert_eq!(
            store.artifact_record(damaged_record.id).unwrap().0.phase,
            ArtifactPhase::RevisionPrepared
        );
    }

    #[test]
    fn authorized_revision_is_bounded_idempotent_and_revalidated_before_completion() {
        let now = Utc::now();
        let valid = ArtifactEngine::generate(&request(ArtifactFormat::Word), now).unwrap();
        let mut damaged_record = valid.record.clone();
        let damaged = b"damaged office package".to_vec();
        damaged_record.artifact_hash = sha256(&damaged);
        let storage = MemoryArtifactStore {
            bytes: Mutex::new(HashMap::from([(
                damaged_record.storage_ref.clone(),
                damaged,
            )])),
            revision_keys: Mutex::new(HashMap::new()),
            revision_authorized: true,
        };
        let store = EventStore::open_memory().unwrap();
        store.insert_artifact_record(&damaged_record).unwrap();
        let sweep = run_artifact_worker(
            &store,
            &storage,
            &FixedRevisionProvider { bytes: valid.bytes },
            1,
            now,
        )
        .unwrap();
        assert_eq!(sweep.completed, 1);
        let completed = store.artifact_record(damaged_record.id).unwrap().0;
        assert_eq!(completed.phase, ArtifactPhase::Completed);
        assert_eq!(completed.revision_attempts, 1);
        assert_eq!(completed.artifact_revision, 1);
        assert!(completed.structure_evidence.unwrap().passed);
        assert!(completed.visual_evidence.unwrap().passed);
        assert_eq!(storage.revision_keys.lock().unwrap().len(), 1);
    }
}
