use std::fs;
#[cfg(not(windows))]
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use quick_xml::{events::Event, Reader};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroize;

use crate::kernel::models::AccessMode;
use crate::kernel::tool_runtime::{ToolExecutionRequest, CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID};

const MAX_ATTACHMENT_BYTES: u64 = 20 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 1024;
const MAX_ARCHIVE_EXPANDED_BYTES: u64 = 100 * 1024 * 1024;
const LANDING_DIR_NAME: &str = "connector-downloads";

#[derive(Clone, Deserialize, Serialize)]
pub struct ConnectorAttachmentMetadata {
    pub account_id: Uuid,
    pub provider_id: String,
    pub parent_remote_ref: String,
    pub attachment_remote_ref: String,
    pub file_name: String,
    pub declared_media_type: String,
    pub size_bytes: u64,
    pub contains_macros: bool,
    pub untrusted_evidence: bool,
}

impl ConnectorAttachmentMetadata {
    fn validate(&self) -> Result<AttachmentType, String> {
        if self.provider_id.trim().is_empty()
            || self.provider_id.len() > 64
            || self.parent_remote_ref.trim().is_empty()
            || self.parent_remote_ref.len() > 1024
            || self.attachment_remote_ref.trim().is_empty()
            || self.attachment_remote_ref.len() > 1024
            || self.file_name.trim().is_empty()
            || self.file_name.len() > 255
            || self.declared_media_type.trim().is_empty()
            || self.declared_media_type.len() > 255
            || self.size_bytes == 0
            || self.size_bytes > MAX_ATTACHMENT_BYTES
            || self.contains_macros
            || !self.untrusted_evidence
        {
            return Err("connector attachment metadata is unsafe".to_string());
        }
        let name = self.file_name.trim();
        if name == "."
            || name == ".."
            || name.contains(['/', '\\', ':'])
            || name.chars().any(char::is_control)
            || Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name)
        {
            return Err("connector attachment filename is unsafe".to_string());
        }
        AttachmentType::from_name_and_media_type(name, &self.declared_media_type)
    }

    pub(crate) fn tool_request(
        &self,
        account_generation: u64,
        workspace_identity: &str,
        access_mode: AccessMode,
        run_id: Option<Uuid>,
    ) -> Result<ToolExecutionRequest, String> {
        self.validate()?;
        Ok(ToolExecutionRequest {
            tool_id: CONNECTOR_ATTACHMENT_DOWNLOAD_TOOL_ID.to_string(),
            input: serde_json::json!({
                "provider_id": self.provider_id,
                "account_id": self.account_id,
                "parent_remote_ref": self.parent_remote_ref,
                "attachment_remote_ref": self.attachment_remote_ref,
                "file_name": self.file_name,
                "media_type": self.declared_media_type,
                "size_bytes": self.size_bytes,
                "account_generation": account_generation,
                "workspace_identity": workspace_identity,
            }),
            access_mode,
            run_id,
        })
    }

    pub(crate) fn expected_landing_ref(&self, landing_id: Uuid) -> Result<String, String> {
        Ok(format!("{landing_id}.{}", self.validate()?.extension()))
    }
}

#[cfg(test)]
pub struct ConnectorAttachmentLandingTicket {
    fingerprint: String,
    workspace_identity: String,
    account_id: Uuid,
    generation: u64,
    expires_at: DateTime<Utc>,
    consumed: bool,
}

#[cfg(test)]
impl ConnectorAttachmentLandingTicket {
    pub(crate) fn from_exact_approval(
        metadata: &ConnectorAttachmentMetadata,
        generation: u64,
        workspace_identity: &str,
        approved_fingerprint: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        let fingerprint = connector_attachment_landing_fingerprint(metadata, generation)?;
        if approved_fingerprint != fingerprint || expires_at <= Utc::now() {
            return Err("connector attachment approval is invalid".to_string());
        }
        Ok(Self {
            fingerprint,
            workspace_identity: workspace_identity.to_string(),
            account_id: metadata.account_id,
            generation,
            expires_at,
            consumed: false,
        })
    }

    pub(crate) fn begin_download(
        &mut self,
        metadata: &ConnectorAttachmentMetadata,
        current_generation: u64,
    ) -> Result<ConnectorAttachmentDownloadPermit, String> {
        if self.consumed
            || self.expires_at <= Utc::now()
            || self.account_id != metadata.account_id
            || self.generation != current_generation
            || self.fingerprint
                != connector_attachment_landing_fingerprint(metadata, current_generation)?
        {
            return Err("connector attachment approval is invalid".to_string());
        }
        self.consumed = true;
        Ok(ConnectorAttachmentDownloadPermit {
            reservation_id: Uuid::new_v4(),
            fingerprint: self.fingerprint.clone(),
            workspace_identity: self.workspace_identity.clone(),
            account_id: self.account_id,
            generation: self.generation,
        })
    }
}

pub struct ConnectorAttachmentDownloadPermit {
    reservation_id: Uuid,
    fingerprint: String,
    workspace_identity: String,
    account_id: Uuid,
    generation: u64,
}

impl ConnectorAttachmentDownloadPermit {
    pub(crate) fn reserved(
        reservation_id: Uuid,
        metadata: &ConnectorAttachmentMetadata,
        generation: u64,
        fingerprint: String,
        workspace_identity: String,
    ) -> Result<Self, String> {
        if fingerprint != connector_attachment_landing_fingerprint(metadata, generation)? {
            return Err("connector attachment reservation is invalid".to_string());
        }
        Ok(Self {
            reservation_id,
            fingerprint,
            workspace_identity,
            account_id: metadata.account_id,
            generation,
        })
    }

    pub(crate) fn reservation_id(&self) -> Uuid {
        self.reservation_id
    }

    pub(crate) fn validate(&self, metadata: &ConnectorAttachmentMetadata) -> Result<(), String> {
        if self.account_id != metadata.account_id
            || self.fingerprint
                != connector_attachment_landing_fingerprint(metadata, self.generation)?
        {
            return Err("connector attachment download permit is invalid".to_string());
        }
        Ok(())
    }

    pub(crate) fn validate_workspace(&self, workspace_identity: &str) -> Result<(), String> {
        if self.workspace_identity != workspace_identity {
            return Err("connector attachment workspace changed before download".to_string());
        }
        Ok(())
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorAttachmentLandingReceipt {
    pub landing_id: Uuid,
    pub account_id: Uuid,
    pub provider_id: String,
    pub account_generation: u64,
    pub landing_ref: String,
    pub media_type: String,
    pub byte_size: u64,
    pub sha256: String,
    #[serde(default)]
    pub storage_identity: String,
    pub untrusted_evidence: bool,
    pub completed_at: DateTime<Utc>,
}

pub struct LandedConnectorAttachment {
    receipt: ConnectorAttachmentLandingReceipt,
    path: PathBuf,
    #[cfg(windows)]
    _file: fs::File,
    #[cfg(windows)]
    _landing_root: crate::kernel::connectors::landing_windows::ManagedLandingRoot,
}

pub(crate) struct StagedConnectorAttachment {
    receipt: ConnectorAttachmentLandingReceipt,
    temp_path: PathBuf,
    final_path: PathBuf,
    #[cfg(windows)]
    file: fs::File,
    #[cfg(windows)]
    landing_root: crate::kernel::connectors::landing_windows::ManagedLandingRoot,
}

pub(crate) struct ConnectorAttachmentCleanupCandidate {
    pub landing_id: Uuid,
    pub claim_id: Uuid,
    pub metadata: ConnectorAttachmentMetadata,
    pub workspace_root: PathBuf,
    pub workspace_identity: String,
    pub storage_identity: Option<String>,
    pub receipt: Option<ConnectorAttachmentLandingReceipt>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectorAttachmentCleanupFailure {
    Unsafe,
    Transient,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectorAttachmentReadyRecoveryFailure {
    Missing,
    Unsafe,
    Transient,
}

impl LandedConnectorAttachment {
    pub(crate) fn receipt(&self) -> &ConnectorAttachmentLandingReceipt {
        &self.receipt
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl StagedConnectorAttachment {
    pub(crate) fn receipt(&self) -> &ConnectorAttachmentLandingReceipt {
        &self.receipt
    }

    pub(crate) fn commit(self) -> Result<LandedConnectorAttachment, String> {
        #[cfg(windows)]
        {
            let identity = self.landing_root.file_identity(&self.file)?.encoded();
            if identity != self.receipt.storage_identity {
                return Err("connector attachment file identity changed before commit".to_string());
            }
            let path = self
                .landing_root
                .rename_staged_file(&self.file, &self.receipt.landing_ref)?;
            let committed_identity = self.landing_root.file_identity(&self.file)?.encoded();
            if committed_identity != self.receipt.storage_identity {
                return Err("connector attachment file identity changed during commit".to_string());
            }
            Ok(LandedConnectorAttachment {
                receipt: self.receipt,
                path,
                _file: self.file,
                _landing_root: self.landing_root,
            })
        }
        #[cfg(not(windows))]
        {
            fs::rename(&self.temp_path, &self.final_path)
                .map_err(|_| "connector attachment commit failed".to_string())?;
            Ok(LandedConnectorAttachment {
                receipt: self.receipt,
                path: self.final_path,
            })
        }
    }
}

pub(crate) fn cleanup_incomplete_connector_attachment(
    candidate: &ConnectorAttachmentCleanupCandidate,
) -> Result<(), ConnectorAttachmentCleanupFailure> {
    #[cfg(windows)]
    {
        use crate::kernel::connectors::landing_windows::{
            IdentityDeleteResult, ManagedFilePresence, ManagedLandingRoot,
        };
        let managed = ManagedLandingRoot::open(&candidate.workspace_root).map_err(|error| {
            if error.contains("unsafe") || error.contains("reparse") {
                ConnectorAttachmentCleanupFailure::Unsafe
            } else {
                ConnectorAttachmentCleanupFailure::Transient
            }
        })?;
        if managed.binding() != candidate.workspace_identity {
            return Err(ConnectorAttachmentCleanupFailure::Unsafe);
        }
        let final_ref = candidate
            .metadata
            .expected_landing_ref(candidate.landing_id)
            .map_err(|_| ConnectorAttachmentCleanupFailure::Unsafe)?;
        let basenames = [format!(".{}.part", candidate.landing_id), final_ref];
        let expected_identity = candidate
            .receipt
            .as_ref()
            .map(|receipt| receipt.storage_identity.as_str())
            .or(candidate.storage_identity.as_deref())
            .filter(|value| !value.is_empty());
        let Some(expected_identity) = expected_identity else {
            for basename in &basenames {
                match managed
                    .file_presence_no_reparse(basename)
                    .map_err(|error| {
                        if error.contains("unsafe") || error.contains("reparse") {
                            ConnectorAttachmentCleanupFailure::Unsafe
                        } else {
                            ConnectorAttachmentCleanupFailure::Transient
                        }
                    })? {
                    ManagedFilePresence::Missing => {}
                    ManagedFilePresence::Present => {
                        return Err(ConnectorAttachmentCleanupFailure::Unsafe)
                    }
                }
            }
            return Ok(());
        };
        let mut deleted = false;
        for basename in basenames {
            match managed
                .delete_file_if_identity(&basename, expected_identity)
                .map_err(|error| {
                    if error.contains("unsafe") || error.contains("reparse") {
                        ConnectorAttachmentCleanupFailure::Unsafe
                    } else {
                        ConnectorAttachmentCleanupFailure::Transient
                    }
                })? {
                IdentityDeleteResult::Missing => {}
                IdentityDeleteResult::Deleted => deleted = true,
                IdentityDeleteResult::IdentityMismatch => {
                    return Err(ConnectorAttachmentCleanupFailure::Unsafe)
                }
            }
        }
        if !deleted {
            return Ok(());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let (_, current_workspace_identity) =
            connector_attachment_workspace_binding(&candidate.workspace_root).map_err(|error| {
                if error.contains("unsafe") {
                    ConnectorAttachmentCleanupFailure::Unsafe
                } else {
                    ConnectorAttachmentCleanupFailure::Transient
                }
            })?;
        if current_workspace_identity != candidate.workspace_identity {
            return Err(ConnectorAttachmentCleanupFailure::Unsafe);
        }
        let landing_root = prepare_landing_root(&candidate.workspace_root).map_err(|error| {
            if error.contains("unsafe") {
                ConnectorAttachmentCleanupFailure::Unsafe
            } else {
                ConnectorAttachmentCleanupFailure::Transient
            }
        })?;
        let final_ref = candidate
            .metadata
            .expected_landing_ref(candidate.landing_id)
            .map_err(|_| ConnectorAttachmentCleanupFailure::Unsafe)?;
        for path in [
            landing_root.join(format!(".{}.part", candidate.landing_id)),
            landing_root.join(final_ref),
        ] {
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    return Err(ConnectorAttachmentCleanupFailure::Unsafe)
                }
                Ok(_) => fs::remove_file(path)
                    .map_err(|_| ConnectorAttachmentCleanupFailure::Transient)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(ConnectorAttachmentCleanupFailure::Transient),
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
pub(crate) fn recover_ready_connector_attachment(
    candidate: &ConnectorAttachmentCleanupCandidate,
) -> Result<LandedConnectorAttachment, ConnectorAttachmentReadyRecoveryFailure> {
    use crate::kernel::connectors::landing_windows::{IdentityOpenResult, ManagedLandingRoot};

    let receipt = candidate
        .receipt
        .as_ref()
        .filter(|receipt| !receipt.storage_identity.is_empty())
        .ok_or(ConnectorAttachmentReadyRecoveryFailure::Unsafe)?;
    if candidate.storage_identity.as_deref() != Some(receipt.storage_identity.as_str()) {
        return Err(ConnectorAttachmentReadyRecoveryFailure::Unsafe);
    }
    let expected_ref = candidate
        .metadata
        .expected_landing_ref(candidate.landing_id)
        .map_err(|_| ConnectorAttachmentReadyRecoveryFailure::Unsafe)?;
    if receipt.landing_id != candidate.landing_id
        || receipt.account_id != candidate.metadata.account_id
        || receipt.provider_id != candidate.metadata.provider_id
        || receipt.landing_ref != expected_ref
        || receipt.media_type != candidate.metadata.declared_media_type
        || receipt.byte_size != candidate.metadata.size_bytes
        || receipt.sha256.len() != 64
        || !receipt.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !receipt.untrusted_evidence
    {
        return Err(ConnectorAttachmentReadyRecoveryFailure::Unsafe);
    }
    let managed = ManagedLandingRoot::open(&candidate.workspace_root).map_err(|error| {
        if error.contains("unsafe") || error.contains("reparse") {
            ConnectorAttachmentReadyRecoveryFailure::Unsafe
        } else {
            ConnectorAttachmentReadyRecoveryFailure::Transient
        }
    })?;
    if managed.binding() != candidate.workspace_identity {
        return Err(ConnectorAttachmentReadyRecoveryFailure::Unsafe);
    }
    let temp_name = format!(".{}.part", candidate.landing_id);
    let temp = managed
        .open_file_if_identity(&temp_name, &receipt.storage_identity)
        .map_err(|_| ConnectorAttachmentReadyRecoveryFailure::Transient)?;
    let final_file = managed
        .open_file_if_identity(&expected_ref, &receipt.storage_identity)
        .map_err(|_| ConnectorAttachmentReadyRecoveryFailure::Transient)?;
    let (mut file, path) = match (temp, final_file) {
        (IdentityOpenResult::IdentityMismatch, _)
        | (_, IdentityOpenResult::IdentityMismatch)
        | (IdentityOpenResult::Opened(_), IdentityOpenResult::Opened(_)) => {
            return Err(ConnectorAttachmentReadyRecoveryFailure::Unsafe)
        }
        (IdentityOpenResult::Missing, IdentityOpenResult::Missing) => {
            return Err(ConnectorAttachmentReadyRecoveryFailure::Missing)
        }
        (IdentityOpenResult::Opened(file), IdentityOpenResult::Missing) => {
            let path = managed
                .rename_staged_file(&file, &expected_ref)
                .map_err(|_| ConnectorAttachmentReadyRecoveryFailure::Transient)?;
            (file, path)
        }
        (IdentityOpenResult::Missing, IdentityOpenResult::Opened(file)) => {
            (file, managed.landing_root().join(&expected_ref))
        }
    };
    validate_ready_file(&mut file, &candidate.metadata, receipt)?;
    Ok(LandedConnectorAttachment {
        receipt: receipt.clone(),
        path,
        _file: file,
        _landing_root: managed,
    })
}

#[cfg(windows)]
fn validate_ready_file(
    file: &mut fs::File,
    metadata: &ConnectorAttachmentMetadata,
    receipt: &ConnectorAttachmentLandingReceipt,
) -> Result<(), ConnectorAttachmentReadyRecoveryFailure> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ConnectorAttachmentReadyRecoveryFailure::Transient)?;
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| ConnectorAttachmentReadyRecoveryFailure::Transient)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .ok_or(ConnectorAttachmentReadyRecoveryFailure::Unsafe)?;
        if total > metadata.size_bytes || total > MAX_ATTACHMENT_BYTES {
            buffer[..count].zeroize();
            return Err(ConnectorAttachmentReadyRecoveryFailure::Unsafe);
        }
        digest.update(&buffer[..count]);
        buffer[..count].zeroize();
    }
    buffer.zeroize();
    if total != receipt.byte_size || format!("{:x}", digest.finalize()) != receipt.sha256 {
        return Err(ConnectorAttachmentReadyRecoveryFailure::Unsafe);
    }
    metadata
        .validate()
        .and_then(|kind| kind.validate_file(file))
        .map_err(|_| ConnectorAttachmentReadyRecoveryFailure::Unsafe)
}

pub(crate) fn connector_attachment_workspace_binding(
    workspace_root: &Path,
) -> Result<(PathBuf, String), String> {
    #[cfg(windows)]
    {
        let managed =
            crate::kernel::connectors::landing_windows::ManagedLandingRoot::open(workspace_root)?;
        if !managed.handles_are_live() {
            return Err("connector attachment workspace handles are unavailable".to_string());
        }
        Ok((managed.workspace_root().to_path_buf(), managed.binding()))
    }
    #[cfg(not(windows))]
    {
        crate::kernel::sandbox::enforce_local_mutation_path(workspace_root)?;
        let workspace_root = fs::canonicalize(workspace_root)
            .map_err(|_| "connector attachment workspace is unavailable".to_string())?;
        if !workspace_root.is_dir() {
            return Err("connector attachment workspace is unavailable".to_string());
        }
        let landing_root = prepare_landing_root(&workspace_root)?;
        let mut digest = Sha256::new();
        digest.update(b"ds-agent.connector-attachment-workspace.v1\0");
        digest.update(landing_root.to_string_lossy().as_bytes());
        Ok((workspace_root, format!("{:x}", digest.finalize())))
    }
}

pub(crate) fn connector_attachment_landing_fingerprint(
    metadata: &ConnectorAttachmentMetadata,
    generation: u64,
) -> Result<String, String> {
    metadata.validate()?;
    let mut digest = Sha256::new();
    for value in [
        "connector.attachment.land.v1".to_string(),
        metadata.account_id.to_string(),
        metadata.provider_id.trim().to_string(),
        metadata.parent_remote_ref.clone(),
        metadata.attachment_remote_ref.clone(),
        metadata.file_name.trim().to_string(),
        metadata.declared_media_type.trim().to_ascii_lowercase(),
        metadata.size_bytes.to_string(),
        generation.to_string(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn stage_connector_attachment(
    workspace_root: &Path,
    metadata: &ConnectorAttachmentMetadata,
    permit: ConnectorAttachmentDownloadPermit,
    source: impl Read,
) -> Result<StagedConnectorAttachment, String> {
    stage_connector_attachment_with_checkpoint(workspace_root, metadata, permit, source, |_, _| {
        Ok(())
    })
}

pub(crate) fn stage_connector_attachment_with_checkpoint(
    workspace_root: &Path,
    metadata: &ConnectorAttachmentMetadata,
    permit: ConnectorAttachmentDownloadPermit,
    mut source: impl Read,
    on_staging: impl FnOnce(Uuid, &str) -> Result<(), String>,
) -> Result<StagedConnectorAttachment, String> {
    let attachment_type = metadata.validate()?;
    #[cfg(windows)]
    let managed_landing =
        crate::kernel::connectors::landing_windows::ManagedLandingRoot::open(workspace_root)?;
    #[cfg(windows)]
    let workspace_identity = managed_landing.binding();
    #[cfg(not(windows))]
    let (_, workspace_identity) = connector_attachment_workspace_binding(workspace_root)?;
    permit.validate_workspace(&workspace_identity)?;
    permit.validate(metadata)?;
    let generation = permit.generation;

    #[cfg(windows)]
    let landing_root = managed_landing.landing_root().to_path_buf();
    #[cfg(not(windows))]
    let landing_root = prepare_landing_root(workspace_root)?;
    let landing_id = permit.reservation_id;
    let temp_path = landing_root.join(format!(".{landing_id}.part"));
    let final_name = metadata.expected_landing_ref(landing_id)?;
    let final_path = landing_root.join(&final_name);
    let result = (|| {
        #[cfg(windows)]
        let mut file = managed_landing.create_staged_file(&format!(".{landing_id}.part"))?;
        #[cfg(not(windows))]
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|_| "connector attachment staging failed".to_string())?;
        #[cfg(windows)]
        let storage_identity = managed_landing.file_identity(&file)?.encoded();
        #[cfg(not(windows))]
        let storage_identity = String::new();
        on_staging(landing_id, &storage_identity)?;
        let mut digest = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = source
                .read(&mut buffer)
                .map_err(|_| "connector attachment download failed".to_string())?;
            if count == 0 {
                break;
            }
            total = total
                .checked_add(count as u64)
                .ok_or_else(|| "connector attachment exceeds the size limit".to_string())?;
            if total > MAX_ATTACHMENT_BYTES || total > metadata.size_bytes {
                buffer[..count].zeroize();
                return Err("connector attachment exceeds the approved size".to_string());
            }
            digest.update(&buffer[..count]);
            file.write_all(&buffer[..count])
                .map_err(|_| "connector attachment staging failed".to_string())?;
            buffer[..count].zeroize();
        }
        buffer.zeroize();
        if total != metadata.size_bytes {
            return Err("connector attachment size did not match metadata".to_string());
        }
        file.sync_all()
            .map_err(|_| "connector attachment staging failed".to_string())?;
        attachment_type.validate_file(&mut file)?;
        #[cfg(windows)]
        if managed_landing.file_identity(&file)?.encoded() != storage_identity {
            return Err("connector attachment file identity changed during staging".to_string());
        }
        Ok(StagedConnectorAttachment {
            receipt: ConnectorAttachmentLandingReceipt {
                landing_id: permit.reservation_id,
                account_id: metadata.account_id,
                provider_id: metadata.provider_id.trim().to_string(),
                account_generation: generation,
                landing_ref: final_name,
                media_type: attachment_type.media_type().to_string(),
                byte_size: total,
                sha256: format!("{:x}", digest.finalize()),
                storage_identity,
                untrusted_evidence: true,
                completed_at: Utc::now(),
            },
            temp_path: temp_path.clone(),
            final_path: final_path.clone(),
            #[cfg(windows)]
            file,
            #[cfg(windows)]
            landing_root: managed_landing,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn prepare_landing_root(workspace_root: &Path) -> Result<PathBuf, String> {
    crate::kernel::sandbox::enforce_local_mutation_path(workspace_root)?;
    let workspace = fs::canonicalize(workspace_root)
        .map_err(|_| "connector attachment workspace is unavailable".to_string())?;
    if !workspace.is_dir() {
        return Err("connector attachment workspace is unavailable".to_string());
    }
    let landing = workspace.join(LANDING_DIR_NAME);
    crate::kernel::sandbox::enforce_local_mutation_path(&landing)?;
    fs::create_dir_all(&landing)
        .map_err(|_| "connector attachment landing directory is unavailable".to_string())?;
    let metadata = fs::symlink_metadata(&landing)
        .map_err(|_| "connector attachment landing directory is unavailable".to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("connector attachment landing directory is unsafe".to_string());
    }
    let landing = fs::canonicalize(&landing)
        .map_err(|_| "connector attachment landing directory is unavailable".to_string())?;
    if !landing.starts_with(&workspace) {
        return Err("connector attachment landing directory is unsafe".to_string());
    }
    Ok(landing)
}

#[derive(Clone, Copy)]
enum AttachmentType {
    Pdf,
    Png,
    Jpeg,
    Text,
    Csv,
    Json,
    Docx,
    Xlsx,
    Pptx,
}

impl AttachmentType {
    fn from_name_and_media_type(name: &str, media_type: &str) -> Result<Self, String> {
        let extension = Path::new(name)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let media_type = media_type.trim().to_ascii_lowercase();
        let kind = match (extension.as_str(), media_type.as_str()) {
            ("pdf", "application/pdf") => Self::Pdf,
            ("png", "image/png") => Self::Png,
            ("jpg" | "jpeg", "image/jpeg") => Self::Jpeg,
            ("txt", "text/plain") => Self::Text,
            ("csv", "text/csv") => Self::Csv,
            ("json", "application/json") => Self::Json,
            ("docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document") => {
                Self::Docx
            }
            ("xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet") => {
                Self::Xlsx
            }
            (
                "pptx",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ) => Self::Pptx,
            _ => return Err("connector attachment type is unsupported or ambiguous".to_string()),
        };
        Ok(kind)
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Text => "txt",
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
        }
    }

    fn media_type(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Text => "text/plain",
            Self::Csv => "text/csv",
            Self::Json => "application/json",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Self::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
        }
    }

    fn validate_file(self, file: &mut fs::File) -> Result<(), String> {
        file.seek(SeekFrom::Start(0))
            .map_err(|_| "connector attachment validation failed".to_string())?;
        let mut prefix = [0u8; 8];
        let count = file
            .read(&mut prefix)
            .map_err(|_| "connector attachment validation failed".to_string())?;
        let prefix = &prefix[..count];
        match self {
            Self::Pdf if prefix.starts_with(b"%PDF-") => Ok(()),
            Self::Png if prefix == b"\x89PNG\r\n\x1a\n" => Ok(()),
            Self::Jpeg if prefix.starts_with(&[0xff, 0xd8, 0xff]) => Ok(()),
            Self::Text | Self::Csv | Self::Json => validate_text_file(file, self),
            Self::Docx | Self::Xlsx | Self::Pptx if prefix.starts_with(b"PK") => {
                validate_office_archive(file, self)
            }
            _ => Err("connector attachment detected type did not match metadata".to_string()),
        }
    }
}

fn validate_text_file(file: &mut fs::File, kind: AttachmentType) -> Result<(), String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "connector attachment validation failed".to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_ATTACHMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "connector attachment validation failed".to_string())?;
    if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
        bytes.zeroize();
        return Err("connector attachment exceeds the size limit".to_string());
    }
    if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
        bytes.zeroize();
        return Err("connector attachment text encoding is invalid".to_string());
    }
    let json_valid = !matches!(kind, AttachmentType::Json)
        || serde_json::from_slice::<serde_json::Value>(&bytes).is_ok();
    bytes.zeroize();
    if json_valid {
        Ok(())
    } else {
        Err("connector attachment JSON is invalid".to_string())
    }
}

fn validate_office_archive(file: &mut fs::File, kind: AttachmentType) -> Result<(), String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "connector attachment archive is invalid".to_string())?;
    let cloned = file
        .try_clone()
        .map_err(|_| "connector attachment archive is invalid".to_string())?;
    let mut archive = zip::ZipArchive::new(cloned)
        .map_err(|_| "connector attachment archive is invalid".to_string())?;
    if archive.is_empty() || archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err("connector attachment archive budget exceeded".to_string());
    }
    let mut declared_expanded = 0u64;
    let mut expanded = 0u64;
    let mut required_part_found = false;
    let mut content_types_valid = false;
    let mut root_relationship_valid = false;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| "connector attachment archive is invalid".to_string())?;
        if entry.enclosed_name().is_none() {
            return Err("connector attachment archive path is unsafe".to_string());
        }
        declared_expanded = declared_expanded
            .checked_add(entry.size())
            .ok_or_else(|| "connector attachment archive budget exceeded".to_string())?;
        if declared_expanded > MAX_ARCHIVE_EXPANDED_BYTES {
            return Err("connector attachment archive budget exceeded".to_string());
        }
        let name = entry.name().to_ascii_lowercase();
        required_part_found |= match kind {
            AttachmentType::Docx => name == "word/document.xml",
            AttachmentType::Xlsx => name == "xl/workbook.xml",
            AttachmentType::Pptx => name == "ppt/presentation.xml",
            _ => false,
        };
        if name.contains("vbaproject")
            || name.contains("/embeddings/")
            || name.ends_with(".exe")
            || name.ends_with(".dll")
            || name.ends_with(".js")
            || name.ends_with(".vbs")
            || name.ends_with(".ps1")
            || name.ends_with(".bat")
            || name.ends_with(".cmd")
            || name.ends_with(".bin")
            || entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0)
        {
            return Err("connector attachment active content is blocked".to_string());
        }
        let mut entry_bytes = 0u64;
        let mut relation_bytes = if name.ends_with(".rels") {
            Some(Vec::new())
        } else {
            None
        };
        let mut content_type_bytes = if name == "[content_types].xml" {
            Some(Vec::new())
        } else {
            None
        };
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = entry
                .read(&mut buffer)
                .map_err(|_| "connector attachment archive is invalid".to_string())?;
            if count == 0 {
                break;
            }
            entry_bytes = entry_bytes
                .checked_add(count as u64)
                .ok_or_else(|| "connector attachment archive budget exceeded".to_string())?;
            if expanded.saturating_add(entry_bytes) > MAX_ARCHIVE_EXPANDED_BYTES {
                buffer[..count].zeroize();
                return Err("connector attachment archive budget exceeded".to_string());
            }
            if let Some(relation) = relation_bytes.as_mut() {
                if relation.len().saturating_add(count) > 1024 * 1024 {
                    buffer[..count].zeroize();
                    return Err("connector attachment relationship is oversized".to_string());
                }
                relation.extend_from_slice(&buffer[..count]);
            }
            if let Some(content_types) = content_type_bytes.as_mut() {
                if content_types.len().saturating_add(count) > 1024 * 1024 {
                    buffer[..count].zeroize();
                    return Err("connector attachment content types are oversized".to_string());
                }
                content_types.extend_from_slice(&buffer[..count]);
            }
            buffer[..count].zeroize();
        }
        buffer.zeroize();
        expanded = expanded
            .checked_add(entry_bytes)
            .ok_or_else(|| "connector attachment archive budget exceeded".to_string())?;
        if let Some(mut relation) = relation_bytes {
            let root_office_document =
                validate_opc_relationships(&relation, name == "_rels/.rels", kind)?;
            root_relationship_valid |= root_office_document;
            relation.zeroize();
        }
        if let Some(mut content_types) = content_type_bytes {
            content_types_valid = validate_opc_content_types(&content_types, kind)?;
            content_types.zeroize();
        }
    }
    if required_part_found && content_types_valid && root_relationship_valid {
        Ok(())
    } else {
        Err("connector attachment Office package type is invalid".to_string())
    }
}

fn validate_opc_content_types(bytes: &[u8], kind: AttachmentType) -> Result<bool, String> {
    let (expected_part, expected_type) = match kind {
        AttachmentType::Docx => (
            "/word/document.xml",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        ),
        AttachmentType::Xlsx => (
            "/xl/workbook.xml",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
        ),
        AttachmentType::Pptx => (
            "/ppt/presentation.xml",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
        ),
        _ => return Err("connector attachment Office package type is invalid".to_string()),
    };
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut found = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event
                    .local_name()
                    .as_ref()
                    .eq_ignore_ascii_case(b"override") =>
            {
                let (part, content_type, _) = opc_attributes(&event)?;
                found |= part.as_deref() == Some(expected_part)
                    && content_type.as_deref() == Some(expected_type);
            }
            Ok(Event::Eof) => break,
            Ok(Event::DocType(_)) | Ok(Event::PI(_)) => {
                return Err("connector attachment OPC XML declarations are unsafe".to_string())
            }
            Ok(_) => {}
            Err(_) => return Err("connector attachment OPC XML is invalid".to_string()),
        }
    }
    Ok(found)
}

fn validate_opc_relationships(
    bytes: &[u8],
    root_relationships: bool,
    kind: AttachmentType,
) -> Result<bool, String> {
    let expected_target = match kind {
        AttachmentType::Docx => "word/document.xml",
        AttachmentType::Xlsx => "xl/workbook.xml",
        AttachmentType::Pptx => "ppt/presentation.xml",
        _ => return Err("connector attachment Office package type is invalid".to_string()),
    };
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut root_office_document = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) | Ok(Event::Empty(event))
                if event
                    .local_name()
                    .as_ref()
                    .eq_ignore_ascii_case(b"relationship") =>
            {
                let (_, _, relationship) = opc_attributes(&event)?;
                let Some((target, target_mode, relationship_type)) = relationship else {
                    return Err("connector attachment OPC relationship is invalid".to_string());
                };
                let target_lower = target.to_ascii_lowercase();
                if target_mode
                    .as_deref()
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("external"))
                    || target_lower.contains("://")
                    || target_lower.contains("%3a")
                    || target_lower.starts_with("file:")
                    || target_lower.starts_with("\\\\")
                    || target_lower.starts_with("//")
                    || target.contains('\0')
                {
                    return Err("connector attachment external relationship is blocked".to_string());
                }
                if root_relationships
                    && relationship_type.ends_with("/officeDocument")
                    && target == expected_target
                {
                    root_office_document = true;
                }
            }
            Ok(Event::Eof) => break,
            Ok(Event::DocType(_)) | Ok(Event::PI(_)) => {
                return Err("connector attachment OPC XML declarations are unsafe".to_string())
            }
            Ok(_) => {}
            Err(_) => return Err("connector attachment OPC XML is invalid".to_string()),
        }
    }
    Ok(root_office_document)
}

type OpcAttributes = (
    Option<String>,
    Option<String>,
    Option<(String, Option<String>, String)>,
);

fn opc_attributes(event: &quick_xml::events::BytesStart<'_>) -> Result<OpcAttributes, String> {
    let mut part = None;
    let mut content_type = None;
    let mut target = None;
    let mut target_mode = None;
    let mut relationship_type = None;
    for attribute in event.attributes().with_checks(true) {
        let attribute =
            attribute.map_err(|_| "connector attachment OPC XML is invalid".to_string())?;
        if attribute.value.contains(&b'&') {
            return Err("connector attachment OPC attribute encoding is unsafe".to_string());
        }
        let key = attribute
            .key
            .as_ref()
            .rsplit(|byte| *byte == b':')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let value = std::str::from_utf8(attribute.value.as_ref())
            .map_err(|_| "connector attachment OPC XML is invalid".to_string())?
            .to_string();
        match key.as_slice() {
            b"partname" => part = Some(value),
            b"contenttype" => content_type = Some(value),
            b"target" => target = Some(value),
            b"targetmode" => target_mode = Some(value),
            b"type" => relationship_type = Some(value),
            _ => {}
        }
    }
    let relationship = match (target, relationship_type) {
        (Some(target), Some(relationship_type)) => Some((target, target_mode, relationship_type)),
        (None, None) => None,
        _ => return Err("connector attachment OPC relationship is invalid".to_string()),
    };
    Ok((part, content_type, relationship))
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use std::io::Cursor;

    use super::*;

    fn metadata(name: &str, media_type: &str, bytes: &[u8]) -> ConnectorAttachmentMetadata {
        ConnectorAttachmentMetadata {
            account_id: Uuid::new_v4(),
            provider_id: "microsoft".to_string(),
            parent_remote_ref: "message-marker".to_string(),
            attachment_remote_ref: "attachment-marker".to_string(),
            file_name: name.to_string(),
            declared_media_type: media_type.to_string(),
            size_bytes: bytes.len() as u64,
            contains_macros: false,
            untrusted_evidence: true,
        }
    }

    fn ticket(
        metadata: &ConnectorAttachmentMetadata,
        generation: u64,
        workspace: &Path,
    ) -> ConnectorAttachmentLandingTicket {
        let fingerprint = connector_attachment_landing_fingerprint(metadata, generation).unwrap();
        let (_, workspace_identity) = connector_attachment_workspace_binding(workspace).unwrap();
        ConnectorAttachmentLandingTicket::from_exact_approval(
            metadata,
            generation,
            &workspace_identity,
            &fingerprint,
            Utc::now() + Duration::minutes(5),
        )
        .unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn workspace_binding_changes_when_landing_directory_is_recreated_at_same_path() {
        let workspace = tempfile::tempdir().expect("workspace");
        let (_, first) = connector_attachment_workspace_binding(workspace.path())
            .expect("first binding is available");
        assert!(first.starts_with("v2:"));
        fs::remove_dir_all(workspace.path().join(LANDING_DIR_NAME))
            .expect("landing directory is removed");
        let (_, second) = connector_attachment_workspace_binding(workspace.path())
            .expect("second binding is available");
        assert_ne!(first, second);
    }

    #[cfg(windows)]
    #[test]
    fn cleanup_without_file_identity_only_succeeds_when_managed_names_are_absent() {
        use crate::kernel::connectors::landing_windows::ManagedLandingRoot;

        let workspace = tempfile::tempdir().expect("workspace");
        let bytes = b"%PDF-1.7\nmissing identity cleanup";
        let metadata = metadata("missing-identity.pdf", "application/pdf", bytes);
        let landing_id = Uuid::new_v4();
        let (_, workspace_identity) = connector_attachment_workspace_binding(workspace.path())
            .expect("workspace binding is available");
        let candidate = ConnectorAttachmentCleanupCandidate {
            landing_id,
            claim_id: Uuid::new_v4(),
            metadata,
            workspace_root: workspace.path().to_path_buf(),
            workspace_identity,
            storage_identity: None,
            receipt: None,
        };

        cleanup_incomplete_connector_attachment(&candidate)
            .expect("no file means there is nothing to repair");

        let managed = ManagedLandingRoot::open(workspace.path()).expect("managed root opens");
        let basename = format!(".{landing_id}.part");
        let staged_file = managed
            .create_staged_file(&basename)
            .expect("unknown staged file is created");
        assert_eq!(
            cleanup_incomplete_connector_attachment(&candidate),
            Err(ConnectorAttachmentCleanupFailure::Unsafe)
        );
        assert!(managed.landing_root().join(&basename).exists());
        drop(staged_file);
    }

    #[test]
    fn safe_landing_streams_to_managed_path_and_returns_path_free_receipt() {
        let workspace = tempfile::tempdir().unwrap();
        let bytes = b"%PDF-1.7\nattachment evidence";
        let metadata = metadata("report.pdf", "application/pdf", bytes);
        let mut ticket = ticket(&metadata, 7, workspace.path());
        let permit = ticket.begin_download(&metadata, 7).unwrap();
        let landed =
            stage_connector_attachment(workspace.path(), &metadata, permit, Cursor::new(bytes))
                .expect("safe attachment stages")
                .commit()
                .expect("safe attachment commits");
        assert!(landed
            .path()
            .starts_with(fs::canonicalize(workspace.path()).unwrap()));
        assert_eq!(fs::read(landed.path()).unwrap(), bytes);
        let receipt = serde_json::to_string(landed.receipt()).unwrap();
        assert!(!receipt.contains(&workspace.path().to_string_lossy().to_string()));
        assert!(!receipt.contains("message-marker"));
        assert!(!receipt.contains("attachment-marker"));
        assert!(landed.receipt().untrusted_evidence);
        assert!(ticket.begin_download(&metadata, 7).is_err());
    }

    #[test]
    fn safe_landing_rejects_path_type_macro_and_late_generation_change() {
        let workspace = tempfile::tempdir().unwrap();
        let bytes = b"%PDF-1.7\nattachment evidence";
        let unsafe_name = metadata("../report.pdf", "application/pdf", bytes);
        assert!(connector_attachment_landing_fingerprint(&unsafe_name, 1).is_err());

        let mismatch = metadata("report.pdf", "application/pdf", b"not a pdf");
        let mut mismatch_ticket = ticket(&mismatch, 1, workspace.path());
        let mismatch_permit = mismatch_ticket.begin_download(&mismatch, 1).unwrap();
        assert!(stage_connector_attachment(
            workspace.path(),
            &mismatch,
            mismatch_permit,
            Cursor::new(b"not a pdf"),
        )
        .is_err());

        let mut macro_metadata = metadata(
            "report.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            b"PK",
        );
        macro_metadata.contains_macros = true;
        assert!(connector_attachment_landing_fingerprint(&macro_metadata, 1).is_err());

        let valid = metadata("report.pdf", "application/pdf", bytes);
        let mut valid_ticket = ticket(&valid, 4, workspace.path());
        let valid_permit = valid_ticket.begin_download(&valid, 4).unwrap();
        let other_workspace = tempfile::tempdir().unwrap();
        assert!(stage_connector_attachment(
            other_workspace.path(),
            &valid,
            valid_permit,
            Cursor::new(bytes),
        )
        .is_err());
        assert!(fs::read_dir(workspace.path().join(LANDING_DIR_NAME))
            .unwrap()
            .next()
            .is_none());
    }

    #[test]
    fn safe_landing_structurally_validates_opc_content_types_and_relationships() {
        fn docx(external: bool, correct_content_type: bool) -> Vec<u8> {
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let options = zip::write::FileOptions::default();
            writer.start_file("[Content_Types].xml", options).unwrap();
            let content_type = if correct_content_type {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
            } else {
                "application/xml"
            };
            write!(
                writer,
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/word/document.xml" ContentType="{content_type}"/></Types>"#
            )
            .unwrap();
            writer.start_file("_rels/.rels", options).unwrap();
            let target_mode = if external {
                r#" TargetMode="External""#
            } else {
                ""
            };
            write!(
                writer,
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"{target_mode}/></Relationships>"#
            )
            .unwrap();
            writer.start_file("word/document.xml", options).unwrap();
            writer.write_all(b"<document/>").unwrap();
            writer.finish().unwrap().into_inner()
        }

        let workspace = tempfile::tempdir().unwrap();
        let valid = docx(false, true);
        let valid_metadata = metadata(
            "report.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            &valid,
        );
        let mut valid_ticket = ticket(&valid_metadata, 1, workspace.path());
        let valid_permit = valid_ticket.begin_download(&valid_metadata, 1).unwrap();
        let landed = stage_connector_attachment(
            workspace.path(),
            &valid_metadata,
            valid_permit,
            Cursor::new(valid),
        )
        .expect("valid OPC package stages")
        .commit()
        .expect("valid OPC package commits");
        assert!(landed.path().is_file());

        for bytes in [docx(true, true), docx(false, false)] {
            let blocked_metadata = metadata(
                "blocked.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                &bytes,
            );
            let mut blocked_ticket = ticket(&blocked_metadata, 2, workspace.path());
            let blocked_permit = blocked_ticket.begin_download(&blocked_metadata, 2).unwrap();
            assert!(stage_connector_attachment(
                workspace.path(),
                &blocked_metadata,
                blocked_permit,
                Cursor::new(bytes),
            )
            .is_err());
        }
    }

    #[test]
    fn safe_landing_rejects_macro_content_inside_office_archive() {
        let workspace = tempfile::tempdir().unwrap();
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::FileOptions::default();
        writer.start_file("word/document.xml", options).unwrap();
        writer.write_all(b"<document/>").unwrap();
        writer.start_file("word/vbaProject.bin", options).unwrap();
        writer.write_all(b"macro-marker").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let metadata = metadata(
            "report.docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            &bytes,
        );
        let mut ticket = ticket(&metadata, 2, workspace.path());
        let permit = ticket.begin_download(&metadata, 2).unwrap();
        assert!(stage_connector_attachment(
            workspace.path(),
            &metadata,
            permit,
            Cursor::new(bytes),
        )
        .is_err());
    }
}
