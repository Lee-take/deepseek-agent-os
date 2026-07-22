use std::fs::{self, File, OpenOptions};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, RawHandle};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{
    FileAttributeTagInfo, FileDispositionInfoEx, FileIdInfo, FileRenameInfo,
    GetFileInformationByHandleEx, SetFileInformationByHandle, DELETE, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_INFO_EX,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
    FILE_GENERIC_WRITE, FILE_ID_INFO, FILE_RENAME_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE,
};

const LANDING_DIR_NAME: &str = "connector-downloads";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StableFileIdentity {
    volume_serial: u64,
    file_id: [u8; 16],
}

impl StableFileIdentity {
    pub(crate) fn encoded(self) -> String {
        format!("{:016x}:{}", self.volume_serial, hex::encode(self.file_id))
    }
}

pub(crate) struct ManagedLandingRoot {
    canonical_workspace: PathBuf,
    landing_root: PathBuf,
    workspace_handle: File,
    landing_handle: File,
    workspace_identity: StableFileIdentity,
    landing_identity: StableFileIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IdentityDeleteResult {
    Missing,
    Deleted,
    IdentityMismatch,
}

pub(crate) enum IdentityOpenResult {
    Missing,
    Opened(File),
    IdentityMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManagedFilePresence {
    Missing,
    Present,
}

impl ManagedLandingRoot {
    pub(crate) fn open(workspace_root: &Path) -> Result<Self, String> {
        crate::kernel::sandbox::enforce_local_mutation_path(workspace_root)?;
        let canonical_workspace = fs::canonicalize(workspace_root)
            .map_err(|_| "connector attachment workspace is unavailable".to_string())?;
        if !canonical_workspace.is_dir() {
            return Err("connector attachment workspace is unavailable".to_string());
        }
        let landing_root = canonical_workspace.join(LANDING_DIR_NAME);
        crate::kernel::sandbox::enforce_local_mutation_path(&landing_root)?;
        fs::create_dir_all(&landing_root)
            .map_err(|_| "connector attachment landing directory is unavailable".to_string())?;

        let workspace_handle = open_directory_no_reparse(&canonical_workspace)?;
        let landing_handle = open_directory_no_reparse(&landing_root)?;
        let workspace_identity = stable_file_identity(&workspace_handle)?;
        let landing_identity = stable_file_identity(&landing_handle)?;
        let canonical_landing = fs::canonicalize(&landing_root)
            .map_err(|_| "connector attachment landing directory is unavailable".to_string())?;
        if !canonical_landing.starts_with(&canonical_workspace) {
            return Err("connector attachment landing directory is unsafe".to_string());
        }
        Ok(Self {
            canonical_workspace,
            landing_root: canonical_landing,
            workspace_handle,
            landing_handle,
            workspace_identity,
            landing_identity,
        })
    }

    pub(crate) fn workspace_root(&self) -> &Path {
        &self.canonical_workspace
    }

    pub(crate) fn landing_root(&self) -> &Path {
        &self.landing_root
    }

    pub(crate) fn binding(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"ds-agent.connector-attachment-workspace.v2\0");
        let path = self.canonical_workspace.to_string_lossy();
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path.as_bytes());
        digest.update(self.workspace_identity.volume_serial.to_be_bytes());
        digest.update(self.workspace_identity.file_id);
        digest.update(self.landing_identity.volume_serial.to_be_bytes());
        digest.update(self.landing_identity.file_id);
        format!("v2:{:x}", digest.finalize())
    }

    pub(crate) fn create_staged_file(&self, basename: &str) -> Result<File, String> {
        if basename.is_empty()
            || basename.contains(['/', '\\', ':'])
            || Path::new(basename)
                .file_name()
                .and_then(|value| value.to_str())
                != Some(basename)
        {
            return Err("connector attachment staging name is unsafe".to_string());
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .access_mode(FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | DELETE.0)
            .share_mode(FILE_SHARE_READ.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(self.landing_root.join(basename))
            .map_err(|_| "connector attachment staging failed".to_string())?;
        reject_reparse(&file)?;
        Ok(file)
    }

    pub(crate) fn rename_staged_file(
        &self,
        file: &File,
        destination_basename: &str,
    ) -> Result<PathBuf, String> {
        if destination_basename.is_empty()
            || destination_basename.contains(['/', '\\', ':'])
            || Path::new(destination_basename)
                .file_name()
                .and_then(|value| value.to_str())
                != Some(destination_basename)
        {
            return Err("connector attachment destination name is unsafe".to_string());
        }
        let destination = self.landing_root.join(destination_basename);
        let wide = destination.as_os_str().encode_wide().collect::<Vec<_>>();
        let byte_len = wide
            .len()
            .checked_mul(size_of::<u16>())
            .ok_or_else(|| "connector attachment destination name is unsafe".to_string())?;
        let allocation_size = size_of::<FILE_RENAME_INFO>()
            .checked_add(byte_len.saturating_sub(size_of::<u16>()))
            .ok_or_else(|| "connector attachment destination name is unsafe".to_string())?;
        let word_count = allocation_size.div_ceil(size_of::<u64>());
        let mut storage = vec![0u64; word_count];
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        unsafe {
            (*info).Anonymous.ReplaceIfExists = false;
            (*info).RootDirectory = HANDLE::default();
            (*info).FileNameLength = u32::try_from(byte_len)
                .map_err(|_| "connector attachment destination name is unsafe".to_string())?;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), (*info).FileName.as_mut_ptr(), wide.len());
            SetFileInformationByHandle(
                file_handle(file),
                FileRenameInfo,
                info.cast(),
                u32::try_from(allocation_size)
                    .map_err(|_| "connector attachment destination name is unsafe".to_string())?,
            )
            .map_err(|error| format!("connector attachment commit failed ({error})"))?;
        }
        Ok(destination)
    }

    pub(crate) fn file_identity(&self, file: &File) -> Result<StableFileIdentity, String> {
        stable_file_identity(file)
    }

    pub(crate) fn file_presence_no_reparse(
        &self,
        basename: &str,
    ) -> Result<ManagedFilePresence, String> {
        if basename.is_empty()
            || basename.contains(['/', '\\', ':'])
            || Path::new(basename)
                .file_name()
                .and_then(|value| value.to_str())
                != Some(basename)
        {
            return Err("connector attachment recovery name is unsafe".to_string());
        }
        let path = self.landing_root.join(basename);
        match OpenOptions::new()
            .read(true)
            .access_mode(FILE_GENERIC_READ.0)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(&path)
        {
            Ok(file) => {
                reject_reparse(&file)?;
                Ok(ManagedFilePresence::Present)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ManagedFilePresence::Missing)
            }
            Err(_) => match fs::symlink_metadata(path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                    Ok(ManagedFilePresence::Present)
                }
                Ok(_) => Err("connector attachment recovery file is unavailable".to_string()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(ManagedFilePresence::Missing)
                }
                Err(_) => Err("connector attachment recovery file is unavailable".to_string()),
            },
        }
    }

    pub(crate) fn delete_file_if_identity(
        &self,
        basename: &str,
        expected_identity: &str,
    ) -> Result<IdentityDeleteResult, String> {
        if basename.is_empty()
            || basename.contains(['/', '\\', ':'])
            || Path::new(basename)
                .file_name()
                .and_then(|value| value.to_str())
                != Some(basename)
        {
            return Err("connector attachment cleanup name is unsafe".to_string());
        }
        let file = match OpenOptions::new()
            .read(true)
            .access_mode(FILE_GENERIC_READ.0 | DELETE.0)
            .share_mode(FILE_SHARE_READ.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(self.landing_root.join(basename))
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(IdentityDeleteResult::Missing)
            }
            Err(_) => return Err("connector attachment cleanup file is unavailable".to_string()),
        };
        reject_reparse(&file)?;
        if stable_file_identity(&file)?.encoded() != expected_identity {
            return Ok(IdentityDeleteResult::IdentityMismatch);
        }
        let disposition = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE,
        };
        unsafe {
            SetFileInformationByHandle(
                file_handle(&file),
                FileDispositionInfoEx,
                (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
                u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>()).unwrap_or(u32::MAX),
            )
            .map_err(|_| "connector attachment cleanup delete failed".to_string())?;
        }
        drop(file);
        Ok(IdentityDeleteResult::Deleted)
    }

    pub(crate) fn open_file_if_identity(
        &self,
        basename: &str,
        expected_identity: &str,
    ) -> Result<IdentityOpenResult, String> {
        if basename.is_empty()
            || basename.contains(['/', '\\', ':'])
            || Path::new(basename)
                .file_name()
                .and_then(|value| value.to_str())
                != Some(basename)
        {
            return Err("connector attachment recovery name is unsafe".to_string());
        }
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .access_mode(FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | DELETE.0)
            .share_mode(FILE_SHARE_READ.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(self.landing_root.join(basename))
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(IdentityOpenResult::Missing)
            }
            Err(_) => return Err("connector attachment recovery file is unavailable".to_string()),
        };
        reject_reparse(&file)?;
        if stable_file_identity(&file)?.encoded() != expected_identity {
            return Ok(IdentityOpenResult::IdentityMismatch);
        }
        Ok(IdentityOpenResult::Opened(file))
    }

    pub(crate) fn handles_are_live(&self) -> bool {
        !self.workspace_handle.as_raw_handle().is_null()
            && !self.landing_handle.as_raw_handle().is_null()
    }
}

fn open_directory_no_reparse(path: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
        .map_err(|_| "connector attachment directory handle is unavailable".to_string())?;
    reject_reparse(&file)?;
    Ok(file)
}

fn reject_reparse(file: &File) -> Result<(), String> {
    let mut info: FILE_ATTRIBUTE_TAG_INFO = unsafe { zeroed() };
    unsafe {
        GetFileInformationByHandleEx(
            file_handle(file),
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(u32::MAX),
        )
        .map_err(|_| "connector attachment file identity is unavailable".to_string())?;
    }
    if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err("connector attachment path is an unsafe reparse point".to_string());
    }
    Ok(())
}

fn stable_file_identity(file: &File) -> Result<StableFileIdentity, String> {
    let mut info: FILE_ID_INFO = unsafe { zeroed() };
    unsafe {
        GetFileInformationByHandleEx(
            file_handle(file),
            FileIdInfo,
            (&mut info as *mut FILE_ID_INFO).cast(),
            u32::try_from(size_of::<FILE_ID_INFO>()).unwrap_or(u32::MAX),
        )
        .map_err(|_| "connector attachment stable file identity is unavailable".to_string())?;
    }
    Ok(StableFileIdentity {
        volume_serial: info.VolumeSerialNumber,
        file_id: info.FileId.Identifier,
    })
}

fn file_handle(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle() as RawHandle)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn held_root_and_file_identity_anchor_rename_and_exact_delete() {
        let workspace = tempfile::tempdir().expect("workspace");
        let managed = ManagedLandingRoot::open(workspace.path()).expect("managed root opens");
        assert!(fs::rename(
            managed.landing_root(),
            workspace.path().join("replaced-landing")
        )
        .is_err());

        let mut file = managed
            .create_staged_file(".identity.part")
            .expect("staged file opens");
        file.write_all(b"stable identity").expect("bytes write");
        file.sync_all().expect("bytes sync");
        let identity = managed
            .file_identity(&file)
            .expect("file identity loads")
            .encoded();
        let final_path = managed
            .rename_staged_file(&file, "identity.txt")
            .expect("handle rename succeeds");
        assert!(final_path.is_file());
        assert_eq!(
            managed
                .file_identity(&file)
                .expect("identity remains stable")
                .encoded(),
            identity
        );
        drop(file);
        assert_eq!(
            managed
                .delete_file_if_identity("identity.txt", "0000000000000000:00")
                .expect("mismatch is reported"),
            IdentityDeleteResult::IdentityMismatch
        );
        assert!(final_path.exists());
        assert_eq!(
            managed
                .delete_file_if_identity("identity.txt", &identity)
                .expect("exact identity deletes"),
            IdentityDeleteResult::Deleted
        );
        assert!(!final_path.exists());
    }
}
