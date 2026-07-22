use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use zip::write::FileOptions;

use crate::kernel::capability::{CapabilityInvocation, CapabilityInvocationStatus};
use crate::kernel::models::AccessMode;
#[cfg(test)]
use crate::kernel::policy::CapabilityAccessStatus;
use crate::kernel::policy::{
    request_capability_access, CapabilityAccessRequest, CapabilityKind, PolicyDecision,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OfficeApp {
    Word,
    Excel,
    PowerPoint,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeSlideSpec {
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeCreateSpec {
    pub app: OfficeApp,
    pub path: String,
    pub title: String,
    pub body: String,
    pub rows: Vec<Vec<String>>,
    pub slides: Vec<OfficeSlideSpec>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeUpdateSpec {
    pub app: OfficeApp,
    pub path: String,
    pub body: String,
    pub rows: Vec<Vec<String>>,
    pub slides: Vec<OfficeSlideSpec>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct OfficeCreateModelContent {
    #[serde(default, alias = "kind", alias = "type")]
    app: Option<String>,
    #[serde(
        default,
        alias = "file_path",
        alias = "relative_path",
        alias = "target"
    )]
    path: Option<String>,
    #[serde(
        default,
        alias = "targetLocation",
        alias = "location",
        alias = "destination"
    )]
    target_location: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default, alias = "content", alias = "text")]
    body: Option<String>,
    #[serde(default)]
    rows: Vec<Vec<Value>>,
    #[serde(default)]
    sheets: Vec<OfficeCreateSheetContent>,
    #[serde(default)]
    slides: Vec<OfficeCreateSlideContent>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct OfficeCreateSheetContent {
    #[serde(default)]
    rows: Vec<Vec<Value>>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct OfficeCreateSlideContent {
    #[serde(default)]
    title: String,
    #[serde(default, alias = "content", alias = "text")]
    body: String,
}

#[cfg(test)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeCreateRequest {
    pub access_mode: AccessMode,
    pub spec: OfficeCreateSpec,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeCreateResult {
    pub path: String,
    pub bytes: u64,
    pub app: OfficeApp,
    pub artifact_kind: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeOpenRequest {
    pub access_mode: AccessMode,
    pub path: String,
    pub preferred_app: Option<OfficeApp>,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeOpenResult {
    pub path: String,
    pub app: OfficeApp,
    pub opener_label: String,
    pub fallback_note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeUpdateRequest {
    pub access_mode: AccessMode,
    pub spec: OfficeUpdateSpec,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeUpdateResult {
    pub path: String,
    pub bytes: u64,
    pub app: OfficeApp,
    pub artifact_kind: String,
    pub summary: String,
}

#[cfg(test)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeCreateOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
    pub result: Option<OfficeCreateResult>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeOpenOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
    pub result: Option<OfficeOpenResult>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OfficeUpdateOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
    pub result: Option<OfficeUpdateResult>,
}

pub trait OfficeArtifactClient {
    fn write_office_artifact(&self, spec: &OfficeCreateSpec) -> Result<OfficeCreateResult, String>;
    fn verify_written_artifact_path(
        &self,
        spec: &OfficeCreateSpec,
        result: &OfficeCreateResult,
    ) -> Result<(), String>;
}

pub trait OfficeOpenClient {
    fn open_office_artifact(
        &self,
        path: &str,
        preferred_app: Option<OfficeApp>,
    ) -> Result<OfficeOpenResult, String>;
}

pub trait OfficeUpdateClient {
    fn update_office_artifact(&self, spec: &OfficeUpdateSpec)
        -> Result<OfficeUpdateResult, String>;
    fn verify_updated_artifact_path(
        &self,
        spec: &OfficeUpdateSpec,
        result: &OfficeUpdateResult,
    ) -> Result<(), String>;
}

pub fn office_create_spec_from_action(
    action_type: &str,
    target: Option<&str>,
    target_location: Option<&str>,
    title: Option<&str>,
    reason: Option<&str>,
    content: Option<&str>,
) -> Result<OfficeCreateSpec, String> {
    let parsed = parse_model_content(content);
    let app = parsed
        .as_ref()
        .and_then(|model| model.app.as_deref())
        .and_then(OfficeApp::from_label)
        .or_else(|| OfficeApp::from_label(action_type))
        .or_else(|| target.and_then(OfficeApp::from_path))
        .or_else(|| title.and_then(OfficeApp::from_label))
        .or_else(|| reason.and_then(OfficeApp::from_label))
        .unwrap_or(OfficeApp::Word);
    let title = parsed
        .as_ref()
        .and_then(|model| model.title.as_deref())
        .or(title)
        .or(reason)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(app.default_title())
        .to_string();
    let body = parsed
        .as_ref()
        .and_then(|model| model.body.as_deref())
        .or_else(|| plain_text_content(content))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();
    let rows = parsed
        .as_ref()
        .map(model_rows)
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| default_rows_for(app, &title, &body));
    let slides = parsed
        .as_ref()
        .map(model_slides)
        .filter(|slides| !slides.is_empty())
        .unwrap_or_else(|| default_slides_for(app, &title, &body));
    let path = parsed
        .as_ref()
        .and_then(|model| model.path.as_deref())
        .or(target)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| default_office_path(app, &title));
    let path = ensure_office_extension(&path, app)?;
    let path = apply_office_target_location(
        &path,
        parsed
            .as_ref()
            .and_then(|model| model.target_location.as_deref())
            .or(target_location),
    );

    Ok(OfficeCreateSpec {
        app,
        path,
        title,
        body,
        rows,
        slides,
    })
}

pub fn office_update_spec_from_action(
    action_type: &str,
    target: Option<&str>,
    target_location: Option<&str>,
    title: Option<&str>,
    reason: Option<&str>,
    content: Option<&str>,
) -> Result<OfficeUpdateSpec, String> {
    let parsed = parse_model_content(content);
    let app = parsed
        .as_ref()
        .and_then(|model| model.app.as_deref())
        .and_then(OfficeApp::from_label)
        .or_else(|| OfficeApp::from_label(action_type))
        .or_else(|| target.and_then(OfficeApp::from_path))
        .or_else(|| title.and_then(OfficeApp::from_label))
        .or_else(|| reason.and_then(OfficeApp::from_label))
        .ok_or_else(|| {
            "office_update target or content must identify Word, Excel, or PowerPoint".to_string()
        })?;
    let path = parsed
        .as_ref()
        .and_then(|model| model.path.as_deref())
        .or(target)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "office_update target is required before dispatch".to_string())?;
    let path = ensure_office_extension(path, app)?;
    let path = apply_office_target_location(
        &path,
        parsed
            .as_ref()
            .and_then(|model| model.target_location.as_deref())
            .or(target_location),
    );
    let body = parsed
        .as_ref()
        .and_then(|model| model.body.as_deref())
        .or_else(|| plain_text_content(content))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();
    let rows = parsed.as_ref().map(model_rows).unwrap_or_default();
    let slides = parsed.as_ref().map(model_slides).unwrap_or_default();

    Ok(OfficeUpdateSpec {
        app,
        path,
        body,
        rows,
        slides,
    })
}

pub struct LocalOfficeArtifactClient {
    workspace_dir: PathBuf,
    desktop_dir: Option<PathBuf>,
    max_bytes: usize,
}

impl LocalOfficeArtifactClient {
    pub fn new_with_desktop_dir(
        workspace_dir: PathBuf,
        max_bytes: usize,
        desktop_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            workspace_dir,
            desktop_dir,
            max_bytes,
        }
    }
}

impl OfficeArtifactClient for LocalOfficeArtifactClient {
    fn write_office_artifact(&self, spec: &OfficeCreateSpec) -> Result<OfficeCreateResult, String> {
        let bytes = build_office_artifact(spec)?;
        if bytes.len() > self.max_bytes {
            return Err(format!(
                "office artifact is too large: {} bytes exceeds {} bytes",
                bytes.len(),
                self.max_bytes
            ));
        }

        std::fs::create_dir_all(&self.workspace_dir)
            .map_err(|error| format!("office workspace directory could not be created: {error}"))?;
        let ResolvedOfficeArtifactPath {
            path: output_path,
            boundary_dir,
        } = resolve_office_artifact_path(
            &self.workspace_dir,
            self.desktop_dir.as_deref(),
            &spec.path,
        )?;
        let parent = output_path
            .parent()
            .ok_or_else(|| "office artifact parent directory is invalid".to_string())?;
        std::fs::create_dir_all(parent).map_err(|error| {
            format!("office artifact parent directory could not be created: {error}")
        })?;
        std::fs::create_dir_all(&boundary_dir).map_err(|error| {
            format!("office artifact boundary directory could not be created: {error}")
        })?;
        let workspace = canonical_boundary_dir(&boundary_dir)?;
        let canonical_parent = parent.canonicalize().map_err(|error| {
            format!("office artifact parent directory could not be resolved: {error}")
        })?;
        if !canonical_parent.starts_with(&workspace) {
            return Err(
                "office artifact path must stay inside the configured workspace or desktop"
                    .to_string(),
            );
        }
        std::fs::write(&output_path, &bytes)
            .map_err(|error| format!("office artifact could not be written: {error}"))?;

        Ok(OfficeCreateResult {
            path: output_path.to_string_lossy().to_string(),
            bytes: bytes.len() as u64,
            app: spec.app,
            artifact_kind: spec.app.artifact_kind().to_string(),
        })
    }

    fn verify_written_artifact_path(
        &self,
        spec: &OfficeCreateSpec,
        result: &OfficeCreateResult,
    ) -> Result<(), String> {
        let expected = resolve_office_artifact_path(
            &self.workspace_dir,
            self.desktop_dir.as_deref(),
            &spec.path,
        )?
        .path
        .canonicalize()
        .map_err(|_| "approved office artifact path could not be resolved".to_string())?;
        let actual = Path::new(&result.path)
            .canonicalize()
            .map_err(|_| "written office artifact path could not be resolved".to_string())?;
        if actual != expected {
            return Err(
                "written office artifact path changed from the approved target".to_string(),
            );
        }
        Ok(())
    }
}

impl OfficeOpenClient for LocalOfficeArtifactClient {
    fn open_office_artifact(
        &self,
        path: &str,
        preferred_app: Option<OfficeApp>,
    ) -> Result<OfficeOpenResult, String> {
        let path = path.trim().replace('\\', "/");
        let app = OfficeApp::from_path(&path)
            .or(preferred_app)
            .ok_or_else(|| {
                "office_open target must be a .docx, .xlsx, or .pptx file".to_string()
            })?;
        let path = ensure_office_extension(&path, app)?;
        let ResolvedOfficeArtifactPath {
            path: output_path,
            boundary_dir,
        } = resolve_office_artifact_path(&self.workspace_dir, self.desktop_dir.as_deref(), &path)?;
        if !output_path.is_file() {
            return Err(format!("office file does not exist: {path}"));
        }

        let workspace = canonical_boundary_dir(&boundary_dir)?;
        let canonical_path = output_path
            .canonicalize()
            .map_err(|error| format!("office file path could not be resolved: {error}"))?;
        if !canonical_path.starts_with(&workspace) {
            return Err(
                "office file path must stay inside the configured workspace or desktop".to_string(),
            );
        }

        let launch_path = office_shell_compatible_path(&canonical_path);
        open_office_file(&launch_path, app, preferred_app).map(|mut result| {
            result.path = launch_path.to_string_lossy().to_string();
            result
        })
    }
}

impl OfficeUpdateClient for LocalOfficeArtifactClient {
    fn update_office_artifact(
        &self,
        spec: &OfficeUpdateSpec,
    ) -> Result<OfficeUpdateResult, String> {
        validate_office_update_spec(spec)?;
        let ResolvedOfficeArtifactPath {
            path: output_path,
            boundary_dir,
        } = resolve_office_artifact_path(
            &self.workspace_dir,
            self.desktop_dir.as_deref(),
            &spec.path,
        )?;
        if !output_path.is_file() {
            return Err(format!("office file does not exist: {}", spec.path));
        }

        let workspace = canonical_boundary_dir(&boundary_dir)?;
        let canonical_path = output_path
            .canonicalize()
            .map_err(|error| format!("office file path could not be resolved: {error}"))?;
        if !canonical_path.starts_with(&workspace) {
            return Err(
                "office update path must stay inside the configured workspace or desktop"
                    .to_string(),
            );
        }

        let existing = std::fs::read(&canonical_path)
            .map_err(|error| format!("office file could not be read for update: {error}"))?;
        let updated = update_office_artifact_bytes(&existing, spec)?;
        if updated.len() > self.max_bytes {
            return Err(format!(
                "office artifact is too large after update: {} bytes exceeds {} bytes",
                updated.len(),
                self.max_bytes
            ));
        }
        std::fs::write(&canonical_path, &updated)
            .map_err(|error| format!("office file could not be written after update: {error}"))?;

        Ok(OfficeUpdateResult {
            path: canonical_path.to_string_lossy().to_string(),
            bytes: updated.len() as u64,
            app: spec.app,
            artifact_kind: spec.app.artifact_kind().to_string(),
            summary: office_update_summary(spec),
        })
    }

    fn verify_updated_artifact_path(
        &self,
        spec: &OfficeUpdateSpec,
        result: &OfficeUpdateResult,
    ) -> Result<(), String> {
        let expected = resolve_office_artifact_path(
            &self.workspace_dir,
            self.desktop_dir.as_deref(),
            &spec.path,
        )?
        .path
        .canonicalize()
        .map_err(|_| "approved office update path could not be resolved".to_string())?;
        let actual = Path::new(&result.path)
            .canonicalize()
            .map_err(|_| "updated office artifact path could not be resolved".to_string())?;
        if actual != expected {
            return Err(
                "updated office artifact path changed from the approved target".to_string(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
pub fn run_office_create_boundary(
    request: OfficeCreateRequest,
    client: &impl OfficeArtifactClient,
) -> Result<OfficeCreateOutcome, String> {
    validate_office_create_spec(&request.spec)?;
    let access_request = office_create_access_request(request.access_mode)?;
    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        let invocation = CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::FileWrite,
            status: CapabilityInvocationStatus::PendingApproval,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(request.spec.path.clone()),
            evidence_ref: None,
            requested_url: None,
            evidence_url: None,
            title: Some(format!(
                "{} creation request",
                request.spec.app.user_facing_name()
            )),
            excerpt: Some(request.spec.title.clone()),
            warnings: vec![
                "office artifact creation requires approval before writing a local file"
                    .to_string(),
            ],
            elapsed_ms: 0,
            created_at: Utc::now(),
        };
        return Ok(OfficeCreateOutcome {
            access_request,
            invocation,
            result: None,
        });
    }

    let started_at = std::time::Instant::now();
    match client.write_office_artifact(&request.spec) {
        Ok(result) => {
            client.verify_written_artifact_path(&request.spec, &result)?;
            let invocation = CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileWrite,
                status: CapabilityInvocationStatus::Succeeded,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(result.path.clone()),
                evidence_ref: Some(result.path.clone()),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("{} created", request.spec.app.user_facing_name())),
                excerpt: Some(format!("{} bytes written", result.bytes)),
                warnings: Vec::new(),
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            };
            Ok(OfficeCreateOutcome {
                access_request,
                invocation,
                result: Some(result),
            })
        }
        Err(error) => {
            let invocation = CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileWrite,
                status: CapabilityInvocationStatus::Failed,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(request.spec.path.clone()),
                evidence_ref: None,
                requested_url: None,
                evidence_url: None,
                title: Some(format!(
                    "{} creation failed",
                    request.spec.app.user_facing_name()
                )),
                excerpt: Some(request.spec.title.clone()),
                warnings: vec![error],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            };
            Ok(OfficeCreateOutcome {
                access_request,
                invocation,
                result: None,
            })
        }
    }
}

#[cfg(test)]
fn office_create_access_request(
    access_mode: AccessMode,
) -> Result<CapabilityAccessRequest, String> {
    let mut request = request_capability_access(access_mode, CapabilityKind::FileWrite)?;
    if access_mode != AccessMode::AskEveryStep {
        request.decision = PolicyDecision::Allow;
        request.status = CapabilityAccessStatus::AutoApproved;
        request.reason =
            "controlled Office artifact creation is allowed without extra confirmation".to_string();
    }
    Ok(request)
}

pub fn run_office_open_boundary(
    request: OfficeOpenRequest,
    client: &impl OfficeOpenClient,
) -> Result<OfficeOpenOutcome, String> {
    let path = request.path.trim();
    if path.is_empty() {
        return Err("office_open target is required before dispatch".to_string());
    }
    let app = OfficeApp::from_path(path)
        .or(request.preferred_app)
        .ok_or_else(|| "office_open target must be a .docx, .xlsx, or .pptx file".to_string())?;
    let path = ensure_office_extension(path, app)?;
    let access_request = request_capability_access(request.access_mode, CapabilityKind::FileRead)?;
    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        let invocation = CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::FileRead,
            status: CapabilityInvocationStatus::PendingApproval,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(path.clone()),
            evidence_ref: Some(path.clone()),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("{} open request", app.user_facing_name())),
            excerpt: Some(path),
            warnings: vec![
                "office file open requires approval before launching a local document".to_string(),
            ],
            elapsed_ms: 0,
            created_at: Utc::now(),
        };
        return Ok(OfficeOpenOutcome {
            access_request,
            invocation,
            result: None,
        });
    }

    let started_at = std::time::Instant::now();
    match client.open_office_artifact(&path, Some(app)) {
        Ok(result) => {
            let warnings = result.fallback_note.clone().into_iter().collect::<Vec<_>>();
            let invocation = CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileRead,
                status: CapabilityInvocationStatus::Succeeded,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(result.path.clone()),
                evidence_ref: Some(result.path.clone()),
                requested_url: None,
                evidence_url: None,
                title: Some(format!(
                    "{} opened with {}",
                    result.app.user_facing_name(),
                    result.opener_label
                )),
                excerpt: Some(result.path.clone()),
                warnings,
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            };
            Ok(OfficeOpenOutcome {
                access_request,
                invocation,
                result: Some(result),
            })
        }
        Err(error) => {
            let invocation = CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileRead,
                status: CapabilityInvocationStatus::Failed,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(path.clone()),
                evidence_ref: None,
                requested_url: None,
                evidence_url: None,
                title: Some(format!("{} open failed", app.user_facing_name())),
                excerpt: Some(path),
                warnings: vec![error],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            };
            Ok(OfficeOpenOutcome {
                access_request,
                invocation,
                result: None,
            })
        }
    }
}

pub fn run_office_update_boundary(
    request: OfficeUpdateRequest,
    client: &impl OfficeUpdateClient,
) -> Result<OfficeUpdateOutcome, String> {
    validate_office_update_spec(&request.spec)?;
    let access_request = request_capability_access(request.access_mode, CapabilityKind::FileWrite)?;
    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        let invocation = CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::FileWrite,
            status: CapabilityInvocationStatus::PendingApproval,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(request.spec.path.clone()),
            evidence_ref: None,
            requested_url: None,
            evidence_url: None,
            title: Some(format!(
                "{} update request",
                request.spec.app.user_facing_name()
            )),
            excerpt: Some(office_update_summary(&request.spec)),
            warnings: vec![
                "office file update requires approval before writing a local file".to_string(),
            ],
            elapsed_ms: 0,
            created_at: Utc::now(),
        };
        return Ok(OfficeUpdateOutcome {
            access_request,
            invocation,
            result: None,
        });
    }

    let started_at = std::time::Instant::now();
    match client.update_office_artifact(&request.spec) {
        Ok(result) => {
            let invocation = CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileWrite,
                status: CapabilityInvocationStatus::Succeeded,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(result.path.clone()),
                evidence_ref: Some(result.path.clone()),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("{} updated", request.spec.app.user_facing_name())),
                excerpt: Some(result.summary.clone()),
                warnings: Vec::new(),
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            };
            Ok(OfficeUpdateOutcome {
                access_request,
                invocation,
                result: Some(result),
            })
        }
        Err(error) => {
            let invocation = CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileWrite,
                status: CapabilityInvocationStatus::Failed,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(request.spec.path.clone()),
                evidence_ref: None,
                requested_url: None,
                evidence_url: None,
                title: Some(format!(
                    "{} update failed",
                    request.spec.app.user_facing_name()
                )),
                excerpt: Some(office_update_summary(&request.spec)),
                warnings: vec![error],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            };
            Ok(OfficeUpdateOutcome {
                access_request,
                invocation,
                result: None,
            })
        }
    }
}

pub fn build_office_artifact(spec: &OfficeCreateSpec) -> Result<Vec<u8>, String> {
    validate_office_create_spec(spec)?;
    match spec.app {
        OfficeApp::Word => build_docx(spec),
        OfficeApp::Excel => build_xlsx(spec),
        OfficeApp::PowerPoint => build_pptx(spec),
    }
}

pub fn update_office_artifact_bytes(
    existing: &[u8],
    spec: &OfficeUpdateSpec,
) -> Result<Vec<u8>, String> {
    validate_office_update_spec(spec)?;
    let mut parts = read_office_zip_parts(existing)?;
    match spec.app {
        OfficeApp::Word => update_docx_parts(&mut parts, spec)?,
        OfficeApp::Excel => update_xlsx_parts(&mut parts, spec)?,
        OfficeApp::PowerPoint => update_pptx_parts(&mut parts, spec)?,
    }
    write_office_zip_parts(parts)
}

fn validate_office_create_spec(spec: &OfficeCreateSpec) -> Result<(), String> {
    if spec.path.trim().is_empty() {
        return Err("office artifact path is required".to_string());
    }
    if spec.title.trim().is_empty() && spec.body.trim().is_empty() {
        return Err("office artifact title or body is required".to_string());
    }
    if !spec.path.ends_with(spec.app.extension()) {
        return Err(format!(
            "office artifact path for {} must end with {}",
            spec.app.user_facing_name(),
            spec.app.extension()
        ));
    }
    Ok(())
}

fn validate_office_update_spec(spec: &OfficeUpdateSpec) -> Result<(), String> {
    if spec.path.trim().is_empty() {
        return Err("office update path is required".to_string());
    }
    if !spec.path.ends_with(spec.app.extension()) {
        return Err(format!(
            "office update path for {} must end with {}",
            spec.app.user_facing_name(),
            spec.app.extension()
        ));
    }
    if spec.body.trim().is_empty() && spec.rows.is_empty() && spec.slides.is_empty() {
        return Err("office update content is required".to_string());
    }
    Ok(())
}

struct ResolvedOfficeArtifactPath {
    path: PathBuf,
    boundary_dir: PathBuf,
}

fn resolve_office_artifact_path(
    workspace_dir: &Path,
    desktop_dir: Option<&Path>,
    relative_path: &str,
) -> Result<ResolvedOfficeArtifactPath, String> {
    let (boundary_dir, relative_path) =
        office_artifact_boundary_and_relative_path(workspace_dir, desktop_dir, relative_path)?;
    let path = Path::new(&relative_path);
    if path.is_absolute() {
        return Err("office artifact path must be relative".to_string());
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return Err(
            "office artifact path must stay inside the configured workspace or desktop".to_string(),
        );
    }

    Ok(ResolvedOfficeArtifactPath {
        path: boundary_dir.join(path),
        boundary_dir,
    })
}

fn office_artifact_boundary_and_relative_path(
    workspace_dir: &Path,
    desktop_dir: Option<&Path>,
    relative_path: &str,
) -> Result<(PathBuf, String), String> {
    let normalized = relative_path.trim().replace('\\', "/");
    let normalized = normalized.trim_matches('/').to_string();
    if let Some(rest) = normalized.strip_prefix("desktop/") {
        let desktop_dir = desktop_dir
            .ok_or_else(|| "desktop directory is unavailable on this machine".to_string())?;
        if rest.trim().is_empty() {
            return Err("desktop office artifact path requires a file name".to_string());
        }
        return Ok((desktop_dir.to_path_buf(), rest.to_string()));
    }
    if let Some(rest) = normalized.strip_prefix("workspace/") {
        if rest.trim().is_empty() {
            return Err("workspace office artifact path requires a file name".to_string());
        }
        return Ok((workspace_dir.to_path_buf(), rest.to_string()));
    }
    Ok((workspace_dir.to_path_buf(), normalized))
}

fn canonical_boundary_dir(boundary_dir: &Path) -> Result<PathBuf, String> {
    boundary_dir
        .canonicalize()
        .map_err(|error| format!("office boundary directory could not be resolved: {error}"))
}

fn build_docx(spec: &OfficeCreateSpec) -> Result<Vec<u8>, String> {
    let mut zip = new_zip_writer();
    add_part(&mut zip, "[Content_Types].xml", docx_content_types())?;
    add_part(
        &mut zip,
        "_rels/.rels",
        root_relationships("word/document.xml"),
    )?;
    add_part(&mut zip, "docProps/core.xml", core_properties(&spec.title))?;
    add_part(
        &mut zip,
        "docProps/app.xml",
        app_properties("DS Agent Office Pack"),
    )?;
    add_part(
        &mut zip,
        "word/_rels/document.xml.rels",
        word_relationships(),
    )?;
    add_part(&mut zip, "word/styles.xml", word_styles())?;
    add_part(&mut zip, "word/settings.xml", word_settings())?;
    add_part(&mut zip, "word/fontTable.xml", word_font_table())?;
    add_part(&mut zip, "word/document.xml", word_document_xml(spec))?;
    finish_zip(zip)
}

fn build_xlsx(spec: &OfficeCreateSpec) -> Result<Vec<u8>, String> {
    let mut zip = new_zip_writer();
    add_part(&mut zip, "[Content_Types].xml", xlsx_content_types())?;
    add_part(
        &mut zip,
        "_rels/.rels",
        root_relationships("xl/workbook.xml"),
    )?;
    add_part(&mut zip, "docProps/core.xml", core_properties(&spec.title))?;
    add_part(
        &mut zip,
        "docProps/app.xml",
        app_properties("DS Agent Office Pack"),
    )?;
    add_part(&mut zip, "xl/workbook.xml", workbook_xml())?;
    add_part(
        &mut zip,
        "xl/_rels/workbook.xml.rels",
        workbook_relationships(),
    )?;
    add_part(&mut zip, "xl/worksheets/sheet1.xml", worksheet_xml(spec))?;
    finish_zip(zip)
}

fn build_pptx(spec: &OfficeCreateSpec) -> Result<Vec<u8>, String> {
    let slides = normalized_slides(spec);
    let mut zip = new_zip_writer();
    add_part(
        &mut zip,
        "[Content_Types].xml",
        pptx_content_types(slides.len()),
    )?;
    add_part(
        &mut zip,
        "_rels/.rels",
        root_relationships("ppt/presentation.xml"),
    )?;
    add_part(&mut zip, "docProps/core.xml", core_properties(&spec.title))?;
    add_part(
        &mut zip,
        "docProps/app.xml",
        app_properties("DS Agent Office Pack"),
    )?;
    add_part(
        &mut zip,
        "ppt/presentation.xml",
        presentation_xml(slides.len()),
    )?;
    add_part(
        &mut zip,
        "ppt/_rels/presentation.xml.rels",
        presentation_relationships(slides.len()),
    )?;
    add_part(
        &mut zip,
        "ppt/slideMasters/slideMaster1.xml",
        slide_master_xml(),
    )?;
    add_part(
        &mut zip,
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        slide_master_relationships(),
    )?;
    add_part(
        &mut zip,
        "ppt/slideLayouts/slideLayout1.xml",
        slide_layout_xml(),
    )?;
    add_part(
        &mut zip,
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        slide_layout_relationships(),
    )?;
    add_part(&mut zip, "ppt/theme/theme1.xml", theme_xml())?;
    for (index, slide) in slides.iter().enumerate() {
        let slide_number = index + 1;
        add_part(
            &mut zip,
            &format!("ppt/slides/slide{slide_number}.xml"),
            slide_xml(slide),
        )?;
        add_part(
            &mut zip,
            &format!("ppt/slides/_rels/slide{slide_number}.xml.rels"),
            slide_relationships(),
        )?;
    }
    finish_zip(zip)
}

fn new_zip_writer() -> zip::ZipWriter<Cursor<Vec<u8>>> {
    zip::ZipWriter::new(Cursor::new(Vec::new()))
}

fn add_part(
    zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>,
    path: &str,
    content: String,
) -> Result<(), String> {
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    zip.start_file(path, options)
        .map_err(|error| format!("office zip part {path} could not be started: {error}"))?;
    zip.write_all(content.as_bytes())
        .map_err(|error| format!("office zip part {path} could not be written: {error}"))
}

fn finish_zip(mut zip: zip::ZipWriter<Cursor<Vec<u8>>>) -> Result<Vec<u8>, String> {
    zip.finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| format!("office zip package could not be finished: {error}"))
}

fn read_office_zip_parts(existing: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(existing.to_vec()))
        .map_err(|error| format!("office file could not be read as an OOXML zip: {error}"))?;
    let mut parts = BTreeMap::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("office zip part could not be opened: {error}"))?;
        if !file.is_file() {
            continue;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|error| format!("office zip part could not be read: {error}"))?;
        parts.insert(file.name().to_string(), bytes);
    }
    Ok(parts)
}

fn write_office_zip_parts(parts: BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut zip = new_zip_writer();
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (path, bytes) in parts {
        zip.start_file(&path, options)
            .map_err(|error| format!("office zip part {path} could not be started: {error}"))?;
        zip.write_all(&bytes)
            .map_err(|error| format!("office zip part {path} could not be written: {error}"))?;
    }
    finish_zip(zip)
}

fn update_docx_parts(
    parts: &mut BTreeMap<String, Vec<u8>>,
    spec: &OfficeUpdateSpec,
) -> Result<(), String> {
    let document_xml = part_as_string(parts, "word/document.xml")?;
    let paragraphs = body_lines(&spec.body)
        .into_iter()
        .map(|line| word_paragraph(&line))
        .collect::<Vec<_>>();
    if paragraphs.is_empty() {
        return Err("word update body is required".to_string());
    }
    let insertion = format!("{}\n    ", paragraphs.join("\n    "));
    let updated = if document_xml.contains("    <w:sectPr>") {
        document_xml.replacen("    <w:sectPr>", &format!("    {insertion}<w:sectPr>"), 1)
    } else {
        insert_before(
            &document_xml,
            "</w:body>",
            &format!("    {}</w:body>", paragraphs.join("\n    ")),
        )?
    };
    parts.insert("word/document.xml".to_string(), updated.into_bytes());
    Ok(())
}

fn update_xlsx_parts(
    parts: &mut BTreeMap<String, Vec<u8>>,
    spec: &OfficeUpdateSpec,
) -> Result<(), String> {
    let sheet_xml = part_as_string(parts, "xl/worksheets/sheet1.xml")?;
    let rows = if spec.rows.is_empty() {
        normalized_rows_from_text(&spec.body)
    } else {
        spec.rows.clone()
    };
    if rows.is_empty() {
        return Err("excel update rows are required".to_string());
    }

    let existing_rows = max_worksheet_row_number(&sheet_xml).max(1);
    let existing_cols = max_worksheet_column_number(&sheet_xml).max(1);
    let appended_rows = rows
        .iter()
        .enumerate()
        .map(|(index, row)| worksheet_row_xml(existing_rows + index + 1, row))
        .collect::<Vec<_>>()
        .join("\n    ");
    let updated = insert_before(
        &sheet_xml,
        "  </sheetData>",
        &format!("    {appended_rows}\n  </sheetData>"),
    )?;
    let new_row_count = existing_rows + rows.len();
    let new_col_count = existing_cols.max(rows.iter().map(Vec::len).max().unwrap_or(1));
    let updated = replace_worksheet_dimension(&updated, new_col_count, new_row_count);
    parts.insert("xl/worksheets/sheet1.xml".to_string(), updated.into_bytes());
    Ok(())
}

fn update_pptx_parts(
    parts: &mut BTreeMap<String, Vec<u8>>,
    spec: &OfficeUpdateSpec,
) -> Result<(), String> {
    let slides = if spec.slides.is_empty() {
        default_slides_for(OfficeApp::PowerPoint, "Updated slide", &spec.body)
    } else {
        spec.slides.clone()
    };
    if slides.is_empty() {
        return Err("powerpoint update slides are required".to_string());
    }

    let existing_slide_count = count_pptx_slides(parts).max(1);
    for (index, slide) in slides.iter().enumerate() {
        let slide_number = existing_slide_count + index + 1;
        parts.insert(
            format!("ppt/slides/slide{slide_number}.xml"),
            slide_xml(slide).into_bytes(),
        );
        parts.insert(
            format!("ppt/slides/_rels/slide{slide_number}.xml.rels"),
            slide_relationships().into_bytes(),
        );
    }

    let final_slide_count = existing_slide_count + slides.len();
    let content_types = part_as_string(parts, "[Content_Types].xml")?;
    parts.insert(
        "[Content_Types].xml".to_string(),
        update_pptx_content_types(&content_types, existing_slide_count + 1, final_slide_count)
            .into_bytes(),
    );
    let presentation = part_as_string(parts, "ppt/presentation.xml")?;
    parts.insert(
        "ppt/presentation.xml".to_string(),
        update_presentation_slides(&presentation, existing_slide_count + 1, final_slide_count)?
            .into_bytes(),
    );
    let presentation_rels = part_as_string(parts, "ppt/_rels/presentation.xml.rels")?;
    parts.insert(
        "ppt/_rels/presentation.xml.rels".to_string(),
        update_presentation_relationships(
            &presentation_rels,
            existing_slide_count + 1,
            final_slide_count,
        )?
        .into_bytes(),
    );
    Ok(())
}

fn part_as_string(parts: &BTreeMap<String, Vec<u8>>, path: &str) -> Result<String, String> {
    let bytes = parts
        .get(path)
        .ok_or_else(|| format!("office package is missing required part {path}"))?;
    String::from_utf8(bytes.clone())
        .map_err(|error| format!("office package part {path} is not UTF-8 XML: {error}"))
}

fn insert_before(value: &str, needle: &str, replacement: &str) -> Result<String, String> {
    let index = value
        .find(needle)
        .ok_or_else(|| format!("office XML marker {needle} was not found"))?;
    let mut updated = String::new();
    updated.push_str(&value[..index]);
    updated.push_str(replacement);
    updated.push_str(&value[index + needle.len()..]);
    Ok(updated)
}

fn open_office_file(
    path: &Path,
    app: OfficeApp,
    preferred_app: Option<OfficeApp>,
) -> Result<OfficeOpenResult, String> {
    if preferred_app.is_some() {
        if let Some(executable) = find_office_executable(app) {
            return Command::new(&executable)
                .arg(path)
                .spawn()
                .map(|_| OfficeOpenResult {
                    path: path.to_string_lossy().to_string(),
                    app,
                    opener_label: app.desktop_app_name().to_string(),
                    fallback_note: None,
                })
                .map_err(|error| {
                    format!(
                        "{} could not be launched for Office file: {error}",
                        app.desktop_app_name()
                    )
                });
        }
    }

    let fallback_note = preferred_app.map(|_| {
        format!(
            "未检测到{}，已使用默认应用打开 {}",
            app.desktop_app_name(),
            path.to_string_lossy()
        )
    });
    open_path_in_default_app(path).map(|()| OfficeOpenResult {
        path: path.to_string_lossy().to_string(),
        app,
        opener_label: "default app".to_string(),
        fallback_note,
    })
}

fn office_shell_compatible_path(path: &Path) -> PathBuf {
    if cfg!(windows) {
        let value = path.to_string_lossy();
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = value.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
    }

    path.to_path_buf()
}

fn open_path_in_default_app(path: &Path) -> Result<(), String> {
    let mut command = if cfg!(target_os = "windows") {
        let mut command = Command::new("rundll32");
        command
            .arg("url.dll,FileProtocolHandler")
            .arg(path.to_string_lossy().to_string());
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(path);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("default Office file application could not be opened: {error}"))
}

fn find_office_executable(app: OfficeApp) -> Option<PathBuf> {
    let executable_name = app.windows_executable_name();
    let mut candidates = Vec::new();

    if cfg!(target_os = "windows") {
        for variable in [
            "ProgramFiles",
            "PROGRAMFILES",
            "ProgramFiles(x86)",
            "PROGRAMFILES(X86)",
        ] {
            if let Some(base) = std::env::var_os(variable) {
                for office_dir in ["Office16", "Office15", "Office14"] {
                    candidates.push(
                        PathBuf::from(&base)
                            .join("Microsoft Office")
                            .join("root")
                            .join(office_dir)
                            .join(executable_name),
                    );
                    candidates.push(
                        PathBuf::from(&base)
                            .join("Microsoft Office")
                            .join(office_dir)
                            .join(executable_name),
                    );
                }
            }
        }
        candidates.extend(find_executable_on_path(&[executable_name]));
    } else if cfg!(target_os = "macos") {
        candidates.push(
            PathBuf::from("/Applications")
                .join(format!("{}.app", app.desktop_app_name()))
                .join("Contents")
                .join("MacOS")
                .join(app.desktop_app_name()),
        );
    } else {
        candidates.extend(find_executable_on_path(app.unix_executable_names()));
    }

    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn find_executable_on_path(names: &[&str]) -> Vec<PathBuf> {
    let Some(path_value) = std::env::var_os("PATH") else {
        return Vec::new();
    };

    std::env::split_paths(&path_value)
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .filter(|candidate| candidate.is_file())
        .collect()
}

fn root_relationships(office_document_target: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="{office_document_target}"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/>
</Relationships>"#
    )
}

fn core_properties(title: &str) -> String {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:dcmitype="http://purl.org/dc/dcmitype/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <dc:title>{}</dc:title>
  <dc:creator>DS Agent</dc:creator>
  <cp:lastModifiedBy>DS Agent</cp:lastModifiedBy>
  <dcterms:created xsi:type="dcterms:W3CDTF">{now}</dcterms:created>
  <dcterms:modified xsi:type="dcterms:W3CDTF">{now}</dcterms:modified>
</cp:coreProperties>"#,
        xml_escape(title)
    )
}

fn app_properties(application: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">
  <Application>{}</Application>
</Properties>"#,
        xml_escape(application)
    )
}

fn docx_content_types() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>
  <Override PartName="/word/fontTable.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.fontTable+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
  <Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>
</Types>"#
        .to_string()
}

fn word_document_xml(spec: &OfficeCreateSpec) -> String {
    let mut paragraphs = Vec::new();
    for line in body_lines(&spec.body) {
        paragraphs.push(word_paragraph(&line));
    }
    if paragraphs.is_empty() {
        paragraphs.push(word_paragraph(" "));
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    {}
    <w:sectPr>
      <w:pgSz w:w="12240" w:h="15840"/>
      <w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/>
    </w:sectPr>
  </w:body>
</w:document>"#,
        paragraphs.join("\n    ")
    )
}

fn word_relationships() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/fontTable" Target="fontTable.xml"/>
</Relationships>"#
        .to_string()
}

fn word_styles() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:docDefaults>
    <w:rPrDefault>
      <w:rPr>
        <w:rFonts w:ascii="Aptos" w:hAnsi="Aptos" w:eastAsia="Microsoft YaHei" w:cs="Aptos"/>
        <w:sz w:val="24"/>
        <w:szCs w:val="24"/>
      </w:rPr>
    </w:rPrDefault>
  </w:docDefaults>
  <w:style w:type="paragraph" w:default="1" w:styleId="Normal">
    <w:name w:val="Normal"/>
    <w:qFormat/>
  </w:style>
</w:styles>"#
        .to_string()
}

fn word_settings() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:compat>
    <w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/>
  </w:compat>
</w:settings>"#
        .to_string()
}

fn word_font_table() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:fonts xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:font w:name="Aptos">
    <w:charset w:val="00"/>
    <w:family w:val="swiss"/>
  </w:font>
  <w:font w:name="Microsoft YaHei">
    <w:charset w:val="86"/>
    <w:family w:val="swiss"/>
  </w:font>
</w:fonts>"#
        .to_string()
}

fn word_paragraph(text: &str) -> String {
    format!(
        r#"<w:p><w:r><w:rPr><w:rFonts w:ascii="Aptos" w:hAnsi="Aptos" w:eastAsia="Microsoft YaHei" w:cs="Aptos"/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr><w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
        xml_escape(text)
    )
}

fn xlsx_content_types() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
  <Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>
</Types>"#
        .to_string()
}

fn workbook_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Sheet1" sheetId="1" r:id="rId1"/>
  </sheets>
  <calcPr calcMode="auto"/>
</workbook>"#
        .to_string()
}

fn workbook_relationships() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#
        .to_string()
}

fn worksheet_xml(spec: &OfficeCreateSpec) -> String {
    let rows = normalized_rows(spec);
    let max_cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let dimension = format!("A1:{}{}", column_name(max_cols), rows.len().max(1));
    let row_xml = rows
        .iter()
        .enumerate()
        .map(|(row_index, row)| worksheet_row_xml(row_index + 1, row))
        .collect::<Vec<_>>()
        .join("\n    ");

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <dimension ref="{dimension}"/>
  <sheetViews><sheetView workbookViewId="0"/></sheetViews>
  <sheetData>
    {row_xml}
  </sheetData>
</worksheet>"#
    )
}

fn worksheet_row_xml(row_number: usize, row: &[String]) -> String {
    let cells = row
        .iter()
        .enumerate()
        .map(|(column_index, value)| worksheet_cell_xml(row_number, column_index + 1, value))
        .collect::<Vec<_>>()
        .join("");
    format!(r#"<row r="{row_number}">{cells}</row>"#)
}

fn worksheet_cell_xml(row_number: usize, column_number: usize, value: &str) -> String {
    let reference = format!("{}{}", column_name(column_number), row_number);
    if let Some(formula) = normalized_formula(value) {
        format!(r#"<c r="{reference}"><f>{}</f></c>"#, xml_escape(&formula))
    } else if let Some(number) = normalized_number(value) {
        format!(r#"<c r="{reference}"><v>{number}</v></c>"#)
    } else {
        format!(
            r#"<c r="{reference}" t="inlineStr"><is><t>{}</t></is></c>"#,
            xml_escape(value)
        )
    }
}

fn normalized_rows(spec: &OfficeCreateSpec) -> Vec<Vec<String>> {
    if !spec.rows.is_empty() {
        return spec.rows.clone();
    }
    let body = spec.body.trim();
    if body.is_empty() {
        return vec![vec![spec.title.clone()]];
    }

    normalized_rows_from_text(body)
}

fn normalized_rows_from_text(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .map(|line| {
            let delimiter = if line.contains('\t') { '\t' } else { ',' };
            if line.contains(delimiter) {
                line.split(delimiter)
                    .map(|cell| cell.trim().to_string())
                    .collect::<Vec<_>>()
            } else {
                vec![line.trim().to_string()]
            }
        })
        .filter(|row| row.iter().any(|cell| !cell.is_empty()))
        .collect::<Vec<_>>()
}

fn normalized_number(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.contains(',')
        || trimmed.chars().any(char::is_whitespace)
        || (trimmed.starts_with('0') && trimmed.len() > 1 && !trimmed.starts_with("0."))
    {
        return None;
    }

    trimmed.parse::<f64>().ok()?;
    Some(trimmed.to_string())
}

fn normalized_formula(value: &str) -> Option<String> {
    let formula = value.trim().strip_prefix('=')?.trim();
    if formula.is_empty() {
        None
    } else {
        Some(formula.to_string())
    }
}

fn max_worksheet_row_number(sheet_xml: &str) -> usize {
    let mut max_row = 0;
    let mut rest = sheet_xml;
    while let Some(index) = rest.find("<row r=\"") {
        rest = &rest[index + "<row r=\"".len()..];
        let row = rest
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect::<String>()
            .parse::<usize>()
            .unwrap_or(0);
        max_row = max_row.max(row);
    }
    max_row
}

fn max_worksheet_column_number(sheet_xml: &str) -> usize {
    let mut max_col = 0;
    let mut rest = sheet_xml;
    while let Some(index) = rest.find("<c r=\"") {
        rest = &rest[index + "<c r=\"".len()..];
        let column = rest
            .chars()
            .take_while(|character| character.is_ascii_alphabetic())
            .collect::<String>();
        max_col = max_col.max(column_number(&column));
    }
    max_col
}

fn replace_worksheet_dimension(sheet_xml: &str, max_cols: usize, max_rows: usize) -> String {
    let dimension = format!("A1:{}{}", column_name(max_cols.max(1)), max_rows.max(1));
    let Some(start) = sheet_xml.find("<dimension ref=\"") else {
        return sheet_xml.to_string();
    };
    let after_start = start + "<dimension ref=\"".len();
    let Some(end_offset) = sheet_xml[after_start..].find("\"/>") else {
        return sheet_xml.to_string();
    };
    let end = after_start + end_offset;
    format!(
        "{}{}{}",
        &sheet_xml[..after_start],
        dimension,
        &sheet_xml[end..]
    )
}

fn column_name(mut column_number: usize) -> String {
    let mut name = String::new();
    while column_number > 0 {
        let remainder = (column_number - 1) % 26;
        name.insert(0, (b'A' + remainder as u8) as char);
        column_number = (column_number - 1) / 26;
    }
    name
}

fn column_number(column_name: &str) -> usize {
    column_name
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .fold(0usize, |accumulator, character| {
            accumulator * 26 + (character.to_ascii_uppercase() as usize - 'A' as usize + 1)
        })
}

fn pptx_content_types(slide_count: usize) -> String {
    let slide_overrides = (1..=slide_count.max(1))
        .map(|slide_number| {
            format!(r#"<Override PartName="/ppt/slides/slide{slide_number}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#)
        })
        .collect::<Vec<_>>()
        .join("\n  ");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>
  <Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/>
  <Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/>
  <Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>
  {slide_overrides}
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
  <Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>
</Types>"#
    )
}

fn presentation_xml(slide_count: usize) -> String {
    let slide_ids = (1..=slide_count.max(1))
        .map(|slide_number| {
            format!(
                r#"<p:sldId id="{}" r:id="rId{}"/>"#,
                255 + slide_number,
                slide_number + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n    ");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>
  <p:sldIdLst>
    {slide_ids}
  </p:sldIdLst>
  <p:sldSz cx="12192000" cy="6858000" type="screen16x9"/>
  <p:notesSz cx="6858000" cy="9144000"/>
</p:presentation>"#
    )
}

fn presentation_relationships(slide_count: usize) -> String {
    let mut relationships = vec![r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>"#.to_string()];
    for slide_number in 1..=slide_count.max(1) {
        relationships.push(format!(
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{slide_number}.xml"/>"#,
            slide_number + 1
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  {}
</Relationships>"#,
        relationships.join("\n  ")
    )
}

fn count_pptx_slides(parts: &BTreeMap<String, Vec<u8>>) -> usize {
    parts
        .keys()
        .filter_map(|path| {
            let name = path
                .strip_prefix("ppt/slides/slide")?
                .strip_suffix(".xml")?;
            if name.contains('/') {
                None
            } else {
                name.parse::<usize>().ok()
            }
        })
        .max()
        .unwrap_or(0)
}

fn update_pptx_content_types(content_types: &str, first_slide: usize, last_slide: usize) -> String {
    let overrides = (first_slide..=last_slide)
        .filter(|slide_number| {
            !content_types.contains(&format!(r#"/ppt/slides/slide{slide_number}.xml"#))
        })
        .map(|slide_number| {
            format!(r#"<Override PartName="/ppt/slides/slide{slide_number}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#)
        })
        .collect::<Vec<_>>();
    if overrides.is_empty() {
        return content_types.to_string();
    }
    match content_types.find("</Types>") {
        Some(index) => format!(
            "{}  {}\n{}",
            &content_types[..index],
            overrides.join("\n  "),
            &content_types[index..]
        ),
        None => content_types.to_string(),
    }
}

fn update_presentation_slides(
    presentation_xml: &str,
    first_slide: usize,
    last_slide: usize,
) -> Result<String, String> {
    let slide_ids = (first_slide..=last_slide)
        .map(|slide_number| {
            format!(
                r#"<p:sldId id="{}" r:id="rId{}"/>"#,
                255 + slide_number,
                slide_number + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n    ");
    insert_before(
        presentation_xml,
        "  </p:sldIdLst>",
        &format!("    {slide_ids}\n  </p:sldIdLst>"),
    )
}

fn update_presentation_relationships(
    presentation_rels: &str,
    first_slide: usize,
    last_slide: usize,
) -> Result<String, String> {
    let relationships = (first_slide..=last_slide)
        .map(|slide_number| {
            format!(
                r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{slide_number}.xml"/>"#,
                slide_number + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n  ");
    insert_before(
        presentation_rels,
        "</Relationships>",
        &format!("  {relationships}\n</Relationships>"),
    )
}

fn slide_master_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>
  <p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>
  <p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>
  <p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles>
</p:sldMaster>"#
        .to_string()
}

fn slide_master_relationships() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/>
</Relationships>"#
        .to_string()
}

fn slide_layout_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1">
  <p:cSld name="Blank"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree></p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sldLayout>"#
        .to_string()
}

fn slide_layout_relationships() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/>
</Relationships>"#
        .to_string()
}

fn slide_relationships() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/>
</Relationships>"#
        .to_string()
}

fn slide_xml(slide: &OfficeSlideSpec) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>
      <p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>
      {}
      {}
    </p:spTree>
  </p:cSld>
  <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>
</p:sld>"#,
        ppt_text_shape(
            2,
            "Title",
            &slide.title,
            PptTextShapeLayout {
                x: 685800,
                y: 609600,
                cx: 10820400,
                cy: 914400,
                font_size: 3600,
                bold: true,
            },
        ),
        ppt_text_shape(
            3,
            "Body",
            &slide.body,
            PptTextShapeLayout {
                x: 914400,
                y: 1828800,
                cx: 10363200,
                cy: 3657600,
                font_size: 2400,
                bold: false,
            },
        )
    )
}

struct PptTextShapeLayout {
    x: i64,
    y: i64,
    cx: i64,
    cy: i64,
    font_size: u32,
    bold: bool,
}

fn ppt_text_shape(id: u32, name: &str, text: &str, layout: PptTextShapeLayout) -> String {
    let PptTextShapeLayout {
        x,
        y,
        cx,
        cy,
        font_size,
        bold,
    } = layout;
    let bold_attr = if bold { r#" b="1""# } else { "" };
    let paragraphs = body_lines(text)
        .into_iter()
        .map(|line| {
            format!(
                r#"<a:p><a:r><a:rPr lang="zh-CN" sz="{font_size}"{bold_attr}/><a:t>{}</a:t></a:r></a:p>"#,
                xml_escape(&line)
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap="square"/><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#
    )
}

fn theme_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="DS Agent">
  <a:themeElements>
    <a:clrScheme name="DS Agent">
      <a:dk1><a:srgbClr val="1F2937"/></a:dk1>
      <a:lt1><a:srgbClr val="FFFFFF"/></a:lt1>
      <a:dk2><a:srgbClr val="334155"/></a:dk2>
      <a:lt2><a:srgbClr val="F8FAFC"/></a:lt2>
      <a:accent1><a:srgbClr val="1B5FA7"/></a:accent1>
      <a:accent2><a:srgbClr val="2775B6"/></a:accent2>
      <a:accent3><a:srgbClr val="6EA8D7"/></a:accent3>
      <a:accent4><a:srgbClr val="94A3B8"/></a:accent4>
      <a:accent5><a:srgbClr val="0F766E"/></a:accent5>
      <a:accent6><a:srgbClr val="D97706"/></a:accent6>
      <a:hlink><a:srgbClr val="1B5FA7"/></a:hlink>
      <a:folHlink><a:srgbClr val="475569"/></a:folHlink>
    </a:clrScheme>
    <a:fontScheme name="DS Agent">
      <a:majorFont><a:latin typeface="Aptos Display"/><a:ea typeface="Microsoft YaHei"/><a:cs typeface=""/><a:font script="Hans" typeface="Microsoft YaHei"/></a:majorFont>
      <a:minorFont><a:latin typeface="Aptos"/><a:ea typeface="Microsoft YaHei"/><a:cs typeface=""/><a:font script="Hans" typeface="Microsoft YaHei"/></a:minorFont>
    </a:fontScheme>
    <a:fmtScheme name="DS Agent">
      <a:fillStyleLst>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:lumMod val="110000"/><a:satMod val="105000"/><a:tint val="67000"/></a:schemeClr></a:gs><a:gs pos="50000"><a:schemeClr val="phClr"><a:lumMod val="105000"/><a:satMod val="103000"/><a:tint val="73000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:lumMod val="105000"/><a:satMod val="109000"/><a:tint val="81000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>
        <a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:satMod val="103000"/><a:lumMod val="102000"/><a:tint val="94000"/></a:schemeClr></a:gs><a:gs pos="50000"><a:schemeClr val="phClr"><a:satMod val="110000"/><a:lumMod val="100000"/><a:shade val="100000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:lumMod val="99000"/><a:satMod val="120000"/><a:shade val="78000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>
      </a:fillStyleLst>
      <a:lnStyleLst>
        <a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>
        <a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>
        <a:ln w="19050" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>
      </a:lnStyleLst>
      <a:effectStyleLst>
        <a:effectStyle><a:effectLst/></a:effectStyle>
        <a:effectStyle><a:effectLst/></a:effectStyle>
        <a:effectStyle><a:effectLst><a:outerShdw blurRad="57150" dist="19050" dir="5400000" algn="ctr" rotWithShape="0"><a:srgbClr val="000000"><a:alpha val="63000"/></a:srgbClr></a:outerShdw></a:effectLst></a:effectStyle>
      </a:effectStyleLst>
      <a:bgFillStyleLst>
        <a:solidFill><a:schemeClr val="phClr"/></a:solidFill>
        <a:solidFill><a:schemeClr val="phClr"><a:tint val="95000"/><a:satMod val="170000"/></a:schemeClr></a:solidFill>
        <a:gradFill rotWithShape="1"><a:gsLst><a:gs pos="0"><a:schemeClr val="phClr"><a:tint val="93000"/><a:satMod val="150000"/><a:shade val="98000"/><a:lumMod val="102000"/></a:schemeClr></a:gs><a:gs pos="50000"><a:schemeClr val="phClr"><a:tint val="98000"/><a:satMod val="130000"/><a:shade val="90000"/><a:lumMod val="103000"/></a:schemeClr></a:gs><a:gs pos="100000"><a:schemeClr val="phClr"><a:shade val="63000"/><a:satMod val="120000"/></a:schemeClr></a:gs></a:gsLst><a:lin ang="5400000" scaled="0"/></a:gradFill>
      </a:bgFillStyleLst>
    </a:fmtScheme>
  </a:themeElements>
  <a:objectDefaults/>
  <a:extraClrSchemeLst/>
</a:theme>"#
        .to_string()
}

fn normalized_slides(spec: &OfficeCreateSpec) -> Vec<OfficeSlideSpec> {
    if !spec.slides.is_empty() {
        return spec.slides.clone();
    }
    vec![OfficeSlideSpec {
        title: spec.title.clone(),
        body: spec.body.clone(),
    }]
}

fn body_lines(body: &str) -> Vec<String> {
    body.lines()
        .map(|line| line.trim_end().to_string())
        .filter(|line| !line.trim().is_empty())
        .collect()
}

fn xml_escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| match character {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect::<Vec<_>>(),
            '>' => "&gt;".chars().collect::<Vec<_>>(),
            '"' => "&quot;".chars().collect::<Vec<_>>(),
            '\'' => "&apos;".chars().collect::<Vec<_>>(),
            other => vec![other],
        })
        .collect()
}

impl OfficeApp {
    pub fn from_label(value: &str) -> Option<Self> {
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.contains("powerpoint")
            || normalized.contains("power point")
            || normalized.contains("ppt")
            || normalized.contains("slides")
            || normalized.contains("presentation")
            || normalized.contains("演示")
            || normalized.contains("幻灯片")
        {
            Some(Self::PowerPoint)
        } else if normalized.contains("excel")
            || normalized.contains("xlsx")
            || normalized.contains("spreadsheet")
            || normalized.contains("workbook")
            || normalized.contains("表格")
            || normalized.contains("工作簿")
        {
            Some(Self::Excel)
        } else if normalized.contains("word")
            || normalized.contains("docx")
            || normalized.contains("document")
            || normalized.contains("文档")
        {
            Some(Self::Word)
        } else {
            None
        }
    }

    pub fn from_path(path: &str) -> Option<Self> {
        let lower = path.trim().to_ascii_lowercase();
        if lower.ends_with(".docx") {
            Some(Self::Word)
        } else if lower.ends_with(".xlsx") {
            Some(Self::Excel)
        } else if lower.ends_with(".pptx") {
            Some(Self::PowerPoint)
        } else {
            None
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Word => ".docx",
            Self::Excel => ".xlsx",
            Self::PowerPoint => ".pptx",
        }
    }

    fn artifact_kind(self) -> &'static str {
        match self {
            Self::Word => "word_document",
            Self::Excel => "excel_workbook",
            Self::PowerPoint => "powerpoint_deck",
        }
    }

    pub fn user_facing_name(self) -> &'static str {
        match self {
            Self::Word => "Word document",
            Self::Excel => "Excel workbook",
            Self::PowerPoint => "PowerPoint deck",
        }
    }

    pub fn desktop_app_name(self) -> &'static str {
        match self {
            Self::Word => "Microsoft Word",
            Self::Excel => "Microsoft Excel",
            Self::PowerPoint => "Microsoft PowerPoint",
        }
    }

    fn windows_executable_name(self) -> &'static str {
        match self {
            Self::Word => "WINWORD.EXE",
            Self::Excel => "EXCEL.EXE",
            Self::PowerPoint => "POWERPNT.EXE",
        }
    }

    fn unix_executable_names(self) -> &'static [&'static str] {
        match self {
            Self::Word => &["libreoffice", "lowriter"],
            Self::Excel => &["libreoffice", "localc"],
            Self::PowerPoint => &["libreoffice", "loimpress"],
        }
    }

    fn default_title(self) -> &'static str {
        match self {
            Self::Word => "DS Agent Word Document",
            Self::Excel => "DS Agent Excel Workbook",
            Self::PowerPoint => "DS Agent PowerPoint Deck",
        }
    }
}

fn parse_model_content(content: Option<&str>) -> Option<OfficeCreateModelContent> {
    let content = content?.trim();
    if !(content.starts_with('{') && content.ends_with('}')) {
        return None;
    }
    serde_json::from_str::<OfficeCreateModelContent>(content).ok()
}

fn plain_text_content(content: Option<&str>) -> Option<&str> {
    let content = content?.trim();
    if content.starts_with('{') && content.ends_with('}') {
        None
    } else {
        Some(content)
    }
}

fn model_rows(model: &OfficeCreateModelContent) -> Vec<Vec<String>> {
    if !model.rows.is_empty() {
        return value_rows_to_strings(&model.rows);
    }
    model
        .sheets
        .first()
        .map(|sheet| value_rows_to_strings(&sheet.rows))
        .unwrap_or_default()
}

fn model_slides(model: &OfficeCreateModelContent) -> Vec<OfficeSlideSpec> {
    model
        .slides
        .iter()
        .map(|slide| OfficeSlideSpec {
            title: slide.title.trim().to_string(),
            body: slide.body.trim().to_string(),
        })
        .filter(|slide| !slide.title.is_empty() || !slide.body.is_empty())
        .collect()
}

fn value_rows_to_strings(rows: &[Vec<Value>]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(value_to_cell_string).collect::<Vec<_>>())
        .filter(|row| row.iter().any(|cell| !cell.trim().is_empty()))
        .collect()
}

fn value_to_cell_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => value.to_string(),
    }
}

fn default_rows_for(app: OfficeApp, title: &str, body: &str) -> Vec<Vec<String>> {
    if app != OfficeApp::Excel {
        return Vec::new();
    }
    let body = body.trim();
    if body.is_empty() {
        return vec![vec![title.to_string()]];
    }
    normalized_rows_from_text(body)
}

fn default_slides_for(app: OfficeApp, title: &str, body: &str) -> Vec<OfficeSlideSpec> {
    if app != OfficeApp::PowerPoint {
        return Vec::new();
    }
    vec![OfficeSlideSpec {
        title: title.to_string(),
        body: body.to_string(),
    }]
}

fn office_update_summary(spec: &OfficeUpdateSpec) -> String {
    match spec.app {
        OfficeApp::Word => format!(
            "appended {} paragraph(s)",
            body_lines(&spec.body).len().max(1)
        ),
        OfficeApp::Excel => {
            let rows = if spec.rows.is_empty() {
                normalized_rows_from_text(&spec.body).len()
            } else {
                spec.rows.len()
            };
            format!("appended {} row(s)", rows.max(1))
        }
        OfficeApp::PowerPoint => {
            let slides = if spec.slides.is_empty() {
                default_slides_for(OfficeApp::PowerPoint, "Updated slide", &spec.body).len()
            } else {
                spec.slides.len()
            };
            format!("appended {} slide(s)", slides.max(1))
        }
    }
}

fn default_office_path(app: OfficeApp, title: &str) -> String {
    format!(
        "office/{}-{}{}",
        slugify(title).unwrap_or_else(|| "ds-agent-office".to_string()),
        Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("artifact"),
        app.extension()
    )
}

fn ensure_office_extension(path: &str, app: OfficeApp) -> Result<String, String> {
    let trimmed = path.trim().replace('\\', "/");
    if trimmed.to_ascii_lowercase().ends_with(app.extension()) {
        return Ok(trimmed);
    }
    if OfficeApp::from_path(&trimmed).is_some() {
        return Err(format!(
            "office_create target extension must match {}",
            app.user_facing_name()
        ));
    }
    Ok(format!("{trimmed}{}", app.extension()))
}

fn apply_office_target_location(path: &str, target_location: Option<&str>) -> String {
    let path = path.trim().replace('\\', "/");
    if path.starts_with("desktop/") || path.starts_with("workspace/") {
        return path;
    }
    match normalize_office_target_location(target_location).as_deref() {
        Some("desktop") => format!("desktop/{path}"),
        Some("workspace") | None => path,
        Some(_) => path,
    }
}

fn normalize_office_target_location(value: Option<&str>) -> Option<String> {
    let normalized = value?.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match normalized.as_str() {
        "desktop" | "user_desktop" | "windows_desktop" | "桌面" => Some("desktop".to_string()),
        "workspace" | "workdir" | "work_dir" | "work_root" | "local_workspace" | "工作区" => {
            Some("workspace".to_string())
        }
        _ => None,
    }
}

fn slugify(value: &str) -> Option<String> {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        None
    } else {
        Some(slug)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use zip::ZipArchive;

    use super::*;

    fn xml_text_between<'a>(xml: &'a str, start: &str, end: &str) -> Option<&'a str> {
        let after_start = xml.split_once(start)?.1;
        Some(after_start.split_once(end)?.0)
    }

    #[test]
    fn builds_valid_word_docx_package() {
        let spec = OfficeCreateSpec {
            app: OfficeApp::Word,
            path: "office/test.docx".to_string(),
            title: "测试文档".to_string(),
            body: "我在测试".to_string(),
            rows: Vec::new(),
            slides: Vec::new(),
        };

        let bytes = build_office_artifact(&spec).expect("docx is generated");
        let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("docx is a zip package");
        assert!(zip.by_name("[Content_Types].xml").is_ok());
        let mut document_xml = String::new();
        zip.by_name("word/document.xml")
            .expect("word document part exists")
            .read_to_string(&mut document_xml)
            .expect("document xml is readable");
        assert!(document_xml.contains("我在测试"));
    }

    #[test]
    fn word_docx_body_contains_only_requested_body_text() {
        let spec = OfficeCreateSpec {
            app: OfficeApp::Word,
            path: "office/test.docx".to_string(),
            title: "在桌面创建 Word 文档".to_string(),
            body: "测试完成".to_string(),
            rows: Vec::new(),
            slides: Vec::new(),
        };

        let bytes = build_office_artifact(&spec).expect("docx is generated");
        let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("docx is a zip package");
        let mut document_xml = String::new();
        zip.by_name("word/document.xml")
            .expect("word document part exists")
            .read_to_string(&mut document_xml)
            .expect("document xml is readable");

        assert!(document_xml.contains("测试完成"));
        assert!(!document_xml.contains("在桌面创建 Word 文档"));
    }

    #[test]
    fn word_docx_includes_core_word_support_parts() {
        let spec = OfficeCreateSpec {
            app: OfficeApp::Word,
            path: "office/test.docx".to_string(),
            title: "测试文档".to_string(),
            body: "测试完成".to_string(),
            rows: Vec::new(),
            slides: Vec::new(),
        };

        let bytes = build_office_artifact(&spec).expect("docx is generated");
        let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("docx is a zip package");

        assert!(zip.by_name("word/styles.xml").is_ok());
        assert!(zip.by_name("word/settings.xml").is_ok());
        assert!(zip.by_name("word/fontTable.xml").is_ok());
        assert!(zip.by_name("word/_rels/document.xml.rels").is_ok());
    }

    #[test]
    fn office_artifacts_core_properties_use_word_safe_second_precision_timestamps() {
        let specs = vec![
            OfficeCreateSpec {
                app: OfficeApp::Word,
                path: "office/test.docx".to_string(),
                title: "测试文档".to_string(),
                body: "测试完成".to_string(),
                rows: Vec::new(),
                slides: Vec::new(),
            },
            OfficeCreateSpec {
                app: OfficeApp::Excel,
                path: "office/test.xlsx".to_string(),
                title: "测试表格".to_string(),
                body: String::new(),
                rows: vec![vec!["项目".to_string(), "数值".to_string()]],
                slides: Vec::new(),
            },
            OfficeCreateSpec {
                app: OfficeApp::PowerPoint,
                path: "office/test.pptx".to_string(),
                title: "测试演示".to_string(),
                body: "测试完成".to_string(),
                rows: Vec::new(),
                slides: vec![OfficeSlideSpec {
                    title: "第一页".to_string(),
                    body: "测试完成".to_string(),
                }],
            },
        ];

        for spec in specs {
            let bytes = build_office_artifact(&spec).expect("office artifact is generated");
            let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("office artifact is a zip");
            let mut core_xml = String::new();
            zip.by_name("docProps/core.xml")
                .expect("core properties exist")
                .read_to_string(&mut core_xml)
                .expect("core properties are readable");

            let created = xml_text_between(
                &core_xml,
                "<dcterms:created xsi:type=\"dcterms:W3CDTF\">",
                "</dcterms:created>",
            )
            .expect("created timestamp exists");
            let modified = xml_text_between(
                &core_xml,
                "<dcterms:modified xsi:type=\"dcterms:W3CDTF\">",
                "</dcterms:modified>",
            )
            .expect("modified timestamp exists");

            assert!(!created.contains("+00:00"));
            assert!(!modified.contains("+00:00"));
            assert!(!created.contains('.'));
            assert!(!modified.contains('.'));
            assert!(core_xml.contains("<dcterms:created xsi:type=\"dcterms:W3CDTF\">"));
            assert!(created.ends_with('Z'));
            assert!(modified.ends_with('Z'));
        }
    }

    #[test]
    fn office_create_spec_applies_structured_desktop_location() {
        let spec = office_create_spec_from_action(
            "office_create",
            Some("测试文档"),
            Some("desktop"),
            Some("测试文档"),
            None,
            Some(r#"{"app":"word","body":"我在测试"}"#),
        )
        .expect("desktop word spec is created");

        assert_eq!(spec.app, OfficeApp::Word);
        assert_eq!(spec.path, "desktop/测试文档.docx");
        assert_eq!(spec.body, "我在测试");
    }

    #[test]
    fn local_office_client_writes_desktop_target_to_desktop_dir() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_dir = temp_dir.path().join("workspace");
        let desktop_dir = temp_dir.path().join("Desktop");
        let client = LocalOfficeArtifactClient::new_with_desktop_dir(
            workspace_dir.clone(),
            8 * 1024 * 1024,
            Some(desktop_dir.clone()),
        );
        let spec = OfficeCreateSpec {
            app: OfficeApp::Word,
            path: "desktop/测试文档.docx".to_string(),
            title: "测试文档".to_string(),
            body: "我在测试".to_string(),
            rows: Vec::new(),
            slides: Vec::new(),
        };

        let result = client
            .write_office_artifact(&spec)
            .expect("desktop office artifact is written");

        assert!(desktop_dir.join("测试文档.docx").is_file());
        assert!(!workspace_dir.join("测试文档.docx").exists());
        assert!(result.path.contains("Desktop"));
    }

    #[cfg(windows)]
    #[test]
    fn office_shell_compatible_path_strips_windows_extended_length_prefix() {
        let launch_path = office_shell_compatible_path(Path::new(r"\\?\D:\Desktop\测试表格.xlsx"));

        assert_eq!(launch_path.to_string_lossy(), r"D:\Desktop\测试表格.xlsx");
    }

    #[test]
    fn builds_valid_excel_xlsx_package() {
        let spec = OfficeCreateSpec {
            app: OfficeApp::Excel,
            path: "office/test.xlsx".to_string(),
            title: "测试表格".to_string(),
            body: String::new(),
            rows: vec![
                vec!["项目".to_string(), "数值".to_string()],
                vec!["测试".to_string(), "1".to_string()],
            ],
            slides: Vec::new(),
        };

        let bytes = build_office_artifact(&spec).expect("xlsx is generated");
        let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("xlsx is a zip package");
        assert!(zip.by_name("xl/workbook.xml").is_ok());
        let mut sheet_xml = String::new();
        zip.by_name("xl/worksheets/sheet1.xml")
            .expect("worksheet exists")
            .read_to_string(&mut sheet_xml)
            .expect("worksheet xml is readable");
        assert!(sheet_xml.contains("测试"));
    }

    #[test]
    fn builds_excel_formula_cells_as_formulas() {
        let spec = OfficeCreateSpec {
            app: OfficeApp::Excel,
            path: "office/formula.xlsx".to_string(),
            title: "Formula workbook".to_string(),
            body: String::new(),
            rows: vec![
                vec!["Item".to_string(), "Value".to_string()],
                vec!["A".to_string(), "1".to_string()],
                vec!["B".to_string(), "2".to_string()],
                vec!["Total".to_string(), "=SUM(B2:B3)".to_string()],
            ],
            slides: Vec::new(),
        };

        let bytes = build_office_artifact(&spec).expect("xlsx is generated");
        let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("xlsx is a zip package");
        let mut sheet_xml = String::new();
        zip.by_name("xl/worksheets/sheet1.xml")
            .expect("worksheet exists")
            .read_to_string(&mut sheet_xml)
            .expect("worksheet xml is readable");

        assert!(sheet_xml.contains(r#"<c r="B4"><f>SUM(B2:B3)</f></c>"#));
        assert!(!sheet_xml.contains(r#"<t>=SUM(B2:B3)</t>"#));
    }

    #[test]
    fn builds_valid_powerpoint_pptx_package() {
        let spec = OfficeCreateSpec {
            app: OfficeApp::PowerPoint,
            path: "office/test.pptx".to_string(),
            title: "测试演示".to_string(),
            body: "我在测试".to_string(),
            rows: Vec::new(),
            slides: vec![OfficeSlideSpec {
                title: "第一页".to_string(),
                body: "我在测试".to_string(),
            }],
        };

        let bytes = build_office_artifact(&spec).expect("pptx is generated");
        let mut zip = ZipArchive::new(Cursor::new(bytes)).expect("pptx is a zip package");
        assert!(zip.by_name("ppt/presentation.xml").is_ok());
        let mut slide_xml = String::new();
        zip.by_name("ppt/slides/slide1.xml")
            .expect("slide exists")
            .read_to_string(&mut slide_xml)
            .expect("slide xml is readable");
        assert!(slide_xml.contains("我在测试"));

        let mut theme_xml = String::new();
        zip.by_name("ppt/theme/theme1.xml")
            .expect("theme exists")
            .read_to_string(&mut theme_xml)
            .expect("theme xml is readable");
        assert!(theme_xml.contains("<a:objectDefaults/>"));
        assert!(theme_xml.contains("<a:extraClrSchemeLst/>"));
        assert!(theme_xml.matches("<a:ln ").count() >= 3);
        assert!(theme_xml.matches("<a:effectStyle>").count() >= 3);
    }

    #[test]
    fn updates_word_docx_by_appending_paragraphs() {
        let create_spec = OfficeCreateSpec {
            app: OfficeApp::Word,
            path: "office/test.docx".to_string(),
            title: "测试文档".to_string(),
            body: "第一段".to_string(),
            rows: Vec::new(),
            slides: Vec::new(),
        };
        let update_spec = OfficeUpdateSpec {
            app: OfficeApp::Word,
            path: "office/test.docx".to_string(),
            body: "第二段".to_string(),
            rows: Vec::new(),
            slides: Vec::new(),
        };

        let bytes = build_office_artifact(&create_spec).expect("docx is generated");
        let updated = update_office_artifact_bytes(&bytes, &update_spec).expect("docx is updated");
        let mut zip = ZipArchive::new(Cursor::new(updated)).expect("updated docx is a zip package");
        let mut document_xml = String::new();
        zip.by_name("word/document.xml")
            .expect("word document part exists")
            .read_to_string(&mut document_xml)
            .expect("document xml is readable");

        assert!(document_xml.contains("第一段"));
        assert!(document_xml.contains("第二段"));
    }

    #[test]
    fn updates_excel_xlsx_by_appending_rows_with_formulas() {
        let create_spec = OfficeCreateSpec {
            app: OfficeApp::Excel,
            path: "office/test.xlsx".to_string(),
            title: "测试表格".to_string(),
            body: String::new(),
            rows: vec![
                vec!["项目".to_string(), "数值".to_string()],
                vec!["测试".to_string(), "1".to_string()],
            ],
            slides: Vec::new(),
        };
        let update_spec = OfficeUpdateSpec {
            app: OfficeApp::Excel,
            path: "office/test.xlsx".to_string(),
            body: String::new(),
            rows: vec![vec!["合计".to_string(), "=SUM(B2:B2)".to_string()]],
            slides: Vec::new(),
        };

        let bytes = build_office_artifact(&create_spec).expect("xlsx is generated");
        let updated = update_office_artifact_bytes(&bytes, &update_spec).expect("xlsx is updated");
        let mut zip = ZipArchive::new(Cursor::new(updated)).expect("updated xlsx is a zip package");
        let mut sheet_xml = String::new();
        zip.by_name("xl/worksheets/sheet1.xml")
            .expect("worksheet exists")
            .read_to_string(&mut sheet_xml)
            .expect("worksheet xml is readable");

        assert!(sheet_xml.contains(r#"<dimension ref="A1:B3"/>"#));
        assert!(sheet_xml.contains(r#"<row r="3">"#));
        assert!(sheet_xml.contains(r#"<c r="B3"><f>SUM(B2:B2)</f></c>"#));
    }

    #[test]
    fn updates_powerpoint_pptx_by_appending_slide() {
        let create_spec = OfficeCreateSpec {
            app: OfficeApp::PowerPoint,
            path: "office/test.pptx".to_string(),
            title: "测试演示".to_string(),
            body: "第一页内容".to_string(),
            rows: Vec::new(),
            slides: vec![OfficeSlideSpec {
                title: "第一页".to_string(),
                body: "第一页内容".to_string(),
            }],
        };
        let update_spec = OfficeUpdateSpec {
            app: OfficeApp::PowerPoint,
            path: "office/test.pptx".to_string(),
            body: String::new(),
            rows: Vec::new(),
            slides: vec![OfficeSlideSpec {
                title: "第二页".to_string(),
                body: "第二页内容".to_string(),
            }],
        };

        let bytes = build_office_artifact(&create_spec).expect("pptx is generated");
        let updated = update_office_artifact_bytes(&bytes, &update_spec).expect("pptx is updated");
        let mut zip = ZipArchive::new(Cursor::new(updated)).expect("updated pptx is a zip package");
        assert!(zip.by_name("ppt/slides/slide2.xml").is_ok());
        let mut slide_xml = String::new();
        zip.by_name("ppt/slides/slide2.xml")
            .expect("slide2 exists")
            .read_to_string(&mut slide_xml)
            .expect("slide2 xml is readable");
        assert!(slide_xml.contains("第二页"));
    }
}
