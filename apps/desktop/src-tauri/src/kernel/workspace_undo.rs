use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::kernel::capability::{
    FileSystemMutationClient, FileSystemMutationOperation, FileSystemMutationResult,
};
use crate::kernel::event_store::EventStore;
use crate::kernel::tool_runtime::ToolExecutionPlan;

pub const MAX_WORKSPACE_UNDO_PREIMAGE_BYTES: u64 = 1024 * 1024;
pub const MAX_WORKSPACE_CHECKPOINT_FILE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMutationCheckpointStatus {
    Intent,
    Prepared,
    EffectStarted,
    Ready,
    NotUndoable,
    UndoStarted,
    Undone,
    Failed,
    RepairRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceUndoCapability {
    None,
    ExactLocal,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceCheckpointEffectState {
    NoEffect,
    KnownApplied,
    EffectUnknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceObjectKind {
    Missing,
    File,
    Directory,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceObjectSnapshot {
    pub kind: WorkspaceObjectKind,
    pub identity: Option<String>,
    pub sha256: Option<String>,
    pub byte_size: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceMutationCheckpoint {
    pub id: Uuid,
    pub tool_invocation_id: Uuid,
    pub run_id: Option<Uuid>,
    pub operation: FileSystemMutationOperation,
    pub source_path: String,
    pub destination_path: Option<String>,
    pub pre_source: Option<WorkspaceObjectSnapshot>,
    pub pre_destination: Option<WorkspaceObjectSnapshot>,
    pub post_target: Option<WorkspaceObjectSnapshot>,
    pub undo_capability: WorkspaceUndoCapability,
    pub effect_state: WorkspaceCheckpointEffectState,
    pub status: WorkspaceMutationCheckpointStatus,
    pub revision: u64,
    pub safe_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkspaceUndoView {
    pub id: Uuid,
    pub tool_invocation_id: Uuid,
    pub run_id: Option<Uuid>,
    pub operation: FileSystemMutationOperation,
    pub status: WorkspaceMutationCheckpointStatus,
    pub effect_state: WorkspaceCheckpointEffectState,
    pub undo_available: bool,
    pub action_revision: Option<String>,
    pub title_code: String,
    pub safe_error_code: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkspaceUndoResult {
    pub checkpoint: WorkspaceUndoView,
    pub acceptance: WorkspaceUndoAcceptance,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceUndoAcceptance {
    Accepted,
    AlreadyAccepted,
}

impl WorkspaceMutationCheckpoint {
    pub(crate) fn intent(
        plan: &ToolExecutionPlan,
        operation: FileSystemMutationOperation,
        path: &str,
        destination: Option<&str>,
    ) -> Result<Self, String> {
        validate_checkpoint_path(path)?;
        if let Some(destination) = destination {
            validate_checkpoint_path(destination)?;
        }
        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            tool_invocation_id: plan.invocation_id,
            run_id: plan.request.run_id,
            operation,
            source_path: path.to_string(),
            destination_path: destination.map(ToString::to_string),
            pre_source: None,
            pre_destination: None,
            post_target: None,
            undo_capability: WorkspaceUndoCapability::None,
            effect_state: WorkspaceCheckpointEffectState::NoEffect,
            status: WorkspaceMutationCheckpointStatus::Intent,
            revision: 0,
            safe_error_code: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub(crate) fn action_revision(&self) -> Option<String> {
        if self.status != WorkspaceMutationCheckpointStatus::Ready
            || self.undo_capability != WorkspaceUndoCapability::ExactLocal
        {
            return None;
        }
        let canonical = serde_json::to_vec(&(
            "ds-agent.workspace-undo.v1",
            self.id,
            self.tool_invocation_id,
            self.operation,
            self.revision,
            &self.post_target,
        ))
        .expect("workspace undo fields are serializable");
        Some(format!("undo1:{}", hex::encode(Sha256::digest(canonical))))
    }

    pub(crate) fn public_view(&self) -> WorkspaceUndoView {
        WorkspaceUndoView {
            id: self.id,
            tool_invocation_id: self.tool_invocation_id,
            run_id: self.run_id,
            operation: self.operation,
            status: self.status,
            effect_state: self.effect_state,
            undo_available: self.action_revision().is_some(),
            action_revision: self.action_revision(),
            title_code: operation_title_code(self.operation).to_string(),
            safe_error_code: self.safe_error_code.clone(),
            updated_at: self.updated_at,
        }
    }
}

pub(crate) fn execute_checkpointed_mutation<T: FileSystemMutationClient + ?Sized>(
    store: &EventStore,
    plan: &ToolExecutionPlan,
    client: &T,
    operation: FileSystemMutationOperation,
    path: &str,
    destination: Option<&str>,
    content: Option<&str>,
) -> Result<FileSystemMutationResult, String> {
    let intent = WorkspaceMutationCheckpoint::intent(plan, operation, path, destination)?;
    store
        .insert_workspace_mutation_intent(&intent)
        .map_err(|error| error.to_string())?;
    let (prepared, preimage) = prepare_checkpoint(intent).inspect_err(|_| {
        let _ = store.fail_workspace_mutation_checkpoint(plan.invocation_id, "precheck_failed");
    })?;
    let prepared = store
        .prepare_workspace_mutation_checkpoint(&prepared, preimage.as_deref())
        .map_err(|error| error.to_string())?;
    if let Err(error) = verify_preconditions(&prepared) {
        let _ =
            store.fail_workspace_mutation_checkpoint(plan.invocation_id, "precondition_changed");
        return Err(error);
    }
    let started = store
        .start_workspace_mutation_effect(&prepared)
        .map_err(|error| error.to_string())?;
    let result = match client.mutate(operation, path, destination, content) {
        Ok(result) => result,
        Err(error) => {
            let _ = store.repair_workspace_mutation_checkpoint(&started, "mutation_failed_unknown");
            return Err(error);
        }
    };
    let completed = complete_checkpoint(started, content).inspect_err(|_| {
        let _ = store.repair_workspace_mutation_checkpoint_by_invocation(
            plan.invocation_id,
            "postcheck_failed_unknown",
        );
    })?;
    store
        .finish_workspace_mutation_checkpoint(&completed)
        .map_err(|error| error.to_string())?;
    Ok(result)
}

pub(crate) fn apply_workspace_undo(
    store: &EventStore,
    checkpoint_id: Uuid,
    action_revision: &str,
) -> Result<WorkspaceUndoResult, String> {
    let claim = store
        .claim_workspace_undo(checkpoint_id, action_revision)
        .map_err(|error| error.to_string())?;
    let Some(mut checkpoint) = claim else {
        let checkpoint = store
            .workspace_mutation_checkpoint(checkpoint_id)
            .map_err(|error| error.to_string())?;
        if checkpoint.status == WorkspaceMutationCheckpointStatus::Undone
            && store
                .workspace_undo_action_was_accepted(checkpoint_id, action_revision)
                .map_err(|error| error.to_string())?
        {
            return Ok(WorkspaceUndoResult {
                checkpoint: checkpoint.public_view(),
                acceptance: WorkspaceUndoAcceptance::AlreadyAccepted,
            });
        }
        return Err("workspace undo action is stale or already consumed".to_string());
    };
    if let Err(error) = apply_exact_undo(
        &checkpoint,
        store
            .workspace_mutation_preimage(checkpoint.id)
            .map_err(|value| value.to_string())?
            .as_deref(),
    ) {
        let _ = store.repair_workspace_mutation_checkpoint(&checkpoint, "undo_failed_unknown");
        return Err(error);
    }
    checkpoint.status = WorkspaceMutationCheckpointStatus::Undone;
    checkpoint.effect_state = WorkspaceCheckpointEffectState::NoEffect;
    checkpoint.revision = checkpoint.revision.saturating_add(1);
    checkpoint.updated_at = Utc::now();
    let checkpoint = store
        .finish_workspace_undo(&checkpoint)
        .map_err(|error| error.to_string())?;
    Ok(WorkspaceUndoResult {
        checkpoint: checkpoint.public_view(),
        acceptance: WorkspaceUndoAcceptance::Accepted,
    })
}

fn prepare_checkpoint(
    mut checkpoint: WorkspaceMutationCheckpoint,
) -> Result<(WorkspaceMutationCheckpoint, Option<Vec<u8>>), String> {
    let capture_preimage = matches!(
        checkpoint.operation,
        FileSystemMutationOperation::UpdateFile | FileSystemMutationOperation::DeleteFile
    );
    let (pre_source, preimage) =
        capture_object(Path::new(&checkpoint.source_path), capture_preimage)?;
    let pre_destination = checkpoint
        .destination_path
        .as_deref()
        .map(|path| capture_object(Path::new(path), false).map(|value| value.0))
        .transpose()?;
    checkpoint.pre_source = Some(pre_source.clone());
    checkpoint.pre_destination = pre_destination.clone();
    checkpoint.undo_capability = undo_capability(
        checkpoint.operation,
        &pre_source,
        pre_destination.as_ref(),
        preimage.as_deref(),
    );
    checkpoint.status = WorkspaceMutationCheckpointStatus::Prepared;
    checkpoint.revision = 1;
    checkpoint.updated_at = Utc::now();
    Ok((checkpoint, preimage))
}

fn complete_checkpoint(
    mut checkpoint: WorkspaceMutationCheckpoint,
    expected_content: Option<&str>,
) -> Result<WorkspaceMutationCheckpoint, String> {
    let target = checkpoint
        .destination_path
        .as_deref()
        .filter(|_| {
            matches!(
                checkpoint.operation,
                FileSystemMutationOperation::RenameFile
                    | FileSystemMutationOperation::RenameDirectory
            )
        })
        .unwrap_or(&checkpoint.source_path);
    let (post_target, _) = capture_object(Path::new(target), false)?;
    validate_postcondition(&checkpoint, &post_target, expected_content)?;
    checkpoint.post_target = Some(post_target);
    checkpoint.effect_state = WorkspaceCheckpointEffectState::KnownApplied;
    checkpoint.status = if checkpoint.undo_capability == WorkspaceUndoCapability::ExactLocal {
        WorkspaceMutationCheckpointStatus::Ready
    } else {
        WorkspaceMutationCheckpointStatus::NotUndoable
    };
    checkpoint.revision = checkpoint.revision.saturating_add(1);
    checkpoint.updated_at = Utc::now();
    Ok(checkpoint)
}

fn verify_preconditions(checkpoint: &WorkspaceMutationCheckpoint) -> Result<(), String> {
    let expected_source = checkpoint
        .pre_source
        .as_ref()
        .ok_or_else(|| "workspace checkpoint has no source precondition".to_string())?;
    let (source, _) = capture_object(Path::new(&checkpoint.source_path), false)?;
    if &source != expected_source {
        return Err("workspace mutation source changed after checkpoint".to_string());
    }
    if let (Some(path), Some(expected)) = (
        checkpoint.destination_path.as_deref(),
        checkpoint.pre_destination.as_ref(),
    ) {
        let (destination, _) = capture_object(Path::new(path), false)?;
        if &destination != expected {
            return Err("workspace mutation destination changed after checkpoint".to_string());
        }
    }
    Ok(())
}

fn validate_postcondition(
    checkpoint: &WorkspaceMutationCheckpoint,
    post_target: &WorkspaceObjectSnapshot,
    expected_content: Option<&str>,
) -> Result<(), String> {
    let expected_kind = match checkpoint.operation {
        FileSystemMutationOperation::CreateFile
        | FileSystemMutationOperation::UpdateFile
        | FileSystemMutationOperation::RenameFile => WorkspaceObjectKind::File,
        FileSystemMutationOperation::CreateDirectory
        | FileSystemMutationOperation::RenameDirectory => WorkspaceObjectKind::Directory,
        FileSystemMutationOperation::DeleteFile | FileSystemMutationOperation::DeleteDirectory => {
            WorkspaceObjectKind::Missing
        }
    };
    if post_target.kind != expected_kind {
        return Err("workspace mutation postcondition did not match the checkpoint".to_string());
    }
    if matches!(
        checkpoint.operation,
        FileSystemMutationOperation::CreateFile | FileSystemMutationOperation::UpdateFile
    ) {
        let expected = expected_content
            .ok_or_else(|| "workspace mutation expected content is unavailable".to_string())?;
        let expected_hash = hex::encode(Sha256::digest(expected.as_bytes()));
        if post_target.byte_size != expected.len() as u64
            || post_target.sha256.as_deref() != Some(expected_hash.as_str())
        {
            return Err("workspace mutation file bytes did not match the request".to_string());
        }
    }
    Ok(())
}

fn undo_capability(
    operation: FileSystemMutationOperation,
    source: &WorkspaceObjectSnapshot,
    destination: Option<&WorkspaceObjectSnapshot>,
    preimage: Option<&[u8]>,
) -> WorkspaceUndoCapability {
    let exact = match operation {
        FileSystemMutationOperation::CreateFile => source.kind == WorkspaceObjectKind::Missing,
        FileSystemMutationOperation::UpdateFile | FileSystemMutationOperation::DeleteFile => {
            source.kind == WorkspaceObjectKind::File && preimage.is_some()
        }
        FileSystemMutationOperation::RenameFile => {
            source.kind == WorkspaceObjectKind::File
                && destination.is_some_and(|value| value.kind == WorkspaceObjectKind::Missing)
        }
        FileSystemMutationOperation::CreateDirectory => source.kind == WorkspaceObjectKind::Missing,
        FileSystemMutationOperation::RenameDirectory => {
            source.kind == WorkspaceObjectKind::Directory
                && destination.is_some_and(|value| value.kind == WorkspaceObjectKind::Missing)
        }
        FileSystemMutationOperation::DeleteDirectory => false,
    };
    if exact {
        WorkspaceUndoCapability::ExactLocal
    } else {
        WorkspaceUndoCapability::None
    }
}

fn apply_exact_undo(
    checkpoint: &WorkspaceMutationCheckpoint,
    preimage: Option<&[u8]>,
) -> Result<(), String> {
    if checkpoint.undo_capability != WorkspaceUndoCapability::ExactLocal {
        return Err("workspace checkpoint has no exact local undo".to_string());
    }
    let post = checkpoint
        .post_target
        .as_ref()
        .ok_or_else(|| "workspace checkpoint has no postcondition".to_string())?;
    let source = Path::new(&checkpoint.source_path);
    match checkpoint.operation {
        FileSystemMutationOperation::CreateFile => delete_exact_object(source, post, false)?,
        FileSystemMutationOperation::UpdateFile => {
            atomic_restore_file(
                source,
                preimage.ok_or_else(|| "workspace undo preimage is unavailable".to_string())?,
                Some(post),
            )?;
        }
        FileSystemMutationOperation::DeleteFile => {
            verify_missing(source)?;
            atomic_restore_file(
                source,
                preimage.ok_or_else(|| "workspace undo preimage is unavailable".to_string())?,
                None,
            )?;
        }
        FileSystemMutationOperation::RenameFile => {
            verify_missing(source)?;
            let destination = checkpoint_destination(checkpoint)?;
            rename_exact_object(destination, source, post)?;
        }
        FileSystemMutationOperation::CreateDirectory => {
            if std::fs::read_dir(source)
                .map_err(|_| "workspace undo directory is unavailable".to_string())?
                .next()
                .is_some()
            {
                return Err("workspace undo refuses to remove a non-empty directory".to_string());
            }
            delete_exact_object(source, post, true)?;
        }
        FileSystemMutationOperation::RenameDirectory => {
            verify_missing(source)?;
            let destination = checkpoint_destination(checkpoint)?;
            rename_exact_object(destination, source, post)?;
        }
        FileSystemMutationOperation::DeleteDirectory => {
            return Err("deleted directories require manual recovery".to_string())
        }
    }
    verify_undo_result(checkpoint, preimage)
}

fn verify_undo_result(
    checkpoint: &WorkspaceMutationCheckpoint,
    preimage: Option<&[u8]>,
) -> Result<(), String> {
    let source = Path::new(&checkpoint.source_path);
    let (current, _) = capture_object(source, false)?;
    let pre = checkpoint
        .pre_source
        .as_ref()
        .ok_or_else(|| "workspace checkpoint precondition is unavailable".to_string())?;
    match checkpoint.operation {
        FileSystemMutationOperation::CreateFile | FileSystemMutationOperation::CreateDirectory => {
            if current.kind != WorkspaceObjectKind::Missing {
                return Err("workspace undo did not remove the created object".to_string());
            }
        }
        FileSystemMutationOperation::UpdateFile | FileSystemMutationOperation::DeleteFile => {
            let bytes =
                preimage.ok_or_else(|| "workspace undo preimage is unavailable".to_string())?;
            let expected_hash = hex::encode(Sha256::digest(bytes));
            if current.kind != WorkspaceObjectKind::File
                || current.byte_size != bytes.len() as u64
                || current.sha256.as_deref() != Some(expected_hash.as_str())
            {
                return Err("workspace undo restored different file bytes".to_string());
            }
        }
        FileSystemMutationOperation::RenameFile | FileSystemMutationOperation::RenameDirectory => {
            if current.kind != pre.kind || current.identity != pre.identity {
                return Err("workspace undo restored a different object identity".to_string());
            }
            verify_missing(checkpoint_destination(checkpoint)?)?;
        }
        FileSystemMutationOperation::DeleteDirectory => unreachable!(),
    }
    Ok(())
}

fn checkpoint_destination(checkpoint: &WorkspaceMutationCheckpoint) -> Result<&Path, String> {
    checkpoint
        .destination_path
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| "workspace checkpoint destination is unavailable".to_string())
}

fn capture_object(
    path: &Path,
    capture_preimage: bool,
) -> Result<(WorkspaceObjectSnapshot, Option<Vec<u8>>), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((
                WorkspaceObjectSnapshot {
                    kind: WorkspaceObjectKind::Missing,
                    identity: None,
                    sha256: None,
                    byte_size: 0,
                },
                None,
            ))
        }
        Err(_) => return Err("workspace checkpoint target is unavailable".to_string()),
    };
    if metadata.file_type().is_symlink() {
        return Err("workspace checkpoint refuses symbolic links".to_string());
    }
    if metadata.is_dir() {
        let identity = object_identity(path, true)?;
        return Ok((
            WorkspaceObjectSnapshot {
                kind: WorkspaceObjectKind::Directory,
                identity: Some(identity),
                sha256: None,
                byte_size: 0,
            },
            None,
        ));
    }
    if !metadata.is_file() {
        return Err("workspace checkpoint target type is unsupported".to_string());
    }
    if metadata.len() > MAX_WORKSPACE_CHECKPOINT_FILE_BYTES {
        return Err("workspace checkpoint file exceeds the bounded safety limit".to_string());
    }
    let mut file = open_object(path, false, false)?;
    let identity = file_identity(&file)?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut captured = capture_preimage.then(Vec::new);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "workspace checkpoint file could not be read".to_string())?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| "workspace checkpoint file is too large".to_string())?;
        if total > MAX_WORKSPACE_CHECKPOINT_FILE_BYTES {
            return Err("workspace checkpoint file exceeds the bounded safety limit".to_string());
        }
        hasher.update(&buffer[..read]);
        if let Some(bytes) = captured.as_mut() {
            if total <= MAX_WORKSPACE_UNDO_PREIMAGE_BYTES {
                bytes.extend_from_slice(&buffer[..read]);
            } else {
                captured = None;
            }
        }
    }
    Ok((
        WorkspaceObjectSnapshot {
            kind: WorkspaceObjectKind::File,
            identity: Some(identity),
            sha256: Some(hex::encode(hasher.finalize())),
            byte_size: total,
        },
        captured,
    ))
}

#[cfg(not(windows))]
fn verify_exact_object(path: &Path, expected: &WorkspaceObjectSnapshot) -> Result<(), String> {
    let (current, _) = capture_object(path, false)?;
    if &current != expected {
        return Err("workspace undo target changed after the original action".to_string());
    }
    Ok(())
}

fn verify_missing(path: &Path) -> Result<(), String> {
    let (current, _) = capture_object(path, false)?;
    if current.kind != WorkspaceObjectKind::Missing {
        return Err("workspace undo destination is no longer empty".to_string());
    }
    Ok(())
}

fn validate_checkpoint_path(value: &str) -> Result<(), String> {
    let path = Path::new(value);
    if value.trim().is_empty() || !path.is_absolute() || path.file_name().is_none() {
        return Err("workspace checkpoint requires an absolute non-root path".to_string());
    }
    Ok(())
}

fn operation_title_code(operation: FileSystemMutationOperation) -> &'static str {
    match operation {
        FileSystemMutationOperation::CreateFile => "created_local_file",
        FileSystemMutationOperation::UpdateFile => "updated_local_file",
        FileSystemMutationOperation::DeleteFile => "deleted_local_file",
        FileSystemMutationOperation::RenameFile => "renamed_local_file",
        FileSystemMutationOperation::CreateDirectory => "created_local_directory",
        FileSystemMutationOperation::RenameDirectory => "renamed_local_directory",
        FileSystemMutationOperation::DeleteDirectory => "deleted_local_directory",
    }
}

fn atomic_restore_file(
    path: &Path,
    bytes: &[u8],
    expected_current: Option<&WorkspaceObjectSnapshot>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "workspace undo target parent is invalid".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|_| "workspace undo target parent is unavailable".to_string())?;
    let temporary = parent.join(format!(".ds-agent-undo-{}.tmp", Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "workspace undo staging file could not be created".to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.flush())
        .and_then(|_| file.sync_all())
        .map_err(|_| "workspace undo staging file could not be committed".to_string())?;
    drop(file);
    let result = if let Some(expected) = expected_current {
        replace_file_atomically_exact(path, &temporary, expected)
    } else {
        land_new_file_atomically(path, &temporary)
    };
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(windows)]
fn replace_file_atomically_exact(
    path: &Path,
    replacement: &Path,
    expected: &WorkspaceObjectSnapshot,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{ReplaceFileW, REPLACEFILE_WRITE_THROUGH};

    let mut guard = open_object_for_replacement(path)?;
    verify_open_snapshot(&mut guard, expected)?;
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replacement = replacement
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        ReplaceFileW(
            PCWSTR(path.as_ptr()),
            PCWSTR(replacement.as_ptr()),
            PCWSTR::null(),
            REPLACEFILE_WRITE_THROUGH,
            None,
            None,
        )
        .map_err(|_| "workspace undo atomic replacement failed".to_string())?;
    }
    drop(guard);
    Ok(())
}

#[cfg(not(windows))]
fn replace_file_atomically_exact(
    path: &Path,
    replacement: &Path,
    expected: &WorkspaceObjectSnapshot,
) -> Result<(), String> {
    verify_exact_object(path, expected)?;
    std::fs::rename(replacement, path)
        .map_err(|_| "workspace undo atomic replacement failed".to_string())
}

#[cfg(windows)]
fn land_new_file_atomically(path: &Path, temporary: &Path) -> Result<(), String> {
    std::fs::rename(temporary, path)
        .map_err(|_| "workspace undo restored file could not be landed".to_string())
}

#[cfg(not(windows))]
fn land_new_file_atomically(path: &Path, temporary: &Path) -> Result<(), String> {
    std::fs::hard_link(temporary, path)
        .map_err(|_| "workspace undo restored file could not be landed".to_string())?;
    std::fs::remove_file(temporary)
        .map_err(|_| "workspace undo staging file could not be retired".to_string())
}

#[cfg(windows)]
fn open_object(path: &Path, directory: bool, delete: bool) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        DELETE, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
        FILE_SHARE_READ,
    };

    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT.0;
    if directory {
        flags |= FILE_FLAG_BACKUP_SEMANTICS.0;
    }
    let mut access = FILE_GENERIC_READ.0;
    if delete {
        access |= DELETE.0;
    }
    let file = OpenOptions::new()
        .read(true)
        .access_mode(access)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(flags)
        .open(path)
        .map_err(|_| "workspace checkpoint object could not be opened".to_string())?;
    reject_reparse(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn open_object_for_replacement(path: &Path) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ,
    };

    let file = OpenOptions::new()
        .read(true)
        .access_mode(FILE_GENERIC_READ.0)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(|_| "workspace undo replacement target could not be opened".to_string())?;
    reject_reparse(&file)?;
    Ok(file)
}

fn verify_open_snapshot(file: &mut File, expected: &WorkspaceObjectSnapshot) -> Result<(), String> {
    if file_identity(file)? != expected.identity.clone().unwrap_or_default() {
        return Err("workspace undo target identity changed".to_string());
    }
    if expected.kind != WorkspaceObjectKind::File {
        return Ok(());
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "workspace undo target could not be verified".to_string())?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "workspace undo target could not be verified".to_string())?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| "workspace undo target is too large".to_string())?;
        if total > MAX_WORKSPACE_CHECKPOINT_FILE_BYTES {
            return Err("workspace undo target exceeds the bounded safety limit".to_string());
        }
        hasher.update(&buffer[..read]);
    }
    if total != expected.byte_size
        || Some(hex::encode(hasher.finalize())).as_ref() != expected.sha256.as_ref()
    {
        return Err("workspace undo target changed after the original action".to_string());
    }
    Ok(())
}

#[cfg(not(windows))]
fn open_object(path: &Path, _directory: bool, _delete: bool) -> Result<File, String> {
    File::open(path).map_err(|_| "workspace checkpoint object could not be opened".to_string())
}

fn object_identity(path: &Path, directory: bool) -> Result<String, String> {
    let file = open_object(path, directory, false)?;
    file_identity(&file)
}

#[cfg(windows)]
fn file_identity(file: &File) -> Result<String, String> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
    };

    let mut info: FILE_ID_INFO = unsafe { zeroed() };
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            u32::try_from(size_of::<FILE_ID_INFO>()).unwrap_or(u32::MAX),
        )
        .map_err(|_| "workspace checkpoint identity is unavailable".to_string())?;
    }
    Ok(format!(
        "v1:{:016x}:{}",
        info.VolumeSerialNumber,
        hex::encode(info.FileId.Identifier)
    ))
}

#[cfg(unix)]
fn file_identity(file: &File) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file
        .metadata()
        .map_err(|_| "workspace checkpoint identity is unavailable".to_string())?;
    Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(all(not(windows), not(unix)))]
fn file_identity(file: &File) -> Result<String, String> {
    let metadata = file
        .metadata()
        .map_err(|_| "workspace checkpoint identity is unavailable".to_string())?;
    Ok(format!(
        "portable:{}:{:?}",
        metadata.len(),
        metadata.modified().ok()
    ))
}

#[cfg(windows)]
fn reject_reparse(file: &File) -> Result<(), String> {
    use std::mem::{size_of, zeroed};
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FileAttributeTagInfo, GetFileInformationByHandleEx, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_ATTRIBUTE_TAG_INFO,
    };

    let mut info: FILE_ATTRIBUTE_TAG_INFO = unsafe { zeroed() };
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(u32::MAX),
        )
        .map_err(|_| "workspace checkpoint attributes are unavailable".to_string())?;
    }
    if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err("workspace checkpoint refuses reparse points".to_string());
    }
    Ok(())
}

#[cfg(windows)]
fn delete_exact_object(
    path: &Path,
    expected: &WorkspaceObjectSnapshot,
    directory: bool,
) -> Result<(), String> {
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FileDispositionInfoEx, SetFileInformationByHandle, FILE_DISPOSITION_FLAG_DELETE,
        FILE_DISPOSITION_INFO_EX,
    };

    let mut file = open_object(path, directory, true)?;
    verify_open_snapshot(&mut file, expected)?;
    let disposition = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE,
    };
    unsafe {
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            FileDispositionInfoEx,
            (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
            u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>()).unwrap_or(u32::MAX),
        )
        .map_err(|_| "workspace undo exact deletion failed".to_string())?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn delete_exact_object(
    path: &Path,
    expected: &WorkspaceObjectSnapshot,
    directory: bool,
) -> Result<(), String> {
    verify_exact_object(path, expected)?;
    if directory {
        std::fs::remove_dir(path)
    } else {
        std::fs::remove_file(path)
    }
    .map_err(|_| "workspace undo exact deletion failed".to_string())
}

#[cfg(windows)]
fn rename_exact_object(
    source: &Path,
    destination: &Path,
    expected: &WorkspaceObjectSnapshot,
) -> Result<(), String> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FileRenameInfo, SetFileInformationByHandle, FILE_RENAME_INFO,
    };

    let directory = expected.kind == WorkspaceObjectKind::Directory;
    let file = open_object(source, directory, true)?;
    if file_identity(&file)? != expected.identity.clone().unwrap_or_default() {
        return Err("workspace undo rename source identity changed".to_string());
    }
    let wide = destination.as_os_str().encode_wide().collect::<Vec<_>>();
    let byte_len = wide
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| "workspace undo destination is invalid".to_string())?;
    let allocation_size = size_of::<FILE_RENAME_INFO>()
        .checked_add(byte_len.saturating_sub(size_of::<u16>()))
        .ok_or_else(|| "workspace undo destination is invalid".to_string())?;
    let mut storage = vec![0u64; allocation_size.div_ceil(size_of::<u64>())];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = HANDLE::default();
        (*info).FileNameLength = u32::try_from(byte_len)
            .map_err(|_| "workspace undo destination is invalid".to_string())?;
        std::ptr::copy_nonoverlapping(wide.as_ptr(), (*info).FileName.as_mut_ptr(), wide.len());
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            FileRenameInfo,
            info.cast(),
            u32::try_from(allocation_size)
                .map_err(|_| "workspace undo destination is invalid".to_string())?,
        )
        .map_err(|_| "workspace undo exact rename failed".to_string())?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn rename_exact_object(
    source: &Path,
    destination: &Path,
    expected: &WorkspaceObjectSnapshot,
) -> Result<(), String> {
    verify_exact_object(source, expected)?;
    std::fs::rename(source, destination)
        .map_err(|_| "workspace undo exact rename failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        apply_workspace_undo, execute_checkpointed_mutation, WorkspaceMutationCheckpointStatus,
        WorkspaceUndoAcceptance,
    };
    use crate::kernel::capability::{FileSystemMutationOperation, LocalFileSystemMutationClient};
    use crate::kernel::event_store::EventStore;
    use crate::kernel::models::AccessMode;
    use crate::kernel::tool_runtime::{
        prepare_tool_execution, ToolExecutionRequest, FILESYSTEM_MUTATE_TOOL_ID,
    };

    fn plan_with(
        operation: &str,
        path: &std::path::Path,
        destination: Option<&std::path::Path>,
        content: Option<&str>,
    ) -> crate::kernel::tool_runtime::ToolExecutionPlan {
        prepare_tool_execution(&ToolExecutionRequest {
            tool_id: FILESYSTEM_MUTATE_TOOL_ID.to_string(),
            input: serde_json::json!({
                "operation": operation,
                "path": path.to_string_lossy(),
                "destination": destination.map(|value| value.to_string_lossy().to_string()),
                "content": content,
                "summary": "test"
            }),
            access_mode: AccessMode::FullAccess,
            run_id: None,
        })
        .unwrap()
    }

    fn plan(
        operation: &str,
        path: &std::path::Path,
    ) -> crate::kernel::tool_runtime::ToolExecutionPlan {
        plan_with(operation, path, None, Some("after"))
    }

    #[test]
    fn update_file_checkpoint_restores_exact_preimage_once() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("report.txt");
        std::fs::write(&path, b"before").unwrap();
        let store = EventStore::open(root.path().join("events.sqlite3")).unwrap();
        let plan = plan("update_file", &path);
        execute_checkpointed_mutation(
            &store,
            &plan,
            &LocalFileSystemMutationClient,
            FileSystemMutationOperation::UpdateFile,
            path.to_str().unwrap(),
            None,
            Some("after"),
        )
        .unwrap();
        let checkpoint = store
            .workspace_mutation_checkpoint_for_invocation(plan.invocation_id)
            .unwrap();
        assert_eq!(checkpoint.status, WorkspaceMutationCheckpointStatus::Ready);
        let action = checkpoint.action_revision().unwrap();
        let undone = apply_workspace_undo(&store, checkpoint.id, &action).unwrap();
        assert_eq!(undone.acceptance, WorkspaceUndoAcceptance::Accepted);
        assert_eq!(std::fs::read(&path).unwrap(), b"before");
        let replay = apply_workspace_undo(&store, checkpoint.id, &action).unwrap();
        assert_eq!(replay.acceptance, WorkspaceUndoAcceptance::AlreadyAccepted);
        assert!(apply_workspace_undo(&store, checkpoint.id, "undo1:wrong").is_err());
    }

    #[test]
    fn undo_rejects_same_content_file_identity_replacement() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("report.txt");
        std::fs::write(&path, b"before").unwrap();
        let store = EventStore::open(root.path().join("events.sqlite3")).unwrap();
        let plan = plan("update_file", &path);
        execute_checkpointed_mutation(
            &store,
            &plan,
            &LocalFileSystemMutationClient,
            FileSystemMutationOperation::UpdateFile,
            path.to_str().unwrap(),
            None,
            Some("after"),
        )
        .unwrap();
        let checkpoint = store
            .workspace_mutation_checkpoint_for_invocation(plan.invocation_id)
            .unwrap();
        let action = checkpoint.action_revision().unwrap();
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"after").unwrap();
        assert!(apply_workspace_undo(&store, checkpoint.id, &action).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"after");
        assert_eq!(
            store
                .workspace_mutation_checkpoint(checkpoint.id)
                .unwrap()
                .status,
            WorkspaceMutationCheckpointStatus::RepairRequired
        );
    }

    #[test]
    fn undo_rejects_in_place_changes_to_the_original_file_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("report.txt");
        std::fs::write(&path, b"before").unwrap();
        let store = EventStore::open(root.path().join("events.sqlite3")).unwrap();
        let plan = plan("update_file", &path);
        execute_checkpointed_mutation(
            &store,
            &plan,
            &LocalFileSystemMutationClient,
            FileSystemMutationOperation::UpdateFile,
            path.to_str().unwrap(),
            None,
            Some("after"),
        )
        .unwrap();
        let checkpoint = store
            .workspace_mutation_checkpoint_for_invocation(plan.invocation_id)
            .unwrap();
        let action = checkpoint.action_revision().unwrap();
        std::fs::write(&path, b"new user edit").unwrap();
        assert!(apply_workspace_undo(&store, checkpoint.id, &action).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"new user edit");
    }

    #[test]
    fn create_and_delete_file_checkpoints_have_exact_inverse_actions() {
        let root = tempfile::tempdir().unwrap();
        let store = EventStore::open(root.path().join("events.sqlite3")).unwrap();
        let created_path = root.path().join("created.txt");
        let create_plan = plan_with("create_file", &created_path, None, Some("created"));
        execute_checkpointed_mutation(
            &store,
            &create_plan,
            &LocalFileSystemMutationClient,
            FileSystemMutationOperation::CreateFile,
            created_path.to_str().unwrap(),
            None,
            Some("created"),
        )
        .unwrap();
        let created = store
            .workspace_mutation_checkpoint_for_invocation(create_plan.invocation_id)
            .unwrap();
        apply_workspace_undo(&store, created.id, &created.action_revision().unwrap()).unwrap();
        assert!(!created_path.exists());

        let deleted_path = root.path().join("deleted.txt");
        std::fs::write(&deleted_path, b"restore me").unwrap();
        let delete_plan = plan_with("delete_file", &deleted_path, None, None);
        execute_checkpointed_mutation(
            &store,
            &delete_plan,
            &LocalFileSystemMutationClient,
            FileSystemMutationOperation::DeleteFile,
            deleted_path.to_str().unwrap(),
            None,
            None,
        )
        .unwrap();
        let deleted = store
            .workspace_mutation_checkpoint_for_invocation(delete_plan.invocation_id)
            .unwrap();
        apply_workspace_undo(&store, deleted.id, &deleted.action_revision().unwrap()).unwrap();
        assert_eq!(std::fs::read(&deleted_path).unwrap(), b"restore me");
    }

    #[test]
    fn rename_checkpoint_restores_the_same_file_identity() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("before.txt");
        let destination = root.path().join("after.txt");
        std::fs::write(&source, b"same object").unwrap();
        let store = EventStore::open(root.path().join("events.sqlite3")).unwrap();
        let plan = plan_with("rename_file", &source, Some(&destination), None);
        execute_checkpointed_mutation(
            &store,
            &plan,
            &LocalFileSystemMutationClient,
            FileSystemMutationOperation::RenameFile,
            source.to_str().unwrap(),
            Some(destination.to_str().unwrap()),
            None,
        )
        .unwrap();
        let checkpoint = store
            .workspace_mutation_checkpoint_for_invocation(plan.invocation_id)
            .unwrap();
        apply_workspace_undo(
            &store,
            checkpoint.id,
            &checkpoint.action_revision().unwrap(),
        )
        .unwrap();
        assert_eq!(std::fs::read(&source).unwrap(), b"same object");
        assert!(!destination.exists());
    }

    #[test]
    fn oversized_file_preimage_is_truthfully_not_undoable() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("large.bin");
        std::fs::write(
            &path,
            vec![b'x'; (super::MAX_WORKSPACE_UNDO_PREIMAGE_BYTES + 1) as usize],
        )
        .unwrap();
        let store = EventStore::open(root.path().join("events.sqlite3")).unwrap();
        let plan = plan("update_file", &path);
        execute_checkpointed_mutation(
            &store,
            &plan,
            &LocalFileSystemMutationClient,
            FileSystemMutationOperation::UpdateFile,
            path.to_str().unwrap(),
            None,
            Some("after"),
        )
        .unwrap();
        let checkpoint = store
            .workspace_mutation_checkpoint_for_invocation(plan.invocation_id)
            .unwrap();
        assert_eq!(
            checkpoint.status,
            WorkspaceMutationCheckpointStatus::NotUndoable
        );
        assert!(checkpoint.action_revision().is_none());
    }
}
