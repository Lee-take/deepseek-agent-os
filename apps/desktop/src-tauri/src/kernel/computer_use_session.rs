use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::kernel::capability::ComputerControlAction;

pub const COMPUTER_USE_STEP_CONTRACT_VERSION: &str = "computer-use-step/v2";
pub const COMPUTER_USE_OBSERVATION_FRESHNESS_SECS: i64 = 120;
const MAX_SAFE_SUMMARY_CHARS: usize = 1_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerUseStepStatus {
    Observed,
    AwaitingApproval,
    Ready,
    ActionStarted,
    AwaitingVerification,
    Verified,
    NeedsReplan,
    UserTakenOver,
    EffectUnknown,
    VerificationFailed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerUseObservationPhase {
    PreAction,
    PostAction,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerUseUndoCapability {
    None,
    CompensationRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ComputerUsePostcondition {
    TargetSemanticFingerprintEquals { expected: String },
    TargetSemanticFingerprintChanged,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerUseVerificationOutcome {
    Verified,
    EvidenceOnly,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerUseApprovalActor {
    User,
    KernelLifecycle,
    DeepSeekModel,
    FrontendPayload,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerUseRecoverySweep {
    pub needs_replan: usize,
    pub effect_unknown: usize,
    pub awaiting_verification: usize,
    pub quarantined: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerUseSession {
    pub contract_version: String,
    pub id: Uuid,
    pub run_id: Option<Uuid>,
    pub safe_goal_summary: String,
    pub active_step_id: Option<Uuid>,
    pub revision: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerUseObservation {
    pub id: Uuid,
    pub phase: ComputerUseObservationPhase,
    pub fingerprint: String,
    pub application_fingerprint: String,
    pub process_fingerprint: String,
    pub window_fingerprint: String,
    pub window_title_fingerprint: String,
    pub frame_fingerprint: String,
    pub target_fingerprint: Option<String>,
    pub semantic_fingerprint: Option<String>,
    pub screenshot_evidence_ref: String,
    pub safe_summary: String,
    pub captured_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerUseActionBinding {
    pub action: ComputerControlAction,
    pub safe_summary: String,
    pub pre_observation_fingerprint: String,
    pub application_fingerprint: String,
    pub process_fingerprint: String,
    pub window_fingerprint: String,
    pub pre_window_title_fingerprint: String,
    pub frame_fingerprint: String,
    pub target_fingerprint: String,
    pub pre_semantic_fingerprint: Option<String>,
    pub postcondition: ComputerUsePostcondition,
    pub action_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerUseCheckpoint {
    pub id: Uuid,
    pub undo_capability: ComputerUseUndoCapability,
    pub action_fingerprint: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerUseVerificationReceipt {
    pub id: Uuid,
    pub action_fingerprint: String,
    pub post_observation_fingerprint: String,
    pub outcome: ComputerUseVerificationOutcome,
    pub safe_summary: String,
    pub verified_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputerUseStep {
    pub contract_version: String,
    pub id: Uuid,
    pub session_id: Uuid,
    pub sequence: u32,
    pub status: ComputerUseStepStatus,
    pub revision: u64,
    pub pre_observation: ComputerUseObservation,
    pub action: Option<ComputerUseActionBinding>,
    pub approval_request_id: Option<Uuid>,
    pub approval_actor: Option<ComputerUseApprovalActor>,
    pub action_started_at: Option<DateTime<Utc>>,
    pub action_start_count: u32,
    pub post_observation: Option<ComputerUseObservation>,
    pub verification: Option<ComputerUseVerificationReceipt>,
    pub checkpoint: ComputerUseCheckpoint,
    pub status_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ComputerUseSession {
    pub fn new(
        run_id: Option<Uuid>,
        safe_goal_summary: String,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        Ok(Self {
            contract_version: COMPUTER_USE_STEP_CONTRACT_VERSION.to_string(),
            id: Uuid::new_v4(),
            run_id,
            safe_goal_summary: safe_text(safe_goal_summary, "safe goal summary")?,
            active_step_id: None,
            revision: 0,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn activate_step(&mut self, step_id: Uuid, now: DateTime<Utc>) -> Result<(), String> {
        if step_id.is_nil() {
            return Err("computer use active step id is invalid".to_string());
        }
        self.active_step_id = Some(step_id);
        self.revision = self.revision.saturating_add(1);
        self.updated_at = now;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.contract_version != COMPUTER_USE_STEP_CONTRACT_VERSION || self.id.is_nil() {
            return Err("computer use session identity is invalid".to_string());
        }
        safe_text(self.safe_goal_summary.clone(), "safe goal summary")?;
        if self.active_step_id.is_some_and(|step_id| step_id.is_nil()) {
            return Err("computer use active step id is invalid".to_string());
        }
        if self.updated_at < self.created_at {
            return Err("computer use session timestamps are invalid".to_string());
        }
        Ok(())
    }
}

impl ComputerUseObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        phase: ComputerUseObservationPhase,
        application_fingerprint: String,
        process_fingerprint: String,
        window_fingerprint: String,
        window_title_fingerprint: String,
        frame_fingerprint: String,
        target_fingerprint: Option<String>,
        semantic_fingerprint: Option<String>,
        screenshot_evidence_ref: String,
        safe_summary: String,
        captured_at: DateTime<Utc>,
    ) -> Result<Self, String> {
        require_fingerprint(&application_fingerprint, "application fingerprint")?;
        require_fingerprint(&process_fingerprint, "process fingerprint")?;
        require_fingerprint(&window_fingerprint, "window fingerprint")?;
        require_fingerprint(&window_title_fingerprint, "window title fingerprint")?;
        require_fingerprint(&frame_fingerprint, "frame fingerprint")?;
        if let Some(value) = target_fingerprint.as_deref() {
            require_fingerprint(value, "target fingerprint")?;
        }
        if let Some(value) = semantic_fingerprint.as_deref() {
            require_fingerprint(value, "semantic fingerprint")?;
        }
        let screenshot_evidence_ref = safe_evidence_ref(screenshot_evidence_ref)?;
        let safe_summary = safe_text(safe_summary, "observation summary")?;
        let id = Uuid::new_v4();
        let valid_until = captured_at
            .checked_add_signed(Duration::seconds(COMPUTER_USE_OBSERVATION_FRESHNESS_SECS))
            .ok_or_else(|| "computer use observation freshness window overflowed".to_string())?;
        let id_text = id.to_string();
        let captured_at_text = timestamp_text(captured_at);
        let valid_until_text = timestamp_text(valid_until);
        let fingerprint = hash_parts(&[
            COMPUTER_USE_STEP_CONTRACT_VERSION,
            &id_text,
            observation_phase_name(phase),
            &application_fingerprint,
            &process_fingerprint,
            &window_fingerprint,
            &window_title_fingerprint,
            &frame_fingerprint,
            target_fingerprint.as_deref().unwrap_or("none"),
            semantic_fingerprint.as_deref().unwrap_or("none"),
            &screenshot_evidence_ref,
            &captured_at_text,
            &valid_until_text,
        ]);
        Ok(Self {
            id,
            phase,
            fingerprint,
            application_fingerprint,
            process_fingerprint,
            window_fingerprint,
            window_title_fingerprint,
            frame_fingerprint,
            target_fingerprint,
            semantic_fingerprint,
            screenshot_evidence_ref,
            safe_summary,
            captured_at,
            valid_until,
        })
    }

    pub fn require_fresh_at(&self, now: DateTime<Utc>) -> Result<(), String> {
        if now < self.captured_at {
            return Err("computer use observation timestamp is in the future".to_string());
        }
        if now > self.valid_until {
            return Err(
                "computer use observation is stale; re-observation and a new approval are required"
                    .to_string(),
            );
        }
        Ok(())
    }
}

impl ComputerUseActionBinding {
    pub fn new(
        pre_observation: &ComputerUseObservation,
        action: ComputerControlAction,
        safe_summary: String,
        postcondition: ComputerUsePostcondition,
    ) -> Result<Self, String> {
        if pre_observation.phase != ComputerUseObservationPhase::PreAction {
            return Err("computer use action requires a pre-action observation".to_string());
        }
        let target_fingerprint = pre_observation.target_fingerprint.clone().ok_or_else(|| {
            "computer use action requires an exact target fingerprint".to_string()
        })?;
        validate_postcondition(&postcondition)?;
        let safe_summary = safe_text(safe_summary, "action summary")?;
        let action_json = serde_json::to_string(&action)
            .map_err(|error| format!("computer use action could not be serialized: {error}"))?;
        let postcondition_json = serde_json::to_string(&postcondition).map_err(|error| {
            format!("computer use postcondition could not be serialized: {error}")
        })?;
        let action_fingerprint = hash_parts(&[
            COMPUTER_USE_STEP_CONTRACT_VERSION,
            &pre_observation.fingerprint,
            &pre_observation.application_fingerprint,
            &pre_observation.process_fingerprint,
            &pre_observation.window_fingerprint,
            &pre_observation.window_title_fingerprint,
            &pre_observation.frame_fingerprint,
            &target_fingerprint,
            &action_json,
            &postcondition_json,
        ]);
        Ok(Self {
            action,
            safe_summary,
            pre_observation_fingerprint: pre_observation.fingerprint.clone(),
            application_fingerprint: pre_observation.application_fingerprint.clone(),
            process_fingerprint: pre_observation.process_fingerprint.clone(),
            window_fingerprint: pre_observation.window_fingerprint.clone(),
            pre_window_title_fingerprint: pre_observation.window_title_fingerprint.clone(),
            frame_fingerprint: pre_observation.frame_fingerprint.clone(),
            target_fingerprint,
            pre_semantic_fingerprint: pre_observation.semantic_fingerprint.clone(),
            postcondition,
            action_fingerprint,
        })
    }
}

impl ComputerUseStep {
    pub fn new_observed(
        session_id: Uuid,
        sequence: u32,
        pre_observation: ComputerUseObservation,
        undo_capability: ComputerUseUndoCapability,
        now: DateTime<Utc>,
    ) -> Result<Self, String> {
        if sequence == 0 {
            return Err("computer use step sequence must be positive".to_string());
        }
        if pre_observation.phase != ComputerUseObservationPhase::PreAction {
            return Err("computer use step requires a pre-action observation".to_string());
        }
        Ok(Self {
            contract_version: COMPUTER_USE_STEP_CONTRACT_VERSION.to_string(),
            id: Uuid::new_v4(),
            session_id,
            sequence,
            status: ComputerUseStepStatus::Observed,
            revision: 0,
            pre_observation,
            action: None,
            approval_request_id: None,
            approval_actor: None,
            action_started_at: None,
            action_start_count: 0,
            post_observation: None,
            verification: None,
            checkpoint: ComputerUseCheckpoint {
                id: Uuid::new_v4(),
                undo_capability,
                action_fingerprint: None,
                created_at: now,
            },
            status_reason: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn bind_action(
        &mut self,
        action: ComputerUseActionBinding,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.require_status(ComputerUseStepStatus::Observed, "bind an action")?;
        self.pre_observation.require_fresh_at(now)?;
        if action.pre_observation_fingerprint != self.pre_observation.fingerprint
            || action.application_fingerprint != self.pre_observation.application_fingerprint
            || action.process_fingerprint != self.pre_observation.process_fingerprint
            || action.window_fingerprint != self.pre_observation.window_fingerprint
            || action.pre_window_title_fingerprint != self.pre_observation.window_title_fingerprint
            || action.frame_fingerprint != self.pre_observation.frame_fingerprint
            || self.pre_observation.target_fingerprint.as_deref()
                != Some(action.target_fingerprint.as_str())
            || action.pre_semantic_fingerprint != self.pre_observation.semantic_fingerprint
        {
            return Err(
                "computer use action is not bound to the current pre-action observation"
                    .to_string(),
            );
        }
        if matches!(
            action.postcondition,
            ComputerUsePostcondition::TargetSemanticFingerprintChanged
        ) && action.pre_semantic_fingerprint.is_none()
        {
            return Err(
                "changed-state verification requires a semantic pre-action fingerprint".to_string(),
            );
        }
        action.validate()?;
        self.checkpoint.action_fingerprint = Some(action.action_fingerprint.clone());
        self.action = Some(action);
        self.transition(
            ComputerUseStepStatus::AwaitingApproval,
            "Exact desktop action is waiting for approval.".to_string(),
            now,
        );
        Ok(())
    }

    pub fn approve(
        &mut self,
        approval_request_id: Uuid,
        approved_action_fingerprint: &str,
        actor: ComputerUseApprovalActor,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.require_status(ComputerUseStepStatus::AwaitingApproval, "bind an approval")?;
        if actor != ComputerUseApprovalActor::User {
            return Err(
                "computer use approval authority can only come from a local user decision"
                    .to_string(),
            );
        }
        self.pre_observation.require_fresh_at(now)?;
        if approval_request_id.is_nil() {
            return Err("computer use approval request id is invalid".to_string());
        }
        require_fingerprint(approved_action_fingerprint, "approved action fingerprint")?;
        let expected = self
            .action
            .as_ref()
            .ok_or_else(|| "computer use step has no exact action to approve".to_string())?
            .action_fingerprint
            .as_str();
        if expected != approved_action_fingerprint {
            return Err("computer use approval is stale or bound to another action".to_string());
        }
        self.approval_request_id = Some(approval_request_id);
        self.approval_actor = Some(actor);
        self.transition(
            ComputerUseStepStatus::Ready,
            "Exact desktop action is approved and ready for revalidation.".to_string(),
            now,
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn mark_action_started(
        &mut self,
        approval_request_id: Uuid,
        current_application_fingerprint: &str,
        current_process_fingerprint: &str,
        current_window_fingerprint: &str,
        current_window_title_fingerprint: &str,
        current_frame_fingerprint: &str,
        current_target_fingerprint: &str,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.require_status(
            ComputerUseStepStatus::Ready,
            "start the external desktop action",
        )?;
        if self.action_start_count != 0 || self.action_started_at.is_some() {
            return Err(
                "computer use action has already started and cannot be replayed".to_string(),
            );
        }
        if self.approval_request_id != Some(approval_request_id) {
            return Err("computer use approval does not match the ready step".to_string());
        }
        if self.approval_actor != Some(ComputerUseApprovalActor::User) {
            return Err("computer use step lacks local-user approval authority".to_string());
        }
        require_fingerprint(
            current_application_fingerprint,
            "current application fingerprint",
        )?;
        require_fingerprint(current_process_fingerprint, "current process fingerprint")?;
        require_fingerprint(current_window_fingerprint, "current window fingerprint")?;
        require_fingerprint(
            current_window_title_fingerprint,
            "current window title fingerprint",
        )?;
        require_fingerprint(current_frame_fingerprint, "current frame fingerprint")?;
        require_fingerprint(current_target_fingerprint, "current target fingerprint")?;
        let action = self
            .action
            .as_ref()
            .ok_or_else(|| "computer use step has no exact action".to_string())?;
        if action.application_fingerprint != current_application_fingerprint {
            return Err(
                "foreground application changed after approval; re-observation is required"
                    .to_string(),
            );
        }
        if action.process_fingerprint != current_process_fingerprint {
            return Err(
                "foreground process changed after approval; re-observation is required".to_string(),
            );
        }
        if action.window_fingerprint != current_window_fingerprint {
            return Err(
                "foreground window changed after approval; re-observation is required".to_string(),
            );
        }
        if action.pre_window_title_fingerprint != current_window_title_fingerprint {
            return Err(
                "foreground window title changed after approval; re-observation is required"
                    .to_string(),
            );
        }
        if action.frame_fingerprint != current_frame_fingerprint {
            return Err(
                "foreground frame changed after approval; re-observation is required".to_string(),
            );
        }
        if action.target_fingerprint != current_target_fingerprint {
            return Err(
                "computer use target changed after approval; re-observation is required"
                    .to_string(),
            );
        }
        self.action_started_at = Some(now);
        self.action_start_count = 1;
        self.transition(
            ComputerUseStepStatus::ActionStarted,
            "ActionStarted was durably recorded before the external desktop effect.".to_string(),
            now,
        );
        Ok(())
    }

    pub fn record_post_observation(
        &mut self,
        observation: ComputerUseObservation,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.require_status(
            ComputerUseStepStatus::ActionStarted,
            "record a post-action observation",
        )?;
        if observation.phase != ComputerUseObservationPhase::PostAction {
            return Err("computer use post-action evidence has the wrong phase".to_string());
        }
        observation.validate()?;
        let action = self
            .action
            .as_ref()
            .ok_or_else(|| "computer use step has no exact action".to_string())?;
        if observation.application_fingerprint != action.application_fingerprint
            || observation.process_fingerprint != action.process_fingerprint
            || observation.window_fingerprint != action.window_fingerprint
            || observation.frame_fingerprint != action.frame_fingerprint
        {
            return Err(
                "post-action observation is bound to a different application, process, window, or frame"
                    .to_string(),
            );
        }
        if observation.target_fingerprint.as_deref() != Some(action.target_fingerprint.as_str()) {
            return Err("post-action observation is bound to a different target".to_string());
        }
        if self
            .action_started_at
            .is_some_and(|started_at| observation.captured_at < started_at)
        {
            return Err("post-action observation predates ActionStarted".to_string());
        }
        self.post_observation = Some(observation);
        self.transition(
            ComputerUseStepStatus::AwaitingVerification,
            "Post-action evidence was captured and awaits deterministic verification.".to_string(),
            now,
        );
        Ok(())
    }

    pub fn record_verification(
        &mut self,
        receipt: ComputerUseVerificationReceipt,
    ) -> Result<(), String> {
        self.require_status(
            ComputerUseStepStatus::AwaitingVerification,
            "record verification",
        )?;
        if receipt.id.is_nil() {
            return Err("computer use verification receipt id is invalid".to_string());
        }
        let safe_summary = safe_text(receipt.safe_summary.clone(), "verification summary")?;
        let action = self
            .action
            .as_ref()
            .ok_or_else(|| "computer use step has no exact action".to_string())?;
        let post = self
            .post_observation
            .as_ref()
            .ok_or_else(|| "computer use step has no post-action observation".to_string())?;
        if receipt.action_fingerprint != action.action_fingerprint
            || receipt.post_observation_fingerprint != post.fingerprint
        {
            return Err(
                "computer use verification is stale or bound to another action".to_string(),
            );
        }
        if receipt.verified_at < post.captured_at {
            return Err("computer use verification predates its evidence".to_string());
        }
        let next_status = match receipt.outcome {
            ComputerUseVerificationOutcome::Verified => {
                let post_semantic = post.semantic_fingerprint.as_deref().ok_or_else(|| {
                    "screenshot-only evidence cannot prove a semantic postcondition".to_string()
                })?;
                let satisfied = match &action.postcondition {
                    ComputerUsePostcondition::TargetSemanticFingerprintEquals { expected } => {
                        post_semantic == expected
                    }
                    ComputerUsePostcondition::TargetSemanticFingerprintChanged => action
                        .pre_semantic_fingerprint
                        .as_deref()
                        .is_some_and(|before| before != post_semantic),
                };
                if !satisfied {
                    return Err(
                        "computer use evidence does not satisfy the deterministic postcondition"
                            .to_string(),
                    );
                }
                ComputerUseStepStatus::Verified
            }
            ComputerUseVerificationOutcome::EvidenceOnly => {
                ComputerUseStepStatus::AwaitingVerification
            }
            ComputerUseVerificationOutcome::Failed => ComputerUseStepStatus::VerificationFailed,
        };
        let verified_at = receipt.verified_at;
        self.verification = Some(receipt);
        self.transition(next_status, safe_summary, verified_at);
        Ok(())
    }

    pub fn take_over(&mut self, reason: String, now: DateTime<Utc>) -> Result<(), String> {
        if matches!(
            self.status,
            ComputerUseStepStatus::Verified
                | ComputerUseStepStatus::NeedsReplan
                | ComputerUseStepStatus::UserTakenOver
                | ComputerUseStepStatus::EffectUnknown
                | ComputerUseStepStatus::VerificationFailed
                | ComputerUseStepStatus::Cancelled
        ) {
            return Err(format!(
                "computer use step in {:?} cannot be taken over",
                self.status
            ));
        }
        self.transition(
            ComputerUseStepStatus::UserTakenOver,
            safe_text(reason, "takeover reason")?,
            now,
        );
        Ok(())
    }

    pub fn require_replan(&mut self, reason: String, now: DateTime<Utc>) -> Result<(), String> {
        if !matches!(
            self.status,
            ComputerUseStepStatus::Observed
                | ComputerUseStepStatus::AwaitingApproval
                | ComputerUseStepStatus::Ready
        ) {
            return Err(format!(
                "computer use step in {:?} cannot require replanning",
                self.status
            ));
        }
        self.approval_request_id = None;
        self.approval_actor = None;
        self.transition(
            ComputerUseStepStatus::NeedsReplan,
            safe_text(reason, "replan reason")?,
            now,
        );
        Ok(())
    }

    pub fn mark_effect_unknown(
        &mut self,
        reason: String,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.require_status(
            ComputerUseStepStatus::ActionStarted,
            "mark its external effect unknown",
        )?;
        self.transition(
            ComputerUseStepStatus::EffectUnknown,
            safe_text(reason, "effect unknown reason")?,
            now,
        );
        Ok(())
    }

    pub fn recover_after_restart(&mut self, now: DateTime<Utc>) -> Result<(), String> {
        self.validate()?;
        match self.status {
            ComputerUseStepStatus::Observed
            | ComputerUseStepStatus::AwaitingApproval
            | ComputerUseStepStatus::Ready => {
                self.approval_request_id = None;
                self.approval_actor = None;
                self.transition(
                    ComputerUseStepStatus::NeedsReplan,
                    "Desktop state may have changed after restart; re-observation is required."
                        .to_string(),
                    now,
                );
            }
            ComputerUseStepStatus::ActionStarted => {
                self.transition(
                    ComputerUseStepStatus::EffectUnknown,
                    "The desktop action may have produced an external effect before restart; automatic replay is blocked."
                        .to_string(),
                    now,
                );
            }
            ComputerUseStepStatus::AwaitingVerification
            | ComputerUseStepStatus::Verified
            | ComputerUseStepStatus::NeedsReplan
            | ComputerUseStepStatus::UserTakenOver
            | ComputerUseStepStatus::EffectUnknown
            | ComputerUseStepStatus::VerificationFailed
            | ComputerUseStepStatus::Cancelled => {}
        }
        Ok(())
    }

    pub fn cancel(&mut self, reason: String, now: DateTime<Utc>) -> Result<(), String> {
        if !matches!(
            self.status,
            ComputerUseStepStatus::Observed
                | ComputerUseStepStatus::AwaitingApproval
                | ComputerUseStepStatus::Ready
                | ComputerUseStepStatus::NeedsReplan
                | ComputerUseStepStatus::UserTakenOver
                | ComputerUseStepStatus::VerificationFailed
        ) {
            return Err(format!(
                "computer use step in {:?} cannot be cancelled",
                self.status
            ));
        }
        self.transition(
            ComputerUseStepStatus::Cancelled,
            safe_text(reason, "cancellation reason")?,
            now,
        );
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.contract_version != COMPUTER_USE_STEP_CONTRACT_VERSION
            || self.id.is_nil()
            || self.session_id.is_nil()
            || self.sequence == 0
        {
            return Err("computer use step identity is invalid".to_string());
        }
        self.pre_observation.validate()?;
        if self.pre_observation.phase != ComputerUseObservationPhase::PreAction {
            return Err("computer use step pre-observation phase is invalid".to_string());
        }
        if self.checkpoint.id.is_nil() {
            return Err("computer use checkpoint identity is invalid".to_string());
        }
        if self.action_start_count > 1
            || (self.action_start_count == 1) != self.action_started_at.is_some()
        {
            return Err("computer use ActionStarted record is inconsistent".to_string());
        }
        match self.action.as_ref() {
            Some(action) => {
                action.validate()?;
                if action.pre_observation_fingerprint != self.pre_observation.fingerprint
                    || action.application_fingerprint
                        != self.pre_observation.application_fingerprint
                    || action.process_fingerprint != self.pre_observation.process_fingerprint
                    || action.window_fingerprint != self.pre_observation.window_fingerprint
                    || action.pre_window_title_fingerprint
                        != self.pre_observation.window_title_fingerprint
                    || action.frame_fingerprint != self.pre_observation.frame_fingerprint
                    || self.pre_observation.target_fingerprint.as_deref()
                        != Some(action.target_fingerprint.as_str())
                    || action.pre_semantic_fingerprint != self.pre_observation.semantic_fingerprint
                    || self.checkpoint.action_fingerprint.as_deref()
                        != Some(action.action_fingerprint.as_str())
                {
                    return Err("computer use action binding is inconsistent".to_string());
                }
            }
            None if self.checkpoint.action_fingerprint.is_some() => {
                return Err("computer use checkpoint action binding is inconsistent".to_string())
            }
            None => {}
        }
        if let Some(observation) = self.post_observation.as_ref() {
            observation.validate()?;
            if observation.phase != ComputerUseObservationPhase::PostAction {
                return Err("computer use post-observation phase is invalid".to_string());
            }
            let action = self.action.as_ref().ok_or_else(|| {
                "computer use post-observation has no exact action binding".to_string()
            })?;
            if observation.application_fingerprint != action.application_fingerprint
                || observation.process_fingerprint != action.process_fingerprint
                || observation.window_fingerprint != action.window_fingerprint
                || observation.frame_fingerprint != action.frame_fingerprint
                || observation.target_fingerprint.as_deref()
                    != Some(action.target_fingerprint.as_str())
            {
                return Err(
                    "computer use post-observation is bound to another application, process, window, frame, or target".to_string(),
                );
            }
            if self
                .action_started_at
                .is_none_or(|started_at| observation.captured_at < started_at)
            {
                return Err(
                    "computer use post-observation is not ordered after ActionStarted".to_string(),
                );
            }
        }
        if let Some(receipt) = self.verification.as_ref() {
            if receipt.id.is_nil() {
                return Err("computer use verification receipt identity is invalid".to_string());
            }
            require_fingerprint(
                &receipt.action_fingerprint,
                "verification action fingerprint",
            )?;
            require_fingerprint(
                &receipt.post_observation_fingerprint,
                "verification post-observation fingerprint",
            )?;
            safe_text(receipt.safe_summary.clone(), "verification summary")?;
            let action = self.action.as_ref().ok_or_else(|| {
                "computer use verification has no exact action binding".to_string()
            })?;
            let post = self.post_observation.as_ref().ok_or_else(|| {
                "computer use verification has no post-action evidence binding".to_string()
            })?;
            if receipt.action_fingerprint != action.action_fingerprint
                || receipt.post_observation_fingerprint != post.fingerprint
                || receipt.verified_at < post.captured_at
            {
                return Err("computer use verification binding is inconsistent".to_string());
            }
            if self.status == ComputerUseStepStatus::AwaitingVerification
                && receipt.outcome != ComputerUseVerificationOutcome::EvidenceOnly
            {
                return Err(
                    "computer use pending verification has an inconsistent outcome".to_string(),
                );
            }
            if self.status == ComputerUseStepStatus::VerificationFailed
                && receipt.outcome != ComputerUseVerificationOutcome::Failed
            {
                return Err("computer use failed verification lacks a failed receipt".to_string());
            }
            if receipt.outcome == ComputerUseVerificationOutcome::Verified {
                let post_semantic = post.semantic_fingerprint.as_deref().ok_or_else(|| {
                    "computer use verified receipt lacks bounded semantic evidence".to_string()
                })?;
                let satisfied = match &action.postcondition {
                    ComputerUsePostcondition::TargetSemanticFingerprintEquals { expected } => {
                        post_semantic == expected
                    }
                    ComputerUsePostcondition::TargetSemanticFingerprintChanged => action
                        .pre_semantic_fingerprint
                        .as_deref()
                        .is_some_and(|before| before != post_semantic),
                };
                if !satisfied {
                    return Err(
                        "computer use verified receipt does not satisfy its deterministic postcondition"
                            .to_string(),
                    );
                }
            }
        }
        let action_required = matches!(
            self.status,
            ComputerUseStepStatus::AwaitingApproval
                | ComputerUseStepStatus::Ready
                | ComputerUseStepStatus::ActionStarted
                | ComputerUseStepStatus::AwaitingVerification
                | ComputerUseStepStatus::Verified
                | ComputerUseStepStatus::EffectUnknown
                | ComputerUseStepStatus::VerificationFailed
        );
        if action_required && self.action.is_none() {
            return Err("computer use step status requires an exact action".to_string());
        }
        let approval_required = matches!(
            self.status,
            ComputerUseStepStatus::Ready
                | ComputerUseStepStatus::ActionStarted
                | ComputerUseStepStatus::AwaitingVerification
                | ComputerUseStepStatus::Verified
                | ComputerUseStepStatus::EffectUnknown
                | ComputerUseStepStatus::VerificationFailed
        );
        if self.approval_request_id.is_some() != self.approval_actor.is_some() {
            return Err("computer use approval identity and actor are inconsistent".to_string());
        }
        if self
            .approval_request_id
            .is_some_and(|approval_request_id| approval_request_id.is_nil())
        {
            return Err("computer use approval request id is invalid".to_string());
        }
        if self
            .approval_actor
            .is_some_and(|actor| actor != ComputerUseApprovalActor::User)
        {
            return Err(
                "computer use approval evidence can only name a local user actor".to_string(),
            );
        }
        if approval_required
            && (self.approval_request_id.is_none()
                || self.approval_actor != Some(ComputerUseApprovalActor::User))
        {
            return Err("computer use step status requires exact local-user approval".to_string());
        }
        let started_required = matches!(
            self.status,
            ComputerUseStepStatus::ActionStarted
                | ComputerUseStepStatus::AwaitingVerification
                | ComputerUseStepStatus::Verified
                | ComputerUseStepStatus::EffectUnknown
                | ComputerUseStepStatus::VerificationFailed
        );
        if started_required && (self.action_started_at.is_none() || self.action_start_count != 1) {
            return Err("computer use step status requires one ActionStarted record".to_string());
        }
        let post_required = matches!(
            self.status,
            ComputerUseStepStatus::AwaitingVerification
                | ComputerUseStepStatus::Verified
                | ComputerUseStepStatus::VerificationFailed
        );
        if post_required && self.post_observation.is_none() {
            return Err("computer use step status requires post-action evidence".to_string());
        }
        if self.status == ComputerUseStepStatus::Verified
            && !self
                .verification
                .as_ref()
                .is_some_and(|receipt| receipt.outcome == ComputerUseVerificationOutcome::Verified)
        {
            return Err("verified computer use step lacks a verified receipt".to_string());
        }
        Ok(())
    }

    fn require_status(
        &self,
        expected: ComputerUseStepStatus,
        operation: &str,
    ) -> Result<(), String> {
        if self.status != expected {
            return Err(format!(
                "computer use step in {:?} cannot {operation}",
                self.status
            ));
        }
        Ok(())
    }

    fn transition(&mut self, status: ComputerUseStepStatus, reason: String, now: DateTime<Utc>) {
        self.status = status;
        self.status_reason = Some(reason);
        self.revision = self.revision.saturating_add(1);
        self.updated_at = now;
    }
}

impl ComputerUseObservation {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_nil() {
            return Err("computer use observation identity is invalid".to_string());
        }
        require_fingerprint(&self.fingerprint, "observation fingerprint")?;
        require_fingerprint(&self.application_fingerprint, "application fingerprint")?;
        require_fingerprint(&self.process_fingerprint, "process fingerprint")?;
        require_fingerprint(&self.window_fingerprint, "window fingerprint")?;
        require_fingerprint(&self.window_title_fingerprint, "window title fingerprint")?;
        require_fingerprint(&self.frame_fingerprint, "frame fingerprint")?;
        if let Some(value) = self.target_fingerprint.as_deref() {
            require_fingerprint(value, "target fingerprint")?;
        }
        if let Some(value) = self.semantic_fingerprint.as_deref() {
            require_fingerprint(value, "semantic fingerprint")?;
        }
        safe_evidence_ref(self.screenshot_evidence_ref.clone())?;
        safe_text(self.safe_summary.clone(), "observation summary")?;
        let expected_valid_until = self
            .captured_at
            .checked_add_signed(Duration::seconds(COMPUTER_USE_OBSERVATION_FRESHNESS_SECS))
            .ok_or_else(|| "computer use observation freshness window overflowed".to_string())?;
        if self.valid_until != expected_valid_until {
            return Err("computer use observation freshness window is inconsistent".to_string());
        }
        let id_text = self.id.to_string();
        let captured_at_text = timestamp_text(self.captured_at);
        let valid_until_text = timestamp_text(self.valid_until);
        let expected = hash_parts(&[
            COMPUTER_USE_STEP_CONTRACT_VERSION,
            &id_text,
            observation_phase_name(self.phase),
            &self.application_fingerprint,
            &self.process_fingerprint,
            &self.window_fingerprint,
            &self.window_title_fingerprint,
            &self.frame_fingerprint,
            self.target_fingerprint.as_deref().unwrap_or("none"),
            self.semantic_fingerprint.as_deref().unwrap_or("none"),
            &self.screenshot_evidence_ref,
            &captured_at_text,
            &valid_until_text,
        ]);
        if expected != self.fingerprint {
            return Err("computer use observation fingerprint is inconsistent".to_string());
        }
        Ok(())
    }
}

impl ComputerUseActionBinding {
    pub fn validate(&self) -> Result<(), String> {
        require_fingerprint(
            &self.pre_observation_fingerprint,
            "pre-observation fingerprint",
        )?;
        require_fingerprint(&self.application_fingerprint, "application fingerprint")?;
        require_fingerprint(&self.process_fingerprint, "process fingerprint")?;
        require_fingerprint(&self.window_fingerprint, "window fingerprint")?;
        require_fingerprint(
            &self.pre_window_title_fingerprint,
            "pre-action window title fingerprint",
        )?;
        require_fingerprint(&self.frame_fingerprint, "frame fingerprint")?;
        require_fingerprint(&self.target_fingerprint, "target fingerprint")?;
        if let Some(value) = self.pre_semantic_fingerprint.as_deref() {
            require_fingerprint(value, "pre-action semantic fingerprint")?;
        }
        require_fingerprint(&self.action_fingerprint, "action fingerprint")?;
        safe_text(self.safe_summary.clone(), "action summary")?;
        validate_postcondition(&self.postcondition)?;
        let action_json = serde_json::to_string(&self.action)
            .map_err(|error| format!("computer use action could not be serialized: {error}"))?;
        let postcondition_json = serde_json::to_string(&self.postcondition).map_err(|error| {
            format!("computer use postcondition could not be serialized: {error}")
        })?;
        let expected = hash_parts(&[
            COMPUTER_USE_STEP_CONTRACT_VERSION,
            &self.pre_observation_fingerprint,
            &self.application_fingerprint,
            &self.process_fingerprint,
            &self.window_fingerprint,
            &self.pre_window_title_fingerprint,
            &self.frame_fingerprint,
            &self.target_fingerprint,
            &action_json,
            &postcondition_json,
        ]);
        if expected != self.action_fingerprint {
            return Err("computer use action fingerprint is inconsistent".to_string());
        }
        Ok(())
    }
}

fn safe_text(value: String, field: &str) -> Result<String, String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{field} is required"));
    }
    if value.chars().count() > MAX_SAFE_SUMMARY_CHARS {
        return Err(format!(
            "{field} exceeds {MAX_SAFE_SUMMARY_CHARS} characters"
        ));
    }
    Ok(value)
}

fn safe_evidence_ref(value: String) -> Result<String, String> {
    let value = value.trim().replace('\\', "/");
    if !value.starts_with("computer-screenshots/")
        || value.contains(':')
        || value
            .split('/')
            .any(|segment| segment == ".." || segment.is_empty())
    {
        return Err(
            "computer use screenshot evidence reference is not a safe local handle".to_string(),
        );
    }
    Ok(value)
}

fn require_fingerprint(value: &str, field: &str) -> Result<(), String> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field} must be a SHA-256 fingerprint"));
    }
    Ok(())
}

fn validate_postcondition(value: &ComputerUsePostcondition) -> Result<(), String> {
    if let ComputerUsePostcondition::TargetSemanticFingerprintEquals { expected } = value {
        require_fingerprint(expected, "expected semantic fingerprint")?;
    }
    Ok(())
}

fn hash_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn timestamp_text(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn observation_phase_name(phase: ComputerUseObservationPhase) -> &'static str {
    match phase {
        ComputerUseObservationPhase::PreAction => "pre_action",
        ComputerUseObservationPhase::PostAction => "post_action",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::capability::ComputerControlAction;

    fn fingerprint(value: &str) -> String {
        hash_parts(&[value])
    }

    fn observation(
        phase: ComputerUseObservationPhase,
        semantic: Option<&str>,
        captured_at: DateTime<Utc>,
    ) -> ComputerUseObservation {
        ComputerUseObservation::new(
            phase,
            fingerprint("notepad-application"),
            fingerprint("notepad-process"),
            fingerprint("notepad-window"),
            fingerprint("notepad-title"),
            fingerprint("notepad-frame"),
            Some(fingerprint("notepad-editor")),
            semantic.map(fingerprint),
            format!(
                "computer-screenshots/{}-notepad.png",
                match phase {
                    ComputerUseObservationPhase::PreAction => "before",
                    ComputerUseObservationPhase::PostAction => "after",
                }
            ),
            "Notepad editor is visible.".to_string(),
            captured_at,
        )
        .expect("observation is valid")
    }

    fn action(pre: &ComputerUseObservation, expected: &str) -> ComputerUseActionBinding {
        ComputerUseActionBinding::new(
            pre,
            ComputerControlAction::TypeText {
                text: "verified text".to_string(),
            },
            "Type the approved text into the isolated Notepad editor.".to_string(),
            ComputerUsePostcondition::TargetSemanticFingerprintEquals {
                expected: fingerprint(expected),
            },
        )
        .expect("action is valid")
    }

    fn approve_as_user(
        step: &mut ComputerUseStep,
        approval_id: Uuid,
        action_fingerprint: &str,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        step.approve(
            approval_id,
            action_fingerprint,
            ComputerUseApprovalActor::User,
            now,
        )
    }

    fn mark_started(
        step: &mut ComputerUseStep,
        approval_id: Uuid,
        window: &str,
        window_title: &str,
        target: &str,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let application = step.pre_observation.application_fingerprint.clone();
        let process = step.pre_observation.process_fingerprint.clone();
        let frame = step.pre_observation.frame_fingerprint.clone();
        step.mark_action_started(
            approval_id,
            &application,
            &process,
            window,
            window_title,
            &frame,
            target,
            now,
        )
    }

    #[test]
    fn verified_step_binds_observation_action_approval_and_postcondition() {
        let now = Utc::now();
        let session = ComputerUseSession::new(None, "Update isolated Notepad.".to_string(), now)
            .expect("session is valid");
        let pre = observation(ComputerUseObservationPhase::PreAction, Some("empty"), now);
        let binding = action(&pre, "verified text");
        let action_fingerprint = binding.action_fingerprint.clone();
        let approval_id = Uuid::new_v4();
        let mut step =
            ComputerUseStep::new_observed(session.id, 1, pre, ComputerUseUndoCapability::None, now)
                .expect("step is valid");

        step.bind_action(binding, now).expect("action binds");
        approve_as_user(&mut step, approval_id, &action_fingerprint, now).expect("approval binds");
        let window = step.pre_observation.window_fingerprint.clone();
        let window_title = step.pre_observation.window_title_fingerprint.clone();
        let target = step
            .pre_observation
            .target_fingerprint
            .clone()
            .expect("target exists");
        mark_started(&mut step, approval_id, &window, &window_title, &target, now)
            .expect("action starts once");
        let post = observation(
            ComputerUseObservationPhase::PostAction,
            Some("verified text"),
            now,
        );
        let post_fingerprint = post.fingerprint.clone();
        step.record_post_observation(post, now)
            .expect("post observation binds");
        step.record_verification(ComputerUseVerificationReceipt {
            id: Uuid::new_v4(),
            action_fingerprint,
            post_observation_fingerprint: post_fingerprint,
            outcome: ComputerUseVerificationOutcome::Verified,
            safe_summary: "The target semantic state matches the deterministic postcondition."
                .to_string(),
            verified_at: now,
        })
        .expect("verification binds");

        assert_eq!(step.status, ComputerUseStepStatus::Verified);
        assert_eq!(step.action_start_count, 1);
        assert_eq!(
            step.checkpoint.undo_capability,
            ComputerUseUndoCapability::None
        );
        step.validate().expect("verified step remains valid");

        let mut stale_pre_binding = step.clone();
        stale_pre_binding
            .action
            .as_mut()
            .expect("action exists")
            .pre_semantic_fingerprint = Some(fingerprint("other-before"));
        assert!(stale_pre_binding.validate().is_err());

        let mut false_verified = step.clone();
        let altered_post = ComputerUseObservation::new(
            ComputerUseObservationPhase::PostAction,
            fingerprint("notepad-application"),
            fingerprint("notepad-process"),
            fingerprint("notepad-window"),
            fingerprint("notepad-title"),
            fingerprint("notepad-frame"),
            Some(fingerprint("notepad-editor")),
            Some(fingerprint("wrong-result")),
            "computer-screenshots/altered-notepad.png".to_string(),
            "Notepad editor is visible.".to_string(),
            now,
        )
        .expect("altered observation is internally valid");
        false_verified
            .verification
            .as_mut()
            .expect("verification exists")
            .post_observation_fingerprint = altered_post.fingerprint.clone();
        false_verified.post_observation = Some(altered_post);
        assert!(false_verified.validate().is_err());
    }

    #[test]
    fn action_cannot_start_twice_or_with_stale_approval_or_target() {
        let now = Utc::now();
        let pre = observation(ComputerUseObservationPhase::PreAction, Some("empty"), now);
        let binding = action(&pre, "verified text");
        let action_fingerprint = binding.action_fingerprint.clone();
        let approval_id = Uuid::new_v4();
        let mut step = ComputerUseStep::new_observed(
            Uuid::new_v4(),
            1,
            pre,
            ComputerUseUndoCapability::None,
            now,
        )
        .expect("step is valid");
        step.bind_action(binding, now).expect("action binds");
        assert!(
            approve_as_user(&mut step, Uuid::new_v4(), &fingerprint("stale-action"), now,).is_err()
        );
        approve_as_user(&mut step, approval_id, &action_fingerprint, now)
            .expect("exact approval binds");
        let window = step.pre_observation.window_fingerprint.clone();
        let window_title = step.pre_observation.window_title_fingerprint.clone();
        let target = step.pre_observation.target_fingerprint.clone().unwrap();
        assert!(mark_started(
            &mut step,
            approval_id,
            &window,
            &window_title,
            &fingerprint("changed-target"),
            now,
        )
        .is_err());
        assert!(mark_started(
            &mut step,
            approval_id,
            &window,
            &fingerprint("changed-title"),
            &target,
            now,
        )
        .is_err());
        mark_started(&mut step, approval_id, &window, &window_title, &target, now)
            .expect("exact action starts");
        assert!(
            mark_started(&mut step, approval_id, &window, &window_title, &target, now,).is_err()
        );
        assert_eq!(step.action_start_count, 1);
    }

    #[test]
    fn stale_observation_and_non_user_approval_fail_closed() {
        let now = Utc::now();
        let stale_captured_at =
            now - Duration::seconds(COMPUTER_USE_OBSERVATION_FRESHNESS_SECS + 1);
        let stale_pre = observation(
            ComputerUseObservationPhase::PreAction,
            Some("empty"),
            stale_captured_at,
        );
        let stale_binding = action(&stale_pre, "verified text");
        let mut stale_step = ComputerUseStep::new_observed(
            Uuid::new_v4(),
            1,
            stale_pre,
            ComputerUseUndoCapability::None,
            stale_captured_at,
        )
        .unwrap();
        assert!(stale_step.bind_action(stale_binding, now).is_err());
        assert_eq!(stale_step.status, ComputerUseStepStatus::Observed);

        let pre = observation(ComputerUseObservationPhase::PreAction, Some("empty"), now);
        let binding = action(&pre, "verified text");
        let action_fingerprint = binding.action_fingerprint.clone();
        let mut step = ComputerUseStep::new_observed(
            Uuid::new_v4(),
            1,
            pre,
            ComputerUseUndoCapability::None,
            now,
        )
        .unwrap();
        step.bind_action(binding, now).unwrap();
        for actor in [
            ComputerUseApprovalActor::DeepSeekModel,
            ComputerUseApprovalActor::FrontendPayload,
            ComputerUseApprovalActor::KernelLifecycle,
        ] {
            assert!(step
                .approve(Uuid::new_v4(), &action_fingerprint, actor, now)
                .is_err());
            assert_eq!(step.status, ComputerUseStepStatus::AwaitingApproval);
            assert!(step.approval_request_id.is_none());
            assert!(step.approval_actor.is_none());
        }
        let mut forged = step.clone();
        forged.approval_request_id = Some(Uuid::new_v4());
        forged.approval_actor = Some(ComputerUseApprovalActor::DeepSeekModel);
        assert!(forged.validate().is_err());
        let stale_approval_time = step.pre_observation.valid_until + Duration::milliseconds(1);
        assert!(step
            .approve(
                Uuid::new_v4(),
                &action_fingerprint,
                ComputerUseApprovalActor::User,
                stale_approval_time,
            )
            .is_err());
        assert_eq!(step.status, ComputerUseStepStatus::AwaitingApproval);
        assert!(step.approval_request_id.is_none());
        assert!(step.approval_actor.is_none());
    }

    #[test]
    fn stale_revalidation_and_execution_failure_have_distinct_fail_closed_states() {
        let now = Utc::now();
        let pre = observation(ComputerUseObservationPhase::PreAction, Some("empty"), now);
        let binding = action(&pre, "verified text");
        let action_fingerprint = binding.action_fingerprint.clone();
        let approval_id = Uuid::new_v4();

        let mut stale = ComputerUseStep::new_observed(
            Uuid::new_v4(),
            1,
            pre.clone(),
            ComputerUseUndoCapability::None,
            now,
        )
        .unwrap();
        stale.bind_action(binding.clone(), now).unwrap();
        approve_as_user(&mut stale, approval_id, &action_fingerprint, now).unwrap();
        stale
            .require_replan(
                "Foreground target changed; a new observation is required.".to_string(),
                now,
            )
            .unwrap();
        assert_eq!(stale.status, ComputerUseStepStatus::NeedsReplan);
        assert_eq!(stale.approval_request_id, None);

        let mut uncertain = ComputerUseStep::new_observed(
            Uuid::new_v4(),
            1,
            pre,
            ComputerUseUndoCapability::None,
            now,
        )
        .unwrap();
        uncertain.bind_action(binding, now).unwrap();
        approve_as_user(&mut uncertain, approval_id, &action_fingerprint, now).unwrap();
        let window = uncertain.pre_observation.window_fingerprint.clone();
        let window_title = uncertain.pre_observation.window_title_fingerprint.clone();
        let target = uncertain
            .pre_observation
            .target_fingerprint
            .clone()
            .unwrap();
        mark_started(
            &mut uncertain,
            approval_id,
            &window,
            &window_title,
            &target,
            now,
        )
        .unwrap();
        assert!(uncertain
            .cancel("Do not hide a possible external effect.".to_string(), now)
            .is_err());
        uncertain
            .mark_effect_unknown(
                "The input backend returned no reliable effect receipt.".to_string(),
                now,
            )
            .unwrap();
        assert_eq!(uncertain.status, ComputerUseStepStatus::EffectUnknown);
        assert_eq!(uncertain.action_start_count, 1);
    }

    #[test]
    fn takeover_and_restart_recovery_fail_closed() {
        let now = Utc::now();
        let pre = observation(ComputerUseObservationPhase::PreAction, Some("empty"), now);
        let binding = action(&pre, "verified text");
        let action_fingerprint = binding.action_fingerprint.clone();
        let approval_id = Uuid::new_v4();
        let mut ready = ComputerUseStep::new_observed(
            Uuid::new_v4(),
            1,
            pre.clone(),
            ComputerUseUndoCapability::CompensationRequired,
            now,
        )
        .unwrap();
        ready.bind_action(binding.clone(), now).unwrap();
        approve_as_user(&mut ready, approval_id, &action_fingerprint, now).unwrap();
        ready
            .take_over("User moved the mouse.".to_string(), now)
            .expect("takeover records");
        assert_eq!(ready.status, ComputerUseStepStatus::UserTakenOver);
        assert!(mark_started(
            &mut ready,
            approval_id,
            &pre.window_fingerprint,
            &pre.window_title_fingerprint,
            pre.target_fingerprint.as_deref().unwrap(),
            now,
        )
        .is_err());

        let mut started = ComputerUseStep::new_observed(
            Uuid::new_v4(),
            1,
            pre,
            ComputerUseUndoCapability::None,
            now,
        )
        .unwrap();
        started.bind_action(binding, now).unwrap();
        approve_as_user(&mut started, approval_id, &action_fingerprint, now).unwrap();
        let window = started.pre_observation.window_fingerprint.clone();
        let window_title = started.pre_observation.window_title_fingerprint.clone();
        let target = started.pre_observation.target_fingerprint.clone().unwrap();
        mark_started(
            &mut started,
            approval_id,
            &window,
            &window_title,
            &target,
            now,
        )
        .unwrap();
        started.recover_after_restart(now).unwrap();
        assert_eq!(started.status, ComputerUseStepStatus::EffectUnknown);
        assert_eq!(started.action_start_count, 1);
        assert!(mark_started(
            &mut started,
            approval_id,
            &window,
            &window_title,
            &target,
            now,
        )
        .is_err());
    }

    #[test]
    fn screenshot_only_evidence_cannot_claim_semantic_verification() {
        let now = Utc::now();
        let pre = observation(ComputerUseObservationPhase::PreAction, Some("empty"), now);
        let binding = action(&pre, "verified text");
        let action_fingerprint = binding.action_fingerprint.clone();
        let approval_id = Uuid::new_v4();
        let mut step = ComputerUseStep::new_observed(
            Uuid::new_v4(),
            1,
            pre,
            ComputerUseUndoCapability::None,
            now,
        )
        .unwrap();
        step.bind_action(binding, now).unwrap();
        approve_as_user(&mut step, approval_id, &action_fingerprint, now).unwrap();
        let window = step.pre_observation.window_fingerprint.clone();
        let window_title = step.pre_observation.window_title_fingerprint.clone();
        let target = step.pre_observation.target_fingerprint.clone().unwrap();
        mark_started(&mut step, approval_id, &window, &window_title, &target, now).unwrap();
        let post = observation(ComputerUseObservationPhase::PostAction, None, now);
        let post_fingerprint = post.fingerprint.clone();
        step.record_post_observation(post, now).unwrap();

        assert!(step
            .record_verification(ComputerUseVerificationReceipt {
                id: Uuid::new_v4(),
                action_fingerprint,
                post_observation_fingerprint: post_fingerprint,
                outcome: ComputerUseVerificationOutcome::Verified,
                safe_summary: "Screenshot captured.".to_string(),
                verified_at: now,
            })
            .is_err());
        assert_eq!(step.status, ComputerUseStepStatus::AwaitingVerification);
    }
}
