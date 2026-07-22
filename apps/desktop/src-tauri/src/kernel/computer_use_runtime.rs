use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use uuid::Uuid;

#[cfg(test)]
use crate::kernel::capability::ComputerControlAction;
use crate::kernel::capability::{ComputerControlClient, ComputerScreenshotClient};
#[cfg(test)]
use crate::kernel::computer_use_session::ComputerUseActionBinding;
use crate::kernel::computer_use_session::{
    ComputerUseObservation, ComputerUseObservationPhase, ComputerUsePostcondition,
    ComputerUseSession, ComputerUseStep, ComputerUseStepStatus, ComputerUseUndoCapability,
    ComputerUseVerificationOutcome, ComputerUseVerificationReceipt,
};
use crate::kernel::event_store::EventStore;

const MAX_REDACTED_SUMMARY_CHARS: usize = 1_000;
const MAX_SEMANTIC_VALUE_CHARS: usize = 32_768;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RedactedComputerUseState {
    pub window_fingerprint: String,
    pub window_title_fingerprint: String,
    pub target_fingerprint: String,
    pub semantic_fingerprint: Option<String>,
    pub safe_summary: String,
}

impl RedactedComputerUseState {
    pub fn validate(&self) -> Result<(), String> {
        require_fingerprint(&self.window_fingerprint, "window fingerprint")?;
        require_fingerprint(&self.window_title_fingerprint, "window title fingerprint")?;
        require_fingerprint(&self.target_fingerprint, "target fingerprint")?;
        if let Some(value) = self.semantic_fingerprint.as_deref() {
            require_fingerprint(value, "semantic fingerprint")?;
        }
        require_safe_summary(&self.safe_summary)?;
        Ok(())
    }
}

pub trait ComputerUseAccessibilityClient {
    fn capture_redacted_state(&self) -> Result<RedactedComputerUseState, String>;
}

pub trait ComputerUseStepPersistence {
    fn load_step(&self, step_id: Uuid) -> Result<ComputerUseStep, String>;
    fn persist_step(&self, step: &ComputerUseStep, expected_revision: u64) -> Result<(), String>;
}

impl ComputerUseStepPersistence for EventStore {
    fn load_step(&self, step_id: Uuid) -> Result<ComputerUseStep, String> {
        self.get_computer_use_step(step_id)
            .map_err(|error| error.to_string())
    }

    fn persist_step(&self, step: &ComputerUseStep, expected_revision: u64) -> Result<(), String> {
        self.update_computer_use_step(step, expected_revision)
            .map_err(|error| error.to_string())
    }
}

impl ComputerUseStepPersistence for Mutex<EventStore> {
    fn load_step(&self, step_id: Uuid) -> Result<ComputerUseStep, String> {
        self.lock()
            .map_err(|_| "computer use event store lock is unavailable".to_string())?
            .get_computer_use_step(step_id)
            .map_err(|error| error.to_string())
    }

    fn persist_step(&self, step: &ComputerUseStep, expected_revision: u64) -> Result<(), String> {
        self.lock()
            .map_err(|_| "computer use event store lock is unavailable".to_string())?
            .update_computer_use_step(step, expected_revision)
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LocalComputerUseAccessibilityClient;

impl ComputerUseAccessibilityClient for LocalComputerUseAccessibilityClient {
    fn capture_redacted_state(&self) -> Result<RedactedComputerUseState, String> {
        #[cfg(windows)]
        {
            WindowsComputerUseAccessibilityClient.capture_redacted_state()
        }
        #[cfg(not(windows))]
        {
            Err("Durable verified Computer Use observation is Windows-first in v0.8".to_string())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComputerUseExecutionPermit {
    pub approval_request_id: Uuid,
    pub local_unlock_confirmed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputerUseExecutionResult {
    pub step: ComputerUseStep,
    pub execution_summary: Option<String>,
    pub safe_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerUseSessionView {
    pub id: Uuid,
    pub run_id: Option<Uuid>,
    pub safe_goal_summary: String,
    pub active_step_id: Option<Uuid>,
    pub revision: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerUseStepView {
    pub id: Uuid,
    pub session_id: Uuid,
    pub sequence: u32,
    pub status: ComputerUseStepStatus,
    pub revision: u64,
    pub pre_observation_fingerprint: String,
    pub window_fingerprint: String,
    pub target_fingerprint: Option<String>,
    pub pre_semantic_fingerprint: Option<String>,
    pub pre_screenshot_evidence_ref: String,
    pub pre_safe_summary: String,
    pub action_display: Option<String>,
    pub action_safe_summary: Option<String>,
    pub action_fingerprint: Option<String>,
    pub approval_request_id: Option<Uuid>,
    pub post_observation_fingerprint: Option<String>,
    pub post_semantic_fingerprint: Option<String>,
    pub post_screenshot_evidence_ref: Option<String>,
    pub verification_outcome: Option<ComputerUseVerificationOutcome>,
    pub verification_safe_summary: Option<String>,
    pub undo_capability: ComputerUseUndoCapability,
    pub status_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<&ComputerUseSession> for ComputerUseSessionView {
    fn from(session: &ComputerUseSession) -> Self {
        Self {
            id: session.id,
            run_id: session.run_id,
            safe_goal_summary: session.safe_goal_summary.clone(),
            active_step_id: session.active_step_id,
            revision: session.revision,
            created_at: session.created_at,
            updated_at: session.updated_at,
        }
    }
}

impl From<&ComputerUseStep> for ComputerUseStepView {
    fn from(step: &ComputerUseStep) -> Self {
        let action = step.action.as_ref();
        let post = step.post_observation.as_ref();
        let verification = step.verification.as_ref();
        Self {
            id: step.id,
            session_id: step.session_id,
            sequence: step.sequence,
            status: step.status,
            revision: step.revision,
            pre_observation_fingerprint: step.pre_observation.fingerprint.clone(),
            window_fingerprint: step.pre_observation.window_fingerprint.clone(),
            target_fingerprint: step.pre_observation.target_fingerprint.clone(),
            pre_semantic_fingerprint: step.pre_observation.semantic_fingerprint.clone(),
            pre_screenshot_evidence_ref: step.pre_observation.screenshot_evidence_ref.clone(),
            pre_safe_summary: step.pre_observation.safe_summary.clone(),
            action_display: action.map(|value| value.action.audit_summary()),
            action_safe_summary: action.map(|value| value.safe_summary.clone()),
            action_fingerprint: action.map(|value| value.action_fingerprint.clone()),
            approval_request_id: step.approval_request_id,
            post_observation_fingerprint: post.map(|value| value.fingerprint.clone()),
            post_semantic_fingerprint: post.and_then(|value| value.semantic_fingerprint.clone()),
            post_screenshot_evidence_ref: post.map(|value| value.screenshot_evidence_ref.clone()),
            verification_outcome: verification.map(|value| value.outcome),
            verification_safe_summary: verification.map(|value| value.safe_summary.clone()),
            undo_capability: step.checkpoint.undo_capability,
            status_reason: step.status_reason.clone(),
            created_at: step.created_at,
            updated_at: step.updated_at,
        }
    }
}

#[cfg(test)]
pub fn create_observed_computer_use_session(
    store: &EventStore,
    run_id: Option<Uuid>,
    safe_goal_summary: String,
    undo_capability: ComputerUseUndoCapability,
    screenshot_client: &impl ComputerScreenshotClient,
    accessibility_client: &impl ComputerUseAccessibilityClient,
) -> Result<(ComputerUseSession, ComputerUseStep), String> {
    let observation = capture_computer_use_observation(
        ComputerUseObservationPhase::PreAction,
        screenshot_client,
        accessibility_client,
    )?;
    let now = observation.captured_at;
    let mut session = ComputerUseSession::new(run_id, safe_goal_summary, now)?;
    let step = ComputerUseStep::new_observed(session.id, 1, observation, undo_capability, now)?;
    store
        .insert_computer_use_session(&session)
        .map_err(|error| error.to_string())?;
    store
        .insert_computer_use_step(&step)
        .map_err(|error| error.to_string())?;
    session.activate_step(step.id, now)?;
    Ok((session, step))
}

#[cfg(test)]
pub fn bind_computer_use_action(
    store: &EventStore,
    step_id: Uuid,
    action: ComputerControlAction,
    safe_summary: String,
    postcondition: ComputerUsePostcondition,
) -> Result<ComputerUseStep, String> {
    let mut step = store
        .get_computer_use_step(step_id)
        .map_err(|error| error.to_string())?;
    let expected_revision = step.revision;
    let binding =
        ComputerUseActionBinding::new(&step.pre_observation, action, safe_summary, postcondition)?;
    step.bind_action(binding, Utc::now())?;
    store
        .update_computer_use_step(&step, expected_revision)
        .map_err(|error| error.to_string())?;
    Ok(step)
}

#[cfg(test)]
pub fn approve_computer_use_step(
    store: &EventStore,
    step_id: Uuid,
    approval_request_id: Uuid,
    approved_action_fingerprint: &str,
) -> Result<ComputerUseStep, String> {
    let mut step = store
        .get_computer_use_step(step_id)
        .map_err(|error| error.to_string())?;
    let expected_revision = step.revision;
    step.approve(approval_request_id, approved_action_fingerprint, Utc::now())?;
    store
        .update_computer_use_step(&step, expected_revision)
        .map_err(|error| error.to_string())?;
    Ok(step)
}

pub fn take_over_computer_use_step(
    store: &EventStore,
    step_id: Uuid,
    reason: String,
) -> Result<ComputerUseStep, String> {
    let mut step = store
        .get_computer_use_step(step_id)
        .map_err(|error| error.to_string())?;
    let expected_revision = step.revision;
    step.take_over(reason, Utc::now())?;
    store
        .update_computer_use_step(&step, expected_revision)
        .map_err(|error| error.to_string())?;
    Ok(step)
}

pub fn execute_ready_computer_use_step(
    store: &impl ComputerUseStepPersistence,
    step_id: Uuid,
    permit: ComputerUseExecutionPermit,
    screenshot_client: &impl ComputerScreenshotClient,
    accessibility_client: &impl ComputerUseAccessibilityClient,
    control_client: &impl ComputerControlClient,
) -> Result<ComputerUseExecutionResult, String> {
    if !permit.local_unlock_confirmed {
        return Err("computer use execution requires an active local unlock".to_string());
    }
    if permit.approval_request_id.is_nil() {
        return Err("computer use execution requires an exact approval request".to_string());
    }

    let mut step = store.load_step(step_id)?;
    if step.status != ComputerUseStepStatus::Ready {
        return Err(format!(
            "computer use step in {:?} is not ready for execution",
            step.status
        ));
    }
    let action = step
        .action
        .clone()
        .ok_or_else(|| "computer use ready step has no exact action".to_string())?;
    let current = accessibility_client.capture_redacted_state()?;
    current.validate()?;
    if current.window_fingerprint != action.window_fingerprint
        || current.window_title_fingerprint != action.pre_window_title_fingerprint
        || current.target_fingerprint != action.target_fingerprint
        || current.semantic_fingerprint != step.pre_observation.semantic_fingerprint
    {
        let expected_revision = step.revision;
        step.require_replan(
            "Foreground window, accessibility target, or bounded semantic state changed after approval; re-observation and a new approval are required."
                .to_string(),
            Utc::now(),
        )?;
        store.persist_step(&step, expected_revision)?;
        return Ok(ComputerUseExecutionResult {
            step,
            execution_summary: None,
            safe_error: Some(
                "Desktop state changed after approval; no input action was sent.".to_string(),
            ),
        });
    }

    let expected_revision = step.revision;
    step.mark_action_started(
        permit.approval_request_id,
        &current.window_fingerprint,
        &current.window_title_fingerprint,
        &current.target_fingerprint,
        Utc::now(),
    )?;
    store.persist_step(&step, expected_revision)?;

    let durable_started = store.load_step(step_id)?;
    if durable_started.status != ComputerUseStepStatus::ActionStarted
        || durable_started.action_start_count != 1
        || durable_started
            .action
            .as_ref()
            .map(|value| &value.action_fingerprint)
            != Some(&action.action_fingerprint)
    {
        return Err(
            "durable ActionStarted binding changed before the desktop effect; execution stopped"
                .to_string(),
        );
    }
    step = durable_started;

    let execution = match control_client
        .execute_control("foreground accessibility target", &action.action)
    {
        Ok(execution) => execution,
        Err(error) => {
            let safe_error = safe_runtime_error(&error);
            let expected_revision = step.revision;
            step.mark_effect_unknown(
                "The desktop input backend did not return a reliable effect receipt; automatic replay is blocked."
                    .to_string(),
                Utc::now(),
            )?;
            store.persist_step(&step, expected_revision)?;
            return Ok(ComputerUseExecutionResult {
                step,
                execution_summary: None,
                safe_error: Some(safe_error),
            });
        }
    };

    let post_observation = match capture_computer_use_observation(
        ComputerUseObservationPhase::PostAction,
        screenshot_client,
        accessibility_client,
    ) {
        Ok(observation) => observation,
        Err(error) => {
            let safe_error = safe_runtime_error(&error);
            let expected_revision = step.revision;
            step.mark_effect_unknown(
                "The desktop action was sent but post-action evidence could not be captured; automatic replay is blocked."
                    .to_string(),
                Utc::now(),
            )?;
            store.persist_step(&step, expected_revision)?;
            return Ok(ComputerUseExecutionResult {
                step,
                execution_summary: Some(safe_execution_summary(&execution.summary)),
                safe_error: Some(safe_error),
            });
        }
    };

    let expected_revision = step.revision;
    if let Err(error) = step.record_post_observation(post_observation, Utc::now()) {
        let safe_error = safe_runtime_error(&error);
        step.mark_effect_unknown(
            "The desktop action was sent but post-action evidence did not bind to the approved window and target; automatic replay is blocked."
                .to_string(),
            Utc::now(),
        )?;
        store.persist_step(&step, expected_revision)?;
        return Ok(ComputerUseExecutionResult {
            step,
            execution_summary: Some(safe_execution_summary(&execution.summary)),
            safe_error: Some(safe_error),
        });
    }
    store.persist_step(&step, expected_revision)?;

    let receipt = automatic_verification_receipt(&step)?;
    let expected_revision = step.revision;
    step.record_verification(receipt)?;
    store.persist_step(&step, expected_revision)?;

    Ok(ComputerUseExecutionResult {
        step,
        execution_summary: Some(safe_execution_summary(&execution.summary)),
        safe_error: None,
    })
}

pub fn capture_computer_use_observation(
    phase: ComputerUseObservationPhase,
    screenshot_client: &impl ComputerScreenshotClient,
    accessibility_client: &impl ComputerUseAccessibilityClient,
) -> Result<ComputerUseObservation, String> {
    let screenshot = screenshot_client.capture_screenshot()?;
    let state = accessibility_client.capture_redacted_state()?;
    state.validate()?;
    ComputerUseObservation::new(
        phase,
        state.window_fingerprint,
        state.window_title_fingerprint,
        Some(state.target_fingerprint),
        state.semantic_fingerprint,
        screenshot.evidence_ref,
        state.safe_summary,
        Utc::now().max(screenshot.captured_at),
    )
}

fn automatic_verification_receipt(
    step: &ComputerUseStep,
) -> Result<ComputerUseVerificationReceipt, String> {
    let action = step
        .action
        .as_ref()
        .ok_or_else(|| "computer use step has no exact action to verify".to_string())?;
    let post = step
        .post_observation
        .as_ref()
        .ok_or_else(|| "computer use step has no post-action observation to verify".to_string())?;
    let (outcome, safe_summary) = match post.semantic_fingerprint.as_deref() {
        None => (
            ComputerUseVerificationOutcome::EvidenceOnly,
            "Post-action screenshot evidence was captured, but no bounded semantic state was available; verification remains pending."
                .to_string(),
        ),
        Some(after) => {
            let satisfied = match &action.postcondition {
                ComputerUsePostcondition::TargetSemanticFingerprintEquals { expected } => {
                    after == expected
                }
                ComputerUsePostcondition::TargetSemanticFingerprintChanged => action
                    .pre_semantic_fingerprint
                    .as_deref()
                    .is_some_and(|before| before != after),
            };
            if satisfied {
                (
                    ComputerUseVerificationOutcome::Verified,
                    "The bounded accessibility state satisfies the deterministic postcondition."
                        .to_string(),
                )
            } else {
                (
                    ComputerUseVerificationOutcome::Failed,
                    "The bounded accessibility state does not satisfy the deterministic postcondition."
                        .to_string(),
                )
            }
        }
    };
    Ok(ComputerUseVerificationReceipt {
        id: Uuid::new_v4(),
        action_fingerprint: action.action_fingerprint.clone(),
        post_observation_fingerprint: post.fingerprint.clone(),
        outcome,
        safe_summary,
        verified_at: Utc::now().max(post.captured_at),
    })
}

fn require_fingerprint(value: &str, field: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "computer use {field} must be a SHA-256 fingerprint"
        ));
    }
    Ok(())
}

fn require_safe_summary(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > MAX_REDACTED_SUMMARY_CHARS {
        return Err("computer use redacted summary is empty or too long".to_string());
    }
    Ok(())
}

fn safe_runtime_error(value: &str) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated = value.chars().take(240).collect::<String>();
    if truncated.is_empty() {
        "Desktop runtime returned an unspecified error.".to_string()
    } else {
        truncated
    }
}

fn safe_execution_summary(value: &str) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated = value.chars().take(240).collect::<String>();
    if truncated.is_empty() {
        "Desktop input backend acknowledged one action.".to_string()
    } else {
        truncated
    }
}

fn fingerprint_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

pub fn accessibility_value_semantic_fingerprint(value: &str) -> Result<String, String> {
    if value.chars().count() > MAX_SEMANTIC_VALUE_CHARS {
        return Err(format!(
            "computer use semantic value exceeds {MAX_SEMANTIC_VALUE_CHARS} characters"
        ));
    }
    Ok(fingerprint_parts(&[
        "windows-accessibility-value/v1",
        value,
    ]))
}

#[cfg(windows)]
fn current_windows_bounded_semantic_value(
    element: &windows::Win32::UI::Accessibility::IUIAutomationElement,
) -> Option<String> {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationTextPattern, IUIAutomationValuePattern, UIA_TextPatternId, UIA_ValuePatternId,
    };

    unsafe {
        element
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .ok()
            .and_then(|pattern| pattern.CurrentValue().ok())
            .map(|value| value.to_string())
            .or_else(|| {
                element
                    .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                    .ok()
                    .and_then(|pattern| pattern.DocumentRange().ok())
                    .and_then(|range| range.GetText(MAX_SEMANTIC_VALUE_CHARS as i32).ok())
                    .map(|value| value.to_string())
            })
    }
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsComputerUseAccessibilityClient;

#[cfg(windows)]
impl ComputerUseAccessibilityClient for WindowsComputerUseAccessibilityClient {
    fn capture_redacted_state(&self) -> Result<RedactedComputerUseState, String> {
        std::thread::spawn(capture_windows_redacted_state)
            .join()
            .map_err(|_| "Windows accessibility observation thread failed".to_string())?
    }
}

#[cfg(windows)]
fn capture_windows_redacted_state() -> Result<RedactedComputerUseState, String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    initialized
        .ok()
        .map_err(|error| format!("Windows accessibility COM initialization failed: {error}"))?;
    let _guard = ComGuard;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return Err("Windows accessibility found no foreground window".to_string());
    }
    let mut process_id = 0u32;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if thread_id == 0 || process_id == 0 {
        return Err("Windows accessibility could not identify the foreground process".to_string());
    }
    let mut class_buffer = [0u16; 256];
    let class_len = unsafe { GetClassNameW(hwnd, &mut class_buffer) }.max(0) as usize;
    let window_class = String::from_utf16_lossy(&class_buffer[..class_len]);
    let mut title_buffer = [0u16; 1_024];
    let title_len = unsafe { GetWindowTextW(hwnd, &mut title_buffer) }.max(0) as usize;
    let window_title_fingerprint =
        fingerprint_parts(&[&String::from_utf16_lossy(&title_buffer[..title_len])]);
    let handle_identity = format!("{:p}", hwnd.0);
    let process_text = process_id.to_string();
    let thread_text = thread_id.to_string();
    let window_fingerprint = fingerprint_parts(&[
        "windows-foreground-window/v1",
        &handle_identity,
        &process_text,
        &thread_text,
        &fingerprint_parts(&[&window_class]),
    ]);

    let automation: IUIAutomation = unsafe {
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("Windows UI Automation client creation failed: {error}"))?
    };
    let focused = unsafe {
        automation
            .GetFocusedElement()
            .map_err(|error| format!("Windows UI Automation found no focused target: {error}"))?
    };
    let target_process_id = unsafe { focused.CurrentProcessId() }
        .map_err(|error| format!("Windows UI Automation target process is unavailable: {error}"))?;
    if target_process_id <= 0 || target_process_id as u32 != process_id {
        return Err(
            "Windows UI Automation focus does not belong to the foreground window".to_string(),
        );
    }
    let control_type = unsafe { focused.CurrentControlType() }
        .map_err(|error| format!("Windows UI Automation target type is unavailable: {error}"))?;
    let control_type_text = control_type.0.to_string();
    let automation_id = unsafe { focused.CurrentAutomationId() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let target_class = unsafe { focused.CurrentClassName() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let target_name = unsafe { focused.CurrentName() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let is_password = unsafe { focused.CurrentIsPassword() }
        .map(|value| value.as_bool())
        .unwrap_or(true);
    let is_enabled = unsafe { focused.CurrentIsEnabled() }
        .map(|value| value.as_bool())
        .unwrap_or(false);
    let is_keyboard_focusable = unsafe { focused.CurrentIsKeyboardFocusable() }
        .map(|value| value.as_bool())
        .unwrap_or(false);
    let target_fingerprint = fingerprint_parts(&[
        "windows-accessibility-target/v1",
        &process_text,
        &control_type_text,
        &fingerprint_parts(&[&automation_id]),
        &fingerprint_parts(&[&target_class]),
        &fingerprint_parts(&[&target_name]),
        if is_password {
            "password"
        } else {
            "not_password"
        },
        if is_enabled { "enabled" } else { "disabled" },
        if is_keyboard_focusable {
            "keyboard_focusable"
        } else {
            "not_keyboard_focusable"
        },
    ]);

    let semantic_fingerprint = if is_password {
        None
    } else {
        let walker = unsafe { automation.RawViewWalker().ok() };
        let mut semantic_element = focused.clone();
        let mut value = None;
        for _ in 0..=16 {
            value = current_windows_bounded_semantic_value(&semantic_element);
            if value.is_some() {
                break;
            }
            let Some(walker) = walker.as_ref() else {
                break;
            };
            let Ok(parent) = (unsafe { walker.GetParentElement(&semantic_element) }) else {
                break;
            };
            let Ok(parent_process_id) = (unsafe { parent.CurrentProcessId() }) else {
                break;
            };
            if parent_process_id != target_process_id {
                break;
            }
            semantic_element = parent;
        }
        value.and_then(|value| {
            if value.chars().count() > MAX_SEMANTIC_VALUE_CHARS {
                None
            } else {
                accessibility_value_semantic_fingerprint(&value).ok()
            }
        })
    };
    let semantic_note = if semantic_fingerprint.is_some() {
        "bounded semantic state captured"
    } else {
        "semantic state unavailable"
    };
    Ok(RedactedComputerUseState {
        window_fingerprint,
        window_title_fingerprint,
        target_fingerprint,
        semantic_fingerprint,
        safe_summary: format!(
            "Foreground Windows accessibility target type {} is {} and {}; {}.",
            control_type.0,
            if is_enabled { "enabled" } else { "disabled" },
            if is_keyboard_focusable {
                "keyboard-focusable"
            } else {
                "not keyboard-focusable"
            },
            semantic_note
        ),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use chrono::Utc;
    use tempfile::tempdir;

    use super::*;
    use crate::kernel::capability::{ComputerControlExecution, ComputerScreenshot};

    struct FakeAccessibilityClient {
        states: Mutex<VecDeque<Result<RedactedComputerUseState, String>>>,
    }

    impl FakeAccessibilityClient {
        fn new(states: Vec<RedactedComputerUseState>) -> Self {
            Self {
                states: Mutex::new(states.into_iter().map(Ok).collect()),
            }
        }
    }

    impl ComputerUseAccessibilityClient for FakeAccessibilityClient {
        fn capture_redacted_state(&self) -> Result<RedactedComputerUseState, String> {
            self.states
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err("no fake accessibility state remains".to_string()))
        }
    }

    struct FakeScreenshotClient {
        refs: Mutex<VecDeque<String>>,
    }

    impl FakeScreenshotClient {
        fn new(count: usize) -> Self {
            Self {
                refs: Mutex::new(
                    (1..=count)
                        .map(|index| format!("computer-screenshots/fake-{index}.png"))
                        .collect(),
                ),
            }
        }
    }

    impl ComputerScreenshotClient for FakeScreenshotClient {
        fn capture_screenshot(&self) -> Result<ComputerScreenshot, String> {
            Ok(ComputerScreenshot {
                display_label: "Fake display".to_string(),
                evidence_ref: self
                    .refs
                    .lock()
                    .unwrap()
                    .pop_front()
                    .ok_or_else(|| "no fake screenshot remains".to_string())?,
                width: 1280,
                height: 720,
                captured_at: Utc::now(),
            })
        }
    }

    struct FakeControlClient {
        calls: AtomicUsize,
        fail: bool,
    }

    impl FakeControlClient {
        fn succeeding() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                fail: true,
            }
        }
    }

    impl ComputerControlClient for FakeControlClient {
        fn execute_control(
            &self,
            _target: &str,
            action: &ComputerControlAction,
        ) -> Result<ComputerControlExecution, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err("fake input backend failed".to_string())
            } else {
                Ok(ComputerControlExecution {
                    summary: action.audit_summary(),
                })
            }
        }
    }

    fn redacted_state(
        window: &str,
        target: &str,
        semantic: Option<&str>,
    ) -> RedactedComputerUseState {
        redacted_state_with_title(window, "stable-title", target, semantic)
    }

    fn redacted_state_with_title(
        window: &str,
        title: &str,
        target: &str,
        semantic: Option<&str>,
    ) -> RedactedComputerUseState {
        RedactedComputerUseState {
            window_fingerprint: fingerprint_parts(&[window]),
            window_title_fingerprint: fingerprint_parts(&[title]),
            target_fingerprint: fingerprint_parts(&[target]),
            semantic_fingerprint: semantic.map(|value| fingerprint_parts(&[value])),
            safe_summary: "Isolated Notepad-like editor is foreground and focused.".to_string(),
        }
    }

    fn setup_ready_step(
        store: &EventStore,
        screenshots: &FakeScreenshotClient,
        accessibility: &FakeAccessibilityClient,
        expected_semantic: &str,
    ) -> (ComputerUseStep, Uuid) {
        let (_, observed) = create_observed_computer_use_session(
            store,
            None,
            "Update an isolated Notepad-like editor.".to_string(),
            ComputerUseUndoCapability::None,
            screenshots,
            accessibility,
        )
        .unwrap();
        let bound = bind_computer_use_action(
            store,
            observed.id,
            ComputerControlAction::TypeText {
                text: "verified text".to_string(),
            },
            "Type the exact approved text into the focused editor.".to_string(),
            ComputerUsePostcondition::TargetSemanticFingerprintEquals {
                expected: fingerprint_parts(&[expected_semantic]),
            },
        )
        .unwrap();
        let approval_id = Uuid::new_v4();
        let ready = approve_computer_use_step(
            store,
            bound.id,
            approval_id,
            &bound.action.as_ref().unwrap().action_fingerprint,
        )
        .unwrap();
        (ready, approval_id)
    }

    #[test]
    fn durable_vertical_step_observes_acts_once_and_verifies() {
        let directory = tempdir().unwrap();
        let store = EventStore::open(directory.path().join("runtime.db")).unwrap();
        let screenshots = FakeScreenshotClient::new(2);
        let accessibility = FakeAccessibilityClient::new(vec![
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "target", Some("after")),
        ]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");

        let result = execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .unwrap();

        assert_eq!(result.step.status, ComputerUseStepStatus::Verified);
        assert_eq!(result.step.action_start_count, 1);
        assert_eq!(control.calls.load(Ordering::SeqCst), 1);
        let public_view = serde_json::to_string(&ComputerUseStepView::from(&result.step)).unwrap();
        assert!(!public_view.contains("verified text"));
        assert!(!public_view.contains("stable-title"));
        assert!(public_view.contains("type text (13 chars)"));
        assert_eq!(
            store.get_computer_use_step(ready.id).unwrap().status,
            ComputerUseStepStatus::Verified
        );
        assert!(execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .is_err());
        assert_eq!(control.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn changed_target_requires_replan_before_any_input() {
        let directory = tempdir().unwrap();
        let store = EventStore::open(directory.path().join("stale.db")).unwrap();
        let screenshots = FakeScreenshotClient::new(1);
        let accessibility = FakeAccessibilityClient::new(vec![
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "other-target", Some("before")),
        ]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");

        let result = execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .unwrap();
        assert_eq!(result.step.status, ComputerUseStepStatus::NeedsReplan);
        assert_eq!(result.step.approval_request_id, None);
        assert_eq!(control.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn changed_semantic_state_requires_replan_before_any_input() {
        let directory = tempdir().unwrap();
        let store = EventStore::open(directory.path().join("stale-semantic.db")).unwrap();
        let screenshots = FakeScreenshotClient::new(1);
        let accessibility = FakeAccessibilityClient::new(vec![
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "target", Some("changed-after-approval")),
        ]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");

        let result = execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .unwrap();

        assert_eq!(result.step.status, ComputerUseStepStatus::NeedsReplan);
        assert_eq!(result.step.approval_request_id, None);
        assert_eq!(control.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn changed_window_title_requires_replan_before_any_input() {
        let directory = tempdir().unwrap();
        let store = EventStore::open(directory.path().join("stale-title.db")).unwrap();
        let screenshots = FakeScreenshotClient::new(1);
        let accessibility = FakeAccessibilityClient::new(vec![
            redacted_state_with_title("window", "before-title", "target", Some("before")),
            redacted_state_with_title("window", "changed-title", "target", Some("before")),
        ]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");

        let result = execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .unwrap();

        assert_eq!(result.step.status, ComputerUseStepStatus::NeedsReplan);
        assert_eq!(result.step.approval_request_id, None);
        assert_eq!(control.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn input_backend_failure_becomes_effect_unknown_and_is_not_replayed() {
        let directory = tempdir().unwrap();
        let store = EventStore::open(directory.path().join("unknown.db")).unwrap();
        let screenshots = FakeScreenshotClient::new(1);
        let accessibility = FakeAccessibilityClient::new(vec![
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "target", Some("before")),
        ]);
        let control = FakeControlClient::failing();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");

        let result = execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .unwrap();
        assert_eq!(result.step.status, ComputerUseStepStatus::EffectUnknown);
        assert_eq!(result.step.action_start_count, 1);
        assert_eq!(control.calls.load(Ordering::SeqCst), 1);
        assert!(execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .is_err());
        assert_eq!(control.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn screenshot_only_post_state_stays_awaiting_verification() {
        let directory = tempdir().unwrap();
        let store = EventStore::open(directory.path().join("evidence-only.db")).unwrap();
        let screenshots = FakeScreenshotClient::new(2);
        let accessibility = FakeAccessibilityClient::new(vec![
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "target", None),
        ]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");

        let result = execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .unwrap();
        assert_eq!(
            result.step.status,
            ComputerUseStepStatus::AwaitingVerification
        );
        assert_eq!(control.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn deterministic_postcondition_failure_is_distinct_from_action_failure() {
        let directory = tempdir().unwrap();
        let store = EventStore::open(directory.path().join("verification-failed.db")).unwrap();
        let screenshots = FakeScreenshotClient::new(2);
        let accessibility = FakeAccessibilityClient::new(vec![
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "target", Some("unexpected-after")),
        ]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");

        let result = execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .unwrap();

        assert_eq!(
            result.step.status,
            ComputerUseStepStatus::VerificationFailed
        );
        assert_eq!(result.step.action_start_count, 1);
        assert_eq!(control.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn post_action_window_title_change_can_still_verify() {
        let directory = tempdir().unwrap();
        let store = EventStore::open(directory.path().join("post-title-changed.db")).unwrap();
        let screenshots = FakeScreenshotClient::new(2);
        let accessibility = FakeAccessibilityClient::new(vec![
            redacted_state_with_title("window", "clean-title", "target", Some("before")),
            redacted_state_with_title("window", "clean-title", "target", Some("before")),
            redacted_state_with_title("window", "dirty-title", "target", Some("after")),
        ]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");

        let result = execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .unwrap();

        assert_eq!(result.step.status, ComputerUseStepStatus::Verified);
        assert_eq!(control.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn post_action_target_change_becomes_effect_unknown_without_replay() {
        let directory = tempdir().unwrap();
        let store = EventStore::open(directory.path().join("post-target-changed.db")).unwrap();
        let screenshots = FakeScreenshotClient::new(2);
        let accessibility = FakeAccessibilityClient::new(vec![
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "target", Some("before")),
            redacted_state("window", "other-target", Some("after")),
        ]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");

        let result = execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .unwrap();

        assert_eq!(result.step.status, ComputerUseStepStatus::EffectUnknown);
        assert_eq!(result.step.action_start_count, 1);
        assert_eq!(control.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            store.get_computer_use_step(ready.id).unwrap().status,
            ComputerUseStepStatus::EffectUnknown
        );
        assert!(execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .is_err());
        assert_eq!(control.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn local_unlock_is_checked_before_revalidation_or_input() {
        let directory = tempdir().unwrap();
        let store = EventStore::open(directory.path().join("locked.db")).unwrap();
        let screenshots = FakeScreenshotClient::new(1);
        let accessibility =
            FakeAccessibilityClient::new(vec![redacted_state("window", "target", Some("before"))]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");

        assert!(execute_ready_computer_use_step(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: false,
            },
            &screenshots,
            &accessibility,
            &control,
        )
        .is_err());
        assert_eq!(control.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            store.get_computer_use_step(ready.id).unwrap().status,
            ComputerUseStepStatus::Ready
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a visible isolated Windows Notepad-like editor session"]
    fn windows_notepad_like_smoke_observes_types_once_and_verifies() {
        use std::process::Command;
        use std::thread;
        use std::time::Duration;

        use crate::kernel::capability::{
            LocalComputerControlClient, LocalComputerScreenshotClient,
        };

        let editor_script = r#"
Add-Type -AssemblyName System.Windows.Forms
$form = New-Object System.Windows.Forms.Form
$form.Text = 'DS Agent Isolated Editor'
$form.Width = 720
$form.Height = 480
$form.StartPosition = 'CenterScreen'
$editor = New-Object System.Windows.Forms.TextBox
$editor.Name = 'dsAgentEditor'
$editor.Multiline = $true
$editor.AcceptsReturn = $true
$editor.AcceptsTab = $true
$editor.Dock = 'Fill'
$form.Controls.Add($editor)
$form.Add_Shown({ [void]$editor.Focus() })
[System.Windows.Forms.Application]::Run($form)
"#;
        let mut editor = Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-Command", editor_script])
            .spawn()
            .expect("isolated Notepad-like editor starts");
        let process_id = editor.id();
        let result = (|| -> Result<(), String> {
            let window_ready = (0..30).any(|_| {
                let ready = Command::new("powershell.exe")
                    .args([
                        "-NoProfile",
                        "-Command",
                        &format!(
                            "$p=Get-Process -Id {process_id} -ErrorAction SilentlyContinue; if($null -ne $p -and $p.MainWindowHandle -ne 0){{'ready'}}"
                        ),
                    ])
                    .output()
                    .ok()
                    .filter(|output| output.status.success())
                    .map(|output| String::from_utf8_lossy(&output.stdout).contains("ready"))
                    .unwrap_or(false);
                if !ready {
                    thread::sleep(Duration::from_millis(250));
                }
                ready
            });
            if !window_ready {
                return Err("Notepad-like editor did not expose a stable main window".to_string());
            }
            let activate_script = format!(
                "$shell = New-Object -ComObject WScript.Shell; [void]$shell.AppActivate({process_id})"
            );
            let activated = Command::new("powershell.exe")
                .args(["-NoProfile", "-Command", &activate_script])
                .status()
                .map_err(|error| format!("Notepad-like editor activation failed: {error}"))?;
            if !activated.success() {
                return Err("Notepad-like editor activation returned a failure status".to_string());
            }
            thread::sleep(Duration::from_millis(500));

            let accessibility = LocalComputerUseAccessibilityClient;
            let mut semantic_ready = false;
            for _ in 0..20 {
                semantic_ready = accessibility
                    .capture_redacted_state()
                    .ok()
                    .and_then(|state| state.semantic_fingerprint)
                    .is_some();
                if semantic_ready {
                    break;
                }
                thread::sleep(Duration::from_millis(250));
            }
            if !semantic_ready {
                return Err("Notepad-like editor did not expose bounded semantic state".to_string());
            }

            let directory = tempdir().map_err(|error| error.to_string())?;
            let store = EventStore::open(directory.path().join("notepad-like-smoke.db"))
                .map_err(|error| error.to_string())?;
            let screenshot = LocalComputerScreenshotClient::new(directory.path().to_path_buf());
            let (_, observed) = create_observed_computer_use_session(
                &store,
                None,
                "Verify one isolated Notepad-like editor action.".to_string(),
                ComputerUseUndoCapability::None,
                &screenshot,
                &accessibility,
            )?;
            if observed.pre_observation.semantic_fingerprint.is_none() {
                return Err(
                    "Notepad-like editor exposed no bounded UI Automation value".to_string()
                );
            }
            let bound = bind_computer_use_action(
                &store,
                observed.id,
                ComputerControlAction::TypeText {
                    text: format!("DS Agent v0.8 verified {}", Uuid::new_v4().simple()),
                },
                "Type one smoke-test value into the isolated Notepad-like editor.".to_string(),
                ComputerUsePostcondition::TargetSemanticFingerprintChanged,
            )?;
            let approval_id = Uuid::new_v4();
            approve_computer_use_step(
                &store,
                bound.id,
                approval_id,
                &bound
                    .action
                    .as_ref()
                    .ok_or_else(|| "smoke action is missing".to_string())?
                    .action_fingerprint,
            )?;
            let control = LocalComputerControlClient::new();
            let result = execute_ready_computer_use_step(
                &store,
                bound.id,
                ComputerUseExecutionPermit {
                    approval_request_id: approval_id,
                    local_unlock_confirmed: true,
                },
                &screenshot,
                &accessibility,
                &control,
            )?;
            if result.step.status != ComputerUseStepStatus::Verified {
                return Err(format!(
                    "Notepad-like smoke ended in {:?}",
                    result.step.status
                ));
            }
            Ok(())
        })();
        let _ = editor.kill();
        let _ = editor.wait();
        result.expect("isolated Notepad-like action verifies");
    }
}
