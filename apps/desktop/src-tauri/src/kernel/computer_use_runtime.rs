use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use uuid::Uuid;

use crate::kernel::capability::{
    ComputerControlAction, ComputerControlClient, ComputerScreenshotClient,
};
use crate::kernel::computer_use_session::{
    ComputerUseActionBinding, ComputerUseApprovalActor, ComputerUseObservation,
    ComputerUseObservationPhase, ComputerUsePostcondition, ComputerUseSession, ComputerUseStep,
    ComputerUseStepStatus, ComputerUseUndoCapability, ComputerUseVerificationOutcome,
    ComputerUseVerificationReceipt,
};
use crate::kernel::event_store::EventStore;

const MAX_REDACTED_SUMMARY_CHARS: usize = 1_000;
const MAX_SEMANTIC_VALUE_CHARS: usize = 32_768;
pub const COMPUTER_USE_OBSERVATION_MAX_ATTEMPTS: usize = 2;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RedactedComputerUseState {
    pub application_fingerprint: String,
    pub process_fingerprint: String,
    pub window_fingerprint: String,
    pub window_title_fingerprint: String,
    pub frame_fingerprint: String,
    pub target_fingerprint: String,
    pub semantic_fingerprint: Option<String>,
    pub safe_summary: String,
}

impl RedactedComputerUseState {
    pub fn validate(&self) -> Result<(), String> {
        require_fingerprint(&self.application_fingerprint, "application fingerprint")?;
        require_fingerprint(&self.process_fingerprint, "process fingerprint")?;
        require_fingerprint(&self.window_fingerprint, "window fingerprint")?;
        require_fingerprint(&self.window_title_fingerprint, "window title fingerprint")?;
        require_fingerprint(&self.frame_fingerprint, "frame fingerprint")?;
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
    pub application_fingerprint: String,
    pub process_fingerprint: String,
    pub window_fingerprint: String,
    pub frame_fingerprint: String,
    pub target_fingerprint: Option<String>,
    pub pre_semantic_fingerprint: Option<String>,
    pub pre_screenshot_evidence_ref: String,
    pub pre_safe_summary: String,
    pub action_display: Option<String>,
    pub action_safe_summary: Option<String>,
    pub action_fingerprint: Option<String>,
    pub approval_request_id: Option<Uuid>,
    pub approval_actor: Option<ComputerUseApprovalActor>,
    pub observation_valid_until: DateTime<Utc>,
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
            application_fingerprint: step.pre_observation.application_fingerprint.clone(),
            process_fingerprint: step.pre_observation.process_fingerprint.clone(),
            window_fingerprint: step.pre_observation.window_fingerprint.clone(),
            frame_fingerprint: step.pre_observation.frame_fingerprint.clone(),
            target_fingerprint: step.pre_observation.target_fingerprint.clone(),
            pre_semantic_fingerprint: step.pre_observation.semantic_fingerprint.clone(),
            pre_screenshot_evidence_ref: step.pre_observation.screenshot_evidence_ref.clone(),
            pre_safe_summary: step.pre_observation.safe_summary.clone(),
            action_display: action.map(|value| value.action.audit_summary()),
            action_safe_summary: action.map(|value| value.safe_summary.clone()),
            action_fingerprint: action.map(|value| value.action_fingerprint.clone()),
            approval_request_id: step.approval_request_id,
            approval_actor: step.approval_actor,
            observation_valid_until: step.pre_observation.valid_until,
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

pub fn persist_observed_computer_use_session(
    store: &EventStore,
    run_id: Option<Uuid>,
    safe_goal_summary: String,
    undo_capability: ComputerUseUndoCapability,
    observation: ComputerUseObservation,
) -> Result<(ComputerUseSession, ComputerUseStep), String> {
    observation.validate()?;
    observation.require_fresh_at(Utc::now())?;
    if observation.phase != ComputerUseObservationPhase::PreAction {
        return Err("computer use session requires a pre-action observation".to_string());
    }
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

pub fn approve_computer_use_step(
    store: &EventStore,
    step_id: Uuid,
    approval_request_id: Uuid,
    approved_action_fingerprint: &str,
    actor: ComputerUseApprovalActor,
) -> Result<ComputerUseStep, String> {
    let mut step = store
        .get_computer_use_step(step_id)
        .map_err(|error| error.to_string())?;
    let expected_revision = step.revision;
    let now = Utc::now();
    if step.pre_observation.require_fresh_at(now).is_err() {
        step.require_replan(
            "The approved observation expired before authority could be bound; re-observation and a new local-user approval are required."
                .to_string(),
            now,
        )?;
        store
            .update_computer_use_step(&step, expected_revision)
            .map_err(|error| error.to_string())?;
        return Err(
            "computer use observation expired before approval; the step now requires re-observation"
                .to_string(),
        );
    }
    step.approve(approval_request_id, approved_action_fingerprint, actor, now)?;
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
    execute_ready_computer_use_step_at(
        store,
        step_id,
        permit,
        screenshot_client,
        accessibility_client,
        control_client,
        Utc::now(),
    )
}

fn execute_ready_computer_use_step_at(
    store: &impl ComputerUseStepPersistence,
    step_id: Uuid,
    permit: ComputerUseExecutionPermit,
    screenshot_client: &impl ComputerScreenshotClient,
    accessibility_client: &impl ComputerUseAccessibilityClient,
    control_client: &impl ComputerControlClient,
    now: DateTime<Utc>,
) -> Result<ComputerUseExecutionResult, String> {
    if !permit.local_unlock_confirmed {
        return Err("computer use execution requires an active local unlock".to_string());
    }
    if permit.approval_request_id.is_nil() {
        return Err("computer use execution requires an exact approval request".to_string());
    }

    let mut step = store.load_step(step_id)?;
    step.validate()?;
    if step.status != ComputerUseStepStatus::Ready {
        return Err(format!(
            "computer use step in {:?} is not ready for execution",
            step.status
        ));
    }
    if step.pre_observation.require_fresh_at(now).is_err() {
        let expected_revision = step.revision;
        step.require_replan(
            "The approved desktop observation expired before execution; re-observation and a new local-user approval are required."
                .to_string(),
            now,
        )?;
        store.persist_step(&step, expected_revision)?;
        return Ok(ComputerUseExecutionResult {
            step,
            execution_summary: None,
            safe_error: Some(
                "Desktop observation expired before execution; no input action was sent."
                    .to_string(),
            ),
        });
    }
    let action = step
        .action
        .clone()
        .ok_or_else(|| "computer use ready step has no exact action".to_string())?;
    let current = accessibility_client.capture_redacted_state()?;
    current.validate()?;
    if current.application_fingerprint != action.application_fingerprint
        || current.process_fingerprint != action.process_fingerprint
        || current.window_fingerprint != action.window_fingerprint
        || current.window_title_fingerprint != action.pre_window_title_fingerprint
        || current.frame_fingerprint != action.frame_fingerprint
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
        &current.application_fingerprint,
        &current.process_fingerprint,
        &current.window_fingerprint,
        &current.window_title_fingerprint,
        &current.frame_fingerprint,
        &current.target_fingerprint,
        now,
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
    let mut last_error = None;
    for _ in 0..COMPUTER_USE_OBSERVATION_MAX_ATTEMPTS {
        match capture_computer_use_observation_once(phase, screenshot_client, accessibility_client)
        {
            Ok(observation) => return Ok(observation),
            Err(error) => last_error = Some(error),
        }
    }
    Err(format!(
        "computer use observation failed after {COMPUTER_USE_OBSERVATION_MAX_ATTEMPTS} bounded attempts: {}",
        last_error.unwrap_or_else(|| "no observation receipt was returned".to_string())
    ))
}

fn capture_computer_use_observation_once(
    phase: ComputerUseObservationPhase,
    screenshot_client: &impl ComputerScreenshotClient,
    accessibility_client: &impl ComputerUseAccessibilityClient,
) -> Result<ComputerUseObservation, String> {
    let screenshot = screenshot_client.capture_screenshot()?;
    let state = accessibility_client.capture_redacted_state()?;
    state.validate()?;
    ComputerUseObservation::new(
        phase,
        state.application_fingerprint,
        state.process_fingerprint,
        state.window_fingerprint,
        state.window_title_fingerprint,
        state.frame_fingerprint,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WindowsComputerUseTargetProfile {
    FileExplorer,
    Excel,
    Generic,
}

#[cfg(windows)]
impl WindowsComputerUseTargetProfile {
    fn from_window_class(window_class: &str) -> Self {
        if window_class.eq_ignore_ascii_case("CabinetWClass")
            || window_class.eq_ignore_ascii_case("ExploreWClass")
        {
            Self::FileExplorer
        } else if window_class.eq_ignore_ascii_case("XLMAIN") {
            Self::Excel
        } else {
            Self::Generic
        }
    }

    fn contract_name(self) -> &'static str {
        match self {
            Self::FileExplorer => "file-explorer",
            Self::Excel => "excel",
            Self::Generic => "generic",
        }
    }

    fn safe_label(self) -> &'static str {
        match self {
            Self::FileExplorer => "File Explorer",
            Self::Excel => "Excel",
            Self::Generic => "Windows",
        }
    }
}

#[cfg(windows)]
fn current_windows_bounded_semantic_value(
    element: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    profile: WindowsComputerUseTargetProfile,
) -> Option<String> {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationLegacyIAccessiblePattern, IUIAutomationSelectionItemPattern,
        IUIAutomationTextPattern, IUIAutomationValuePattern, UIA_LegacyIAccessiblePatternId,
        UIA_SelectionItemPatternId, UIA_TextPatternId, UIA_ValuePatternId,
    };

    let value = || unsafe {
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
            .or_else(|| {
                element
                    .GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(
                        UIA_LegacyIAccessiblePatternId,
                    )
                    .ok()
                    .and_then(|pattern| pattern.CurrentValue().ok())
                    .map(|value| value.to_string())
            })
    };
    let selection = || unsafe {
        element
            .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
            .ok()
            .and_then(|pattern| pattern.CurrentIsSelected().ok())
            .map(|selected| {
                if selected.as_bool() {
                    "selection:selected".to_string()
                } else {
                    "selection:not_selected".to_string()
                }
            })
    };
    match profile {
        WindowsComputerUseTargetProfile::FileExplorer => selection().or_else(value),
        WindowsComputerUseTargetProfile::Excel | WindowsComputerUseTargetProfile::Generic => {
            value().or_else(selection)
        }
    }
}

#[cfg(windows)]
fn current_windows_accessibility_ancestor_fingerprint(
    focused: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    target_process_id: i32,
    include_volatile_labels: bool,
) -> String {
    let mut ancestor_fingerprints = vec!["windows-accessibility-ancestor-frame/v1".to_string()];
    let mut current = focused.clone();
    for _ in 0..16 {
        let Ok(parent) = (unsafe { walker.GetParentElement(&current) }) else {
            break;
        };
        let Ok(parent_process_id) = (unsafe { parent.CurrentProcessId() }) else {
            break;
        };
        if parent_process_id != target_process_id {
            break;
        }
        let control_type = unsafe { parent.CurrentControlType() }
            .map(|value| value.0.to_string())
            .unwrap_or_default();
        let automation_id = unsafe { parent.CurrentAutomationId() }
            .map(|value| value.to_string())
            .unwrap_or_default();
        let class_name = unsafe { parent.CurrentClassName() }
            .map(|value| value.to_string())
            .unwrap_or_default();
        let framework_id = unsafe { parent.CurrentFrameworkId() }
            .map(|value| value.to_string())
            .unwrap_or_default();
        let name = unsafe { parent.CurrentName() }
            .map(|value| value.to_string())
            .unwrap_or_default();
        let item_status = unsafe { parent.CurrentItemStatus() }
            .map(|value| value.to_string())
            .unwrap_or_default();
        let help_text = unsafe { parent.CurrentHelpText() }
            .map(|value| value.to_string())
            .unwrap_or_default();
        let stable_fingerprint = fingerprint_parts(&[
            &control_type,
            &fingerprint_parts(&[&automation_id]),
            &fingerprint_parts(&[&class_name]),
            &fingerprint_parts(&[&framework_id]),
        ]);
        ancestor_fingerprints.push(if include_volatile_labels {
            fingerprint_parts(&[
                &stable_fingerprint,
                &fingerprint_parts(&[&name]),
                &fingerprint_parts(&[&item_status]),
                &fingerprint_parts(&[&help_text]),
            ])
        } else {
            stable_fingerprint
        });
        current = parent;
    }
    let parts = ancestor_fingerprints
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    fingerprint_parts(&parts)
}

#[cfg(windows)]
fn current_windows_selected_accessibility_descendant(
    root: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    target_process_id: i32,
    profile: WindowsComputerUseTargetProfile,
    expected_target: Option<&WindowsBoundAccessibilityTarget>,
) -> Option<windows::Win32::UI::Accessibility::IUIAutomationElement> {
    use windows::Win32::UI::Accessibility::{
        IUIAutomationGridItemPattern, IUIAutomationLegacyIAccessiblePattern,
        IUIAutomationSelectionItemPattern, IUIAutomationValuePattern, UIA_DataItemControlTypeId,
        UIA_GridItemPatternId, UIA_LegacyIAccessiblePatternId, UIA_ListItemControlTypeId,
        UIA_SelectionItemPatternId, UIA_ValuePatternId,
    };

    let mut pending = Vec::new();
    if let Ok(child) = unsafe { walker.GetFirstChildElement(root) } {
        pending.push(child);
    }
    let mut visited = 0usize;
    while let Some(element) = pending.pop() {
        visited += 1;
        let max_visited = if expected_target.is_some() {
            1_024
        } else {
            4_096
        };
        if visited > max_visited {
            return None;
        }
        let process_matches = unsafe { element.CurrentProcessId() }
            .map(|process_id| process_id == target_process_id)
            .unwrap_or(false);
        if process_matches {
            let is_selected = unsafe {
                element
                    .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                        UIA_SelectionItemPatternId,
                    )
                    .ok()
                    .and_then(|pattern| pattern.CurrentIsSelected().ok())
                    .map(|selected| selected.as_bool())
                    .unwrap_or(false)
            };
            let exact_target_kind = match profile {
                WindowsComputerUseTargetProfile::FileExplorer => unsafe {
                    element
                        .CurrentControlType()
                        .map(|control_type| control_type == UIA_ListItemControlTypeId)
                        .unwrap_or(false)
                        && match expected_target {
                            Some(WindowsBoundAccessibilityTarget::FileExplorer { target_name }) => {
                                element
                                    .CurrentName()
                                    .map(|name| name == target_name.as_str())
                                    .unwrap_or(false)
                            }
                            None | Some(WindowsBoundAccessibilityTarget::Any) => true,
                            Some(_) => false,
                        }
                },
                WindowsComputerUseTargetProfile::Excel => unsafe {
                    let is_data_item = element
                        .CurrentControlType()
                        .map(|control_type| control_type == UIA_DataItemControlTypeId)
                        .unwrap_or(false);
                    let supports_value = element
                        .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                        .is_ok()
                        || element
                            .GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(
                                UIA_LegacyIAccessiblePatternId,
                            )
                            .is_ok();
                    let exact_excel_target = match expected_target {
                        Some(WindowsBoundAccessibilityTarget::Excel {
                            worksheet_automation_id,
                            cell_automation_id,
                            row,
                            column,
                        }) => {
                            let address_matches = element
                                .CurrentAutomationId()
                                .map(|value| value == cell_automation_id.as_str())
                                .unwrap_or(false);
                            let grid_matches = element
                                .GetCurrentPatternAs::<IUIAutomationGridItemPattern>(
                                    UIA_GridItemPatternId,
                                )
                                .ok()
                                .is_some_and(|grid| {
                                    grid.CurrentRow().ok() == Some(*row)
                                        && grid.CurrentColumn().ok() == Some(*column)
                                });
                            let worksheet_matches = element
                                .GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(
                                    UIA_SelectionItemPatternId,
                                )
                                .ok()
                                .and_then(|selection| selection.CurrentSelectionContainer().ok())
                                .and_then(|container| container.CurrentAutomationId().ok())
                                .map(|value| value == worksheet_automation_id.as_str())
                                .unwrap_or(false);
                            address_matches && grid_matches && worksheet_matches
                        }
                        None | Some(WindowsBoundAccessibilityTarget::Any) => true,
                        Some(_) => false,
                    };
                    is_data_item && supports_value && exact_excel_target
                },
                WindowsComputerUseTargetProfile::Generic => false,
            };
            if is_selected && exact_target_kind {
                return Some(element);
            }
            if let Ok(child) = unsafe { walker.GetFirstChildElement(&element) } {
                pending.push(child);
            }
        }
        if let Ok(sibling) = unsafe { walker.GetNextSiblingElement(&element) } {
            pending.push(sibling);
        }
    }
    None
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsComputerUseAccessibilityClient;

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum WindowsComputerUseWindowBinding {
    Foreground,
    Exact {
        window_handle: isize,
        process_id: u32,
    },
}

#[cfg(windows)]
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub struct WindowsBoundComputerUseAccessibilityClient {
    window_handle: isize,
    process_id: u32,
    target: WindowsBoundAccessibilityTarget,
}

#[cfg(windows)]
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum WindowsBoundAccessibilityTarget {
    Any,
    FileExplorer {
        target_name: String,
    },
    Excel {
        worksheet_automation_id: String,
        cell_automation_id: String,
        row: i32,
        column: i32,
    },
}

#[cfg(windows)]
impl WindowsBoundAccessibilityTarget {
    fn contract_fingerprint(&self) -> String {
        match self {
            Self::Any => fingerprint_parts(&["windows-bound-target/v1", "any"]),
            Self::FileExplorer { target_name } => fingerprint_parts(&[
                "windows-bound-target/v1",
                "file-explorer",
                &fingerprint_parts(&[target_name]),
            ]),
            Self::Excel {
                worksheet_automation_id,
                cell_automation_id,
                row,
                column,
            } => fingerprint_parts(&[
                "windows-bound-target/v1",
                "excel",
                &fingerprint_parts(&[worksheet_automation_id]),
                &fingerprint_parts(&[cell_automation_id]),
                &row.to_string(),
                &column.to_string(),
            ]),
        }
    }
}

#[cfg(windows)]
#[allow(dead_code)]
impl WindowsBoundComputerUseAccessibilityClient {
    pub fn new(window_handle: isize, process_id: u32) -> Result<Self, String> {
        if window_handle == 0 || process_id == 0 {
            return Err(
                "bound Windows accessibility observation requires an exact HWND and process"
                    .to_string(),
            );
        }
        Ok(Self {
            window_handle,
            process_id,
            target: WindowsBoundAccessibilityTarget::Any,
        })
    }

    pub fn new_file_explorer(
        window_handle: isize,
        process_id: u32,
        target_name: String,
    ) -> Result<Self, String> {
        let mut client = Self::new(window_handle, process_id)?;
        if target_name.trim().is_empty() {
            return Err(
                "bound File Explorer observation requires an exact target name".to_string(),
            );
        }
        client.target = WindowsBoundAccessibilityTarget::FileExplorer { target_name };
        Ok(client)
    }

    pub fn new_excel(
        window_handle: isize,
        process_id: u32,
        worksheet_automation_id: String,
        cell_automation_id: String,
        row: i32,
        column: i32,
    ) -> Result<Self, String> {
        let mut client = Self::new(window_handle, process_id)?;
        if worksheet_automation_id.trim().is_empty()
            || cell_automation_id.trim().is_empty()
            || row < 0
            || column < 0
        {
            return Err(
                "bound Excel observation requires an exact worksheet, cell, row, and column"
                    .to_string(),
            );
        }
        client.target = WindowsBoundAccessibilityTarget::Excel {
            worksheet_automation_id,
            cell_automation_id,
            row,
            column,
        };
        Ok(client)
    }
}

#[cfg(windows)]
impl ComputerUseAccessibilityClient for WindowsComputerUseAccessibilityClient {
    fn capture_redacted_state(&self) -> Result<RedactedComputerUseState, String> {
        std::thread::spawn(|| {
            capture_windows_redacted_state(WindowsComputerUseWindowBinding::Foreground, None)
        })
        .join()
        .map_err(|_| "Windows accessibility observation thread failed".to_string())?
    }
}

#[cfg(windows)]
impl ComputerUseAccessibilityClient for WindowsBoundComputerUseAccessibilityClient {
    fn capture_redacted_state(&self) -> Result<RedactedComputerUseState, String> {
        use std::sync::mpsc;
        use std::time::Duration;

        let binding = WindowsComputerUseWindowBinding::Exact {
            window_handle: self.window_handle,
            process_id: self.process_id,
        };
        let target = self.target.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(capture_windows_redacted_state(binding, Some(&target)));
        });
        receiver.recv_timeout(Duration::from_secs(3)).map_err(|error| {
            format!(
                "bound Windows accessibility observation timed out after 3 seconds during exact target discovery: {error}"
            )
        })?
    }
}

#[cfg(windows)]
fn capture_windows_redacted_state(
    binding: WindowsComputerUseWindowBinding,
    expected_target: Option<&WindowsBoundAccessibilityTarget>,
) -> Result<RedactedComputerUseState, String> {
    use windows::Win32::Foundation::HWND;
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

    let (hwnd, expected_process_id, window_binding_name, safe_window_label) = match binding {
        WindowsComputerUseWindowBinding::Foreground => {
            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.0.is_null() {
                return Err("Windows accessibility found no foreground window".to_string());
            }
            (hwnd, None, "windows-foreground-window/v1", "Foreground")
        }
        WindowsComputerUseWindowBinding::Exact {
            window_handle,
            process_id,
        } => (
            HWND(window_handle as _),
            Some(process_id),
            "windows-exact-window/v1",
            "Bound",
        ),
    };
    let mut process_id = 0u32;
    let thread_id = unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    if thread_id == 0 || process_id == 0 {
        return Err(
            "Windows accessibility could not identify the bound window process".to_string(),
        );
    }
    if expected_process_id.is_some_and(|expected| expected != process_id) {
        return Err(
            "Windows accessibility bound HWND no longer belongs to the expected process"
                .to_string(),
        );
    }
    let mut class_buffer = [0u16; 256];
    let class_len = unsafe { GetClassNameW(hwnd, &mut class_buffer) }.max(0) as usize;
    let window_class = String::from_utf16_lossy(&class_buffer[..class_len]);
    let target_profile = WindowsComputerUseTargetProfile::from_window_class(&window_class);
    let mut title_buffer = [0u16; 1_024];
    let title_len = unsafe { GetWindowTextW(hwnd, &mut title_buffer) }.max(0) as usize;
    let window_title_fingerprint =
        fingerprint_parts(&[&String::from_utf16_lossy(&title_buffer[..title_len])]);
    let handle_identity = format!("{:p}", hwnd.0);
    let process_text = process_id.to_string();
    let thread_text = thread_id.to_string();
    let process_fingerprint = fingerprint_parts(&["windows-process/v1", process_text.as_str()]);
    let window_fingerprint = fingerprint_parts(&[
        window_binding_name,
        &handle_identity,
        &process_text,
        &thread_text,
        &fingerprint_parts(&[&window_class]),
    ]);

    let automation: IUIAutomation = unsafe {
        CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("Windows UI Automation client creation failed: {error}"))?
    };
    let control_walker = unsafe {
        automation.ControlViewWalker().map_err(|error| {
            format!("Windows UI Automation control walker is unavailable: {error}")
        })?
    };
    let focused = match binding {
        WindowsComputerUseWindowBinding::Exact { .. } => {
            if !matches!(
                target_profile,
                WindowsComputerUseTargetProfile::FileExplorer
                    | WindowsComputerUseTargetProfile::Excel
            ) {
                return Err(
                    "bound Windows accessibility observation supports only File Explorer or Excel"
                        .to_string(),
                );
            }
            let root = unsafe {
                automation.ElementFromHandle(hwnd).map_err(|error| {
                    format!("Windows UI Automation could not inspect the bound window: {error}")
                })?
            };
            current_windows_selected_accessibility_descendant(
                &root,
                &control_walker,
                process_id as i32,
                target_profile,
                expected_target,
            )
            .ok_or_else(|| {
                "Windows UI Automation found no selected target in the exact bound window"
                    .to_string()
            })?
        }
        WindowsComputerUseWindowBinding::Foreground => {
            let focused = unsafe {
                automation.GetFocusedElement().map_err(|error| {
                    format!("Windows UI Automation found no focused target: {error}")
                })?
            };
            let focused_process_id = unsafe { focused.CurrentProcessId() }.map_err(|error| {
                format!("Windows UI Automation target process is unavailable: {error}")
            })?;
            if focused_process_id <= 0 || focused_process_id as u32 != process_id {
                return Err(
                    "Windows UI Automation focus does not belong to the foreground window"
                        .to_string(),
                );
            }
            if target_profile == WindowsComputerUseTargetProfile::FileExplorer {
                let root = unsafe {
                    automation.ElementFromHandle(hwnd).map_err(|error| {
                        format!("Windows UI Automation could not inspect File Explorer: {error}")
                    })?
                };
                current_windows_selected_accessibility_descendant(
                    &root,
                    &control_walker,
                    focused_process_id,
                    target_profile,
                    None,
                )
                .ok_or_else(|| {
                    "Windows UI Automation found no selected File Explorer target".to_string()
                })?
            } else {
                focused
            }
        }
    };
    let target_process_id = unsafe { focused.CurrentProcessId() }
        .map_err(|error| format!("Windows UI Automation target process is unavailable: {error}"))?;
    if target_process_id <= 0 || target_process_id as u32 != process_id {
        return Err(
            "Windows UI Automation target does not belong to the foreground window".to_string(),
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
    let target_item_status = unsafe { focused.CurrentItemStatus() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let target_help_text = unsafe { focused.CurrentHelpText() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let target_localized_control_type = unsafe { focused.CurrentLocalizedControlType() }
        .map(|value| value.to_string())
        .unwrap_or_default();
    let target_framework_id = unsafe { focused.CurrentFrameworkId() }
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
    let expected_target_fingerprint = expected_target
        .map(WindowsBoundAccessibilityTarget::contract_fingerprint)
        .unwrap_or_else(|| fingerprint_parts(&["windows-bound-target/v1", "unspecified"]));
    let stable_target_fingerprint = if binding == WindowsComputerUseWindowBinding::Foreground {
        fingerprint_parts(&[
            "windows-accessibility-target/v1",
            target_profile.contract_name(),
            &process_text,
            &control_type_text,
            &fingerprint_parts(&[&automation_id]),
            &fingerprint_parts(&[&target_class]),
            &fingerprint_parts(&[&target_name]),
            &fingerprint_parts(&[&target_framework_id]),
            if is_password {
                "password"
            } else {
                "not_password"
            },
            if is_enabled { "enabled" } else { "disabled" },
        ])
    } else {
        fingerprint_parts(&[
            "windows-accessibility-bound-target/v1",
            target_profile.contract_name(),
            &process_text,
            &control_type_text,
            &fingerprint_parts(&[&target_class]),
            &fingerprint_parts(&[&target_name]),
            &fingerprint_parts(&[&target_framework_id]),
            &expected_target_fingerprint,
            if is_password {
                "password"
            } else {
                "not_password"
            },
            if is_enabled { "enabled" } else { "disabled" },
        ])
    };
    let target_fingerprint = if binding == WindowsComputerUseWindowBinding::Foreground {
        fingerprint_parts(&[
            &stable_target_fingerprint,
            &fingerprint_parts(&[&target_item_status]),
            &fingerprint_parts(&[&target_help_text]),
            &fingerprint_parts(&[&target_localized_control_type]),
            if is_keyboard_focusable {
                "keyboard_focusable"
            } else {
                "not_keyboard_focusable"
            },
        ])
    } else {
        stable_target_fingerprint
    };
    let application_fingerprint = fingerprint_parts(&[
        "windows-application/v1",
        target_profile.contract_name(),
        &fingerprint_parts(&[&window_class]),
        &fingerprint_parts(&[&target_class]),
    ]);
    let walker = unsafe {
        automation
            .RawViewWalker()
            .map_err(|error| format!("Windows UI Automation tree walker is unavailable: {error}"))?
    };
    let ancestor_fingerprint = current_windows_accessibility_ancestor_fingerprint(
        &focused,
        &walker,
        target_process_id,
        binding == WindowsComputerUseWindowBinding::Foreground,
    );
    let frame_fingerprint = if binding == WindowsComputerUseWindowBinding::Foreground {
        fingerprint_parts(&[
            "windows-accessibility-frame/v1",
            target_profile.contract_name(),
            &process_fingerprint,
            &window_fingerprint,
            &control_type_text,
            &fingerprint_parts(&[&automation_id]),
            &fingerprint_parts(&[&target_class]),
            &ancestor_fingerprint,
        ])
    } else {
        fingerprint_parts(&[
            "windows-accessibility-bound-frame/v1",
            target_profile.contract_name(),
            &process_fingerprint,
            &window_fingerprint,
            &control_type_text,
            &fingerprint_parts(&[&target_class]),
            &expected_target_fingerprint,
        ])
    };

    let semantic_fingerprint = if is_password {
        None
    } else {
        let mut semantic_element = focused.clone();
        let mut value = None;
        for _ in 0..=16 {
            value = current_windows_bounded_semantic_value(&semantic_element, target_profile);
            if value.is_some() {
                break;
            }
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
        application_fingerprint,
        process_fingerprint,
        window_fingerprint,
        window_title_fingerprint,
        frame_fingerprint,
        target_fingerprint,
        semantic_fingerprint,
        safe_summary: format!(
            "{safe_window_label} {} accessibility target type {} is {} and {}; {}.",
            target_profile.safe_label(),
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

    #[cfg(windows)]
    #[test]
    fn bound_windows_clients_require_exact_nonzero_hwnd_and_process() {
        use crate::kernel::capability::{ComputerControlAction, WindowsBoundComputerControlClient};

        assert!(WindowsBoundComputerUseAccessibilityClient::new(0, 1).is_err());
        assert!(WindowsBoundComputerUseAccessibilityClient::new(1, 0).is_err());
        assert!(WindowsBoundComputerControlClient::new(0, 1).is_err());
        assert!(WindowsBoundComputerControlClient::new(1, 0).is_err());
        let observation = WindowsBoundComputerUseAccessibilityClient::new(1, 1).unwrap();
        let control = WindowsBoundComputerControlClient::new(1, 1).unwrap();
        assert!(observation.capture_redacted_state().is_err());
        assert!(control
            .execute_control(
                "invalid-bound-window",
                &ComputerControlAction::SelectAccessibilityTarget,
            )
            .is_err());
    }

    struct FakeAccessibilityClient {
        states: Mutex<VecDeque<Result<RedactedComputerUseState, String>>>,
    }

    impl FakeAccessibilityClient {
        fn new(states: Vec<RedactedComputerUseState>) -> Self {
            Self {
                states: Mutex::new(states.into_iter().map(Ok).collect()),
            }
        }

        fn with_results(states: Vec<Result<RedactedComputerUseState, String>>) -> Self {
            Self {
                states: Mutex::new(states.into()),
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

    struct InMemoryStepPersistence {
        step: Mutex<ComputerUseStep>,
    }

    impl InMemoryStepPersistence {
        fn new(step: ComputerUseStep) -> Self {
            Self {
                step: Mutex::new(step),
            }
        }
    }

    impl ComputerUseStepPersistence for InMemoryStepPersistence {
        fn load_step(&self, step_id: Uuid) -> Result<ComputerUseStep, String> {
            let step = self.step.lock().unwrap().clone();
            if step.id != step_id {
                return Err("in-memory computer use step does not exist".to_string());
            }
            Ok(step)
        }

        fn persist_step(
            &self,
            step: &ComputerUseStep,
            expected_revision: u64,
        ) -> Result<(), String> {
            let mut current = self.step.lock().unwrap();
            if current.id != step.id || current.revision != expected_revision {
                return Err("in-memory computer use step changed concurrently".to_string());
            }
            *current = step.clone();
            Ok(())
        }
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
        redacted_state_with_identity(
            "application",
            "process",
            window,
            title,
            "frame",
            target,
            semantic,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn redacted_state_with_identity(
        application: &str,
        process: &str,
        window: &str,
        title: &str,
        frame: &str,
        target: &str,
        semantic: Option<&str>,
    ) -> RedactedComputerUseState {
        RedactedComputerUseState {
            application_fingerprint: fingerprint_parts(&[application]),
            process_fingerprint: fingerprint_parts(&[process]),
            window_fingerprint: fingerprint_parts(&[window]),
            window_title_fingerprint: fingerprint_parts(&[title]),
            frame_fingerprint: fingerprint_parts(&[frame]),
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
        setup_ready_step_with_action(
            store,
            screenshots,
            accessibility,
            expected_semantic,
            ComputerControlAction::TypeText {
                text: "verified text".to_string(),
            },
        )
    }

    fn setup_ready_step_with_action(
        store: &EventStore,
        screenshots: &FakeScreenshotClient,
        accessibility: &FakeAccessibilityClient,
        expected_semantic: &str,
        action: ComputerControlAction,
    ) -> (ComputerUseStep, Uuid) {
        let observation = capture_computer_use_observation(
            ComputerUseObservationPhase::PreAction,
            screenshots,
            accessibility,
        )
        .unwrap();
        let (_, observed) = persist_observed_computer_use_session(
            store,
            None,
            "Update an isolated Notepad-like editor.".to_string(),
            ComputerUseUndoCapability::None,
            observation,
        )
        .unwrap();
        let bound = bind_computer_use_action(
            store,
            observed.id,
            action,
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
            ComputerUseApprovalActor::User,
        )
        .unwrap();
        (ready, approval_id)
    }

    #[test]
    fn observation_retry_is_bounded_and_never_retries_an_action() {
        let screenshots = FakeScreenshotClient::new(COMPUTER_USE_OBSERVATION_MAX_ATTEMPTS);
        let accessibility = FakeAccessibilityClient::with_results(vec![
            Err("transient accessibility failure".to_string()),
            Ok(redacted_state("window", "target", Some("before"))),
        ]);
        let observation = capture_computer_use_observation(
            ComputerUseObservationPhase::PreAction,
            &screenshots,
            &accessibility,
        )
        .expect("the second observation attempt succeeds");
        assert_eq!(
            observation.screenshot_evidence_ref,
            "computer-screenshots/fake-2.png"
        );

        let screenshots = FakeScreenshotClient::new(COMPUTER_USE_OBSERVATION_MAX_ATTEMPTS);
        let accessibility = FakeAccessibilityClient::with_results(vec![
            Err("first failure".to_string()),
            Err("second failure".to_string()),
        ]);
        let error = capture_computer_use_observation(
            ComputerUseObservationPhase::PreAction,
            &screenshots,
            &accessibility,
        )
        .expect_err("observation stops at the fixed retry bound");
        assert!(error.contains("failed after 2 bounded attempts"));
    }

    #[test]
    fn wrong_application_process_window_or_frame_stops_before_input() {
        let cases = [
            (
                "application",
                redacted_state_with_identity(
                    "changed-application",
                    "process",
                    "window",
                    "stable-title",
                    "frame",
                    "target",
                    Some("before"),
                ),
            ),
            (
                "process",
                redacted_state_with_identity(
                    "application",
                    "changed-process",
                    "window",
                    "stable-title",
                    "frame",
                    "target",
                    Some("before"),
                ),
            ),
            (
                "window",
                redacted_state_with_identity(
                    "application",
                    "process",
                    "changed-window",
                    "stable-title",
                    "frame",
                    "target",
                    Some("before"),
                ),
            ),
            (
                "frame",
                redacted_state_with_identity(
                    "application",
                    "process",
                    "window",
                    "stable-title",
                    "changed-frame",
                    "target",
                    Some("before"),
                ),
            ),
        ];

        for (label, drifted) in cases {
            let store = EventStore::open_memory().unwrap();
            let screenshots = FakeScreenshotClient::new(1);
            let accessibility = FakeAccessibilityClient::new(vec![
                redacted_state("window", "target", Some("before")),
                drifted,
            ]);
            let control = FakeControlClient::succeeding();
            let (ready, approval_id) =
                setup_ready_step(&store, &screenshots, &accessibility, "after");

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
                ComputerUseStepStatus::NeedsReplan,
                "{label} drift must require replanning"
            );
            assert!(result.step.approval_request_id.is_none());
            assert!(result.step.approval_actor.is_none());
            assert_eq!(
                control.calls.load(Ordering::SeqCst),
                0,
                "{label} drift must stop before input"
            );
        }
    }

    #[test]
    fn file_explorer_folder_or_file_drift_stops_before_semantic_selection() {
        let cases = [
            (
                "folder",
                redacted_state_with_identity(
                    "file-explorer",
                    "explorer-process",
                    "explorer-window",
                    "isolated-folder",
                    "other-folder-frame",
                    "generated-file",
                    Some("selection:not_selected"),
                ),
            ),
            (
                "file",
                redacted_state_with_identity(
                    "file-explorer",
                    "explorer-process",
                    "explorer-window",
                    "isolated-folder",
                    "isolated-folder-frame",
                    "other-file",
                    Some("selection:not_selected"),
                ),
            ),
        ];

        for (label, drifted) in cases {
            let store = EventStore::open_memory().unwrap();
            let screenshots = FakeScreenshotClient::new(1);
            let accessibility = FakeAccessibilityClient::new(vec![
                redacted_state_with_identity(
                    "file-explorer",
                    "explorer-process",
                    "explorer-window",
                    "isolated-folder",
                    "isolated-folder-frame",
                    "generated-file",
                    Some("selection:not_selected"),
                ),
                drifted,
            ]);
            let control = FakeControlClient::succeeding();
            let (ready, approval_id) = setup_ready_step_with_action(
                &store,
                &screenshots,
                &accessibility,
                "selection:selected",
                ComputerControlAction::SelectAccessibilityTarget,
            );

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
                ComputerUseStepStatus::NeedsReplan,
                "{label} drift must require replanning"
            );
            assert_eq!(control.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn excel_workbook_sheet_or_cell_drift_stops_before_semantic_write() {
        let cases = [
            (
                "workbook",
                redacted_state_with_identity(
                    "excel",
                    "excel-process",
                    "excel-window",
                    "other-workbook",
                    "sheet-one-frame",
                    "cell-b3",
                    Some("before"),
                ),
            ),
            (
                "sheet",
                redacted_state_with_identity(
                    "excel",
                    "excel-process",
                    "excel-window",
                    "generated-workbook",
                    "other-sheet-frame",
                    "cell-b3",
                    Some("before"),
                ),
            ),
            (
                "cell",
                redacted_state_with_identity(
                    "excel",
                    "excel-process",
                    "excel-window",
                    "generated-workbook",
                    "sheet-one-frame",
                    "cell-c4",
                    Some("before"),
                ),
            ),
        ];

        for (label, drifted) in cases {
            let store = EventStore::open_memory().unwrap();
            let screenshots = FakeScreenshotClient::new(1);
            let accessibility = FakeAccessibilityClient::new(vec![
                redacted_state_with_identity(
                    "excel",
                    "excel-process",
                    "excel-window",
                    "generated-workbook",
                    "sheet-one-frame",
                    "cell-b3",
                    Some("before"),
                ),
                drifted,
            ]);
            let control = FakeControlClient::succeeding();
            let (ready, approval_id) = setup_ready_step_with_action(
                &store,
                &screenshots,
                &accessibility,
                "after",
                ComputerControlAction::SetAccessibilityValue {
                    value: "after".to_string(),
                },
            );

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
                ComputerUseStepStatus::NeedsReplan,
                "{label} drift must require replanning"
            );
            assert_eq!(control.calls.load(Ordering::SeqCst), 0);
        }
    }

    #[cfg(windows)]
    #[test]
    fn excel_object_model_corroboration_blocks_uia_false_completion_and_wrong_targets() {
        let directory = tempfile::tempdir().expect("temp dir");
        let workbook = directory.path().join("generated.xlsx");
        std::fs::write(&workbook, b"isolated workbook placeholder").expect("workbook placeholder");
        let workbook_text = workbook.to_string_lossy();
        let result = |value: &str, other_b3: &str, sheet: &str, cell: &str| {
            format!(
                "{value}\ntarget-sentinel\n{other_b3}\nother-sentinel\n{workbook_text}\n1\n{sheet}\n{cell}\n"
            )
        };

        let missing_write = validate_excel_object_model_result(
            &result("before", "other-before", "C5B_Target", "B3"),
            "after",
            &workbook,
            "C5B_Target",
            "B3",
        )
        .expect_err("UIA-only value change must not verify");
        assert!(missing_write.contains("exact target write is missing"));

        let wrong_sheet = validate_excel_object_model_result(
            &result("after", "other-before", "C5B_Other", "B3"),
            "after",
            &workbook,
            "C5B_Target",
            "B3",
        )
        .expect_err("wrong sheet must fail");
        assert!(wrong_sheet.contains("wrong workbook/sheet/cell binding"));

        let wrong_cell = validate_excel_object_model_result(
            &result("after", "other-before", "C5B_Target", "C4"),
            "after",
            &workbook,
            "C5B_Target",
            "B3",
        )
        .expect_err("wrong cell must fail");
        assert!(wrong_cell.contains("wrong workbook/sheet/cell binding"));

        let wrong_target = validate_excel_object_model_result(
            &result("after", "changed-wrong-cell", "C5B_Target", "B3"),
            "after",
            &workbook,
            "C5B_Target",
            "B3",
        )
        .expect_err("wrong-target write must fail");
        assert!(wrong_target.contains("wrong-target write"));

        validate_excel_object_model_result(
            &result("after", "other-before", "C5B_Target", "B3"),
            "after",
            &workbook,
            "C5B_Target",
            "B3",
        )
        .expect("UIA and object-model outcome corroborate");
    }

    #[cfg(windows)]
    #[test]
    fn explorer_shell_corroboration_rejects_multiselected_decoy() {
        let directory = tempfile::tempdir().expect("temp dir");
        let target = directory.path().join("target.txt");
        let decoy = directory.path().join("decoy.txt");
        std::fs::write(&target, b"target").expect("target");
        std::fs::write(&decoy, b"decoy").expect("decoy");

        let error = validate_exact_explorer_selection_paths(&[target.clone(), decoy], &target)
            .expect_err("multiple selected paths must fail closed");
        assert!(error.contains("selection count was 2, expected 1"));
        validate_exact_explorer_selection_paths(&[target.clone()], &target)
            .expect("one exact selected path corroborates");
    }

    #[cfg(windows)]
    #[test]
    fn excel_smoke_deadline_fails_closed_with_exact_phase() {
        validate_excel_smoke_deadline(
            std::time::Duration::from_secs(45),
            "discover-exact-uia-cell",
        )
        .expect("deadline boundary remains allowed");
        let error = validate_excel_smoke_deadline(
            std::time::Duration::from_millis(45_001),
            "legacy-action-host",
        )
        .expect_err("elapsed smoke phase must fail closed");
        assert!(error.contains("45 second internal deadline"));
        assert!(error.contains("legacy-action-host"));
    }

    #[test]
    fn stale_approved_observation_requires_replan_before_revalidation() {
        let store = EventStore::open_memory().unwrap();
        let screenshots = FakeScreenshotClient::new(1);
        let accessibility =
            FakeAccessibilityClient::new(vec![redacted_state("window", "target", Some("before"))]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step(&store, &screenshots, &accessibility, "after");
        let expired_at = ready.pre_observation.valid_until + chrono::Duration::milliseconds(1);

        let result = execute_ready_computer_use_step_at(
            &store,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &screenshots,
            &accessibility,
            &control,
            expired_at,
        )
        .unwrap();

        assert_eq!(result.step.status, ComputerUseStepStatus::NeedsReplan);
        assert!(result.step.approval_request_id.is_none());
        assert!(result.step.approval_actor.is_none());
        assert_eq!(control.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn action_mutation_after_approval_is_rejected_before_input() {
        let store = EventStore::open_memory().unwrap();
        let screenshots = FakeScreenshotClient::new(1);
        let accessibility =
            FakeAccessibilityClient::new(vec![redacted_state("window", "target", Some("before"))]);
        let (mut ready, approval_id) =
            setup_ready_step(&store, &screenshots, &accessibility, "after");
        ready.action.as_mut().unwrap().action = ComputerControlAction::PressKey {
            key: "ENTER".to_string(),
        };
        let persistence = InMemoryStepPersistence::new(ready.clone());
        let no_observation = FakeAccessibilityClient::new(Vec::new());
        let no_screenshot = FakeScreenshotClient::new(0);
        let control = FakeControlClient::succeeding();

        let error = execute_ready_computer_use_step(
            &persistence,
            ready.id,
            ComputerUseExecutionPermit {
                approval_request_id: approval_id,
                local_unlock_confirmed: true,
            },
            &no_screenshot,
            &no_observation,
            &control,
        )
        .expect_err("mutated approved action must fail domain validation");

        assert!(error.contains("action fingerprint is inconsistent"));
        assert_eq!(control.calls.load(Ordering::SeqCst), 0);
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
    fn excel_semantic_write_without_a_cell_receipt_cannot_complete() {
        let store = EventStore::open_memory().unwrap();
        let screenshots = FakeScreenshotClient::new(2);
        let accessibility = FakeAccessibilityClient::new(vec![
            redacted_state_with_identity(
                "excel",
                "excel-process",
                "excel-window",
                "generated-workbook",
                "sheet-one-frame",
                "cell-b3",
                Some("before"),
            ),
            redacted_state_with_identity(
                "excel",
                "excel-process",
                "excel-window",
                "generated-workbook",
                "sheet-one-frame",
                "cell-b3",
                Some("before"),
            ),
            redacted_state_with_identity(
                "excel",
                "excel-process",
                "excel-window",
                "generated-workbook",
                "sheet-one-frame",
                "cell-b3",
                None,
            ),
        ]);
        let control = FakeControlClient::succeeding();
        let (ready, approval_id) = setup_ready_step_with_action(
            &store,
            &screenshots,
            &accessibility,
            "after",
            ComputerControlAction::SetAccessibilityValue {
                value: "after".to_string(),
            },
        );

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
    fn powershell_literal(value: &std::path::Path) -> String {
        format!("'{}'", value.to_string_lossy().replace('\'', "''"))
    }

    #[cfg(windows)]
    fn wait_for_file(path: &std::path::Path, attempts: usize) -> bool {
        use std::thread;
        use std::time::Duration;

        (0..attempts).any(|_| {
            if path.is_file() {
                true
            } else {
                thread::sleep(Duration::from_millis(250));
                false
            }
        })
    }

    #[cfg(windows)]
    fn c5b_installed_smoke_directory(name: &str) -> Result<std::path::PathBuf, String> {
        const ROOT_ENV: &str = "DEEPSEEK_AGENT_OS_C5B_SMOKE_ROOT";

        let root = std::env::var_os(ROOT_ENV)
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
            .ok_or_else(|| {
                format!(
                    "{ROOT_ENV} must name a fresh absolute authorized isolation root for installed C5B smokes"
                )
            })?;
        if !root.is_absolute() {
            return Err(format!(
                "{ROOT_ENV} must be an absolute authorized isolation root"
            ));
        }
        let directory = root.join(name);
        if directory.exists()
            && std::fs::read_dir(&directory)
                .map_err(|error| format!("C5B smoke directory is unreadable: {error}"))?
                .next()
                .is_some()
        {
            return Err(format!(
                "C5B smoke directory must be fresh and empty: {}",
                directory.display()
            ));
        }
        std::fs::create_dir_all(&directory)
            .map_err(|error| format!("C5B smoke directory creation failed: {error}"))?;
        Ok(directory)
    }

    #[cfg(windows)]
    fn select_file_in_exact_explorer_window(
        directory: &std::path::Path,
        file_name: &str,
    ) -> Result<isize, String> {
        use std::process::Command;

        let directory = powershell_literal(directory);
        let file_name = format!("'{}'", file_name.replace('\'', "''"));
        let script = format!(
            r#"
$targetDirectory = [IO.Path]::GetFullPath({directory}).TrimEnd('\')
$shell = New-Object -ComObject Shell.Application
$window = @($shell.Windows()) | Where-Object {{
  try {{
    $_.FullName -like '*explorer.exe' -and
      [IO.Path]::GetFullPath(([Uri]$_.LocationURL).LocalPath).TrimEnd('\') -eq $targetDirectory
  }} catch {{
    $false
  }}
}} | Select-Object -First 1
if ($null -eq $window) {{ throw 'isolated File Explorer window was not found' }}
$item = $window.Document.Folder.ParseName({file_name})
if ($null -eq $item) {{ throw 'isolated File Explorer target file was not found' }}
$window.Document.SelectItem($item, 29)
[Console]::Out.Write([string]$window.HWND)
"#,
        );
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .output()
            .map_err(|error| format!("File Explorer target setup failed: {error}"))?;
        if output.status.success() {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<isize>()
                .map_err(|error| format!("File Explorer did not return its exact HWND: {error}"))
        } else {
            Err(format!(
                "File Explorer target setup failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    #[cfg(windows)]
    fn windows_process_id_for_handle(window_handle: isize) -> Result<u32, String> {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

        if window_handle == 0 {
            return Err("exact Windows HWND is empty".to_string());
        }
        let mut process_id = 0u32;
        let thread_id =
            unsafe { GetWindowThreadProcessId(HWND(window_handle as _), Some(&mut process_id)) };
        if thread_id == 0 || process_id == 0 {
            return Err("exact Windows HWND has no live process identity".to_string());
        }
        Ok(process_id)
    }

    #[cfg(windows)]
    fn close_exact_explorer_window(directory: &std::path::Path) {
        use std::process::Command;

        let directory = powershell_literal(directory);
        let script = format!(
            r#"
$targetDirectory = [IO.Path]::GetFullPath({directory}).TrimEnd('\')
$shell = New-Object -ComObject Shell.Application
@($shell.Windows()) | Where-Object {{
  try {{
    $_.FullName -like '*explorer.exe' -and
      [IO.Path]::GetFullPath(([Uri]$_.LocationURL).LocalPath).TrimEnd('\') -eq $targetDirectory
  }} catch {{
    $false
  }}
}} | ForEach-Object {{ $_.Quit() }}
"#,
        );
        let _ = Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .status();
    }

    #[cfg(windows)]
    fn validate_exact_explorer_selection_paths(
        selected_paths: &[std::path::PathBuf],
        expected_path: &std::path::Path,
    ) -> Result<(), String> {
        if selected_paths.len() != 1 {
            return Err(format!(
                "exact File Explorer selection count was {}, expected 1",
                selected_paths.len()
            ));
        }
        let actual = selected_paths[0]
            .canonicalize()
            .map_err(|error| format!("selected File Explorer path is invalid: {error}"))?;
        let expected = expected_path
            .canonicalize()
            .map_err(|error| format!("expected File Explorer path is invalid: {error}"))?;
        if actual != expected {
            return Err(format!(
                "selected File Explorer path mismatch: {}",
                actual.display()
            ));
        }
        Ok(())
    }

    #[cfg(windows)]
    fn corroborate_exact_explorer_selection(
        window_handle: isize,
        directory: &std::path::Path,
        file_name: &str,
    ) -> Result<(), String> {
        use std::process::Command;

        let expected_path = directory.join(file_name);
        let directory = powershell_literal(directory);
        let script = format!(
            r#"
$targetDirectory = [IO.Path]::GetFullPath({directory}).TrimEnd('\')
$shell = New-Object -ComObject Shell.Application
$window = @($shell.Windows()) | Where-Object {{
  try {{
    [Int64]$_.HWND -eq {window_handle} -and
      $_.FullName -like '*explorer.exe' -and
      [IO.Path]::GetFullPath(([Uri]$_.LocationURL).LocalPath).TrimEnd('\') -eq $targetDirectory
  }} catch {{
    $false
  }}
}} | Select-Object -First 1
if ($null -eq $window) {{ throw 'exact File Explorer HWND and LocationURL did not corroborate' }}
@($window.Document.SelectedItems()) | ForEach-Object {{
  [Console]::Out.WriteLine([IO.Path]::GetFullPath([string]$_.Path))
}}
"#,
        );
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .output()
            .map_err(|error| format!("File Explorer selection corroboration failed: {error}"))?;
        if output.status.success() {
            let selected_paths = String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| std::path::PathBuf::from(line.trim()))
                .collect::<Vec<_>>();
            validate_exact_explorer_selection_paths(&selected_paths, &expected_path)
        } else {
            Err(format!(
                "File Explorer UIA target lacked Shell LocationURL/SelectedItems corroboration: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    }

    #[cfg(windows)]
    struct CorroboratedExplorerAccessibilityClient {
        inner: WindowsBoundComputerUseAccessibilityClient,
        window_handle: isize,
        directory: std::path::PathBuf,
        target_name: String,
    }

    #[cfg(windows)]
    impl ComputerUseAccessibilityClient for CorroboratedExplorerAccessibilityClient {
        fn capture_redacted_state(&self) -> Result<RedactedComputerUseState, String> {
            let state = self.inner.capture_redacted_state()?;
            corroborate_exact_explorer_selection(
                self.window_handle,
                &self.directory,
                &self.target_name,
            )?;
            Ok(state)
        }
    }

    #[cfg(windows)]
    fn validate_excel_smoke_deadline(
        elapsed: std::time::Duration,
        phase: &str,
    ) -> Result<(), String> {
        if elapsed > std::time::Duration::from_secs(45) {
            Err(format!(
                "Excel smoke exceeded its 45 second internal deadline in phase {phase} after {} ms",
                elapsed.as_millis()
            ))
        } else {
            Ok(())
        }
    }

    #[cfg(windows)]
    fn request_excel_object_model_result(
        verify: &std::path::Path,
        result_file: &std::path::Path,
    ) -> Result<String, String> {
        use std::thread;
        use std::time::Duration;

        if verify.exists() {
            std::fs::remove_file(verify).map_err(|error| {
                format!("stale Excel verifier request could not be removed: {error}")
            })?;
        }
        if result_file.exists() {
            std::fs::remove_file(result_file).map_err(|error| {
                format!("stale Excel verifier result could not be removed: {error}")
            })?;
        }
        std::fs::write(verify, b"verify")
            .map_err(|error| format!("Excel verifier request could not be written: {error}"))?;
        for _ in 0..24 {
            if result_file.is_file() && !verify.exists() {
                return std::fs::read_to_string(result_file)
                    .map_err(|error| format!("Excel verifier result is unreadable: {error}"));
            }
            thread::sleep(Duration::from_millis(250));
        }
        Err(
            "Excel object-model verifier timed out after 6 seconds without a bounded result"
                .to_string(),
        )
    }

    #[cfg(windows)]
    fn validate_excel_object_model_result(
        actual: &str,
        expected_value: &str,
        workbook: &std::path::Path,
        expected_sheet: &str,
        expected_cell: &str,
    ) -> Result<(), String> {
        let fields = actual.lines().collect::<Vec<_>>();
        if fields.len() != 8 {
            return Err(format!(
                "Excel object-model verifier returned {} fields instead of 8",
                fields.len()
            ));
        }
        if fields[1] != "target-sentinel"
            || fields[2] != "other-before"
            || fields[3] != "other-sentinel"
        {
            return Err(format!(
                "Excel object-model verifier found a wrong-target write: target A1={:?}, other B3={:?}, other A1={:?}",
                fields[1], fields[2], fields[3]
            ));
        }
        let actual_workbook = std::path::PathBuf::from(fields[4])
            .canonicalize()
            .map_err(|error| format!("Excel reported workbook path is invalid: {error}"))?;
        let expected_workbook = workbook
            .canonicalize()
            .map_err(|error| format!("generated workbook path is invalid: {error}"))?;
        if actual_workbook != expected_workbook
            || fields[5] != "1"
            || fields[6] != expected_sheet
            || fields[7] != expected_cell
        {
            return Err(format!(
                "Excel object-model verifier found a wrong workbook/sheet/cell binding: workbook={:?}, count={:?}, sheet={:?}, cell={:?}",
                fields[4], fields[5], fields[6], fields[7]
            ));
        }
        if fields[0] != expected_value {
            return Err(format!(
                "Excel exact target write is missing: {expected_sheet}!{expected_cell} expected {expected_value:?}, actual {:?}; no wrong-target sentinel changed",
                fields[0]
            ));
        }
        Ok(())
    }

    #[cfg(windows)]
    struct ExcelObjectModelCorroboratingControlClient {
        inner: crate::kernel::capability::WindowsBoundComputerControlClient,
        verify: std::path::PathBuf,
        result_file: std::path::PathBuf,
        workbook: std::path::PathBuf,
        expected_value: String,
        expected_sheet: String,
        expected_cell: String,
    }

    #[cfg(windows)]
    impl ComputerControlClient for ExcelObjectModelCorroboratingControlClient {
        fn execute_control(
            &self,
            target: &str,
            action: &ComputerControlAction,
        ) -> Result<crate::kernel::capability::ComputerControlExecution, String> {
            let execution = self.inner.execute_control(target, action)?;
            let actual = request_excel_object_model_result(&self.verify, &self.result_file)?;
            validate_excel_object_model_result(
                &actual,
                &self.expected_value,
                &self.workbook,
                &self.expected_sheet,
                &self.expected_cell,
            )?;
            Ok(execution)
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a visible installed File Explorer session over isolated generated files"]
    fn windows_file_explorer_isolated_selection_smoke_verifies_exact_file() {
        use std::process::Command;
        use std::thread;
        use std::time::Duration;

        use crate::kernel::capability::{
            WindowsBoundComputerControlClient, WindowsBoundComputerScreenshotClient,
        };

        let directory = c5b_installed_smoke_directory("file-explorer")
            .expect("isolated File Explorer directory");
        let target_name = "c5b-target.txt";
        let decoy_name = "c5b-decoy.txt";
        let target_path = directory.join(target_name);
        let decoy_path = directory.join(decoy_name);
        std::fs::write(&target_path, b"exact isolated C5B file")
            .expect("isolated target file is generated");
        std::fs::write(&decoy_path, b"decoy").expect("isolated decoy file is generated");
        let expected_bytes = std::fs::read(&target_path).expect("target file is readable");
        let expected_decoy_bytes = std::fs::read(&decoy_path).expect("decoy file is readable");
        let _ = Command::new("explorer.exe")
            .arg(&directory)
            .spawn()
            .expect("File Explorer starts")
            .wait();
        let result = (|| -> Result<(), String> {
            let selected_semantic = accessibility_value_semantic_fingerprint("selection:selected")?;
            let not_selected_semantic =
                accessibility_value_semantic_fingerprint("selection:not_selected")?;
            let mut last_diagnostic = "setup not attempted".to_string();
            let target_binding = (0..40).find_map(|_| {
                let window_handle =
                    match select_file_in_exact_explorer_window(&directory, target_name) {
                        Ok(window_handle) => window_handle,
                        Err(error) => {
                            last_diagnostic = error;
                            thread::sleep(Duration::from_millis(250));
                            return None;
                        }
                    };
                let process_id = match windows_process_id_for_handle(window_handle) {
                    Ok(process_id) => process_id,
                    Err(error) => {
                        last_diagnostic = error;
                        thread::sleep(Duration::from_millis(250));
                        return None;
                    }
                };
                let accessibility =
                    match WindowsBoundComputerUseAccessibilityClient::new_file_explorer(
                        window_handle,
                        process_id,
                        target_name.to_string(),
                    ) {
                        Ok(accessibility) => CorroboratedExplorerAccessibilityClient {
                            inner: accessibility,
                            window_handle,
                            directory: directory.clone(),
                            target_name: target_name.to_string(),
                        },
                        Err(error) => {
                            last_diagnostic = error;
                            return None;
                        }
                    };
                thread::sleep(Duration::from_millis(250));
                match accessibility.capture_redacted_state() {
                    Ok(state)
                        if state.safe_summary.contains("File Explorer")
                            && state.semantic_fingerprint.as_deref()
                                == Some(selected_semantic.as_str()) =>
                    {
                        Some((window_handle, process_id, accessibility, state))
                    }
                    Ok(state) => {
                        let semantic = if state.semantic_fingerprint.as_deref()
                            == Some(not_selected_semantic.as_str())
                        {
                            "not-selected"
                        } else if state.semantic_fingerprint.is_some() {
                            "other"
                        } else {
                            "unavailable"
                        };
                        last_diagnostic = format!("{}; semantic {semantic}", state.safe_summary);
                        None
                    }
                    Err(error) => {
                        last_diagnostic = error;
                        None
                    }
                }
            });
            let (window_handle, process_id, accessibility, target_state) =
                target_binding.ok_or_else(|| {
                format!(
                    "File Explorer did not expose the selected isolated file in its exact HWND through UI Automation: {last_diagnostic}"
                )
            })?;
            let decoy_window = select_file_in_exact_explorer_window(&directory, decoy_name)?;
            if decoy_window != window_handle {
                return Err(
                    "File Explorer exact HWND changed while selecting the decoy".to_string()
                );
            }
            thread::sleep(Duration::from_millis(300));
            let decoy_accessibility = CorroboratedExplorerAccessibilityClient {
                inner: WindowsBoundComputerUseAccessibilityClient::new_file_explorer(
                    window_handle,
                    process_id,
                    decoy_name.to_string(),
                )?,
                window_handle,
                directory: directory.clone(),
                target_name: decoy_name.to_string(),
            };
            let decoy_state = decoy_accessibility.capture_redacted_state()?;
            if decoy_state.target_fingerprint == target_state.target_fingerprint {
                return Err(
                    "File Explorer target fingerprint did not distinguish the isolated files"
                        .to_string(),
                );
            }
            let restored_window = select_file_in_exact_explorer_window(&directory, target_name)?;
            if restored_window != window_handle {
                return Err(
                    "File Explorer exact HWND changed while restoring the target".to_string(),
                );
            }
            thread::sleep(Duration::from_millis(300));
            let restored_state = accessibility.capture_redacted_state()?;
            if restored_state.frame_fingerprint != target_state.frame_fingerprint
                || restored_state.target_fingerprint != target_state.target_fingerprint
                || restored_state.semantic_fingerprint != target_state.semantic_fingerprint
            {
                let component = |label: &str, before: &str, after: &str| {
                    format!(
                        "{label}={} (pre={before}, restored={after})",
                        if before == after { "match" } else { "mismatch" }
                    )
                };
                return Err(format!(
                    "File Explorer could not restore the exact isolated folder/file identity: {}; {}; {}; semantic={} (pre={:?}, restored={:?})",
                    component(
                        "frame/ancestor",
                        &target_state.frame_fingerprint,
                        &restored_state.frame_fingerprint,
                    ),
                    component(
                        "target",
                        &target_state.target_fingerprint,
                        &restored_state.target_fingerprint,
                    ),
                    component(
                        "window",
                        &target_state.window_fingerprint,
                        &restored_state.window_fingerprint,
                    ),
                    if target_state.semantic_fingerprint == restored_state.semantic_fingerprint {
                        "match"
                    } else {
                        "mismatch"
                    },
                    target_state.semantic_fingerprint,
                    restored_state.semantic_fingerprint,
                ));
            }

            let store = EventStore::open(directory.join("file-explorer-smoke.db"))
                .map_err(|error| error.to_string())?;
            let screenshot = WindowsBoundComputerScreenshotClient::new(
                directory.join("screenshots"),
                window_handle,
                process_id,
            )?;
            let observation = capture_computer_use_observation(
                ComputerUseObservationPhase::PreAction,
                &screenshot,
                &accessibility,
            )?;
            let (_, observed) = persist_observed_computer_use_session(
                &store,
                None,
                "Select one exact generated file in an isolated File Explorer window.".to_string(),
                ComputerUseUndoCapability::None,
                observation,
            )?;
            let bound = bind_computer_use_action(
                &store,
                observed.id,
                ComputerControlAction::SelectAccessibilityTarget,
                "Select the exact focused generated file through UI Automation.".to_string(),
                ComputerUsePostcondition::TargetSemanticFingerprintEquals {
                    expected: selected_semantic,
                },
            )?;
            let approval_id = Uuid::new_v4();
            approve_computer_use_step(
                &store,
                bound.id,
                approval_id,
                &bound
                    .action
                    .as_ref()
                    .ok_or_else(|| "File Explorer smoke action is missing".to_string())?
                    .action_fingerprint,
                ComputerUseApprovalActor::User,
            )?;
            let control = WindowsBoundComputerControlClient::new_file_explorer(
                window_handle,
                process_id,
                target_name.to_string(),
            )?;
            let run = execute_ready_computer_use_step(
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
            if run.step.status != ComputerUseStepStatus::Verified
                || run.step.action_start_count != 1
            {
                return Err(format!(
                    "File Explorer smoke ended in {:?} with {} action starts",
                    run.step.status, run.step.action_start_count
                ));
            }
            if std::fs::read(&target_path).map_err(|error| error.to_string())? != expected_bytes {
                return Err("File Explorer selection changed the generated file".to_string());
            }
            if std::fs::read(&decoy_path).map_err(|error| error.to_string())?
                != expected_decoy_bytes
            {
                return Err("File Explorer selection changed the generated decoy file".to_string());
            }
            Ok(())
        })();
        close_exact_explorer_window(&directory);
        result.expect("isolated File Explorer selection verifies");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires visible installed Microsoft Excel over an isolated generated workbook"]
    fn windows_excel_isolated_cell_value_smoke_verifies_exact_outcome() {
        use std::process::Command;
        use std::thread;
        use std::time::Duration;

        use crate::kernel::capability::{
            WindowsBoundComputerControlClient, WindowsBoundComputerScreenshotClient,
        };
        use wait_timeout::ChildExt;

        let directory = c5b_installed_smoke_directory("excel").expect("isolated Excel directory");
        let workbook = directory.join("c5b-generated.xlsx");
        let ready = directory.join("excel-ready.txt");
        let stop = directory.join("excel-stop.txt");
        let verify = directory.join("excel-verify.txt");
        let result_file = directory.join("excel-result.txt");
        let workbook_literal = powershell_literal(&workbook);
        let ready_literal = powershell_literal(&ready);
        let stop_literal = powershell_literal(&stop);
        let verify_literal = powershell_literal(&verify);
        let result_literal = powershell_literal(&result_file);
        let script = format!(
            r#"
$excel = $null
$workbook = $null
try {{
  $excel = New-Object -ComObject Excel.Application
  $excel.Visible = $true
  $excel.DisplayAlerts = $false
  $workbook = $excel.Workbooks.Add()
  $target = $workbook.Worksheets.Item(1)
  $target.Name = 'C5B_Target'
  $other = $workbook.Worksheets.Add()
  $other.Name = 'C5B_Other'
  $target.Activate()
  $target.Range('A1').Value2 = 'target-sentinel'
  $target.Range('B3').Value2 = 'before'
  $other.Range('A1').Value2 = 'other-sentinel'
  $other.Range('B3').Value2 = 'other-before'
  $target.Activate()
  $target.Range('B3').Select()
  $workbook.SaveAs({workbook_literal}, 51)
  [IO.File]::WriteAllText({ready_literal}, [string]$excel.Hwnd)
  while (-not (Test-Path -LiteralPath {stop_literal})) {{
    if (Test-Path -LiteralPath {verify_literal}) {{
      [IO.File]::WriteAllLines({result_literal}, [string[]]@(
        [string]$target.Range('B3').Value2,
        [string]$target.Range('A1').Value2,
        [string]$other.Range('B3').Value2,
        [string]$other.Range('A1').Value2,
        [string]$workbook.FullName,
        [string]$excel.Workbooks.Count,
        [string]$excel.ActiveSheet.Name,
        [string]$excel.ActiveCell.Address($false, $false)
      ))
      Remove-Item -LiteralPath {verify_literal} -Force
    }}
    Start-Sleep -Milliseconds 100
  }}
}} finally {{
  if ($null -ne $workbook) {{ $workbook.Close($false) }}
  if ($null -ne $excel) {{ $excel.Quit() }}
}}
"#,
        );
        let mut excel_host = Command::new("powershell.exe")
            .args(["-NoProfile", "-STA", "-Command", &script])
            .spawn()
            .expect("isolated Excel host starts");
        let run_result = (|| -> Result<(), String> {
            let smoke_started = std::time::Instant::now();
            let phase = |name: &str| -> Result<(), String> {
                let elapsed = smoke_started.elapsed();
                eprintln!(
                    "C5B Excel smoke phase={name} elapsed_ms={}",
                    elapsed.as_millis()
                );
                validate_excel_smoke_deadline(elapsed, name)
            };
            phase("wait-for-workbook")?;
            if !wait_for_file(&ready, 32) {
                return Err(
                    "Excel timed out after 8 seconds opening the isolated generated workbook"
                        .to_string(),
                );
            }
            phase("bind-exact-hwnd")?;
            let hwnd = std::fs::read_to_string(&ready)
                .map_err(|error| error.to_string())?
                .trim()
                .to_string();
            if hwnd.is_empty() {
                return Err("Excel did not report its exact window handle".to_string());
            }
            let window_handle = hwnd
                .parse::<isize>()
                .map_err(|error| format!("Excel reported an invalid HWND: {error}"))?;
            let process_id = windows_process_id_for_handle(window_handle)?;
            let accessibility = WindowsBoundComputerUseAccessibilityClient::new_excel(
                window_handle,
                process_id,
                "C5B_Target".to_string(),
                "B3".to_string(),
                3,
                2,
            )?;
            let before_semantic = accessibility_value_semantic_fingerprint("before")?;
            phase("discover-exact-uia-cell")?;
            let mut last_target_diagnostic = "exact target discovery was not attempted".to_string();
            let mut pre_state = None;
            for attempt in 1..=4 {
                thread::sleep(Duration::from_millis(250));
                match accessibility.capture_redacted_state() {
                    Ok(state)
                        if state.safe_summary.contains("Excel")
                            && state.semantic_fingerprint.as_deref()
                                == Some(before_semantic.as_str()) =>
                    {
                        pre_state = Some(state);
                        break;
                    }
                    Ok(state) => {
                        last_target_diagnostic = format!(
                            "attempt {attempt}: {}; semantic={:?}",
                            state.safe_summary, state.semantic_fingerprint
                        );
                    }
                    Err(error) => {
                        last_target_diagnostic = format!("attempt {attempt}: {error}");
                    }
                }
            }
            let pre_state = pre_state.ok_or_else(|| {
                format!(
                    "Excel did not expose C5B_Target!B3 (provider GridItem row 3 column 2) through UI Automation within 4 bounded attempts after {} ms: {last_target_diagnostic}",
                    smoke_started.elapsed().as_millis()
                )
            })?;
            if pre_state.application_fingerprint.is_empty()
                || pre_state.frame_fingerprint.is_empty()
                || pre_state.target_fingerprint.is_empty()
            {
                return Err("Excel target identity was incomplete".to_string());
            }
            phase("corroborate-pre-object-model")?;
            let pre_object_model = request_excel_object_model_result(&verify, &result_file)?;
            validate_excel_object_model_result(
                &pre_object_model,
                "before",
                &workbook,
                "C5B_Target",
                "B3",
            )?;

            phase("capture-pre-observation")?;
            let store = EventStore::open(directory.join("excel-smoke.db"))
                .map_err(|error| error.to_string())?;
            let screenshot = WindowsBoundComputerScreenshotClient::new(
                directory.join("screenshots"),
                window_handle,
                process_id,
            )?;
            let observation = capture_computer_use_observation(
                ComputerUseObservationPhase::PreAction,
                &screenshot,
                &accessibility,
            )?;
            let (_, observed) = persist_observed_computer_use_session(
                &store,
                None,
                "Set one exact cell in an isolated generated Excel workbook.".to_string(),
                ComputerUseUndoCapability::None,
                observation,
            )?;
            let expected_value = "DS Agent C5B exact cell";
            let bound = bind_computer_use_action(
                &store,
                observed.id,
                ComputerControlAction::SetAccessibilityValue {
                    value: expected_value.to_string(),
                },
                "Set the exact focused generated-workbook cell through UI Automation.".to_string(),
                ComputerUsePostcondition::TargetSemanticFingerprintEquals {
                    expected: accessibility_value_semantic_fingerprint(expected_value)?,
                },
            )?;
            let approval_id = Uuid::new_v4();
            approve_computer_use_step(
                &store,
                bound.id,
                approval_id,
                &bound
                    .action
                    .as_ref()
                    .ok_or_else(|| "Excel smoke action is missing".to_string())?
                    .action_fingerprint,
                ComputerUseApprovalActor::User,
            )?;
            let control = ExcelObjectModelCorroboratingControlClient {
                inner: WindowsBoundComputerControlClient::new_excel(
                    window_handle,
                    process_id,
                    "C5B_Target".to_string(),
                    "B3".to_string(),
                    3,
                    2,
                )?,
                verify: verify.clone(),
                result_file: result_file.clone(),
                workbook: workbook.clone(),
                expected_value: expected_value.to_string(),
                expected_sheet: "C5B_Target".to_string(),
                expected_cell: "B3".to_string(),
            };
            phase("execute-exact-uia-edit-and-object-model-corroboration")?;
            let run = execute_ready_computer_use_step(
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
            if run.step.status != ComputerUseStepStatus::Verified
                || run.step.action_start_count != 1
            {
                return Err(format!(
                    "Excel smoke ended in {:?} with {} action starts: {}",
                    run.step.status,
                    run.step.action_start_count,
                    run.safe_error
                        .as_deref()
                        .unwrap_or("no corroborated effect receipt was returned")
                ));
            }
            phase("verified")?;
            let actual =
                std::fs::read_to_string(&result_file).map_err(|error| error.to_string())?;
            validate_excel_object_model_result(
                &actual,
                expected_value,
                &workbook,
                "C5B_Target",
                "B3",
            )?;
            Ok(())
        })();
        let _ = std::fs::write(&stop, b"stop");
        if excel_host
            .wait_timeout(Duration::from_secs(5))
            .ok()
            .flatten()
            .is_none()
        {
            let _ = excel_host.kill();
            let _ = excel_host.wait();
        }
        run_result.expect("isolated Excel cell action verifies");
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
            let observation = capture_computer_use_observation(
                ComputerUseObservationPhase::PreAction,
                &screenshot,
                &accessibility,
            )?;
            let (_, observed) = persist_observed_computer_use_session(
                &store,
                None,
                "Verify one isolated Notepad-like editor action.".to_string(),
                ComputerUseUndoCapability::None,
                observation,
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
                ComputerUseApprovalActor::User,
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
