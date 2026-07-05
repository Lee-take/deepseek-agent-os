use std::io::Cursor;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::kernel::codex_bridge_contract::{
    CodexBridgeCapability, CodexBridgeControlRequest, CodexBridgeControlResponse,
    CodexBridgeNetworkSearchRequest, CodexBridgeNetworkSearchResponse,
    CodexBridgeScreenshotRequest, CodexBridgeScreenshotResponse, CODEX_BRIDGE_CONTRACT_VERSION,
};
use crate::kernel::codex_bridge_http::CodexBridgeHttpClient;
use crate::kernel::computer_use::{
    bridge_endpoint_from_env, bridge_transport_from_env, BRIDGE_ENDPOINT_ENV_VAR,
    BRIDGE_TRANSPORT_ENV_VAR,
};
use crate::kernel::models::{AccessMode, LargeModelProvider, NetworkSearchSourceModel};
use crate::kernel::policy::{
    request_capability_access, CapabilityAccessRequest, CapabilityKind, PolicyDecision,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityInvocationStatus {
    Succeeded,
    PendingApproval,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BrowserBrowseRequest {
    pub access_mode: AccessMode,
    pub url: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BrowserSubmitRequest {
    pub access_mode: AccessMode,
    pub url: String,
    pub summary: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BrowserPage {
    pub final_url: String,
    pub title: String,
    pub text: String,
}

pub trait BrowserPageClient {
    fn fetch_page(&self, url: &str) -> Result<BrowserPage, String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NetworkSearchRequest {
    pub access_mode: AccessMode,
    pub query: String,
    pub scope: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NetworkSearchResultItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NetworkSearchResult {
    pub provider: String,
    pub query: String,
    pub scope: String,
    pub search_url: String,
    pub items: Vec<NetworkSearchResultItem>,
}

pub trait NetworkSearchClient {
    fn search(&self, query: &str, scope: &str) -> Result<NetworkSearchResult, String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileReadRequest {
    pub access_mode: AccessMode,
    pub path: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileWriteRequest {
    pub access_mode: AccessMode,
    pub path: String,
    pub summary: String,
    pub content: String,
    pub approval_granted: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileSystemMutationOperation {
    CreateFile,
    UpdateFile,
    DeleteFile,
    RenameFile,
    CreateDirectory,
    RenameDirectory,
    DeleteDirectory,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileSystemMutationRequest {
    pub access_mode: AccessMode,
    pub operation: FileSystemMutationOperation,
    pub path: String,
    pub destination: Option<String>,
    pub content: Option<String>,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileContent {
    pub path: String,
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default = "default_file_encoding")]
    pub encoding: String,
}

pub trait FileContentClient {
    fn read_file(&self, path: &str) -> Result<FileContent, String>;
}

fn default_file_encoding() -> String {
    "utf-8".to_string()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileWriteResult {
    pub path: String,
    pub bytes: u64,
    #[serde(default = "default_file_encoding")]
    pub encoding: String,
}

pub trait FileWriteClient {
    fn write_file(&self, path: &str, content: &str) -> Result<FileWriteResult, String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileSystemMutationResult {
    pub path: String,
    pub destination: Option<String>,
    pub bytes: u64,
    pub summary: String,
}

pub trait FileSystemMutationClient {
    fn mutate(
        &self,
        operation: FileSystemMutationOperation,
        path: &str,
        destination: Option<&str>,
        content: Option<&str>,
    ) -> Result<FileSystemMutationResult, String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceFolderRequest {
    pub access_mode: AccessMode,
    pub folder_path: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceFolderFile {
    pub path: String,
    pub title: String,
    pub text: String,
    pub bytes: u64,
    #[serde(default = "default_file_encoding")]
    pub encoding: String,
}

pub trait EvidenceFolderClient {
    fn read_text_files(&self, folder_path: &str) -> Result<Vec<EvidenceFolderFile>, String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalReadRequest {
    pub access_mode: AccessMode,
    pub command: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalWriteRequest {
    pub access_mode: AccessMode,
    pub command: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EmailSendRequest {
    pub access_mode: AccessMode,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EmailDraftRequest {
    pub access_mode: AccessMode,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EmailReadRequest {
    pub access_mode: AccessMode,
    pub mailbox: String,
    pub query: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DriveReadRequest {
    pub access_mode: AccessMode,
    pub location: String,
    pub query: String,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DriveWriteRequest {
    pub access_mode: AccessMode,
    pub location: String,
    pub summary: String,
    pub package_json: Option<String>,
    #[serde(default)]
    pub export_file: Option<DriveWriteExportFile>,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DriveWriteExportFile {
    pub file_name: String,
    pub content: String,
    #[serde(default)]
    pub content_base64: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DriveFolderEntry {
    pub path: String,
    pub title: String,
    pub bytes: u64,
    #[serde(default = "default_file_encoding")]
    pub encoding: String,
    pub excerpt: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DriveReadResult {
    pub location: String,
    pub query: String,
    pub entries: Vec<DriveFolderEntry>,
    pub total_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DriveWriteResult {
    pub path: String,
    pub bytes: u64,
}

pub trait DriveLocalFolderClient {
    fn read_local_folder(&self, location: &str, query: &str) -> Result<DriveReadResult, String>;

    fn write_export_package(
        &self,
        location: &str,
        summary: &str,
        package_json: &str,
    ) -> Result<DriveWriteResult, String>;

    fn write_export_file(
        &self,
        location: &str,
        file_name: &str,
        content: &[u8],
    ) -> Result<DriveWriteResult, String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalCommandOutput {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub trait TerminalReadClient {
    fn run_readonly_command(&self, command: &str) -> Result<TerminalCommandOutput, String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerScreenshotRequest {
    pub access_mode: AccessMode,
    pub approval_granted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerControlRequest {
    pub access_mode: AccessMode,
    pub target: String,
    pub action: String,
    pub approval_granted: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerControlMouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerControlScrollAxis {
    Vertical,
    Horizontal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComputerControlAction {
    Click {
        x: i32,
        y: i32,
        button: ComputerControlMouseButton,
    },
    Move {
        x: i32,
        y: i32,
    },
    TypeText {
        text: String,
    },
    PressKey {
        key: String,
    },
    Hotkey {
        keys: Vec<String>,
    },
    Scroll {
        delta: i32,
        axis: ComputerControlScrollAxis,
    },
}

impl ComputerControlAction {
    fn audit_summary(&self) -> String {
        match self {
            ComputerControlAction::Click { x, y, button } => {
                format!("click {button:?} at ({x}, {y})")
            }
            ComputerControlAction::Move { x, y } => format!("move pointer to ({x}, {y})"),
            ComputerControlAction::TypeText { text } => {
                format!("type text ({} chars)", text.chars().count())
            }
            ComputerControlAction::PressKey { key } => format!("press key {key}"),
            ComputerControlAction::Hotkey { keys } => format!("press hotkey {}", keys.join("+")),
            ComputerControlAction::Scroll { delta, axis } => {
                format!("scroll {axis:?} by {delta}")
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerControlExecution {
    pub summary: String,
}

pub trait ComputerControlClient {
    fn execute_control(
        &self,
        target: &str,
        action: &ComputerControlAction,
    ) -> Result<ComputerControlExecution, String>;
}

pub trait LocalComputerControlInputBackend {
    fn move_mouse_abs(&mut self, x: i32, y: i32) -> Result<(), String>;

    fn click_mouse(&mut self, button: ComputerControlMouseButton) -> Result<(), String>;

    fn type_text(&mut self, text: &str) -> Result<(), String>;

    fn key_down(&mut self, key: &str) -> Result<(), String>;

    fn key_up(&mut self, key: &str) -> Result<(), String>;

    fn key_click(&mut self, key: &str) -> Result<(), String>;

    fn scroll(&mut self, delta: i32, axis: ComputerControlScrollAxis) -> Result<(), String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerScreenshot {
    pub display_label: String,
    pub evidence_ref: String,
    pub width: u32,
    pub height: u32,
    pub captured_at: DateTime<Utc>,
}

pub trait ComputerScreenshotClient {
    fn capture_screenshot(&self) -> Result<ComputerScreenshot, String>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapturedScreenshotImage {
    pub display_label: String,
    pub width: u32,
    pub height: u32,
    pub png_bytes: Vec<u8>,
}

pub trait LocalScreenshotCaptureBackend {
    fn capture_primary_display(&self) -> Result<CapturedScreenshotImage, String>;
}

pub struct LocalFileContentClient {
    max_bytes: u64,
}

impl LocalFileContentClient {
    pub fn new(max_bytes: u64) -> Self {
        Self { max_bytes }
    }
}

impl FileContentClient for LocalFileContentClient {
    fn read_file(&self, path: &str) -> Result<FileContent, String> {
        let path_buf = std::path::PathBuf::from(path);
        let metadata = std::fs::metadata(&path_buf)
            .map_err(|error| format!("file metadata could not be read: {error}"))?;
        if !metadata.is_file() {
            return Err("selected path is not a file".to_string());
        }
        if metadata.len() > self.max_bytes {
            return Err(format!(
                "file is too large for preview: {} bytes exceeds {} bytes",
                metadata.len(),
                self.max_bytes
            ));
        }

        let text = std::fs::read_to_string(&path_buf)
            .map_err(|error| format!("file could not be read as UTF-8 text: {error}"))?;
        let title = path_buf
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .unwrap_or(path)
            .to_string();

        Ok(FileContent {
            path: path_buf.to_string_lossy().to_string(),
            title,
            text,
            bytes: metadata.len(),
            encoding: "utf-8".to_string(),
        })
    }
}

pub struct LocalWorkspaceFileWriteClient {
    workspace_dir: PathBuf,
    max_bytes: usize,
}

impl LocalWorkspaceFileWriteClient {
    pub fn new(workspace_dir: PathBuf, max_bytes: usize) -> Self {
        Self {
            workspace_dir,
            max_bytes,
        }
    }
}

impl FileWriteClient for LocalWorkspaceFileWriteClient {
    fn write_file(&self, path: &str, content: &str) -> Result<FileWriteResult, String> {
        let content_bytes = content.as_bytes();
        if content_bytes.len() > self.max_bytes {
            return Err(format!(
                "file write content is too large: {} bytes exceeds {} bytes",
                content_bytes.len(),
                self.max_bytes
            ));
        }
        let output_path = resolve_workspace_file_write_path(&self.workspace_dir, path)?;
        let parent = output_path
            .parent()
            .ok_or_else(|| "file write target parent directory is invalid".to_string())?;
        std::fs::create_dir_all(parent).map_err(|error| {
            format!("workspace file parent directory could not be created: {error}")
        })?;
        let canonical_parent = parent.canonicalize().map_err(|error| {
            format!("workspace file parent directory could not be resolved: {error}")
        })?;
        let workspace = canonical_workspace_dir(&self.workspace_dir)?;
        if !canonical_parent.starts_with(&workspace) {
            return Err("file write target must stay inside the configured workspace".to_string());
        }

        std::fs::write(&output_path, content)
            .map_err(|error| format!("workspace file could not be written: {error}"))?;

        Ok(FileWriteResult {
            path: output_path.to_string_lossy().to_string(),
            bytes: content_bytes.len() as u64,
            encoding: "utf-8".to_string(),
        })
    }
}

pub struct LocalFileSystemMutationClient;

impl FileSystemMutationClient for LocalFileSystemMutationClient {
    fn mutate(
        &self,
        operation: FileSystemMutationOperation,
        path: &str,
        destination: Option<&str>,
        content: Option<&str>,
    ) -> Result<FileSystemMutationResult, String> {
        let path = PathBuf::from(path);
        reject_root_mutation_path(&path)?;

        match operation {
            FileSystemMutationOperation::CreateFile => {
                if path.exists() {
                    return Err("local file already exists".to_string());
                }
                let body = content.unwrap_or_default();
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        format!("local file parent directory could not be created: {error}")
                    })?;
                }
                std::fs::write(&path, body)
                    .map_err(|error| format!("local file could not be created: {error}"))?;
                Ok(FileSystemMutationResult {
                    path: path.to_string_lossy().to_string(),
                    destination: None,
                    bytes: body.len() as u64,
                    summary: "created local file".to_string(),
                })
            }
            FileSystemMutationOperation::UpdateFile => {
                let metadata = std::fs::metadata(&path)
                    .map_err(|error| format!("local file metadata could not be read: {error}"))?;
                if !metadata.is_file() {
                    return Err("local file update target is not a file".to_string());
                }
                let body = content.unwrap_or_default();
                std::fs::write(&path, body)
                    .map_err(|error| format!("local file could not be updated: {error}"))?;
                Ok(FileSystemMutationResult {
                    path: path.to_string_lossy().to_string(),
                    destination: None,
                    bytes: body.len() as u64,
                    summary: "updated local file".to_string(),
                })
            }
            FileSystemMutationOperation::DeleteFile => {
                let metadata = std::fs::metadata(&path)
                    .map_err(|error| format!("local file metadata could not be read: {error}"))?;
                if !metadata.is_file() {
                    return Err("local file delete target is not a file".to_string());
                }
                let bytes = metadata.len();
                std::fs::remove_file(&path)
                    .map_err(|error| format!("local file could not be deleted: {error}"))?;
                Ok(FileSystemMutationResult {
                    path: path.to_string_lossy().to_string(),
                    destination: None,
                    bytes,
                    summary: "deleted local file".to_string(),
                })
            }
            FileSystemMutationOperation::RenameFile => {
                let destination = destination
                    .map(PathBuf::from)
                    .ok_or_else(|| "local file rename destination is required".to_string())?;
                reject_root_mutation_path(&destination)?;
                let metadata = std::fs::metadata(&path)
                    .map_err(|error| format!("local file metadata could not be read: {error}"))?;
                if !metadata.is_file() {
                    return Err("local file rename target is not a file".to_string());
                }
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        format!("local file destination directory could not be created: {error}")
                    })?;
                }
                std::fs::rename(&path, &destination)
                    .map_err(|error| format!("local file could not be renamed: {error}"))?;
                Ok(FileSystemMutationResult {
                    path: path.to_string_lossy().to_string(),
                    destination: Some(destination.to_string_lossy().to_string()),
                    bytes: metadata.len(),
                    summary: "renamed local file".to_string(),
                })
            }
            FileSystemMutationOperation::CreateDirectory => {
                std::fs::create_dir_all(&path)
                    .map_err(|error| format!("local directory could not be created: {error}"))?;
                Ok(FileSystemMutationResult {
                    path: path.to_string_lossy().to_string(),
                    destination: None,
                    bytes: 0,
                    summary: "created local directory".to_string(),
                })
            }
            FileSystemMutationOperation::RenameDirectory => {
                let destination = destination
                    .map(PathBuf::from)
                    .ok_or_else(|| "local directory rename destination is required".to_string())?;
                reject_root_mutation_path(&destination)?;
                let metadata = std::fs::metadata(&path).map_err(|error| {
                    format!("local directory metadata could not be read: {error}")
                })?;
                if !metadata.is_dir() {
                    return Err("local directory rename target is not a directory".to_string());
                }
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        format!("local directory destination parent could not be created: {error}")
                    })?;
                }
                std::fs::rename(&path, &destination)
                    .map_err(|error| format!("local directory could not be renamed: {error}"))?;
                Ok(FileSystemMutationResult {
                    path: path.to_string_lossy().to_string(),
                    destination: Some(destination.to_string_lossy().to_string()),
                    bytes: 0,
                    summary: "renamed local directory".to_string(),
                })
            }
            FileSystemMutationOperation::DeleteDirectory => {
                let metadata = std::fs::metadata(&path).map_err(|error| {
                    format!("local directory metadata could not be read: {error}")
                })?;
                if !metadata.is_dir() {
                    return Err("local directory delete target is not a directory".to_string());
                }
                std::fs::remove_dir_all(&path)
                    .map_err(|error| format!("local directory could not be deleted: {error}"))?;
                Ok(FileSystemMutationResult {
                    path: path.to_string_lossy().to_string(),
                    destination: None,
                    bytes: 0,
                    summary: "deleted local directory".to_string(),
                })
            }
        }
    }
}

pub struct LocalEvidenceFolderClient {
    max_files: usize,
    max_file_bytes: u64,
}

impl LocalEvidenceFolderClient {
    pub fn new(max_files: usize, max_file_bytes: u64) -> Self {
        Self {
            max_files,
            max_file_bytes,
        }
    }
}

impl EvidenceFolderClient for LocalEvidenceFolderClient {
    fn read_text_files(&self, folder_path: &str) -> Result<Vec<EvidenceFolderFile>, String> {
        let folder = std::path::PathBuf::from(folder_path);
        let metadata = std::fs::metadata(&folder)
            .map_err(|error| format!("folder metadata could not be read: {error}"))?;
        if !metadata.is_dir() {
            return Err("selected path is not a folder".to_string());
        }

        let mut files = Vec::new();
        for entry in std::fs::read_dir(&folder)
            .map_err(|error| format!("folder could not be listed: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("folder entry could not be read: {error}"))?;
            let path = entry.path();
            if files.len() >= self.max_files {
                break;
            }
            if !is_supported_text_file(&path) {
                continue;
            }

            let metadata = entry
                .metadata()
                .map_err(|error| format!("file metadata could not be read: {error}"))?;
            if !metadata.is_file() || metadata.len() > self.max_file_bytes {
                continue;
            }

            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("file could not be read as UTF-8 text: {error}"))?;
            let title = path
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .unwrap_or_default()
                .to_string();
            files.push(EvidenceFolderFile {
                path: path.to_string_lossy().to_string(),
                title,
                text,
                bytes: metadata.len(),
                encoding: "utf-8".to_string(),
            });
        }

        Ok(files)
    }
}

pub struct LocalDriveFolderClient {
    max_files: usize,
    max_file_bytes: u64,
}

impl LocalDriveFolderClient {
    pub fn new(max_files: usize, max_file_bytes: u64) -> Self {
        Self {
            max_files,
            max_file_bytes,
        }
    }
}

impl DriveLocalFolderClient for LocalDriveFolderClient {
    fn read_local_folder(&self, location: &str, query: &str) -> Result<DriveReadResult, String> {
        let folder = std::path::PathBuf::from(location);
        let metadata = std::fs::metadata(&folder)
            .map_err(|error| format!("local drive folder metadata could not be read: {error}"))?;
        if !metadata.is_dir() {
            return Err("selected local drive path is not a folder".to_string());
        }

        let query_lower = query.to_lowercase();
        let mut entries = Vec::new();
        let mut total_bytes = 0_u64;
        for entry in std::fs::read_dir(&folder)
            .map_err(|error| format!("local drive folder could not be listed: {error}"))?
        {
            if entries.len() >= self.max_files {
                break;
            }

            let entry = entry
                .map_err(|error| format!("local drive folder entry could not be read: {error}"))?;
            let path = entry.path();
            if !is_supported_text_file(&path) {
                continue;
            }

            let metadata = entry
                .metadata()
                .map_err(|error| format!("local drive file metadata could not be read: {error}"))?;
            if !metadata.is_file() || metadata.len() > self.max_file_bytes {
                continue;
            }

            let text = std::fs::read_to_string(&path).map_err(|error| {
                format!("local drive file could not be read as UTF-8 text: {error}")
            })?;
            let title = path
                .file_name()
                .and_then(|file_name| file_name.to_str())
                .unwrap_or_default()
                .to_string();
            let searchable = format!("{} {}", title.to_lowercase(), text.to_lowercase());
            if !searchable.contains(&query_lower) {
                continue;
            }

            total_bytes += metadata.len();
            entries.push(DriveFolderEntry {
                path: path.to_string_lossy().to_string(),
                title,
                bytes: metadata.len(),
                encoding: "utf-8".to_string(),
                excerpt: excerpt_text(&text),
            });
        }

        Ok(DriveReadResult {
            location: folder.to_string_lossy().to_string(),
            query: query.to_string(),
            entries,
            total_bytes,
        })
    }

    fn write_export_package(
        &self,
        location: &str,
        _summary: &str,
        package_json: &str,
    ) -> Result<DriveWriteResult, String> {
        let folder = std::path::PathBuf::from(location);
        let metadata = std::fs::metadata(&folder)
            .map_err(|error| format!("local export folder metadata could not be read: {error}"))?;
        if !metadata.is_dir() {
            return Err("selected local export path is not a folder".to_string());
        }

        let file_name = format!("deepseek-agent-os-work-package-{}.json", Uuid::new_v4());
        let output_path = folder.join(file_name);
        std::fs::write(&output_path, package_json)
            .map_err(|error| format!("local work package export could not be written: {error}"))?;

        Ok(DriveWriteResult {
            path: output_path.to_string_lossy().to_string(),
            bytes: package_json.len() as u64,
        })
    }

    fn write_export_file(
        &self,
        location: &str,
        file_name: &str,
        content: &[u8],
    ) -> Result<DriveWriteResult, String> {
        let folder = std::path::PathBuf::from(location);
        let metadata = std::fs::metadata(&folder)
            .map_err(|error| format!("local export folder metadata could not be read: {error}"))?;
        if !metadata.is_dir() {
            return Err("selected local export path is not a folder".to_string());
        }

        let file_name = normalize_export_file_name(file_name)?;
        let output_path = folder.join(file_name);
        std::fs::write(&output_path, content)
            .map_err(|error| format!("local export file could not be written: {error}"))?;

        Ok(DriveWriteResult {
            path: output_path.to_string_lossy().to_string(),
            bytes: content.len() as u64,
        })
    }
}

pub struct LocalTerminalReadClient {
    working_dir: std::path::PathBuf,
    max_output_chars: usize,
}

const TERMINAL_READ_DIRECTORY_LIST_PREFIX: &str = "ds-agent:list-directory ";
const TERMINAL_READ_DIRECTORY_ENTRY_LIMIT: usize = 100;

impl LocalTerminalReadClient {
    pub fn new(working_dir: std::path::PathBuf, max_output_chars: usize) -> Self {
        Self {
            working_dir,
            max_output_chars,
        }
    }

    fn list_directory(&self, location: &str) -> Result<TerminalCommandOutput, String> {
        let folder = PathBuf::from(location);
        let metadata = std::fs::metadata(&folder)
            .map_err(|error| format!("local directory metadata could not be read: {error}"))?;
        if !metadata.is_dir() {
            return Err("local directory listing target is not a directory".to_string());
        }

        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&folder)
            .map_err(|error| format!("local directory could not be listed: {error}"))?
        {
            if entries.len() >= TERMINAL_READ_DIRECTORY_ENTRY_LIMIT {
                break;
            }

            let entry = entry
                .map_err(|error| format!("local directory entry could not be read: {error}"))?;
            let metadata = entry.metadata().map_err(|error| {
                format!("local directory entry metadata could not be read: {error}")
            })?;
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = if metadata.is_dir() { "dir" } else { "file" };
            let bytes = if metadata.is_file() {
                metadata.len().to_string()
            } else {
                "-".to_string()
            };
            entries.push((name, kind.to_string(), bytes));
        }
        entries.sort_by_key(|(name, _, _)| name.to_ascii_lowercase());

        let mut lines = vec!["Name\tType\tBytes".to_string()];
        if entries.is_empty() {
            lines.push("(empty directory)\tdir\t-".to_string());
        } else {
            lines.extend(
                entries
                    .into_iter()
                    .map(|(name, kind, bytes)| format!("{name}\t{kind}\t{bytes}")),
            );
        }
        lines.push(format!(
            "Limit\tInfo\t{} entries shown",
            TERMINAL_READ_DIRECTORY_ENTRY_LIMIT
        ));

        Ok(TerminalCommandOutput {
            command: format!("{TERMINAL_READ_DIRECTORY_LIST_PREFIX}{location}"),
            stdout: truncate_chars(&lines.join("\n"), self.max_output_chars),
            stderr: String::new(),
            exit_code: 0,
        })
    }
}

impl TerminalReadClient for LocalTerminalReadClient {
    fn run_readonly_command(&self, command: &str) -> Result<TerminalCommandOutput, String> {
        if let Some(location) = command.strip_prefix(TERMINAL_READ_DIRECTORY_LIST_PREFIX) {
            return self.list_directory(location.trim());
        }

        let output = if cfg!(windows) {
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", command])
                .current_dir(&self.working_dir)
                .output()
        } else {
            std::process::Command::new("sh")
                .args(["-lc", command])
                .current_dir(&self.working_dir)
                .output()
        }
        .map_err(|error| format!("terminal command could not be started: {error}"))?;

        Ok(TerminalCommandOutput {
            command: command.to_string(),
            stdout: truncate_chars(
                &String::from_utf8_lossy(&output.stdout),
                self.max_output_chars,
            ),
            stderr: truncate_chars(
                &String::from_utf8_lossy(&output.stderr),
                self.max_output_chars,
            ),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }
}

pub struct LocalComputerScreenshotClient {
    evidence_base_dir: PathBuf,
}

impl LocalComputerScreenshotClient {
    pub fn new(evidence_base_dir: PathBuf) -> Self {
        Self { evidence_base_dir }
    }

    pub fn capture_with_backend(
        &self,
        backend: &impl LocalScreenshotCaptureBackend,
    ) -> Result<ComputerScreenshot, String> {
        let captured = backend.capture_primary_display()?;
        if captured.width == 0 || captured.height == 0 {
            return Err("local screen inspection returned empty dimensions".to_string());
        }
        if captured.png_bytes.is_empty() {
            return Err("local screen inspection returned empty PNG bytes".to_string());
        }

        let display_label = non_empty_string(captured.display_label)
            .unwrap_or_else(|| "Primary display".to_string());
        let evidence_ref = write_computer_screenshot_evidence(
            &self.evidence_base_dir,
            &display_label,
            &captured.png_bytes,
        )?;

        Ok(ComputerScreenshot {
            display_label,
            evidence_ref,
            width: captured.width,
            height: captured.height,
            captured_at: Utc::now(),
        })
    }
}

impl ComputerScreenshotClient for LocalComputerScreenshotClient {
    fn capture_screenshot(&self) -> Result<ComputerScreenshot, String> {
        self.capture_with_backend(&XcapLocalScreenshotCaptureBackend)
    }
}

struct XcapLocalScreenshotCaptureBackend;

impl LocalScreenshotCaptureBackend for XcapLocalScreenshotCaptureBackend {
    fn capture_primary_display(&self) -> Result<CapturedScreenshotImage, String> {
        let monitors = xcap::Monitor::all().map_err(|error| {
            format!("local screen inspection display enumeration failed: {error}")
        })?;
        let monitor = monitors
            .iter()
            .find(|monitor| monitor.is_primary().unwrap_or(false))
            .or_else(|| monitors.first())
            .ok_or_else(|| "local screen inspection found no display to inspect".to_string())?;
        let display_label = monitor
            .friendly_name()
            .or_else(|_| monitor.name())
            .unwrap_or_else(|_| "Primary display".to_string());
        let image = monitor
            .capture_image()
            .map_err(|error| format!("local screen inspection failed: {error}"))?;
        let width = image.width();
        let height = image.height();
        let dynamic_image = xcap::image::DynamicImage::ImageRgba8(image);
        let mut buffer = Cursor::new(Vec::new());
        dynamic_image
            .write_to(&mut buffer, xcap::image::ImageFormat::Png)
            .map_err(|error| format!("local screen inspection PNG encoding failed: {error}"))?;

        Ok(CapturedScreenshotImage {
            display_label,
            width,
            height,
            png_bytes: buffer.into_inner(),
        })
    }
}

enum CodexBridgeClientRuntime {
    Unconfigured,
    SetupError(String),
    Http(CodexBridgeHttpClient),
}

fn codex_bridge_runtime_from_env() -> CodexBridgeClientRuntime {
    match bridge_transport_from_env() {
        Some(transport) if transport.trim().eq_ignore_ascii_case("http") => {}
        Some(transport) if transport.trim().is_empty() => return CodexBridgeClientRuntime::Unconfigured,
        Some(transport) => {
            return CodexBridgeClientRuntime::SetupError(format!(
                "Local bridge service transport '{transport}' is selected, but this build only executes the HTTP bridge transport"
            ))
        }
        None => return CodexBridgeClientRuntime::Unconfigured,
    }

    let endpoint = match bridge_endpoint_from_env() {
        Some(endpoint) if !endpoint.trim().is_empty() => endpoint,
        _ => return CodexBridgeClientRuntime::Unconfigured,
    };

    match CodexBridgeHttpClient::new(&endpoint, codex_bridge_http_timeout()) {
        Ok(client) => CodexBridgeClientRuntime::Http(client),
        Err(error) => CodexBridgeClientRuntime::SetupError(error),
    }
}

fn codex_bridge_http_timeout() -> Duration {
    Duration::from_secs(10)
}

pub struct CodexBridgeNetworkSearchClient {
    runtime: CodexBridgeClientRuntime,
    large_model_provider: LargeModelProvider,
}

impl CodexBridgeNetworkSearchClient {
    pub fn from_env(large_model_provider: LargeModelProvider) -> Self {
        Self {
            runtime: codex_bridge_runtime_from_env(),
            large_model_provider,
        }
    }

    #[cfg(test)]
    pub fn with_http_endpoint(
        large_model_provider: LargeModelProvider,
        endpoint: &str,
    ) -> Result<Self, String> {
        Ok(Self {
            runtime: CodexBridgeClientRuntime::Http(CodexBridgeHttpClient::new(
                endpoint,
                codex_bridge_http_timeout(),
            )?),
            large_model_provider,
        })
    }
}

impl NetworkSearchClient for CodexBridgeNetworkSearchClient {
    fn search(&self, query: &str, scope: &str) -> Result<NetworkSearchResult, String> {
        let http_client = match &self.runtime {
            CodexBridgeClientRuntime::Http(client) => client,
            CodexBridgeClientRuntime::SetupError(error) => return Err(error.clone()),
            CodexBridgeClientRuntime::Unconfigured => {
                return Err(format!(
                    "Selected model web search through the local bridge service requires {BRIDGE_TRANSPORT_ENV_VAR}=http and {BRIDGE_ENDPOINT_ENV_VAR} pointing to a local bridge service"
                ))
            }
        };
        let request =
            CodexBridgeNetworkSearchRequest::new(self.large_model_provider, query, scope)?;
        let response = http_client.network_search(&request)?;
        validate_codex_bridge_network_search_response(&response)?;

        Ok(NetworkSearchResult {
            provider: response.provider.trim().to_string(),
            query: response.query.trim().to_string(),
            scope: response.scope.trim().to_string(),
            search_url: response.search_url.trim().to_string(),
            items: response
                .items
                .iter()
                .map(|item| NetworkSearchResultItem {
                    title: item.title.trim().to_string(),
                    url: item.url.trim().to_string(),
                    snippet: item.snippet.trim().to_string(),
                })
                .collect(),
        })
    }
}

pub struct CodexBridgeComputerScreenshotClient {
    runtime: CodexBridgeClientRuntime,
    evidence_base_dir: Option<PathBuf>,
}

impl CodexBridgeComputerScreenshotClient {
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            runtime: CodexBridgeClientRuntime::Unconfigured,
            evidence_base_dir: None,
        }
    }

    pub fn from_env(evidence_base_dir: PathBuf) -> Self {
        Self {
            runtime: codex_bridge_runtime_from_env(),
            evidence_base_dir: Some(evidence_base_dir),
        }
    }

    #[cfg(test)]
    pub fn with_http_endpoint(endpoint: &str, evidence_base_dir: PathBuf) -> Result<Self, String> {
        Ok(Self {
            runtime: CodexBridgeClientRuntime::Http(CodexBridgeHttpClient::new(
                endpoint,
                codex_bridge_http_timeout(),
            )?),
            evidence_base_dir: Some(evidence_base_dir),
        })
    }
}

impl ComputerScreenshotClient for CodexBridgeComputerScreenshotClient {
    fn capture_screenshot(&self) -> Result<ComputerScreenshot, String> {
        let http_client = match &self.runtime {
            CodexBridgeClientRuntime::Http(client) => client,
            CodexBridgeClientRuntime::SetupError(error) => return Err(error.clone()),
            CodexBridgeClientRuntime::Unconfigured => {
                return Err(format!(
                    "Local bridge service screen inspection requires {BRIDGE_TRANSPORT_ENV_VAR}=http and {BRIDGE_ENDPOINT_ENV_VAR} pointing to a local bridge service"
                ))
            }
        };
        let evidence_base_dir = self.evidence_base_dir.as_ref().ok_or_else(|| {
            "Local bridge service screen inspection requires a local evidence directory".to_string()
        })?;
        let response = http_client.screenshot(&CodexBridgeScreenshotRequest::new(None))?;
        validate_codex_bridge_screenshot_response(&response)?;
        let png_bytes = general_purpose::STANDARD
            .decode(response.png_base64.trim())
            .map_err(|error| {
                format!("local bridge service screenshot PNG base64 could not be decoded: {error}")
            })?;
        if png_bytes.is_empty() {
            return Err("local bridge service screenshot returned empty PNG bytes".to_string());
        }
        let display_label = response.display_label.trim().to_string();
        let evidence_ref =
            write_computer_screenshot_evidence(evidence_base_dir, &display_label, &png_bytes)?;

        Ok(ComputerScreenshot {
            display_label,
            evidence_ref,
            width: response.width,
            height: response.height,
            captured_at: response.captured_at,
        })
    }
}

pub struct LocalComputerControlClient;

impl LocalComputerControlClient {
    pub fn new() -> Self {
        Self
    }

    pub fn execute_with_backend(
        &self,
        action: &ComputerControlAction,
        backend: &mut impl LocalComputerControlInputBackend,
    ) -> Result<ComputerControlExecution, String> {
        match action {
            ComputerControlAction::Click { x, y, button } => {
                backend.move_mouse_abs(*x, *y)?;
                backend.click_mouse(*button)?;
            }
            ComputerControlAction::Move { x, y } => {
                backend.move_mouse_abs(*x, *y)?;
            }
            ComputerControlAction::TypeText { text } => {
                backend.type_text(text)?;
            }
            ComputerControlAction::PressKey { key } => {
                backend.key_click(key)?;
            }
            ComputerControlAction::Hotkey { keys } => {
                let mut pressed: Vec<&str> = Vec::new();
                for key in keys {
                    if let Err(error) = backend.key_down(key) {
                        for pressed_key in pressed.iter().rev() {
                            let _ = backend.key_up(pressed_key);
                        }
                        return Err(error);
                    }
                    pressed.push(key.as_str());
                }
                for key in pressed.iter().rev() {
                    backend.key_up(key)?;
                }
            }
            ComputerControlAction::Scroll { delta, axis } => {
                backend.scroll(*delta, *axis)?;
            }
        }

        Ok(ComputerControlExecution {
            summary: action.audit_summary(),
        })
    }
}

impl ComputerControlClient for LocalComputerControlClient {
    fn execute_control(
        &self,
        _target: &str,
        action: &ComputerControlAction,
    ) -> Result<ComputerControlExecution, String> {
        let mut backend = EnigoLocalComputerControlInputBackend::new()?;
        self.execute_with_backend(action, &mut backend)
    }
}

pub struct CodexBridgeComputerControlClient {
    runtime: CodexBridgeClientRuntime,
}

impl CodexBridgeComputerControlClient {
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            runtime: CodexBridgeClientRuntime::Unconfigured,
        }
    }

    pub fn from_env() -> Self {
        Self {
            runtime: codex_bridge_runtime_from_env(),
        }
    }

    #[cfg(test)]
    pub fn with_http_endpoint(endpoint: &str) -> Result<Self, String> {
        Ok(Self {
            runtime: CodexBridgeClientRuntime::Http(CodexBridgeHttpClient::new(
                endpoint,
                codex_bridge_http_timeout(),
            )?),
        })
    }
}

impl ComputerControlClient for CodexBridgeComputerControlClient {
    fn execute_control(
        &self,
        target: &str,
        action: &ComputerControlAction,
    ) -> Result<ComputerControlExecution, String> {
        let http_client = match &self.runtime {
            CodexBridgeClientRuntime::Http(client) => client,
            CodexBridgeClientRuntime::SetupError(error) => return Err(error.clone()),
            CodexBridgeClientRuntime::Unconfigured => {
                return Err(format!(
                    "Local bridge service mouse and keyboard control requires {BRIDGE_TRANSPORT_ENV_VAR}=http and {BRIDGE_ENDPOINT_ENV_VAR} pointing to a local bridge service"
                ))
            }
        };
        let request = CodexBridgeControlRequest::new(
            target,
            &computer_control_action_contract_string(action),
        )?;
        let response = http_client.control(&request)?;
        validate_codex_bridge_control_response(&response)?;

        Ok(ComputerControlExecution {
            summary: response.summary.trim().to_string(),
        })
    }
}

struct EnigoLocalComputerControlInputBackend {
    enigo: enigo::Enigo,
}

impl EnigoLocalComputerControlInputBackend {
    fn new() -> Result<Self, String> {
        let enigo = enigo::Enigo::new(&enigo::Settings::default())
            .map_err(|error| format!("local mouse and keyboard control setup failed: {error}"))?;
        Ok(Self { enigo })
    }
}

impl LocalComputerControlInputBackend for EnigoLocalComputerControlInputBackend {
    fn move_mouse_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        enigo::Mouse::move_mouse(&mut self.enigo, x, y, enigo::Coordinate::Abs)
            .map_err(|error| format!("computer control mouse move failed: {error}"))
    }

    fn click_mouse(&mut self, button: ComputerControlMouseButton) -> Result<(), String> {
        enigo::Mouse::button(
            &mut self.enigo,
            enigo_button(button),
            enigo::Direction::Click,
        )
        .map_err(|error| format!("computer control mouse click failed: {error}"))
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        enigo::Keyboard::text(&mut self.enigo, text)
            .map_err(|error| format!("computer control text input failed: {error}"))
    }

    fn key_down(&mut self, key: &str) -> Result<(), String> {
        let key = enigo_key(key)?;
        enigo::Keyboard::key(&mut self.enigo, key, enigo::Direction::Press)
            .map_err(|error| format!("computer control key press failed: {error}"))
    }

    fn key_up(&mut self, key: &str) -> Result<(), String> {
        let key = enigo_key(key)?;
        enigo::Keyboard::key(&mut self.enigo, key, enigo::Direction::Release)
            .map_err(|error| format!("computer control key release failed: {error}"))
    }

    fn key_click(&mut self, key: &str) -> Result<(), String> {
        let key = enigo_key(key)?;
        enigo::Keyboard::key(&mut self.enigo, key, enigo::Direction::Click)
            .map_err(|error| format!("computer control key click failed: {error}"))
    }

    fn scroll(&mut self, delta: i32, axis: ComputerControlScrollAxis) -> Result<(), String> {
        enigo::Mouse::scroll(&mut self.enigo, delta, enigo_axis(axis))
            .map_err(|error| format!("computer control scroll failed: {error}"))
    }
}

fn enigo_button(button: ComputerControlMouseButton) -> enigo::Button {
    match button {
        ComputerControlMouseButton::Left => enigo::Button::Left,
        ComputerControlMouseButton::Middle => enigo::Button::Middle,
        ComputerControlMouseButton::Right => enigo::Button::Right,
    }
}

fn enigo_axis(axis: ComputerControlScrollAxis) -> enigo::Axis {
    match axis {
        ComputerControlScrollAxis::Vertical => enigo::Axis::Vertical,
        ComputerControlScrollAxis::Horizontal => enigo::Axis::Horizontal,
    }
}

fn enigo_key(key: &str) -> Result<enigo::Key, String> {
    if key.len() == 1 {
        let character = key
            .chars()
            .next()
            .ok_or_else(|| "key name is required".to_string())?;
        return Ok(enigo::Key::Unicode(character));
    }

    match key {
        "ctrl" => Ok(enigo::Key::Control),
        "shift" => Ok(enigo::Key::Shift),
        "alt" => Ok(enigo::Key::Alt),
        "meta" => Ok(enigo::Key::Meta),
        "enter" => Ok(enigo::Key::Return),
        "tab" => Ok(enigo::Key::Tab),
        "escape" => Ok(enigo::Key::Escape),
        "backspace" => Ok(enigo::Key::Backspace),
        "delete" => Ok(enigo::Key::Delete),
        "space" => Ok(enigo::Key::Space),
        "up" => Ok(enigo::Key::UpArrow),
        "down" => Ok(enigo::Key::DownArrow),
        "left" => Ok(enigo::Key::LeftArrow),
        "right" => Ok(enigo::Key::RightArrow),
        _ => Err(format!("unsupported key name '{key}'")),
    }
}

pub struct HttpBrowserPageClient {
    client: reqwest::blocking::Client,
}

impl HttpBrowserPageClient {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("DeepSeek-Agent-OS/0.1.0 browser-capability")
            .timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|error| format!("browser client setup failed: {error}"))?;

        Ok(Self { client })
    }
}

impl BrowserPageClient for HttpBrowserPageClient {
    fn fetch_page(&self, url: &str) -> Result<BrowserPage, String> {
        let response = self
            .client
            .get(url)
            .send()
            .map_err(|error| format!("browser fetch failed: {error}"))?;
        let final_url = response.url().to_string();
        let html = response
            .error_for_status()
            .map_err(|error| format!("browser fetch returned an error status: {error}"))?
            .text()
            .map_err(|error| format!("browser response could not be read: {error}"))?;

        Ok(BrowserPage {
            final_url,
            title: extract_html_title(&html),
            text: html_to_text(&html),
        })
    }
}

pub struct HttpNetworkSearchClient {
    client: reqwest::blocking::Client,
    source_model: NetworkSearchSourceModel,
}

impl HttpNetworkSearchClient {
    pub fn new(source_model: NetworkSearchSourceModel) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("DeepSeek-Agent-OS/0.1.0 network-search")
            .timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|error| format!("network search client setup failed: {error}"))?;

        Ok(Self {
            client,
            source_model,
        })
    }

    fn provider_label(&self) -> &'static str {
        match self.source_model {
            NetworkSearchSourceModel::FreeWebSource => "free web source search",
            NetworkSearchSourceModel::FreeLocalBrowser => "free local browser search",
            NetworkSearchSourceModel::FreeSourceAggregator => "free source aggregator search",
        }
    }
}

impl NetworkSearchClient for HttpNetworkSearchClient {
    fn search(&self, query: &str, scope: &str) -> Result<NetworkSearchResult, String> {
        let search_query = if scope.eq_ignore_ascii_case("public web") {
            query.to_string()
        } else {
            format!("{query} {scope}")
        };
        let search_url = reqwest::Url::parse_with_params(
            "https://duckduckgo.com/html/",
            &[("q", search_query.as_str())],
        )
        .map_err(|error| format!("network search URL could not be built: {error}"))?;

        let response = self
            .client
            .get(search_url.clone())
            .send()
            .map_err(|error| format!("network search request failed: {error}"))?;
        let html = response
            .error_for_status()
            .map_err(|error| format!("network search returned an error status: {error}"))?
            .text()
            .map_err(|error| format!("network search response could not be read: {error}"))?;
        let items = extract_search_result_items(&html, 5);

        if items.is_empty() {
            return Err("source-backed search returned no source links".to_string());
        }

        Ok(NetworkSearchResult {
            provider: self.provider_label().to_string(),
            query: query.to_string(),
            scope: scope.to_string(),
            search_url: search_url.to_string(),
            items,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapabilityInvocation {
    pub id: Uuid,
    pub capability: CapabilityKind,
    pub status: CapabilityInvocationStatus,
    pub policy_decision: PolicyDecision,
    #[serde(default)]
    pub approval_request_id: Option<Uuid>,
    pub requested_resource: Option<String>,
    pub evidence_ref: Option<String>,
    pub requested_url: Option<String>,
    pub evidence_url: Option<String>,
    pub title: Option<String>,
    pub excerpt: Option<String>,
    pub warnings: Vec<String>,
    pub elapsed_ms: u128,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BrowserBrowseOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BrowserSubmitOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NetworkSearchOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileReadOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceFolderOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalReadOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalWriteOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerScreenshotOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerControlOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EmailSendOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EmailDraftOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EmailReadOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DriveReadOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DriveWriteOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileWriteOutcome {
    pub access_request: CapabilityAccessRequest,
    pub invocation: CapabilityInvocation,
}

pub fn run_browser_browse(
    request: BrowserBrowseRequest,
    client: &impl BrowserPageClient,
) -> Result<BrowserBrowseOutcome, String> {
    let normalized_url = normalize_browser_url(&request.url)?;
    let started_at = Instant::now();
    let access_request =
        request_capability_access(request.access_mode, CapabilityKind::BrowserBrowse)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(BrowserBrowseOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::BrowserBrowse,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(normalized_url.clone()),
                evidence_ref: Some(normalized_url.clone()),
                requested_url: Some(normalized_url.clone()),
                evidence_url: Some(normalized_url),
                title: None,
                excerpt: None,
                warnings: vec![
                    "browser browse requires approval before fetching the page".to_string()
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let page = match client.fetch_page(&normalized_url) {
        Ok(page) => page,
        Err(error) => {
            return Ok(BrowserBrowseOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::BrowserBrowse,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(normalized_url.clone()),
                    evidence_ref: Some(normalized_url.clone()),
                    requested_url: Some(normalized_url.clone()),
                    evidence_url: Some(normalized_url),
                    title: None,
                    excerpt: None,
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    Ok(BrowserBrowseOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::BrowserBrowse,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(normalized_url.clone()),
            evidence_ref: Some(page.final_url.clone()),
            requested_url: Some(normalized_url),
            evidence_url: Some(page.final_url),
            title: non_empty_string(page.title),
            excerpt: non_empty_string(excerpt_text(&page.text)),
            warnings: Vec::new(),
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_browser_submit_boundary(
    request: BrowserSubmitRequest,
) -> Result<BrowserSubmitOutcome, String> {
    let normalized_url = normalize_browser_url(&request.url)?;
    let summary = normalize_browser_submit_summary(&request.summary)?;
    let started_at = Instant::now();
    let access_request =
        request_capability_access(request.access_mode, CapabilityKind::BrowserSubmit)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(BrowserSubmitOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::BrowserSubmit,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(normalized_url.clone()),
                evidence_ref: Some(normalized_url.clone()),
                requested_url: Some(normalized_url.clone()),
                evidence_url: Some(normalized_url),
                title: Some(format!("Browser submit request: {summary}")),
                excerpt: Some(excerpt_text(&summary)),
                warnings: vec![
                    "browser submit requires explicit approval in this access mode".to_string(),
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    Ok(BrowserSubmitOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::BrowserSubmit,
            status: CapabilityInvocationStatus::Failed,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(normalized_url.clone()),
            evidence_ref: Some(normalized_url.clone()),
            requested_url: Some(normalized_url.clone()),
            evidence_url: Some(normalized_url.clone()),
            title: Some(format!("Browser submit blocked: {normalized_url}")),
            excerpt: Some(excerpt_text(&summary)),
            warnings: vec![
                "browser submit execution is not enabled in boundary v1; no form was submitted"
                    .to_string(),
            ],
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_network_search_boundary(
    request: NetworkSearchRequest,
    client: &impl NetworkSearchClient,
) -> Result<NetworkSearchOutcome, String> {
    let query = normalize_network_search_query(&request.query)?;
    let scope = normalize_network_search_scope(&request.scope);
    let requested_resource = format!("{scope}: {query}");
    let started_at = Instant::now();
    let access_request =
        request_capability_access(request.access_mode, CapabilityKind::NetworkSearch)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(NetworkSearchOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::NetworkSearch,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(requested_resource.clone()),
                evidence_ref: Some(requested_resource),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("Network search request: {query}")),
                excerpt: Some(excerpt_text(&scope)),
                warnings: vec![
                    "network search requires explicit approval in this access mode".to_string(),
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let result = match client.search(&query, &scope) {
        Ok(result) => result,
        Err(error) => {
            return Ok(NetworkSearchOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::NetworkSearch,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(requested_resource.clone()),
                    evidence_ref: Some(requested_resource),
                    requested_url: None,
                    evidence_url: None,
                    title: Some(format!("Network search failed: {query}")),
                    excerpt: Some(excerpt_text(&scope)),
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    if result.items.is_empty() {
        return Ok(NetworkSearchOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::NetworkSearch,
                status: CapabilityInvocationStatus::Failed,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(requested_resource.clone()),
                evidence_ref: Some(requested_resource),
                requested_url: Some(result.search_url),
                evidence_url: None,
                title: Some(format!("Network search failed: {query}")),
                excerpt: Some(excerpt_text(&scope)),
                warnings: vec!["source-backed search returned no source links".to_string()],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let first_result_url = result.items.first().map(|item| item.url.clone());
    let search_url = result.search_url.clone();
    let provider_label = network_search_provider_label(&result);
    let excerpt = network_search_excerpt(&result);
    Ok(NetworkSearchOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::NetworkSearch,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(requested_resource.clone()),
            evidence_ref: first_result_url.clone(),
            requested_url: Some(search_url),
            evidence_url: first_result_url,
            title: Some(format!(
                "Network search results via {provider_label}: {query}"
            )),
            excerpt: Some(excerpt),
            warnings: Vec::new(),
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

fn network_search_provider_label(result: &NetworkSearchResult) -> &str {
    let provider = result.provider.trim();
    if provider.is_empty() {
        "source-backed search"
    } else {
        provider
    }
}

pub fn run_file_read(
    request: FileReadRequest,
    client: &impl FileContentClient,
) -> Result<FileReadOutcome, String> {
    let normalized_path = normalize_file_path(&request.path)?;
    let started_at = Instant::now();
    let access_request = request_capability_access(request.access_mode, CapabilityKind::FileRead)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(FileReadOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileRead,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(normalized_path.clone()),
                evidence_ref: Some(normalized_path),
                requested_url: None,
                evidence_url: None,
                title: None,
                excerpt: None,
                warnings: vec!["file read requires approval before reading the file".to_string()],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let file = match client.read_file(&normalized_path) {
        Ok(file) => file,
        Err(error) => {
            return Ok(FileReadOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::FileRead,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(normalized_path.clone()),
                    evidence_ref: Some(normalized_path),
                    requested_url: None,
                    evidence_url: None,
                    title: None,
                    excerpt: None,
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    let metadata_warning = file_read_metadata_warning(&file);

    Ok(FileReadOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::FileRead,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(normalized_path),
            evidence_ref: Some(file.path),
            requested_url: None,
            evidence_url: None,
            title: non_empty_string(file.title),
            excerpt: non_empty_string(excerpt_text(&file.text)),
            warnings: vec![metadata_warning],
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_file_write_boundary(
    request: FileWriteRequest,
    client: &impl FileWriteClient,
) -> Result<FileWriteOutcome, String> {
    let path = normalize_file_write_field(&request.path, "file write path")?;
    let summary = normalize_file_write_field(&request.summary, "file write summary")?;
    let content = validate_file_write_content(&request.content)?;
    let started_at = Instant::now();
    let access_request = request_capability_access(request.access_mode, CapabilityKind::FileWrite)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(FileWriteOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileWrite,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(path.clone()),
                evidence_ref: Some(path),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("File write request: {summary}")),
                excerpt: Some(excerpt_text(&summary)),
                warnings: vec![
                    "file write requires explicit approval in this access mode".to_string()
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let write_result = match client.write_file(&path, &content) {
        Ok(write_result) => write_result,
        Err(error) => {
            return Ok(FileWriteOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::FileWrite,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(path.clone()),
                    evidence_ref: Some(path.clone()),
                    requested_url: None,
                    evidence_url: None,
                    title: Some(format!("File write failed: {path}")),
                    excerpt: Some(excerpt_text(&summary)),
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    Ok(FileWriteOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::FileWrite,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(path.clone()),
            evidence_ref: Some(write_result.path.clone()),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("File written: {path}")),
            excerpt: Some(format!(
                "{} ({} text, {} bytes written)",
                excerpt_text(&summary),
                file_write_result_encoding(&write_result),
                write_result.bytes
            )),
            warnings: Vec::new(),
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_filesystem_mutation_boundary(
    request: FileSystemMutationRequest,
    client: &impl FileSystemMutationClient,
) -> Result<FileWriteOutcome, String> {
    let path = normalize_filesystem_path_field(&request.path, "filesystem path")?;
    let destination = request
        .destination
        .as_deref()
        .map(|value| normalize_filesystem_path_field(value, "filesystem destination"))
        .transpose()?;
    let content = match request.operation {
        FileSystemMutationOperation::CreateFile | FileSystemMutationOperation::UpdateFile => Some(
            validate_filesystem_mutation_content(request.content.as_deref())?,
        ),
        _ => None,
    };
    validate_filesystem_mutation_request(request.operation, &path, destination.as_deref())?;
    let started_at = Instant::now();
    let access_request = request_capability_access(request.access_mode, CapabilityKind::FileWrite)?;
    let operation_label = filesystem_mutation_operation_label(request.operation);
    let target_summary = filesystem_mutation_target_summary(&path, destination.as_deref());

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(FileWriteOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileWrite,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(path.clone()),
                evidence_ref: Some(path),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("File system request: {operation_label}")),
                excerpt: Some(excerpt_text(&target_summary)),
                warnings: vec![
                    "file system mutation requires explicit approval in this access mode"
                        .to_string(),
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let mutation_result = match client.mutate(
        request.operation,
        &path,
        destination.as_deref(),
        content.as_deref(),
    ) {
        Ok(result) => result,
        Err(error) => {
            return Ok(FileWriteOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::FileWrite,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(path.clone()),
                    evidence_ref: destination.clone().or(Some(path.clone())),
                    requested_url: None,
                    evidence_url: None,
                    title: Some(format!("File system mutation failed: {operation_label}")),
                    excerpt: Some(excerpt_text(&target_summary)),
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    let evidence_ref = mutation_result
        .destination
        .clone()
        .unwrap_or_else(|| mutation_result.path.clone());
    Ok(FileWriteOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::FileWrite,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(path),
            evidence_ref: Some(evidence_ref),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("File system mutation: {operation_label}")),
            excerpt: Some(format!(
                "{} ({} bytes)",
                mutation_result.summary, mutation_result.bytes
            )),
            warnings: Vec::new(),
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

fn file_write_result_encoding(result: &FileWriteResult) -> &str {
    let encoding = result.encoding.trim();
    if encoding.is_empty() {
        "utf-8"
    } else {
        encoding
    }
}

pub fn run_evidence_folder_ingest(
    request: EvidenceFolderRequest,
    client: &impl EvidenceFolderClient,
) -> Result<EvidenceFolderOutcome, String> {
    let normalized_folder = normalize_file_path(&request.folder_path)?;
    let started_at = Instant::now();
    let access_request = request_capability_access(request.access_mode, CapabilityKind::FileRead)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(EvidenceFolderOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::FileRead,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(normalized_folder.clone()),
                evidence_ref: Some(normalized_folder),
                requested_url: None,
                evidence_url: None,
                title: None,
                excerpt: None,
                warnings: vec![
                    "evidence folder ingestion requires approval before scanning files".to_string(),
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let files = match client.read_text_files(&normalized_folder) {
        Ok(files) => files,
        Err(error) => {
            return Ok(EvidenceFolderOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::FileRead,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(normalized_folder.clone()),
                    evidence_ref: Some(normalized_folder),
                    requested_url: None,
                    evidence_url: None,
                    title: None,
                    excerpt: None,
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    let total_bytes = files.iter().map(|file| file.bytes).sum::<u64>();
    let file_names = files
        .iter()
        .map(|file| {
            format!(
                "{} ({} text, {} bytes)",
                file.title,
                evidence_folder_file_encoding(file),
                file.bytes
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let excerpt = if files.is_empty() {
        "0 text files found.".to_string()
    } else {
        format!(
            "{} text files, {} bytes: {}",
            files.len(),
            total_bytes,
            file_names
        )
    };

    Ok(EvidenceFolderOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::FileRead,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(normalized_folder.clone()),
            evidence_ref: Some(normalized_folder.clone()),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("Evidence folder: {normalized_folder}")),
            excerpt: Some(excerpt),
            warnings: Vec::new(),
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

fn evidence_folder_file_encoding(file: &EvidenceFolderFile) -> &str {
    let encoding = file.encoding.trim();
    if encoding.is_empty() {
        "utf-8"
    } else {
        encoding
    }
}

pub fn run_terminal_read(
    request: TerminalReadRequest,
    client: &impl TerminalReadClient,
) -> Result<TerminalReadOutcome, String> {
    let normalized_command = normalize_terminal_read_command(&request.command)?;
    let started_at = Instant::now();
    let access_request =
        request_capability_access(request.access_mode, CapabilityKind::TerminalRead)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(TerminalReadOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::TerminalRead,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(normalized_command.clone()),
                evidence_ref: Some(normalized_command),
                requested_url: None,
                evidence_url: None,
                title: None,
                excerpt: None,
                warnings: vec![
                    "terminal read requires approval before running the command".to_string()
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let output = match client.run_readonly_command(&normalized_command) {
        Ok(output) => output,
        Err(error) => {
            return Ok(TerminalReadOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::TerminalRead,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(normalized_command.clone()),
                    evidence_ref: Some(normalized_command),
                    requested_url: None,
                    evidence_url: None,
                    title: None,
                    excerpt: None,
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    let command_excerpt = terminal_output_excerpt(&output);
    let status = if output.exit_code == 0 {
        CapabilityInvocationStatus::Succeeded
    } else {
        CapabilityInvocationStatus::Failed
    };
    let warnings = if output.exit_code == 0 {
        Vec::new()
    } else {
        vec![format!(
            "terminal command exited with code {}",
            output.exit_code
        )]
    };

    Ok(TerminalReadOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::TerminalRead,
            status,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(normalized_command.clone()),
            evidence_ref: Some(normalized_command.clone()),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("Terminal read: {normalized_command}")),
            excerpt: non_empty_string(excerpt_text(&command_excerpt)),
            warnings,
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_terminal_write_boundary(
    request: TerminalWriteRequest,
) -> Result<TerminalWriteOutcome, String> {
    let normalized_command = normalize_terminal_write_command(&request.command)?;
    let started_at = Instant::now();
    let access_request =
        request_capability_access(request.access_mode, CapabilityKind::TerminalWrite)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(TerminalWriteOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::TerminalWrite,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(normalized_command.clone()),
                evidence_ref: Some(normalized_command.clone()),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("Terminal write request: {normalized_command}")),
                excerpt: None,
                warnings: vec![
                    "terminal write requires approval before command execution".to_string()
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    Ok(TerminalWriteOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::TerminalWrite,
            status: CapabilityInvocationStatus::Failed,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(normalized_command.clone()),
            evidence_ref: Some(normalized_command.clone()),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("Terminal write blocked: {normalized_command}")),
            excerpt: Some("No command was run.".to_string()),
            warnings: vec![
                "terminal write execution is not enabled in boundary v1; no command was run"
                    .to_string(),
            ],
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_computer_screenshot(
    request: ComputerScreenshotRequest,
    client: &impl ComputerScreenshotClient,
) -> Result<ComputerScreenshotOutcome, String> {
    let started_at = Instant::now();
    let access_request =
        request_capability_access(request.access_mode, CapabilityKind::ComputerScreenshot)?;
    let requested_resource = "visible desktop screenshot".to_string();

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(ComputerScreenshotOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::ComputerScreenshot,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(requested_resource.clone()),
                evidence_ref: Some(requested_resource),
                requested_url: None,
                evidence_url: None,
                title: None,
                excerpt: None,
                warnings: vec![
                    "computer screenshot requires approval before inspecting the screen"
                        .to_string(),
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let screenshot = match client.capture_screenshot() {
        Ok(screenshot) => screenshot,
        Err(error) => {
            return Ok(ComputerScreenshotOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::ComputerScreenshot,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(requested_resource.clone()),
                    evidence_ref: Some(requested_resource),
                    requested_url: None,
                    evidence_url: None,
                    title: None,
                    excerpt: None,
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    let excerpt = format!(
        "{}x{} captured at {}",
        screenshot.width,
        screenshot.height,
        screenshot.captured_at.to_rfc3339()
    );

    Ok(ComputerScreenshotOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::ComputerScreenshot,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(requested_resource),
            evidence_ref: Some(screenshot.evidence_ref.clone()),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("Computer screenshot: {}", screenshot.display_label)),
            excerpt: Some(excerpt),
            warnings: Vec::new(),
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_computer_control_boundary(
    request: ComputerControlRequest,
    client: &impl ComputerControlClient,
) -> Result<ComputerControlOutcome, String> {
    let target = normalize_computer_control_field(&request.target, "computer control target")?;
    let action = normalize_computer_control_field(&request.action, "computer control action")?;
    let structured_action = parse_computer_control_action(&action)?;
    let requested_resource = format!("{target}: {action}");
    let started_at = Instant::now();
    let access_request =
        request_capability_access(request.access_mode, CapabilityKind::ComputerControl)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(ComputerControlOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::ComputerControl,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(requested_resource.clone()),
                evidence_ref: Some(requested_resource),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("Computer control request: {target}")),
                excerpt: Some(structured_action.audit_summary()),
                warnings: vec![
                    "computer control requires explicit approval in this access mode".to_string(),
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let execution = match client.execute_control(&target, &structured_action) {
        Ok(execution) => execution,
        Err(error) => {
            return Ok(ComputerControlOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::ComputerControl,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(requested_resource.clone()),
                    evidence_ref: Some(requested_resource),
                    requested_url: None,
                    evidence_url: None,
                    title: Some(format!("Computer control failed: {target}")),
                    excerpt: Some(structured_action.audit_summary()),
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    Ok(ComputerControlOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::ComputerControl,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(requested_resource.clone()),
            evidence_ref: Some(requested_resource),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("Computer control executed: {target}")),
            excerpt: Some(format!(
                "{}; {}",
                structured_action.audit_summary(),
                execution.summary
            )),
            warnings: Vec::new(),
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_email_send_boundary(request: EmailSendRequest) -> Result<EmailSendOutcome, String> {
    let to = normalize_email_field(&request.to, "email recipient")?;
    let subject = normalize_email_field(&request.subject, "email subject")?;
    let body = normalize_email_field(&request.body, "email body")?;
    let started_at = Instant::now();
    let access_request = request_capability_access(request.access_mode, CapabilityKind::EmailSend)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(EmailSendOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::EmailSend,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(to.clone()),
                evidence_ref: Some(to),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("Email send request: {subject}")),
                excerpt: Some(excerpt_text(&body)),
                warnings: vec!["email send requires explicit approval before sending".to_string()],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    Ok(EmailSendOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::EmailSend,
            status: CapabilityInvocationStatus::Failed,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(to.clone()),
            evidence_ref: Some(to),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("Email send blocked: {subject}")),
            excerpt: Some(excerpt_text(&body)),
            warnings: vec![
                "email send execution is not enabled in boundary v1; no email was sent".to_string(),
            ],
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_email_draft_boundary(request: EmailDraftRequest) -> Result<EmailDraftOutcome, String> {
    let to = normalize_email_field(&request.to, "email recipient")?;
    let subject = normalize_email_field(&request.subject, "email subject")?;
    let body = normalize_email_field(&request.body, "email body")?;
    let started_at = Instant::now();
    let access_request =
        request_capability_access(request.access_mode, CapabilityKind::EmailDraft)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(EmailDraftOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::EmailDraft,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(to.clone()),
                evidence_ref: Some(to),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("Email draft request: {subject}")),
                excerpt: Some(excerpt_text(&body)),
                warnings: vec![
                    "email draft creation requires explicit approval in this access mode"
                        .to_string(),
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    Ok(EmailDraftOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::EmailDraft,
            status: CapabilityInvocationStatus::Failed,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(to.clone()),
            evidence_ref: Some(to),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("Email draft blocked: {subject}")),
            excerpt: Some(excerpt_text(&body)),
            warnings: vec![
                "email draft creation is not enabled in boundary v1; no mailbox draft was created"
                    .to_string(),
            ],
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_email_read_boundary(request: EmailReadRequest) -> Result<EmailReadOutcome, String> {
    let mailbox = normalize_email_field(&request.mailbox, "email mailbox")?;
    let query = normalize_email_field(&request.query, "email query")?;
    let requested_resource = format!("{mailbox}: {query}");
    let started_at = Instant::now();
    let access_request = request_capability_access(request.access_mode, CapabilityKind::EmailRead)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(EmailReadOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::EmailRead,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(requested_resource.clone()),
                evidence_ref: Some(requested_resource),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("Email read request: {mailbox}")),
                excerpt: Some(excerpt_text(&query)),
                warnings: vec![
                    "email read requires explicit approval in this access mode".to_string()
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    Ok(EmailReadOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::EmailRead,
            status: CapabilityInvocationStatus::Failed,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(requested_resource.clone()),
            evidence_ref: Some(requested_resource),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("Email read blocked: {mailbox}")),
            excerpt: Some(excerpt_text(&query)),
            warnings: vec![
                "email read execution is not enabled in boundary v1; no email was read".to_string(),
            ],
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_drive_read_boundary(
    request: DriveReadRequest,
    client: &impl DriveLocalFolderClient,
) -> Result<DriveReadOutcome, String> {
    let location = normalize_drive_field(&request.location, "drive location")?;
    let query = normalize_drive_field(&request.query, "drive query")?;
    let requested_resource = format!("{location}: {query}");
    let started_at = Instant::now();
    let access_request = request_capability_access(request.access_mode, CapabilityKind::DriveRead)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(DriveReadOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::DriveRead,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(requested_resource.clone()),
                evidence_ref: Some(requested_resource),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("Drive read request: {location}")),
                excerpt: Some(excerpt_text(&query)),
                warnings: vec![
                    "drive read requires explicit approval in this access mode".to_string()
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let read_result = match client.read_local_folder(&location, &query) {
        Ok(read_result) => read_result,
        Err(error) => {
            return Ok(DriveReadOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::DriveRead,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(requested_resource.clone()),
                    evidence_ref: Some(location.clone()),
                    requested_url: None,
                    evidence_url: None,
                    title: Some(format!("Drive local folder read failed: {location}")),
                    excerpt: Some(excerpt_text(&query)),
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    Ok(DriveReadOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::DriveRead,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(requested_resource.clone()),
            evidence_ref: Some(read_result.location.clone()),
            requested_url: None,
            evidence_url: None,
            title: Some(format!("Local drive folder: {location}")),
            excerpt: Some(drive_read_excerpt(&read_result)),
            warnings: if read_result.entries.is_empty() {
                vec!["no local drive files matched the query".to_string()]
            } else {
                Vec::new()
            },
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

pub fn run_drive_write_boundary(
    request: DriveWriteRequest,
    client: &impl DriveLocalFolderClient,
) -> Result<DriveWriteOutcome, String> {
    let location = normalize_drive_field(&request.location, "drive location")?;
    let summary = normalize_drive_field(&request.summary, "drive write summary")?;
    let requested_resource = format!("{location}: {summary}");
    let started_at = Instant::now();
    let access_request =
        request_capability_access(request.access_mode, CapabilityKind::DriveWrite)?;

    if access_request.decision == PolicyDecision::Ask && !request.approval_granted {
        return Ok(DriveWriteOutcome {
            invocation: CapabilityInvocation {
                id: Uuid::new_v4(),
                capability: CapabilityKind::DriveWrite,
                status: CapabilityInvocationStatus::PendingApproval,
                policy_decision: access_request.decision,
                approval_request_id: None,
                requested_resource: Some(requested_resource.clone()),
                evidence_ref: Some(requested_resource),
                requested_url: None,
                evidence_url: None,
                title: Some(format!("Drive write request: {location}")),
                excerpt: Some(excerpt_text(&summary)),
                warnings: vec![
                    "drive write requires explicit approval in this access mode".to_string()
                ],
                elapsed_ms: started_at.elapsed().as_millis(),
                created_at: Utc::now(),
            },
            access_request,
        });
    }

    let write_result = match (&request.export_file, request.package_json.as_deref()) {
        (Some(export_file), None) => {
            let content = decode_drive_export_file_content(export_file)?;
            client.write_export_file(&location, &export_file.file_name, &content)
        }
        (None, Some(package_json)) => {
            let package_json = package_json.trim();
            if package_json.is_empty() {
                return Err("drive export package json is required".to_string());
            }
            client.write_export_package(&location, &summary, package_json)
        }
        (Some(_), Some(_)) => {
            return Err(
                "drive write accepts either export_file or package_json, not both".to_string(),
            );
        }
        (None, None) => {
            return Err("drive export package json or export file content is required".to_string());
        }
    };

    let write_result = match write_result {
        Ok(write_result) => write_result,
        Err(error) => {
            return Ok(DriveWriteOutcome {
                invocation: CapabilityInvocation {
                    id: Uuid::new_v4(),
                    capability: CapabilityKind::DriveWrite,
                    status: CapabilityInvocationStatus::Failed,
                    policy_decision: access_request.decision,
                    approval_request_id: None,
                    requested_resource: Some(requested_resource.clone()),
                    evidence_ref: Some(location.clone()),
                    requested_url: None,
                    evidence_url: None,
                    title: Some(format!("Drive export package failed: {location}")),
                    excerpt: Some(excerpt_text(&summary)),
                    warnings: vec![error],
                    elapsed_ms: started_at.elapsed().as_millis(),
                    created_at: Utc::now(),
                },
                access_request,
            });
        }
    };

    Ok(DriveWriteOutcome {
        invocation: CapabilityInvocation {
            id: Uuid::new_v4(),
            capability: CapabilityKind::DriveWrite,
            status: CapabilityInvocationStatus::Succeeded,
            policy_decision: access_request.decision,
            approval_request_id: None,
            requested_resource: Some(requested_resource.clone()),
            evidence_ref: Some(write_result.path.clone()),
            requested_url: None,
            evidence_url: None,
            title: Some("Drive export written".to_string()),
            excerpt: Some(format!(
                "{} -> {} ({} bytes)",
                excerpt_text(&summary),
                local_output_file_name(&write_result.path),
                write_result.bytes
            )),
            warnings: Vec::new(),
            elapsed_ms: started_at.elapsed().as_millis(),
            created_at: Utc::now(),
        },
        access_request,
    })
}

fn local_output_file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .filter(|file_name| !file_name.trim().is_empty())
        .unwrap_or(path)
        .to_string()
}

fn normalize_export_file_name(file_name: &str) -> Result<String, String> {
    let file_name = file_name.trim();
    if file_name.is_empty() {
        return Err("export file name is required".to_string());
    }

    let path = Path::new(file_name);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        || path.file_name().and_then(|value| value.to_str()) != Some(file_name)
    {
        return Err("export file name must not include directories".to_string());
    }

    Ok(file_name.to_string())
}

fn decode_drive_export_file_content(export_file: &DriveWriteExportFile) -> Result<Vec<u8>, String> {
    match export_file.content_base64.as_deref() {
        Some(encoded_content) => {
            if !export_file.content.is_empty() {
                return Err(
                    "drive export file accepts either content or content_base64, not both"
                        .to_string(),
                );
            }
            let encoded_content = encoded_content.trim();
            if encoded_content.is_empty() {
                return Err("drive export file base64 content is required".to_string());
            }
            general_purpose::STANDARD
                .decode(encoded_content)
                .map_err(|error| format!("drive export file base64 content is invalid: {error}"))
        }
        None => {
            if export_file.content.is_empty() {
                return Err("drive export file content is required".to_string());
            }
            Ok(export_file.content.as_bytes().to_vec())
        }
    }
}

fn normalize_network_search_query(query: &str) -> Result<String, String> {
    let normalized = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err("network search query is required".to_string());
    }

    Ok(normalized)
}

fn normalize_browser_submit_summary(summary: &str) -> Result<String, String> {
    let normalized = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err("browser submit summary is required".to_string());
    }

    Ok(normalized)
}

fn normalize_network_search_scope(scope: &str) -> String {
    let normalized = scope.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        "public web".to_string()
    } else {
        normalized
    }
}

fn normalize_computer_control_field(value: &str, label: &str) -> Result<String, String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err(format!("{label} is required"));
    }

    Ok(normalized)
}

pub fn parse_computer_control_action(value: &str) -> Result<ComputerControlAction, String> {
    let trimmed = value.trim();
    let Some((verb, payload)) = trimmed.split_once(':') else {
        return Err(
            "structured computer control action is required: use click:x,y[,button], move:x,y, type:text, press:key, hotkey:key+key, or scroll:delta[,axis]"
                .to_string(),
        );
    };
    let verb = verb.trim().to_lowercase();
    let payload = payload.trim();
    if payload.is_empty() {
        return Err("computer control action payload is required".to_string());
    }

    match verb.as_str() {
        "click" => {
            let parts = split_control_payload(payload);
            if parts.len() != 2 && parts.len() != 3 {
                return Err("click action must be click:x,y[,left|middle|right]".to_string());
            }
            Ok(ComputerControlAction::Click {
                x: parse_control_coordinate(parts[0], "click x")?,
                y: parse_control_coordinate(parts[1], "click y")?,
                button: if parts.len() == 3 {
                    parse_control_mouse_button(parts[2])?
                } else {
                    ComputerControlMouseButton::Left
                },
            })
        }
        "move" => {
            let parts = split_control_payload(payload);
            if parts.len() != 2 {
                return Err("move action must be move:x,y".to_string());
            }
            Ok(ComputerControlAction::Move {
                x: parse_control_coordinate(parts[0], "move x")?,
                y: parse_control_coordinate(parts[1], "move y")?,
            })
        }
        "type" => {
            if payload.contains('\0') {
                return Err("type action text cannot contain null bytes".to_string());
            }
            if payload.chars().count() > 2_000 {
                return Err("type action text exceeds 2000 characters".to_string());
            }
            Ok(ComputerControlAction::TypeText {
                text: payload.to_string(),
            })
        }
        "press" => Ok(ComputerControlAction::PressKey {
            key: normalize_control_key(payload)?,
        }),
        "hotkey" => {
            let keys = payload
                .split('+')
                .map(normalize_control_key)
                .collect::<Result<Vec<_>, _>>()?;
            if keys.len() < 2 || keys.len() > 5 {
                return Err("hotkey action must contain 2 to 5 keys".to_string());
            }
            Ok(ComputerControlAction::Hotkey { keys })
        }
        "scroll" => {
            let parts = split_control_payload(payload);
            if parts.is_empty() || parts.len() > 2 {
                return Err("scroll action must be scroll:delta[,vertical|horizontal]".to_string());
            }
            let delta = parts[0]
                .parse::<i32>()
                .map_err(|_| "scroll delta must be an integer".to_string())?;
            if delta == 0 || !(-120..=120).contains(&delta) {
                return Err("scroll delta must be between -120 and 120 and cannot be 0".to_string());
            }
            Ok(ComputerControlAction::Scroll {
                delta,
                axis: if parts.len() == 2 {
                    parse_control_scroll_axis(parts[1])?
                } else {
                    ComputerControlScrollAxis::Vertical
                },
            })
        }
        _ => Err(format!(
            "unsupported computer control action '{verb}': use click, move, type, press, hotkey, or scroll"
        )),
    }
}

fn computer_control_action_contract_string(action: &ComputerControlAction) -> String {
    match action {
        ComputerControlAction::Click { x, y, button } => {
            format!("click:{x},{y},{}", mouse_button_contract_name(*button))
        }
        ComputerControlAction::Move { x, y } => format!("move:{x},{y}"),
        ComputerControlAction::TypeText { text } => format!("type:{text}"),
        ComputerControlAction::PressKey { key } => format!("press:{key}"),
        ComputerControlAction::Hotkey { keys } => format!("hotkey:{}", keys.join("+")),
        ComputerControlAction::Scroll { delta, axis } => {
            format!("scroll:{delta},{}", scroll_axis_contract_name(*axis))
        }
    }
}

fn mouse_button_contract_name(button: ComputerControlMouseButton) -> &'static str {
    match button {
        ComputerControlMouseButton::Left => "left",
        ComputerControlMouseButton::Middle => "middle",
        ComputerControlMouseButton::Right => "right",
    }
}

fn scroll_axis_contract_name(axis: ComputerControlScrollAxis) -> &'static str {
    match axis {
        ComputerControlScrollAxis::Vertical => "vertical",
        ComputerControlScrollAxis::Horizontal => "horizontal",
    }
}

fn validate_codex_bridge_screenshot_response(
    response: &CodexBridgeScreenshotResponse,
) -> Result<(), String> {
    validate_codex_bridge_contract_version(&response.contract_version)?;
    if response.capability != CodexBridgeCapability::ComputerScreenshot {
        return Err("local bridge service screenshot returned the wrong capability".to_string());
    }
    if response.width == 0 || response.height == 0 {
        return Err("local bridge service screenshot returned empty dimensions".to_string());
    }
    if response.display_label.trim().is_empty() {
        return Err("local bridge service screenshot returned an empty display label".to_string());
    }
    if response.png_base64.trim().is_empty() {
        return Err("local bridge service screenshot returned empty PNG base64".to_string());
    }

    Ok(())
}

fn validate_codex_bridge_control_response(
    response: &CodexBridgeControlResponse,
) -> Result<(), String> {
    validate_codex_bridge_contract_version(&response.contract_version)?;
    if response.capability != CodexBridgeCapability::ComputerControl {
        return Err("local bridge service control returned the wrong capability".to_string());
    }
    if response.summary.trim().is_empty() {
        return Err("local bridge service control returned an empty summary".to_string());
    }

    Ok(())
}

fn validate_codex_bridge_network_search_response(
    response: &CodexBridgeNetworkSearchResponse,
) -> Result<(), String> {
    validate_codex_bridge_contract_version(&response.contract_version)?;
    if response.capability != CodexBridgeCapability::NetworkSearch {
        return Err(
            "local bridge service network search returned the wrong capability".to_string(),
        );
    }
    if response.provider.trim().is_empty() {
        return Err("local bridge service network search returned an empty provider".to_string());
    }
    if response.query.trim().is_empty() {
        return Err("local bridge service network search returned an empty query".to_string());
    }
    if response.scope.trim().is_empty() {
        return Err("local bridge service network search returned an empty scope".to_string());
    }
    if !is_http_url(response.search_url.trim()) {
        return Err(
            "local bridge service network search returned an invalid search URL".to_string(),
        );
    }
    if response.items.is_empty() {
        return Err("local bridge service network search returned no source links".to_string());
    }
    for item in &response.items {
        if item.title.trim().is_empty() {
            return Err(
                "local bridge service network search returned an empty source title".to_string(),
            );
        }
        if !is_http_url(item.url.trim()) {
            return Err(
                "local bridge service network search returned an invalid source URL".to_string(),
            );
        }
    }

    Ok(())
}

fn validate_codex_bridge_contract_version(contract_version: &str) -> Result<(), String> {
    if contract_version != CODEX_BRIDGE_CONTRACT_VERSION {
        return Err(format!(
            "local bridge service version mismatch: expected {CODEX_BRIDGE_CONTRACT_VERSION}, got {contract_version}"
        ));
    }

    Ok(())
}

fn is_http_url(value: &str) -> bool {
    reqwest::Url::parse(value)
        .map(|url| matches!(url.scheme(), "http" | "https"))
        .unwrap_or(false)
}

fn split_control_payload(payload: &str) -> Vec<&str> {
    payload
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn parse_control_coordinate(value: &str, label: &str) -> Result<i32, String> {
    let coordinate = value
        .parse::<i32>()
        .map_err(|_| format!("{label} must be an integer"))?;
    if !(0..=100_000).contains(&coordinate) {
        return Err(format!("{label} must be between 0 and 100000"));
    }

    Ok(coordinate)
}

fn parse_control_mouse_button(value: &str) -> Result<ComputerControlMouseButton, String> {
    match value.trim().to_lowercase().as_str() {
        "left" => Ok(ComputerControlMouseButton::Left),
        "middle" => Ok(ComputerControlMouseButton::Middle),
        "right" => Ok(ComputerControlMouseButton::Right),
        _ => Err("mouse button must be left, middle, or right".to_string()),
    }
}

fn parse_control_scroll_axis(value: &str) -> Result<ComputerControlScrollAxis, String> {
    match value.trim().to_lowercase().as_str() {
        "vertical" | "v" => Ok(ComputerControlScrollAxis::Vertical),
        "horizontal" | "h" => Ok(ComputerControlScrollAxis::Horizontal),
        _ => Err("scroll axis must be vertical or horizontal".to_string()),
    }
}

fn normalize_control_key(value: &str) -> Result<String, String> {
    let key = value.trim().to_lowercase();
    if key.is_empty() {
        return Err("key name is required".to_string());
    }
    let normalized = match key.as_str() {
        "control" => "ctrl".to_string(),
        "cmd" | "command" | "super" | "windows" => "meta".to_string(),
        "return" => "enter".to_string(),
        "esc" => "escape".to_string(),
        "arrowup" => "up".to_string(),
        "arrowdown" => "down".to_string(),
        "arrowleft" => "left".to_string(),
        "arrowright" => "right".to_string(),
        _ => key,
    };

    let is_single_ascii = normalized.len() == 1
        && normalized
            .chars()
            .all(|character| character.is_ascii_alphanumeric());
    let allowed_named = matches!(
        normalized.as_str(),
        "ctrl"
            | "shift"
            | "alt"
            | "meta"
            | "enter"
            | "tab"
            | "escape"
            | "backspace"
            | "delete"
            | "space"
            | "up"
            | "down"
            | "left"
            | "right"
    );
    if is_single_ascii || allowed_named {
        Ok(normalized)
    } else {
        Err(format!("unsupported key name '{normalized}'"))
    }
}

fn normalize_drive_field(value: &str, label: &str) -> Result<String, String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err(format!("{label} is required"));
    }

    Ok(normalized)
}

fn normalize_file_write_field(value: &str, label: &str) -> Result<String, String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err(format!("{label} is required"));
    }

    Ok(normalized)
}

fn validate_file_write_content(content: &str) -> Result<String, String> {
    if content.trim().is_empty() {
        return Err("file write content is required".to_string());
    }

    Ok(content.to_string())
}

fn normalize_filesystem_path_field(value: &str, label: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} is required"));
    }
    if !Path::new(trimmed).is_absolute() {
        return Err(format!("{label} must be an absolute local path"));
    }

    Ok(trimmed.to_string())
}

fn validate_filesystem_mutation_content(content: Option<&str>) -> Result<String, String> {
    content
        .map(ToString::to_string)
        .ok_or_else(|| "filesystem file content is required".to_string())
}

fn validate_filesystem_mutation_request(
    operation: FileSystemMutationOperation,
    path: &str,
    destination: Option<&str>,
) -> Result<(), String> {
    reject_root_mutation_path(Path::new(path))?;
    match operation {
        FileSystemMutationOperation::RenameFile | FileSystemMutationOperation::RenameDirectory => {
            let destination = destination.ok_or_else(|| {
                "filesystem rename destination is required before dispatch".to_string()
            })?;
            reject_root_mutation_path(Path::new(destination))?;
        }
        _ => {}
    }

    Ok(())
}

fn reject_root_mutation_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.file_name().is_none() {
        return Err("filesystem mutation refuses to target a filesystem root".to_string());
    }

    Ok(())
}

fn filesystem_mutation_operation_label(operation: FileSystemMutationOperation) -> &'static str {
    match operation {
        FileSystemMutationOperation::CreateFile => "create_file",
        FileSystemMutationOperation::UpdateFile => "update_file",
        FileSystemMutationOperation::DeleteFile => "delete_file",
        FileSystemMutationOperation::RenameFile => "rename_file",
        FileSystemMutationOperation::CreateDirectory => "create_directory",
        FileSystemMutationOperation::RenameDirectory => "rename_directory",
        FileSystemMutationOperation::DeleteDirectory => "delete_directory",
    }
}

fn filesystem_mutation_target_summary(path: &str, destination: Option<&str>) -> String {
    match destination {
        Some(destination) => format!("{path} -> {destination}"),
        None => path.to_string(),
    }
}

fn normalize_browser_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("browser URL is required".to_string());
    }

    let lower = trimmed.to_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err("browser URL must start with http:// or https://".to_string());
    }

    Ok(trimmed.to_string())
}

fn normalize_file_path(path: &str) -> Result<String, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("file path is required".to_string());
    }

    Ok(trimmed.to_string())
}

fn normalize_terminal_read_command(command: &str) -> Result<String, String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err("terminal command is required".to_string());
    }

    if let Some(path) = normalize_terminal_directory_listing_target(trimmed) {
        return Ok(format!("{TERMINAL_READ_DIRECTORY_LIST_PREFIX}{path}"));
    }

    let normalized = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err("terminal command is required".to_string());
    }

    if terminal_read_allowed_commands().contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(format!(
            "terminal command is not in the TerminalRead allowlist: {normalized}"
        ))
    }
}

fn normalize_terminal_directory_listing_target(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let explicit = trimmed
        .strip_prefix("list_directory:")
        .or_else(|| trimmed.strip_prefix("list-directory:"))
        .or_else(|| trimmed.strip_prefix(TERMINAL_READ_DIRECTORY_LIST_PREFIX));
    if let Some(path) = explicit.and_then(clean_terminal_directory_path) {
        return Some(path);
    }

    if let Some(path) =
        clean_terminal_directory_path(trimmed).filter(|path| looks_like_local_directory_path(path))
    {
        return Some(path);
    }

    let tokens = split_terminal_command_tokens(trimmed)?;
    let command_name = tokens.first()?.to_ascii_lowercase();
    match command_name.as_str() {
        "dir" | "ls" if tokens.len() == 2 => clean_terminal_directory_path(&tokens[1]),
        "get-childitem" | "gci" => terminal_get_child_item_path(&tokens),
        _ => None,
    }
}

fn terminal_get_child_item_path(tokens: &[String]) -> Option<String> {
    match tokens {
        [_, path] => clean_terminal_directory_path(path),
        [_, option, path]
            if matches!(
                option.to_ascii_lowercase().as_str(),
                "-literalpath" | "-path"
            ) =>
        {
            clean_terminal_directory_path(path)
        }
        _ => None,
    }
}

fn clean_terminal_directory_path(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }

    let unquoted = if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    let unquoted = unquoted.trim();
    if unquoted.is_empty() {
        None
    } else {
        Some(unquoted.to_string())
    }
}

fn looks_like_local_directory_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with("\\\\")
        || path.starts_with('/')
        || (bytes.len() >= 3
            && bytes[1] == b':'
            && bytes[0].is_ascii_alphabetic()
            && matches!(bytes[2], b'\\' | b'/'))
}

fn split_terminal_command_tokens(command: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for character in command.chars() {
        match (quote, character) {
            (Some(active_quote), value) if value == active_quote => {
                quote = None;
            }
            (Some(_), value) => current.push(value),
            (None, '\'' | '"') => {
                quote = Some(character);
            }
            (None, value) if value.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            (None, value) => current.push(value),
        }
    }

    if quote.is_some() {
        return None;
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    if tokens.is_empty() {
        None
    } else {
        Some(tokens)
    }
}

fn terminal_read_allowed_commands() -> &'static [&'static str] {
    &[
        "pwd",
        "git status --short",
        "git diff --stat",
        "git branch --show-current",
    ]
}

fn normalize_terminal_write_command(command: &str) -> Result<String, String> {
    let normalized = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err("terminal write command is required".to_string());
    }

    Ok(normalized)
}

fn normalize_email_field(value: &str, label: &str) -> Result<String, String> {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return Err(format!("{label} is required"));
    }

    Ok(normalized)
}

fn terminal_output_excerpt(output: &TerminalCommandOutput) -> String {
    let mut parts = Vec::new();
    if !output.stdout.trim().is_empty() {
        parts.push(format!("stdout: {}", output.stdout.trim()));
    }
    if !output.stderr.trim().is_empty() {
        parts.push(format!("stderr: {}", output.stderr.trim()));
    }
    if parts.is_empty() {
        parts.push(format!(
            "command exited with code {} and no output",
            output.exit_code
        ));
    }
    parts.join(" ")
}

fn drive_read_excerpt(result: &DriveReadResult) -> String {
    if result.entries.is_empty() {
        return format!("0 matching files for query '{}'", result.query);
    }

    let file_list = result
        .entries
        .iter()
        .map(|entry| {
            format!(
                "{} ({} text, {} bytes)",
                entry.title,
                drive_folder_entry_encoding(entry),
                entry.bytes
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "{} matching files, {} bytes: {}",
        result.entries.len(),
        result.total_bytes,
        file_list
    )
}

fn drive_folder_entry_encoding(entry: &DriveFolderEntry) -> &str {
    let encoding = entry.encoding.trim();
    if encoding.is_empty() {
        "utf-8"
    } else {
        encoding
    }
}

fn network_search_excerpt(result: &NetworkSearchResult) -> String {
    result
        .items
        .iter()
        .take(3)
        .map(|item| {
            format!(
                "{}: {} ({})",
                item.title,
                excerpt_text(&item.snippet),
                item.url
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_supported_text_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_lowercase()),
        Some(extension)
            if matches!(
                extension.as_str(),
                "txt" | "md" | "csv" | "json" | "log" | "yaml" | "yml"
            )
    )
}

fn extract_search_result_items(html: &str, max_results: usize) -> Vec<NetworkSearchResultItem> {
    let mut items: Vec<NetworkSearchResultItem> = Vec::new();
    let mut cursor = 0_usize;
    let lower = html.to_lowercase();

    while items.len() < max_results {
        let Some(anchor_offset) = lower[cursor..].find("<a ") else {
            break;
        };
        let anchor_start = cursor + anchor_offset;
        let Some(tag_end_offset) = lower[anchor_start..].find('>') else {
            break;
        };
        let tag_end = anchor_start + tag_end_offset;
        let tag = &html[anchor_start..=tag_end];
        let Some(raw_href) = extract_html_attribute(tag, "href") else {
            cursor = tag_end + 1;
            continue;
        };
        let Some(url) = normalize_search_result_url(&raw_href) else {
            cursor = tag_end + 1;
            continue;
        };
        let Some(anchor_close_offset) = lower[tag_end + 1..].find("</a>") else {
            cursor = tag_end + 1;
            continue;
        };
        let anchor_close = tag_end + 1 + anchor_close_offset;
        let title = html_to_text(&html[tag_end + 1..anchor_close]);
        if title.is_empty() || items.iter().any(|item| item.url == url) {
            cursor = anchor_close + 4;
            continue;
        }

        let snippet_window_end = lower[anchor_close..]
            .find("<a ")
            .map(|next_anchor| anchor_close + next_anchor)
            .unwrap_or_else(|| (anchor_close + 700).min(html.len()));
        let snippet = html_to_text(&html[anchor_close..snippet_window_end]);
        items.push(NetworkSearchResultItem {
            title,
            url,
            snippet: if snippet.is_empty() {
                "source link returned by NetworkSearch".to_string()
            } else {
                excerpt_text(&snippet)
            },
        });
        cursor = anchor_close + 4;
    }

    items
}

fn extract_html_attribute(tag: &str, attribute: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let needle = format!("{attribute}=");
    let start = lower.find(&needle)? + needle.len();
    let quote = tag[start..].chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value_start = start + quote.len_utf8();
    let value_end = tag[value_start..].find(quote)? + value_start;
    Some(decode_basic_entities(&tag[value_start..value_end]))
}

fn normalize_search_result_url(raw_href: &str) -> Option<String> {
    let href = raw_href.trim();
    if href.is_empty() || href.starts_with('#') {
        return None;
    }

    let url = if href.starts_with("/l/") {
        reqwest::Url::parse(&format!("https://duckduckgo.com{href}")).ok()?
    } else if href.starts_with("http://") || href.starts_with("https://") {
        reqwest::Url::parse(href).ok()?
    } else {
        return None;
    };

    if url
        .domain()
        .is_some_and(|domain| domain.ends_with("duckduckgo.com"))
    {
        if let Some((_, target)) = url.query_pairs().find(|(key, _)| key == "uddg") {
            return reqwest::Url::parse(&target).ok().map(|url| url.to_string());
        }
        return None;
    }

    Some(url.to_string())
}

fn excerpt_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(280)
        .collect()
}

fn file_read_metadata_warning(file: &FileContent) -> String {
    let bytes = if file.bytes == 0 {
        file.text.as_bytes().len() as u64
    } else {
        file.bytes
    };
    let encoding = file.encoding.trim();
    let encoding = if encoding.is_empty() {
        "utf-8"
    } else {
        encoding
    };

    format!("file metadata: {encoding} text, {bytes} bytes")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn resolve_workspace_file_write_path(workspace_dir: &Path, path: &str) -> Result<PathBuf, String> {
    let workspace = canonical_workspace_dir(workspace_dir)?;
    let requested = PathBuf::from(path.trim());
    if requested.as_os_str().is_empty() {
        return Err("file write path is required".to_string());
    }
    if requested
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("file write path cannot contain parent-directory traversal".to_string());
    }

    let candidate = if requested.is_absolute() {
        requested
    } else {
        workspace.join(requested)
    };
    if !candidate.starts_with(&workspace) {
        return Err("file write target must stay inside the configured workspace".to_string());
    }
    if candidate.file_name().is_none() {
        return Err("file write target must include a file name".to_string());
    }

    Ok(candidate)
}

fn canonical_workspace_dir(workspace_dir: &Path) -> Result<PathBuf, String> {
    let workspace = workspace_dir
        .canonicalize()
        .map_err(|error| format!("workspace directory could not be resolved: {error}"))?;
    let metadata = std::fs::metadata(&workspace)
        .map_err(|error| format!("workspace directory metadata could not be read: {error}"))?;
    if !metadata.is_dir() {
        return Err("configured workspace path is not a directory".to_string());
    }

    Ok(workspace)
}

fn write_computer_screenshot_evidence(
    evidence_base_dir: &Path,
    display_label: &str,
    png_bytes: &[u8],
) -> Result<String, String> {
    let file_name = format!(
        "{}-{}.png",
        Uuid::new_v4(),
        evidence_file_slug(display_label)
    );
    let evidence_dir = evidence_base_dir.join("computer-screenshots");
    std::fs::create_dir_all(&evidence_dir).map_err(|error| {
        format!("computer screenshot evidence folder could not be created: {error}")
    })?;
    let evidence_path = evidence_dir.join(&file_name);
    std::fs::write(&evidence_path, png_bytes).map_err(|error| {
        format!("computer screenshot evidence file could not be written: {error}")
    })?;

    Ok(format!("computer-screenshots/{file_name}"))
}

fn evidence_file_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }

    if slug.is_empty() {
        "display".to_string()
    } else {
        slug
    }
}

fn extract_html_title(html: &str) -> String {
    let lower = html.to_lowercase();
    let Some(start_index) = lower.find("<title") else {
        return String::new();
    };
    let Some(start_close_offset) = lower[start_index..].find('>') else {
        return String::new();
    };
    let title_start = start_index + start_close_offset + 1;
    let Some(end_offset) = lower[title_start..].find("</title>") else {
        return String::new();
    };

    decode_basic_entities(&html[title_start..title_start + end_offset])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn html_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_entity = false;
    let mut entity = String::new();

    for character in html.chars() {
        match character {
            '<' => {
                in_tag = true;
                if !text.ends_with(' ') {
                    text.push(' ');
                }
            }
            '>' => {
                in_tag = false;
            }
            '&' if !in_tag => {
                in_entity = true;
                entity.clear();
            }
            ';' if in_entity => {
                in_entity = false;
                text.push_str(&decode_basic_entity(&entity));
            }
            _ if in_tag => {}
            _ if in_entity => {
                if entity.len() < 12 {
                    entity.push(character);
                }
            }
            _ => text.push(character),
        }
    }

    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_basic_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn decode_basic_entity(entity: &str) -> String {
    match entity {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "#39" => "'".to_string(),
        "nbsp" => " ".to_string(),
        _ => format!("&{entity};"),
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalTerminalReadClient, TERMINAL_READ_DIRECTORY_LIST_PREFIX};

    use std::cell::{Cell, RefCell};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use base64::{engine::general_purpose, Engine as _};
    use chrono::{TimeZone, Utc};

    use crate::kernel::capability::{
        parse_computer_control_action, run_browser_browse, run_browser_submit_boundary,
        run_computer_control_boundary, run_computer_screenshot, run_drive_read_boundary,
        run_drive_write_boundary, run_email_draft_boundary, run_email_read_boundary,
        run_email_send_boundary, run_evidence_folder_ingest, run_file_read,
        run_file_write_boundary, run_filesystem_mutation_boundary, run_network_search_boundary,
        run_terminal_read, run_terminal_write_boundary, BrowserBrowseRequest, BrowserPage,
        BrowserPageClient, BrowserSubmitRequest, CapabilityInvocation, CapabilityInvocationStatus,
        CapturedScreenshotImage, CodexBridgeComputerControlClient,
        CodexBridgeComputerScreenshotClient, CodexBridgeNetworkSearchClient, ComputerControlAction,
        ComputerControlClient, ComputerControlExecution, ComputerControlMouseButton,
        ComputerControlRequest, ComputerScreenshot, ComputerScreenshotClient,
        ComputerScreenshotRequest, DriveFolderEntry, DriveReadRequest, DriveWriteExportFile,
        DriveWriteRequest, EmailDraftRequest, EmailReadRequest, EmailSendRequest,
        EvidenceFolderClient, EvidenceFolderFile, EvidenceFolderRequest, FileContent,
        FileContentClient, FileReadRequest, FileSystemMutationOperation, FileSystemMutationRequest,
        FileWriteRequest, FileWriteResult, HttpBrowserPageClient, LocalComputerControlClient,
        LocalComputerControlInputBackend, LocalComputerScreenshotClient, LocalDriveFolderClient,
        LocalFileSystemMutationClient, LocalScreenshotCaptureBackend,
        LocalWorkspaceFileWriteClient, NetworkSearchClient, NetworkSearchRequest,
        NetworkSearchResult, NetworkSearchResultItem, TerminalCommandOutput, TerminalReadClient,
        TerminalReadRequest, TerminalWriteRequest,
    };
    use crate::kernel::codex_bridge_contract::{
        CodexBridgeCapability, CodexBridgeControlResponse, CodexBridgeNetworkSearchItem,
        CodexBridgeNetworkSearchResponse, CodexBridgeScreenshotResponse,
        CODEX_BRIDGE_CONTRACT_VERSION,
    };
    use crate::kernel::models::{AccessMode, LargeModelProvider};
    use crate::kernel::policy::{CapabilityKind, PolicyDecision};

    struct FakeBrowserPageClient {
        calls: Cell<u32>,
    }

    struct FakeFileContentClient {
        calls: Cell<u32>,
    }

    struct FakeEvidenceFolderClient {
        calls: Cell<u32>,
    }

    struct FakeTerminalReadClient {
        calls: Cell<u32>,
        exit_code: i32,
    }

    struct FakeNetworkSearchClient {
        calls: Cell<u32>,
        failing: bool,
        provider: String,
    }

    struct FakeComputerScreenshotClient {
        calls: Cell<u32>,
        failing: bool,
    }

    struct FakeComputerControlClient {
        calls: Cell<u32>,
        failing: bool,
        last_action: RefCell<Option<ComputerControlAction>>,
    }

    #[derive(Default)]
    struct FakeLocalControlInputBackend {
        operations: Vec<String>,
    }

    struct FakeLocalScreenshotBackend {
        capture: Result<CapturedScreenshotImage, String>,
    }

    struct RecordedHttpRequest {
        raw: String,
    }

    #[test]
    fn evidence_folder_file_legacy_json_defaults_utf8_encoding() {
        let file = serde_json::from_str::<EvidenceFolderFile>(
            r#"{"path":"fixtures/evidence/revenue.md","title":"revenue.md","text":"Revenue","bytes":7}"#,
        )
        .expect("legacy evidence file json is readable");

        assert_eq!(file.encoding, "utf-8");
    }

    #[test]
    fn drive_folder_entry_legacy_json_defaults_utf8_encoding() {
        let entry = serde_json::from_str::<DriveFolderEntry>(
            r#"{"path":"fixtures/evidence/budget.md","title":"budget.md","bytes":6,"excerpt":"Budget"}"#,
        )
        .expect("legacy drive folder entry json is readable");

        assert_eq!(entry.encoding, "utf-8");
    }

    #[test]
    fn file_write_result_legacy_json_defaults_utf8_encoding() {
        let result =
            serde_json::from_str::<FileWriteResult>(r#"{"path":"docs/brief.md","bytes":10}"#)
                .expect("legacy file write result json is readable");

        assert_eq!(result.encoding, "utf-8");
    }

    impl FakeBrowserPageClient {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
            }
        }
    }

    impl FakeFileContentClient {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
            }
        }
    }

    impl FakeEvidenceFolderClient {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
            }
        }
    }

    impl FakeTerminalReadClient {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
                exit_code: 0,
            }
        }

        fn with_exit_code(exit_code: i32) -> Self {
            Self {
                calls: Cell::new(0),
                exit_code,
            }
        }
    }

    impl FakeNetworkSearchClient {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
                failing: false,
                provider: "fake source search".to_string(),
            }
        }

        fn failing() -> Self {
            Self {
                calls: Cell::new(0),
                failing: true,
                provider: "fake source search".to_string(),
            }
        }

        fn blank_provider() -> Self {
            Self {
                calls: Cell::new(0),
                failing: false,
                provider: "   ".to_string(),
            }
        }
    }

    impl FakeComputerScreenshotClient {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
                failing: false,
            }
        }

        fn failing() -> Self {
            Self {
                calls: Cell::new(0),
                failing: true,
            }
        }
    }

    impl FakeComputerControlClient {
        fn new() -> Self {
            Self {
                calls: Cell::new(0),
                failing: false,
                last_action: RefCell::new(None),
            }
        }

        fn failing() -> Self {
            Self {
                calls: Cell::new(0),
                failing: true,
                last_action: RefCell::new(None),
            }
        }
    }

    impl FakeLocalScreenshotBackend {
        fn with_png(display_label: &str, width: u32, height: u32, png_bytes: Vec<u8>) -> Self {
            Self {
                capture: Ok(CapturedScreenshotImage {
                    display_label: display_label.to_string(),
                    width,
                    height,
                    png_bytes,
                }),
            }
        }
    }

    fn serve_one_json_response(response_body: String) -> (String, JoinHandle<RecordedHttpRequest>) {
        let listener = TcpListener::bind("127.0.1.0:0").expect("bind fake bridge server");
        let endpoint = format!("http://{}", listener.local_addr().expect("local addr"));
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept fake bridge request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("set read timeout");
            let raw = read_one_http_request(&mut stream);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write fake bridge response");
            RecordedHttpRequest { raw }
        });

        (endpoint, handle)
    }

    fn read_one_http_request(stream: &mut std::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let bytes_read = stream.read(&mut buffer).expect("read request chunk");
            if bytes_read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..bytes_read]);
            if http_request_complete(&request) {
                break;
            }
        }

        String::from_utf8(request).expect("request is utf8")
    }

    fn http_request_complete(request: &[u8]) -> bool {
        let Some(headers_end) = find_header_end(request) else {
            return false;
        };
        let headers = String::from_utf8_lossy(&request[..headers_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);

        request.len() >= headers_end + 4 + content_length
    }

    fn find_header_end(request: &[u8]) -> Option<usize> {
        request.windows(4).position(|window| window == b"\r\n\r\n")
    }

    fn http_request_body(raw: &str) -> serde_json::Value {
        let body = raw.split("\r\n\r\n").nth(1).expect("request body exists");
        serde_json::from_str(body).expect("request body is json")
    }

    impl BrowserPageClient for FakeBrowserPageClient {
        fn fetch_page(&self, url: &str) -> Result<BrowserPage, String> {
            self.calls.set(self.calls.get() + 1);
            Ok(BrowserPage {
                final_url: url.to_string(),
                title: "Weekly Operations Brief".to_string(),
                text: "Revenue improved, but guest complaints increased in the west wing."
                    .to_string(),
            })
        }
    }

    impl FileContentClient for FakeFileContentClient {
        fn read_file(&self, path: &str) -> Result<FileContent, String> {
            self.calls.set(self.calls.get() + 1);
            Ok(FileContent {
                path: path.to_string(),
                title: "ops-evidence.md".to_string(),
                text: "Occupancy rose by 4 points. Maintenance tickets remain above target."
                    .to_string(),
                bytes: 68,
                encoding: "utf-8".to_string(),
            })
        }
    }

    impl EvidenceFolderClient for FakeEvidenceFolderClient {
        fn read_text_files(&self, folder_path: &str) -> Result<Vec<EvidenceFolderFile>, String> {
            self.calls.set(self.calls.get() + 1);
            Ok(vec![
                EvidenceFolderFile {
                    path: format!("{folder_path}\\revenue.md"),
                    title: "revenue.md".to_string(),
                    text: "Revenue improved by 6 percent.".to_string(),
                    bytes: 30,
                    encoding: "utf-8".to_string(),
                },
                EvidenceFolderFile {
                    path: format!("{folder_path}\\complaints.txt"),
                    title: "complaints.txt".to_string(),
                    text: "Guest complaints increased in the west wing.".to_string(),
                    bytes: 44,
                    encoding: "utf-8".to_string(),
                },
            ])
        }
    }

    impl TerminalReadClient for FakeTerminalReadClient {
        fn run_readonly_command(&self, command: &str) -> Result<TerminalCommandOutput, String> {
            self.calls.set(self.calls.get() + 1);
            Ok(TerminalCommandOutput {
                command: command.to_string(),
                stdout: if self.exit_code == 0 {
                    " M README.md\n?? docs/superpowers/plans/example.md".to_string()
                } else {
                    String::new()
                },
                stderr: if self.exit_code == 0 {
                    String::new()
                } else {
                    "fatal: not a git repository".to_string()
                },
                exit_code: self.exit_code,
            })
        }
    }

    impl NetworkSearchClient for FakeNetworkSearchClient {
        fn search(&self, query: &str, scope: &str) -> Result<NetworkSearchResult, String> {
            self.calls.set(self.calls.get() + 1);
            if self.failing {
                return Err("source-backed search provider failed".to_string());
            }

            Ok(NetworkSearchResult {
                provider: self.provider.clone(),
                query: query.to_string(),
                scope: scope.to_string(),
                search_url: "https://search.example/?q=DeepSeek+Agent+OS".to_string(),
                items: vec![NetworkSearchResultItem {
                    title: "DeepSeek Agent OS project notes".to_string(),
                    url: "https://example.com/deepseek-agent-os".to_string(),
                    snippet: "A source-backed result with a durable URL.".to_string(),
                }],
            })
        }
    }

    impl ComputerScreenshotClient for FakeComputerScreenshotClient {
        fn capture_screenshot(&self) -> Result<ComputerScreenshot, String> {
            self.calls.set(self.calls.get() + 1);
            if self.failing {
                return Err("local screen inspection route unavailable".to_string());
            }

            Ok(ComputerScreenshot {
                display_label: "Primary display".to_string(),
                evidence_ref: "computer-screenshots/primary-display.png".to_string(),
                width: 1920,
                height: 1080,
                captured_at: chrono::Utc::now(),
            })
        }
    }

    impl ComputerControlClient for FakeComputerControlClient {
        fn execute_control(
            &self,
            _target: &str,
            action: &ComputerControlAction,
        ) -> Result<ComputerControlExecution, String> {
            self.calls.set(self.calls.get() + 1);
            self.last_action.replace(Some(action.clone()));
            if self.failing {
                return Err("local mouse and keyboard control route unavailable".to_string());
            }

            Ok(ComputerControlExecution {
                summary: "executed fake desktop input".to_string(),
            })
        }
    }

    impl LocalComputerControlInputBackend for FakeLocalControlInputBackend {
        fn move_mouse_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
            self.operations.push(format!("move:{x},{y}"));
            Ok(())
        }

        fn click_mouse(&mut self, button: ComputerControlMouseButton) -> Result<(), String> {
            self.operations.push(format!("click:{button:?}"));
            Ok(())
        }

        fn type_text(&mut self, text: &str) -> Result<(), String> {
            self.operations.push(format!("type:{text}"));
            Ok(())
        }

        fn key_down(&mut self, key: &str) -> Result<(), String> {
            self.operations.push(format!("down:{key}"));
            Ok(())
        }

        fn key_up(&mut self, key: &str) -> Result<(), String> {
            self.operations.push(format!("up:{key}"));
            Ok(())
        }

        fn key_click(&mut self, key: &str) -> Result<(), String> {
            self.operations.push(format!("key:{key}"));
            Ok(())
        }

        fn scroll(
            &mut self,
            delta: i32,
            axis: super::ComputerControlScrollAxis,
        ) -> Result<(), String> {
            self.operations.push(format!("scroll:{axis:?}:{delta}"));
            Ok(())
        }
    }

    impl LocalScreenshotCaptureBackend for FakeLocalScreenshotBackend {
        fn capture_primary_display(&self) -> Result<CapturedScreenshotImage, String> {
            self.capture.clone()
        }
    }

    #[test]
    fn browser_browse_returns_structured_tool_result() {
        let client = FakeBrowserPageClient::new();
        let outcome = run_browser_browse(
            BrowserBrowseRequest {
                access_mode: AccessMode::LimitedAuto,
                url: "https://example.com/ops-brief".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("browser browse succeeds");

        assert_eq!(
            outcome.access_request.capability,
            CapabilityKind::BrowserBrowse
        );
        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::BrowserBrowse);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Weekly Operations Brief")
        );
        assert_eq!(
            outcome.invocation.excerpt.as_deref(),
            Some("Revenue improved, but guest complaints increased in the west wing.")
        );
        assert_eq!(
            outcome.invocation.evidence_url.as_deref(),
            Some("https://example.com/ops-brief")
        );
        assert_eq!(client.calls.get(), 1);
    }

    #[test]
    fn browser_browse_waits_for_approval_when_policy_asks() {
        let client = FakeBrowserPageClient::new();
        let outcome = run_browser_browse(
            BrowserBrowseRequest {
                access_mode: AccessMode::AskEveryStep,
                url: "https://example.com/ops-brief".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("browser browse returns pending result");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(outcome.invocation.title, None);
        assert_eq!(
            outcome.invocation.evidence_url.as_deref(),
            Some("https://example.com/ops-brief")
        );
        assert_eq!(client.calls.get(), 0);
    }

    #[test]
    fn browser_browse_uses_existing_approval_when_policy_asks() {
        let client = FakeBrowserPageClient::new();
        let outcome = run_browser_browse(
            BrowserBrowseRequest {
                access_mode: AccessMode::AskEveryStep,
                url: "https://example.com/ops-brief".to_string(),
                approval_granted: true,
            },
            &client,
        )
        .expect("approved browser browse succeeds");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Weekly Operations Brief")
        );
        assert_eq!(client.calls.get(), 1);
    }

    #[test]
    fn capability_invocation_defaults_legacy_approval_request_id() {
        let invocation_json = serde_json::json!({
            "id": uuid::Uuid::new_v4(),
            "capability": "browser_browse",
            "status": "succeeded",
            "policy_decision": "allow",
            "requested_resource": "https://example.com",
            "evidence_ref": "https://example.com/final",
            "requested_url": "https://example.com",
            "evidence_url": "https://example.com/final",
            "title": "Example",
            "excerpt": "Example evidence text",
            "warnings": [],
            "elapsed_ms": 12,
            "created_at": chrono::Utc::now()
        });

        let invocation = serde_json::from_value::<CapabilityInvocation>(invocation_json)
            .expect("legacy invocation parses");

        assert_eq!(invocation.approval_request_id, None);
    }

    #[test]
    fn capability_invocation_serializes_approval_request_id() {
        let approval_request_id = uuid::Uuid::new_v4();
        let invocation = CapabilityInvocation {
            id: uuid::Uuid::new_v4(),
            capability: CapabilityKind::EmailSend,
            status: CapabilityInvocationStatus::Failed,
            policy_decision: PolicyDecision::Ask,
            approval_request_id: Some(approval_request_id),
            requested_resource: Some("ops@example.com".to_string()),
            evidence_ref: Some("ops@example.com".to_string()),
            requested_url: None,
            evidence_url: None,
            title: Some("Email send blocked: Weekly brief".to_string()),
            excerpt: Some("Approved email send attempt.".to_string()),
            warnings: vec!["email send execution is not enabled".to_string()],
            elapsed_ms: 1,
            created_at: chrono::Utc::now(),
        };

        let value = serde_json::to_value(&invocation).expect("invocation serializes");

        assert_eq!(
            value["approval_request_id"],
            approval_request_id.to_string()
        );
    }

    #[test]
    fn browser_submit_boundary_waits_for_approval_when_policy_asks() {
        let outcome = run_browser_submit_boundary(BrowserSubmitRequest {
            access_mode: AccessMode::AskOnRisk,
            url: "https://example.com/contact".to_string(),
            summary: "Submit the contact form.".to_string(),
            approval_granted: false,
        })
        .expect("browser submit boundary returns pending result");

        assert_eq!(
            outcome.access_request.capability,
            CapabilityKind::BrowserSubmit
        );
        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_url.as_deref(),
            Some("https://example.com/contact")
        );
    }

    #[test]
    fn browser_submit_boundary_blocks_submit_after_policy_allows() {
        let outcome = run_browser_submit_boundary(BrowserSubmitRequest {
            access_mode: AccessMode::FullAccess,
            url: "https://example.com/contact".to_string(),
            summary: "Submit the contact form.".to_string(),
            approval_granted: false,
        })
        .expect("browser submit boundary records blocked submission");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::BrowserSubmit);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Browser submit blocked: https://example.com/contact")
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("not enabled")));
    }

    #[test]
    fn browser_submit_boundary_rejects_missing_fields() {
        let error = run_browser_submit_boundary(BrowserSubmitRequest {
            access_mode: AccessMode::AskOnRisk,
            url: " ".to_string(),
            summary: " ".to_string(),
            approval_granted: false,
        })
        .expect_err("blank browser submit should fail validation");

        assert!(error.contains("browser URL is required"));
    }

    #[test]
    fn file_read_returns_structured_tool_result() {
        let client = FakeFileContentClient::new();
        let outcome = run_file_read(
            FileReadRequest {
                access_mode: AccessMode::AskOnRisk,
                path: "fixtures/evidence/ops-evidence.md".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("file read succeeds");

        assert_eq!(outcome.access_request.capability, CapabilityKind::FileRead);
        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::FileRead);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(outcome.invocation.title.as_deref(), Some("ops-evidence.md"));
        assert_eq!(
            outcome.invocation.excerpt.as_deref(),
            Some("Occupancy rose by 4 points. Maintenance tickets remain above target.")
        );
        assert_eq!(
            outcome.invocation.evidence_ref.as_deref(),
            Some("fixtures/evidence/ops-evidence.md")
        );
        assert_eq!(
            outcome.invocation.warnings,
            vec!["file metadata: utf-8 text, 68 bytes".to_string()]
        );
        assert_eq!(client.calls.get(), 1);
    }

    #[test]
    fn file_read_waits_for_approval_when_policy_asks() {
        let client = FakeFileContentClient::new();
        let outcome = run_file_read(
            FileReadRequest {
                access_mode: AccessMode::AskEveryStep,
                path: "fixtures/evidence/ops-evidence.md".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("file read returns pending result");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(outcome.invocation.title, None);
        assert_eq!(
            outcome.invocation.evidence_ref.as_deref(),
            Some("fixtures/evidence/ops-evidence.md")
        );
        assert_eq!(client.calls.get(), 0);
    }

    #[test]
    fn evidence_folder_ingest_returns_manifest_tool_result() {
        let client = FakeEvidenceFolderClient::new();
        let outcome = run_evidence_folder_ingest(
            EvidenceFolderRequest {
                access_mode: AccessMode::AskOnRisk,
                folder_path: "fixtures/evidence".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("folder ingest succeeds");

        assert_eq!(outcome.access_request.capability, CapabilityKind::FileRead);
        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::FileRead);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Evidence folder: fixtures/evidence")
        );
        assert_eq!(
            outcome.invocation.evidence_ref.as_deref(),
            Some("fixtures/evidence")
        );
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .expect("excerpt exists")
            .contains("2 text files"));
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .expect("excerpt exists")
            .contains("revenue.md"));
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .expect("excerpt exists")
            .contains("utf-8 text"));
        assert_eq!(client.calls.get(), 1);
    }

    #[test]
    fn evidence_folder_ingest_waits_for_approval_when_policy_asks() {
        let client = FakeEvidenceFolderClient::new();
        let outcome = run_evidence_folder_ingest(
            EvidenceFolderRequest {
                access_mode: AccessMode::AskEveryStep,
                folder_path: "fixtures/evidence".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("folder ingest returns pending result");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.evidence_ref.as_deref(),
            Some("fixtures/evidence")
        );
        assert_eq!(client.calls.get(), 0);
    }

    #[test]
    fn terminal_read_returns_structured_tool_result() {
        let client = FakeTerminalReadClient::new();
        let outcome = run_terminal_read(
            TerminalReadRequest {
                access_mode: AccessMode::AskOnRisk,
                command: "git status --short".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("terminal read succeeds");

        assert_eq!(
            outcome.access_request.capability,
            CapabilityKind::TerminalRead
        );
        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::TerminalRead);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Terminal read: git status --short")
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("git status --short")
        );
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .expect("excerpt exists")
            .contains("README.md"));
        assert_eq!(client.calls.get(), 1);
    }

    #[test]
    fn terminal_read_lists_local_directory_without_running_shell() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let listed_dir = temp_dir.path().join("folder with spaces");
        std::fs::create_dir(&listed_dir).expect("create listed dir");
        std::fs::write(listed_dir.join("alpha.txt"), "Alpha evidence").expect("write file");
        std::fs::create_dir(listed_dir.join("nested")).expect("create nested dir");
        let client = LocalTerminalReadClient::new(temp_dir.path().to_path_buf(), 4_000);
        let command = format!(
            "Get-ChildItem -LiteralPath '{}'",
            listed_dir.to_string_lossy()
        );

        let outcome = run_terminal_read(
            TerminalReadRequest {
                access_mode: AccessMode::FullAccess,
                command,
                approval_granted: false,
            },
            &client,
        )
        .expect("directory listing command is normalized safely");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert!(outcome
            .invocation
            .requested_resource
            .as_deref()
            .unwrap_or_default()
            .starts_with(TERMINAL_READ_DIRECTORY_LIST_PREFIX));
        let excerpt = outcome
            .invocation
            .excerpt
            .as_deref()
            .expect("directory excerpt exists");
        assert!(excerpt.contains("alpha.txt"));
        assert!(excerpt.contains("nested"));
    }

    #[test]
    fn terminal_read_records_failed_invocation_for_nonzero_exit_code() {
        let client = FakeTerminalReadClient::with_exit_code(128);
        let outcome = run_terminal_read(
            TerminalReadRequest {
                access_mode: AccessMode::AskOnRisk,
                command: "git status --short".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("terminal read records nonzero output");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("exited with code 128")));
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("fatal: not a git repository"));
        assert_eq!(client.calls.get(), 1);
    }

    #[test]
    fn terminal_read_waits_for_approval_when_policy_asks() {
        let client = FakeTerminalReadClient::new();
        let outcome = run_terminal_read(
            TerminalReadRequest {
                access_mode: AccessMode::AskEveryStep,
                command: "git status --short".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("terminal read returns pending result");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("git status --short")
        );
        assert_eq!(client.calls.get(), 0);
    }

    #[test]
    fn terminal_read_rejects_commands_outside_allowlist() {
        let client = FakeTerminalReadClient::new();
        let error = run_terminal_read(
            TerminalReadRequest {
                access_mode: AccessMode::LimitedAuto,
                command: "git reset --hard".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect_err("mutating command should be rejected");

        assert!(error.contains("not in the TerminalRead allowlist"));
        assert_eq!(client.calls.get(), 0);
    }

    #[test]
    fn terminal_write_boundary_waits_for_approval_when_policy_asks() {
        let outcome = run_terminal_write_boundary(TerminalWriteRequest {
            access_mode: AccessMode::AskOnRisk,
            command: "npm install".to_string(),
            approval_granted: false,
        })
        .expect("terminal write boundary returns pending result");

        assert_eq!(
            outcome.access_request.capability,
            CapabilityKind::TerminalWrite
        );
        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("npm install")
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("requires approval")));
    }

    #[test]
    fn terminal_write_boundary_blocks_execution_after_approval() {
        let outcome = run_terminal_write_boundary(TerminalWriteRequest {
            access_mode: AccessMode::AskOnRisk,
            command: "npm install".to_string(),
            approval_granted: true,
        })
        .expect("terminal write boundary records blocked execution");

        assert_eq!(
            outcome.access_request.capability,
            CapabilityKind::TerminalWrite
        );
        assert_eq!(outcome.invocation.capability, CapabilityKind::TerminalWrite);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("npm install")
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("not enabled")));
    }

    #[test]
    fn terminal_write_boundary_rejects_blank_commands() {
        let error = run_terminal_write_boundary(TerminalWriteRequest {
            access_mode: AccessMode::FullAccess,
            command: "   ".to_string(),
            approval_granted: false,
        })
        .expect_err("blank command should fail validation");

        assert!(error.contains("terminal write command is required"));
    }

    #[test]
    fn computer_screenshot_boundary_returns_structured_capture_result() {
        let client = FakeComputerScreenshotClient::new();
        let outcome = run_computer_screenshot(
            ComputerScreenshotRequest {
                access_mode: AccessMode::LimitedAuto,
                approval_granted: false,
            },
            &client,
        )
        .expect("computer screenshot succeeds");

        assert_eq!(
            outcome.access_request.capability,
            CapabilityKind::ComputerScreenshot
        );
        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(
            outcome.invocation.capability,
            CapabilityKind::ComputerScreenshot
        );
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Computer screenshot: Primary display")
        );
        assert_eq!(
            outcome.invocation.evidence_ref.as_deref(),
            Some("computer-screenshots/primary-display.png")
        );
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .expect("excerpt exists")
            .contains("1920x1080"));
        assert_eq!(client.calls.get(), 1);
    }

    #[test]
    fn computer_screenshot_boundary_waits_for_approval_when_policy_asks() {
        let client = FakeComputerScreenshotClient::new();
        let outcome = run_computer_screenshot(
            ComputerScreenshotRequest {
                access_mode: AccessMode::AskEveryStep,
                approval_granted: false,
            },
            &client,
        )
        .expect("computer screenshot returns pending result");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(client.calls.get(), 0);
    }

    #[test]
    fn computer_screenshot_boundary_waits_for_approval_on_ask_on_risk() {
        let client = FakeComputerScreenshotClient::new();
        let outcome = run_computer_screenshot(
            ComputerScreenshotRequest {
                access_mode: AccessMode::AskOnRisk,
                approval_granted: false,
            },
            &client,
        )
        .expect("computer screenshot returns pending result");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(client.calls.get(), 0);
    }

    #[test]
    fn computer_screenshot_boundary_records_capture_failure() {
        let client = FakeComputerScreenshotClient::failing();
        let outcome = run_computer_screenshot(
            ComputerScreenshotRequest {
                access_mode: AccessMode::LimitedAuto,
                approval_granted: false,
            },
            &client,
        )
        .expect("computer screenshot records failure");

        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("local screen inspection route unavailable")));
    }

    #[test]
    fn codex_bridge_screenshot_client_records_unconnected_bridge_failure() {
        let client = CodexBridgeComputerScreenshotClient::new();
        let outcome = run_computer_screenshot(
            ComputerScreenshotRequest {
                access_mode: AccessMode::LimitedAuto,
                approval_granted: false,
            },
            &client,
        )
        .expect("codex bridge screenshot records failure");

        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("Local bridge service")));
    }

    #[test]
    fn codex_bridge_http_screenshot_client_posts_contract_and_writes_evidence_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let response_body = serde_json::json!({
            "contract_version": "deepseek-agent-os.codex-bridge.v1",
            "capability": "computer_screenshot",
            "display_label": "Codex Primary",
            "width": 1280,
            "height": 720,
            "png_base64": "ZmFrZSBicmlkZ2UgcG5n",
            "captured_at": "2026-06-29T12:00:00Z"
        })
        .to_string();
        let (endpoint, handle) = serve_one_json_response(response_body);
        let client = CodexBridgeComputerScreenshotClient::with_http_endpoint(
            &endpoint,
            temp_dir.path().to_path_buf(),
        )
        .expect("http screenshot client");

        let screenshot = client
            .capture_screenshot()
            .expect("bridge screenshot succeeds");
        let recorded = handle.join().expect("fake bridge thread joins");
        let body = http_request_body(&recorded.raw);

        assert!(recorded.raw.starts_with("POST /screenshot HTTP/1.1"));
        assert_eq!(
            body["contract_version"],
            "deepseek-agent-os.codex-bridge.v1"
        );
        assert_eq!(body["capability"], "computer_screenshot");
        assert_eq!(screenshot.display_label, "Codex Primary");
        assert_eq!(screenshot.width, 1280);
        assert_eq!(screenshot.height, 720);
        assert!(screenshot.evidence_ref.ends_with("-codex-primary.png"));
        let evidence_path = temp_dir.path().join(&screenshot.evidence_ref);
        assert_eq!(
            std::fs::read(evidence_path).expect("bridge evidence file exists"),
            b"fake bridge png"
        );
    }

    #[test]
    fn local_computer_screenshot_client_writes_png_evidence_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let backend = FakeLocalScreenshotBackend::with_png(
            "External Monitor",
            1440,
            900,
            b"fake png bytes".to_vec(),
        );
        let client = LocalComputerScreenshotClient::new(temp_dir.path().to_path_buf());

        let screenshot = client
            .capture_with_backend(&backend)
            .expect("local screenshot writes evidence");

        assert_eq!(screenshot.display_label, "External Monitor");
        assert_eq!(screenshot.width, 1440);
        assert_eq!(screenshot.height, 900);
        assert!(screenshot.evidence_ref.starts_with("computer-screenshots/"));
        assert!(screenshot.evidence_ref.ends_with("-external-monitor.png"));
        let evidence_path = temp_dir.path().join(&screenshot.evidence_ref);
        assert_eq!(
            std::fs::read(evidence_path).expect("evidence file exists"),
            b"fake png bytes"
        );
    }

    #[test]
    fn local_computer_screenshot_client_rejects_empty_png_bytes() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let backend =
            FakeLocalScreenshotBackend::with_png("Primary display", 1920, 1080, Vec::new());
        let client = LocalComputerScreenshotClient::new(temp_dir.path().to_path_buf());

        let error = client
            .capture_with_backend(&backend)
            .expect_err("empty PNG bytes should fail");

        assert!(error.contains("empty PNG bytes"));
        assert!(!temp_dir.path().join("computer-screenshots").exists());
    }

    #[test]
    fn computer_control_action_parser_accepts_click_and_hotkey_actions() {
        assert_eq!(
            parse_computer_control_action("click:120,340,right").expect("click action parses"),
            ComputerControlAction::Click {
                x: 120,
                y: 340,
                button: ComputerControlMouseButton::Right,
            }
        );
        assert_eq!(
            parse_computer_control_action("hotkey:ctrl+shift+p").expect("hotkey action parses"),
            ComputerControlAction::Hotkey {
                keys: vec!["ctrl".to_string(), "shift".to_string(), "p".to_string()],
            }
        );
    }

    #[test]
    fn computer_control_action_parser_rejects_unstructured_natural_language() {
        let error = parse_computer_control_action("Click the Submit button")
            .expect_err("natural language should fail");

        assert!(error.contains("structured computer control action is required"));
    }

    #[test]
    fn computer_control_boundary_waits_for_approval_even_in_full_access() {
        let client = FakeComputerControlClient::new();
        let outcome = run_computer_control_boundary(
            ComputerControlRequest {
                access_mode: AccessMode::FullAccess,
                target: "Browser window".to_string(),
                action: "click:120,340".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("computer control boundary returns pending result");

        assert_eq!(
            outcome.access_request.capability,
            CapabilityKind::ComputerControl
        );
        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("Browser window: click:120,340")
        );
        assert_eq!(client.calls.get(), 0);
    }

    #[test]
    fn computer_control_boundary_executes_structured_action_after_approval() {
        let client = FakeComputerControlClient::new();
        let outcome = run_computer_control_boundary(
            ComputerControlRequest {
                access_mode: AccessMode::FullAccess,
                target: "Browser window".to_string(),
                action: "click:120,340,left".to_string(),
                approval_granted: true,
            },
            &client,
        )
        .expect("computer control boundary records execution");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.capability,
            CapabilityKind::ComputerControl
        );
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Computer control executed: Browser window")
        );
        assert_eq!(client.calls.get(), 1);
        assert_eq!(
            client.last_action.borrow().as_ref(),
            Some(&ComputerControlAction::Click {
                x: 120,
                y: 340,
                button: ComputerControlMouseButton::Left,
            })
        );
    }

    #[test]
    fn computer_control_boundary_records_executor_failure_after_approval() {
        let client = FakeComputerControlClient::failing();
        let outcome = run_computer_control_boundary(
            ComputerControlRequest {
                access_mode: AccessMode::FullAccess,
                target: "Browser window".to_string(),
                action: "click:120,340".to_string(),
                approval_granted: true,
            },
            &client,
        )
        .expect("computer control boundary records execution failure");

        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("local mouse and keyboard control route unavailable")));
        assert_eq!(client.calls.get(), 1);
    }

    #[test]
    fn codex_bridge_control_client_records_unconnected_bridge_failure() {
        let client = CodexBridgeComputerControlClient::new();
        let outcome = run_computer_control_boundary(
            ComputerControlRequest {
                access_mode: AccessMode::FullAccess,
                target: "Browser window".to_string(),
                action: "click:120,340".to_string(),
                approval_granted: true,
            },
            &client,
        )
        .expect("codex bridge control records failure");

        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("Local bridge service")));
    }

    #[test]
    fn codex_bridge_http_control_client_posts_target_and_structured_action() {
        let response_body = serde_json::json!({
            "contract_version": "deepseek-agent-os.codex-bridge.v1",
            "capability": "computer_control",
            "summary": "clicked left at (120, 340)"
        })
        .to_string();
        let (endpoint, handle) = serve_one_json_response(response_body);
        let client =
            CodexBridgeComputerControlClient::with_http_endpoint(&endpoint).expect("http client");

        let execution = client
            .execute_control(
                "Browser window",
                &ComputerControlAction::Click {
                    x: 120,
                    y: 340,
                    button: ComputerControlMouseButton::Left,
                },
            )
            .expect("bridge control succeeds");
        let recorded = handle.join().expect("fake bridge thread joins");
        let body = http_request_body(&recorded.raw);

        assert!(recorded.raw.starts_with("POST /control HTTP/1.1"));
        assert_eq!(
            body["contract_version"],
            "deepseek-agent-os.codex-bridge.v1"
        );
        assert_eq!(body["capability"], "computer_control");
        assert_eq!(body["target"], "Browser window");
        assert_eq!(body["action"], "click:120,340,left");
        assert_eq!(execution.summary, "clicked left at (120, 340)");
    }

    #[test]
    fn local_computer_control_client_translates_click_to_move_then_click() {
        let client = LocalComputerControlClient::new();
        let mut backend = FakeLocalControlInputBackend::default();

        let execution = client
            .execute_with_backend(
                &ComputerControlAction::Click {
                    x: 42,
                    y: 84,
                    button: ComputerControlMouseButton::Left,
                },
                &mut backend,
            )
            .expect("click translates");

        assert_eq!(backend.operations, vec!["move:42,84", "click:Left"]);
        assert!(execution.summary.contains("click"));
    }

    #[test]
    fn local_computer_control_client_releases_hotkey_keys_in_reverse_order() {
        let client = LocalComputerControlClient::new();
        let mut backend = FakeLocalControlInputBackend::default();

        client
            .execute_with_backend(
                &ComputerControlAction::Hotkey {
                    keys: vec!["ctrl".to_string(), "shift".to_string(), "p".to_string()],
                },
                &mut backend,
            )
            .expect("hotkey translates");

        assert_eq!(
            backend.operations,
            vec![
                "down:ctrl",
                "down:shift",
                "down:p",
                "up:p",
                "up:shift",
                "up:ctrl"
            ]
        );
    }

    #[test]
    fn computer_control_boundary_rejects_missing_fields() {
        let client = FakeComputerControlClient::new();
        let error = run_computer_control_boundary(
            ComputerControlRequest {
                access_mode: AccessMode::FullAccess,
                target: " ".to_string(),
                action: " ".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect_err("blank computer control should fail validation");

        assert!(error.contains("computer control target is required"));
    }

    #[test]
    fn email_send_boundary_waits_for_approval_even_in_full_access() {
        let outcome = run_email_send_boundary(EmailSendRequest {
            access_mode: AccessMode::FullAccess,
            to: "ops@example.com".to_string(),
            subject: "Weekly brief".to_string(),
            body: "Please review the attached operating notes.".to_string(),
            approval_granted: false,
        })
        .expect("email send boundary returns pending result");

        assert_eq!(outcome.access_request.capability, CapabilityKind::EmailSend);
        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("ops@example.com")
        );
    }

    #[test]
    fn email_send_boundary_blocks_send_after_approval() {
        let outcome = run_email_send_boundary(EmailSendRequest {
            access_mode: AccessMode::FullAccess,
            to: "ops@example.com".to_string(),
            subject: "Weekly brief".to_string(),
            body: "Please review the attached operating notes.".to_string(),
            approval_granted: true,
        })
        .expect("email send boundary records blocked send");

        assert_eq!(outcome.invocation.capability, CapabilityKind::EmailSend);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Email send blocked: Weekly brief")
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("not enabled")));
    }

    #[test]
    fn email_send_boundary_rejects_missing_fields() {
        let error = run_email_send_boundary(EmailSendRequest {
            access_mode: AccessMode::AskOnRisk,
            to: " ".to_string(),
            subject: " ".to_string(),
            body: " ".to_string(),
            approval_granted: false,
        })
        .expect_err("blank email should fail validation");

        assert!(error.contains("email recipient is required"));
    }

    #[test]
    fn email_draft_boundary_waits_for_approval_when_policy_asks() {
        let outcome = run_email_draft_boundary(EmailDraftRequest {
            access_mode: AccessMode::AskEveryStep,
            to: "ops@example.com".to_string(),
            subject: "Weekly brief".to_string(),
            body: "Please review the attached operating notes.".to_string(),
            approval_granted: false,
        })
        .expect("email draft boundary returns pending result");

        assert_eq!(
            outcome.access_request.capability,
            CapabilityKind::EmailDraft
        );
        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("ops@example.com")
        );
    }

    #[test]
    fn email_draft_boundary_blocks_draft_creation_after_policy_allows() {
        let outcome = run_email_draft_boundary(EmailDraftRequest {
            access_mode: AccessMode::AskOnRisk,
            to: "ops@example.com".to_string(),
            subject: "Weekly brief".to_string(),
            body: "Please review the attached operating notes.".to_string(),
            approval_granted: false,
        })
        .expect("email draft boundary records blocked draft");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::EmailDraft);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Email draft blocked: Weekly brief")
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("not enabled")));
    }

    #[test]
    fn email_draft_boundary_rejects_missing_fields() {
        let error = run_email_draft_boundary(EmailDraftRequest {
            access_mode: AccessMode::AskOnRisk,
            to: " ".to_string(),
            subject: " ".to_string(),
            body: " ".to_string(),
            approval_granted: false,
        })
        .expect_err("blank email draft should fail validation");

        assert!(error.contains("email recipient is required"));
    }

    #[test]
    fn email_read_boundary_waits_for_approval_when_policy_asks() {
        let outcome = run_email_read_boundary(EmailReadRequest {
            access_mode: AccessMode::AskOnRisk,
            mailbox: "Inbox".to_string(),
            query: "weekly brief".to_string(),
            approval_granted: false,
        })
        .expect("email read boundary returns pending result");

        assert_eq!(outcome.access_request.capability, CapabilityKind::EmailRead);
        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("Inbox: weekly brief")
        );
    }

    #[test]
    fn email_read_boundary_blocks_read_after_policy_allows() {
        let outcome = run_email_read_boundary(EmailReadRequest {
            access_mode: AccessMode::FullAccess,
            mailbox: "Inbox".to_string(),
            query: "weekly brief".to_string(),
            approval_granted: false,
        })
        .expect("email read boundary records blocked read");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::EmailRead);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Email read blocked: Inbox")
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("not enabled")));
    }

    #[test]
    fn email_read_boundary_rejects_missing_fields() {
        let error = run_email_read_boundary(EmailReadRequest {
            access_mode: AccessMode::AskOnRisk,
            mailbox: " ".to_string(),
            query: " ".to_string(),
            approval_granted: false,
        })
        .expect_err("blank email read should fail validation");

        assert!(error.contains("email mailbox is required"));
    }

    #[test]
    fn drive_read_boundary_waits_for_approval_when_policy_asks() {
        let client = LocalDriveFolderClient::new(10, 512 * 1024);
        let outcome = run_drive_read_boundary(
            DriveReadRequest {
                access_mode: AccessMode::AskEveryStep,
                location: "Shared drive".to_string(),
                query: "2026 budget".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("drive read boundary returns pending result");

        assert_eq!(outcome.access_request.capability, CapabilityKind::DriveRead);
        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("Shared drive: 2026 budget")
        );
    }

    #[test]
    fn drive_read_local_folder_returns_matching_manifest_after_policy_allows() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let budget_path = temp_dir.path().join("budget-plan.md");
        let ops_path = temp_dir.path().join("operations.md");
        std::fs::write(&budget_path, "Budget assumptions for 2026.").expect("write budget");
        std::fs::write(&ops_path, "Operations notes.").expect("write ops");
        let client = LocalDriveFolderClient::new(10, 512 * 1024);

        let outcome = run_drive_read_boundary(
            DriveReadRequest {
                access_mode: AccessMode::AskOnRisk,
                location: temp_dir.path().to_string_lossy().to_string(),
                query: "budget".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("drive read returns local folder manifest");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::DriveRead);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("budget-plan.md"));
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("utf-8 text"));
        assert!(!outcome
            .invocation
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("operations.md"));
    }

    #[test]
    fn drive_read_local_folder_records_failure_when_folder_is_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let missing_folder = temp_dir.path().join("missing-local-drive-folder");
        let client = LocalDriveFolderClient::new(10, 512 * 1024);
        let outcome = run_drive_read_boundary(
            DriveReadRequest {
                access_mode: AccessMode::AskOnRisk,
                location: missing_folder.to_string_lossy().to_string(),
                query: "2026 budget".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("drive read boundary records local folder failure");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::DriveRead);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        let expected_title = format!(
            "Drive local folder read failed: {}",
            missing_folder.to_string_lossy()
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some(expected_title.as_str())
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("metadata could not be read")));
    }

    #[test]
    fn drive_read_boundary_rejects_missing_fields() {
        let client = LocalDriveFolderClient::new(10, 512 * 1024);
        let error = run_drive_read_boundary(
            DriveReadRequest {
                access_mode: AccessMode::AskOnRisk,
                location: " ".to_string(),
                query: " ".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect_err("blank drive read should fail validation");

        assert!(error.contains("drive location is required"));
    }

    #[test]
    fn drive_write_boundary_waits_for_approval_when_policy_asks() {
        let client = LocalDriveFolderClient::new(10, 512 * 1024);
        let outcome = run_drive_write_boundary(
            DriveWriteRequest {
                access_mode: AccessMode::AskOnRisk,
                location: "Shared drive".to_string(),
                summary: "Upload weekly report.".to_string(),
                package_json: None,
                export_file: None,
                approval_granted: false,
            },
            &client,
        )
        .expect("drive write boundary returns pending result");

        assert_eq!(
            outcome.access_request.capability,
            CapabilityKind::DriveWrite
        );
        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("Shared drive: Upload weekly report.")
        );
    }

    #[test]
    fn drive_write_local_export_package_writes_json_after_policy_allows() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let client = LocalDriveFolderClient::new(10, 512 * 1024);
        let package_json = r#"{"version":"deepseek-agent-os.work-package.v1"}"#.to_string();

        let outcome = run_drive_write_boundary(
            DriveWriteRequest {
                access_mode: AccessMode::FullAccess,
                location: temp_dir.path().to_string_lossy().to_string(),
                summary: "Export current work package".to_string(),
                package_json: Some(package_json.clone()),
                export_file: None,
                approval_granted: false,
            },
            &client,
        )
        .expect("drive write exports local package");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::DriveWrite);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        let output_path = outcome.invocation.evidence_ref.expect("output path");
        assert!(output_path.ends_with(".json"));
        let output_file_name = std::path::Path::new(&output_path)
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .expect("output file name");
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains(output_file_name));
        assert_eq!(
            std::fs::read_to_string(output_path).expect("read package"),
            package_json
        );
    }

    #[test]
    fn drive_write_local_export_file_writes_markdown_after_policy_allows() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let client = LocalDriveFolderClient::new(10, 512 * 1024);
        let report_markdown = "# Operations Briefing Draft\n\n## Summary\nReady.".to_string();

        let outcome = run_drive_write_boundary(
            DriveWriteRequest {
                access_mode: AccessMode::FullAccess,
                location: temp_dir.path().to_string_lossy().to_string(),
                summary: "Export Operations Briefing report".to_string(),
                package_json: None,
                export_file: Some(DriveWriteExportFile {
                    file_name: "operations-briefing-test.md".to_string(),
                    content: report_markdown.clone(),
                    content_base64: None,
                }),
                approval_granted: false,
            },
            &client,
        )
        .expect("drive write exports local markdown report");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::DriveWrite);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        let output_path = outcome.invocation.evidence_ref.expect("output path");
        assert!(output_path.ends_with("operations-briefing-test.md"));
        assert_eq!(
            std::fs::read_to_string(output_path).expect("read report"),
            report_markdown
        );
    }

    #[test]
    fn drive_write_local_export_file_writes_binary_bytes_after_policy_allows() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let client = LocalDriveFolderClient::new(10, 512 * 1024);
        let pdf_bytes = b"%PDF-1.4\n%agent-os\n".to_vec();

        let outcome = run_drive_write_boundary(
            DriveWriteRequest {
                access_mode: AccessMode::FullAccess,
                location: temp_dir.path().to_string_lossy().to_string(),
                summary: "Export Operations Briefing PDF report".to_string(),
                package_json: None,
                export_file: Some(DriveWriteExportFile {
                    file_name: "operations-briefing-test.pdf".to_string(),
                    content: String::new(),
                    content_base64: Some(general_purpose::STANDARD.encode(&pdf_bytes)),
                }),
                approval_granted: false,
            },
            &client,
        )
        .expect("drive write exports local PDF report");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        let output_path = outcome.invocation.evidence_ref.expect("output path");
        assert!(output_path.ends_with("operations-briefing-test.pdf"));
        assert_eq!(std::fs::read(output_path).expect("read pdf"), pdf_bytes);
    }

    #[test]
    fn drive_write_local_export_records_failure_when_folder_is_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let missing_folder = temp_dir.path().join("missing-local-export-folder");
        let client = LocalDriveFolderClient::new(10, 512 * 1024);
        let outcome = run_drive_write_boundary(
            DriveWriteRequest {
                access_mode: AccessMode::FullAccess,
                location: missing_folder.to_string_lossy().to_string(),
                summary: "Upload weekly report.".to_string(),
                package_json: Some("{}".to_string()),
                export_file: None,
                approval_granted: false,
            },
            &client,
        )
        .expect("drive write boundary records local export failure");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::DriveWrite);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        let expected_title = format!(
            "Drive export package failed: {}",
            missing_folder.to_string_lossy()
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some(expected_title.as_str())
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("metadata could not be read")));
    }

    #[test]
    fn drive_write_boundary_rejects_missing_fields() {
        let client = LocalDriveFolderClient::new(10, 512 * 1024);
        let error = run_drive_write_boundary(
            DriveWriteRequest {
                access_mode: AccessMode::AskOnRisk,
                location: " ".to_string(),
                summary: " ".to_string(),
                package_json: None,
                export_file: None,
                approval_granted: false,
            },
            &client,
        )
        .expect_err("blank drive write should fail validation");

        assert!(error.contains("drive location is required"));
    }

    #[test]
    fn browser_http_client_uses_current_release_user_agent() {
        let html = "<html><head><title>Ops</title></head><body>Evidence</body></html>";
        let (endpoint, handle) = serve_one_json_response(html.to_string());
        let client = HttpBrowserPageClient::new().expect("browser client");

        let page = client.fetch_page(&endpoint).expect("page fetched");
        let recorded = handle.join().expect("fake browser server joins");
        let normalized_headers = recorded.raw.to_ascii_lowercase();

        assert_eq!(page.title, "Ops");
        assert!(recorded.raw.starts_with("GET / HTTP/1.1"));
        assert!(
            normalized_headers.contains("user-agent: deepseek-agent-os/0.1.0 browser-capability")
        );
    }

    #[test]
    fn network_search_boundary_waits_for_approval_when_policy_asks() {
        let client = FakeNetworkSearchClient::new();
        let outcome = run_network_search_boundary(
            NetworkSearchRequest {
                access_mode: AccessMode::AskEveryStep,
                query: "DeepSeek Agent OS".to_string(),
                scope: "public web".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("network search boundary returns pending result");

        assert_eq!(client.calls.get(), 0);
        assert_eq!(
            outcome.access_request.capability,
            CapabilityKind::NetworkSearch
        );
        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("public web: DeepSeek Agent OS")
        );
    }

    #[test]
    fn network_search_boundary_runs_source_client_after_policy_allows() {
        let client = FakeNetworkSearchClient::new();
        let outcome = run_network_search_boundary(
            NetworkSearchRequest {
                access_mode: AccessMode::AskOnRisk,
                query: "DeepSeek Agent OS".to_string(),
                scope: "public web".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("network search boundary records source-backed search");

        assert_eq!(client.calls.get(), 1);
        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::NetworkSearch);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Network search results via fake source search: DeepSeek Agent OS")
        );
        assert_eq!(
            outcome.invocation.requested_url.as_deref(),
            Some("https://search.example/?q=DeepSeek+Agent+OS")
        );
        assert_eq!(
            outcome.invocation.evidence_url.as_deref(),
            Some("https://example.com/deepseek-agent-os")
        );
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("durable URL"));
    }

    #[test]
    fn network_search_boundary_uses_fallback_label_for_blank_provider() {
        let client = FakeNetworkSearchClient::blank_provider();
        let outcome = run_network_search_boundary(
            NetworkSearchRequest {
                access_mode: AccessMode::AskOnRisk,
                query: "DeepSeek Agent OS".to_string(),
                scope: "public web".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("network search boundary records source-backed search");

        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("Network search results via source-backed search: DeepSeek Agent OS")
        );
    }

    #[test]
    fn codex_bridge_network_search_client_posts_query_and_maps_source_links() {
        let response_body = serde_json::json!({
            "contract_version": CODEX_BRIDGE_CONTRACT_VERSION,
            "capability": "network_search",
            "provider": "external bridge search",
            "query": "hotel ADR",
            "scope": "public web",
            "search_url": "https://bridge.local/search?q=hotel",
            "items": [
                {
                    "title": "Source",
                    "url": "https://example.com/source",
                    "snippet": "A source-backed result."
                }
            ]
        })
        .to_string();
        let (endpoint, handle) = serve_one_json_response(response_body);
        let client = CodexBridgeNetworkSearchClient::with_http_endpoint(
            LargeModelProvider::ChatGpt,
            &endpoint,
        )
        .expect("bridge client");

        let outcome = run_network_search_boundary(
            NetworkSearchRequest {
                access_mode: AccessMode::FullAccess,
                query: "hotel ADR".to_string(),
                scope: "".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("native bridge search runs");
        let recorded = handle.join().expect("server joins");

        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.invocation.evidence_url.as_deref(),
            Some("https://example.com/source")
        );
        assert!(recorded.raw.starts_with("POST /network-search "));
        assert!(recorded
            .raw
            .contains("\"large_model_provider\":\"chatgpt\""));
    }

    #[test]
    fn local_bridge_runtime_validation_errors_use_service_wording() {
        let captured_at = Utc.with_ymd_and_hms(2026, 6, 29, 12, 0, 0).unwrap();
        let errors = vec![
            super::validate_codex_bridge_screenshot_response(&CodexBridgeScreenshotResponse {
                contract_version: CODEX_BRIDGE_CONTRACT_VERSION.to_string(),
                capability: CodexBridgeCapability::ComputerControl,
                display_label: "Primary".to_string(),
                width: 1920,
                height: 1080,
                png_base64: "iVBORw0KGgo=".to_string(),
                captured_at,
            })
            .expect_err("wrong screenshot capability fails"),
            super::validate_codex_bridge_control_response(&CodexBridgeControlResponse {
                contract_version: CODEX_BRIDGE_CONTRACT_VERSION.to_string(),
                capability: CodexBridgeCapability::ComputerScreenshot,
                summary: "clicked".to_string(),
            })
            .expect_err("wrong control capability fails"),
            super::validate_codex_bridge_network_search_response(
                &CodexBridgeNetworkSearchResponse {
                    contract_version: CODEX_BRIDGE_CONTRACT_VERSION.to_string(),
                    capability: CodexBridgeCapability::NetworkSearch,
                    provider: "external bridge search".to_string(),
                    query: "hotel ADR".to_string(),
                    scope: "public web".to_string(),
                    search_url: "https://bridge.local/search?q=hotel".to_string(),
                    items: vec![CodexBridgeNetworkSearchItem {
                        title: "".to_string(),
                        url: "https://example.com/source".to_string(),
                        snippet: "Source-backed result.".to_string(),
                    }],
                },
            )
            .expect_err("blank source title fails"),
        ];

        for error in errors {
            assert!(error.contains("local bridge service"), "{error}");
            assert!(!error.contains("external bridge"), "{error}");
            assert!(!error.contains("codex bridge"), "{error}");
        }
    }

    #[test]
    fn network_search_boundary_records_provider_failure() {
        let client = FakeNetworkSearchClient::failing();
        let outcome = run_network_search_boundary(
            NetworkSearchRequest {
                access_mode: AccessMode::AskOnRisk,
                query: "DeepSeek Agent OS".to_string(),
                scope: "public web".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("network search provider failure is recorded");

        assert_eq!(client.calls.get(), 1);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("provider failed")));
    }

    #[test]
    fn network_search_html_parser_keeps_source_links() {
        let html = r#"
            <html>
              <body>
                <a class="result__a" href="/l/?uddg=https%3A%2F%2Fexample.com%2Fsource%3Fa%3D1">Example &amp; Source</a>
                <a class="result__a" href="https://second.example/report">Second Source</a>
              </body>
            </html>
        "#;

        let items = super::extract_search_result_items(html, 5);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Example & Source");
        assert_eq!(items[0].url, "https://example.com/source?a=1");
        assert_eq!(items[1].url, "https://second.example/report");
    }

    #[test]
    fn network_search_boundary_rejects_missing_query() {
        let client = FakeNetworkSearchClient::new();
        let error = run_network_search_boundary(
            NetworkSearchRequest {
                access_mode: AccessMode::AskOnRisk,
                query: " ".to_string(),
                scope: "public web".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect_err("blank network search should fail validation");

        assert!(error.contains("network search query is required"));
    }

    #[test]
    fn file_write_boundary_waits_for_approval_when_policy_asks() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let client = LocalWorkspaceFileWriteClient::new(temp_dir.path().to_path_buf(), 512 * 1024);
        let outcome = run_file_write_boundary(
            FileWriteRequest {
                access_mode: AccessMode::AskOnRisk,
                path: "docs/brief.md".to_string(),
                summary: "Update the briefing draft.".to_string(),
                content: "Draft body".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("file write boundary returns pending result");

        assert_eq!(outcome.access_request.capability, CapabilityKind::FileWrite);
        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert_eq!(
            outcome.invocation.requested_resource.as_deref(),
            Some("docs/brief.md")
        );
    }

    #[test]
    fn file_write_boundary_writes_workspace_file_after_policy_allows() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let client = LocalWorkspaceFileWriteClient::new(temp_dir.path().to_path_buf(), 512 * 1024);
        let outcome = run_file_write_boundary(
            FileWriteRequest {
                access_mode: AccessMode::FullAccess,
                path: "docs/brief.md".to_string(),
                summary: "Update the briefing draft.".to_string(),
                content: "Draft body".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("file write boundary records workspace write");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Allow);
        assert_eq!(outcome.invocation.capability, CapabilityKind::FileWrite);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            outcome.invocation.title.as_deref(),
            Some("File written: docs/brief.md")
        );
        assert_eq!(
            std::fs::read_to_string(temp_dir.path().join("docs/brief.md")).expect("file written"),
            "Draft body"
        );
        assert!(outcome
            .invocation
            .excerpt
            .as_deref()
            .unwrap_or_default()
            .contains("utf-8 text"));
        assert!(outcome.invocation.warnings.is_empty());
    }

    #[test]
    fn file_write_boundary_rejects_paths_outside_workspace() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let outside_dir = tempfile::tempdir().expect("outside dir");
        let client = LocalWorkspaceFileWriteClient::new(temp_dir.path().to_path_buf(), 512 * 1024);
        let outside_path = outside_dir.path().join("brief.md");

        let outcome = run_file_write_boundary(
            FileWriteRequest {
                access_mode: AccessMode::FullAccess,
                path: outside_path.to_string_lossy().to_string(),
                summary: "Attempt outside write.".to_string(),
                content: "Outside body".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect("file write boundary records rejected outside write");

        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::Failed
        );
        assert!(!outside_path.exists());
        assert!(outcome
            .invocation
            .warnings
            .iter()
            .any(|warning| warning.contains("workspace")));
    }

    #[test]
    fn file_write_boundary_rejects_missing_fields() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let client = LocalWorkspaceFileWriteClient::new(temp_dir.path().to_path_buf(), 512 * 1024);
        let error = run_file_write_boundary(
            FileWriteRequest {
                access_mode: AccessMode::AskOnRisk,
                path: " ".to_string(),
                summary: " ".to_string(),
                content: " ".to_string(),
                approval_granted: false,
            },
            &client,
        )
        .expect_err("blank file write should fail validation");

        assert!(error.contains("file write path is required"));
    }

    #[test]
    fn filesystem_mutation_creates_updates_deletes_file_on_local_windows_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let client = LocalFileSystemMutationClient;
        let file_path = temp_dir.path().join("notes").join("daily.txt");

        let create = run_filesystem_mutation_boundary(
            FileSystemMutationRequest {
                access_mode: AccessMode::FullAccess,
                operation: FileSystemMutationOperation::CreateFile,
                path: file_path.to_string_lossy().to_string(),
                destination: None,
                content: Some("first draft".to_string()),
                approval_granted: false,
            },
            &client,
        )
        .expect("file create succeeds");

        assert_eq!(create.access_request.decision, PolicyDecision::Allow);
        assert_eq!(
            create.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            std::fs::read_to_string(&file_path).expect("created file"),
            "first draft"
        );

        let update = run_filesystem_mutation_boundary(
            FileSystemMutationRequest {
                access_mode: AccessMode::FullAccess,
                operation: FileSystemMutationOperation::UpdateFile,
                path: file_path.to_string_lossy().to_string(),
                destination: None,
                content: Some("second draft".to_string()),
                approval_granted: false,
            },
            &client,
        )
        .expect("file update succeeds");

        assert_eq!(
            update.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert_eq!(
            std::fs::read_to_string(&file_path).expect("updated file"),
            "second draft"
        );

        let delete = run_filesystem_mutation_boundary(
            FileSystemMutationRequest {
                access_mode: AccessMode::FullAccess,
                operation: FileSystemMutationOperation::DeleteFile,
                path: file_path.to_string_lossy().to_string(),
                destination: None,
                content: None,
                approval_granted: false,
            },
            &client,
        )
        .expect("file delete succeeds");

        assert_eq!(
            delete.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert!(!file_path.exists());
    }

    #[test]
    fn filesystem_mutation_creates_renames_deletes_directory_on_local_windows_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let client = LocalFileSystemMutationClient;
        let source_dir = temp_dir.path().join("incoming");
        let renamed_dir = temp_dir.path().join("processed");

        let create = run_filesystem_mutation_boundary(
            FileSystemMutationRequest {
                access_mode: AccessMode::FullAccess,
                operation: FileSystemMutationOperation::CreateDirectory,
                path: source_dir.to_string_lossy().to_string(),
                destination: None,
                content: None,
                approval_granted: false,
            },
            &client,
        )
        .expect("directory create succeeds");

        assert_eq!(
            create.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert!(source_dir.is_dir());
        std::fs::write(source_dir.join("entry.txt"), "directory body").expect("seed nested file");

        let rename = run_filesystem_mutation_boundary(
            FileSystemMutationRequest {
                access_mode: AccessMode::FullAccess,
                operation: FileSystemMutationOperation::RenameDirectory,
                path: source_dir.to_string_lossy().to_string(),
                destination: Some(renamed_dir.to_string_lossy().to_string()),
                content: None,
                approval_granted: false,
            },
            &client,
        )
        .expect("directory rename succeeds");

        assert_eq!(
            rename.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert!(!source_dir.exists());
        assert!(renamed_dir.join("entry.txt").is_file());

        let delete = run_filesystem_mutation_boundary(
            FileSystemMutationRequest {
                access_mode: AccessMode::FullAccess,
                operation: FileSystemMutationOperation::DeleteDirectory,
                path: renamed_dir.to_string_lossy().to_string(),
                destination: None,
                content: None,
                approval_granted: false,
            },
            &client,
        )
        .expect("directory delete succeeds");

        assert_eq!(
            delete.invocation.status,
            CapabilityInvocationStatus::Succeeded
        );
        assert!(!renamed_dir.exists());
    }

    #[test]
    fn filesystem_mutation_waits_for_approval_without_mutating() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let client = LocalFileSystemMutationClient;
        let file_path = temp_dir.path().join("pending.txt");

        let outcome = run_filesystem_mutation_boundary(
            FileSystemMutationRequest {
                access_mode: AccessMode::AskOnRisk,
                operation: FileSystemMutationOperation::CreateFile,
                path: file_path.to_string_lossy().to_string(),
                destination: None,
                content: Some("pending body".to_string()),
                approval_granted: false,
            },
            &client,
        )
        .expect("filesystem mutation waits for approval");

        assert_eq!(outcome.access_request.decision, PolicyDecision::Ask);
        assert_eq!(
            outcome.invocation.status,
            CapabilityInvocationStatus::PendingApproval
        );
        assert!(!file_path.exists());
    }
}
