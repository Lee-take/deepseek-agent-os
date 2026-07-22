use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::kernel::goal_lifecycle::{
    GoalCompletionEvidenceReceipt, GoalCompletionProjection, GoalCompletionStatus,
    GoalFrozenEnvelope, GoalLifecycleProjection,
};

pub const CONTEXT_CHECKPOINT_VERSION: &str = "ds-agent.context-checkpoint/v1";
const GAP_FINGERPRINT_DOMAIN: &[u8] = b"ds-agent.goal-gap-fingerprint.v1\0";
const GAP_SET_FINGERPRINT_DOMAIN: &[u8] = b"ds-agent.goal-gap-set-fingerprint.v1\0";
const CHECKPOINT_FINGERPRINT_DOMAIN: &[u8] = b"ds-agent.context-checkpoint-fingerprint.v1\0";
const IDENTITY_FINGERPRINT_DOMAIN: &[u8] = b"ds-agent.context-identity-fingerprint.v1\0";
const TOOL_ROUND_FINGERPRINT_DOMAIN: &[u8] = b"ds-agent.context-tool-round-fingerprint.v1\0";
const MAX_CHECKPOINT_IDENTITIES: usize = 1_024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GoalContinuationObservationStage {
    InitialModel,
    AfterToolRound,
    AfterModelFollowup,
    Final,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GoalModelUsage {
    pub request_id: Uuid,
    pub elapsed_ms: u64,
    pub total_tokens: Option<u32>,
    pub estimated_cost_micro_usd: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GoalToolUsage {
    pub invocation_id: Uuid,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GoalContinuationObservation {
    pub stage: GoalContinuationObservationStage,
    pub local_tool_round: u32,
    pub model_usage: Vec<GoalModelUsage>,
    pub tool_usage: Vec<GoalToolUsage>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalGap {
    pub code: String,
    pub goal_revision: String,
    pub frozen_fingerprint: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalLoopBudgetLimits {
    pub max_model_rounds: u32,
    pub max_tool_rounds: u32,
    pub max_elapsed_ms: u64,
    pub max_tokens: u64,
    pub max_cost_micro_usd: u64,
    pub max_consecutive_non_improvement: u32,
}

impl Default for GoalLoopBudgetLimits {
    fn default() -> Self {
        Self {
            max_model_rounds: 5,
            max_tool_rounds: 4,
            max_elapsed_ms: 15 * 60 * 1_000,
            max_tokens: 64_000,
            max_cost_micro_usd: 5_000_000,
            max_consecutive_non_improvement: 2,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalLoopBudgetState {
    pub limits: GoalLoopBudgetLimits,
    pub model_rounds: u32,
    pub tool_rounds: u32,
    pub elapsed_ms: u64,
    pub tokens: u64,
    pub cost_micro_usd: u64,
    pub token_unknown_rounds: u32,
    pub cost_unknown_rounds: u32,
    pub evidence_total: u32,
    pub new_evidence_count: u32,
    pub consecutive_non_improvement: u32,
    pub accounted_model_request_ids: Vec<Uuid>,
    pub accounted_tool_invocation_ids: Vec<Uuid>,
    pub accounted_tool_round_fingerprints: Vec<String>,
    pub accounted_evidence_ids: Vec<Uuid>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalContinuationBlockerCode {
    ModelRoundBudgetExhausted,
    ToolRoundBudgetExhausted,
    ElapsedBudgetExhausted,
    TokenBudgetExhausted,
    CostBudgetExhausted,
    NoNewEvidence,
    RepeatedGaps,
}

impl GoalContinuationBlockerCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModelRoundBudgetExhausted => "model_round_budget_exhausted",
            Self::ToolRoundBudgetExhausted => "tool_round_budget_exhausted",
            Self::ElapsedBudgetExhausted => "elapsed_budget_exhausted",
            Self::TokenBudgetExhausted => "token_budget_exhausted",
            Self::CostBudgetExhausted => "cost_budget_exhausted",
            Self::NoNewEvidence => "no_new_evidence",
            Self::RepeatedGaps => "repeated_gaps",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GoalContinuationBlocker {
    pub code: GoalContinuationBlockerCode,
    pub gap_fingerprint: String,
    pub model_rounds: u32,
    pub tool_rounds: u32,
    pub elapsed_ms: u64,
    pub tokens: u64,
    pub cost_micro_usd: u64,
    pub evidence_total: u32,
}

impl GoalContinuationBlocker {
    pub(crate) fn stable_reason(&self) -> String {
        format!("goal_continuation_{}", self.code.as_str())
    }

    pub(crate) fn user_message(&self) -> String {
        match self.code {
            GoalContinuationBlockerCode::ModelRoundBudgetExhausted
            | GoalContinuationBlockerCode::ToolRoundBudgetExhausted
            | GoalContinuationBlockerCode::ElapsedBudgetExhausted
            | GoalContinuationBlockerCode::TokenBudgetExhausted
            | GoalContinuationBlockerCode::CostBudgetExhausted => {
                "DS Agent 已达到本任务的安全预算上限，未继续执行新的动作。请检查现有证据和缺口后再决定是否创建新任务。".to_string()
            }
            GoalContinuationBlockerCode::NoNewEvidence => {
                "DS Agent 本轮没有获得新的完成证据，已停止继续尝试，避免无证据循环。".to_string()
            }
            GoalContinuationBlockerCode::RepeatedGaps => {
                "DS Agent 检测到相同完成缺口连续未改善，已停止重复尝试并保留现有证据。".to_string()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCheckpointStatus {
    Continue,
    Complete,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextAuthorizationIdentity {
    pub group_id: Uuid,
    pub task_id: Uuid,
    pub projection_revision: u64,
    pub manifest_revision: String,
    pub manifest_fingerprint: String,
    pub preview_hash: String,
    pub status: String,
    pub capability_request_fingerprints: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextResourceIdentity {
    pub claim_id: Uuid,
    pub tool_invocation_id: Uuid,
    pub access: String,
    pub resource_key_fingerprint: String,
    pub lease_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextArtifactIdentity {
    pub artifact_id: String,
    pub kind: String,
    pub identity_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSourceIdentity {
    pub invocation_id: Uuid,
    pub tool_id: String,
    pub tool_version: String,
    pub request_fingerprint: String,
    pub source_fingerprint: String,
}

#[derive(Clone, Debug)]
pub(crate) struct ContextCheckpointSeed {
    pub run_id: Uuid,
    pub goal: GoalFrozenEnvelope,
    pub completion: GoalCompletionProjection,
    pub authorizations: Vec<ContextAuthorizationIdentity>,
    pub resources: Vec<ContextResourceIdentity>,
    pub artifacts: Vec<ContextArtifactIdentity>,
    pub sources: Vec<ContextSourceIdentity>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCheckpoint {
    pub version: String,
    pub run_id: Uuid,
    pub goal: GoalFrozenEnvelope,
    pub constraints: Vec<String>,
    pub authorizations: Vec<ContextAuthorizationIdentity>,
    pub evidence: Vec<GoalCompletionEvidenceReceipt>,
    pub gaps: Vec<GoalGap>,
    pub gap_fingerprint: String,
    pub resources: Vec<ContextResourceIdentity>,
    pub artifacts: Vec<ContextArtifactIdentity>,
    pub sources: Vec<ContextSourceIdentity>,
    pub budget: GoalLoopBudgetState,
    pub status: ContextCheckpointStatus,
    pub blocker: Option<GoalContinuationBlocker>,
    pub fingerprint: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct GoalGapCanonical<'a> {
    code: &'a str,
    goal_revision: &'a str,
    frozen_fingerprint: &'a str,
}

#[derive(Serialize)]
struct GoalGapSetCanonical<'a> {
    goal_revision: &'a str,
    frozen_fingerprint: &'a str,
    gap_fingerprints: Vec<&'a str>,
}

#[derive(Serialize)]
struct ContextCheckpointCanonical<'a> {
    version: &'a str,
    run_id: Uuid,
    goal: &'a GoalFrozenEnvelope,
    constraints: &'a [String],
    authorizations: &'a [ContextAuthorizationIdentity],
    evidence: &'a [GoalCompletionEvidenceReceipt],
    gaps: &'a [GoalGap],
    gap_fingerprint: &'a str,
    resources: &'a [ContextResourceIdentity],
    artifacts: &'a [ContextArtifactIdentity],
    sources: &'a [ContextSourceIdentity],
    budget: &'a GoalLoopBudgetState,
    status: ContextCheckpointStatus,
    blocker: &'a Option<GoalContinuationBlocker>,
}

#[derive(Serialize)]
struct ContextCheckpointPrompt<'a> {
    version: &'a str,
    goal_id: Uuid,
    goal_revision: &'a str,
    frozen_fingerprint: &'a str,
    user_goal: &'a str,
    constraints: &'a [String],
    done_when: Vec<(&'a str, &'a str)>,
    authorizations: &'a [ContextAuthorizationIdentity],
    evidence: &'a [GoalCompletionEvidenceReceipt],
    gaps: &'a [GoalGap],
    gap_fingerprint: &'a str,
    resources: &'a [ContextResourceIdentity],
    artifacts: &'a [ContextArtifactIdentity],
    sources: &'a [ContextSourceIdentity],
    budget: &'a GoalLoopBudgetState,
    status: ContextCheckpointStatus,
    blocker: &'a Option<GoalContinuationBlocker>,
    checkpoint_fingerprint: &'a str,
}

impl ContextCheckpoint {
    pub(crate) fn advance(
        previous: Option<&Self>,
        mut seed: ContextCheckpointSeed,
        mut observation: GoalContinuationObservation,
    ) -> Result<Self, &'static str> {
        if let Some(previous) = previous {
            previous.validate()?;
        }
        if seed.run_id.is_nil()
            || seed.run_id != seed.completion.goal_id
            || seed.goal.revision != seed.completion.revision
            || seed.goal.fingerprint != seed.completion.frozen_fingerprint
        {
            return Err("context_checkpoint_goal_binding_invalid");
        }
        normalize_seed(&mut seed)?;
        normalize_observation(&mut observation)?;

        let gaps = gaps_from_completion(&seed.goal, &seed.completion)?;
        let gap_fingerprint = gap_set_fingerprint(&seed.goal, &gaps)?;
        let mut budget = previous
            .map(|value| value.budget.clone())
            .unwrap_or_default();
        let previous_gap_fingerprint = previous.map(|value| value.gap_fingerprint.as_str());
        let existing_blocker = previous.and_then(|value| value.blocker.clone());
        let created_at = previous
            .map(|value| value.created_at)
            .unwrap_or(observation.observed_at);

        account_model_usage(&mut budget, &observation.model_usage);
        account_tool_usage(&mut budget, &observation.tool_usage);
        let new_evidence_count = account_evidence(&mut budget, &seed.completion.evidence);
        let new_tool_round = if observation.stage
            == GoalContinuationObservationStage::AfterToolRound
            && !observation.tool_usage.is_empty()
        {
            account_tool_round(&mut budget, seed.run_id, &seed.goal, &observation)?
        } else {
            false
        };
        budget.new_evidence_count = new_evidence_count;
        budget.evidence_total = u32::try_from(budget.accounted_evidence_ids.len())
            .map_err(|_| "context_checkpoint_evidence_limit")?;

        if new_tool_round {
            if previous_gap_fingerprint == Some(gap_fingerprint.as_str()) && new_evidence_count == 0
            {
                budget.consecutive_non_improvement = budget
                    .consecutive_non_improvement
                    .checked_add(1)
                    .ok_or("context_checkpoint_counter_overflow")?;
            } else {
                budget.consecutive_non_improvement = 0;
            }
        }

        let blocker = existing_blocker.or_else(|| {
            blocker_for(
                seed.completion.status,
                &gap_fingerprint,
                &budget,
                new_tool_round,
            )
        });
        let status = if blocker.is_some() {
            ContextCheckpointStatus::Blocked
        } else if seed.completion.status == GoalCompletionStatus::Complete {
            ContextCheckpointStatus::Complete
        } else {
            ContextCheckpointStatus::Continue
        };
        let constraints = seed.goal.envelope.constraints.clone();
        let mut checkpoint = Self {
            version: CONTEXT_CHECKPOINT_VERSION.to_string(),
            run_id: seed.run_id,
            goal: seed.goal,
            constraints,
            authorizations: seed.authorizations,
            evidence: seed.completion.evidence,
            gaps,
            gap_fingerprint,
            resources: seed.resources,
            artifacts: seed.artifacts,
            sources: seed.sources,
            budget,
            status,
            blocker,
            fingerprint: String::new(),
            created_at,
            updated_at: observation.observed_at,
        };
        checkpoint.fingerprint = checkpoint.recompute_fingerprint()?;
        checkpoint.validate()?;
        if let Some(previous) = previous {
            checkpoint.validate_monotonic(previous)?;
        }
        Ok(checkpoint)
    }

    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.version != CONTEXT_CHECKPOINT_VERSION
            || self.run_id.is_nil()
            || self.run_id
                != self
                    .evidence
                    .first()
                    .map_or(self.run_id, |item| item.goal_id)
            || self.goal.revision.is_empty()
            || !valid_hash(&self.goal.revision)
            || !valid_hash(&self.goal.fingerprint)
            || self.constraints != self.goal.envelope.constraints
            || self.updated_at < self.created_at
            || self.authorizations.len() > MAX_CHECKPOINT_IDENTITIES
            || self.evidence.len() > MAX_CHECKPOINT_IDENTITIES
            || self.resources.len() > MAX_CHECKPOINT_IDENTITIES
            || self.artifacts.len() > MAX_CHECKPOINT_IDENTITIES
            || self.sources.len() > MAX_CHECKPOINT_IDENTITIES
        {
            return Err("context_checkpoint_invalid");
        }
        for gap in &self.gaps {
            if gap.goal_revision != self.goal.revision
                || gap.frozen_fingerprint != self.goal.fingerprint
                || gap.fingerprint != gap_fingerprint(gap)?
                || !safe_code(&gap.code)
            {
                return Err("context_checkpoint_gap_invalid");
            }
        }
        if self.gap_fingerprint != gap_set_fingerprint(&self.goal, &self.gaps)?
            || self
                .authorizations
                .iter()
                .any(|item| !authorization_identity_valid(item, self.run_id))
            || self.resources.iter().any(|item| {
                item.claim_id.is_nil()
                    || item.tool_invocation_id.is_nil()
                    || !matches!(item.access.as_str(), "read" | "write")
                    || !valid_hash(&item.resource_key_fingerprint)
            })
            || self.artifacts.iter().any(|item| {
                !safe_code(&item.artifact_id)
                    || !safe_code(&item.kind)
                    || !valid_hash(&item.identity_fingerprint)
            })
            || self.sources.iter().any(|item| {
                item.invocation_id.is_nil()
                    || !safe_code(&item.tool_id)
                    || !safe_code(&item.tool_version)
                    || !valid_hash(&item.request_fingerprint)
                    || !valid_hash(&item.source_fingerprint)
            })
            || !budget_valid(&self.budget)
        {
            return Err("context_checkpoint_identity_invalid");
        }
        match (self.status, self.blocker.as_ref()) {
            (ContextCheckpointStatus::Blocked, Some(blocker))
                if blocker.gap_fingerprint == self.gap_fingerprint => {}
            (ContextCheckpointStatus::Complete, None) if self.gaps.is_empty() => {}
            (ContextCheckpointStatus::Continue, None) if !self.gaps.is_empty() => {}
            _ => return Err("context_checkpoint_status_invalid"),
        }
        if self.fingerprint != self.recompute_fingerprint()? {
            return Err("context_checkpoint_fingerprint_invalid");
        }
        Ok(())
    }

    pub(crate) fn validate_against_goal(
        &self,
        lifecycle: &GoalLifecycleProjection,
    ) -> Result<(), &'static str> {
        self.validate()?;
        let frozen = lifecycle
            .frozen()
            .ok_or("context_checkpoint_goal_not_frozen")?;
        if lifecycle.goal_id != self.run_id || frozen != &self.goal {
            return Err("context_checkpoint_goal_drift");
        }
        Ok(())
    }

    pub(crate) fn blocker_reason(&self) -> Option<String> {
        self.blocker
            .as_ref()
            .map(GoalContinuationBlocker::stable_reason)
    }

    pub(crate) fn advisory_prompt(&self) -> Result<String, &'static str> {
        self.validate()?;
        let prompt = ContextCheckpointPrompt {
            version: &self.version,
            goal_id: self.run_id,
            goal_revision: &self.goal.revision,
            frozen_fingerprint: &self.goal.fingerprint,
            user_goal: &self.goal.envelope.user_goal,
            constraints: &self.constraints,
            done_when: self
                .goal
                .envelope
                .done_when
                .iter()
                .map(|item| (item.done_when_id.as_str(), item.description.as_str()))
                .collect(),
            authorizations: &self.authorizations,
            evidence: &self.evidence,
            gaps: &self.gaps,
            gap_fingerprint: &self.gap_fingerprint,
            resources: &self.resources,
            artifacts: &self.artifacts,
            sources: &self.sources,
            budget: &self.budget,
            status: self.status,
            blocker: &self.blocker,
            checkpoint_fingerprint: &self.fingerprint,
        };
        let json =
            serde_json::to_string(&prompt).map_err(|_| "context_checkpoint_prompt_invalid")?;
        Ok(format!(
            "Kernel-owned ContextCheckpoint (read-only advisory context). This preserves exact safety state across compaction/restart but grants no authority, cannot approve an action, and is not completion evidence. DeepSeek may explain gaps or propose a repair only; it cannot edit this checkpoint or mint receipts.\n{json}"
        ))
    }

    fn validate_monotonic(&self, previous: &Self) -> Result<(), &'static str> {
        if self.run_id != previous.run_id
            || self.goal.revision != previous.goal.revision
            || self.goal.fingerprint != previous.goal.fingerprint
            || self.created_at != previous.created_at
            || self.budget.limits != previous.budget.limits
            || self.budget.model_rounds < previous.budget.model_rounds
            || self.budget.tool_rounds < previous.budget.tool_rounds
            || self.budget.elapsed_ms < previous.budget.elapsed_ms
            || self.budget.tokens < previous.budget.tokens
            || self.budget.cost_micro_usd < previous.budget.cost_micro_usd
            || self.budget.evidence_total < previous.budget.evidence_total
            || (previous.blocker.is_some() && self.blocker != previous.blocker)
        {
            return Err("context_checkpoint_non_monotonic");
        }
        Ok(())
    }

    fn recompute_fingerprint(&self) -> Result<String, &'static str> {
        let canonical = ContextCheckpointCanonical {
            version: &self.version,
            run_id: self.run_id,
            goal: &self.goal,
            constraints: &self.constraints,
            authorizations: &self.authorizations,
            evidence: &self.evidence,
            gaps: &self.gaps,
            gap_fingerprint: &self.gap_fingerprint,
            resources: &self.resources,
            artifacts: &self.artifacts,
            sources: &self.sources,
            budget: &self.budget,
            status: self.status,
            blocker: &self.blocker,
        };
        let bytes =
            serde_json::to_vec(&canonical).map_err(|_| "context_checkpoint_fingerprint_invalid")?;
        Ok(domain_hash(CHECKPOINT_FINGERPRINT_DOMAIN, &bytes))
    }
}

pub(crate) fn identity_fingerprint(value: &[u8]) -> String {
    domain_hash(IDENTITY_FINGERPRINT_DOMAIN, value)
}

fn normalize_seed(seed: &mut ContextCheckpointSeed) -> Result<(), &'static str> {
    if seed.authorizations.len() > MAX_CHECKPOINT_IDENTITIES
        || seed.completion.evidence.len() > MAX_CHECKPOINT_IDENTITIES
        || seed.resources.len() > MAX_CHECKPOINT_IDENTITIES
        || seed.artifacts.len() > MAX_CHECKPOINT_IDENTITIES
        || seed.sources.len() > MAX_CHECKPOINT_IDENTITIES
    {
        return Err("context_checkpoint_identity_limit");
    }
    seed.authorizations.sort_by_key(|item| item.group_id);
    seed.authorizations.dedup_by_key(|item| item.group_id);
    for item in &mut seed.authorizations {
        item.capability_request_fingerprints.sort();
        item.capability_request_fingerprints.dedup();
    }
    seed.resources.sort_by_key(|item| item.claim_id);
    seed.resources.dedup_by_key(|item| item.claim_id);
    seed.artifacts.sort_by(|left, right| {
        (&left.artifact_id, &left.kind, &left.identity_fingerprint).cmp(&(
            &right.artifact_id,
            &right.kind,
            &right.identity_fingerprint,
        ))
    });
    seed.artifacts.dedup();
    seed.sources.sort_by_key(|item| item.invocation_id);
    seed.sources.dedup_by_key(|item| item.invocation_id);
    Ok(())
}

fn normalize_observation(
    observation: &mut GoalContinuationObservation,
) -> Result<(), &'static str> {
    if observation.model_usage.len() > MAX_CHECKPOINT_IDENTITIES
        || observation.tool_usage.len() > MAX_CHECKPOINT_IDENTITIES
    {
        return Err("context_checkpoint_observation_limit");
    }
    observation.model_usage.sort_by_key(|item| item.request_id);
    observation.model_usage.dedup_by_key(|item| item.request_id);
    observation
        .tool_usage
        .sort_by_key(|item| item.invocation_id);
    observation
        .tool_usage
        .dedup_by_key(|item| item.invocation_id);
    Ok(())
}

fn gaps_from_completion(
    goal: &GoalFrozenEnvelope,
    completion: &GoalCompletionProjection,
) -> Result<Vec<GoalGap>, &'static str> {
    let mut codes = completion
        .failure_codes
        .iter()
        .map(|code| {
            serde_json::to_value(code)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .ok_or("context_checkpoint_gap_invalid")
        })
        .collect::<Result<Vec<_>, _>>()?;
    codes.sort();
    codes.dedup();
    codes
        .into_iter()
        .map(|code| {
            let mut gap = GoalGap {
                code,
                goal_revision: goal.revision.clone(),
                frozen_fingerprint: goal.fingerprint.clone(),
                fingerprint: String::new(),
            };
            gap.fingerprint = gap_fingerprint(&gap)?;
            Ok(gap)
        })
        .collect()
}

fn gap_fingerprint(gap: &GoalGap) -> Result<String, &'static str> {
    if !safe_code(&gap.code)
        || !valid_hash(&gap.goal_revision)
        || !valid_hash(&gap.frozen_fingerprint)
    {
        return Err("context_checkpoint_gap_invalid");
    }
    let bytes = serde_json::to_vec(&GoalGapCanonical {
        code: &gap.code,
        goal_revision: &gap.goal_revision,
        frozen_fingerprint: &gap.frozen_fingerprint,
    })
    .map_err(|_| "context_checkpoint_gap_invalid")?;
    Ok(domain_hash(GAP_FINGERPRINT_DOMAIN, &bytes))
}

fn gap_set_fingerprint(
    goal: &GoalFrozenEnvelope,
    gaps: &[GoalGap],
) -> Result<String, &'static str> {
    let bytes = serde_json::to_vec(&GoalGapSetCanonical {
        goal_revision: &goal.revision,
        frozen_fingerprint: &goal.fingerprint,
        gap_fingerprints: gaps.iter().map(|gap| gap.fingerprint.as_str()).collect(),
    })
    .map_err(|_| "context_checkpoint_gap_invalid")?;
    Ok(domain_hash(GAP_SET_FINGERPRINT_DOMAIN, &bytes))
}

fn account_model_usage(budget: &mut GoalLoopBudgetState, usage: &[GoalModelUsage]) {
    let mut accounted = budget
        .accounted_model_request_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    for item in usage {
        if item.request_id.is_nil() || !accounted.insert(item.request_id) {
            continue;
        }
        budget.model_rounds = budget.model_rounds.saturating_add(1);
        budget.elapsed_ms = budget.elapsed_ms.saturating_add(item.elapsed_ms);
        match item.total_tokens {
            Some(tokens) => budget.tokens = budget.tokens.saturating_add(u64::from(tokens)),
            None => budget.token_unknown_rounds = budget.token_unknown_rounds.saturating_add(1),
        }
        match item.estimated_cost_micro_usd {
            Some(cost) => budget.cost_micro_usd = budget.cost_micro_usd.saturating_add(cost),
            None => budget.cost_unknown_rounds = budget.cost_unknown_rounds.saturating_add(1),
        }
        budget.accounted_model_request_ids.push(item.request_id);
    }
    budget.accounted_model_request_ids.sort();
}

fn account_tool_usage(budget: &mut GoalLoopBudgetState, usage: &[GoalToolUsage]) {
    let mut accounted = budget
        .accounted_tool_invocation_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    for item in usage {
        if item.invocation_id.is_nil() || !accounted.insert(item.invocation_id) {
            continue;
        }
        budget.elapsed_ms = budget.elapsed_ms.saturating_add(item.elapsed_ms);
        budget
            .accounted_tool_invocation_ids
            .push(item.invocation_id);
    }
    budget.accounted_tool_invocation_ids.sort();
}

fn account_evidence(
    budget: &mut GoalLoopBudgetState,
    evidence: &[GoalCompletionEvidenceReceipt],
) -> u32 {
    let mut accounted = budget
        .accounted_evidence_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut added = 0_u32;
    for item in evidence {
        if item.evidence_id.is_nil() || !accounted.insert(item.evidence_id) {
            continue;
        }
        added = added.saturating_add(1);
        budget.accounted_evidence_ids.push(item.evidence_id);
    }
    budget.accounted_evidence_ids.sort();
    added
}

fn account_tool_round(
    budget: &mut GoalLoopBudgetState,
    run_id: Uuid,
    goal: &GoalFrozenEnvelope,
    observation: &GoalContinuationObservation,
) -> Result<bool, &'static str> {
    let mut model_ids = observation
        .model_usage
        .iter()
        .map(|item| item.request_id)
        .collect::<Vec<_>>();
    model_ids.sort();
    let mut tool_ids = observation
        .tool_usage
        .iter()
        .map(|item| item.invocation_id)
        .collect::<Vec<_>>();
    tool_ids.sort();
    let bytes = serde_json::to_vec(&(
        run_id,
        &goal.revision,
        &goal.fingerprint,
        observation.local_tool_round,
        model_ids,
        tool_ids,
    ))
    .map_err(|_| "context_checkpoint_round_invalid")?;
    let fingerprint = domain_hash(TOOL_ROUND_FINGERPRINT_DOMAIN, &bytes);
    if budget
        .accounted_tool_round_fingerprints
        .iter()
        .any(|item| item == &fingerprint)
    {
        return Ok(false);
    }
    budget.tool_rounds = budget
        .tool_rounds
        .checked_add(1)
        .ok_or("context_checkpoint_counter_overflow")?;
    budget.accounted_tool_round_fingerprints.push(fingerprint);
    budget.accounted_tool_round_fingerprints.sort();
    Ok(true)
}

fn blocker_for(
    completion_status: GoalCompletionStatus,
    gap_fingerprint: &str,
    budget: &GoalLoopBudgetState,
    new_tool_round: bool,
) -> Option<GoalContinuationBlocker> {
    if completion_status == GoalCompletionStatus::Complete {
        return None;
    }
    let code = if budget.model_rounds >= budget.limits.max_model_rounds {
        Some(GoalContinuationBlockerCode::ModelRoundBudgetExhausted)
    } else if budget.tool_rounds >= budget.limits.max_tool_rounds {
        Some(GoalContinuationBlockerCode::ToolRoundBudgetExhausted)
    } else if budget.elapsed_ms >= budget.limits.max_elapsed_ms {
        Some(GoalContinuationBlockerCode::ElapsedBudgetExhausted)
    } else if budget.tokens >= budget.limits.max_tokens {
        Some(GoalContinuationBlockerCode::TokenBudgetExhausted)
    } else if budget.cost_micro_usd >= budget.limits.max_cost_micro_usd {
        Some(GoalContinuationBlockerCode::CostBudgetExhausted)
    } else if new_tool_round && budget.evidence_total == 0 {
        Some(GoalContinuationBlockerCode::NoNewEvidence)
    } else if new_tool_round
        && budget.consecutive_non_improvement >= budget.limits.max_consecutive_non_improvement
    {
        Some(GoalContinuationBlockerCode::RepeatedGaps)
    } else {
        None
    }?;
    Some(GoalContinuationBlocker {
        code,
        gap_fingerprint: gap_fingerprint.to_string(),
        model_rounds: budget.model_rounds,
        tool_rounds: budget.tool_rounds,
        elapsed_ms: budget.elapsed_ms,
        tokens: budget.tokens,
        cost_micro_usd: budget.cost_micro_usd,
        evidence_total: budget.evidence_total,
    })
}

fn authorization_identity_valid(item: &ContextAuthorizationIdentity, run_id: Uuid) -> bool {
    item.group_id != Uuid::nil()
        && item.task_id == run_id
        && valid_hash(&item.manifest_revision)
        && valid_hash(&item.manifest_fingerprint)
        && valid_hash(&item.preview_hash)
        && safe_code(&item.status)
        && item
            .capability_request_fingerprints
            .iter()
            .all(|fingerprint| valid_hash(fingerprint))
}

fn budget_valid(budget: &GoalLoopBudgetState) -> bool {
    let limits = &budget.limits;
    limits.max_model_rounds > 0
        && limits.max_tool_rounds > 0
        && limits.max_elapsed_ms > 0
        && limits.max_tokens > 0
        && limits.max_cost_micro_usd > 0
        && limits.max_consecutive_non_improvement > 0
        && budget.model_rounds
            == u32::try_from(budget.accounted_model_request_ids.len()).unwrap_or(u32::MAX)
        && budget.tool_rounds
            == u32::try_from(budget.accounted_tool_round_fingerprints.len()).unwrap_or(u32::MAX)
        && budget.evidence_total
            == u32::try_from(budget.accounted_evidence_ids.len()).unwrap_or(u32::MAX)
        && all_unique(&budget.accounted_model_request_ids)
        && all_unique(&budget.accounted_tool_invocation_ids)
        && all_unique(&budget.accounted_evidence_ids)
        && all_unique(&budget.accounted_tool_round_fingerprints)
        && budget
            .accounted_tool_round_fingerprints
            .iter()
            .all(|fingerprint| valid_hash(fingerprint))
}

fn all_unique<T: Ord + Clone>(values: &[T]) -> bool {
    values.iter().cloned().collect::<BTreeSet<_>>().len() == values.len()
}

fn safe_code(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn domain_hash(domain: &[u8], value: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use serde_json::json;

    use super::*;
    use crate::kernel::event_store::EventStore;
    use crate::kernel::goal_envelope::GOAL_ENVELOPE_PROPOSAL_VERSION;
    use crate::kernel::goal_lifecycle::{
        completion_projection, GoalCompletionEvidenceReceipt, GoalValidationContext,
        GoalVerifierEvidenceStatus,
    };
    use crate::kernel::local_directory::WorkspaceReadinessCode;
    use crate::kernel::models::AccessMode;
    use crate::kernel::tool_runtime::FILE_READ_TOOL_ID;

    fn seed() -> ContextCheckpointSeed {
        let store = EventStore::open_memory().expect("store opens");
        let run_id = Uuid::new_v4();
        let proposal = crate::kernel::goal_envelope::GoalEnvelopeProposal::parse_value(json!({
            "version": GOAL_ENVELOPE_PROPOSAL_VERSION,
            "user_goal": "Create one verified brief.",
            "assumptions": [],
            "constraints": ["Keep output inside the approved workspace."],
            "done_when": [{"done_when_id":"brief-ready","description":"The brief is verified."}],
            "required_artifacts": [{"artifact_id":"brief","description":"The brief."}],
            "verifiers": [{"verifier_id":"brief-verifier","done_when_id":"brief-ready","description":"Verify the brief.","evidence_kind":"brief-evidence"}],
            "proposed_capabilities": [FILE_READ_TOOL_ID],
            "external_targets": [{"target_id":"selected-workspace","description":"Bound locally."}],
            "stop_conditions": ["Stop without evidence."]
        }))
        .expect("proposal parses");
        let context =
            GoalValidationContext::new(AccessMode::FullAccess, WorkspaceReadinessCode::Ready)
                .with_enabled_tool(FILE_READ_TOOL_ID, true)
                .with_verifier_kind("brief-evidence")
                .with_target_binding(
                    "selected-workspace",
                    crate::kernel::goal_lifecycle::GoalTargetBindingKind::Workspace,
                    b"bounded-workspace-identity",
                );
        let validated = store
            .submit_goal_proposal(run_id, &proposal, &context)
            .expect("goal validates");
        let lifecycle = store
            .freeze_goal_envelope(run_id, validated.revision().expect("revision"))
            .expect("goal freezes");
        let goal = lifecycle.frozen().expect("frozen goal").clone();
        let completion = completion_projection(&lifecycle, &[]).expect("projection builds");
        ContextCheckpointSeed {
            run_id,
            goal,
            completion,
            authorizations: Vec::new(),
            resources: Vec::new(),
            artifacts: Vec::new(),
            sources: Vec::new(),
        }
    }

    fn observation(
        stage: GoalContinuationObservationStage,
        request_id: Uuid,
        tool_id: Option<Uuid>,
        observed_at: DateTime<Utc>,
    ) -> GoalContinuationObservation {
        GoalContinuationObservation {
            stage,
            local_tool_round: u32::from(tool_id.is_some()),
            model_usage: vec![GoalModelUsage {
                request_id,
                elapsed_ms: 10,
                total_tokens: Some(20),
                estimated_cost_micro_usd: Some(30),
            }],
            tool_usage: tool_id
                .map(|invocation_id| {
                    vec![GoalToolUsage {
                        invocation_id,
                        elapsed_ms: 5,
                    }]
                })
                .unwrap_or_default(),
            observed_at,
        }
    }

    #[test]
    fn gaps_are_stable_secret_free_and_revision_bound() {
        let seed = seed();
        let checkpoint = ContextCheckpoint::advance(
            None,
            seed,
            observation(
                GoalContinuationObservationStage::Final,
                Uuid::new_v4(),
                None,
                Utc::now(),
            ),
        )
        .expect("checkpoint builds");

        assert_eq!(checkpoint.gaps.len(), 2);
        assert!(checkpoint
            .gaps
            .iter()
            .all(|gap| gap.code.starts_with("missing_")));
        assert!(!serde_json::to_string(&checkpoint.gaps)
            .unwrap()
            .contains("bounded-workspace-identity"));
        assert!(checkpoint
            .gaps
            .iter()
            .all(|gap| gap.goal_revision == checkpoint.goal.revision));
    }

    #[test]
    fn no_evidence_and_budget_exhaustion_become_deterministic_blockers() {
        let now = Utc::now();
        let no_evidence = ContextCheckpoint::advance(
            None,
            seed(),
            observation(
                GoalContinuationObservationStage::AfterToolRound,
                Uuid::new_v4(),
                Some(Uuid::new_v4()),
                now,
            ),
        )
        .expect("checkpoint builds");
        assert_eq!(
            no_evidence.blocker.as_ref().map(|item| item.code),
            Some(GoalContinuationBlockerCode::NoNewEvidence)
        );

        let budget_seed = seed();
        let initial = ContextCheckpoint::advance(
            None,
            budget_seed.clone(),
            observation(
                GoalContinuationObservationStage::InitialModel,
                Uuid::new_v4(),
                None,
                now,
            ),
        )
        .expect("checkpoint builds");
        let mut exhausting_observation = observation(
            GoalContinuationObservationStage::AfterModelFollowup,
            Uuid::new_v4(),
            None,
            now + Duration::seconds(1),
        );
        exhausting_observation.model_usage[0].total_tokens = Some(64_000);
        let exhausted =
            ContextCheckpoint::advance(Some(&initial), budget_seed, exhausting_observation)
                .expect("checkpoint advances");
        assert_eq!(
            exhausted.blocker.as_ref().map(|item| item.code),
            Some(GoalContinuationBlockerCode::TokenBudgetExhausted)
        );
    }

    #[test]
    fn repeated_unchanged_gaps_become_a_deterministic_blocker() {
        let now = Utc::now();
        let mut stable_seed = seed();
        stable_seed
            .completion
            .evidence
            .push(GoalCompletionEvidenceReceipt {
                evidence_id: Uuid::new_v4(),
                goal_id: stable_seed.run_id,
                revision: stable_seed.goal.revision.clone(),
                frozen_fingerprint: stable_seed.goal.fingerprint.clone(),
                verifier_id: "brief-verifier".to_string(),
                done_when_id: "brief-ready".to_string(),
                evidence_kind: "brief-evidence".to_string(),
                artifact_ids: vec!["brief".to_string()],
                status: GoalVerifierEvidenceStatus::Passed,
                source_fingerprint: "3".repeat(64),
            });

        let first = ContextCheckpoint::advance(
            None,
            stable_seed.clone(),
            observation(
                GoalContinuationObservationStage::AfterToolRound,
                Uuid::new_v4(),
                Some(Uuid::new_v4()),
                now,
            ),
        )
        .expect("first checkpoint builds");
        let second = ContextCheckpoint::advance(
            Some(&first),
            stable_seed.clone(),
            observation(
                GoalContinuationObservationStage::AfterToolRound,
                Uuid::new_v4(),
                Some(Uuid::new_v4()),
                now + Duration::seconds(1),
            ),
        )
        .expect("second checkpoint builds");
        let blocked = ContextCheckpoint::advance(
            Some(&second),
            stable_seed,
            observation(
                GoalContinuationObservationStage::AfterToolRound,
                Uuid::new_v4(),
                Some(Uuid::new_v4()),
                now + Duration::seconds(2),
            ),
        )
        .expect("third checkpoint builds");

        assert_eq!(blocked.budget.consecutive_non_improvement, 2);
        assert_eq!(
            blocked.blocker.as_ref().map(|item| item.code),
            Some(GoalContinuationBlockerCode::RepeatedGaps)
        );
    }

    #[test]
    fn checkpoint_prompt_preserves_safety_state_without_minting_authority() {
        let mut seed = seed();
        seed.authorizations.push(ContextAuthorizationIdentity {
            group_id: Uuid::new_v4(),
            task_id: seed.run_id,
            projection_revision: 7,
            manifest_revision: "a".repeat(64),
            manifest_fingerprint: "b".repeat(64),
            preview_hash: "c".repeat(64),
            status: "approved".to_string(),
            capability_request_fingerprints: vec!["d".repeat(64)],
        });
        seed.resources.push(ContextResourceIdentity {
            claim_id: Uuid::new_v4(),
            tool_invocation_id: Uuid::new_v4(),
            access: "write".to_string(),
            resource_key_fingerprint: "e".repeat(64),
            lease_expires_at: Utc::now() + Duration::minutes(5),
        });
        seed.artifacts.push(ContextArtifactIdentity {
            artifact_id: "brief".to_string(),
            kind: "pptx".to_string(),
            identity_fingerprint: "f".repeat(64),
        });
        seed.sources.push(ContextSourceIdentity {
            invocation_id: Uuid::new_v4(),
            tool_id: FILE_READ_TOOL_ID.to_string(),
            tool_version: "1".to_string(),
            request_fingerprint: "1".repeat(64),
            source_fingerprint: "2".repeat(64),
        });
        let checkpoint = ContextCheckpoint::advance(
            None,
            seed,
            observation(
                GoalContinuationObservationStage::Final,
                Uuid::new_v4(),
                None,
                Utc::now(),
            ),
        )
        .expect("checkpoint builds");
        let prompt = checkpoint.advisory_prompt().expect("prompt renders");

        assert!(prompt.contains("grants no authority"));
        assert!(prompt.contains("cannot edit this checkpoint or mint receipts"));
        assert!(prompt.contains("approved"));
        assert!(!prompt.contains("bounded-workspace-identity"));
    }
}
