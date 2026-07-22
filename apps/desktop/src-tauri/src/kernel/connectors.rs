#![allow(dead_code)]

pub mod catalog;
pub mod contract;
pub mod domain;
pub mod draft;
pub mod google;
pub mod http;
pub(crate) mod landing;
#[cfg(windows)]
pub(crate) mod landing_windows;
#[cfg(test)]
mod lifecycle_tests;
pub mod microsoft;
pub mod mutation;
pub mod oauth;
pub mod provider;
pub(crate) mod read_execution;
pub(crate) mod reconciliation;
pub(crate) mod revocation;
pub(crate) mod runtime_registry;
pub mod sync;

use std::collections::HashMap;
#[cfg(windows)]
use std::fs::{self, OpenOptions};
#[cfg(windows)]
use std::io::{Read, Write};
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroize;

use crate::kernel::policy::CapabilityKind;
use crate::kernel::tool_runtime::{
    tool_request_fingerprint, ToolExecutionRequest, ToolExecutionStatus, ToolInvocationRecord,
    CONNECTOR_MUTATE_TOOL_ID,
};

use mutation::ConnectorMutationIntent;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ConnectorCredentialHandle(String);

impl ConnectorCredentialHandle {
    pub fn new() -> Self {
        Self(format!("connector-credential:{}", Uuid::new_v4()))
    }

    #[cfg(windows)]
    fn vault_file_name(&self) -> String {
        format!("{:x}.credential", Sha256::digest(self.0.as_bytes()))
    }

    #[cfg(windows)]
    fn vault_entropy(&self) -> [u8; 32] {
        Sha256::digest(format!("ds-agent.connector-vault.v1:{}", self.0).as_bytes()).into()
    }
}

pub struct ConnectorSecret(String);

impl ConnectorSecret {
    pub fn new(mut value: String) -> Result<Self, String> {
        if value.trim().is_empty() {
            value.zeroize();
            return Err("connector secret is required".to_string());
        }
        Ok(Self(value))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for ConnectorSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub trait ConnectorCredentialStore {
    fn put(&mut self, secret: ConnectorSecret) -> Result<ConnectorCredentialHandle, String> {
        let handle = ConnectorCredentialHandle::new();
        self.put_new_at(&handle, secret)?;
        Ok(handle)
    }
    fn put_new_at(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        if self.contains(handle) {
            return Err("connector credential already exists".to_string());
        }
        self.put_at(handle, secret)
    }
    fn put_at(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String>;
    fn read(&self, handle: &ConnectorCredentialHandle) -> Result<ConnectorSecret, String>;
    fn replace(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String>;
    fn delete(
        &mut self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<ConnectorCredentialDeleteOutcome, String>;
    fn contains(&self, handle: &ConnectorCredentialHandle) -> bool;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorCredentialDeleteOutcome {
    Deleted,
    AlreadyAbsent,
}

#[cfg(test)]
#[derive(Default)]
pub struct FakeConnectorCredentialStore {
    secrets: HashMap<ConnectorCredentialHandle, ConnectorSecret>,
}

#[cfg(test)]
impl ConnectorCredentialStore for FakeConnectorCredentialStore {
    fn put_new_at(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        if self.secrets.contains_key(handle) {
            return Err("connector credential already exists".to_string());
        }
        self.secrets.insert(handle.clone(), secret);
        Ok(())
    }

    fn put_at(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        self.secrets.insert(handle.clone(), secret);
        Ok(())
    }

    fn delete(
        &mut self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<ConnectorCredentialDeleteOutcome, String> {
        Ok(if self.secrets.remove(handle).is_some() {
            ConnectorCredentialDeleteOutcome::Deleted
        } else {
            ConnectorCredentialDeleteOutcome::AlreadyAbsent
        })
    }

    fn read(&self, handle: &ConnectorCredentialHandle) -> Result<ConnectorSecret, String> {
        let secret = self
            .secrets
            .get(handle)
            .ok_or_else(|| "connector credential is unavailable".to_string())?;
        ConnectorSecret::new(secret.expose().to_string())
    }

    fn replace(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        if !self.secrets.contains_key(handle) {
            return Err("connector credential is unavailable".to_string());
        }
        self.secrets.insert(handle.clone(), secret);
        Ok(())
    }

    fn contains(&self, handle: &ConnectorCredentialHandle) -> bool {
        self.secrets.contains_key(handle)
    }
}

#[cfg(windows)]
const CONNECTOR_VAULT_MAX_PLAINTEXT_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const CONNECTOR_VAULT_MAX_PROTECTED_BYTES: usize = 128 * 1024;

#[cfg(windows)]
pub struct WindowsConnectorCredentialStore {
    root: PathBuf,
}

#[cfg(windows)]
impl WindowsConnectorCredentialStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, String> {
        fs::create_dir_all(root.as_ref())
            .map_err(|_| "Windows could not initialize the connector vault".to_string())?;
        let root = fs::canonicalize(root.as_ref())
            .map_err(|_| "Windows could not initialize the connector vault".to_string())?;
        if !root.is_dir() {
            return Err("Windows connector vault path is invalid".to_string());
        }
        Self::cleanup_staged_files(&root)?;
        Ok(Self { root })
    }

    fn cleanup_staged_files(root: &Path) -> Result<(), String> {
        let mut entries_seen = 0usize;
        let mut staged_seen = 0usize;
        for entry in fs::read_dir(root)
            .map_err(|_| "Windows could not inspect the connector vault".to_string())?
        {
            entries_seen += 1;
            if entries_seen > 4096 {
                return Err("Windows connector vault entry budget exceeded".to_string());
            }
            let entry =
                entry.map_err(|_| "Windows could not inspect the connector vault".to_string())?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with('.') || !name.ends_with(".tmp") {
                continue;
            }
            staged_seen += 1;
            if staged_seen > 128 {
                return Err("Windows connector vault staging budget exceeded".to_string());
            }
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|_| "Windows could not inspect the connector vault".to_string())?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("Windows connector vault staging path is unsafe".to_string());
            }
            fs::remove_file(entry.path())
                .map_err(|_| "Windows could not clean the connector vault".to_string())?;
        }
        Ok(())
    }

    fn credential_path(&self, handle: &ConnectorCredentialHandle) -> PathBuf {
        self.root.join(handle.vault_file_name())
    }

    fn protect(
        handle: &ConnectorCredentialHandle,
        secret: &ConnectorSecret,
    ) -> Result<Vec<u8>, String> {
        use windows::core::w;
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Cryptography::{
            CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        };

        let mut plaintext = secret.expose().as_bytes().to_vec();
        if plaintext.len() > CONNECTOR_VAULT_MAX_PLAINTEXT_BYTES {
            plaintext.zeroize();
            return Err("connector credential exceeds the vault limit".to_string());
        }
        let input = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_mut_ptr(),
        };
        let mut entropy = handle.vault_entropy();
        let entropy_blob = CRYPT_INTEGER_BLOB {
            cbData: entropy.len() as u32,
            pbData: entropy.as_mut_ptr(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        let result = unsafe {
            CryptProtectData(
                &input,
                w!("DS Agent connector credential"),
                Some(&entropy_blob),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        plaintext.zeroize();
        entropy.zeroize();
        result.map_err(|_| "Windows could not protect the connector credential".to_string())?;
        if output.pbData.is_null() || output.cbData as usize > CONNECTOR_VAULT_MAX_PROTECTED_BYTES {
            if !output.pbData.is_null() {
                let protected = unsafe {
                    std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize)
                };
                protected.zeroize();
                unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
            }
            return Err("Windows protected connector credential is invalid".to_string());
        }
        let protected =
            unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
        let value = protected.to_vec();
        protected.zeroize();
        unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
        Ok(value)
    }

    fn unprotect(
        handle: &ConnectorCredentialHandle,
        mut protected: Vec<u8>,
    ) -> Result<ConnectorSecret, String> {
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        };

        if protected.is_empty() || protected.len() > CONNECTOR_VAULT_MAX_PROTECTED_BYTES {
            protected.zeroize();
            return Err("connector credential is unreadable".to_string());
        }
        let input = CRYPT_INTEGER_BLOB {
            cbData: protected.len() as u32,
            pbData: protected.as_mut_ptr(),
        };
        let mut entropy = handle.vault_entropy();
        let entropy_blob = CRYPT_INTEGER_BLOB {
            cbData: entropy.len() as u32,
            pbData: entropy.as_mut_ptr(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        let result = unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy_blob),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        protected.zeroize();
        entropy.zeroize();
        result.map_err(|_| "connector credential is unreadable".to_string())?;
        if output.pbData.is_null() || output.cbData as usize > CONNECTOR_VAULT_MAX_PLAINTEXT_BYTES {
            if !output.pbData.is_null() {
                let plaintext = unsafe {
                    std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize)
                };
                plaintext.zeroize();
                unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
            }
            return Err("connector credential is unreadable".to_string());
        }
        let plaintext =
            unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
        let value = String::from_utf8(plaintext.to_vec());
        plaintext.zeroize();
        unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
        match value {
            Ok(value) => ConnectorSecret::new(value),
            Err(error) => {
                let mut bytes = error.into_bytes();
                bytes.zeroize();
                Err("connector credential is unreadable".to_string())
            }
        }
    }

    fn write_protected(
        &self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
        replace_existing: bool,
    ) -> Result<(), String> {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };

        let destination = self.credential_path(handle);
        if let Ok(metadata) = fs::symlink_metadata(&destination) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err("connector credential path is unsafe".to_string());
            }
        }
        let temp = self.root.join(format!(".{}.tmp", Uuid::new_v4()));
        let mut protected = Self::protect(handle, &secret)?;
        let write_result = (|| -> Result<(), String> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|_| "Windows could not stage the connector credential".to_string())?;
            file.write_all(&protected)
                .and_then(|_| file.sync_all())
                .map_err(|_| "Windows could not stage the connector credential".to_string())?;
            drop(file);
            let source_wide = temp
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let destination_wide = destination
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect::<Vec<_>>();
            let flags = if replace_existing {
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH
            } else {
                MOVEFILE_WRITE_THROUGH
            };
            unsafe {
                MoveFileExW(
                    PCWSTR(source_wide.as_ptr()),
                    PCWSTR(destination_wide.as_ptr()),
                    flags,
                )
            }
            .map_err(|_| "Windows could not commit the connector credential".to_string())
        })();
        protected.zeroize();
        if write_result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        write_result
    }

    fn read_protected(&self, handle: &ConnectorCredentialHandle) -> Result<Vec<u8>, String> {
        let path = self.credential_path(handle);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| "connector credential is unavailable".to_string())?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() as usize > CONNECTOR_VAULT_MAX_PROTECTED_BYTES
        {
            return Err("connector credential is unreadable".to_string());
        }
        let canonical = fs::canonicalize(&path)
            .map_err(|_| "connector credential is unreadable".to_string())?;
        if !canonical.starts_with(&self.root) {
            return Err("connector credential path is unsafe".to_string());
        }
        let mut file = OpenOptions::new()
            .read(true)
            .open(&canonical)
            .map_err(|_| "connector credential is unavailable".to_string())?;
        let mut protected = Vec::with_capacity(metadata.len() as usize);
        Read::take(&mut file, (CONNECTOR_VAULT_MAX_PROTECTED_BYTES + 1) as u64)
            .read_to_end(&mut protected)
            .map_err(|_| "connector credential is unreadable".to_string())?;
        if protected.len() > CONNECTOR_VAULT_MAX_PROTECTED_BYTES {
            protected.zeroize();
            return Err("connector credential is unreadable".to_string());
        }
        Ok(protected)
    }
}

#[cfg(windows)]
impl ConnectorCredentialStore for WindowsConnectorCredentialStore {
    fn put_new_at(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        self.write_protected(handle, secret, false)
    }

    fn put_at(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        self.write_protected(handle, secret, true)
    }

    fn read(&self, handle: &ConnectorCredentialHandle) -> Result<ConnectorSecret, String> {
        Self::unprotect(handle, self.read_protected(handle)?)
    }

    fn replace(
        &mut self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        if !self.contains(handle) {
            return Err("connector credential is unavailable".to_string());
        }
        self.write_protected(handle, secret, true)
    }

    fn delete(
        &mut self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<ConnectorCredentialDeleteOutcome, String> {
        match fs::remove_file(self.credential_path(handle)) {
            Ok(()) => Ok(ConnectorCredentialDeleteOutcome::Deleted),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ConnectorCredentialDeleteOutcome::AlreadyAbsent)
            }
            Err(_) => Err("Windows could not delete the connector credential".to_string()),
        }
    }

    fn contains(&self, handle: &ConnectorCredentialHandle) -> bool {
        fs::symlink_metadata(self.credential_path(handle))
            .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
            .unwrap_or(false)
    }
}

pub trait ConnectorCredentialRefresher: Send + Sync {
    fn needs_refresh(&self, current: &ConnectorSecret) -> bool;
    fn refresh(&self, current: &ConnectorSecret) -> Result<ConnectorSecret, String>;
}

pub struct ConnectorRuntime<S: ConnectorCredentialStore + Send> {
    credential_store: Mutex<S>,
    credential_locks: Mutex<HashMap<ConnectorCredentialHandle, Arc<Mutex<()>>>>,
    authorization_locks: Mutex<HashMap<Uuid, Arc<Mutex<()>>>>,
}

impl<S: ConnectorCredentialStore + Send> ConnectorRuntime<S> {
    pub fn new(credential_store: S) -> Self {
        Self {
            credential_store: Mutex::new(credential_store),
            credential_locks: Mutex::new(HashMap::new()),
            authorization_locks: Mutex::new(HashMap::new()),
        }
    }

    fn credential_lock(
        &self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<Arc<Mutex<()>>, String> {
        let mut locks = self
            .credential_locks
            .lock()
            .map_err(|_| "connector credential lock registry failed".to_string())?;
        Ok(Arc::clone(
            locks
                .entry(handle.clone())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        ))
    }

    fn authorization_lock(&self, authorization_id: Uuid) -> Result<Arc<Mutex<()>>, String> {
        let mut locks = self
            .authorization_locks
            .lock()
            .map_err(|_| "connector authorization lock registry failed".to_string())?;
        Ok(Arc::clone(
            locks
                .entry(authorization_id)
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        ))
    }

    pub(crate) fn with_authorization_fence<T>(
        &self,
        authorization_id: Uuid,
        operation: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let authorization_lock = self.authorization_lock(authorization_id)?;
        let _guard = authorization_lock
            .lock()
            .map_err(|_| "connector authorization lock failed".to_string())?;
        operation()
    }

    pub(super) fn with_account_credential<T, E>(
        &self,
        handle: &ConnectorCredentialHandle,
        operation: impl FnOnce(ConnectorSecret) -> Result<(T, Option<ConnectorSecret>), E>,
    ) -> Result<T, E>
    where
        E: From<String>,
    {
        let credential_lock = self.credential_lock(handle).map_err(E::from)?;
        let _guard = credential_lock
            .lock()
            .map_err(|_| E::from("connector account credential lock failed".to_string()))?;
        let current = {
            let store = self
                .credential_store
                .lock()
                .map_err(|_| E::from("connector credential store lock failed".to_string()))?;
            store.read(handle).map_err(E::from)?
        };
        let (result, replacement) = operation(current)?;
        if let Some(replacement) = replacement {
            self.credential_store
                .lock()
                .map_err(|_| E::from("connector credential store lock failed".to_string()))?
                .replace(handle, replacement)
                .map_err(E::from)?;
        }
        Ok(result)
    }

    pub fn refresh_account_credential(
        &self,
        account: &ConnectorAccount,
        refresher: &dyn ConnectorCredentialRefresher,
    ) -> Result<(), String> {
        if account.health != ConnectorHealth::Connected {
            return Err("connector account is not connected".to_string());
        }
        self.with_account_credential(&account.credential_handle, |current| {
            if !refresher.needs_refresh(&current) {
                return Ok(((), None));
            }
            let refreshed = refresher
                .refresh(&current)
                .map_err(|_| "connector credential refresh failed".to_string())?;
            Ok(((), Some(refreshed)))
        })
    }

    pub fn disconnect_account(&self, account: &mut ConnectorAccount) -> Result<(), String> {
        self.delete_account_credential(account)?;
        account.health = ConnectorHealth::Disconnected;
        account.updated_at = Utc::now();
        Ok(())
    }

    pub fn delete_account_credential(
        &self,
        account: &ConnectorAccount,
    ) -> Result<ConnectorCredentialDeleteOutcome, String> {
        let credential_lock = self.credential_lock(&account.credential_handle)?;
        let _guard = credential_lock
            .lock()
            .map_err(|_| "connector account credential lock failed".to_string())?;
        let mut store = self
            .credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?;
        store.delete(&account.credential_handle)
    }

    pub(crate) fn delete_authorization_handles(
        &self,
        session: &oauth::ConnectorAuthorizationSession,
    ) -> Result<(), String> {
        self.delete_authorization_handles_and_review(session, None)
    }

    pub(crate) fn delete_authorization_handles_and_review(
        &self,
        session: &oauth::ConnectorAuthorizationSession,
        action_authority_handle: Option<&ConnectorCredentialHandle>,
    ) -> Result<(), String> {
        let mut store = self
            .credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?;
        store.delete(&session.verifier_handle)?;
        store.delete(&session.result_credential_handle)?;
        if let Some(handle) = action_authority_handle {
            store.delete(handle)?;
        }
        Ok(())
    }

    pub(crate) fn read_authorization_review_authority(
        &self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<ConnectorSecret, String> {
        self.credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?
            .read(handle)
    }

    pub(crate) fn put_authorization_review_authority(
        &self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        self.credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?
            .put_new_at(handle, secret)
    }

    pub(crate) fn authorization_review_authority_exists(
        &self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<bool, String> {
        Ok(self
            .credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?
            .contains(handle))
    }

    pub(crate) fn read_authorization_verifier(
        &self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<ConnectorSecret, String> {
        self.credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?
            .read(handle)
    }

    pub(crate) fn put_authorization_verifier(
        &self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        self.credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?
            .put_new_at(handle, secret)
    }

    pub(crate) fn put_authorization_result(
        &self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        self.credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?
            .put_new_at(handle, secret)
    }

    pub(crate) fn delete_authorization_verifier(
        &self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<ConnectorCredentialDeleteOutcome, String> {
        self.credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?
            .delete(handle)
    }

    pub(crate) fn delete_authorization_review_authority(
        &self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<ConnectorCredentialDeleteOutcome, String> {
        self.credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?
            .delete(handle)
    }

    #[cfg(test)]
    pub(crate) fn contains_credential(
        &self,
        handle: &ConnectorCredentialHandle,
    ) -> Result<bool, String> {
        Ok(self
            .credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?
            .contains(handle))
    }

    #[cfg(test)]
    pub(crate) fn replace_credential_for_test(
        &self,
        handle: &ConnectorCredentialHandle,
        secret: ConnectorSecret,
    ) -> Result<(), String> {
        self.credential_store
            .lock()
            .map_err(|_| "connector credential store lock failed".to_string())?
            .replace(handle, secret)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorCapability {
    MailSearch,
    MailReadThread,
    MailReadAttachment,
    MailSyncInbox,
    MailCreateDraft,
    MailSendDraft,
    CalendarListEvents,
    CalendarSyncEvents,
    CalendarFindFreeTime,
    CalendarCreateEvent,
    CalendarUpdateEvent,
    CalendarCancelEvent,
}

impl ConnectorCapability {
    pub fn external_mutation(self) -> bool {
        matches!(
            self,
            Self::MailSendDraft
                | Self::CalendarCreateEvent
                | Self::CalendarUpdateEvent
                | Self::CalendarCancelEvent
        )
    }

    fn from_contract_name(value: &str) -> Option<Self> {
        match value {
            "mail_send_draft" => Some(Self::MailSendDraft),
            "calendar_create_event" => Some(Self::CalendarCreateEvent),
            "calendar_update_event" => Some(Self::CalendarUpdateEvent),
            "calendar_cancel_event" => Some(Self::CalendarCancelEvent),
            _ => None,
        }
    }

    pub(crate) fn contract_name(self) -> &'static str {
        match self {
            Self::MailSearch => "mail_search",
            Self::MailReadThread => "mail_read_thread",
            Self::MailReadAttachment => "mail_read_attachment",
            Self::MailSyncInbox => "mail_sync_inbox",
            Self::MailCreateDraft => "mail_create_draft",
            Self::MailSendDraft => "mail_send_draft",
            Self::CalendarListEvents => "calendar_list_events",
            Self::CalendarSyncEvents => "calendar_sync_events",
            Self::CalendarFindFreeTime => "calendar_find_free_time",
            Self::CalendarCreateEvent => "calendar_create_event",
            Self::CalendarUpdateEvent => "calendar_update_event",
            Self::CalendarCancelEvent => "calendar_cancel_event",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorHealth {
    Connected,
    NeedsRepair,
    DisconnectPending,
    Disconnected,
    RevocationPending,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorAccount {
    pub id: Uuid,
    pub provider_id: String,
    pub display_name: String,
    pub tenant_ref: Option<String>,
    pub credential_handle: ConnectorCredentialHandle,
    pub granted_capabilities: Vec<ConnectorCapability>,
    pub health: ConnectorHealth,
    pub connected_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorAccountSummary {
    pub id: Uuid,
    pub provider_id: String,
    pub display_name: String,
    pub granted_capabilities: Vec<ConnectorCapability>,
    pub health: ConnectorHealth,
    pub connected_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRecoveryKind {
    Attachment,
    Account,
    Sync,
    Reconciliation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRecoveryStatus {
    RepairRequired,
    NeedsRepair,
    DisconnectPending,
    RevocationPending,
    SyncExhausted,
    ReconciliationRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRecoveryReasonCode {
    AttachmentLegacyWorkspaceUnbound,
    AttachmentLegacyReceiptIncomplete,
    AttachmentRetentionIdentityChanged,
    AttachmentStoredIdentityChanged,
    AttachmentExecutionRecordIncomplete,
    AttachmentRecoveryRequired,
    AccountNeedsRepair,
    AccountDisconnectPending,
    AccountRevocationPending,
    SyncRetryExhausted,
    ReconciliationRequired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRecoveryExternalEffectState {
    LocalFilePreserved,
    NoExternalWrite,
    LocalCredentialRemovalPending,
    ExternalResultUncertain,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRecoveryNextStepCode {
    RetryLocalCleanup,
    InspectFileManually,
    ReviewAccountConnection,
    WaitForLocalDisconnectRecovery,
    RepairAccountConnection,
    VerifyProviderState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRecoverySyncCapability {
    Mail,
    Calendar,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ConnectorRecoveryAction {
    RetryAttachmentCleanup { action_revision: String },
    ResumeSync { action_revision: String },
    InspectExternalResult { action_revision: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorRecoveryAcceptance {
    Accepted,
    AlreadyAccepted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ConnectorRecoveryItem {
    pub id: Uuid,
    pub kind: ConnectorRecoveryKind,
    pub status: ConnectorRecoveryStatus,
    pub title: String,
    pub reason_code: ConnectorRecoveryReasonCode,
    pub external_effect_state: ConnectorRecoveryExternalEffectState,
    pub next_step_code: ConnectorRecoveryNextStepCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_capability: Option<ConnectorRecoverySyncCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<ConnectorRecoveryAction>,
    pub updated_at: DateTime<Utc>,
}

pub(crate) struct ConnectorDisconnectTicket {
    account: ConnectorAccount,
    generation: u64,
}

impl ConnectorDisconnectTicket {
    pub(crate) fn new(account: ConnectorAccount, generation: u64) -> Self {
        Self {
            account,
            generation,
        }
    }

    pub(crate) fn account(&self) -> &ConnectorAccount {
        &self.account
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectorDisconnectPhase {
    Started,
    CredentialDeleteFailed,
    Completed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectorDisconnectSource {
    User,
    Startup,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ConnectorDisconnectReceipt {
    pub account_id: Uuid,
    pub provider_id: String,
    pub generation: u64,
    pub phase: ConnectorDisconnectPhase,
    pub source: ConnectorDisconnectSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_delete_outcome: Option<ConnectorCredentialDeleteOutcome>,
    pub changed_at: DateTime<Utc>,
}

impl From<&ConnectorAccount> for ConnectorAccountSummary {
    fn from(account: &ConnectorAccount) -> Self {
        Self {
            id: account.id,
            provider_id: account.provider_id.clone(),
            display_name: account.display_name.clone(),
            granted_capabilities: account.granted_capabilities.clone(),
            health: account.health,
            connected_at: account.connected_at,
            updated_at: account.updated_at,
        }
    }
}

pub fn connector_context_summary(accounts: &[ConnectorAccount]) -> String {
    let mut summaries = accounts
        .iter()
        .map(|account| {
            format!(
                "provider_label={:?} health={:?} abilities={}",
                catalog::provider_label(&account.provider_id),
                account.health,
                catalog::user_abilities(&account.granted_capabilities).len()
            )
        })
        .collect::<Vec<_>>();
    summaries.sort();
    summaries.join("\n")
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorEvidenceRef {
    pub provider_id: String,
    pub account_id: Uuid,
    pub remote_object_ref: String,
    pub retrieved_at: DateTime<Utc>,
    pub bounded_summary: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorInvocationStatus {
    PendingApproval,
    Running,
    Succeeded,
    Failed,
    ReconciliationRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorMutationEnvelope {
    pub provider_id: String,
    pub account_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_generation: Option<u64>,
    pub capability: ConnectorCapability,
    pub target_ref: String,
    pub preview_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<ConnectorMutationIntent>,
    pub idempotency_key: String,
    pub automation_run_id: Uuid,
    pub agent_run_id: Option<Uuid>,
    pub access_mode: crate::kernel::models::AccessMode,
}

impl ConnectorMutationEnvelope {
    fn tool_request(&self) -> Result<ToolExecutionRequest, String> {
        let account_generation = self.account_generation.ok_or_else(|| {
            "legacy connector mutation has no frozen account generation".to_string()
        })?;
        self.validate_intent_binding()?;
        let mut input = serde_json::json!({
            "provider_id": self.provider_id,
            "account_id": self.account_id.to_string(),
            "account_generation": account_generation,
            "capability": self.capability.contract_name(),
            "target_ref": self.target_ref,
            "preview_hash": self.preview_hash,
            "idempotency_key": self.idempotency_key,
            "automation_run_id": self.automation_run_id.to_string(),
        });
        if let Some(intent_hash) = &self.intent_hash {
            input["intent_hash"] = serde_json::Value::String(intent_hash.clone());
        }
        Ok(ToolExecutionRequest {
            tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            input,
            access_mode: self.access_mode,
            run_id: self.agent_run_id,
        })
    }

    pub(crate) fn validate_intent_binding(&self) -> Result<(), String> {
        match (&self.intent_hash, &self.intent) {
            (None, None) => Ok(()),
            (Some(expected), Some(intent))
                if intent.capability() == self.capability
                    && intent.target_ref() == self.target_ref
                    && intent.hash().as_deref() == Ok(expected.as_str()) =>
            {
                Ok(())
            }
            _ => Err("connector mutation intent does not match its frozen envelope".to_string()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorMutationReceipt {
    pub provider_id: String,
    pub account_id: Uuid,
    pub capability: ConnectorCapability,
    pub target_ref: String,
    pub request_fingerprint: String,
    pub idempotency_key: String,
    pub reconciled: bool,
    pub evidence: ConnectorEvidenceRef,
}

impl ConnectorMutationReceipt {
    fn applied(
        invocation: &ConnectorInvocation,
        evidence: ConnectorEvidenceRef,
        reconciled: bool,
    ) -> Result<Self, String> {
        let mutation = invocation
            .mutation
            .as_ref()
            .ok_or_else(|| "connector mutation envelope is missing".to_string())?;
        Ok(Self {
            provider_id: mutation.provider_id.clone(),
            account_id: mutation.account_id,
            capability: mutation.capability,
            target_ref: mutation.target_ref.clone(),
            request_fingerprint: invocation.request_fingerprint.clone(),
            idempotency_key: mutation.idempotency_key.clone(),
            reconciled,
            evidence,
        })
    }
}

pub fn connector_invocation_transition_allowed(
    from: ConnectorInvocationStatus,
    to: ConnectorInvocationStatus,
) -> bool {
    use ConnectorInvocationStatus::*;
    matches!(
        (from, to),
        (PendingApproval, Running)
            | (PendingApproval, Failed)
            | (Running, Succeeded)
            | (Running, Failed)
            | (Running, ReconciliationRequired)
            | (ReconciliationRequired, Succeeded)
            | (ReconciliationRequired, Failed)
    )
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectorInvocation {
    pub id: Uuid,
    pub provider_id: String,
    pub account_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_generation: Option<u64>,
    pub capability: ConnectorCapability,
    pub automation_run_id: Option<Uuid>,
    pub tool_invocation_id: Option<Uuid>,
    pub request_fingerprint: String,
    pub idempotency_key: String,
    #[serde(default)]
    pub mutation: Option<ConnectorMutationEnvelope>,
    pub status: ConnectorInvocationStatus,
    pub evidence: Vec<ConnectorEvidenceRef>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ConnectorInvocation {
    pub fn new(
        provider_id: String,
        account_id: Uuid,
        capability: ConnectorCapability,
        automation_run_id: Option<Uuid>,
        tool_invocation_id: Option<Uuid>,
        request_fingerprint: String,
        idempotency_key: String,
    ) -> Result<Self, String> {
        let provider_id = required_text(provider_id, "connector provider")?;
        let request_fingerprint = required_text(request_fingerprint, "request fingerprint")?;
        let idempotency_key = required_text(idempotency_key, "idempotency key")?;
        if capability.external_mutation() {
            return Err(
                "external connector mutation must be derived from its exact tool request"
                    .to_string(),
            );
        }
        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            provider_id,
            account_id,
            account_generation: None,
            capability,
            automation_run_id,
            tool_invocation_id,
            request_fingerprint,
            idempotency_key,
            mutation: None,
            status: ConnectorInvocationStatus::Running,
            evidence: Vec::new(),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn from_tool_request(
        request: &ToolExecutionRequest,
        tool_record: &ToolInvocationRecord,
    ) -> Result<Self, String> {
        if request.tool_id != CONNECTOR_MUTATE_TOOL_ID
            || tool_record.tool_id != CONNECTOR_MUTATE_TOOL_ID
            || tool_record.status != ToolExecutionStatus::WaitingForConfirmation
        {
            return Err("connector mutation requires a pending connector tool request".to_string());
        }
        let fingerprint = tool_request_fingerprint(request);
        if tool_record.request_fingerprint != fingerprint {
            return Err("connector tool record does not match the exact request".to_string());
        }
        let input = request
            .input
            .as_object()
            .ok_or_else(|| "connector mutation input must be an object".to_string())?;
        let text = |name: &str| {
            input
                .get(name)
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .ok_or_else(|| format!("connector mutation {name} is required"))
        };
        let provider_id = text("provider_id")?;
        let account_id = Uuid::parse_str(&text("account_id")?)
            .map_err(|_| "connector mutation account id is invalid".to_string())?;
        let account_generation = input
            .get("account_generation")
            .and_then(|value| value.as_u64())
            .ok_or_else(|| "connector mutation account generation is required".to_string())?;
        let capability = ConnectorCapability::from_contract_name(&text("capability")?)
            .filter(|capability| capability.external_mutation())
            .ok_or_else(|| "connector mutation capability is invalid".to_string())?;
        let target_ref = text("target_ref")?;
        let preview_hash = text("preview_hash")?;
        let intent_hash = input
            .get("intent_hash")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        if intent_hash.as_ref().is_some_and(|hash| {
            hash.len() != 72
                || !hash.starts_with("intent1:")
                || !hash[8..].bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err("connector mutation intent hash is invalid".to_string());
        }
        let idempotency_key = text("idempotency_key")?;
        let automation_run_id = Uuid::parse_str(&text("automation_run_id")?)
            .map_err(|_| "connector mutation automation run id is invalid".to_string())?;
        let mutation = ConnectorMutationEnvelope {
            provider_id: provider_id.clone(),
            account_id,
            account_generation: Some(account_generation),
            capability,
            target_ref,
            preview_hash,
            intent_hash,
            intent: None,
            idempotency_key: idempotency_key.clone(),
            automation_run_id,
            agent_run_id: request.run_id,
            access_mode: request.access_mode,
        };
        Ok(Self {
            id: Uuid::new_v4(),
            provider_id,
            account_id,
            account_generation: Some(account_generation),
            capability,
            automation_run_id: Some(automation_run_id),
            tool_invocation_id: Some(tool_record.id),
            request_fingerprint: fingerprint,
            idempotency_key,
            mutation: Some(mutation),
            status: ConnectorInvocationStatus::PendingApproval,
            evidence: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
    }

    pub fn bind_intent(mut self, intent: ConnectorMutationIntent) -> Result<Self, String> {
        let mutation = self
            .mutation
            .as_mut()
            .ok_or_else(|| "connector mutation envelope is missing".to_string())?;
        let intent_hash = intent.hash()?;
        if mutation.intent_hash.as_deref() != Some(intent_hash.as_str())
            || intent.capability() != mutation.capability
            || intent.target_ref() != mutation.target_ref
        {
            return Err(
                "connector mutation intent does not match the exact tool request".to_string(),
            );
        }
        mutation.intent = Some(intent);
        mutation.validate_intent_binding()?;
        Ok(self)
    }

    pub fn mutation_intent(&self) -> Result<&ConnectorMutationIntent, String> {
        let mutation = self
            .mutation
            .as_ref()
            .ok_or_else(|| "connector mutation envelope is missing".to_string())?;
        mutation.validate_intent_binding()?;
        mutation
            .intent
            .as_ref()
            .ok_or_else(|| "connector mutation intent is unavailable".to_string())
    }
}

pub trait ConnectorProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn capabilities(&self) -> &'static [ConnectorCapability];
}

pub trait ConnectorDraftProvider: ConnectorProvider {
    fn create_draft(
        &self,
        _account: &ConnectorAccount,
        _title: &str,
    ) -> Result<ConnectorEvidenceRef, String> {
        Err("connector draft capability is unavailable".to_string())
    }
}

pub trait ConnectorMutationProvider: ConnectorProvider {
    fn apply_mutation(
        &self,
        _account: &ConnectorAccount,
        _invocation: &ConnectorInvocation,
    ) -> Result<ConnectorMutationApplyOutcome, String> {
        Err("connector mutation capability is unavailable".to_string())
    }
}

pub trait ConnectorMutationReconciler: ConnectorProvider {
    fn reconcile_mutation(
        &self,
        _account: &ConnectorAccount,
        _invocation: &ConnectorInvocation,
    ) -> Result<ConnectorReconciliationOutcome, String> {
        Err("connector reconciliation capability is unavailable".to_string())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "the unboxed receipt is a public provider trait contract used across connector implementations; remove only with a versioned provider API migration and measured allocation and layout evidence"
)]
pub enum ConnectorMutationApplyOutcome {
    Applied(ConnectorMutationReceipt),
    ReconciliationRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::large_enum_variant,
    reason = "the unboxed receipt is a public provider trait contract used across connector implementations; remove only with a versioned provider API migration and measured allocation and layout evidence"
)]
pub enum ConnectorReconciliationOutcome {
    Applied(ConnectorMutationReceipt),
    KnownNotApplied,
    StillUncertain,
}

#[cfg(test)]
const FAKE_PROVIDER_CAPABILITIES: &[ConnectorCapability] = &[
    ConnectorCapability::MailSearch,
    ConnectorCapability::MailReadThread,
    ConnectorCapability::MailSyncInbox,
    ConnectorCapability::MailCreateDraft,
    ConnectorCapability::MailSendDraft,
    ConnectorCapability::CalendarListEvents,
    ConnectorCapability::CalendarSyncEvents,
    ConnectorCapability::CalendarCreateEvent,
];

#[cfg(test)]
#[derive(Default)]
pub struct FakeConnectorRemoteState {
    applied: Mutex<HashMap<(Uuid, String), ConnectorMutationReceipt>>,
}

#[cfg(test)]
pub struct FakeConnectorProvider {
    remote: Arc<FakeConnectorRemoteState>,
    timeout_after_next_apply: Arc<AtomicBool>,
}

#[cfg(test)]
impl Default for FakeConnectorProvider {
    fn default() -> Self {
        Self {
            remote: Arc::new(FakeConnectorRemoteState::default()),
            timeout_after_next_apply: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(test)]
impl FakeConnectorProvider {
    pub fn with_remote_state(remote: Arc<FakeConnectorRemoteState>) -> Self {
        Self {
            remote,
            timeout_after_next_apply: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn remote_state(&self) -> Arc<FakeConnectorRemoteState> {
        Arc::clone(&self.remote)
    }

    pub fn timeout_after_next_apply(&self) {
        self.timeout_after_next_apply.store(true, Ordering::SeqCst);
    }

    pub fn applied_count(&self) -> usize {
        self.remote
            .applied
            .lock()
            .map(|items| items.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
impl ConnectorProvider for FakeConnectorProvider {
    fn provider_id(&self) -> &'static str {
        "fake"
    }

    fn capabilities(&self) -> &'static [ConnectorCapability] {
        FAKE_PROVIDER_CAPABILITIES
    }
}

#[cfg(test)]
impl provider::MailConnectorProvider for FakeConnectorProvider {
    fn search_mail_page(
        &self,
        account: &ConnectorAccount,
        request: &provider::MailSearchRequest,
        _continuation: Option<&provider::ConnectorReadContinuation>,
    ) -> provider::ConnectorProviderResult<provider::ConnectorReadPage<domain::MailThread>> {
        validate_connector_invocation(account, self, ConnectorCapability::MailSearch)
            .map_err(|_| provider::ConnectorProviderFailure::PermissionDenied)?;
        let thread = fake_mail_thread("fake:thread:1", Utc::now());
        Ok(provider::ConnectorReadPage::new(
            if request.max_results() == 0 {
                Vec::new()
            } else {
                vec![thread]
            },
            None,
        ))
    }

    fn read_thread(
        &self,
        account: &ConnectorAccount,
        request: &provider::MailThreadRequest,
    ) -> provider::ConnectorProviderResult<domain::MailThread> {
        validate_connector_invocation(account, self, ConnectorCapability::MailReadThread)
            .map_err(|_| provider::ConnectorProviderFailure::PermissionDenied)?;
        Ok(fake_mail_thread(request.thread_ref(), Utc::now()))
    }
}

#[cfg(test)]
impl provider::CalendarConnectorProvider for FakeConnectorProvider {
    fn list_events_page(
        &self,
        account: &ConnectorAccount,
        request: &provider::CalendarListRequest,
        _continuation: Option<&provider::ConnectorReadContinuation>,
    ) -> provider::ConnectorProviderResult<provider::ConnectorReadPage<domain::CalendarEvent>> {
        validate_connector_invocation(account, self, ConnectorCapability::CalendarListEvents)
            .map_err(|_| provider::ConnectorProviderFailure::PermissionDenied)?;
        Ok(provider::ConnectorReadPage::new(
            vec![fake_calendar_event(request.starts_at(), request.ends_at())],
            None,
        ))
    }
}

#[cfg(test)]
impl sync::MailSyncProvider for FakeConnectorProvider {
    fn sync_mail_page(
        &self,
        account: &ConnectorAccount,
        _request: &sync::MailSyncRequest,
        _continuation: Option<&sync::ConnectorOpaqueContinuation>,
    ) -> provider::ConnectorProviderResult<sync::ConnectorSyncPage<domain::MailMessage>> {
        validate_connector_invocation(account, self, ConnectorCapability::MailSyncInbox)
            .map_err(|_| provider::ConnectorProviderFailure::PermissionDenied)?;
        Ok(sync::ConnectorSyncPage::new(
            Vec::new(),
            sync::ConnectorSyncContinuation::Delta(
                sync::ConnectorOpaqueContinuation::new("fake:mail:delta:1".to_string())
                    .map_err(|_| provider::ConnectorProviderFailure::InvalidResponse)?,
            ),
        ))
    }
}

#[cfg(test)]
impl sync::CalendarSyncProvider for FakeConnectorProvider {
    fn sync_calendar_page(
        &self,
        account: &ConnectorAccount,
        _request: &sync::CalendarSyncRequest,
        _continuation: Option<&sync::ConnectorOpaqueContinuation>,
    ) -> provider::ConnectorProviderResult<sync::ConnectorSyncPage<domain::CalendarEvent>> {
        validate_connector_invocation(account, self, ConnectorCapability::CalendarSyncEvents)
            .map_err(|_| provider::ConnectorProviderFailure::PermissionDenied)?;
        Ok(sync::ConnectorSyncPage::new(
            Vec::new(),
            sync::ConnectorSyncContinuation::Delta(
                sync::ConnectorOpaqueContinuation::new("fake:calendar:delta:1".to_string())
                    .map_err(|_| provider::ConnectorProviderFailure::InvalidResponse)?,
            ),
        ))
    }
}

#[cfg(test)]
impl ConnectorDraftProvider for FakeConnectorProvider {
    fn create_draft(
        &self,
        account: &ConnectorAccount,
        title: &str,
    ) -> Result<ConnectorEvidenceRef, String> {
        validate_connector_invocation(account, self, ConnectorCapability::MailCreateDraft)?;
        let title = required_text(title.to_string(), "draft title")?;
        Ok(ConnectorEvidenceRef {
            provider_id: self.provider_id().to_string(),
            account_id: account.id,
            remote_object_ref: format!("fake:draft:{}", stable_text_key(&title)),
            retrieved_at: Utc::now(),
            bounded_summary: Some("A reviewable draft was created.".to_string()),
        })
    }
}

#[cfg(test)]
impl ConnectorMutationProvider for FakeConnectorProvider {
    fn apply_mutation(
        &self,
        account: &ConnectorAccount,
        invocation: &ConnectorInvocation,
    ) -> Result<ConnectorMutationApplyOutcome, String> {
        if account.health != ConnectorHealth::Connected
            || account.provider_id != self.provider_id()
            || invocation.account_id != account.id
            || invocation.provider_id != self.provider_id()
            || !invocation.capability.external_mutation()
            || invocation.status != ConnectorInvocationStatus::Running
        {
            return Err("connector mutation is not ready".to_string());
        }
        let mut applied = self
            .remote
            .applied
            .lock()
            .map_err(|_| "fake provider state lock failed".to_string())?;
        let receipt = applied
            .entry((account.id, invocation.idempotency_key.clone()))
            .or_insert_with(|| {
                ConnectorMutationReceipt::applied(
                    invocation,
                    ConnectorEvidenceRef {
                        provider_id: self.provider_id().to_string(),
                        account_id: account.id,
                        remote_object_ref: format!(
                            "fake:mutation:{}",
                            stable_text_key(&invocation.idempotency_key)
                        ),
                        retrieved_at: Utc::now(),
                        bounded_summary: Some("One external mutation was applied.".to_string()),
                    },
                    false,
                )
                .expect("running fake mutation has a frozen envelope")
            })
            .clone();
        if self.timeout_after_next_apply.swap(false, Ordering::SeqCst) {
            return Ok(ConnectorMutationApplyOutcome::ReconciliationRequired);
        }
        Ok(ConnectorMutationApplyOutcome::Applied(receipt))
    }
}

#[cfg(test)]
impl ConnectorMutationReconciler for FakeConnectorProvider {
    fn reconcile_mutation(
        &self,
        account: &ConnectorAccount,
        invocation: &ConnectorInvocation,
    ) -> Result<ConnectorReconciliationOutcome, String> {
        if account.health != ConnectorHealth::Connected {
            return Err("connector account is not connected".to_string());
        }
        let applied = self
            .remote
            .applied
            .lock()
            .map_err(|_| "fake provider state lock failed".to_string())?;
        Ok(
            match applied
                .get(&(account.id, invocation.idempotency_key.clone()))
                .cloned()
            {
                Some(mut receipt) => {
                    receipt.reconciled = true;
                    ConnectorReconciliationOutcome::Applied(receipt)
                }
                None => ConnectorReconciliationOutcome::KnownNotApplied,
            },
        )
    }
}

#[cfg(test)]
fn fake_mail_thread(thread_ref: &str, now: DateTime<Utc>) -> domain::MailThread {
    let address = domain::MailAddress {
        display_name: Some("Fake sender".to_string()),
        address: "sender@example.com".to_string(),
    };
    domain::MailThread {
        remote_ref: thread_ref.to_string(),
        messages: vec![domain::MailMessage {
            remote_ref: format!("{thread_ref}:message:1"),
            thread_ref: thread_ref.to_string(),
            from: address.clone(),
            to: vec![address],
            subject: "Untrusted fake mail".to_string(),
            received_at: now,
            bounded_body_summary: Some("Untrusted fake provider evidence.".to_string()),
            attachments: Vec::new(),
            has_attachments: false,
            untrusted_evidence: true,
        }],
    }
}

#[cfg(test)]
fn fake_calendar_event(starts_at: DateTime<Utc>, ends_at: DateTime<Utc>) -> domain::CalendarEvent {
    let event_ends_at = std::cmp::min(ends_at, starts_at + chrono::Duration::hours(1));
    domain::CalendarEvent {
        remote_ref: "fake:event:1".to_string(),
        calendar_ref: "fake:calendar:1".to_string(),
        title: "Untrusted fake calendar event".to_string(),
        starts_at,
        ends_at: event_ends_at,
        timezone: "UTC".to_string(),
        attendees: Vec::new(),
        meeting_url: None,
        recurrence: None,
        untrusted_evidence: true,
    }
}

#[cfg(test)]
fn stable_text_key(value: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(value.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn validate_connector_invocation(
    account: &ConnectorAccount,
    provider: &dyn ConnectorProvider,
    capability: ConnectorCapability,
) -> Result<(), String> {
    if account.health != ConnectorHealth::Connected {
        return Err("connector account is not connected".to_string());
    }
    if account.provider_id != provider.provider_id() {
        return Err("connector provider does not own this account".to_string());
    }
    if !account.granted_capabilities.contains(&capability)
        || !provider.capabilities().contains(&capability)
    {
        return Err("connector capability is not granted".to_string());
    }
    if capability.external_mutation() {
        return Err("external connector mutation requires exact tool approval".to_string());
    }
    Ok(())
}

pub(crate) fn validate_connector_mutation_invocation(
    account: &ConnectorAccount,
    provider: &dyn ConnectorProvider,
    invocation: &ConnectorInvocation,
) -> Result<(), String> {
    if account.health != ConnectorHealth::Connected
        || account.provider_id != provider.provider_id()
        || invocation.account_id != account.id
        || invocation.provider_id != provider.provider_id()
        || !invocation.capability.external_mutation()
        || invocation.status != ConnectorInvocationStatus::Running
        || !account
            .granted_capabilities
            .contains(&invocation.capability)
        || !provider.capabilities().contains(&invocation.capability)
    {
        return Err("connector mutation is not ready".to_string());
    }
    let intent = invocation.mutation_intent()?;
    if intent.capability() != invocation.capability {
        return Err("connector mutation intent capability changed".to_string());
    }
    Ok(())
}

pub fn bind_connector_invocation_to_tool_record(
    invocation: &ConnectorInvocation,
    tool_record: &ToolInvocationRecord,
) -> Result<(), String> {
    bind_connector_invocation_to_tool_record_with_status(
        invocation,
        tool_record,
        ToolExecutionStatus::WaitingForConfirmation,
        "connector mutation is not bound to its exact pending approval",
    )
}

pub(crate) fn bind_running_connector_invocation_to_tool_record(
    invocation: &ConnectorInvocation,
    tool_record: &ToolInvocationRecord,
) -> Result<(), String> {
    bind_connector_invocation_to_tool_record_with_status(
        invocation,
        tool_record,
        ToolExecutionStatus::Running,
        "connector reconciliation is not bound to its exact running Tool",
    )
}

fn bind_connector_invocation_to_tool_record_with_status(
    invocation: &ConnectorInvocation,
    tool_record: &ToolInvocationRecord,
    expected_status: ToolExecutionStatus,
    error: &str,
) -> Result<(), String> {
    if !invocation.capability.external_mutation() {
        return Err("connector invocation is not an external mutation".to_string());
    }
    let expected_fingerprint = invocation
        .mutation
        .as_ref()
        .map(ConnectorMutationEnvelope::tool_request)
        .transpose()?
        .map(|request| tool_request_fingerprint(&request));
    if invocation.tool_invocation_id != Some(tool_record.id)
        || invocation.request_fingerprint != tool_record.request_fingerprint
        || expected_fingerprint.as_deref() != Some(invocation.request_fingerprint.as_str())
        || invocation.mutation.as_ref().is_some_and(|mutation| {
            mutation.provider_id != invocation.provider_id
                || mutation.account_id != invocation.account_id
                || mutation.account_generation != invocation.account_generation
                || mutation.capability != invocation.capability
                || mutation.idempotency_key != invocation.idempotency_key
                || Some(mutation.automation_run_id) != invocation.automation_run_id
        })
        || tool_record.capability != CapabilityKind::ConnectorWrite
        || tool_record.status != expected_status
        || tool_record.approval_request_id.is_none()
    {
        return Err(error.to_string());
    }
    Ok(())
}

fn required_text(value: String, field: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{field} is required"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::agent_run::{AgentRunFinish, AgentRunStatus, AgentRunTransition};
    use crate::kernel::automation::{
        AutomationCheckpoint, AutomationDefinition, AutomationRunStatus, ReviewQueueItem,
        ReviewQueueItemStatus,
    };
    use crate::kernel::connectors::provider::{MailConnectorProvider, MailSearchRequest};
    use crate::kernel::connectors::reconciliation::{
        reconcile_due_connector_mutations, ConnectorReconcilerRegistry,
    };
    use crate::kernel::event_store::EventStore;
    use crate::kernel::models::AccessMode;
    use crate::kernel::policy::{request_capability_access, CapabilityKind};
    use crate::kernel::tool_runtime::{
        prepare_tool_execution, ToolExecutionRequest, ToolInvocationRecord,
        CONNECTOR_MUTATE_TOOL_ID,
    };
    use chrono::Duration;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const FAKE_CAPABILITIES: &[ConnectorCapability] = &[
        ConnectorCapability::MailSearch,
        ConnectorCapability::MailReadThread,
        ConnectorCapability::MailSendDraft,
    ];

    struct FakeProvider;

    impl ConnectorProvider for FakeProvider {
        fn provider_id(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> &'static [ConnectorCapability] {
            FAKE_CAPABILITIES
        }
    }

    struct SingleFakeReconciler<'a>(&'a FakeConnectorProvider);

    impl ConnectorReconcilerRegistry for SingleFakeReconciler<'_> {
        fn reconciler(&self, provider_id: &str) -> Option<&dyn ConnectorMutationReconciler> {
            (provider_id == self.0.provider_id()).then_some(self.0)
        }
    }

    fn account(handle: ConnectorCredentialHandle) -> ConnectorAccount {
        let now = Utc::now();
        ConnectorAccount {
            id: Uuid::new_v4(),
            provider_id: "fake".to_string(),
            display_name: "Test account".to_string(),
            tenant_ref: None,
            credential_handle: handle,
            granted_capabilities: FAKE_CAPABILITIES.to_vec(),
            health: ConnectorHealth::Connected,
            connected_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn credential_store_returns_only_opaque_serializable_handle() {
        let marker = "refresh-token-must-never-serialize";
        let mut store = FakeConnectorCredentialStore::default();
        let handle = store
            .put(ConnectorSecret::new(marker.to_string()).expect("secret is valid"))
            .expect("secret stores");
        let account_json =
            serde_json::to_string(&account(handle.clone())).expect("account serializes");
        assert!(!account_json.contains(marker));
        assert!(!account_json.contains("refresh_token"));
        assert!(store.contains(&handle));
        store.delete(&handle).expect("secret deletes");
        assert!(!store.contains(&handle));
    }

    #[test]
    fn fake_provider_reads_typed_bounded_evidence_and_fails_closed_for_mutation() {
        let mut store = FakeConnectorCredentialStore::default();
        let handle = store
            .put(ConnectorSecret::new("fake-token".to_string()).expect("secret is valid"))
            .expect("secret stores");
        let account = account(handle);
        let provider = FakeConnectorProvider::default();
        let request = MailSearchRequest::new("contract".to_string(), 1).unwrap();
        let evidence = provider
            .search_mail(&account, &request)
            .expect("read succeeds");
        assert_eq!(evidence.len(), 1);
        assert!(validate_connector_invocation(
            &account,
            &provider,
            ConnectorCapability::MailSendDraft
        )
        .is_err());
    }

    #[test]
    fn disconnected_and_unknown_capabilities_fail_closed() {
        let mut account = account(ConnectorCredentialHandle::new());
        let provider = FakeConnectorProvider::default();
        let request = MailSearchRequest::new("contract".to_string(), 1).unwrap();
        account.health = ConnectorHealth::Disconnected;
        assert!(provider.search_mail(&account, &request).is_err());
        account.health = ConnectorHealth::Connected;
        assert!(validate_connector_invocation(
            &account,
            &provider,
            ConnectorCapability::CalendarListEvents
        )
        .is_err());
    }

    #[test]
    fn external_mutation_requires_exact_tool_invocation_and_waits_for_approval() {
        assert!(ConnectorInvocation::new(
            "fake".to_string(),
            Uuid::new_v4(),
            ConnectorCapability::MailSendDraft,
            Some(Uuid::new_v4()),
            None,
            "sha256:frozen-preview".to_string(),
            "send:one-time-key".to_string(),
        )
        .is_err());
        let request = ToolExecutionRequest {
            tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            input: json!({
                "provider_id": "fake",
                "account_id": Uuid::new_v4().to_string(),
                "account_generation": 0,
                "capability": "mail_send_draft",
                "target_ref": "draft:one",
                "preview_hash": "sha256:frozen-preview",
                "idempotency_key": "send:one-time-key",
                "automation_run_id": Uuid::new_v4().to_string()
            }),
            access_mode: AccessMode::FullAccess,
            run_id: Some(Uuid::new_v4()),
        };
        let plan = prepare_tool_execution(&request).expect("tool plan is valid");
        let tool = ToolInvocationRecord::waiting_for_confirmation(&plan, Uuid::new_v4());
        let invocation = ConnectorInvocation::from_tool_request(&request, &tool)
            .expect("exact mutation invocation is valid");
        assert_eq!(
            invocation.status,
            ConnectorInvocationStatus::PendingApproval
        );
    }

    struct CountingRefresher {
        calls: AtomicUsize,
    }

    impl ConnectorCredentialRefresher for CountingRefresher {
        fn needs_refresh(&self, current: &ConnectorSecret) -> bool {
            current.expose() == "expired"
        }

        fn refresh(&self, _current: &ConnectorSecret) -> Result<ConnectorSecret, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(20));
            ConnectorSecret::new("fresh".to_string())
        }
    }

    #[test]
    fn concurrent_refresh_is_single_flight_per_account() {
        let mut store = FakeConnectorCredentialStore::default();
        let handle = store
            .put(ConnectorSecret::new("expired".to_string()).expect("secret is valid"))
            .expect("secret stores");
        let account = Arc::new(account(handle));
        let runtime = Arc::new(ConnectorRuntime::new(store));
        let refresher = Arc::new(CountingRefresher {
            calls: AtomicUsize::new(0),
        });
        let threads = (0..4)
            .map(|_| {
                let runtime = Arc::clone(&runtime);
                let account = Arc::clone(&account);
                let refresher = Arc::clone(&refresher);
                std::thread::spawn(move || {
                    runtime
                        .refresh_account_credential(&account, refresher.as_ref())
                        .expect("refresh succeeds")
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().expect("thread finishes");
        }
        assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
    }

    struct BlockingRefresher {
        entered: Arc<std::sync::Barrier>,
        release: Arc<std::sync::Barrier>,
    }

    impl ConnectorCredentialRefresher for BlockingRefresher {
        fn needs_refresh(&self, _current: &ConnectorSecret) -> bool {
            true
        }

        fn refresh(&self, _current: &ConnectorSecret) -> Result<ConnectorSecret, String> {
            self.entered.wait();
            self.release.wait();
            ConnectorSecret::new("fresh-after-wait".to_string())
        }
    }

    #[test]
    fn disconnect_waits_for_inflight_refresh_and_prevents_credential_resurrection() {
        let mut store = FakeConnectorCredentialStore::default();
        let handle = store
            .put(ConnectorSecret::new("expired".to_string()).expect("secret is valid"))
            .expect("secret stores");
        let account = account(handle);
        let runtime = Arc::new(ConnectorRuntime::new(store));
        let entered = Arc::new(std::sync::Barrier::new(2));
        let release = Arc::new(std::sync::Barrier::new(2));
        let refresher = Arc::new(BlockingRefresher {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        });

        let refresh_thread = {
            let runtime = Arc::clone(&runtime);
            let account = account.clone();
            let refresher = Arc::clone(&refresher);
            std::thread::spawn(move || {
                runtime.refresh_account_credential(&account, refresher.as_ref())
            })
        };
        entered.wait();
        let disconnect_thread = {
            let runtime = Arc::clone(&runtime);
            let mut account = account.clone();
            std::thread::spawn(move || runtime.disconnect_account(&mut account))
        };
        release.wait();
        refresh_thread
            .join()
            .expect("refresh thread finishes")
            .expect("refresh succeeds before disconnect");
        disconnect_thread
            .join()
            .expect("disconnect thread finishes")
            .expect("disconnect succeeds");
        assert!(runtime
            .with_account_credential(&account.credential_handle, |secret| {
                Ok::<_, String>((secret.expose().to_string(), None))
            })
            .is_err());
    }

    #[test]
    fn disconnect_deletes_credential_and_fails_closed() {
        let mut store = FakeConnectorCredentialStore::default();
        let handle = store
            .put(ConnectorSecret::new("token".to_string()).expect("secret is valid"))
            .expect("secret stores");
        let mut account = account(handle);
        let runtime = ConnectorRuntime::new(store);
        runtime
            .disconnect_account(&mut account)
            .expect("disconnect succeeds");
        assert_eq!(account.health, ConnectorHealth::Disconnected);
        assert!(validate_connector_invocation(
            &account,
            &FakeProvider,
            ConnectorCapability::MailSearch
        )
        .is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_credential_store_round_trip_and_delete() {
        let marker = format!("ds-agent-credential-test:{}", Uuid::new_v4());
        let temp_dir = tempfile::tempdir().expect("temporary connector vault creates");
        let mut store = WindowsConnectorCredentialStore::new(temp_dir.path())
            .expect("Windows connector vault initializes");
        let handle = store
            .put(
                ConnectorSecret::new(format!("{marker}{}", "x".repeat(16 * 1024)))
                    .expect("secret is valid"),
            )
            .expect("Windows credential stores");
        let loaded = store.read(&handle).expect("Windows credential reads");
        assert!(loaded.expose().starts_with(&marker));
        let protected = fs::read(store.credential_path(&handle)).expect("ciphertext reads");
        assert!(!protected
            .windows(marker.len())
            .any(|bytes| bytes == marker.as_bytes()));
        store
            .replace(
                &handle,
                ConnectorSecret::new("replacement".to_string()).expect("replacement is valid"),
            )
            .expect("Windows credential replaces atomically");
        assert_eq!(
            store.read(&handle).expect("replacement reads").expose(),
            "replacement"
        );
        let other_handle = ConnectorCredentialHandle::new();
        store
            .put_at(
                &other_handle,
                ConnectorSecret::new("other-secret".to_string()).unwrap(),
            )
            .unwrap();
        fs::copy(
            store.credential_path(&handle),
            store.credential_path(&other_handle),
        )
        .expect("ciphertext swap copies");
        assert!(store.read(&other_handle).is_err());
        store.delete(&other_handle).unwrap();
        assert_eq!(
            store.delete(&handle).expect("Windows credential deletes"),
            ConnectorCredentialDeleteOutcome::Deleted
        );
        assert_eq!(
            store.delete(&handle).expect("repeat delete is idempotent"),
            ConnectorCredentialDeleteOutcome::AlreadyAbsent
        );
        assert!(!store.contains(&handle));
        let oversized_handle = ConnectorCredentialHandle::new();
        assert!(store
            .put_at(
                &oversized_handle,
                ConnectorSecret::new("x".repeat(CONNECTOR_VAULT_MAX_PLAINTEXT_BYTES + 1))
                    .expect("oversized secret builds"),
            )
            .is_err());
        assert!(!store.contains(&oversized_handle));
        assert!(fs::read_dir(temp_dir.path())
            .expect("vault lists")
            .all(|entry| !entry
                .expect("vault entry reads")
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")));

        let staged = temp_dir.path().join(format!(".{}.tmp", Uuid::new_v4()));
        fs::write(&staged, b"orphan-protected-marker").unwrap();
        WindowsConnectorCredentialStore::new(temp_dir.path())
            .expect("vault startup removes bounded staged residue");
        assert!(!staged.exists());
    }

    #[test]
    fn provider_errors_are_typed_without_provider_secret_text() {
        let error = provider::ConnectorProviderFailure::InvalidResponse.to_string();
        assert!(!error.contains("marker-refresh-token"));
        assert_eq!(error, "connector provider returned an invalid response");
    }

    #[test]
    fn connector_mutation_binds_to_exact_pending_tool_approval() {
        let run_id = Uuid::new_v4();
        let account_id = Uuid::new_v4();
        let automation_run_id = Uuid::new_v4();
        let request = ToolExecutionRequest {
            tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            input: json!({
                "provider_id": "fake",
                "account_id": account_id.to_string(),
                "account_generation": 0,
                "capability": "mail_send_draft",
                "target_ref": "draft:42",
                "preview_hash": "sha256:preview",
                "idempotency_key": "send:42:once",
                "automation_run_id": automation_run_id.to_string()
            }),
            access_mode: AccessMode::FullAccess,
            run_id: Some(run_id),
        };
        let plan = prepare_tool_execution(&request).expect("tool plan is valid");
        let approval_id = Uuid::new_v4();
        let tool_record = ToolInvocationRecord::waiting_for_confirmation(&plan, approval_id);
        let invocation = ConnectorInvocation::from_tool_request(&request, &tool_record)
            .expect("connector invocation is valid");
        assert_eq!(invocation.account_generation, Some(0));
        bind_connector_invocation_to_tool_record(&invocation, &tool_record)
            .expect("exact approval binds");

        let mut changed = invocation;
        changed.request_fingerprint = "sha256:changed-preview".to_string();
        assert!(bind_connector_invocation_to_tool_record(&changed, &tool_record).is_err());

        let invocation = ConnectorInvocation::from_tool_request(&request, &tool_record)
            .expect("connector invocation is valid");
        let mut changed = invocation.clone();
        changed.account_id = Uuid::new_v4();
        assert!(bind_connector_invocation_to_tool_record(&changed, &tool_record).is_err());
        let mut changed = invocation.clone();
        changed.account_generation = Some(1);
        assert!(bind_connector_invocation_to_tool_record(&changed, &tool_record).is_err());
        let mut changed = invocation;
        changed
            .mutation
            .as_mut()
            .expect("mutation envelope exists")
            .target_ref = "draft:other".to_string();
        assert!(bind_connector_invocation_to_tool_record(&changed, &tool_record).is_err());

        let mut legacy_request = request;
        legacy_request
            .input
            .as_object_mut()
            .expect("input is an object")
            .remove("account_generation");
        assert!(prepare_tool_execution(&legacy_request).is_err());
    }

    #[test]
    fn one_shot_connector_approval_is_reserved_when_execution_starts() {
        let store = EventStore::open_memory().expect("store opens");
        let mut account = account(ConnectorCredentialHandle::new());
        account.granted_capabilities = FAKE_PROVIDER_CAPABILITIES.to_vec();
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let access_request =
            request_capability_access(AccessMode::FullAccess, CapabilityKind::ConnectorWrite)
                .expect("connector write requires approval");
        store
            .append_capability_access_request(&access_request)
            .expect("approval request persists");

        let mut invocation_ids = Vec::new();
        for sequence in 0..2 {
            let definition = AutomationDefinition::once(
                format!("Approval reservation {sequence}"),
                "UTC".to_string(),
                Utc::now() - Duration::minutes(1),
            )
            .expect("definition is valid");
            store
                .upsert_automation_definition(&definition)
                .expect("definition persists");
            let automation_run = store
                .claim_due_automation_run(definition.id, Utc::now(), format!("worker-{sequence}"))
                .expect("claim succeeds")
                .expect("run is due");
            let request = ToolExecutionRequest {
                tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
                input: json!({
                    "provider_id": "fake",
                    "account_id": account.id.to_string(),
                    "account_generation": 0,
                    "capability": "mail_send_draft",
                    "target_ref": format!("draft:{sequence}"),
                    "preview_hash": format!("sha256:preview-{sequence}"),
                    "idempotency_key": format!("send:{sequence}:once"),
                    "automation_run_id": automation_run.id.to_string()
                }),
                access_mode: AccessMode::FullAccess,
                run_id: Some(Uuid::new_v4()),
            };
            let plan = prepare_tool_execution(&request).expect("tool plan is valid");
            let tool = ToolInvocationRecord::waiting_for_confirmation(&plan, access_request.id);
            let invocation = ConnectorInvocation::from_tool_request(&request, &tool)
                .expect("connector invocation is valid");
            let mut review = ReviewQueueItem {
                id: Uuid::new_v4(),
                automation_run_id: automation_run.id,
                agent_run_id: request.run_id,
                tool_invocation_id: None,
                status: ReviewQueueItemStatus::PendingReview,
                preview_fingerprint: Some(tool.request_fingerprint.clone()),
                revision: 0,
                title: format!("Review mutation {sequence}"),
                evidence_ref: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            review
                .request_approval(tool.id, tool.request_fingerprint.clone(), Utc::now())
                .expect("review binds exact tool");
            store.append_tool_invocation(&tool).expect("tool persists");
            store
                .upsert_review_queue_item(&review)
                .expect("review persists");
            assert!(store
                .append_connector_invocation(&invocation)
                .expect("invocation persists"));
            invocation_ids.push(invocation.id);
        }
        store
            .resolve_capability_access_request(
                access_request.id,
                true,
                "Approve one exact connector mutation".to_string(),
            )
            .expect("approval persists");
        store
            .start_approved_connector_invocation(invocation_ids[0], Utc::now())
            .expect("first exact mutation reserves approval");
        assert!(store
            .start_approved_connector_invocation(invocation_ids[1], Utc::now())
            .is_err());
        assert_eq!(
            store
                .list_capability_access_records()
                .expect("permission records load")
                .into_iter()
                .find(|record| record.request.id == access_request.id)
                .expect("approval exists")
                .grant_state,
            crate::kernel::policy::CapabilityGrantState::OneShotConsumed
        );
    }

    #[test]
    fn account_generation_change_invalidates_frozen_mutation_before_approval_consumption() {
        let store = EventStore::open_memory().expect("store opens");
        let mut account = account(ConnectorCredentialHandle::new());
        account.granted_capabilities = FAKE_PROVIDER_CAPABILITIES.to_vec();
        store
            .upsert_connector_account(&account)
            .expect("account persists");
        let definition = AutomationDefinition::once(
            "Generation-bound mutation".to_string(),
            "UTC".to_string(),
            Utc::now() - Duration::minutes(1),
        )
        .expect("definition is valid");
        store
            .upsert_automation_definition(&definition)
            .expect("definition persists");
        let automation_run_id = store
            .claim_due_automation_run(definition.id, Utc::now(), "generation-test".to_string())
            .expect("run claim succeeds")
            .expect("run is due")
            .id;
        let access_request =
            request_capability_access(AccessMode::FullAccess, CapabilityKind::ConnectorWrite)
                .expect("connector write requires approval");
        let request = ToolExecutionRequest {
            tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            input: json!({
                "provider_id": "fake",
                "account_id": account.id.to_string(),
                "account_generation": 0,
                "capability": "mail_send_draft",
                "target_ref": "draft:generation-zero",
                "preview_hash": "sha256:generation-zero",
                "idempotency_key": "send:generation-zero:once",
                "automation_run_id": automation_run_id.to_string()
            }),
            access_mode: AccessMode::FullAccess,
            run_id: Some(Uuid::new_v4()),
        };
        let plan = prepare_tool_execution(&request).expect("tool plan is valid");
        let tool = ToolInvocationRecord::waiting_for_confirmation(&plan, access_request.id);
        let invocation = ConnectorInvocation::from_tool_request(&request, &tool)
            .expect("connector invocation is valid");
        let mut review = ReviewQueueItem {
            id: Uuid::new_v4(),
            automation_run_id,
            agent_run_id: request.run_id,
            tool_invocation_id: None,
            status: ReviewQueueItemStatus::PendingReview,
            preview_fingerprint: Some(tool.request_fingerprint.clone()),
            revision: 0,
            title: "Review generation-bound mutation".to_string(),
            evidence_ref: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        review
            .request_approval(tool.id, tool.request_fingerprint.clone(), Utc::now())
            .expect("review binds exact tool");
        store
            .append_capability_access_request(&access_request)
            .expect("approval request persists");
        store.append_tool_invocation(&tool).expect("tool persists");
        store
            .upsert_review_queue_item(&review)
            .expect("review persists");
        assert!(store
            .append_connector_invocation(&invocation)
            .expect("invocation persists"));
        store
            .resolve_capability_access_request(
                access_request.id,
                true,
                "Approve the exact generation-zero mutation".to_string(),
            )
            .expect("approval persists");

        account.credential_handle = ConnectorCredentialHandle::new();
        account.updated_at = Utc::now();
        store
            .upsert_connector_account(&account)
            .expect("account generation advances");
        assert_eq!(
            store
                .connector_account_sync_generation(&account)
                .expect("new generation reads"),
            1
        );
        assert!(store
            .start_approved_connector_invocation(invocation.id, Utc::now())
            .is_err());
        assert_eq!(
            store
                .connector_invocation(invocation.id)
                .expect("invocation remains pending")
                .status,
            ConnectorInvocationStatus::PendingApproval
        );
        assert_eq!(
            store
                .list_capability_access_records()
                .expect("approval loads")
                .into_iter()
                .find(|record| record.request.id == access_request.id)
                .expect("approval exists")
                .grant_state,
            crate::kernel::policy::CapabilityGrantState::OneShotAvailable
        );
    }

    #[test]
    fn fake_provider_full_contract_recovers_timeout_without_duplicate_mutation() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("fake-provider.sqlite3");
        let mut credential_store = FakeConnectorCredentialStore::default();
        let handle = credential_store
            .put(ConnectorSecret::new("fake-token".to_string()).expect("secret is valid"))
            .expect("credential stores");
        let mut account = account(handle);
        account.granted_capabilities = FAKE_PROVIDER_CAPABILITIES.to_vec();
        let remote = Arc::new(FakeConnectorRemoteState::default());
        let provider = FakeConnectorProvider::with_remote_state(Arc::clone(&remote));

        let search = MailSearchRequest::new("contract".to_string(), 1).unwrap();
        let reads = provider
            .search_mail(&account, &search)
            .expect("read succeeds");
        assert_eq!(reads.len(), 1);
        let draft = provider
            .create_draft(&account, "Review this reply")
            .expect("draft succeeds");
        assert!(draft.remote_object_ref.starts_with("fake:draft:"));

        let store = EventStore::open(&path).expect("store opens");
        let definition = AutomationDefinition::once(
            "Fake provider contract mutation".to_string(),
            "UTC".to_string(),
            Utc::now() - Duration::minutes(1),
        )
        .expect("definition is valid");
        store
            .upsert_automation_definition(&definition)
            .expect("definition persists");
        let automation_run_id = store
            .claim_due_automation_run(definition.id, Utc::now(), "contract".to_string())
            .expect("run claim succeeds")
            .expect("run is due")
            .id;
        let request = ToolExecutionRequest {
            tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
            input: json!({
                "provider_id": "fake",
                "account_id": account.id.to_string(),
                "account_generation": 0,
                "capability": "mail_send_draft",
                "target_ref": draft.remote_object_ref,
                "preview_hash": "sha256:reviewed-draft",
                "idempotency_key": "fake:send:reviewed-draft:once",
                "automation_run_id": automation_run_id.to_string()
            }),
            access_mode: AccessMode::FullAccess,
            run_id: Some(Uuid::new_v4()),
        };
        let plan = prepare_tool_execution(&request).expect("tool plan is valid");
        let access_request =
            request_capability_access(AccessMode::FullAccess, CapabilityKind::ConnectorWrite)
                .expect("connector write requires approval");
        let tool_record = ToolInvocationRecord::waiting_for_confirmation(&plan, access_request.id);
        let mut invocation = ConnectorInvocation::from_tool_request(&request, &tool_record)
            .expect("connector invocation is valid");
        bind_connector_invocation_to_tool_record(&invocation, &tool_record)
            .expect("exact approval binds");

        store
            .upsert_connector_account(&account)
            .expect("account persists");
        store
            .append_capability_access_request(&access_request)
            .expect("approval request persists");
        store
            .append_tool_invocation(&tool_record)
            .expect("pending tool persists");
        let mut review_item = ReviewQueueItem {
            id: Uuid::new_v4(),
            automation_run_id,
            agent_run_id: None,
            tool_invocation_id: None,
            status: ReviewQueueItemStatus::PendingReview,
            preview_fingerprint: Some(tool_record.request_fingerprint.clone()),
            revision: 0,
            title: "Review fake provider mutation".to_string(),
            evidence_ref: Some(draft.remote_object_ref.clone()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        review_item
            .request_approval(
                tool_record.id,
                tool_record.request_fingerprint.clone(),
                Utc::now(),
            )
            .expect("review binds exact tool");
        store
            .upsert_review_queue_item(&review_item)
            .expect("review persists");
        assert!(store
            .append_connector_invocation(&invocation)
            .expect("invocation persists"));
        store
            .resolve_capability_access_request(
                access_request.id,
                true,
                "Approved exact fake-provider mutation".to_string(),
            )
            .expect("exact mutation is approved");
        invocation = store
            .start_approved_connector_invocation(invocation.id, Utc::now())
            .expect("approved invocation starts");
        provider.timeout_after_next_apply();
        assert_eq!(
            provider
                .apply_mutation(&account, &invocation)
                .expect("provider applies"),
            ConnectorMutationApplyOutcome::ReconciliationRequired
        );
        store
            .mark_connector_invocation_reconciliation_required(invocation.id, Utc::now())
            .expect("uncertain outcome persists");
        drop(store);

        drop(provider);
        let restarted_provider = FakeConnectorProvider::with_remote_state(remote);
        let store = EventStore::open(&path).expect("store reopens");
        let recovered = store
            .connector_invocation(invocation.id)
            .expect("invocation reloads");
        assert_eq!(
            recovered.status,
            ConnectorInvocationStatus::ReconciliationRequired
        );
        let sweep = reconcile_due_connector_mutations(
            &store,
            &SingleFakeReconciler(&restarted_provider),
            Utc::now(),
            1,
        )
        .expect("persistent reconciliation worker succeeds");
        assert_eq!(sweep.claimed, 1);
        assert_eq!(sweep.completed, 1);
        assert_eq!(
            store
                .connector_invocation(invocation.id)
                .expect("completed invocation reloads")
                .status,
            ConnectorInvocationStatus::Succeeded
        );
        assert_eq!(restarted_provider.applied_count(), 1);

        let runtime = ConnectorRuntime::new(credential_store);
        runtime
            .disconnect_account(&mut account)
            .expect("local disconnect succeeds");
        assert_eq!(account.health, ConnectorHealth::Disconnected);
        assert!(restarted_provider.search_mail(&account, &search).is_err());
    }

    #[test]
    fn automation_connector_end_to_end_recovers_without_duplicate_side_effect() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("automation-connector-e2e.sqlite3");
        let now = Utc::now();
        let definition = AutomationDefinition::once(
            "Prepare one reply and wait for approval before sending.".to_string(),
            "Asia/Shanghai".to_string(),
            now - Duration::minutes(1),
        )
        .expect("automation definition is valid");
        let mut account = account(ConnectorCredentialHandle::new());
        account.granted_capabilities = FAKE_PROVIDER_CAPABILITIES.to_vec();
        let provider = FakeConnectorProvider::default();

        let automation_run_id;
        let agent_run_id;
        let review_item_id;
        let tool_invocation_id;
        {
            let store = EventStore::open(&path).expect("store opens");
            store
                .upsert_automation_definition(&definition)
                .expect("automation persists");
            store
                .upsert_connector_account(&account)
                .expect("account persists");
            let (automation_run, agent_run) = store
                .enqueue_due_automation_agent_run(
                    definition.id,
                    now,
                    "e2e-scheduler".to_string(),
                    format!("automation:{}", definition.id),
                )
                .expect("due window can enqueue")
                .expect("due window exists");
            automation_run_id = automation_run.id;
            agent_run_id = agent_run.id;
            store
                .claim_agent_run(agent_run.id, "e2e-worker".to_string(), 60)
                .expect("agent worker claims the run");
            assert_eq!(
                store
                    .reconcile_automation_agent_runs(Utc::now())
                    .expect("running state projects"),
                1
            );

            let draft = provider
                .create_draft(&account, "Frozen reply for review")
                .expect("provider creates a draft");
            let request = ToolExecutionRequest {
                tool_id: CONNECTOR_MUTATE_TOOL_ID.to_string(),
                input: json!({
                    "provider_id": "fake",
                    "account_id": account.id.to_string(),
                    "account_generation": 0,
                    "capability": "mail_send_draft",
                    "target_ref": draft.remote_object_ref,
                    "preview_hash": "sha256:frozen-reviewed-reply",
                    "idempotency_key": "e2e:send:frozen-reply:once",
                    "automation_run_id": automation_run.id.to_string()
                }),
                access_mode: AccessMode::FullAccess,
                run_id: Some(agent_run.id),
            };
            let plan = prepare_tool_execution(&request).expect("connector tool plan is valid");
            let access_request =
                request_capability_access(AccessMode::FullAccess, CapabilityKind::ConnectorWrite)
                    .expect("connector write requires approval");
            let tool_record =
                ToolInvocationRecord::waiting_for_confirmation(&plan, access_request.id);
            tool_invocation_id = tool_record.id;
            store
                .append_capability_access_request(&access_request)
                .expect("approval request persists");
            store
                .append_tool_invocation(&tool_record)
                .expect("pending tool persists");
            let mut review_item = ReviewQueueItem {
                id: Uuid::new_v4(),
                automation_run_id: automation_run.id,
                agent_run_id: Some(agent_run.id),
                tool_invocation_id: None,
                status: ReviewQueueItemStatus::PendingReview,
                preview_fingerprint: Some(tool_record.request_fingerprint.clone()),
                revision: 0,
                title: "Review the frozen reply before sending".to_string(),
                evidence_ref: Some(draft.remote_object_ref.clone()),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            review_item_id = review_item.id;
            review_item
                .request_approval(
                    tool_record.id,
                    tool_record.request_fingerprint.clone(),
                    Utc::now(),
                )
                .expect("review binds the exact tool request");
            store
                .upsert_review_queue_item(&review_item)
                .expect("review item persists");
            let connector_invocation =
                ConnectorInvocation::from_tool_request(&request, &tool_record)
                    .expect("connector invocation is valid");
            assert!(store
                .append_connector_invocation(&connector_invocation)
                .expect("connector invocation persists"));
            store
                .append_agent_run_transition(
                    &AgentRunTransition::new(
                        agent_run.id,
                        AgentRunStatus::WaitingForConfirmation,
                        "Exact connected-account change is waiting for approval.".to_string(),
                        Some(tool_record.id),
                    )
                    .expect("waiting transition is valid"),
                )
                .expect("agent waits for approval");
            assert_eq!(
                store
                    .reconcile_automation_agent_runs(Utc::now())
                    .expect("waiting state projects"),
                1
            );
            assert!(store
                .start_approved_connector_invocation(connector_invocation.id, Utc::now())
                .is_err());
            store
                .resolve_capability_access_request(
                    access_request.id,
                    true,
                    "Approved the exact frozen reply".to_string(),
                )
                .expect("user approval persists");
            let running = store
                .start_approved_connector_invocation(connector_invocation.id, Utc::now())
                .expect("approved connector mutation starts");
            provider.timeout_after_next_apply();
            assert_eq!(
                provider
                    .apply_mutation(&account, &running)
                    .expect("provider applies before timeout"),
                ConnectorMutationApplyOutcome::ReconciliationRequired
            );
            store
                .mark_connector_invocation_reconciliation_required(running.id, Utc::now())
                .expect("uncertain remote outcome persists");
        }

        let store = EventStore::open(&path).expect("store reopens after simulated crash");
        assert!(store
            .enqueue_due_automation_agent_run(
                definition.id,
                Utc::now(),
                "e2e-scheduler-restart".to_string(),
                format!("automation:{}", definition.id),
            )
            .expect("duplicate wake is safe")
            .is_none());
        let mut claims = store
            .claim_due_connector_reconciliations(Utc::now(), 1)
            .expect("reconciliation claim recovers");
        let claim = claims.pop().expect("one reconciliation is due");
        let recovered = claim.invocation();
        assert_eq!(
            recovered.status,
            ConnectorInvocationStatus::ReconciliationRequired
        );
        let ConnectorReconciliationOutcome::Applied(receipt) = provider
            .reconcile_mutation(&account, &recovered)
            .expect("provider reconciliation succeeds")
        else {
            panic!("remote mutation must be found after restart");
        };
        let mut wrong_receipt = receipt.clone();
        wrong_receipt.target_ref = "fake:draft:wrong".to_string();
        assert!(store
            .complete_claimed_connector_reconciliation(&claim, wrong_receipt, Utc::now())
            .is_err());
        let evidence_ref = receipt.evidence.remote_object_ref.clone();
        let completed = store
            .complete_claimed_connector_reconciliation(&claim, receipt.clone(), Utc::now())
            .expect("reconciled mutation completes with evidence");
        assert_eq!(completed.status, ConnectorInvocationStatus::Succeeded);
        assert_eq!(
            store
                .complete_connector_invocation(recovered.id, receipt, Utc::now())
                .expect("completion replay is idempotent")
                .status,
            ConnectorInvocationStatus::Succeeded
        );
        store
            .upsert_automation_checkpoint(&AutomationCheckpoint {
                automation_run_id,
                dedup_key: recovered.idempotency_key.clone(),
                tool_invocation_id: Some(tool_invocation_id),
                evidence_ref: Some(evidence_ref.clone()),
                recorded_at: Utc::now(),
            })
            .expect("evidence checkpoint persists");
        store
            .append_agent_run_finish(
                &AgentRunFinish::completed(
                    agent_run_id,
                    "Approved connected-account change completed with provider evidence."
                        .to_string(),
                )
                .expect("agent completion is valid"),
            )
            .expect("agent completion persists");
        assert_eq!(
            store
                .reconcile_automation_agent_runs(Utc::now())
                .expect("agent completion projects"),
            1
        );

        assert_eq!(provider.applied_count(), 1);
        assert_eq!(store.list_automation_runs().expect("runs load").len(), 1);
        assert_eq!(
            store
                .automation_run(automation_run_id)
                .expect("automation run loads")
                .status,
            AutomationRunStatus::Completed
        );
        let review = store
            .review_queue_item(review_item_id)
            .expect("review item loads");
        assert_eq!(review.status, ReviewQueueItemStatus::Accepted);
        assert_eq!(review.evidence_ref.as_deref(), Some(evidence_ref.as_str()));
        let tool = store
            .list_tool_invocations()
            .expect("tool audit loads")
            .into_iter()
            .find(|record| record.id == tool_invocation_id)
            .expect("tool audit exists");
        assert_eq!(tool.status, ToolExecutionStatus::Succeeded);
        assert!(tool.verification.passed);
        assert!(tool
            .evidence
            .iter()
            .any(|item| item.kind == "connector_remote_state"));
        assert_eq!(
            store
                .list_capability_access_records()
                .expect("permission records load")
                .into_iter()
                .find(|record| record.request.capability == CapabilityKind::ConnectorWrite)
                .expect("connector grant exists")
                .grant_state,
            crate::kernel::policy::CapabilityGrantState::OneShotConsumed
        );
        assert_eq!(
            store
                .automation_checkpoint(automation_run_id)
                .expect("checkpoint loads")
                .evidence_ref
                .as_deref(),
            Some(evidence_ref.as_str())
        );
    }

    #[test]
    fn marker_secret_stays_out_of_sqlite_dto_error_and_model_context() {
        let marker = format!("marker-refresh-token:{}", Uuid::new_v4());
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("secret-leak.sqlite3");
        let mut credential_store = FakeConnectorCredentialStore::default();
        let handle = credential_store
            .put(ConnectorSecret::new(marker.clone()).expect("secret is valid"))
            .expect("secret stores");
        let account = account(handle);
        let dto = serde_json::to_string(&ConnectorAccountSummary::from(&account))
            .expect("summary serializes");
        let model_context = connector_context_summary(std::slice::from_ref(&account));
        assert!(!model_context.contains(&account.provider_id));
        assert!(!model_context.contains(&account.display_name));
        assert!(model_context.contains("provider_label="));
        assert!(model_context.contains("abilities="));
        let provider_error = provider::ConnectorProviderFailure::InvalidResponse.to_string();
        {
            let store = EventStore::open(&path).expect("store opens");
            store
                .upsert_connector_account(&account)
                .expect("account persists");
        }
        let sqlite_bytes = std::fs::read(&path).expect("sqlite reads");
        let sqlite = String::from_utf8_lossy(&sqlite_bytes).into_owned();
        for output in [dto, model_context, provider_error, sqlite] {
            assert!(!output.contains(&marker));
            assert!(!output.contains("refresh_token"));
        }
    }
}
