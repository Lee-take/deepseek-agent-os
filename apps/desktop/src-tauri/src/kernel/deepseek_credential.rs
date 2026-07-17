use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroize;

use crate::kernel::deepseek::{
    DeepSeekReadinessTransport, DeepSeekTransportFailure, DEEPSEEK_API_BASE_URL,
    DEEPSEEK_API_KEY_ENV, DEEPSEEK_FLASH_MODEL, DEEPSEEK_PRO_MODEL,
};
use crate::kernel::local_directory::{WorkspaceReadinessCode, WorkspaceReadinessProjection};

const DEEPSEEK_KEY_MAX_BYTES: usize = 8 * 1024;
const DEEPSEEK_PROTECTED_KEY_MAX_BYTES: usize = 32 * 1024;
const DEEPSEEK_STATE_MAX_BYTES: usize = 32 * 1024;
const DEEPSEEK_KEY_FILE: &str = "deepseek-api-key.credential";
const DEEPSEEK_STATE_FILE: &str = "verification-state.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingOverallStatus {
    SetupRequired,
    Checking,
    Blocked,
    Ready,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingNextStep {
    DeepseekKey,
    Workspace,
    Doctor,
    Ready,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionReadinessStatus {
    Current,
    UpdateAvailable,
    CheckUnavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VersionReadinessProjection {
    pub current_version: String,
    pub status: VersionReadinessStatus,
    pub blocking: bool,
    pub message_key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OnboardingReadinessProjection {
    pub schema_version: u32,
    pub overall: OnboardingOverallStatus,
    pub next_step: OnboardingNextStep,
    pub deepseek: DeepSeekReadinessProjection,
    pub workspace: WorkspaceReadinessProjection,
    pub version: VersionReadinessProjection,
    pub checked_at: Option<DateTime<Utc>>,
}

pub fn build_onboarding_readiness_projection(
    deepseek: DeepSeekReadinessProjection,
    workspace: WorkspaceReadinessProjection,
    current_version: impl Into<String>,
) -> OnboardingReadinessProjection {
    let key_needs_input = matches!(
        deepseek.code,
        DeepSeekReadinessCode::KeyMissing
            | DeepSeekReadinessCode::KeyFormatInvalid
            | DeepSeekReadinessCode::AuthenticationFailed
    );
    let workspace_missing = workspace.code == WorkspaceReadinessCode::WorkspaceMissing;
    let ready = deepseek.chat_completion_ready && workspace.code == WorkspaceReadinessCode::Ready;
    let checking = deepseek.verification == DeepSeekVerificationState::Checking;
    let overall = if ready {
        OnboardingOverallStatus::Ready
    } else if checking {
        OnboardingOverallStatus::Checking
    } else if key_needs_input || workspace_missing {
        OnboardingOverallStatus::SetupRequired
    } else {
        OnboardingOverallStatus::Blocked
    };
    let next_step = if key_needs_input {
        OnboardingNextStep::DeepseekKey
    } else if deepseek.chat_completion_ready && workspace_missing {
        OnboardingNextStep::Workspace
    } else if ready {
        OnboardingNextStep::Ready
    } else {
        OnboardingNextStep::Doctor
    };
    let checked_at = deepseek.last_verified_at;
    OnboardingReadinessProjection {
        schema_version: 1,
        overall,
        next_step,
        deepseek,
        workspace,
        version: VersionReadinessProjection {
            current_version: current_version.into(),
            status: VersionReadinessStatus::Current,
            blocking: false,
            message_key: "onboarding.version.current".to_string(),
        },
        checked_at,
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepSeekCredentialSource {
    Stored,
    Environment,
    Missing,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepSeekVerificationState {
    NotChecked,
    Checking,
    Verified,
    Blocked,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepSeekReadinessCode {
    Ready,
    NotChecked,
    KeyMissing,
    KeyFormatInvalid,
    AuthenticationFailed,
    InsufficientBalance,
    RateLimited,
    NetworkUnavailable,
    NetworkTimeout,
    ModelUnavailable,
    RequestInvalid,
    ProviderUnavailable,
    ProviderProtocolError,
    CredentialStoreUnavailable,
}

impl DeepSeekReadinessCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::NotChecked => "not_checked",
            Self::KeyMissing => "key_missing",
            Self::KeyFormatInvalid => "key_format_invalid",
            Self::AuthenticationFailed => "authentication_failed",
            Self::InsufficientBalance => "insufficient_balance",
            Self::RateLimited => "rate_limited",
            Self::NetworkUnavailable => "network_unavailable",
            Self::NetworkTimeout => "network_timeout",
            Self::ModelUnavailable => "model_unavailable",
            Self::RequestInvalid => "request_invalid",
            Self::ProviderUnavailable => "provider_unavailable",
            Self::ProviderProtocolError => "provider_protocol_error",
            Self::CredentialStoreUnavailable => "credential_store_unavailable",
        }
    }

    pub fn retryable(self) -> bool {
        matches!(
            self,
            Self::InsufficientBalance
                | Self::RateLimited
                | Self::NetworkUnavailable
                | Self::NetworkTimeout
                | Self::ModelUnavailable
                | Self::ProviderUnavailable
                | Self::ProviderProtocolError
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct DeepSeekReadinessProjection {
    pub source: DeepSeekCredentialSource,
    pub configured: bool,
    pub verification: DeepSeekVerificationState,
    pub code: DeepSeekReadinessCode,
    pub chat_completion_ready: bool,
    pub balance_available: Option<bool>,
    pub flash_model: String,
    pub pro_model: String,
    pub flash_available: Option<bool>,
    pub pro_available: Option<bool>,
    pub retryable: bool,
    pub last_verified_at: Option<DateTime<Utc>>,
    pub message_key: String,
}

impl Default for DeepSeekReadinessProjection {
    fn default() -> Self {
        Self::missing()
    }
}

impl DeepSeekReadinessProjection {
    fn missing() -> Self {
        Self::for_code(
            DeepSeekCredentialSource::Missing,
            DeepSeekVerificationState::NotChecked,
            DeepSeekReadinessCode::KeyMissing,
            None,
        )
    }

    fn for_code(
        source: DeepSeekCredentialSource,
        verification: DeepSeekVerificationState,
        code: DeepSeekReadinessCode,
        receipt: Option<&DeepSeekVerificationReceipt>,
    ) -> Self {
        let ready = verification == DeepSeekVerificationState::Verified
            && code == DeepSeekReadinessCode::Ready;
        Self {
            source,
            configured: source != DeepSeekCredentialSource::Missing,
            verification,
            code,
            chat_completion_ready: ready,
            balance_available: receipt.and_then(|value| value.balance_available),
            flash_model: DEEPSEEK_FLASH_MODEL.to_string(),
            pro_model: DEEPSEEK_PRO_MODEL.to_string(),
            flash_available: receipt.and_then(|value| value.flash_available),
            pro_available: receipt.and_then(|value| value.pro_available),
            retryable: code.retryable(),
            last_verified_at: receipt.map(|value| value.verified_at),
            message_key: format!("onboarding.deepseek.{}", code.as_str()),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeepSeekVerificationReceiptStatus {
    Verified,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeepSeekVerificationReceipt {
    pub schema_version: u32,
    pub credential_revision: u64,
    pub status: DeepSeekVerificationReceiptStatus,
    pub code: DeepSeekReadinessCode,
    pub balance_available: Option<bool>,
    pub flash_available: Option<bool>,
    pub pro_available: Option<bool>,
    pub verified_at: DateTime<Utc>,
    pub endpoint_origin: String,
}

impl DeepSeekVerificationReceipt {
    fn blocked(
        credential_revision: u64,
        code: DeepSeekReadinessCode,
        balance_available: Option<bool>,
        flash_available: Option<bool>,
        pro_available: Option<bool>,
    ) -> Self {
        Self {
            schema_version: 1,
            credential_revision,
            status: DeepSeekVerificationReceiptStatus::Blocked,
            code,
            balance_available,
            flash_available,
            pro_available,
            verified_at: Utc::now(),
            endpoint_origin: DEEPSEEK_API_BASE_URL.to_string(),
        }
    }

    fn verified(credential_revision: u64, flash_available: bool, pro_available: bool) -> Self {
        Self {
            schema_version: 1,
            credential_revision,
            status: DeepSeekVerificationReceiptStatus::Verified,
            code: DeepSeekReadinessCode::Ready,
            balance_available: Some(true),
            flash_available: Some(flash_available),
            pro_available: Some(pro_available),
            verified_at: Utc::now(),
            endpoint_origin: DEEPSEEK_API_BASE_URL.to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct DeepSeekCredentialState {
    schema_version: u32,
    credential_revision: u64,
    receipt: Option<DeepSeekVerificationReceipt>,
}

impl Default for DeepSeekCredentialState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            credential_revision: 0,
            receipt: None,
        }
    }
}

pub struct DeepSeekSecret(String);

impl DeepSeekSecret {
    pub fn new(mut value: String) -> Result<Self, DeepSeekReadinessCode> {
        let normalized = value.trim().to_string();
        value.zeroize();
        if normalized.is_empty() || normalized.len() > DEEPSEEK_KEY_MAX_BYTES {
            let mut normalized = normalized;
            normalized.zeroize();
            return Err(DeepSeekReadinessCode::KeyFormatInvalid);
        }
        Ok(Self(normalized))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for DeepSeekSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub trait DeepSeekCredentialStore: Send {
    fn put(&mut self, secret: DeepSeekSecret) -> Result<(), DeepSeekReadinessCode>;
    fn read(&self) -> Result<DeepSeekSecret, DeepSeekReadinessCode>;
    fn delete(&mut self) -> Result<(), DeepSeekReadinessCode>;
    fn contains(&self) -> Result<bool, DeepSeekReadinessCode>;
}

pub struct FileDeepSeekCredentialStateStore {
    root: PathBuf,
}

impl FileDeepSeekCredentialStateStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, DeepSeekReadinessCode> {
        fs::create_dir_all(root.as_ref())
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        let root = fs::canonicalize(root.as_ref())
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if !root.is_dir() {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        cleanup_staged_files(&root, ".deepseek-state-")?;
        Ok(Self { root })
    }

    fn path(&self) -> PathBuf {
        self.root.join(DEEPSEEK_STATE_FILE)
    }

    fn load(&self) -> Result<DeepSeekCredentialState, DeepSeekReadinessCode> {
        let path = self.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(DeepSeekCredentialState::default())
            }
            Err(_) => return Err(DeepSeekReadinessCode::CredentialStoreUnavailable),
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() as usize > DEEPSEEK_STATE_MAX_BYTES
        {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        let canonical = fs::canonicalize(&path)
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if !canonical.starts_with(&self.root) {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        let mut file = OpenOptions::new()
            .read(true)
            .open(canonical)
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        let mut bytes = Vec::new();
        Read::take(&mut file, (DEEPSEEK_STATE_MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if bytes.len() > DEEPSEEK_STATE_MAX_BYTES {
            bytes.zeroize();
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        let parsed = serde_json::from_slice::<DeepSeekCredentialState>(&bytes)
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable);
        bytes.zeroize();
        let state = parsed?;
        if state.schema_version != 1 {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        Ok(state)
    }

    fn save(&self, state: &DeepSeekCredentialState) -> Result<(), DeepSeekReadinessCode> {
        let bytes = serde_json::to_vec_pretty(state)
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if bytes.len() > DEEPSEEK_STATE_MAX_BYTES {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        atomic_write(&self.root, &self.path(), ".deepseek-state-", &bytes)
    }

    fn delete(&self) -> Result<(), DeepSeekReadinessCode> {
        remove_regular_file_if_present(&self.path())
    }
}

struct EnvironmentReceipt {
    key_digest: [u8; 32],
    receipt: DeepSeekVerificationReceipt,
}

impl Drop for EnvironmentReceipt {
    fn drop(&mut self) {
        self.key_digest.zeroize();
    }
}

pub struct DeepSeekCredentialRuntime<S: DeepSeekCredentialStore> {
    store: Mutex<S>,
    state_store: FileDeepSeekCredentialStateStore,
    lifecycle: Mutex<()>,
    environment_receipt: Mutex<Option<EnvironmentReceipt>>,
}

impl<S: DeepSeekCredentialStore> DeepSeekCredentialRuntime<S> {
    pub fn new(store: S, state_store: FileDeepSeekCredentialStateStore) -> Self {
        Self {
            store: Mutex::new(store),
            state_store,
            lifecycle: Mutex::new(()),
            environment_receipt: Mutex::new(None),
        }
    }

    pub fn projection(
        &self,
        read_env: impl Fn(&str) -> Option<String>,
    ) -> DeepSeekReadinessProjection {
        let _lifecycle = match self.lifecycle.lock() {
            Ok(lock) => lock,
            Err(_) => return credential_store_unavailable_projection(),
        };
        self.projection_locked(read_env)
    }

    fn projection_locked(
        &self,
        read_env: impl Fn(&str) -> Option<String>,
    ) -> DeepSeekReadinessProjection {
        let has_stored = match self.store.lock() {
            Ok(store) => match store.contains() {
                Ok(contains) => contains,
                Err(_) => return credential_store_unavailable_projection(),
            },
            Err(_) => return credential_store_unavailable_projection(),
        };
        if has_stored {
            let state = match self.state_store.load() {
                Ok(state) if state.credential_revision > 0 => state,
                _ => return credential_store_unavailable_projection(),
            };
            return projection_from_receipt(
                DeepSeekCredentialSource::Stored,
                state.credential_revision,
                state.receipt.as_ref(),
            );
        }

        let Some(secret) = environment_secret(read_env) else {
            return DeepSeekReadinessProjection::missing();
        };
        let digest = secret_digest(&secret);
        let projection = self
            .environment_receipt
            .lock()
            .ok()
            .and_then(|receipt| {
                receipt
                    .as_ref()
                    .filter(|receipt| receipt.key_digest == digest)
                    .map(|receipt| {
                        projection_from_receipt(
                            DeepSeekCredentialSource::Environment,
                            receipt.receipt.credential_revision,
                            Some(&receipt.receipt),
                        )
                    })
            })
            .unwrap_or_else(|| {
                DeepSeekReadinessProjection::for_code(
                    DeepSeekCredentialSource::Environment,
                    DeepSeekVerificationState::NotChecked,
                    DeepSeekReadinessCode::NotChecked,
                    None,
                )
            });
        drop(secret);
        projection
    }

    pub fn save_and_verify(
        &self,
        api_key: String,
        verifier: &impl DeepSeekReadinessTransport,
        read_env: impl Fn(&str) -> Option<String> + Copy,
    ) -> DeepSeekReadinessProjection {
        let secret = match DeepSeekSecret::new(api_key) {
            Ok(secret) => secret,
            Err(code) => {
                let source = self.projection(read_env).source;
                return DeepSeekReadinessProjection::for_code(
                    source,
                    DeepSeekVerificationState::Blocked,
                    code,
                    None,
                );
            }
        };
        let revision = {
            let _lifecycle = match self.lifecycle.lock() {
                Ok(lock) => lock,
                Err(_) => return credential_store_unavailable_projection(),
            };
            let mut state = match self.state_store.load() {
                Ok(state) => state,
                Err(_) => return credential_store_unavailable_projection(),
            };
            let mut store = match self.store.lock() {
                Ok(store) => store,
                Err(_) => return credential_store_unavailable_projection(),
            };
            state.credential_revision = state.credential_revision.saturating_add(1).max(1);
            state.receipt = None;
            if self.state_store.save(&state).is_err() {
                return credential_store_unavailable_projection();
            }
            if let Ok(mut receipt) = self.environment_receipt.lock() {
                *receipt = None;
            }
            if store.put(secret).is_err() {
                return credential_store_unavailable_projection();
            }
            state.credential_revision
        };
        self.verify_stored_revision(revision, verifier, read_env)
    }

    pub fn verify(
        &self,
        verifier: &impl DeepSeekReadinessTransport,
        read_env: impl Fn(&str) -> Option<String> + Copy,
    ) -> DeepSeekReadinessProjection {
        let source = {
            let _lifecycle = match self.lifecycle.lock() {
                Ok(lock) => lock,
                Err(_) => return credential_store_unavailable_projection(),
            };
            let has_stored = match self.store.lock() {
                Ok(store) => match store.contains() {
                    Ok(contains) => contains,
                    Err(_) => return credential_store_unavailable_projection(),
                },
                Err(_) => return credential_store_unavailable_projection(),
            };
            if has_stored {
                let state = match self.state_store.load() {
                    Ok(state) if state.credential_revision > 0 => state,
                    _ => return credential_store_unavailable_projection(),
                };
                Some((DeepSeekCredentialSource::Stored, state.credential_revision))
            } else if environment_secret(read_env).is_some() {
                Some((DeepSeekCredentialSource::Environment, 0))
            } else {
                None
            }
        };
        match source {
            Some((DeepSeekCredentialSource::Stored, revision)) => {
                self.verify_stored_revision(revision, verifier, read_env)
            }
            Some((DeepSeekCredentialSource::Environment, _)) => {
                self.verify_environment(verifier, read_env)
            }
            _ => DeepSeekReadinessProjection::missing(),
        }
    }

    fn verify_stored_revision(
        &self,
        revision: u64,
        verifier: &impl DeepSeekReadinessTransport,
        read_env: impl Fn(&str) -> Option<String> + Copy,
    ) -> DeepSeekReadinessProjection {
        let secret = {
            let store = match self.store.lock() {
                Ok(store) => store,
                Err(_) => return credential_store_unavailable_projection(),
            };
            match store.read() {
                Ok(secret) => secret,
                Err(_) => return credential_store_unavailable_projection(),
            }
        };
        let receipt = run_verification(verifier, &secret, revision);
        drop(secret);
        let _lifecycle = match self.lifecycle.lock() {
            Ok(lock) => lock,
            Err(_) => return credential_store_unavailable_projection(),
        };
        let mut state = match self.state_store.load() {
            Ok(state) => state,
            Err(_) => return credential_store_unavailable_projection(),
        };
        if state.credential_revision != revision {
            return self.projection_locked(read_env);
        }
        state.receipt = Some(receipt.clone());
        if self.state_store.save(&state).is_err() {
            return credential_store_unavailable_projection();
        }
        projection_from_receipt(DeepSeekCredentialSource::Stored, revision, Some(&receipt))
    }

    fn verify_environment(
        &self,
        verifier: &impl DeepSeekReadinessTransport,
        read_env: impl Fn(&str) -> Option<String> + Copy,
    ) -> DeepSeekReadinessProjection {
        let Some(secret) = environment_secret(read_env) else {
            return DeepSeekReadinessProjection::missing();
        };
        let digest = secret_digest(&secret);
        let receipt = run_verification(verifier, &secret, 0);
        drop(secret);
        let Some(current_secret) = environment_secret(read_env) else {
            return DeepSeekReadinessProjection::missing();
        };
        let current_digest = secret_digest(&current_secret);
        drop(current_secret);
        if current_digest != digest {
            return DeepSeekReadinessProjection::for_code(
                DeepSeekCredentialSource::Environment,
                DeepSeekVerificationState::NotChecked,
                DeepSeekReadinessCode::NotChecked,
                None,
            );
        }
        match self.environment_receipt.lock() {
            Ok(mut environment_receipt) => {
                *environment_receipt = Some(EnvironmentReceipt {
                    key_digest: digest,
                    receipt: receipt.clone(),
                });
            }
            Err(_) => return credential_store_unavailable_projection(),
        }
        projection_from_receipt(DeepSeekCredentialSource::Environment, 0, Some(&receipt))
    }

    pub fn remove(&self, read_env: impl Fn(&str) -> Option<String>) -> DeepSeekReadinessProjection {
        let _lifecycle = match self.lifecycle.lock() {
            Ok(lock) => lock,
            Err(_) => return credential_store_unavailable_projection(),
        };
        let mut store = match self.store.lock() {
            Ok(store) => store,
            Err(_) => return credential_store_unavailable_projection(),
        };
        if store.delete().is_err() || self.state_store.delete().is_err() {
            return credential_store_unavailable_projection();
        }
        if let Ok(mut receipt) = self.environment_receipt.lock() {
            *receipt = None;
        }
        drop(store);
        self.projection_locked(read_env)
    }

    pub fn resolve_ready_key(
        &self,
        read_env: impl Fn(&str) -> Option<String>,
    ) -> Result<DeepSeekSecret, DeepSeekReadinessCode> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        let has_stored = self
            .store
            .lock()
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?
            .contains()?;
        if has_stored {
            let state = self.state_store.load()?;
            let ready = state
                .receipt
                .as_ref()
                .filter(|receipt| receipt.credential_revision == state.credential_revision)
                .map(|receipt| {
                    receipt.status == DeepSeekVerificationReceiptStatus::Verified
                        && receipt.code == DeepSeekReadinessCode::Ready
                })
                .unwrap_or(false);
            if !ready {
                return Err(state
                    .receipt
                    .as_ref()
                    .map(|receipt| receipt.code)
                    .unwrap_or(DeepSeekReadinessCode::NotChecked));
            }
            return self
                .store
                .lock()
                .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?
                .read();
        }
        let secret = environment_secret(read_env).ok_or(DeepSeekReadinessCode::KeyMissing)?;
        let digest = secret_digest(&secret);
        let ready = self
            .environment_receipt
            .lock()
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?
            .as_ref()
            .filter(|receipt| receipt.key_digest == digest)
            .map(|receipt| {
                receipt.receipt.status == DeepSeekVerificationReceiptStatus::Verified
                    && receipt.receipt.code == DeepSeekReadinessCode::Ready
            })
            .unwrap_or(false);
        if ready {
            Ok(secret)
        } else {
            Err(DeepSeekReadinessCode::NotChecked)
        }
    }
}

fn projection_from_receipt(
    source: DeepSeekCredentialSource,
    revision: u64,
    receipt: Option<&DeepSeekVerificationReceipt>,
) -> DeepSeekReadinessProjection {
    let receipt = receipt.filter(|receipt| receipt.credential_revision == revision);
    match receipt {
        Some(receipt) if receipt.status == DeepSeekVerificationReceiptStatus::Verified => {
            DeepSeekReadinessProjection::for_code(
                source,
                DeepSeekVerificationState::Verified,
                receipt.code,
                Some(receipt),
            )
        }
        Some(receipt) => DeepSeekReadinessProjection::for_code(
            source,
            DeepSeekVerificationState::Blocked,
            receipt.code,
            Some(receipt),
        ),
        None => DeepSeekReadinessProjection::for_code(
            source,
            DeepSeekVerificationState::NotChecked,
            DeepSeekReadinessCode::NotChecked,
            None,
        ),
    }
}

fn run_verification(
    verifier: &impl DeepSeekReadinessTransport,
    secret: &DeepSeekSecret,
    revision: u64,
) -> DeepSeekVerificationReceipt {
    let balance = match verifier.fetch_user_balance(secret.expose()) {
        Ok(balance) => balance,
        Err(error) => {
            return DeepSeekVerificationReceipt::blocked(
                revision,
                readiness_code_from_transport(error, false),
                None,
                None,
                None,
            )
        }
    };
    if !balance.is_available {
        return DeepSeekVerificationReceipt::blocked(
            revision,
            DeepSeekReadinessCode::InsufficientBalance,
            Some(false),
            None,
            None,
        );
    }
    let models = match verifier.fetch_models(secret.expose()) {
        Ok(models) => models,
        Err(error) => {
            return DeepSeekVerificationReceipt::blocked(
                revision,
                readiness_code_from_transport(error, true),
                Some(true),
                None,
                None,
            )
        }
    };
    let flash_available = models
        .data
        .iter()
        .any(|model| model.id == DEEPSEEK_FLASH_MODEL);
    let pro_available = models
        .data
        .iter()
        .any(|model| model.id == DEEPSEEK_PRO_MODEL);
    if !flash_available || !pro_available {
        return DeepSeekVerificationReceipt::blocked(
            revision,
            DeepSeekReadinessCode::ModelUnavailable,
            Some(true),
            Some(flash_available),
            Some(pro_available),
        );
    }
    DeepSeekVerificationReceipt::verified(revision, flash_available, pro_available)
}

fn readiness_code_from_transport(
    failure: DeepSeekTransportFailure,
    during_models: bool,
) -> DeepSeekReadinessCode {
    match failure {
        DeepSeekTransportFailure::HttpStatus(401) => DeepSeekReadinessCode::AuthenticationFailed,
        DeepSeekTransportFailure::HttpStatus(402) => DeepSeekReadinessCode::InsufficientBalance,
        DeepSeekTransportFailure::HttpStatus(429) => DeepSeekReadinessCode::RateLimited,
        DeepSeekTransportFailure::HttpStatus(400 | 422) => DeepSeekReadinessCode::RequestInvalid,
        DeepSeekTransportFailure::HttpStatus(500 | 503) => {
            DeepSeekReadinessCode::ProviderUnavailable
        }
        DeepSeekTransportFailure::HttpStatus(404) if during_models => {
            DeepSeekReadinessCode::ModelUnavailable
        }
        DeepSeekTransportFailure::NetworkUnavailable => DeepSeekReadinessCode::NetworkUnavailable,
        DeepSeekTransportFailure::Timeout => DeepSeekReadinessCode::NetworkTimeout,
        DeepSeekTransportFailure::Protocol | DeepSeekTransportFailure::HttpStatus(_) => {
            DeepSeekReadinessCode::ProviderProtocolError
        }
    }
}

fn credential_store_unavailable_projection() -> DeepSeekReadinessProjection {
    DeepSeekReadinessProjection::for_code(
        DeepSeekCredentialSource::Missing,
        DeepSeekVerificationState::Blocked,
        DeepSeekReadinessCode::CredentialStoreUnavailable,
        None,
    )
}

fn environment_secret(read_env: impl Fn(&str) -> Option<String>) -> Option<DeepSeekSecret> {
    read_env(DEEPSEEK_API_KEY_ENV).and_then(|value| DeepSeekSecret::new(value).ok())
}

fn secret_digest(secret: &DeepSeekSecret) -> [u8; 32] {
    Sha256::digest(secret.expose().as_bytes()).into()
}

fn cleanup_staged_files(root: &Path, prefix: &str) -> Result<(), DeepSeekReadinessCode> {
    let mut entries_seen = 0usize;
    let mut staged_seen = 0usize;
    for entry in
        fs::read_dir(root).map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?
    {
        let entry = entry.map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        entries_seen += 1;
        if entries_seen > 4096 {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(prefix) || !name.ends_with(".tmp") {
            continue;
        }
        staged_seen += 1;
        if staged_seen > 64 {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        fs::remove_file(entry.path())
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
    }
    Ok(())
}

fn atomic_write(
    root: &Path,
    destination: &Path,
    prefix: &str,
    bytes: &[u8],
) -> Result<(), DeepSeekReadinessCode> {
    if let Ok(metadata) = fs::symlink_metadata(destination) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
    }
    let temp = root.join(format!("{prefix}{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        drop(file);
        atomic_replace(&temp, destination)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(windows)]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), DeepSeekReadinessCode> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)
}

#[cfg(not(windows))]
fn atomic_replace(source: &Path, destination: &Path) -> Result<(), DeepSeekReadinessCode> {
    fs::rename(source, destination).map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)
}

fn remove_regular_file_if_present(path: &Path) -> Result<(), DeepSeekReadinessCode> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(DeepSeekReadinessCode::CredentialStoreUnavailable)
        }
        Ok(_) => {
            fs::remove_file(path).map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(DeepSeekReadinessCode::CredentialStoreUnavailable),
    }
}

#[cfg(windows)]
pub struct WindowsDeepSeekCredentialStore {
    root: PathBuf,
}

#[cfg(windows)]
impl WindowsDeepSeekCredentialStore {
    pub fn new(root: impl AsRef<Path>) -> Result<Self, DeepSeekReadinessCode> {
        fs::create_dir_all(root.as_ref())
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        let root = fs::canonicalize(root.as_ref())
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if !root.is_dir() {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        cleanup_staged_files(&root, ".deepseek-key-")?;
        Ok(Self { root })
    }

    fn path(&self) -> PathBuf {
        self.root.join(DEEPSEEK_KEY_FILE)
    }

    fn entropy() -> [u8; 32] {
        Sha256::digest(b"ds-agent.deepseek-credential.v1").into()
    }

    fn protect(secret: &DeepSeekSecret) -> Result<Vec<u8>, DeepSeekReadinessCode> {
        use windows::core::w;
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Cryptography::{
            CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        };

        let mut plaintext = secret.expose().as_bytes().to_vec();
        let input = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_mut_ptr(),
        };
        let mut entropy = Self::entropy();
        let entropy_blob = CRYPT_INTEGER_BLOB {
            cbData: entropy.len() as u32,
            pbData: entropy.as_mut_ptr(),
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        let result = unsafe {
            CryptProtectData(
                &input,
                w!("DS Agent DeepSeek credential"),
                Some(&entropy_blob),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        plaintext.zeroize();
        entropy.zeroize();
        result.map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if output.pbData.is_null() || output.cbData as usize > DEEPSEEK_PROTECTED_KEY_MAX_BYTES {
            if !output.pbData.is_null() {
                let protected = unsafe {
                    std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize)
                };
                protected.zeroize();
                unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
            }
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        let protected =
            unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
        let value = protected.to_vec();
        protected.zeroize();
        unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
        Ok(value)
    }

    fn unprotect(mut protected: Vec<u8>) -> Result<DeepSeekSecret, DeepSeekReadinessCode> {
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        };

        if protected.is_empty() || protected.len() > DEEPSEEK_PROTECTED_KEY_MAX_BYTES {
            protected.zeroize();
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        let input = CRYPT_INTEGER_BLOB {
            cbData: protected.len() as u32,
            pbData: protected.as_mut_ptr(),
        };
        let mut entropy = Self::entropy();
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
        result.map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if output.pbData.is_null() || output.cbData as usize > DEEPSEEK_KEY_MAX_BYTES {
            if !output.pbData.is_null() {
                let plaintext = unsafe {
                    std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize)
                };
                plaintext.zeroize();
                unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
            }
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        let plaintext =
            unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
        let value = String::from_utf8(plaintext.to_vec());
        plaintext.zeroize();
        unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
        match value {
            Ok(value) => DeepSeekSecret::new(value),
            Err(error) => {
                let mut bytes = error.into_bytes();
                bytes.zeroize();
                Err(DeepSeekReadinessCode::CredentialStoreUnavailable)
            }
        }
    }

    fn read_protected(&self) -> Result<Vec<u8>, DeepSeekReadinessCode> {
        let path = self.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() as usize > DEEPSEEK_PROTECTED_KEY_MAX_BYTES
        {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        let canonical = fs::canonicalize(&path)
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if !canonical.starts_with(&self.root) {
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        let mut file = OpenOptions::new()
            .read(true)
            .open(canonical)
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        let mut protected = Vec::new();
        Read::take(&mut file, (DEEPSEEK_PROTECTED_KEY_MAX_BYTES + 1) as u64)
            .read_to_end(&mut protected)
            .map_err(|_| DeepSeekReadinessCode::CredentialStoreUnavailable)?;
        if protected.len() > DEEPSEEK_PROTECTED_KEY_MAX_BYTES {
            protected.zeroize();
            return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
        }
        Ok(protected)
    }
}

#[cfg(windows)]
impl DeepSeekCredentialStore for WindowsDeepSeekCredentialStore {
    fn put(&mut self, secret: DeepSeekSecret) -> Result<(), DeepSeekReadinessCode> {
        let mut protected = Self::protect(&secret)?;
        let result = atomic_write(&self.root, &self.path(), ".deepseek-key-", &protected);
        protected.zeroize();
        result
    }

    fn read(&self) -> Result<DeepSeekSecret, DeepSeekReadinessCode> {
        Self::unprotect(self.read_protected()?)
    }

    fn delete(&mut self) -> Result<(), DeepSeekReadinessCode> {
        remove_regular_file_if_present(&self.path())
    }

    fn contains(&self) -> Result<bool, DeepSeekReadinessCode> {
        match fs::symlink_metadata(self.path()) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
            Ok(_) => Err(DeepSeekReadinessCode::CredentialStoreUnavailable),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err(DeepSeekReadinessCode::CredentialStoreUnavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use crate::kernel::deepseek::{
        DeepSeekModelDescriptor, DeepSeekModelListResponse, DeepSeekReadinessTransport,
        DeepSeekTransportFailure, DeepSeekUserBalanceResponse,
    };

    use super::*;

    #[derive(Default)]
    struct FakeStore {
        secret: Option<DeepSeekSecret>,
        inside_call: Option<Arc<AtomicBool>>,
        fail_put: bool,
    }

    impl DeepSeekCredentialStore for FakeStore {
        fn put(&mut self, secret: DeepSeekSecret) -> Result<(), DeepSeekReadinessCode> {
            self.enter();
            if self.fail_put {
                self.exit();
                return Err(DeepSeekReadinessCode::CredentialStoreUnavailable);
            }
            self.secret = Some(secret);
            self.exit();
            Ok(())
        }

        fn read(&self) -> Result<DeepSeekSecret, DeepSeekReadinessCode> {
            self.enter();
            let result = self
                .secret
                .as_ref()
                .map(|secret| DeepSeekSecret::new(secret.expose().to_string()).unwrap())
                .ok_or(DeepSeekReadinessCode::CredentialStoreUnavailable);
            self.exit();
            result
        }

        fn delete(&mut self) -> Result<(), DeepSeekReadinessCode> {
            self.enter();
            self.secret = None;
            self.exit();
            Ok(())
        }

        fn contains(&self) -> Result<bool, DeepSeekReadinessCode> {
            Ok(self.secret.is_some())
        }
    }

    impl FakeStore {
        fn enter(&self) {
            if let Some(flag) = &self.inside_call {
                flag.store(true, Ordering::SeqCst);
            }
        }

        fn exit(&self) {
            if let Some(flag) = &self.inside_call {
                flag.store(false, Ordering::SeqCst);
            }
        }
    }

    struct ScriptedVerifier {
        balance: Mutex<VecDeque<Result<bool, DeepSeekTransportFailure>>>,
        models: Mutex<VecDeque<Result<Vec<&'static str>, DeepSeekTransportFailure>>>,
        store_inside_call: Option<Arc<AtomicBool>>,
    }

    impl ScriptedVerifier {
        fn success() -> Self {
            Self::new(
                vec![Ok(true)],
                vec![Ok(vec![DEEPSEEK_FLASH_MODEL, DEEPSEEK_PRO_MODEL])],
            )
        }

        fn new(
            balance: Vec<Result<bool, DeepSeekTransportFailure>>,
            models: Vec<Result<Vec<&'static str>, DeepSeekTransportFailure>>,
        ) -> Self {
            Self {
                balance: Mutex::new(balance.into()),
                models: Mutex::new(models.into()),
                store_inside_call: None,
            }
        }
    }

    impl DeepSeekReadinessTransport for ScriptedVerifier {
        fn fetch_user_balance(
            &self,
            _api_key: &str,
        ) -> Result<DeepSeekUserBalanceResponse, DeepSeekTransportFailure> {
            if let Some(flag) = &self.store_inside_call {
                assert!(
                    !flag.load(Ordering::SeqCst),
                    "vault lock crossed network I/O"
                );
            }
            self.balance
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Ok(true))
                .map(|is_available| DeepSeekUserBalanceResponse {
                    is_available,
                    balance_infos: Vec::new(),
                })
        }

        fn fetch_models(
            &self,
            _api_key: &str,
        ) -> Result<DeepSeekModelListResponse, DeepSeekTransportFailure> {
            if let Some(flag) = &self.store_inside_call {
                assert!(
                    !flag.load(Ordering::SeqCst),
                    "vault lock crossed network I/O"
                );
            }
            self.models
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(vec![DEEPSEEK_FLASH_MODEL, DEEPSEEK_PRO_MODEL]))
                .map(|models| DeepSeekModelListResponse {
                    data: models
                        .into_iter()
                        .map(|id| DeepSeekModelDescriptor { id: id.to_string() })
                        .collect(),
                })
        }
    }

    fn runtime(store: FakeStore) -> (tempfile::TempDir, DeepSeekCredentialRuntime<FakeStore>) {
        let temp = tempfile::tempdir().unwrap();
        let state_store = FileDeepSeekCredentialStateStore::new(temp.path()).unwrap();
        (temp, DeepSeekCredentialRuntime::new(store, state_store))
    }

    #[test]
    fn key_input_is_bounded_and_zeroizable() {
        assert!(matches!(
            DeepSeekSecret::new("  ".to_string()),
            Err(DeepSeekReadinessCode::KeyFormatInvalid)
        ));
        assert!(matches!(
            DeepSeekSecret::new("x".repeat(DEEPSEEK_KEY_MAX_BYTES + 1)),
            Err(DeepSeekReadinessCode::KeyFormatInvalid)
        ));
        assert_eq!(
            DeepSeekSecret::new(" key ".to_string()).unwrap().expose(),
            "key"
        );
    }

    #[test]
    fn invalid_submitted_key_does_not_create_or_replace_a_stored_key() {
        let (_temp, runtime) = runtime(FakeStore::default());
        let projection =
            runtime.save_and_verify("   ".to_string(), &ScriptedVerifier::success(), |_| None);
        assert_eq!(projection.source, DeepSeekCredentialSource::Missing);
        assert!(!projection.configured);
        assert_eq!(projection.code, DeepSeekReadinessCode::KeyFormatInvalid);
        assert_eq!(
            runtime.projection(|_| None).code,
            DeepSeekReadinessCode::KeyMissing
        );
    }

    #[test]
    fn fake_store_put_read_replace_delete_round_trip() {
        let mut store = FakeStore::default();
        store
            .put(DeepSeekSecret::new("first".to_string()).unwrap())
            .unwrap();
        assert_eq!(store.read().unwrap().expose(), "first");
        store
            .put(DeepSeekSecret::new("second".to_string()).unwrap())
            .unwrap();
        assert_eq!(store.read().unwrap().expose(), "second");
        store.delete().unwrap();
        assert!(!store.contains().expect("fake store remains available"));
    }

    #[test]
    fn failed_atomic_replacement_preserves_old_secret_and_invalidates_readiness() {
        let (_temp, runtime) = runtime(FakeStore::default());
        let ready =
            runtime.save_and_verify("old-key".to_string(), &ScriptedVerifier::success(), |_| {
                None
            });
        assert!(ready.chat_completion_ready);

        runtime.store.lock().unwrap().fail_put = true;
        let failed =
            runtime.save_and_verify("new-key".to_string(), &ScriptedVerifier::success(), |_| {
                None
            });
        assert_eq!(
            failed.code,
            DeepSeekReadinessCode::CredentialStoreUnavailable
        );
        assert!(!failed.chat_completion_ready);

        let store = runtime.store.lock().unwrap();
        assert_eq!(store.secret.as_ref().unwrap().expose(), "old-key");
        drop(store);
        assert!(matches!(
            runtime.resolve_ready_key(|_| None),
            Err(DeepSeekReadinessCode::NotChecked)
        ));
    }

    #[test]
    fn environment_presence_is_not_readiness() {
        let (_temp, runtime) = runtime(FakeStore::default());
        let projection = runtime.projection(|_| Some("env-key".to_string()));
        assert_eq!(projection.source, DeepSeekCredentialSource::Environment);
        assert_eq!(
            projection.verification,
            DeepSeekVerificationState::NotChecked
        );
        assert!(!projection.chat_completion_ready);
    }

    #[test]
    fn stored_key_precedes_environment_and_requires_receipt() {
        let (_temp, runtime) = runtime(FakeStore::default());
        let projection = runtime.save_and_verify(
            "stored-key".to_string(),
            &ScriptedVerifier::success(),
            |_| Some("env-key".to_string()),
        );
        assert_eq!(projection.source, DeepSeekCredentialSource::Stored);
        assert!(projection.chat_completion_ready);
        assert_eq!(
            runtime
                .resolve_ready_key(|_| Some("env-key".to_string()))
                .unwrap()
                .expose(),
            "stored-key"
        );
    }

    #[test]
    fn replacing_key_increments_revision_and_invalidates_old_receipt() {
        let (temp, runtime) = runtime(FakeStore::default());
        runtime.save_and_verify("one".to_string(), &ScriptedVerifier::success(), |_| None);
        let first = FileDeepSeekCredentialStateStore::new(temp.path())
            .unwrap()
            .load()
            .unwrap();
        runtime.save_and_verify("two".to_string(), &ScriptedVerifier::success(), |_| None);
        let second = FileDeepSeekCredentialStateStore::new(temp.path())
            .unwrap()
            .load()
            .unwrap();
        assert_eq!(second.credential_revision, first.credential_revision + 1);
        assert_eq!(
            second.receipt.unwrap().credential_revision,
            second.credential_revision
        );
    }

    #[test]
    fn verification_does_not_hold_vault_call_across_network_io() {
        let flag = Arc::new(AtomicBool::new(false));
        let store = FakeStore {
            secret: None,
            inside_call: Some(flag.clone()),
            fail_put: false,
        };
        let (_temp, runtime) = runtime(store);
        let mut verifier = ScriptedVerifier::success();
        verifier.store_inside_call = Some(flag);
        let projection = runtime.save_and_verify("key".to_string(), &verifier, |_| None);
        assert!(projection.chat_completion_ready);
    }

    #[test]
    fn unavailable_balance_is_blocked_without_amounts() {
        let (_temp, runtime) = runtime(FakeStore::default());
        let verifier = ScriptedVerifier::new(vec![Ok(false)], vec![]);
        let projection = runtime.save_and_verify("key".to_string(), &verifier, |_| None);
        assert_eq!(projection.code, DeepSeekReadinessCode::InsufficientBalance);
        assert_eq!(projection.balance_available, Some(false));
        assert!(!projection.chat_completion_ready);
    }

    #[test]
    fn missing_required_model_is_blocked() {
        let (_temp, runtime) = runtime(FakeStore::default());
        let verifier = ScriptedVerifier::new(vec![Ok(true)], vec![Ok(vec![DEEPSEEK_FLASH_MODEL])]);
        let projection = runtime.save_and_verify("key".to_string(), &verifier, |_| None);
        assert_eq!(projection.code, DeepSeekReadinessCode::ModelUnavailable);
        assert_eq!(projection.flash_available, Some(true));
        assert_eq!(projection.pro_available, Some(false));
    }

    #[test]
    fn transport_failures_map_to_frozen_codes() {
        let cases = [
            (
                DeepSeekTransportFailure::HttpStatus(401),
                false,
                DeepSeekReadinessCode::AuthenticationFailed,
            ),
            (
                DeepSeekTransportFailure::HttpStatus(402),
                false,
                DeepSeekReadinessCode::InsufficientBalance,
            ),
            (
                DeepSeekTransportFailure::HttpStatus(429),
                false,
                DeepSeekReadinessCode::RateLimited,
            ),
            (
                DeepSeekTransportFailure::HttpStatus(400),
                false,
                DeepSeekReadinessCode::RequestInvalid,
            ),
            (
                DeepSeekTransportFailure::HttpStatus(422),
                false,
                DeepSeekReadinessCode::RequestInvalid,
            ),
            (
                DeepSeekTransportFailure::HttpStatus(500),
                false,
                DeepSeekReadinessCode::ProviderUnavailable,
            ),
            (
                DeepSeekTransportFailure::HttpStatus(503),
                false,
                DeepSeekReadinessCode::ProviderUnavailable,
            ),
            (
                DeepSeekTransportFailure::NetworkUnavailable,
                false,
                DeepSeekReadinessCode::NetworkUnavailable,
            ),
            (
                DeepSeekTransportFailure::Timeout,
                false,
                DeepSeekReadinessCode::NetworkTimeout,
            ),
            (
                DeepSeekTransportFailure::Protocol,
                false,
                DeepSeekReadinessCode::ProviderProtocolError,
            ),
            (
                DeepSeekTransportFailure::HttpStatus(404),
                true,
                DeepSeekReadinessCode::ModelUnavailable,
            ),
        ];
        for (failure, during_models, expected) in cases {
            assert_eq!(
                readiness_code_from_transport(failure, during_models),
                expected
            );
        }
    }

    #[test]
    fn receipt_and_projection_are_secret_path_and_provider_body_free() {
        let (_temp, runtime) = runtime(FakeStore::default());
        let projection = runtime.save_and_verify(
            "marker-secret-value".to_string(),
            &ScriptedVerifier::success(),
            |_| None,
        );
        let json = serde_json::to_string(&projection).unwrap();
        assert!(!json.contains("marker-secret-value"));
        assert!(!json.contains("provider raw body"));
        assert!(!json.contains("app_data"));
        assert!(!json.contains("vault"));
        assert!(!json.contains("total_balance"));
        assert!(json.contains(DEEPSEEK_FLASH_MODEL));
        assert!(json.contains(DEEPSEEK_PRO_MODEL));
    }

    #[test]
    fn remove_deletes_stored_key_and_receipt_then_reveals_environment() {
        let (temp, runtime) = runtime(FakeStore::default());
        runtime.save_and_verify("stored".to_string(), &ScriptedVerifier::success(), |_| None);
        let projection = runtime.remove(|_| Some("env".to_string()));
        assert_eq!(projection.source, DeepSeekCredentialSource::Environment);
        assert_eq!(
            projection.verification,
            DeepSeekVerificationState::NotChecked
        );
        assert!(!temp.path().join(DEEPSEEK_STATE_FILE).exists());
    }

    #[test]
    fn onboarding_transition_table_is_kernel_owned() {
        let missing_workspace = WorkspaceReadinessProjection {
            configured: false,
            workspace_name: None,
            workspace_root_display: None,
            root_exists: false,
            managed_directories_ready: false,
            writable: None,
            code: WorkspaceReadinessCode::WorkspaceMissing,
            retryable: true,
            message_key: "onboarding.workspace.workspace_missing".to_string(),
        };
        let ready_workspace = WorkspaceReadinessProjection {
            configured: true,
            workspace_name: Some("Work".to_string()),
            workspace_root_display: Some("Work".to_string()),
            root_exists: true,
            managed_directories_ready: true,
            writable: Some(true),
            code: WorkspaceReadinessCode::Ready,
            retryable: false,
            message_key: "onboarding.workspace.ready".to_string(),
        };
        let missing = build_onboarding_readiness_projection(
            DeepSeekReadinessProjection::missing(),
            missing_workspace.clone(),
            "1.0.2",
        );
        assert_eq!(missing.next_step, OnboardingNextStep::DeepseekKey);
        let verified = DeepSeekReadinessProjection::for_code(
            DeepSeekCredentialSource::Stored,
            DeepSeekVerificationState::Verified,
            DeepSeekReadinessCode::Ready,
            Some(&DeepSeekVerificationReceipt::verified(1, true, true)),
        );
        let needs_workspace =
            build_onboarding_readiness_projection(verified.clone(), missing_workspace, "1.0.2");
        assert_eq!(needs_workspace.next_step, OnboardingNextStep::Workspace);
        let ready = build_onboarding_readiness_projection(verified, ready_workspace, "1.0.2");
        assert_eq!(ready.overall, OnboardingOverallStatus::Ready);
        assert_eq!(ready.next_step, OnboardingNextStep::Ready);
    }
}
