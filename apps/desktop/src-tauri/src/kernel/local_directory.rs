use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const LOCAL_DIRECTORY_SETTINGS_FILE: &str = "local-directories.json";
pub const LOCAL_EVIDENCE_DIR_NAME: &str = "evidence";
pub const LOCAL_EXPORT_DIR_NAME: &str = "exports";
pub const LOCAL_REPORTS_DIR_NAME: &str = "reports";
pub const LOCAL_RUNS_DIR_NAME: &str = "runs";
pub const LOCAL_SOURCES_DIR_NAME: &str = "sources";
pub const LOCAL_WORK_PACKAGES_DIR_NAME: &str = "work-packages";
pub const LOCAL_MEMORY_DIR_NAME: &str = "memory";
pub const LOCAL_LOGS_DIR_NAME: &str = "logs";

#[derive(Debug, thiserror::Error)]
pub enum LocalDirectoryError {
    #[error("workspace directory is required")]
    MissingWorkspace,

    #[error("evidence directory is required")]
    MissingEvidence,

    #[error("export directory is required")]
    MissingExport,

    #[error("workspace directory must exist")]
    WorkspaceNotDirectory,

    #[error("evidence directory must exist")]
    EvidenceNotDirectory,

    #[error("export directory must exist")]
    ExportNotDirectory,

    #[error("local workspace directory structure could not be created: {0}")]
    Create(std::io::Error),

    #[error("local workspace data could not be migrated: {0}")]
    Migrate(std::io::Error),

    #[error("local directory settings could not be read: {0}")]
    Read(std::io::Error),

    #[error("local directory settings could not be written: {0}")]
    Write(std::io::Error),

    #[error("local directory settings are invalid json: {0}")]
    Json(serde_json::Error),

    #[error("local workspace managed directories must stay inside the workspace root")]
    ManagedDirectoryEscape,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalDirectorySettings {
    pub workspace_dir: String,
    #[serde(default)]
    pub workspace_name: String,
    #[serde(default)]
    pub evidence_dir: String,
    #[serde(default)]
    pub export_dir: String,
}

impl LocalDirectorySettings {
    #[cfg(test)]
    pub fn new(
        workspace_dir: String,
        evidence_dir: String,
        export_dir: String,
    ) -> Result<Self, LocalDirectoryError> {
        Self::from_optional_dirs(workspace_dir, None, Some(evidence_dir), Some(export_dir))
    }

    pub fn from_workspace_dir(workspace_dir: String) -> Result<Self, LocalDirectoryError> {
        Self::from_optional_dirs(workspace_dir, None, None, None)
    }

    pub fn from_workspace_dir_and_name(
        workspace_dir: String,
        workspace_name: String,
    ) -> Result<Self, LocalDirectoryError> {
        Self::from_optional_dirs(workspace_dir, Some(workspace_name), None, None)
    }

    pub fn from_optional_dirs(
        workspace_dir: String,
        workspace_name: Option<String>,
        evidence_dir: Option<String>,
        export_dir: Option<String>,
    ) -> Result<Self, LocalDirectoryError> {
        let workspace_dir = workspace_dir.trim().to_string();

        if workspace_dir.is_empty() {
            return Err(LocalDirectoryError::MissingWorkspace);
        }

        let evidence_dir = evidence_dir
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| derive_workspace_subdir(&workspace_dir, LOCAL_EVIDENCE_DIR_NAME));
        let export_dir = export_dir
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| derive_workspace_subdir(&workspace_dir, LOCAL_EXPORT_DIR_NAME));
        let workspace_name = workspace_name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| derive_workspace_name(&workspace_dir));

        let settings = Self {
            workspace_dir,
            workspace_name,
            evidence_dir,
            export_dir,
        };

        settings.validate_required_paths()?;
        Ok(settings)
    }

    fn workspace_exists(&self) -> bool {
        Path::new(&self.workspace_dir).is_dir()
    }

    fn evidence_exists(&self) -> bool {
        Path::new(&self.evidence_dir).is_dir()
    }

    fn export_exists(&self) -> bool {
        Path::new(&self.export_dir).is_dir()
    }

    fn validate_required_paths(&self) -> Result<(), LocalDirectoryError> {
        if self.workspace_dir.trim().is_empty() {
            return Err(LocalDirectoryError::MissingWorkspace);
        }
        if self.evidence_dir.trim().is_empty() {
            return Err(LocalDirectoryError::MissingEvidence);
        }
        if self.export_dir.trim().is_empty() {
            return Err(LocalDirectoryError::MissingExport);
        }

        Ok(())
    }

    fn normalize_derived_directories(&mut self) -> Result<(), LocalDirectoryError> {
        self.workspace_dir = self.workspace_dir.trim().to_string();
        self.workspace_name = self.workspace_name.trim().to_string();
        self.evidence_dir = self.evidence_dir.trim().to_string();
        self.export_dir = self.export_dir.trim().to_string();

        if self.workspace_dir.is_empty() {
            return Err(LocalDirectoryError::MissingWorkspace);
        }
        if self.workspace_name.is_empty() {
            self.workspace_name = derive_workspace_name(&self.workspace_dir);
        }
        if self.evidence_dir.is_empty() {
            self.evidence_dir =
                derive_workspace_subdir(&self.workspace_dir, LOCAL_EVIDENCE_DIR_NAME);
        }
        if self.export_dir.is_empty() {
            self.export_dir = derive_workspace_subdir(&self.workspace_dir, LOCAL_EXPORT_DIR_NAME);
        }

        self.validate_required_paths()
    }

    fn ensure_directory_structure(&self) -> Result<(), LocalDirectoryError> {
        let workspace = Path::new(&self.workspace_dir);
        fs::create_dir_all(workspace).map_err(LocalDirectoryError::Create)?;
        let canonical_workspace = workspace
            .canonicalize()
            .map_err(LocalDirectoryError::Create)?;
        for directory in self.standard_directories() {
            let directory = PathBuf::from(directory);
            if !directory.starts_with(workspace) {
                return Err(LocalDirectoryError::ManagedDirectoryEscape);
            }
            fs::create_dir_all(&directory).map_err(LocalDirectoryError::Create)?;
            let canonical = directory
                .canonicalize()
                .map_err(LocalDirectoryError::Create)?;
            if !canonical.starts_with(&canonical_workspace) {
                return Err(LocalDirectoryError::ManagedDirectoryEscape);
            }
        }

        Ok(())
    }

    fn standard_directories(&self) -> Vec<String> {
        let mut directories = vec![
            self.workspace_dir.clone(),
            self.evidence_dir.clone(),
            self.export_dir.clone(),
        ];
        directories.extend(
            [
                LOCAL_REPORTS_DIR_NAME,
                LOCAL_RUNS_DIR_NAME,
                LOCAL_SOURCES_DIR_NAME,
                LOCAL_WORK_PACKAGES_DIR_NAME,
                LOCAL_MEMORY_DIR_NAME,
                LOCAL_LOGS_DIR_NAME,
            ]
            .into_iter()
            .map(|name| derive_workspace_subdir(&self.workspace_dir, name)),
        );
        directories
    }

    fn validate_existing_directories(&self) -> Result<(), LocalDirectoryError> {
        if !self.workspace_exists() {
            return Err(LocalDirectoryError::WorkspaceNotDirectory);
        }
        if !self.evidence_exists() {
            return Err(LocalDirectoryError::EvidenceNotDirectory);
        }
        if !self.export_exists() {
            return Err(LocalDirectoryError::ExportNotDirectory);
        }

        Ok(())
    }
}

fn derive_workspace_subdir(workspace_dir: &str, name: &str) -> String {
    Path::new(workspace_dir)
        .join(name)
        .to_string_lossy()
        .to_string()
}

fn derive_workspace_name(workspace_dir: &str) -> String {
    Path::new(workspace_dir)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("DS Agent Workspace")
        .to_string()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalDirectoryState {
    pub app_data_dir: String,
    pub settings_file: String,
    pub settings: Option<LocalDirectorySettings>,
    pub needs_setup: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalDirectoryReadinessStatus {
    pub needs_setup: bool,
    pub workspace_configured: bool,
    pub evidence_configured: bool,
    pub export_configured: bool,
    pub paths_redacted: bool,
    pub note: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceReadinessCode {
    Ready,
    WorkspaceMissing,
    WorkspaceUnavailable,
    WorkspacePermissionDenied,
    WorkspaceProbeCleanupFailed,
    WorkspaceSettingsInvalid,
}

impl WorkspaceReadinessCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::WorkspaceMissing => "workspace_missing",
            Self::WorkspaceUnavailable => "workspace_unavailable",
            Self::WorkspacePermissionDenied => "workspace_permission_denied",
            Self::WorkspaceProbeCleanupFailed => "workspace_probe_cleanup_failed",
            Self::WorkspaceSettingsInvalid => "workspace_settings_invalid",
        }
    }

    pub fn retryable(self) -> bool {
        !matches!(self, Self::Ready | Self::WorkspaceSettingsInvalid)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceReadinessProjection {
    pub configured: bool,
    pub workspace_name: Option<String>,
    pub workspace_root_display: Option<String>,
    pub root_exists: bool,
    pub managed_directories_ready: bool,
    pub writable: Option<bool>,
    pub code: WorkspaceReadinessCode,
    pub retryable: bool,
    pub message_key: String,
}

impl WorkspaceReadinessProjection {
    fn for_code(
        settings: Option<&LocalDirectorySettings>,
        code: WorkspaceReadinessCode,
        root_exists: bool,
        managed_directories_ready: bool,
        writable: Option<bool>,
    ) -> Self {
        Self {
            configured: settings.is_some(),
            workspace_name: settings.map(|settings| settings.workspace_name.clone()),
            workspace_root_display: settings
                .map(|settings| derive_workspace_name(&settings.workspace_dir)),
            root_exists,
            managed_directories_ready,
            writable,
            code,
            retryable: code.retryable(),
            message_key: format!("onboarding.workspace.{}", code.as_str()),
        }
    }

    pub fn settings_invalid() -> Self {
        Self::for_code(
            None,
            WorkspaceReadinessCode::WorkspaceSettingsInvalid,
            false,
            false,
            None,
        )
    }
}

pub fn workspace_readiness_projection_from_setup_error(
    error: &LocalDirectoryError,
) -> WorkspaceReadinessProjection {
    let code = match error {
        LocalDirectoryError::WorkspaceNotDirectory
        | LocalDirectoryError::EvidenceNotDirectory
        | LocalDirectoryError::ExportNotDirectory => WorkspaceReadinessCode::WorkspaceUnavailable,
        LocalDirectoryError::Create(error) | LocalDirectoryError::Migrate(error) => {
            match error.kind() {
                std::io::ErrorKind::PermissionDenied => {
                    WorkspaceReadinessCode::WorkspacePermissionDenied
                }
                _ => WorkspaceReadinessCode::WorkspaceUnavailable,
            }
        }
        LocalDirectoryError::MissingWorkspace
        | LocalDirectoryError::MissingEvidence
        | LocalDirectoryError::MissingExport
        | LocalDirectoryError::Read(_)
        | LocalDirectoryError::Write(_)
        | LocalDirectoryError::Json(_)
        | LocalDirectoryError::ManagedDirectoryEscape => {
            WorkspaceReadinessCode::WorkspaceSettingsInvalid
        }
    };
    WorkspaceReadinessProjection::for_code(None, code, false, false, None)
}

impl Default for LocalDirectoryReadinessStatus {
    fn default() -> Self {
        Self {
            needs_setup: true,
            workspace_configured: false,
            evidence_configured: false,
            export_configured: false,
            paths_redacted: true,
            note: "local workspace needs setup on this machine".to_string(),
        }
    }
}

pub fn local_directory_readiness_from_state(
    state: &LocalDirectoryState,
) -> LocalDirectoryReadinessStatus {
    let Some(settings) = state.settings.as_ref() else {
        return LocalDirectoryReadinessStatus::default();
    };
    let workspace_configured = settings.workspace_exists();
    let evidence_configured = settings.evidence_exists();
    let export_configured = settings.export_exists();
    let needs_setup =
        state.needs_setup || !workspace_configured || !evidence_configured || !export_configured;

    LocalDirectoryReadinessStatus {
        needs_setup,
        workspace_configured,
        evidence_configured,
        export_configured,
        paths_redacted: true,
        note: if needs_setup {
            "local workspace settings are incomplete on this machine".to_string()
        } else {
            "local workspace and DS Agent managed directories are configured; paths are redacted"
                .to_string()
        },
    }
}

pub fn workspace_readiness_projection_from_state(
    state: &LocalDirectoryState,
) -> WorkspaceReadinessProjection {
    let Some(settings) = state.settings.as_ref() else {
        return WorkspaceReadinessProjection::for_code(
            None,
            WorkspaceReadinessCode::WorkspaceMissing,
            false,
            false,
            None,
        );
    };
    let workspace = Path::new(&settings.workspace_dir);
    if !workspace.is_dir() {
        return WorkspaceReadinessProjection::for_code(
            Some(settings),
            WorkspaceReadinessCode::WorkspaceUnavailable,
            false,
            false,
            None,
        );
    }
    let canonical_workspace = match workspace.canonicalize() {
        Ok(path) => path,
        Err(_) => {
            return WorkspaceReadinessProjection::for_code(
                Some(settings),
                WorkspaceReadinessCode::WorkspaceUnavailable,
                true,
                false,
                None,
            )
        }
    };
    let managed_directories_ready = settings
        .standard_directories()
        .into_iter()
        .all(|directory| {
            let directory = PathBuf::from(directory);
            directory.is_dir()
                && directory
                    .canonicalize()
                    .map(|canonical| canonical.starts_with(&canonical_workspace))
                    .unwrap_or(false)
        });
    if !managed_directories_ready {
        return WorkspaceReadinessProjection::for_code(
            Some(settings),
            WorkspaceReadinessCode::WorkspaceSettingsInvalid,
            true,
            false,
            None,
        );
    }
    match workspace_write_probe(workspace) {
        Ok(()) => WorkspaceReadinessProjection::for_code(
            Some(settings),
            WorkspaceReadinessCode::Ready,
            true,
            true,
            Some(true),
        ),
        Err(WorkspaceReadinessCode::WorkspaceProbeCleanupFailed) => {
            WorkspaceReadinessProjection::for_code(
                Some(settings),
                WorkspaceReadinessCode::WorkspaceProbeCleanupFailed,
                true,
                true,
                Some(false),
            )
        }
        Err(_) => WorkspaceReadinessProjection::for_code(
            Some(settings),
            WorkspaceReadinessCode::WorkspacePermissionDenied,
            true,
            true,
            Some(false),
        ),
    }
}

fn workspace_write_probe(root: &Path) -> Result<(), WorkspaceReadinessCode> {
    workspace_write_probe_with_cleanup(root, |path| fs::remove_file(path))
}

fn workspace_write_probe_with_cleanup(
    root: &Path,
    cleanup: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), WorkspaceReadinessCode> {
    let canonical_root = root
        .canonicalize()
        .map_err(|_| WorkspaceReadinessCode::WorkspaceUnavailable)?;
    let probe = root.join(format!(".ds-agent-readiness-{}.tmp", Uuid::new_v4()));
    if probe.parent() != Some(root) || !probe.starts_with(root) {
        return Err(WorkspaceReadinessCode::WorkspaceSettingsInvalid);
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|_| WorkspaceReadinessCode::WorkspacePermissionDenied)?;
    let write_result = file
        .write_all(b"ds-agent-readiness-v1")
        .and_then(|_| file.sync_all());
    drop(file);
    if write_result.is_err() {
        let _ = fs::remove_file(&probe);
        return Err(WorkspaceReadinessCode::WorkspacePermissionDenied);
    }
    let canonical_probe = match probe.canonicalize() {
        Ok(path) if path.starts_with(&canonical_root) => path,
        _ => {
            let _ = fs::remove_file(&probe);
            return Err(WorkspaceReadinessCode::WorkspaceSettingsInvalid);
        }
    };
    if cleanup(&canonical_probe).is_err() {
        let _ = fs::remove_file(&canonical_probe);
        return Err(WorkspaceReadinessCode::WorkspaceProbeCleanupFailed);
    }
    if canonical_probe.exists() {
        let _ = fs::remove_file(&canonical_probe);
        return Err(WorkspaceReadinessCode::WorkspaceProbeCleanupFailed);
    }
    Ok(())
}

pub fn load_local_directory_state(
    app_data_dir: impl AsRef<Path>,
) -> Result<LocalDirectoryState, LocalDirectoryError> {
    let app_data_dir = app_data_dir.as_ref();
    let settings_file = app_data_dir.join(LOCAL_DIRECTORY_SETTINGS_FILE);
    let settings: Option<LocalDirectorySettings> = if settings_file.exists() {
        let settings_json =
            fs::read_to_string(&settings_file).map_err(LocalDirectoryError::Read)?;
        let mut settings: LocalDirectorySettings =
            serde_json::from_str(&settings_json).map_err(LocalDirectoryError::Json)?;
        settings.normalize_derived_directories()?;
        if settings.workspace_exists() {
            // Existing compatible settings are repaired in place without moving data.
            let _ = settings.ensure_directory_structure();
        }
        Some(settings)
    } else {
        None
    };

    Ok(LocalDirectoryState {
        app_data_dir: app_data_dir.to_string_lossy().to_string(),
        settings_file: settings_file.to_string_lossy().to_string(),
        needs_setup: settings
            .as_ref()
            .map(|settings| settings.validate_existing_directories().is_err())
            .unwrap_or(true),
        settings,
    })
}

pub fn save_local_directory_settings(
    app_data_dir: impl AsRef<Path>,
    settings: LocalDirectorySettings,
) -> Result<LocalDirectoryState, LocalDirectoryError> {
    let app_data_dir = app_data_dir.as_ref();
    let previous_settings = load_local_directory_state(app_data_dir)
        .ok()
        .and_then(|state| state.settings);
    settings.ensure_directory_structure()?;
    if let Some(previous_settings) = previous_settings.as_ref() {
        migrate_managed_workspace_data(previous_settings, &settings)?;
    }
    settings.validate_existing_directories()?;
    fs::create_dir_all(app_data_dir).map_err(LocalDirectoryError::Write)?;
    let settings_file = app_data_dir.join(LOCAL_DIRECTORY_SETTINGS_FILE);
    let settings_json =
        serde_json::to_string_pretty(&settings).map_err(LocalDirectoryError::Json)?;
    fs::write(&settings_file, settings_json).map_err(LocalDirectoryError::Write)?;

    load_local_directory_state(app_data_dir)
}

fn migrate_managed_workspace_data(
    previous: &LocalDirectorySettings,
    next: &LocalDirectorySettings,
) -> Result<(), LocalDirectoryError> {
    if paths_equivalent(
        Path::new(&previous.workspace_dir),
        Path::new(&next.workspace_dir),
    ) {
        return Ok(());
    }

    for (source, destination) in managed_directory_migration_pairs(previous, next) {
        if paths_equivalent(&source, &destination) {
            continue;
        }
        move_directory_contents(&source, &destination)?;
    }

    Ok(())
}

fn managed_directory_migration_pairs(
    previous: &LocalDirectorySettings,
    next: &LocalDirectorySettings,
) -> Vec<(PathBuf, PathBuf)> {
    let mut pairs = vec![
        (
            PathBuf::from(&previous.evidence_dir),
            PathBuf::from(&next.evidence_dir),
        ),
        (
            PathBuf::from(&previous.export_dir),
            PathBuf::from(&next.export_dir),
        ),
    ];

    pairs.extend(
        [
            LOCAL_REPORTS_DIR_NAME,
            LOCAL_RUNS_DIR_NAME,
            LOCAL_SOURCES_DIR_NAME,
            LOCAL_WORK_PACKAGES_DIR_NAME,
            LOCAL_MEMORY_DIR_NAME,
            LOCAL_LOGS_DIR_NAME,
        ]
        .into_iter()
        .map(|name| {
            (
                Path::new(&previous.workspace_dir).join(name),
                Path::new(&next.workspace_dir).join(name),
            )
        }),
    );

    pairs
}

fn move_directory_contents(source: &Path, destination: &Path) -> Result<(), LocalDirectoryError> {
    if !source.is_dir() {
        return Ok(());
    }

    fs::create_dir_all(destination).map_err(LocalDirectoryError::Migrate)?;
    for entry in fs::read_dir(source).map_err(LocalDirectoryError::Migrate)? {
        let entry = entry.map_err(LocalDirectoryError::Migrate)?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());

        if source_path.is_dir() && destination_path.is_dir() {
            move_directory_contents(&source_path, &destination_path)?;
            remove_dir_if_empty(&source_path)?;
            continue;
        }

        let destination_path = if destination_path.exists() {
            unique_migration_destination(destination, &entry.file_name())
        } else {
            destination_path
        };
        move_path(&source_path, &destination_path)?;
    }

    remove_dir_if_empty(source)?;
    Ok(())
}

fn move_path(source: &Path, destination: &Path) -> Result<(), LocalDirectoryError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(LocalDirectoryError::Migrate)?;
    }

    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(_) if source.is_dir() => {
            copy_dir_all(source, destination)?;
            fs::remove_dir_all(source).map_err(LocalDirectoryError::Migrate)
        }
        Err(_) => {
            fs::copy(source, destination).map_err(LocalDirectoryError::Migrate)?;
            fs::remove_file(source).map_err(LocalDirectoryError::Migrate)
        }
    }
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<(), LocalDirectoryError> {
    fs::create_dir_all(destination).map_err(LocalDirectoryError::Migrate)?;
    for entry in fs::read_dir(source).map_err(LocalDirectoryError::Migrate)? {
        let entry = entry.map_err(LocalDirectoryError::Migrate)?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_all(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).map_err(LocalDirectoryError::Migrate)?;
        }
    }
    Ok(())
}

fn unique_migration_destination(destination: &Path, file_name: &OsStr) -> PathBuf {
    let file_name = file_name.to_string_lossy();
    for index in 1.. {
        let candidate = destination.join(format!("{file_name}.migrated-{index}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!("migration destination suffix loop should return")
}

fn remove_dir_if_empty(directory: &Path) -> Result<(), LocalDirectoryError> {
    if directory.is_dir()
        && fs::read_dir(directory)
            .map_err(LocalDirectoryError::Migrate)?
            .next()
            .is_none()
    {
        fs::remove_dir(directory).map_err(LocalDirectoryError::Migrate)?;
    }

    Ok(())
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        load_local_directory_state, save_local_directory_settings,
        workspace_readiness_projection_from_setup_error, workspace_readiness_projection_from_state,
        workspace_write_probe_with_cleanup, LocalDirectoryError, LocalDirectorySettings,
        WorkspaceReadinessCode, LOCAL_DIRECTORY_SETTINGS_FILE, LOCAL_EVIDENCE_DIR_NAME,
        LOCAL_EXPORT_DIR_NAME, LOCAL_LOGS_DIR_NAME, LOCAL_MEMORY_DIR_NAME, LOCAL_REPORTS_DIR_NAME,
        LOCAL_RUNS_DIR_NAME, LOCAL_SOURCES_DIR_NAME, LOCAL_WORK_PACKAGES_DIR_NAME,
    };

    #[test]
    fn missing_settings_requires_first_run_setup() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let state = load_local_directory_state(temp_dir.path()).expect("state loads");

        assert!(state.needs_setup);
        assert!(state.settings.is_none());
        assert_eq!(state.app_data_dir, temp_dir.path().to_string_lossy());
        assert!(state.settings_file.ends_with(LOCAL_DIRECTORY_SETTINGS_FILE));
    }

    #[test]
    fn save_then_load_local_directory_settings() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_dir = temp_dir.path().join("workspace");

        let saved = save_local_directory_settings(
            temp_dir.path(),
            LocalDirectorySettings::from_workspace_dir(format!(
                "  {}  ",
                workspace_dir.to_string_lossy()
            ))
            .expect("settings validate"),
        )
        .expect("settings save");

        assert!(!saved.needs_setup);
        let settings = saved.settings.as_ref().expect("saved settings");
        assert_eq!(settings.workspace_dir, workspace_dir.to_string_lossy());
        assert_eq!(settings.workspace_name, "workspace");
        assert_eq!(
            settings.evidence_dir,
            workspace_dir
                .join(LOCAL_EVIDENCE_DIR_NAME)
                .to_string_lossy()
        );
        assert_eq!(
            settings.export_dir,
            workspace_dir.join(LOCAL_EXPORT_DIR_NAME).to_string_lossy()
        );
        for directory_name in [
            LOCAL_EVIDENCE_DIR_NAME,
            LOCAL_EXPORT_DIR_NAME,
            LOCAL_REPORTS_DIR_NAME,
            LOCAL_RUNS_DIR_NAME,
            LOCAL_SOURCES_DIR_NAME,
            LOCAL_WORK_PACKAGES_DIR_NAME,
            LOCAL_MEMORY_DIR_NAME,
            LOCAL_LOGS_DIR_NAME,
        ] {
            assert!(
                workspace_dir.join(directory_name).is_dir(),
                "{directory_name} should be created under workspace"
            );
        }

        let loaded = load_local_directory_state(temp_dir.path()).expect("state reloads");
        assert_eq!(loaded, saved);
    }

    #[test]
    fn saving_local_directory_settings_creates_workspace_structure() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_dir = temp_dir.path().join("new-workspace");

        let saved = save_local_directory_settings(
            temp_dir.path(),
            LocalDirectorySettings::from_workspace_dir(workspace_dir.to_string_lossy().to_string())
                .expect("settings validate"),
        )
        .expect("workspace structure should be created");

        assert!(!saved.needs_setup);
        assert!(workspace_dir.is_dir());
        assert!(workspace_dir.join(LOCAL_EVIDENCE_DIR_NAME).is_dir());
        assert!(workspace_dir.join(LOCAL_EXPORT_DIR_NAME).is_dir());
        assert!(temp_dir.path().join(LOCAL_DIRECTORY_SETTINGS_FILE).exists());
    }

    #[test]
    fn first_run_workspace_setup_does_not_migrate_unrelated_existing_data() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let app_data_dir = temp_dir.path().join("fresh-app-data");
        let unrelated_workspace = temp_dir.path().join("old-workspace");
        let unrelated_file = unrelated_workspace
            .join(LOCAL_EVIDENCE_DIR_NAME)
            .join("keep.txt");
        fs::create_dir_all(unrelated_file.parent().expect("unrelated parent"))
            .expect("create unrelated workspace");
        fs::write(&unrelated_file, "keep in place").expect("write unrelated data");
        let new_workspace = temp_dir.path().join("new-workspace");

        save_local_directory_settings(
            &app_data_dir,
            LocalDirectorySettings::from_workspace_dir(new_workspace.to_string_lossy().to_string())
                .expect("new settings"),
        )
        .expect("first-run settings save");

        assert_eq!(
            fs::read_to_string(&unrelated_file).expect("unrelated data remains"),
            "keep in place"
        );
        assert!(!new_workspace
            .join(LOCAL_EVIDENCE_DIR_NAME)
            .join("keep.txt")
            .exists());
    }

    #[test]
    fn saving_local_directory_settings_preserves_workspace_name() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_dir = temp_dir.path().join("workspace");

        let saved = save_local_directory_settings(
            temp_dir.path(),
            LocalDirectorySettings::from_workspace_dir_and_name(
                workspace_dir.to_string_lossy().to_string(),
                "  Hotel Ops  ".to_string(),
            )
            .expect("settings validate"),
        )
        .expect("settings save");
        let loaded = load_local_directory_state(temp_dir.path()).expect("state reloads");

        assert_eq!(
            saved
                .settings
                .as_ref()
                .expect("saved settings")
                .workspace_name,
            "Hotel Ops"
        );
        assert_eq!(loaded, saved);
    }

    #[test]
    fn changing_workspace_migrates_managed_directory_data_and_rewrites_paths() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let app_data_dir = temp_dir.path().join("app-data");
        let old_workspace_dir = temp_dir.path().join("old-workspace");
        let new_workspace_dir = temp_dir.path().join("new-workspace");

        save_local_directory_settings(
            &app_data_dir,
            LocalDirectorySettings::from_workspace_dir(
                old_workspace_dir.to_string_lossy().to_string(),
            )
            .expect("old settings validate"),
        )
        .expect("old settings save");

        let old_evidence_file = old_workspace_dir
            .join(LOCAL_EVIDENCE_DIR_NAME)
            .join("source-note.txt");
        let old_export_file = old_workspace_dir
            .join(LOCAL_EXPORT_DIR_NAME)
            .join("briefing.md");
        let old_report_file = old_workspace_dir
            .join(LOCAL_REPORTS_DIR_NAME)
            .join("report.md");
        let old_package_file = old_workspace_dir
            .join(LOCAL_WORK_PACKAGES_DIR_NAME)
            .join("package.json");
        fs::write(&old_evidence_file, "source evidence").expect("write evidence");
        fs::write(&old_export_file, "export").expect("write export");
        fs::write(&old_report_file, "report").expect("write report");
        fs::write(&old_package_file, "{}").expect("write work package");

        let saved = save_local_directory_settings(
            &app_data_dir,
            LocalDirectorySettings::from_workspace_dir(
                new_workspace_dir.to_string_lossy().to_string(),
            )
            .expect("new settings validate"),
        )
        .expect("new settings save");
        let settings = saved.settings.expect("settings saved");

        assert_eq!(settings.workspace_dir, new_workspace_dir.to_string_lossy());
        assert_eq!(
            settings.evidence_dir,
            new_workspace_dir
                .join(LOCAL_EVIDENCE_DIR_NAME)
                .to_string_lossy()
        );
        assert_eq!(
            settings.export_dir,
            new_workspace_dir
                .join(LOCAL_EXPORT_DIR_NAME)
                .to_string_lossy()
        );
        assert!(new_workspace_dir
            .join(LOCAL_EVIDENCE_DIR_NAME)
            .join("source-note.txt")
            .is_file());
        assert!(new_workspace_dir
            .join(LOCAL_EXPORT_DIR_NAME)
            .join("briefing.md")
            .is_file());
        assert!(new_workspace_dir
            .join(LOCAL_REPORTS_DIR_NAME)
            .join("report.md")
            .is_file());
        assert!(new_workspace_dir
            .join(LOCAL_WORK_PACKAGES_DIR_NAME)
            .join("package.json")
            .is_file());
        assert!(!old_evidence_file.exists());
        assert!(!old_export_file.exists());
        assert!(!old_report_file.exists());
        assert!(!old_package_file.exists());
    }

    #[test]
    fn loading_local_directory_settings_recreates_missing_managed_directories() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_dir = temp_dir.path().join("workspace");

        let saved = save_local_directory_settings(
            temp_dir.path(),
            LocalDirectorySettings::from_workspace_dir(workspace_dir.to_string_lossy().to_string())
                .expect("settings validate"),
        )
        .expect("settings save");
        assert!(!saved.needs_setup);

        let evidence_dir = workspace_dir.join(LOCAL_EVIDENCE_DIR_NAME);
        fs::remove_dir_all(&evidence_dir).expect("remove evidence dir");
        let loaded = load_local_directory_state(temp_dir.path()).expect("state reloads");

        assert!(!loaded.needs_setup);
        assert!(evidence_dir.is_dir());
        assert!(loaded.settings.is_some());
    }

    #[test]
    fn loading_workspace_only_settings_derives_managed_directories() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_dir = temp_dir.path().join("workspace");
        fs::create_dir_all(&workspace_dir).expect("workspace dir");
        fs::write(
            temp_dir.path().join(LOCAL_DIRECTORY_SETTINGS_FILE),
            format!(
                "{{\"workspace_dir\":\"{}\"}}",
                workspace_dir.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .expect("write legacy settings");

        let loaded = load_local_directory_state(temp_dir.path()).expect("state reloads");
        let settings = loaded.settings.expect("settings are present");

        assert!(!loaded.needs_setup);
        assert_eq!(
            settings.evidence_dir,
            workspace_dir
                .join(LOCAL_EVIDENCE_DIR_NAME)
                .to_string_lossy()
        );
        assert!(workspace_dir.join(LOCAL_EVIDENCE_DIR_NAME).is_dir());
    }

    #[test]
    fn corrupt_workspace_settings_are_not_rewritten_or_deleted() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let settings_file = temp_dir.path().join(LOCAL_DIRECTORY_SETTINGS_FILE);
        fs::write(&settings_file, "{not valid json").expect("write corrupt settings");

        let error = load_local_directory_state(temp_dir.path())
            .expect_err("corrupt settings must be reported");

        assert!(matches!(error, super::LocalDirectoryError::Json(_)));
        assert_eq!(
            fs::read_to_string(settings_file).expect("corrupt settings preserved"),
            "{not valid json"
        );
    }

    #[test]
    fn local_directory_settings_reject_blank_required_paths() {
        let error = LocalDirectorySettings::from_workspace_dir(" ".to_string())
            .expect_err("blank workspace should fail");

        assert_eq!(error.to_string(), "workspace directory is required");
    }

    #[test]
    fn workspace_readiness_uses_bounded_probe_and_redacted_display() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_dir = temp_dir.path().join("Hotel Workspace");
        let state = save_local_directory_settings(
            temp_dir.path().join("app-data"),
            LocalDirectorySettings::from_workspace_dir_and_name(
                workspace_dir.to_string_lossy().to_string(),
                "Hotel Ops".to_string(),
            )
            .expect("settings"),
        )
        .expect("save");

        let projection = workspace_readiness_projection_from_state(&state);
        let json = serde_json::to_string(&projection).expect("projection json");

        assert_eq!(projection.code, WorkspaceReadinessCode::Ready);
        assert_eq!(projection.writable, Some(true));
        assert_eq!(projection.workspace_name.as_deref(), Some("Hotel Ops"));
        assert_eq!(
            projection.workspace_root_display.as_deref(),
            Some("Hotel Workspace")
        );
        assert!(!json.contains(&temp_dir.path().to_string_lossy().to_string()));
        assert!(fs::read_dir(&workspace_dir)
            .expect("workspace entries")
            .all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".ds-agent-readiness-")));
    }

    #[test]
    fn workspace_managed_directories_cannot_escape_root() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let workspace_dir = temp_dir.path().join("workspace");
        let outside = temp_dir.path().join("outside");
        let settings = LocalDirectorySettings::from_optional_dirs(
            workspace_dir.to_string_lossy().to_string(),
            None,
            Some(outside.to_string_lossy().to_string()),
            None,
        )
        .expect("settings parse");

        let error = save_local_directory_settings(temp_dir.path().join("app-data"), settings)
            .expect_err("escape must fail");

        assert!(matches!(
            error,
            super::LocalDirectoryError::ManagedDirectoryEscape
        ));
        assert!(!outside.exists());
    }

    #[test]
    fn workspace_probe_cleanup_failure_blocks_readiness_without_residue() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let error = workspace_write_probe_with_cleanup(temp_dir.path(), |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected cleanup failure",
            ))
        })
        .expect_err("cleanup failure must block");

        assert_eq!(error, WorkspaceReadinessCode::WorkspaceProbeCleanupFailed);
        assert!(fs::read_dir(temp_dir.path())
            .expect("root entries")
            .next()
            .is_none());
    }

    #[test]
    fn workspace_probe_maps_create_failure_to_permission_denied() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let file_root = temp_dir.path().join("not-a-directory");
        fs::write(&file_root, "occupied").expect("write file root");

        let error = workspace_write_probe_with_cleanup(&file_root, |_| Ok(()))
            .expect_err("probe create must fail");

        assert_eq!(error, WorkspaceReadinessCode::WorkspacePermissionDenied);
        assert_eq!(
            fs::read_to_string(file_root).expect("file root remains"),
            "occupied"
        );
    }

    #[test]
    fn workspace_setup_errors_map_to_stable_secret_free_codes() {
        let permission =
            workspace_readiness_projection_from_setup_error(&LocalDirectoryError::Create(
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "private path detail"),
            ));
        let invalid = workspace_readiness_projection_from_setup_error(
            &LocalDirectoryError::ManagedDirectoryEscape,
        );
        let permission_json = serde_json::to_string(&permission).expect("projection serializes");

        assert_eq!(
            permission.code,
            WorkspaceReadinessCode::WorkspacePermissionDenied
        );
        assert_eq!(
            invalid.code,
            WorkspaceReadinessCode::WorkspaceSettingsInvalid
        );
        assert!(!permission_json.contains("private path detail"));
    }
}
