use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub const SKILL_MANIFEST_SCHEMA_VERSION: &str = "ds-agent.skill.v1";
pub const MAX_REMOTE_SKILL_PACKAGE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_SKILL_ENTRY_BYTES: usize = 32 * 1024;

#[derive(Debug, Error)]
pub enum SkillManifestError {
    #[error("invalid skill manifest json: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("unsupported skill manifest schema version: {0}")]
    UnsupportedSchemaVersion(String),

    #[error("skill manifest field is required: {0}")]
    MissingField(&'static str),

    #[error("skill entry kind is blocked by default: {0}")]
    BlockedEntryKind(String),

    #[error("skill permission is blocked by default: {0}")]
    BlockedPermission(String),

    #[error("skill source integrity is required")]
    MissingIntegrity,

    #[error("skill source is blocked by default: {0}")]
    BlockedSource(String),

    #[error("skill package entry file is missing: {0}")]
    MissingEntryFile(String),

    #[error("skill package contains blocked files: {0}")]
    BlockedPackageFiles(String),

    #[error("skill package manifest is missing")]
    MissingPackageManifest,

    #[error("invalid skill package archive: {0}")]
    InvalidPackage(String),

    #[error("skill entry integrity mismatch: expected {expected}, got {actual}")]
    IntegrityMismatch { expected: String, actual: String },
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrustLevel {
    #[default]
    Untrusted,
    LocalDeclarative,
    RemoteDeclarative,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillEnablementStatus {
    Enabled,
    Disabled,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillPackageKind {
    #[default]
    Skill,
    Plugin,
    SystemSkill,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillUpdatePolicy {
    #[default]
    Automatic,
    Pinned,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillUpdateState {
    #[default]
    Current,
    UpdateAvailable,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillUpdateCheckStatus {
    Current,
    UpdateAvailable,
    Updated,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillSourceIntegrity {
    pub algorithm: String,
    pub hash: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillSource {
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub integrity: Option<SkillSourceIntegrity>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillSourceIdentity {
    pub provider: String,
    pub repository_url: String,
    #[serde(default)]
    pub requested_revision: Option<String>,
    pub resolved_revision: String,
    #[serde(default)]
    pub package_path: Option<String>,
    pub source_format: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillPermissionDeclaration {
    pub kind: String,
    pub scope: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillEntry {
    pub kind: String,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillManifest {
    pub schema_version: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub source: SkillSource,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub permissions: Vec<SkillPermissionDeclaration>,
    pub entry: SkillEntry,
    #[serde(default)]
    pub trust_level: SkillTrustLevel,
    #[serde(default)]
    pub risk_warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillInstallationRecord {
    pub id: Uuid,
    pub manifest: SkillManifest,
    pub installed_from: String,
    pub installed_at: DateTime<Utc>,
    #[serde(default)]
    pub package_kind: SkillPackageKind,
    #[serde(default)]
    pub system_protected: bool,
    #[serde(default)]
    pub source_identity: Option<SkillSourceIdentity>,
    #[serde(default)]
    pub update_policy: SkillUpdatePolicy,
    #[serde(default)]
    pub entry_content: Option<String>,
    #[serde(default)]
    pub entry_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillEnablementChange {
    pub id: Uuid,
    pub skill_id: Uuid,
    pub status: SkillEnablementStatus,
    pub note: String,
    pub changed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillTrustReset {
    pub id: Uuid,
    pub skill_id: Uuid,
    pub note: String,
    pub reset_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillUninstallRecord {
    pub id: Uuid,
    pub skill_id: Uuid,
    pub note: String,
    pub uninstalled_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillUpdateRecord {
    pub id: Uuid,
    pub skill_id: Uuid,
    pub manifest: SkillManifest,
    pub updated_from: String,
    pub applied_at: DateTime<Utc>,
    pub source_identity: Option<SkillSourceIdentity>,
    pub previous_version: String,
    pub previous_revision: Option<String>,
    pub entry_content: String,
    pub entry_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillUpdateCheckRecord {
    pub id: Uuid,
    pub skill_id: Uuid,
    pub installed_revision: Option<String>,
    pub latest_revision: Option<String>,
    pub status: SkillUpdateCheckStatus,
    pub note: String,
    pub checked_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillUpdateFailureRecord {
    pub id: Uuid,
    pub skill_id: Uuid,
    pub candidate_revision: Option<String>,
    pub error: String,
    pub failed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillRecord {
    pub id: Uuid,
    pub manifest: SkillManifest,
    pub installed_from: String,
    pub installed_at: DateTime<Utc>,
    pub enablement_status: SkillEnablementStatus,
    pub last_audit_note: Option<String>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub package_kind: SkillPackageKind,
    #[serde(default)]
    pub system_protected: bool,
    #[serde(default)]
    pub source_identity: Option<SkillSourceIdentity>,
    #[serde(default)]
    pub update_policy: SkillUpdatePolicy,
    #[serde(default)]
    pub update_state: SkillUpdateState,
    #[serde(default)]
    pub last_update_checked_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_update_failure: Option<String>,
    #[serde(default)]
    pub rollback_version: Option<String>,
    #[serde(default)]
    pub rollback_revision: Option<String>,
    #[serde(default)]
    pub entry_available: bool,
    #[serde(default)]
    pub entry_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillPackagePreflight {
    pub manifest: SkillManifest,
    pub package_files: Vec<String>,
    pub blocked_files: Vec<String>,
    pub warnings: Vec<String>,
    pub audit_summary: String,
    #[serde(skip)]
    pub entry_content: Option<String>,
    #[serde(default)]
    pub entry_sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillActivationContext {
    pub skill_id: Uuid,
    pub skill_name: String,
    pub skill_version: String,
    pub entry_kind: String,
    pub entry_path: String,
    pub entry_sha256: String,
    pub input_summary: String,
    pub instructions: String,
    pub capability_summary: String,
    pub permission_summary: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillSourceVerification {
    pub verified: bool,
    pub source_kind: String,
    pub source_url: String,
    pub integrity_algorithm: Option<String>,
    pub integrity_hash: Option<String>,
    pub provenance: String,
    pub checked_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillExecutionStatus {
    Planned,
    Blocked,
    Activated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillExecutionRecord {
    pub id: Uuid,
    pub skill_id: Uuid,
    pub skill_name: String,
    pub status: SkillExecutionStatus,
    pub entry_kind: String,
    pub entry_path: String,
    pub input_summary: String,
    pub execution_plan: String,
    pub blocked_reason: Option<String>,
    pub requested_at: DateTime<Utc>,
    #[serde(default)]
    pub tool_invocation_id: Option<Uuid>,
    #[serde(default)]
    pub run_id: Option<Uuid>,
    #[serde(default)]
    pub evidence_ref: Option<String>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

impl SkillManifest {
    pub fn from_json(value: &str) -> Result<Self, SkillManifestError> {
        let mut manifest = serde_json::from_str::<Self>(value)?;
        manifest.normalize();
        manifest.validate()?;
        manifest.trust_level = manifest_trust_level(&manifest);
        manifest.risk_warnings = skill_manifest_warnings(&manifest);
        Ok(manifest)
    }

    fn normalize(&mut self) {
        self.schema_version = self.schema_version.trim().to_string();
        self.name = self.name.trim().to_string();
        self.version = self.version.trim().to_string();
        self.description = self.description.trim().to_string();
        self.author = self.author.trim().to_string();
        self.license = self.license.trim().to_string();
        self.source.kind = self.source.kind.trim().to_ascii_lowercase();
        self.source.url = self.source.url.trim().to_string();
        if let Some(integrity) = &mut self.source.integrity {
            integrity.algorithm = integrity.algorithm.trim().to_ascii_lowercase();
            integrity.hash = integrity.hash.trim().to_string();
        }
        self.capabilities = self
            .capabilities
            .iter()
            .map(|capability| capability.trim().to_ascii_lowercase())
            .filter(|capability| !capability.is_empty())
            .collect();
        for permission in &mut self.permissions {
            permission.kind = permission.kind.trim().to_ascii_lowercase();
            permission.scope = permission.scope.trim().to_ascii_lowercase();
            permission.reason = permission.reason.trim().to_string();
        }
        self.entry.kind = self.entry.kind.trim().to_ascii_lowercase();
        self.entry.path = self.entry.path.trim().to_string();
    }

    fn validate(&self) -> Result<(), SkillManifestError> {
        if self.schema_version != SKILL_MANIFEST_SCHEMA_VERSION {
            return Err(SkillManifestError::UnsupportedSchemaVersion(
                self.schema_version.clone(),
            ));
        }
        require_field(&self.name, "name")?;
        require_field(&self.version, "version")?;
        require_field(&self.description, "description")?;
        require_field(&self.author, "author")?;
        require_field(&self.license, "license")?;
        require_field(&self.source.kind, "source.kind")?;
        require_field(&self.source.url, "source.url")?;
        require_field(&self.entry.path, "entry.path")?;
        validate_source(&self.source)?;
        validate_integrity(&self.source)?;
        validate_entry_kind(&self.entry.kind)?;
        for permission in &self.permissions {
            validate_permission(permission)?;
        }
        Ok(())
    }
}

impl SkillInstallationRecord {
    pub fn new(manifest: SkillManifest, installed_from: String) -> Result<Self, String> {
        let installed_from = installed_from.trim().to_string();
        if installed_from.is_empty() {
            return Err("skill installed_from is required".to_string());
        }
        Ok(Self {
            id: Uuid::new_v4(),
            manifest,
            installed_from,
            installed_at: Utc::now(),
            package_kind: SkillPackageKind::Skill,
            system_protected: false,
            source_identity: None,
            update_policy: SkillUpdatePolicy::Automatic,
            entry_content: None,
            entry_sha256: None,
        })
    }

    pub fn from_preflight(
        preflight: SkillPackagePreflight,
        installed_from: String,
    ) -> Result<Self, String> {
        let entry_content = preflight.entry_content.ok_or_else(|| {
            "skill package entry content is required for installation".to_string()
        })?;
        let entry_sha256 = preflight
            .entry_sha256
            .ok_or_else(|| "skill package entry hash is required for installation".to_string())?;
        let mut installation = Self::new(preflight.manifest, installed_from)?;
        installation.entry_content = Some(entry_content);
        installation.entry_sha256 = Some(entry_sha256);
        Ok(installation)
    }
}

impl SkillEnablementChange {
    pub fn new(
        skill_id: Uuid,
        status: SkillEnablementStatus,
        note: String,
    ) -> Result<Self, String> {
        let note = note.trim().to_string();
        if note.is_empty() {
            return Err("skill enablement audit note is required".to_string());
        }
        Ok(Self {
            id: Uuid::new_v4(),
            skill_id,
            status,
            note,
            changed_at: Utc::now(),
        })
    }
}

impl SkillTrustReset {
    pub fn new(skill_id: Uuid, note: String) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            skill_id,
            note: required_string(note, "skill trust reset audit note")?,
            reset_at: Utc::now(),
        })
    }
}

impl SkillUninstallRecord {
    pub fn new(skill_id: Uuid, note: String) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            skill_id,
            note: required_string(note, "skill uninstall audit note")?,
            uninstalled_at: Utc::now(),
        })
    }
}

impl SkillUpdateRecord {
    pub fn from_candidate(
        skill_id: Uuid,
        previous_version: String,
        previous_revision: Option<String>,
        candidate: SkillInstallationRecord,
    ) -> Result<Self, String> {
        let entry_content = candidate
            .entry_content
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| "skill update candidate entry content is required".to_string())?;
        let entry_sha256 = candidate
            .entry_sha256
            .filter(|hash| !hash.trim().is_empty())
            .ok_or_else(|| "skill update candidate entry hash is required".to_string())?;
        Ok(Self {
            id: Uuid::new_v4(),
            skill_id,
            manifest: candidate.manifest,
            updated_from: required_string(candidate.installed_from, "skill update source")?,
            applied_at: Utc::now(),
            source_identity: candidate.source_identity,
            previous_version: required_string(previous_version, "previous skill version")?,
            previous_revision,
            entry_content,
            entry_sha256,
        })
    }
}

impl SkillUpdateCheckRecord {
    pub fn new(
        skill_id: Uuid,
        installed_revision: Option<String>,
        latest_revision: Option<String>,
        status: SkillUpdateCheckStatus,
        note: String,
    ) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            skill_id,
            installed_revision,
            latest_revision,
            status,
            note: required_string(note, "skill update check note")?,
            checked_at: Utc::now(),
        })
    }
}

impl SkillUpdateFailureRecord {
    pub fn new(
        skill_id: Uuid,
        candidate_revision: Option<String>,
        error: String,
    ) -> Result<Self, String> {
        Ok(Self {
            id: Uuid::new_v4(),
            skill_id,
            candidate_revision,
            error: required_string(error, "skill update failure")?,
            failed_at: Utc::now(),
        })
    }
}

impl SkillPackagePreflight {
    pub fn from_manifest_and_files(
        manifest_json: &str,
        package_files: &[String],
    ) -> Result<Self, SkillManifestError> {
        let manifest = SkillManifest::from_json(manifest_json)?;
        let package_files = normalize_package_files(package_files);
        let entry_path = normalize_package_path(&manifest.entry.path);
        if !package_files.iter().any(|path| path == &entry_path) {
            return Err(SkillManifestError::MissingEntryFile(
                manifest.entry.path.clone(),
            ));
        }

        let blocked_files = blocked_package_files(&package_files);
        if !blocked_files.is_empty() {
            return Err(SkillManifestError::BlockedPackageFiles(
                blocked_files.join(", "),
            ));
        }

        let mut warnings = manifest.risk_warnings.clone();
        warnings.push("package preflight does not execute skill code".to_string());
        let audit_summary = format!(
            "package preflight accepted {} files without executable payloads",
            package_files.len()
        );

        Ok(Self {
            manifest,
            package_files,
            blocked_files,
            warnings,
            audit_summary,
            entry_content: None,
            entry_sha256: None,
        })
    }

    pub fn from_zip_bytes(bytes: &[u8]) -> Result<Self, SkillManifestError> {
        use std::io::{Cursor, Read};

        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec()))
            .map_err(|error| SkillManifestError::InvalidPackage(error.to_string()))?;
        let mut package_files = Vec::new();
        let mut manifest_json = None;

        for index in 0..archive.len() {
            let mut file = archive
                .by_index(index)
                .map_err(|error| SkillManifestError::InvalidPackage(error.to_string()))?;
            let path = normalize_package_path(file.name());
            if path.is_empty() {
                continue;
            }
            let is_manifest = matches!(path.as_str(), "skill.json" | "manifest.json");
            package_files.push(path);
            if is_manifest {
                let mut value = String::new();
                file.read_to_string(&mut value)
                    .map_err(|error| SkillManifestError::InvalidPackage(error.to_string()))?;
                manifest_json = Some(value);
            }
        }

        let manifest_json = manifest_json.ok_or(SkillManifestError::MissingPackageManifest)?;
        let mut preflight = Self::from_manifest_and_files(&manifest_json, &package_files)?;
        let entry_path = normalize_package_path(&preflight.manifest.entry.path);
        let mut entry_content = None;
        for index in 0..archive.len() {
            let mut file = archive
                .by_index(index)
                .map_err(|error| SkillManifestError::InvalidPackage(error.to_string()))?;
            if normalize_package_path(file.name()) != entry_path {
                continue;
            }
            if file.size() > MAX_SKILL_ENTRY_BYTES as u64 {
                return Err(SkillManifestError::InvalidPackage(format!(
                    "skill entry is too large: {} bytes",
                    file.size()
                )));
            }
            let mut value = String::new();
            file.read_to_string(&mut value)
                .map_err(|error| SkillManifestError::InvalidPackage(error.to_string()))?;
            entry_content = Some(value);
            break;
        }
        let entry_content = entry_content.ok_or_else(|| {
            SkillManifestError::MissingEntryFile(preflight.manifest.entry.path.clone())
        })?;
        preflight.entry_sha256 = Some(sha256_hex(entry_content.as_bytes()));
        preflight.entry_content = Some(entry_content);
        Ok(preflight)
    }

    pub fn from_remote_zip_bytes(
        source_url: &str,
        bytes: &[u8],
    ) -> Result<Self, SkillManifestError> {
        let source_url = source_url.trim();
        validate_remote_skill_source_url(source_url)?;
        if bytes.len() > MAX_REMOTE_SKILL_PACKAGE_BYTES {
            return Err(SkillManifestError::InvalidPackage(format!(
                "remote skill package is too large: {} bytes",
                bytes.len()
            )));
        }

        let mut preflight = Self::from_zip_bytes(bytes)?;
        if preflight.manifest.source.url != source_url {
            return Err(SkillManifestError::BlockedSource(format!(
                "remote source mismatch: manifest={} requested={}",
                preflight.manifest.source.url, source_url
            )));
        }
        if !matches!(
            preflight.manifest.source.kind.as_str(),
            "github" | "huggingface" | "hugging_face"
        ) {
            return Err(SkillManifestError::BlockedSource(format!(
                "{}:{}",
                preflight.manifest.source.kind, preflight.manifest.source.url
            )));
        }
        let expected_hash = preflight
            .manifest
            .source
            .integrity
            .as_ref()
            .map(|integrity| integrity.hash.trim().to_ascii_lowercase())
            .ok_or(SkillManifestError::MissingIntegrity)?;
        let actual_hash = preflight.entry_sha256.as_ref().cloned().ok_or_else(|| {
            SkillManifestError::MissingEntryFile(preflight.manifest.entry.path.clone())
        })?;
        if expected_hash != actual_hash {
            return Err(SkillManifestError::IntegrityMismatch {
                expected: expected_hash,
                actual: actual_hash,
            });
        }

        preflight.warnings.push(
            "remote package was previewed only; installation requires explicit user action"
                .to_string(),
        );
        Ok(preflight)
    }
}

impl SkillSourceVerification {
    pub fn for_manifest(manifest: &SkillManifest) -> Result<Self, SkillManifestError> {
        validate_source(&manifest.source)?;
        validate_integrity(&manifest.source)?;
        let integrity = manifest.source.integrity.as_ref();
        Ok(Self {
            verified: true,
            source_kind: manifest.source.kind.clone(),
            source_url: manifest.source.url.clone(),
            integrity_algorithm: integrity.map(|value| value.algorithm.clone()),
            integrity_hash: integrity.map(|value| value.hash.clone()),
            provenance: source_provenance(&manifest.source)?,
            checked_at: Utc::now(),
        })
    }
}

impl SkillExecutionRecord {
    pub fn for_skill(record: &SkillRecord, input_summary: String) -> Result<Self, String> {
        let input_summary = required_string(input_summary, "skill execution input summary")?;
        let blocked_reason = skill_execution_blocked_reason(record);
        let status = if blocked_reason.is_some() {
            SkillExecutionStatus::Blocked
        } else {
            SkillExecutionStatus::Planned
        };
        let execution_plan = if status == SkillExecutionStatus::Planned {
            declarative_skill_execution_plan(record, &input_summary)
        } else {
            "Skill execution blocked before any skill content was run.".to_string()
        };

        Ok(Self {
            id: Uuid::new_v4(),
            skill_id: record.id,
            skill_name: record.manifest.name.clone(),
            status,
            entry_kind: record.manifest.entry.kind.clone(),
            entry_path: record.manifest.entry.path.clone(),
            input_summary,
            execution_plan,
            blocked_reason,
            requested_at: Utc::now(),
            tool_invocation_id: None,
            run_id: None,
            evidence_ref: None,
            completed_at: None,
        })
    }

    pub fn activated(
        activation: &SkillActivationContext,
        tool_invocation_id: Uuid,
        run_id: Option<Uuid>,
        evidence_ref: String,
    ) -> Result<Self, String> {
        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            skill_id: activation.skill_id,
            skill_name: activation.skill_name.clone(),
            status: SkillExecutionStatus::Activated,
            entry_kind: activation.entry_kind.clone(),
            entry_path: activation.entry_path.clone(),
            input_summary: activation.input_summary.clone(),
            execution_plan: format!(
                "Activated hash-verified declarative entry {} for the bounded DS Agent loop; subsequent tools remain independently permissioned and verified.",
                activation.entry_path
            ),
            blocked_reason: None,
            requested_at: now,
            tool_invocation_id: Some(tool_invocation_id),
            run_id,
            evidence_ref: Some(required_string(evidence_ref, "skill activation evidence ref")?),
            completed_at: Some(now),
        })
    }
}

impl SkillActivationContext {
    pub fn for_installation(
        record: &SkillRecord,
        installation: &SkillInstallationRecord,
        input_summary: String,
    ) -> Result<Self, String> {
        if record.id != installation.id {
            return Err("skill activation record does not match installation".to_string());
        }
        if let Some(reason) = skill_execution_blocked_reason(record) {
            return Err(reason);
        }
        let input_summary = required_string(input_summary, "skill activation input summary")?;
        let instructions = installation
            .entry_content
            .as_ref()
            .filter(|content| !content.trim().is_empty())
            .ok_or_else(|| "skill activation requires installed entry content".to_string())?
            .clone();
        let entry_sha256 = installation
            .entry_sha256
            .as_ref()
            .filter(|hash| !hash.trim().is_empty())
            .ok_or_else(|| "skill activation requires an installed entry hash".to_string())?
            .to_ascii_lowercase();
        let actual_sha256 = sha256_hex(instructions.as_bytes());
        if actual_sha256 != entry_sha256 {
            return Err(format!(
                "skill entry integrity check failed: expected {entry_sha256}, got {actual_sha256}"
            ));
        }
        let capability_summary = if record.manifest.capabilities.is_empty() {
            "none".to_string()
        } else {
            record.manifest.capabilities.join(", ")
        };
        let permission_summary = if record.manifest.permissions.is_empty() {
            "none".to_string()
        } else {
            record
                .manifest
                .permissions
                .iter()
                .map(|permission| format!("{}:{}", permission.kind, permission.scope))
                .collect::<Vec<_>>()
                .join(", ")
        };
        Ok(Self {
            skill_id: record.id,
            skill_name: record.manifest.name.clone(),
            skill_version: record.manifest.version.clone(),
            entry_kind: record.manifest.entry.kind.clone(),
            entry_path: record.manifest.entry.path.clone(),
            entry_sha256,
            input_summary,
            instructions,
            capability_summary,
            permission_summary,
        })
    }
}

fn skill_execution_blocked_reason(record: &SkillRecord) -> Option<String> {
    if record.enablement_status == SkillEnablementStatus::Disabled {
        return Some("skill is disabled".to_string());
    }
    if record.manifest.trust_level == SkillTrustLevel::Untrusted {
        return Some("skill trust is unverified".to_string());
    }
    if !matches!(
        record.manifest.entry.kind.as_str(),
        "declarative_workflow" | "prompt_pack" | "context_pack"
    ) {
        return Some(format!(
            "skill entry kind is not executable in the safe runtime: {}",
            record.manifest.entry.kind
        ));
    }
    None
}

fn declarative_skill_execution_plan(record: &SkillRecord, input_summary: &str) -> String {
    let capabilities = if record.manifest.capabilities.is_empty() {
        "none".to_string()
    } else {
        record.manifest.capabilities.join(", ")
    };
    let permissions = if record.manifest.permissions.is_empty() {
        "none".to_string()
    } else {
        record
            .manifest
            .permissions
            .iter()
            .map(|permission| format!("{}:{}", permission.kind, permission.scope))
            .collect::<Vec<_>>()
            .join(", ")
    };

    format!(
        "Prepare declarative skill '{}' v{} from entry {}. Input: {}. Capabilities: {}. Declared permissions: {}. No script, binary, native module, terminal, or hidden network code is executed by this safe runtime plan.",
        record.manifest.name,
        record.manifest.version,
        record.manifest.entry.path,
        input_summary,
        capabilities,
        permissions
    )
}

fn require_field(value: &str, field: &'static str) -> Result<(), SkillManifestError> {
    if value.trim().is_empty() {
        return Err(SkillManifestError::MissingField(field));
    }
    Ok(())
}

fn required_string(value: String, field: &'static str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{field} is required"));
    }
    Ok(value)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn validate_integrity(source: &SkillSource) -> Result<(), SkillManifestError> {
    let Some(integrity) = &source.integrity else {
        return Err(SkillManifestError::MissingIntegrity);
    };
    if integrity.algorithm != "sha256" || integrity.hash.is_empty() {
        return Err(SkillManifestError::MissingIntegrity);
    }
    Ok(())
}

fn validate_source(source: &SkillSource) -> Result<(), SkillManifestError> {
    let url = source.url.to_ascii_lowercase();
    match source.kind.as_str() {
        "local" if url.starts_with("file://") => Ok(()),
        "github" if url.starts_with("https://github.com/") => Ok(()),
        "huggingface" | "hugging_face" if url.starts_with("https://huggingface.co/") => Ok(()),
        kind => Err(SkillManifestError::BlockedSource(format!(
            "{kind}:{}",
            source.url
        ))),
    }
}

pub fn validate_remote_skill_source_url(source_url: &str) -> Result<(), SkillManifestError> {
    let url = source_url.trim().to_ascii_lowercase();
    if url.starts_with("https://github.com/") || url.starts_with("https://huggingface.co/") {
        Ok(())
    } else {
        Err(SkillManifestError::BlockedSource(source_url.to_string()))
    }
}

fn source_provenance(source: &SkillSource) -> Result<String, SkillManifestError> {
    let without_scheme = source
        .url
        .strip_prefix("https://")
        .or_else(|| source.url.strip_prefix("file://"))
        .unwrap_or(&source.url);
    let mut parts = without_scheme.split('/').filter(|part| !part.is_empty());
    match source.kind.as_str() {
        "local" => Ok(format!("local:{}", without_scheme.trim_start_matches('/'))),
        "github" => {
            let host = parts.next().unwrap_or_default();
            let owner = parts.next().unwrap_or_default();
            let repo = parts.next().unwrap_or_default();
            if host == "github.com" && !owner.is_empty() && !repo.is_empty() {
                Ok(format!("github.com/{owner}/{repo}"))
            } else {
                Err(SkillManifestError::BlockedSource(source.url.clone()))
            }
        }
        "huggingface" | "hugging_face" => {
            let host = parts.next().unwrap_or_default();
            let owner = parts.next().unwrap_or_default();
            let repo = parts.next().unwrap_or_default();
            if host == "huggingface.co" && !owner.is_empty() && !repo.is_empty() {
                Ok(format!("huggingface.co/{owner}/{repo}"))
            } else {
                Err(SkillManifestError::BlockedSource(source.url.clone()))
            }
        }
        _ => Err(SkillManifestError::BlockedSource(source.url.clone())),
    }
}

fn validate_entry_kind(kind: &str) -> Result<(), SkillManifestError> {
    match kind {
        "declarative_workflow" | "prompt_pack" | "context_pack" => Ok(()),
        "script" | "binary" | "native_module" | "node_module" => {
            Err(SkillManifestError::BlockedEntryKind(kind.to_string()))
        }
        _ => Err(SkillManifestError::BlockedEntryKind(kind.to_string())),
    }
}

fn validate_permission(permission: &SkillPermissionDeclaration) -> Result<(), SkillManifestError> {
    match permission.kind.as_str() {
        "file_read" if matches!(permission.scope.as_str(), "user_selected" | "workspace") => Ok(()),
        "network_search" | "browser_open" | "memory_candidate_review" => Ok(()),
        "file_write"
        | "terminal_write"
        | "email_send"
        | "drive_write"
        | "browser_submit"
        | "computer_control"
        | "native_code"
        | "unrestricted_network" => Err(SkillManifestError::BlockedPermission(
            permission.kind.clone(),
        )),
        _ => Err(SkillManifestError::BlockedPermission(
            permission.kind.clone(),
        )),
    }
}

fn manifest_trust_level(manifest: &SkillManifest) -> SkillTrustLevel {
    if manifest.source.kind == "local" && manifest.entry.kind == "declarative_workflow" {
        SkillTrustLevel::LocalDeclarative
    } else {
        SkillTrustLevel::RemoteDeclarative
    }
}

fn skill_manifest_warnings(manifest: &SkillManifest) -> Vec<String> {
    let mut warnings = Vec::new();
    if manifest.source.kind != "local" {
        warnings.push("remote source requires provenance review before install".to_string());
    }
    if manifest.permissions.is_empty() {
        warnings.push("skill declares no permissions; execution will be read-only".to_string());
    }
    warnings
}

fn normalize_package_files(package_files: &[String]) -> Vec<String> {
    package_files
        .iter()
        .map(|path| normalize_package_path(path))
        .filter(|path| !path.is_empty())
        .collect()
}

fn normalize_package_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

fn blocked_package_files(package_files: &[String]) -> Vec<String> {
    package_files
        .iter()
        .filter(|path| is_blocked_package_file(path))
        .cloned()
        .collect()
}

fn is_blocked_package_file(path: &str) -> bool {
    let path_lower = path.to_ascii_lowercase();
    if path_lower
        .split('/')
        .any(|segment| segment.is_empty() || segment == "..")
    {
        return true;
    }
    if path_lower.ends_with("/package.json") || path_lower == "package.json" {
        return true;
    }
    matches!(
        path_lower.rsplit('.').next(),
        Some(
            "bat"
                | "cmd"
                | "com"
                | "dll"
                | "dylib"
                | "exe"
                | "jar"
                | "js"
                | "mjs"
                | "msi"
                | "node"
                | "ps1"
                | "sh"
                | "so"
                | "vbs"
                | "wasm"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::event_store::EventStore;

    fn safe_manifest_json() -> String {
        serde_json::json!({
            "schema_version": "ds-agent.skill.v1",
            "name": "office-briefing-safe",
            "version": "0.1.0",
            "description": "Prepare bounded office briefing drafts from user-selected evidence.",
            "author": "DS Agent test",
            "license": "Apache-2.0",
            "source": {
                "kind": "local",
                "url": "file:///skills/office-briefing-safe",
                "integrity": {
                    "algorithm": "sha256",
                    "hash": "8f14e45fceea167a5a36dedd4bea2543"
                }
            },
            "capabilities": ["workflow_template"],
            "permissions": [
                {
                    "kind": "file_read",
                    "scope": "user_selected",
                    "reason": "Read evidence files selected by the user."
                }
            ],
            "entry": {
                "kind": "declarative_workflow",
                "path": "workflow.json"
            }
        })
        .to_string()
    }

    fn github_manifest_json(source_url: &str, entry_content: &str) -> String {
        serde_json::json!({
            "schema_version": "ds-agent.skill.v1",
            "name": "github-workflow-pack",
            "version": "1.2.3",
            "description": "Declarative workflow from a public repository.",
            "author": "Open community",
            "license": "Apache-2.0",
            "source": {
                "kind": "github",
                "url": source_url,
                "integrity": {
                    "algorithm": "sha256",
                    "hash": sha256_hex(entry_content.as_bytes())
                }
            },
            "capabilities": ["workflow_template"],
            "permissions": [],
            "entry": {
                "kind": "declarative_workflow",
                "path": "workflow.json"
            }
        })
        .to_string()
    }

    #[test]
    fn validates_safe_declarative_skill_manifest() {
        let manifest = SkillManifest::from_json(&safe_manifest_json()).expect("manifest validates");

        assert_eq!(manifest.name, "office-briefing-safe");
        assert_eq!(manifest.trust_level, SkillTrustLevel::LocalDeclarative);
        assert!(manifest.risk_warnings.is_empty());
    }

    #[test]
    fn rejects_manifest_that_requests_terminal_write() {
        let manifest_json = serde_json::json!({
            "schema_version": "ds-agent.skill.v1",
            "name": "dangerous-terminal",
            "version": "0.1.0",
            "description": "Runs setup commands.",
            "author": "Unknown",
            "license": "MIT",
            "source": {
                "kind": "github",
                "url": "https://github.com/example/dangerous-terminal",
                "integrity": {
                    "algorithm": "sha256",
                    "hash": "8f14e45fceea167a5a36dedd4bea2543"
                }
            },
            "capabilities": ["workflow_template"],
            "permissions": [
                {
                    "kind": "terminal_write",
                    "scope": "workspace",
                    "reason": "Install dependencies."
                }
            ],
            "entry": {
                "kind": "declarative_workflow",
                "path": "workflow.json"
            }
        })
        .to_string();

        let error = SkillManifest::from_json(&manifest_json).expect_err("permission is blocked");

        assert!(error.to_string().contains("terminal_write"));
    }

    #[test]
    fn rejects_script_entry_by_default() {
        let manifest_json = serde_json::json!({
            "schema_version": "ds-agent.skill.v1",
            "name": "script-runner",
            "version": "0.1.0",
            "description": "Runs a local script.",
            "author": "Unknown",
            "license": "MIT",
            "source": {
                "kind": "local",
                "url": "file:///skills/script-runner",
                "integrity": {
                    "algorithm": "sha256",
                    "hash": "8f14e45fceea167a5a36dedd4bea2543"
                }
            },
            "capabilities": ["workflow_template"],
            "permissions": [],
            "entry": {
                "kind": "script",
                "path": "run.ps1"
            }
        })
        .to_string();

        let error = SkillManifest::from_json(&manifest_json).expect_err("script entry is blocked");

        assert!(error.to_string().contains("script"));
    }

    #[test]
    fn verifies_github_skill_source_with_integrity_metadata() {
        let manifest_json = serde_json::json!({
            "schema_version": "ds-agent.skill.v1",
            "name": "github-workflow-pack",
            "version": "1.2.3",
            "description": "Declarative workflow from a public repository.",
            "author": "Open community",
            "license": "Apache-2.0",
            "source": {
                "kind": "github",
                "url": "https://github.com/example/ds-agent-skills/releases/download/v1.2.3/github-workflow-pack.zip",
                "integrity": {
                    "algorithm": "sha256",
                    "hash": "8f14e45fceea167a5a36dedd4bea2543"
                }
            },
            "capabilities": ["workflow_template"],
            "permissions": [],
            "entry": {
                "kind": "declarative_workflow",
                "path": "workflow.json"
            }
        })
        .to_string();
        let manifest = SkillManifest::from_json(&manifest_json).expect("manifest validates");

        let verification =
            SkillSourceVerification::for_manifest(&manifest).expect("source verifies");

        assert!(verification.verified);
        assert_eq!(verification.source_kind, "github");
        assert_eq!(verification.integrity_algorithm.as_deref(), Some("sha256"));
        assert!(verification
            .provenance
            .contains("github.com/example/ds-agent-skills"));
    }

    #[test]
    fn rejects_remote_skill_source_without_https_or_known_host() {
        let manifest_json = serde_json::json!({
            "schema_version": "ds-agent.skill.v1",
            "name": "spoofed-source",
            "version": "0.1.0",
            "description": "Pretends to be a skill repository.",
            "author": "Unknown",
            "license": "MIT",
            "source": {
                "kind": "github",
                "url": "http://github.evil.example/open/skill.zip",
                "integrity": {
                    "algorithm": "sha256",
                    "hash": "8f14e45fceea167a5a36dedd4bea2543"
                }
            },
            "capabilities": ["workflow_template"],
            "permissions": [],
            "entry": {
                "kind": "declarative_workflow",
                "path": "workflow.json"
            }
        })
        .to_string();

        let error = SkillManifest::from_json(&manifest_json).expect_err("source is blocked");

        assert!(error.to_string().contains("source"));
    }

    #[test]
    fn package_preflight_accepts_declarative_manifest_with_safe_files() {
        let package_files = vec![
            "manifest.json".to_string(),
            "workflow.json".to_string(),
            "README.md".to_string(),
        ];

        let preflight =
            SkillPackagePreflight::from_manifest_and_files(&safe_manifest_json(), &package_files)
                .expect("safe package preflights");

        assert_eq!(preflight.manifest.name, "office-briefing-safe");
        assert_eq!(preflight.package_files, package_files);
        assert!(preflight.blocked_files.is_empty());
        assert_eq!(
            preflight.audit_summary,
            "package preflight accepted 3 files without executable payloads"
        );
    }

    #[test]
    fn package_preflight_blocks_script_or_native_payloads_before_install() {
        let package_files = vec![
            "manifest.json".to_string(),
            "workflow.json".to_string(),
            "tools/run.ps1".to_string(),
            "native/bridge.node".to_string(),
        ];

        let error =
            SkillPackagePreflight::from_manifest_and_files(&safe_manifest_json(), &package_files)
                .expect_err("package contains blocked payloads");

        assert!(error.to_string().contains("tools/run.ps1"));
        assert!(error.to_string().contains("native/bridge.node"));
    }

    #[test]
    fn package_preflight_reads_manifest_and_file_list_from_zip_bytes() {
        let zip_bytes = skill_zip_bytes(vec![
            ("skill.json", safe_manifest_json()),
            ("workflow.json", "{}".to_string()),
            ("README.md", "safe declarative skill".to_string()),
        ]);

        let preflight =
            SkillPackagePreflight::from_zip_bytes(&zip_bytes).expect("zip package preflights");

        assert_eq!(preflight.manifest.name, "office-briefing-safe");
        assert_eq!(
            preflight.package_files,
            vec![
                "skill.json".to_string(),
                "workflow.json".to_string(),
                "README.md".to_string()
            ]
        );
        assert_eq!(preflight.entry_content.as_deref(), Some("{}"));
        let expected_entry_sha256 = sha256_hex(b"{}");
        assert_eq!(
            preflight.entry_sha256.as_deref(),
            Some(expected_entry_sha256.as_str())
        );
    }

    #[test]
    fn package_preflight_blocks_executable_payloads_inside_zip_bytes() {
        let zip_bytes = skill_zip_bytes(vec![
            ("skill.json", safe_manifest_json()),
            ("workflow.json", "{}".to_string()),
            ("scripts/install.ps1", "Write-Host bad".to_string()),
        ]);

        let error = SkillPackagePreflight::from_zip_bytes(&zip_bytes)
            .expect_err("zip executable payload is blocked");

        assert!(error.to_string().contains("scripts/install.ps1"));
    }

    #[test]
    fn remote_package_preflight_accepts_verified_github_zip_without_installing() {
        let source_url = "https://github.com/example/ds-agent-skills/releases/download/v1.2.3/github-workflow-pack.zip";
        let entry_content = "{}";
        let zip_bytes = skill_zip_bytes(vec![
            (
                "skill.json",
                github_manifest_json(source_url, entry_content),
            ),
            ("workflow.json", entry_content.to_string()),
            ("README.md", "safe remote declarative skill".to_string()),
        ]);

        let preflight = SkillPackagePreflight::from_remote_zip_bytes(source_url, &zip_bytes)
            .expect("remote package preflights");

        assert_eq!(preflight.manifest.name, "github-workflow-pack");
        assert!(preflight
            .warnings
            .iter()
            .any(|warning| warning.contains("remote package was previewed only")));
    }

    #[test]
    fn remote_package_preflight_rejects_tampered_entry_content() {
        let source_url = "https://github.com/example/ds-agent-skills/releases/download/v1.2.3/github-workflow-pack.zip";
        let zip_bytes = skill_zip_bytes(vec![
            ("skill.json", github_manifest_json(source_url, "{}")),
            ("workflow.json", "{\"tampered\":true}".to_string()),
        ]);

        let error = SkillPackagePreflight::from_remote_zip_bytes(source_url, &zip_bytes)
            .expect_err("tampered entry is rejected");

        assert!(error.to_string().contains("integrity mismatch"));
    }

    #[test]
    fn remote_package_preflight_rejects_manifest_source_mismatch() {
        let source_url = "https://github.com/example/ds-agent-skills/releases/download/v1.2.3/github-workflow-pack.zip";
        let entry_content = "{}";
        let zip_bytes = skill_zip_bytes(vec![
            (
                "skill.json",
                github_manifest_json(
                    "https://github.com/example/other/releases/download/v1.0.0/other.zip",
                    entry_content,
                ),
            ),
            ("workflow.json", entry_content.to_string()),
        ]);

        let error = SkillPackagePreflight::from_remote_zip_bytes(source_url, &zip_bytes)
            .expect_err("source mismatch is blocked");

        assert!(error.to_string().contains("source"));
    }

    #[test]
    fn remote_package_preflight_rejects_oversized_payloads_before_zip_read() {
        let oversized = vec![0_u8; MAX_REMOTE_SKILL_PACKAGE_BYTES + 1];

        let error = SkillPackagePreflight::from_remote_zip_bytes(
            "https://huggingface.co/example/ds-agent-skill/resolve/main/skill.zip",
            &oversized,
        )
        .expect_err("oversized payload is blocked");

        assert!(error.to_string().contains("too large"));
    }

    #[test]
    fn event_store_installs_and_disables_skill_without_executing_code() {
        let store = EventStore::open_memory().expect("store opens");
        let manifest = SkillManifest::from_json(&safe_manifest_json()).expect("manifest validates");
        let installed =
            SkillInstallationRecord::new(manifest, "local import".to_string()).expect("record");

        assert!(store
            .append_skill_installation(&installed)
            .expect("install records"));

        let disabled = SkillEnablementChange::new(
            installed.id,
            SkillEnablementStatus::Disabled,
            "User disabled before first run.".to_string(),
        )
        .expect("disable record");
        store
            .append_skill_enablement_change(&disabled)
            .expect("disable records");

        let records = store.list_skill_records().expect("skill records");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].manifest.name, "office-briefing-safe");
        assert_eq!(
            records[0].manifest.trust_level,
            SkillTrustLevel::LocalDeclarative
        );
        assert_eq!(
            records[0].enablement_status,
            SkillEnablementStatus::Disabled
        );
        assert_eq!(
            records[0].last_audit_note.as_deref(),
            Some("User disabled before first run.")
        );
    }

    #[test]
    fn event_store_can_reset_trust_and_uninstall_skill_with_audit_history() {
        let store = EventStore::open_memory().expect("store opens");
        let manifest = SkillManifest::from_json(&safe_manifest_json()).expect("manifest validates");
        let installed =
            SkillInstallationRecord::new(manifest, "local import".to_string()).expect("record");
        store
            .append_skill_installation(&installed)
            .expect("install records");

        let reset = SkillTrustReset::new(
            installed.id,
            "Source was replaced; require review before reuse.".to_string(),
        )
        .expect("trust reset record");
        store
            .append_skill_trust_reset(&reset)
            .expect("trust reset records");
        let records = store.list_skill_records().expect("records load");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].manifest.trust_level, SkillTrustLevel::Untrusted);
        assert_eq!(
            records[0].enablement_status,
            SkillEnablementStatus::Disabled
        );
        assert_eq!(
            records[0].last_audit_note.as_deref(),
            Some("Source was replaced; require review before reuse.")
        );

        let uninstall =
            SkillUninstallRecord::new(installed.id, "User removed the skill.".to_string())
                .expect("uninstall record");
        store
            .append_skill_uninstall(&uninstall)
            .expect("uninstall records");

        assert!(store
            .list_skill_records()
            .expect("records reload")
            .is_empty());
        assert_eq!(
            store
                .list_skill_uninstalls()
                .expect("uninstalls load")
                .first()
                .map(|record| record.note.as_str()),
            Some("User removed the skill.")
        );
    }

    #[test]
    fn event_store_updates_skill_in_place_and_retains_rollback_metadata() {
        let store = EventStore::open_memory().expect("store opens");
        let first_entry = r#"{"steps":[{"tool":"network.search"}]}"#;
        let first_zip = skill_zip_bytes(vec![
            ("skill.json", safe_manifest_json()),
            ("workflow.json", first_entry.to_string()),
        ]);
        let mut installed = SkillInstallationRecord::from_preflight(
            SkillPackagePreflight::from_zip_bytes(&first_zip).expect("first package preflights"),
            "github repository".to_string(),
        )
        .expect("first package installs");
        installed.source_identity = Some(SkillSourceIdentity {
            provider: "github".to_string(),
            repository_url: "https://github.com/example/skills".to_string(),
            requested_revision: None,
            resolved_revision: "commit-one".to_string(),
            package_path: None,
            source_format: "ds_agent_manifest".to_string(),
        });
        store
            .append_skill_installation(&installed)
            .expect("first install records");

        let next_entry = r#"{"steps":[{"tool":"browser.open"}]}"#;
        let next_manifest = safe_manifest_json().replace("0.1.0", "0.2.0");
        let next_zip = skill_zip_bytes(vec![
            ("skill.json", next_manifest),
            ("workflow.json", next_entry.to_string()),
        ]);
        let mut candidate = SkillInstallationRecord::from_preflight(
            SkillPackagePreflight::from_zip_bytes(&next_zip).expect("next package preflights"),
            "github repository update".to_string(),
        )
        .expect("candidate package builds");
        candidate.source_identity = Some(SkillSourceIdentity {
            resolved_revision: "commit-two".to_string(),
            ..installed
                .source_identity
                .clone()
                .expect("first source identity exists")
        });
        let update = SkillUpdateRecord::from_candidate(
            installed.id,
            installed.manifest.version.clone(),
            installed
                .source_identity
                .as_ref()
                .map(|source| source.resolved_revision.clone()),
            candidate,
        )
        .expect("update record builds");

        store.append_skill_update(&update).expect("update records");
        let records = store.list_skill_records().expect("records load");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, installed.id);
        assert_eq!(records[0].manifest.version, "0.2.0");
        assert_eq!(records[0].rollback_version.as_deref(), Some("0.1.0"));
        assert_eq!(records[0].rollback_revision.as_deref(), Some("commit-one"));
        assert_eq!(
            records[0]
                .source_identity
                .as_ref()
                .map(|source| source.resolved_revision.as_str()),
            Some("commit-two")
        );
        assert_eq!(
            store
                .prepare_skill_activation(installed.id, "Use the updated Skill.".to_string())
                .expect("updated entry activates")
                .instructions,
            next_entry
        );
    }

    #[test]
    fn failed_skill_update_keeps_last_working_version() {
        let store = EventStore::open_memory().expect("store opens");
        let manifest = SkillManifest::from_json(&safe_manifest_json()).expect("manifest validates");
        let installed =
            SkillInstallationRecord::new(manifest, "local import".to_string()).expect("record");
        store
            .append_skill_installation(&installed)
            .expect("install records");
        let failure = SkillUpdateFailureRecord::new(
            installed.id,
            Some("candidate-commit".to_string()),
            "candidate requests a blocked executable".to_string(),
        )
        .expect("failure record builds");

        store
            .append_skill_update_failure(&failure)
            .expect("failure records");
        let records = store.list_skill_records().expect("records load");

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].manifest.version, "0.1.0");
        assert_eq!(records[0].update_state, SkillUpdateState::Failed);
        assert_eq!(
            records[0].last_update_failure.as_deref(),
            Some("candidate requests a blocked executable")
        );
    }

    #[test]
    fn protected_system_skill_cannot_be_uninstalled() {
        let store = EventStore::open_memory().expect("store opens");
        let manifest = SkillManifest::from_json(&safe_manifest_json()).expect("manifest validates");
        let mut installed =
            SkillInstallationRecord::new(manifest, "DS Agent system".to_string()).expect("record");
        installed.package_kind = SkillPackageKind::SystemSkill;
        installed.system_protected = true;
        store
            .append_skill_installation(&installed)
            .expect("system skill records");
        let uninstall = SkillUninstallRecord::new(
            installed.id,
            "Attempt to remove protected system skill.".to_string(),
        )
        .expect("uninstall record builds");

        let error = store
            .append_skill_uninstall(&uninstall)
            .expect_err("protected system skill is retained");

        assert!(error.to_string().contains("protected system skill"));
        assert_eq!(store.list_skill_records().expect("records load").len(), 1);
    }

    #[test]
    fn skill_execution_prepares_enabled_trusted_declarative_skill_and_records_audit() {
        let store = EventStore::open_memory().expect("store opens");
        let manifest = SkillManifest::from_json(&safe_manifest_json()).expect("manifest validates");
        let installed =
            SkillInstallationRecord::new(manifest, "local import".to_string()).expect("record");
        store
            .append_skill_installation(&installed)
            .expect("install records");

        let execution = store
            .prepare_skill_execution(
                installed.id,
                "Draft a bounded operations briefing from selected evidence.".to_string(),
            )
            .expect("execution prepares");

        assert_eq!(execution.skill_id, installed.id);
        assert_eq!(execution.status, SkillExecutionStatus::Planned);
        assert!(execution.execution_plan.contains("workflow.json"));
        assert!(execution.blocked_reason.is_none());
        assert_eq!(
            store
                .list_skill_executions()
                .expect("executions load")
                .first()
                .map(|record| record.id),
            Some(execution.id)
        );
    }

    #[test]
    fn skill_activation_loads_only_hash_verified_installed_entry_content() {
        let store = EventStore::open_memory().expect("store opens");
        let entry_content = r#"{"steps":[{"tool":"network.search"}]}"#;
        let zip_bytes = skill_zip_bytes(vec![
            ("skill.json", safe_manifest_json()),
            ("workflow.json", entry_content.to_string()),
        ]);
        let preflight =
            SkillPackagePreflight::from_zip_bytes(&zip_bytes).expect("package preflights");
        let installed =
            SkillInstallationRecord::from_preflight(preflight, "local verified zip".to_string())
                .expect("installation retains entry evidence");
        store
            .append_skill_installation(&installed)
            .expect("installation records");

        let activation = store
            .prepare_skill_activation(
                installed.id,
                "Use the workflow to collect public evidence.".to_string(),
            )
            .expect("trusted installed entry activates");

        assert_eq!(activation.skill_id, installed.id);
        assert_eq!(activation.instructions, entry_content);
        assert_eq!(
            activation.entry_sha256,
            sha256_hex(entry_content.as_bytes())
        );
        assert_eq!(activation.entry_kind, "declarative_workflow");
    }

    #[test]
    fn skill_activation_blocks_manifest_only_installation_without_entry_evidence() {
        let store = EventStore::open_memory().expect("store opens");
        let manifest = SkillManifest::from_json(&safe_manifest_json()).expect("manifest validates");
        let installed =
            SkillInstallationRecord::new(manifest, "manifest only".to_string()).expect("record");
        store
            .append_skill_installation(&installed)
            .expect("installation records");

        let error = store
            .prepare_skill_activation(installed.id, "Run it".to_string())
            .expect_err("entry evidence is required");

        assert!(error.to_string().contains("entry content"));
    }

    #[test]
    fn skill_execution_blocks_disabled_installed_skill() {
        let store = EventStore::open_memory().expect("store opens");
        let manifest = SkillManifest::from_json(&safe_manifest_json()).expect("manifest validates");
        let installed =
            SkillInstallationRecord::new(manifest, "local import".to_string()).expect("record");
        store
            .append_skill_installation(&installed)
            .expect("install records");
        let disabled = SkillEnablementChange::new(
            installed.id,
            SkillEnablementStatus::Disabled,
            "User disabled before execution.".to_string(),
        )
        .expect("disable record");
        store
            .append_skill_enablement_change(&disabled)
            .expect("disable records");

        let execution = store
            .prepare_skill_execution(installed.id, "Use when relevant".to_string())
            .expect("disabled skill records a blocked execution");

        assert_eq!(execution.status, SkillExecutionStatus::Blocked);
        assert_eq!(
            execution.blocked_reason.as_deref(),
            Some("skill is disabled")
        );
        assert_eq!(
            store.list_skill_executions().expect("executions load"),
            vec![execution]
        );
    }

    fn skill_zip_bytes(files: Vec<(&str, String)>) -> Vec<u8> {
        use std::io::{Cursor, Write};
        use zip::write::FileOptions;

        let cursor = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(cursor);
        for (path, content) in files {
            zip.start_file(
                path,
                FileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .expect("zip file starts");
            zip.write_all(content.as_bytes()).expect("zip file writes");
        }
        zip.finish().expect("zip finishes").into_inner()
    }
}
