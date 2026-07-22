use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) const APP_UPDATE_MAX_BYTES: u64 = 256 * 1024 * 1024;
const RECEIPT_DATABASE_NAME: &str = "update-receipts.sqlite3";
const RECEIPT_TOKEN_PREFIX: &str = "dsur1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LandedAppUpdate {
    pub download_receipt: String,
    pub sha256: String,
    pub byte_size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ScheduledAppUpdate {
    pub restart_scheduled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReceiptStatus {
    Intent,
    Downloading,
    Staged,
    Ready,
    InstallPending,
    InstallScheduled,
    Failed,
    RepairRequired,
}

impl ReceiptStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Downloading => "downloading",
            Self::Staged => "staged",
            Self::Ready => "ready",
            Self::InstallPending => "install_pending",
            Self::InstallScheduled => "install_scheduled",
            Self::Failed => "failed",
            Self::RepairRequired => "repair_required",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "intent" => Ok(Self::Intent),
            "downloading" => Ok(Self::Downloading),
            "staged" => Ok(Self::Staged),
            "ready" => Ok(Self::Ready),
            "install_pending" => Ok(Self::InstallPending),
            "install_scheduled" => Ok(Self::InstallScheduled),
            "failed" => Ok(Self::Failed),
            "repair_required" => Ok(Self::RepairRequired),
            _ => Err("update receipt has an unsupported state".to_string()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReceiptRecord {
    id: Uuid,
    revision: u64,
    latest_version: String,
    asset_name: String,
    part_name: String,
    final_name: String,
    status: ReceiptStatus,
    file_identity: Option<String>,
    sha256: Option<String>,
    byte_size: Option<u64>,
    install_claim: Option<Uuid>,
}

struct ReceiptStore {
    connection: Connection,
}

impl ReceiptStore {
    fn open(root: &Path) -> Result<Self, String> {
        let database_path = root.join(RECEIPT_DATABASE_NAME);
        validate_receipt_store_paths(&database_path)?;
        let connection = Connection::open(&database_path)
            .map_err(|_| "update receipt store is unavailable".to_string())?;
        connection
            .execute_batch(
                r#"
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = FULL;
                CREATE TABLE IF NOT EXISTS app_update_receipts (
                    id TEXT PRIMARY KEY NOT NULL,
                    revision INTEGER NOT NULL,
                    latest_version TEXT NOT NULL,
                    asset_name TEXT NOT NULL,
                    part_name TEXT NOT NULL,
                    final_name TEXT NOT NULL,
                    status TEXT NOT NULL,
                    file_identity TEXT,
                    sha256 TEXT,
                    byte_size INTEGER,
                    install_claim TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_app_update_receipts_status
                    ON app_update_receipts (status, updated_at);
                "#,
            )
            .map_err(|_| "update receipt store could not be initialized".to_string())?;
        validate_receipt_store_paths(&database_path)?;
        Ok(Self { connection })
    }

    fn begin(&self, latest_version: &str, asset_name: &str) -> Result<ReceiptRecord, String> {
        validate_public_label(latest_version, "update version")?;
        validate_asset_name(asset_name)?;
        let id = Uuid::new_v4();
        let safe_name = safe_asset_name(asset_name);
        let part_name = format!(".{}-{safe_name}.part", id.simple());
        let final_name = format!("{}-{safe_name}", id.simple());
        let now = Utc::now().to_rfc3339();
        self.connection
            .execute(
                r#"INSERT INTO app_update_receipts
                   (id, revision, latest_version, asset_name, part_name, final_name,
                    status, created_at, updated_at)
                   VALUES (?1, 0, ?2, ?3, ?4, ?5, 'intent', ?6, ?6)"#,
                params![
                    id.to_string(),
                    latest_version,
                    asset_name,
                    part_name,
                    final_name,
                    now
                ],
            )
            .map_err(|_| "update receipt intent could not be recorded".to_string())?;
        self.load(id)
    }

    fn mark_downloading(
        &self,
        record: &ReceiptRecord,
        file_identity: &str,
    ) -> Result<ReceiptRecord, String> {
        self.transition(
            record,
            ReceiptStatus::Intent,
            ReceiptStatus::Downloading,
            Some(file_identity),
            None,
            None,
            None,
        )
    }

    fn mark_staged(
        &self,
        record: &ReceiptRecord,
        sha256: &str,
        byte_size: u64,
    ) -> Result<ReceiptRecord, String> {
        validate_sha256(sha256)?;
        if byte_size == 0 || byte_size > APP_UPDATE_MAX_BYTES {
            return Err("update receipt byte size is invalid".to_string());
        }
        self.transition(
            record,
            ReceiptStatus::Downloading,
            ReceiptStatus::Staged,
            record.file_identity.as_deref(),
            Some(sha256),
            Some(byte_size),
            None,
        )
    }

    fn mark_ready(&self, record: &ReceiptRecord) -> Result<ReceiptRecord, String> {
        if record.file_identity.is_none() || record.sha256.is_none() || record.byte_size.is_none() {
            return Err("staged update receipt is incomplete".to_string());
        }
        self.transition(
            record,
            ReceiptStatus::Staged,
            ReceiptStatus::Ready,
            record.file_identity.as_deref(),
            record.sha256.as_deref(),
            record.byte_size,
            None,
        )
    }

    fn claim_install(&self, token: &str) -> Result<ReceiptRecord, String> {
        let (id, revision) = parse_receipt_token(token)?;
        let claim = Uuid::new_v4();
        let changed = self
            .connection
            .execute(
                r#"UPDATE app_update_receipts
                   SET status = 'install_pending', revision = revision + 1,
                       install_claim = ?3, updated_at = ?4
                   WHERE id = ?1 AND revision = ?2 AND status = 'ready'
                     AND file_identity IS NOT NULL AND sha256 IS NOT NULL
                     AND byte_size IS NOT NULL AND install_claim IS NULL"#,
                params![
                    id.to_string(),
                    revision,
                    claim.to_string(),
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|_| "update install approval could not be consumed".to_string())?;
        if changed != 1 {
            return Err("update download receipt is stale or already consumed".to_string());
        }
        self.load(id)
    }

    fn mark_install_scheduled(&self, record: &ReceiptRecord) -> Result<ReceiptRecord, String> {
        self.transition(
            record,
            ReceiptStatus::InstallPending,
            ReceiptStatus::InstallScheduled,
            record.file_identity.as_deref(),
            record.sha256.as_deref(),
            record.byte_size,
            record.install_claim,
        )
    }

    fn mark_failed(&self, record: &ReceiptRecord) -> Result<(), String> {
        self.force_terminal(record, ReceiptStatus::Failed)
    }

    fn mark_repair_required(&self, record: &ReceiptRecord) -> Result<(), String> {
        self.force_terminal(record, ReceiptStatus::RepairRequired)
    }

    fn force_terminal(&self, record: &ReceiptRecord, status: ReceiptStatus) -> Result<(), String> {
        let changed = self
            .connection
            .execute(
                r#"UPDATE app_update_receipts
                   SET status = ?3, revision = revision + 1, updated_at = ?4
                   WHERE id = ?1 AND revision = ?2"#,
                params![
                    record.id.to_string(),
                    record.revision,
                    status.as_str(),
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|_| "update receipt terminal state could not be recorded".to_string())?;
        if changed != 1 {
            return Err("update receipt changed concurrently".to_string());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn transition(
        &self,
        record: &ReceiptRecord,
        expected: ReceiptStatus,
        next: ReceiptStatus,
        file_identity: Option<&str>,
        sha256: Option<&str>,
        byte_size: Option<u64>,
        install_claim: Option<Uuid>,
    ) -> Result<ReceiptRecord, String> {
        let changed = self
            .connection
            .execute(
                r#"UPDATE app_update_receipts
                   SET status = ?4, revision = revision + 1, file_identity = ?5,
                       sha256 = ?6, byte_size = ?7, install_claim = ?8,
                       updated_at = ?9
                   WHERE id = ?1 AND revision = ?2 AND status = ?3"#,
                params![
                    record.id.to_string(),
                    record.revision,
                    expected.as_str(),
                    next.as_str(),
                    file_identity,
                    sha256,
                    byte_size,
                    install_claim.map(|value| value.to_string()),
                    Utc::now().to_rfc3339()
                ],
            )
            .map_err(|_| "update receipt transition could not be recorded".to_string())?;
        if changed != 1 {
            return Err("update receipt changed concurrently".to_string());
        }
        self.load(record.id)
    }

    fn load(&self, id: Uuid) -> Result<ReceiptRecord, String> {
        self.connection
            .query_row(
                r#"SELECT id, revision, latest_version, asset_name, part_name,
                          final_name, status, file_identity, sha256, byte_size,
                          install_claim
                   FROM app_update_receipts WHERE id = ?1"#,
                params![id.to_string()],
                receipt_from_row,
            )
            .optional()
            .map_err(|_| "update receipt could not be loaded".to_string())?
            .ok_or_else(|| "update receipt does not exist".to_string())
    }

    fn recoverable(&self) -> Result<Vec<ReceiptRecord>, String> {
        let mut statement = self
            .connection
            .prepare(
                r#"SELECT id, revision, latest_version, asset_name, part_name,
                          final_name, status, file_identity, sha256, byte_size,
                          install_claim
                   FROM app_update_receipts
                   WHERE status IN ('intent', 'downloading', 'staged', 'install_pending')
                   ORDER BY updated_at ASC"#,
            )
            .map_err(|_| "update recovery state could not be read".to_string())?;
        let records = statement
            .query_map([], receipt_from_row)
            .map_err(|_| "update recovery state could not be read".to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "update recovery state is invalid".to_string())?;
        Ok(records)
    }
}

fn validate_receipt_store_paths(database_path: &Path) -> Result<(), String> {
    let database_name = database_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "update receipt store path is invalid".to_string())?;
    let parent = database_path
        .parent()
        .ok_or_else(|| "update receipt store path is invalid".to_string())?;
    for name in [
        database_name.to_string(),
        format!("{database_name}-wal"),
        format!("{database_name}-shm"),
    ] {
        let path = parent.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if receipt_store_metadata_is_safe(&metadata) => {}
            Ok(_) => return Err("update receipt store path is unsafe".to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("update receipt store path is unavailable".to_string()),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn receipt_store_metadata_is_safe(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT_VALUE: u32 = 0x0000_0400;
    metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT_VALUE == 0
}

#[cfg(not(windows))]
fn receipt_store_metadata_is_safe(metadata: &std::fs::Metadata) -> bool {
    metadata.is_file() && !metadata.file_type().is_symlink()
}

fn receipt_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReceiptRecord> {
    let id: String = row.get(0)?;
    let revision: u64 = row.get(1)?;
    let status: String = row.get(6)?;
    let install_claim: Option<String> = row.get(10)?;
    Ok(ReceiptRecord {
        id: Uuid::parse_str(&id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        revision,
        latest_version: row.get(2)?,
        asset_name: row.get(3)?,
        part_name: row.get(4)?,
        final_name: row.get(5)?,
        status: ReceiptStatus::parse(&status).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::other(error)),
            )
        })?,
        file_identity: row.get(7)?,
        sha256: row.get(8)?,
        byte_size: row.get(9)?,
        install_claim: install_claim
            .map(|value| Uuid::parse_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
    })
}

pub(crate) fn land_update_reader_at(
    root: &Path,
    latest_version: &str,
    asset_name: &str,
    content_length: Option<u64>,
    mut reader: impl Read,
) -> Result<LandedAppUpdate, String> {
    let content_length = validate_content_length(content_length)?;
    let directory = ManagedUpdateDirectory::open(root)?;
    let store = ReceiptStore::open(directory.root())?;
    let intent = store.begin(latest_version, asset_name)?;
    let mut file = match directory.create_staged_file(&intent.part_name) {
        Ok(file) => file,
        Err(error) => {
            let _ = store.mark_failed(&intent);
            return Err(error);
        }
    };
    let identity = match directory.file_identity(&file) {
        Ok(identity) => identity,
        Err(error) => {
            let _ = directory.delete_open_file(&file);
            let _ = store.mark_repair_required(&intent);
            return Err(error);
        }
    };
    let downloading = match store.mark_downloading(&intent, &identity) {
        Ok(record) => record,
        Err(error) => {
            let _ = directory.delete_open_file(&file);
            return Err(error);
        }
    };

    let streamed = stream_and_hash(&mut reader, &mut file, content_length);
    let (sha256, byte_size) = match streamed {
        Ok(result) => result,
        Err(error) => {
            let cleanup = directory.delete_open_file(&file);
            let _ = if cleanup.is_ok() {
                store.mark_failed(&downloading)
            } else {
                store.mark_repair_required(&downloading)
            };
            return Err(error);
        }
    };
    let staged = match store.mark_staged(&downloading, &sha256, byte_size) {
        Ok(record) => record,
        Err(error) => {
            let _ = directory.delete_open_file(&file);
            return Err(error);
        }
    };
    if let Err(error) = directory.rename_staged_file(&file, &staged.final_name) {
        let cleanup = directory.delete_open_file(&file);
        let _ = if cleanup.is_ok() {
            store.mark_failed(&staged)
        } else {
            store.mark_repair_required(&staged)
        };
        return Err(error);
    }
    let ready = match store.mark_ready(&staged) {
        Ok(record) => record,
        Err(error) => {
            let _ = directory.delete_open_file(&file);
            return Err(error);
        }
    };

    Ok(LandedAppUpdate {
        download_receipt: receipt_token(ready.id, ready.revision),
        sha256,
        byte_size,
    })
}

pub(crate) fn schedule_install_at(
    root: &Path,
    download_receipt: &str,
    spawn: impl FnOnce(&Path, &str, u64) -> Result<(), String>,
) -> Result<ScheduledAppUpdate, String> {
    let directory = ManagedUpdateDirectory::open(root)?;
    let store = ReceiptStore::open(directory.root())?;
    let pending = store.claim_install(download_receipt)?;
    let identity = pending
        .file_identity
        .as_deref()
        .ok_or_else(|| "update receipt has no file identity".to_string())?;
    let expected_hash = pending
        .sha256
        .as_deref()
        .ok_or_else(|| "update receipt has no cryptographic hash".to_string())?;
    let expected_size = pending
        .byte_size
        .ok_or_else(|| "update receipt has no byte size".to_string())?;
    let mut file = match directory.open_file_if_identity(&pending.final_name, identity) {
        Ok(Some(file)) => file,
        Ok(None) | Err(_) => {
            let _ = store.mark_repair_required(&pending);
            return Err("downloaded update installer identity changed".to_string());
        }
    };
    let (actual_hash, actual_size) = match hash_open_file(&mut file) {
        Ok(result) => result,
        Err(error) => {
            let _ = store.mark_repair_required(&pending);
            return Err(error);
        }
    };
    if actual_hash != expected_hash || actual_size != expected_size {
        let _ = store.mark_repair_required(&pending);
        return Err("downloaded update installer verification failed".to_string());
    }
    let installer_path = directory.root().join(&pending.final_name);
    if let Err(error) = spawn(&installer_path, expected_hash, expected_size) {
        let _ = store.mark_failed(&pending);
        return Err(error);
    }
    // Once the runner exists, returning an error would leave the UI believing
    // nothing can happen while the already-started runner waits to install.
    // A failed final transition deliberately leaves install_pending as the
    // durable uncertain checkpoint; startup recovery will never replay it.
    let _ = store.mark_install_scheduled(&pending);
    Ok(ScheduledAppUpdate {
        restart_scheduled: true,
    })
}

pub(crate) fn recover_update_receipts_at(root: &Path) -> Result<(), String> {
    let directory = ManagedUpdateDirectory::open(root)?;
    let store = ReceiptStore::open(directory.root())?;
    for record in store.recoverable()? {
        match record.status {
            ReceiptStatus::Intent => {
                store.mark_failed(&record)?;
            }
            ReceiptStatus::Downloading => {
                let Some(identity) = record.file_identity.as_deref() else {
                    store.mark_repair_required(&record)?;
                    continue;
                };
                match directory.delete_file_if_identity(&record.part_name, identity) {
                    Ok(FileDeleteResult::Deleted | FileDeleteResult::Missing) => {
                        store.mark_failed(&record)?;
                    }
                    Ok(FileDeleteResult::IdentityMismatch) | Err(_) => {
                        store.mark_repair_required(&record)?;
                    }
                }
            }
            ReceiptStatus::Staged
                if recover_staged_receipt(&directory, &store, &record).is_err() =>
            {
                store.mark_repair_required(&record)?;
            }
            ReceiptStatus::InstallPending => {
                store.mark_repair_required(&record)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn recover_staged_receipt(
    directory: &ManagedUpdateDirectory,
    store: &ReceiptStore,
    record: &ReceiptRecord,
) -> Result<(), String> {
    let identity = record
        .file_identity
        .as_deref()
        .ok_or_else(|| "staged update receipt has no file identity".to_string())?;
    let expected_hash = record
        .sha256
        .as_deref()
        .ok_or_else(|| "staged update receipt has no hash".to_string())?;
    let expected_size = record
        .byte_size
        .ok_or_else(|| "staged update receipt has no size".to_string())?;

    if let Some(mut final_file) = directory.open_file_if_identity(&record.final_name, identity)? {
        let (hash, size) = hash_open_file(&mut final_file)?;
        if hash != expected_hash || size != expected_size {
            return Err("staged update final file verification failed".to_string());
        }
        store.mark_ready(record)?;
        return Ok(());
    }
    let mut part_file = directory
        .open_file_if_identity(&record.part_name, identity)?
        .ok_or_else(|| "staged update file is missing".to_string())?;
    let (hash, size) = hash_open_file(&mut part_file)?;
    if hash != expected_hash || size != expected_size {
        return Err("staged update file verification failed".to_string());
    }
    directory.rename_staged_file(&part_file, &record.final_name)?;
    store.mark_ready(record)?;
    Ok(())
}

fn validate_content_length(content_length: Option<u64>) -> Result<u64, String> {
    match content_length {
        Some(length) if (1..=APP_UPDATE_MAX_BYTES).contains(&length) => Ok(length),
        Some(_) => Err("update installer Content-Length is outside the allowed limit".to_string()),
        None => Err("update installer response has no bounded Content-Length".to_string()),
    }
}

fn stream_and_hash(
    reader: &mut impl Read,
    file: &mut File,
    declared_size: u64,
) -> Result<(String, u64), String> {
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|_| "update installer stream could not be read".to_string())?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| "update installer stream exceeded the allowed limit".to_string())?;
        if total > declared_size || total > APP_UPDATE_MAX_BYTES {
            return Err("update installer stream exceeded its declared size".to_string());
        }
        file.write_all(&buffer[..read])
            .map_err(|_| "update installer stream could not be stored".to_string())?;
        hasher.update(&buffer[..read]);
    }
    if total != declared_size {
        return Err("update installer stream did not match its declared size".to_string());
    }
    file.flush()
        .and_then(|_| file.sync_all())
        .map_err(|_| "update installer staging file could not be committed".to_string())?;
    Ok((hex::encode(hasher.finalize()), total))
}

fn hash_open_file(file: &mut File) -> Result<(String, u64), String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| "update installer could not be revalidated".to_string())?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "update installer could not be revalidated".to_string())?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| "update installer is too large".to_string())?;
        if total > APP_UPDATE_MAX_BYTES {
            return Err("update installer is too large".to_string());
        }
        hasher.update(&buffer[..read]);
    }
    Ok((hex::encode(hasher.finalize()), total))
}

fn validate_public_label(value: &str, label: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 160
        || value.chars().any(|character| character.is_control())
    {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

fn validate_asset_name(asset_name: &str) -> Result<(), String> {
    validate_public_label(asset_name, "update asset name")?;
    let normalized = asset_name.to_ascii_lowercase();
    if asset_name.contains(['/', '\\', ':'])
        || Path::new(asset_name)
            .file_name()
            .and_then(|value| value.to_str())
            != Some(asset_name)
        || !(normalized.ends_with(".exe") || normalized.ends_with(".msi"))
    {
        return Err("update asset name is invalid".to_string());
    }
    Ok(())
}

fn safe_asset_name(asset_name: &str) -> String {
    asset_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || character == '.'
                || character == '-'
                || character == '_'
            {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn validate_sha256(value: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("update receipt SHA-256 is invalid".to_string());
    }
    Ok(())
}

fn receipt_token(id: Uuid, revision: u64) -> String {
    format!("{RECEIPT_TOKEN_PREFIX}.{}.{revision}", id.simple())
}

fn parse_receipt_token(value: &str) -> Result<(Uuid, u64), String> {
    let mut parts = value.split('.');
    if parts.next() != Some(RECEIPT_TOKEN_PREFIX) {
        return Err("update download receipt is invalid".to_string());
    }
    let id = Uuid::parse_str(
        parts
            .next()
            .ok_or_else(|| "update download receipt is invalid".to_string())?,
    )
    .map_err(|_| "update download receipt is invalid".to_string())?;
    let revision = parts
        .next()
        .ok_or_else(|| "update download receipt is invalid".to_string())?
        .parse::<u64>()
        .map_err(|_| "update download receipt is invalid".to_string())?;
    if parts.next().is_some() {
        return Err("update download receipt is invalid".to_string());
    }
    Ok((id, revision))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileDeleteResult {
    Deleted,
    Missing,
    IdentityMismatch,
}

#[cfg(windows)]
mod managed_directory {
    use std::fs::{self, File, OpenOptions};
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, RawHandle};
    use std::path::{Path, PathBuf};

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        FileAttributeTagInfo, FileDispositionInfoEx, FileIdInfo, FileRenameInfo,
        GetFileInformationByHandleEx, SetFileInformationByHandle, DELETE,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_DISPOSITION_FLAG_DELETE,
        FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_ID_INFO, FILE_RENAME_INFO, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };

    use super::FileDeleteResult;

    pub(super) struct ManagedUpdateDirectory {
        root: PathBuf,
        _root_handle: File,
    }

    impl ManagedUpdateDirectory {
        pub(super) fn open(root: &Path) -> Result<Self, String> {
            fs::create_dir_all(root)
                .map_err(|_| "managed update directory is unavailable".to_string())?;
            let original_handle = OpenOptions::new()
                .read(true)
                .access_mode(FILE_GENERIC_READ.0)
                .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
                .open(root)
                .map_err(|_| "managed update directory handle is unavailable".to_string())?;
            reject_reparse(&original_handle)?;
            let canonical = fs::canonicalize(root)
                .map_err(|_| "managed update directory is unavailable".to_string())?;
            let handle = OpenOptions::new()
                .read(true)
                .access_mode(FILE_GENERIC_READ.0)
                .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
                .open(&canonical)
                .map_err(|_| "managed update directory handle is unavailable".to_string())?;
            reject_reparse(&handle)?;
            drop(original_handle);
            Ok(Self {
                root: canonical,
                _root_handle: handle,
            })
        }

        pub(super) fn root(&self) -> &Path {
            &self.root
        }

        pub(super) fn create_staged_file(&self, name: &str) -> Result<File, String> {
            validate_name(name)?;
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .access_mode(FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0 | DELETE.0)
                .share_mode(FILE_SHARE_READ.0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
                .open(self.root.join(name))
                .map_err(|_| "update installer staging file could not be created".to_string())?;
            reject_reparse(&file)?;
            Ok(file)
        }

        pub(super) fn file_identity(&self, file: &File) -> Result<String, String> {
            stable_file_identity(file)
        }

        pub(super) fn rename_staged_file(
            &self,
            file: &File,
            destination_name: &str,
        ) -> Result<(), String> {
            validate_name(destination_name)?;
            let destination = self.root.join(destination_name);
            let wide = destination.as_os_str().encode_wide().collect::<Vec<_>>();
            let byte_len = wide
                .len()
                .checked_mul(size_of::<u16>())
                .ok_or_else(|| "update installer destination is invalid".to_string())?;
            let allocation_size = size_of::<FILE_RENAME_INFO>()
                .checked_add(byte_len.saturating_sub(size_of::<u16>()))
                .ok_or_else(|| "update installer destination is invalid".to_string())?;
            let mut storage = vec![0u64; allocation_size.div_ceil(size_of::<u64>())];
            let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            unsafe {
                (*info).Anonymous.ReplaceIfExists = false;
                (*info).RootDirectory = HANDLE::default();
                (*info).FileNameLength = u32::try_from(byte_len)
                    .map_err(|_| "update installer destination is invalid".to_string())?;
                std::ptr::copy_nonoverlapping(
                    wide.as_ptr(),
                    (*info).FileName.as_mut_ptr(),
                    wide.len(),
                );
                SetFileInformationByHandle(
                    file_handle(file),
                    FileRenameInfo,
                    info.cast(),
                    u32::try_from(allocation_size)
                        .map_err(|_| "update installer destination is invalid".to_string())?,
                )
                .map_err(|_| "update installer atomic landing failed".to_string())?;
            }
            Ok(())
        }

        pub(super) fn delete_open_file(&self, file: &File) -> Result<(), String> {
            delete_handle(file)
        }

        pub(super) fn delete_file_if_identity(
            &self,
            name: &str,
            expected_identity: &str,
        ) -> Result<FileDeleteResult, String> {
            validate_name(name)?;
            let file = match OpenOptions::new()
                .read(true)
                .access_mode(FILE_GENERIC_READ.0 | DELETE.0)
                .share_mode(FILE_SHARE_READ.0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
                .open(self.root.join(name))
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(FileDeleteResult::Missing)
                }
                Err(_) => return Err("update recovery file is unavailable".to_string()),
            };
            reject_reparse(&file)?;
            if stable_file_identity(&file)? != expected_identity {
                return Ok(FileDeleteResult::IdentityMismatch);
            }
            delete_handle(&file)?;
            Ok(FileDeleteResult::Deleted)
        }

        pub(super) fn open_file_if_identity(
            &self,
            name: &str,
            expected_identity: &str,
        ) -> Result<Option<File>, String> {
            validate_name(name)?;
            let file = match OpenOptions::new()
                .read(true)
                .access_mode(FILE_GENERIC_READ.0)
                .share_mode(FILE_SHARE_READ.0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
                .open(self.root.join(name))
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err("update installer is unavailable".to_string()),
            };
            reject_reparse(&file)?;
            if stable_file_identity(&file)? != expected_identity {
                return Err("update installer identity changed".to_string());
            }
            Ok(Some(file))
        }
    }

    fn validate_name(name: &str) -> Result<(), String> {
        if name.is_empty()
            || name.contains(['/', '\\', ':'])
            || Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name)
        {
            return Err("managed update file name is unsafe".to_string());
        }
        Ok(())
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
            .map_err(|_| "managed update file identity is unavailable".to_string())?;
        }
        if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err("managed update path is an unsafe reparse point".to_string());
        }
        Ok(())
    }

    fn stable_file_identity(file: &File) -> Result<String, String> {
        let mut info: FILE_ID_INFO = unsafe { zeroed() };
        unsafe {
            GetFileInformationByHandleEx(
                file_handle(file),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                u32::try_from(size_of::<FILE_ID_INFO>()).unwrap_or(u32::MAX),
            )
            .map_err(|_| "managed update stable file identity is unavailable".to_string())?;
        }
        Ok(format!(
            "v1:{:016x}:{}",
            info.VolumeSerialNumber,
            hex::encode(info.FileId.Identifier)
        ))
    }

    fn delete_handle(file: &File) -> Result<(), String> {
        let disposition = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE,
        };
        unsafe {
            SetFileInformationByHandle(
                file_handle(file),
                FileDispositionInfoEx,
                (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
                u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>()).unwrap_or(u32::MAX),
            )
            .map_err(|_| "managed update file cleanup failed".to_string())?;
        }
        Ok(())
    }

    fn file_handle(file: &File) -> HANDLE {
        HANDLE(file.as_raw_handle() as RawHandle)
    }
}

#[cfg(not(windows))]
mod managed_directory {
    use std::fs::{self, File, OpenOptions};
    use std::path::{Path, PathBuf};

    use super::FileDeleteResult;

    pub(super) struct ManagedUpdateDirectory {
        root: PathBuf,
    }

    impl ManagedUpdateDirectory {
        pub(super) fn open(root: &Path) -> Result<Self, String> {
            fs::create_dir_all(root)
                .map_err(|_| "managed update directory is unavailable".to_string())?;
            let metadata = fs::symlink_metadata(root)
                .map_err(|_| "managed update directory is unavailable".to_string())?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err("managed update directory is unsafe".to_string());
            }
            Ok(Self {
                root: fs::canonicalize(root)
                    .map_err(|_| "managed update directory is unavailable".to_string())?,
            })
        }

        pub(super) fn root(&self) -> &Path {
            &self.root
        }

        pub(super) fn create_staged_file(&self, name: &str) -> Result<File, String> {
            validate_name(name)?;
            OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(self.root.join(name))
                .map_err(|_| "update installer staging file could not be created".to_string())
        }

        pub(super) fn file_identity(&self, file: &File) -> Result<String, String> {
            portable_file_identity(file)
        }

        pub(super) fn rename_staged_file(
            &self,
            file: &File,
            destination_name: &str,
        ) -> Result<(), String> {
            validate_name(destination_name)?;
            let expected_identity = self.file_identity(file)?;
            let source = fs::read_dir(&self.root)
                .map_err(|_| "update installer staging file is unavailable".to_string())?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    path.extension().and_then(|value| value.to_str()) == Some("part")
                        && File::open(path)
                            .ok()
                            .and_then(|candidate| self.file_identity(&candidate).ok())
                            .as_deref()
                            == Some(expected_identity.as_str())
                })
                .ok_or_else(|| "update installer staging file is unavailable".to_string())?;
            fs::rename(source, self.root.join(destination_name))
                .map_err(|_| "update installer atomic landing failed".to_string())
        }

        pub(super) fn delete_open_file(&self, file: &File) -> Result<(), String> {
            let identity = self.file_identity(file)?;
            let path = fs::read_dir(&self.root)
                .map_err(|_| "managed update file cleanup failed".to_string())?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    File::open(path)
                        .ok()
                        .and_then(|candidate| self.file_identity(&candidate).ok())
                        .as_deref()
                        == Some(identity.as_str())
                })
                .ok_or_else(|| "managed update file cleanup failed".to_string())?;
            fs::remove_file(path).map_err(|_| "managed update file cleanup failed".to_string())
        }

        pub(super) fn delete_file_if_identity(
            &self,
            name: &str,
            expected_identity: &str,
        ) -> Result<FileDeleteResult, String> {
            match self.open_file_if_identity(name, expected_identity) {
                Ok(Some(_)) => {
                    fs::remove_file(self.root.join(name))
                        .map_err(|_| "managed update file cleanup failed".to_string())?;
                    Ok(FileDeleteResult::Deleted)
                }
                Ok(None) => Ok(FileDeleteResult::Missing),
                Err(_) => Ok(FileDeleteResult::IdentityMismatch),
            }
        }

        pub(super) fn open_file_if_identity(
            &self,
            name: &str,
            expected_identity: &str,
        ) -> Result<Option<File>, String> {
            validate_name(name)?;
            let file = match File::open(self.root.join(name)) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err("update installer is unavailable".to_string()),
            };
            if self.file_identity(&file)? != expected_identity {
                return Err("update installer identity changed".to_string());
            }
            Ok(Some(file))
        }
    }

    fn validate_name(name: &str) -> Result<(), String> {
        if name.is_empty()
            || name.contains(['/', '\\', ':'])
            || Path::new(name).file_name().and_then(|value| value.to_str()) != Some(name)
        {
            return Err("managed update file name is unsafe".to_string());
        }
        Ok(())
    }

    #[cfg(unix)]
    fn portable_file_identity(file: &File) -> Result<String, String> {
        use std::os::unix::fs::MetadataExt;

        let metadata = file
            .metadata()
            .map_err(|_| "managed update stable file identity is unavailable".to_string())?;
        Ok(format!("unix:{}:{}", metadata.dev(), metadata.ino()))
    }

    #[cfg(not(unix))]
    fn portable_file_identity(file: &File) -> Result<String, String> {
        let metadata = file
            .metadata()
            .map_err(|_| "managed update stable file identity is unavailable".to_string())?;
        Ok(format!(
            "portable:{}:{:?}",
            metadata.len(),
            metadata.modified().ok()
        ))
    }
}

use managed_directory::ManagedUpdateDirectory;

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        land_update_reader_at, parse_receipt_token, recover_update_receipts_at,
        schedule_install_at, stream_and_hash, ManagedUpdateDirectory, ReceiptStatus, ReceiptStore,
        APP_UPDATE_MAX_BYTES,
    };

    #[test]
    fn bounded_stream_requires_content_length_and_rejects_oversize() {
        let root = tempfile::tempdir().unwrap();
        assert!(land_update_reader_at(
            root.path(),
            "1.0.0",
            "DS.Agent_1.0.0_x64-setup.exe",
            None,
            Cursor::new(b"installer"),
        )
        .is_err());
        assert!(land_update_reader_at(
            root.path(),
            "1.0.0",
            "DS.Agent_1.0.0_x64-setup.exe",
            Some(APP_UPDATE_MAX_BYTES + 1),
            Cursor::new(b"installer"),
        )
        .is_err());
    }

    #[test]
    fn receipt_store_rejects_an_unsafe_database_entry() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("update-receipts.sqlite3")).unwrap();
        assert!(ReceiptStore::open(root.path()).is_err());
    }

    #[test]
    fn bounded_stream_rejects_actual_size_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let result = land_update_reader_at(
            root.path(),
            "1.0.0",
            "DS.Agent_1.0.0_x64-setup.exe",
            Some(8),
            Cursor::new(b"installer-extra"),
        );
        assert!(result.is_err());
        let store = ReceiptStore::open(root.path()).unwrap();
        assert!(store.recoverable().unwrap().is_empty());
    }

    #[test]
    fn ready_receipt_is_path_free_persistent_and_one_shot() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"bounded fake installer";
        let landed = land_update_reader_at(
            root.path(),
            "1.0.0",
            "DS.Agent_1.0.0_x64-setup.exe",
            Some(bytes.len() as u64),
            Cursor::new(bytes),
        )
        .unwrap();
        assert!(!landed.download_receipt.contains(['/', '\\', ':']));
        assert_eq!(landed.byte_size, bytes.len() as u64);
        assert_eq!(landed.sha256.len(), 64);

        let calls = AtomicUsize::new(0);
        schedule_install_at(root.path(), &landed.download_receipt, |_, hash, size| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(hash, landed.sha256);
            assert_eq!(size, landed.byte_size);
            Ok(())
        })
        .unwrap();
        assert!(
            schedule_install_at(root.path(), &landed.download_receipt, |_, _, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn install_revalidates_hash_before_spawn() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"bounded fake installer";
        let landed = land_update_reader_at(
            root.path(),
            "1.0.0",
            "DS.Agent_1.0.0_x64-setup.exe",
            Some(bytes.len() as u64),
            Cursor::new(bytes),
        )
        .unwrap();
        let (id, _) = parse_receipt_token(&landed.download_receipt).unwrap();
        let store = ReceiptStore::open(root.path()).unwrap();
        let record = store.load(id).unwrap();
        std::fs::write(root.path().join(record.final_name), b"tampered installer").unwrap();

        let calls = AtomicUsize::new(0);
        assert!(
            schedule_install_at(root.path(), &landed.download_receipt, |_, _, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            store.load(id).unwrap().status,
            ReceiptStatus::RepairRequired
        );
    }

    #[test]
    fn install_rejects_file_identity_replacement_before_spawn() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"bounded fake installer";
        let landed = land_update_reader_at(
            root.path(),
            "1.0.0",
            "DS.Agent_1.0.0_x64-setup.exe",
            Some(bytes.len() as u64),
            Cursor::new(bytes),
        )
        .unwrap();
        let (id, _) = parse_receipt_token(&landed.download_receipt).unwrap();
        let store = ReceiptStore::open(root.path()).unwrap();
        let record = store.load(id).unwrap();
        let final_path = root.path().join(record.final_name);
        std::fs::remove_file(&final_path).unwrap();
        std::fs::write(&final_path, bytes).unwrap();

        let calls = AtomicUsize::new(0);
        assert!(
            schedule_install_at(root.path(), &landed.download_receipt, |_, _, _| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            store.load(id).unwrap().status,
            ReceiptStatus::RepairRequired
        );
    }

    #[test]
    fn restart_recovery_deletes_only_the_identity_bound_partial_file() {
        let root = tempfile::tempdir().unwrap();
        let directory = ManagedUpdateDirectory::open(root.path()).unwrap();
        let store = ReceiptStore::open(directory.root()).unwrap();
        let intent = store
            .begin("1.0.0", "DS.Agent_1.0.0_x64-setup.exe")
            .unwrap();
        let mut file = directory.create_staged_file(&intent.part_name).unwrap();
        let identity = directory.file_identity(&file).unwrap();
        let downloading = store.mark_downloading(&intent, &identity).unwrap();
        std::io::Write::write_all(&mut file, b"partial").unwrap();
        file.sync_all().unwrap();
        drop(file);
        drop(store);
        drop(directory);

        recover_update_receipts_at(root.path()).unwrap();
        assert!(!root.path().join(&downloading.part_name).exists());
        let reopened = ReceiptStore::open(root.path()).unwrap();
        assert_eq!(
            reopened.load(downloading.id).unwrap().status,
            ReceiptStatus::Failed
        );
    }

    #[test]
    fn restart_recovery_finishes_the_exact_staged_rename_window() {
        let root = tempfile::tempdir().unwrap();
        let directory = ManagedUpdateDirectory::open(root.path()).unwrap();
        let store = ReceiptStore::open(directory.root()).unwrap();
        let intent = store
            .begin("1.0.0", "DS.Agent_1.0.0_x64-setup.exe")
            .unwrap();
        let mut file = directory.create_staged_file(&intent.part_name).unwrap();
        let identity = directory.file_identity(&file).unwrap();
        let downloading = store.mark_downloading(&intent, &identity).unwrap();
        let bytes = b"bounded fake installer";
        let (hash, size) =
            stream_and_hash(&mut Cursor::new(bytes), &mut file, bytes.len() as u64).unwrap();
        let staged = store.mark_staged(&downloading, &hash, size).unwrap();
        directory
            .rename_staged_file(&file, &staged.final_name)
            .unwrap();
        drop(file);
        drop(store);
        drop(directory);

        recover_update_receipts_at(root.path()).unwrap();
        let reopened = ReceiptStore::open(root.path()).unwrap();
        let ready = reopened.load(staged.id).unwrap();
        assert_eq!(ready.status, ReceiptStatus::Ready);
        assert!(root.path().join(ready.final_name).exists());
    }

    #[test]
    fn restart_recovery_never_replays_install_pending() {
        let root = tempfile::tempdir().unwrap();
        let bytes = b"bounded fake installer";
        let landed = land_update_reader_at(
            root.path(),
            "1.0.0",
            "DS.Agent_1.0.0_x64-setup.exe",
            Some(bytes.len() as u64),
            Cursor::new(bytes),
        )
        .unwrap();
        let (id, _) = parse_receipt_token(&landed.download_receipt).unwrap();
        let store = ReceiptStore::open(root.path()).unwrap();
        assert_eq!(
            store
                .claim_install(&landed.download_receipt)
                .unwrap()
                .status,
            ReceiptStatus::InstallPending
        );
        drop(store);

        recover_update_receipts_at(root.path()).unwrap();
        let reopened = ReceiptStore::open(root.path()).unwrap();
        assert_eq!(
            reopened.load(id).unwrap().status,
            ReceiptStatus::RepairRequired
        );
    }
}
