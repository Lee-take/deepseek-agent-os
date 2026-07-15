use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const EXPERT_TEAM_MAX_MEMBERS: usize = 4;
pub const EXPERT_TEAM_MAX_TOTAL_ATTEMPTS: usize = 8;
pub const EXPERT_TEAM_MAX_DEPENDENCIES: usize = 3;
pub const EXPERT_TEAM_MAX_RESOURCES: usize = 8;
pub const EXPERT_TEAM_MAX_CAPABILITIES: usize = 4;
pub const EXPERT_TEAM_MAX_CLAIMS: usize = 32;
pub const EXPERT_TEAM_MAX_EVIDENCE: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpertRole {
    Research,
    Analysis,
    Production,
    Review,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpertCapability {
    FileRead,
    NetworkSearch,
    BrowserBrowse,
    ManagedStagingWrite,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpertResourceAccess {
    Read,
    Write,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertResourceRequirement {
    pub key: String,
    pub access: ExpertResourceAccess,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertBudget {
    pub max_elapsed_ms: u64,
    pub max_tool_calls: u32,
    pub max_tokens: u32,
    pub max_output_bytes: u64,
    pub max_staged_bytes: u64,
}

impl Default for ExpertBudget {
    fn default() -> Self {
        Self {
            max_elapsed_ms: 180_000,
            max_tool_calls: 8,
            max_tokens: 12_000,
            max_output_bytes: 64 * 1024,
            max_staged_bytes: 128 * 1024,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertOutputContract {
    #[serde(default)]
    pub min_evidence_sources: usize,
    #[serde(default)]
    pub require_claims: bool,
    #[serde(default)]
    pub require_staged_output: bool,
    #[serde(default)]
    pub require_review: bool,
    #[serde(default)]
    pub fail_on_unresolved_conflict: bool,
}

impl Default for ExpertOutputContract {
    fn default() -> Self {
        Self {
            min_evidence_sources: 0,
            require_claims: false,
            require_staged_output: false,
            require_review: false,
            fail_on_unresolved_conflict: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertRetryPolicy {
    pub max_attempts: u8,
    #[serde(default)]
    pub substitute_role: Option<ExpertRole>,
}

impl Default for ExpertRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            substitute_role: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertTeamPlanItem {
    pub key: String,
    pub role: ExpertRole,
    pub prompt: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<ExpertCapability>,
    #[serde(default)]
    pub resources: Vec<ExpertResourceRequirement>,
    #[serde(default)]
    pub budget: ExpertBudget,
    #[serde(default)]
    pub output_contract: ExpertOutputContract,
    #[serde(default)]
    pub retry_policy: ExpertRetryPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertAttemptContract {
    pub team_id: Uuid,
    pub parent_run_id: Uuid,
    pub parent_input_revision: String,
    pub key: String,
    pub role: ExpertRole,
    pub attempt: u8,
    #[serde(default)]
    pub previous_attempt_run_id: Option<Uuid>,
    pub prompt: String,
    pub depends_on: Vec<String>,
    pub capabilities: Vec<ExpertCapability>,
    pub resources: Vec<ExpertResourceRequirement>,
    pub budget: ExpertBudget,
    pub output_contract: ExpertOutputContract,
    pub retry_policy: ExpertRetryPolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpertClaimStance {
    Supports,
    Contradicts,
    Uncertain,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertClaim {
    pub key: String,
    pub statement: String,
    pub stance: ExpertClaimStance,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertReviewVerdict {
    pub target_revision: String,
    pub decision: ExpertReviewDecision,
    #[serde(default)]
    pub findings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpertReviewDecision {
    Accept,
    Reject,
    NeedsRevision,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertOutput {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub claims: Vec<ExpertClaim>,
    #[serde(default)]
    pub staged_content: Option<String>,
    #[serde(default)]
    pub staged_relative_path: Option<String>,
    #[serde(default)]
    pub review: Option<ExpertReviewVerdict>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertEvidenceRef {
    pub id: String,
    pub kind: String,
    pub reference: String,
    pub summary: String,
    pub verified: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertStagingManifest {
    pub relative_path: String,
    pub absolute_path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertQualityGate {
    pub code: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpertExternalEffectState {
    None,
    VerifiedReadOnly,
    ManagedStagingOnly,
    Uncertain,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertAttemptUsage {
    pub elapsed_ms: u64,
    pub tool_calls: u32,
    pub tokens: u32,
    pub output_bytes: u64,
    pub staged_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertAttemptResult {
    pub id: Uuid,
    pub run_id: Uuid,
    pub parent_run_id: Uuid,
    pub key: String,
    pub role: ExpertRole,
    pub attempt: u8,
    pub parent_input_revision: String,
    pub output_revision: String,
    pub summary: String,
    pub claims: Vec<ExpertClaim>,
    pub evidence: Vec<ExpertEvidenceRef>,
    pub unresolved_conflicts: Vec<String>,
    pub missing_evidence: Vec<String>,
    pub usage: ExpertAttemptUsage,
    pub quality_gates: Vec<ExpertQualityGate>,
    pub staging: Option<ExpertStagingManifest>,
    pub review: Option<ExpertReviewVerdict>,
    pub external_effect_state: ExpertExternalEffectState,
    pub retry_eligible: bool,
    pub recorded_at: DateTime<Utc>,
}

impl ExpertAttemptResult {
    pub fn passed(&self) -> bool {
        !self.quality_gates.is_empty() && self.quality_gates.iter().all(|gate| gate.passed)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExpertMergeReceipt {
    pub id: Uuid,
    pub parent_run_id: Uuid,
    pub parent_input_revision: String,
    pub production_run_id: Uuid,
    pub production_revision: String,
    pub review_run_id: Uuid,
    pub merged_at: DateTime<Utc>,
}

pub fn sha256_text(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

pub fn parent_input_revision(prompt: &str) -> String {
    sha256_text(prompt.trim())
}

pub fn validate_team_plan(
    parent_run_id: Uuid,
    parent_prompt: &str,
    items: &[ExpertTeamPlanItem],
) -> Result<Vec<ExpertAttemptContract>, String> {
    if parent_run_id.is_nil() {
        return Err("expert team parent run id is required".to_string());
    }
    if items.len() < 2 || items.len() > EXPERT_TEAM_MAX_MEMBERS {
        return Err(format!(
            "expert team must contain between 2 and {EXPERT_TEAM_MAX_MEMBERS} roles"
        ));
    }

    let team_id = Uuid::new_v4();
    let revision = parent_input_revision(parent_prompt);
    let mut keys = HashSet::new();
    let mut roles = HashSet::new();
    for item in items {
        validate_key(&item.key, "expert subtask key")?;
        if !keys.insert(item.key.trim().to_ascii_lowercase()) {
            return Err("expert subtask keys must be unique".to_string());
        }
        if !roles.insert(item.role) {
            return Err("expert roles must be unique within one team".to_string());
        }
        validate_text(&item.prompt, "expert prompt", 4_000)?;
        if item.output_contract.min_evidence_sources > 16 {
            return Err("expert output contract requires too many evidence sources".to_string());
        }
        validate_budget(&item.budget)?;
        validate_retry_policy(item.role, &item.retry_policy)?;
        validate_capabilities(item)?;
        validate_resources(item)?;
    }

    for item in items {
        if item.depends_on.len() > EXPERT_TEAM_MAX_DEPENDENCIES {
            return Err(format!(
                "expert role may depend on at most {EXPERT_TEAM_MAX_DEPENDENCIES} roles"
            ));
        }
        let mut dependencies = HashSet::new();
        for dependency in &item.depends_on {
            validate_key(dependency, "expert dependency key")?;
            let normalized = dependency.trim().to_ascii_lowercase();
            if normalized == item.key.trim().to_ascii_lowercase() {
                return Err("expert role cannot depend on itself".to_string());
            }
            if !keys.contains(&normalized) {
                return Err(format!("expert dependency `{dependency}` does not exist"));
            }
            if !dependencies.insert(normalized) {
                return Err("expert dependencies must be unique".to_string());
            }
        }
    }
    reject_dependency_cycles(items)?;

    Ok(items
        .iter()
        .map(|item| ExpertAttemptContract {
            team_id,
            parent_run_id,
            parent_input_revision: revision.clone(),
            key: item.key.trim().to_string(),
            role: item.role,
            attempt: 1,
            previous_attempt_run_id: None,
            prompt: item.prompt.trim().to_string(),
            depends_on: item
                .depends_on
                .iter()
                .map(|value| value.trim().to_string())
                .collect(),
            capabilities: item.capabilities.clone(),
            resources: item.resources.clone(),
            budget: item.budget.clone(),
            output_contract: host_output_contract(item.role, &item.output_contract),
            retry_policy: item.retry_policy.clone(),
        })
        .collect())
}

fn host_output_contract(role: ExpertRole, proposed: &ExpertOutputContract) -> ExpertOutputContract {
    let mut contract = proposed.clone();
    match role {
        ExpertRole::Research => {
            contract.min_evidence_sources = contract.min_evidence_sources.max(2);
            contract.require_claims = true;
        }
        ExpertRole::Analysis => {
            contract.min_evidence_sources = contract.min_evidence_sources.max(1);
            contract.require_claims = true;
        }
        ExpertRole::Production => {
            contract.require_staged_output = true;
        }
        ExpertRole::Review => {
            contract.require_review = true;
        }
    }
    contract
}

fn validate_budget(budget: &ExpertBudget) -> Result<(), String> {
    if !(1_000..=900_000).contains(&budget.max_elapsed_ms)
        || !(1..=32).contains(&budget.max_tool_calls)
        || !(256..=64_000).contains(&budget.max_tokens)
        || !(1_024..=1_048_576).contains(&budget.max_output_bytes)
        || budget.max_staged_bytes > 4 * 1_048_576
    {
        return Err("expert budget is outside host limits".to_string());
    }
    Ok(())
}

fn validate_retry_policy(role: ExpertRole, policy: &ExpertRetryPolicy) -> Result<(), String> {
    if !(1..=2).contains(&policy.max_attempts) {
        return Err("expert retry policy allows one or two total attempts".to_string());
    }
    if policy.substitute_role.is_some_and(|substitute| {
        substitute == role
            || matches!(role, ExpertRole::Production | ExpertRole::Review)
            || matches!(substitute, ExpertRole::Production | ExpertRole::Review)
    }) {
        return Err(
            "expert substitute role may only switch between research and analysis".to_string(),
        );
    }
    Ok(())
}

fn validate_capabilities(item: &ExpertTeamPlanItem) -> Result<(), String> {
    if item.capabilities.is_empty() || item.capabilities.len() > EXPERT_TEAM_MAX_CAPABILITIES {
        return Err("expert capability scope is empty or too large".to_string());
    }
    let unique = item.capabilities.iter().copied().collect::<HashSet<_>>();
    if unique.len() != item.capabilities.len() {
        return Err("expert capability scope contains duplicates".to_string());
    }
    let staging = unique.contains(&ExpertCapability::ManagedStagingWrite);
    if staging != (item.role == ExpertRole::Production) {
        return Err("managed staging write is required only for the production role".to_string());
    }
    if item.role == ExpertRole::Review && item.output_contract.require_staged_output {
        return Err("review role cannot produce staged output".to_string());
    }
    Ok(())
}

fn validate_resources(item: &ExpertTeamPlanItem) -> Result<(), String> {
    if item.resources.len() > EXPERT_TEAM_MAX_RESOURCES {
        return Err("expert resource scope is too large".to_string());
    }
    let mut keys = HashSet::new();
    for resource in &item.resources {
        validate_key(&resource.key, "expert resource key")?;
        if !keys.insert(resource.key.trim().to_ascii_lowercase()) {
            return Err("expert resource keys must be unique per role".to_string());
        }
        if resource.access == ExpertResourceAccess::Write && item.role != ExpertRole::Production {
            return Err("only production may request a write resource".to_string());
        }
    }
    if item.role == ExpertRole::Production
        && !item
            .resources
            .iter()
            .any(|resource| resource.access == ExpertResourceAccess::Write)
    {
        return Err("production requires a managed staging write resource".to_string());
    }
    Ok(())
}

fn reject_dependency_cycles(items: &[ExpertTeamPlanItem]) -> Result<(), String> {
    let dependencies = items
        .iter()
        .map(|item| {
            (
                item.key.trim().to_ascii_lowercase(),
                item.depends_on
                    .iter()
                    .map(|value| value.trim().to_ascii_lowercase())
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<HashMap<_, _>>();
    fn visit(
        key: &str,
        dependencies: &HashMap<String, Vec<String>>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) -> Result<(), String> {
        if visited.contains(key) {
            return Ok(());
        }
        if !visiting.insert(key.to_string()) {
            return Err("expert dependency graph contains a cycle".to_string());
        }
        for dependency in dependencies.get(key).into_iter().flatten() {
            visit(dependency, dependencies, visiting, visited)?;
        }
        visiting.remove(key);
        visited.insert(key.to_string());
        Ok(())
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for key in dependencies.keys() {
        visit(key, &dependencies, &mut visiting, &mut visited)?;
    }
    Ok(())
}

pub fn resources_conflict(
    left: &[ExpertResourceRequirement],
    right: &[ExpertResourceRequirement],
) -> bool {
    left.iter().any(|left_resource| {
        right.iter().any(|right_resource| {
            left_resource.key.eq_ignore_ascii_case(&right_resource.key)
                && (left_resource.access == ExpertResourceAccess::Write
                    || right_resource.access == ExpertResourceAccess::Write)
        })
    })
}

pub fn evidence_identity(kind: &str, reference: &str) -> String {
    sha256_text(&format!(
        "{}\n{}",
        kind.trim().to_ascii_lowercase(),
        reference.trim()
    ))
}

pub fn deduplicate_evidence(evidence: Vec<ExpertEvidenceRef>) -> Vec<ExpertEvidenceRef> {
    let mut by_id = HashMap::<String, ExpertEvidenceRef>::new();
    for mut item in evidence.into_iter().take(EXPERT_TEAM_MAX_EVIDENCE) {
        item.kind = item.kind.trim().to_string();
        item.reference = item.reference.trim().to_string();
        item.id = evidence_identity(&item.kind, &item.reference);
        by_id
            .entry(item.id.clone())
            .and_modify(|existing| {
                existing.verified |= item.verified;
                if existing.summary.is_empty() {
                    existing.summary = item.summary.clone();
                }
            })
            .or_insert(item);
    }
    let mut evidence = by_id.into_values().collect::<Vec<_>>();
    evidence.sort_by(|left, right| left.id.cmp(&right.id));
    evidence
}

pub fn unresolved_claim_conflicts(claims: &[ExpertClaim]) -> Vec<String> {
    let mut stances = HashMap::<String, HashSet<ExpertClaimStance>>::new();
    for claim in claims.iter().take(EXPERT_TEAM_MAX_CLAIMS) {
        stances
            .entry(claim.key.trim().to_ascii_lowercase())
            .or_default()
            .insert(claim.stance);
    }
    let mut conflicts = stances
        .into_iter()
        .filter_map(|(key, stances)| {
            (stances.contains(&ExpertClaimStance::Supports)
                && stances.contains(&ExpertClaimStance::Contradicts))
            .then_some(key)
        })
        .collect::<Vec<_>>();
    conflicts.sort();
    conflicts
}

pub fn safe_staging_relative_path(value: &str) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 240 {
        return Err("expert staging relative path is empty or too long".to_string());
    }
    let path = Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("expert staging path must contain only normal relative components".to_string());
    }
    Ok(path.to_path_buf())
}

pub fn stage_expert_output(
    staging_base: &Path,
    contract: &ExpertAttemptContract,
    relative_path: &str,
    content: &str,
) -> Result<ExpertStagingManifest, String> {
    let relative_path = safe_staging_relative_path(relative_path)?;
    let bytes = u64::try_from(content.len()).map_err(|_| "staged output is too large")?;
    if bytes == 0 || bytes > contract.budget.max_staged_bytes {
        return Err("expert staged output exceeds its immutable budget".to_string());
    }
    let root = staging_base
        .join(contract.parent_run_id.to_string())
        .join(format!("{}-{}", contract.key, contract.attempt));
    fs::create_dir_all(staging_base)
        .map_err(|error| format!("create expert staging base: {error}"))?;
    reject_reparse_components(staging_base, staging_base)?;
    let parent_root = staging_base.join(contract.parent_run_id.to_string());
    ensure_safe_directory(staging_base, &parent_root)?;
    ensure_safe_directory(staging_base, &root)?;
    reject_reparse_components(staging_base, &root)?;
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("canonicalize expert staging root: {error}"))?;
    let target = canonical_root.join(&relative_path);
    if let Some(parent) = target.parent() {
        ensure_safe_directory(&canonical_root, parent)?;
        reject_reparse_components(&canonical_root, parent)?;
        let canonical_parent = parent
            .canonicalize()
            .map_err(|error| format!("canonicalize expert staging directory: {error}"))?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err("expert staging path escaped its run-scoped root".to_string());
        }
    }
    if target.exists() {
        let metadata = fs::symlink_metadata(&target)
            .map_err(|error| format!("inspect expert staging target: {error}"))?;
        if is_reparse_or_symlink(&metadata) || !metadata.is_file() {
            return Err("expert staging target is not a regular file".to_string());
        }
    }
    fs::write(&target, content.as_bytes())
        .map_err(|error| format!("write expert staged output: {error}"))?;
    verify_staging_manifest(
        &canonical_root,
        &ExpertStagingManifest {
            relative_path: relative_path.to_string_lossy().replace('\\', "/"),
            absolute_path: target.to_string_lossy().to_string(),
            sha256: sha256_text(content),
            bytes,
        },
    )
}

fn ensure_safe_directory(base: &Path, target: &Path) -> Result<(), String> {
    let relative = target
        .strip_prefix(base)
        .map_err(|_| "expert staging directory escaped its base")?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err("expert staging directory contains an unsafe component".to_string());
        }
        current.push(component.as_os_str());
        if current.exists() {
            let metadata = fs::symlink_metadata(&current)
                .map_err(|error| format!("inspect expert staging directory: {error}"))?;
            if is_reparse_or_symlink(&metadata) || !metadata.is_dir() {
                return Err(
                    "expert staging directory cannot traverse a file, symlink, or junction"
                        .to_string(),
                );
            }
        } else {
            fs::create_dir(&current)
                .map_err(|error| format!("create expert staging directory: {error}"))?;
        }
    }
    Ok(())
}

pub fn verify_staging_manifest(
    expected_root: &Path,
    manifest: &ExpertStagingManifest,
) -> Result<ExpertStagingManifest, String> {
    let relative = safe_staging_relative_path(&manifest.relative_path)?;
    let canonical_root = expected_root
        .canonicalize()
        .map_err(|error| format!("canonicalize expert staging verification root: {error}"))?;
    reject_reparse_components(&canonical_root, &canonical_root.join(&relative))?;
    let expected_path = canonical_root.join(relative);
    let recorded_path = PathBuf::from(&manifest.absolute_path);
    if expected_path != recorded_path {
        return Err("expert staging manifest path does not match its run-scoped root".to_string());
    }
    let canonical_target = expected_path
        .canonicalize()
        .map_err(|error| format!("canonicalize expert staged output: {error}"))?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err("expert staged output escaped its run-scoped root".to_string());
    }
    let metadata = fs::symlink_metadata(&canonical_target)
        .map_err(|error| format!("inspect expert staged output: {error}"))?;
    if is_reparse_or_symlink(&metadata) || !metadata.is_file() {
        return Err("expert staged output is not a regular file".to_string());
    }
    let contents = fs::read(&canonical_target)
        .map_err(|error| format!("read expert staged output: {error}"))?;
    let bytes = u64::try_from(contents.len()).map_err(|_| "staged output is too large")?;
    let sha256 = hex::encode(Sha256::digest(&contents));
    if bytes != manifest.bytes || sha256 != manifest.sha256 {
        return Err("expert staged output changed after its revision was recorded".to_string());
    }
    Ok(manifest.clone())
}

fn reject_reparse_components(base: &Path, target: &Path) -> Result<(), String> {
    let mut current = base.to_path_buf();
    if current.exists() {
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("inspect expert staging base: {error}"))?;
        if is_reparse_or_symlink(&metadata) {
            return Err("expert staging base cannot be a symlink or junction".to_string());
        }
    }
    if let Ok(relative) = target.strip_prefix(base) {
        for component in relative.components() {
            current.push(component.as_os_str());
            if current.exists() {
                let metadata = fs::symlink_metadata(&current)
                    .map_err(|error| format!("inspect expert staging component: {error}"))?;
                if is_reparse_or_symlink(&metadata) {
                    return Err(
                        "expert staging path cannot traverse a symlink or junction".to_string()
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_or_symlink(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn validate_key(value: &str, field: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 64
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(format!(
            "{field} must use 1-64 ASCII letters, numbers, hyphens, or underscores"
        ));
    }
    Ok(())
}

fn validate_text(value: &str, field: &str, max_chars: usize) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_chars {
        return Err(format!(
            "{field} is empty or exceeds {max_chars} characters"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, role: ExpertRole, depends_on: &[&str]) -> ExpertTeamPlanItem {
        let production = role == ExpertRole::Production;
        ExpertTeamPlanItem {
            key: key.to_string(),
            role,
            prompt: format!("Perform the {key} work with explicit evidence."),
            depends_on: depends_on.iter().map(|value| value.to_string()).collect(),
            capabilities: if production {
                vec![
                    ExpertCapability::FileRead,
                    ExpertCapability::ManagedStagingWrite,
                ]
            } else {
                vec![ExpertCapability::FileRead]
            },
            resources: vec![ExpertResourceRequirement {
                key: if production { "draft" } else { "evidence" }.to_string(),
                access: if production {
                    ExpertResourceAccess::Write
                } else {
                    ExpertResourceAccess::Read
                },
            }],
            budget: ExpertBudget::default(),
            output_contract: ExpertOutputContract {
                require_staged_output: production,
                require_review: role == ExpertRole::Review,
                ..ExpertOutputContract::default()
            },
            retry_policy: ExpertRetryPolicy::default(),
        }
    }

    #[test]
    fn validates_four_role_acyclic_team_and_binds_parent_revision() {
        let parent_id = Uuid::new_v4();
        let plan = vec![
            item("research", ExpertRole::Research, &[]),
            item("analysis", ExpertRole::Analysis, &["research"]),
            item("draft", ExpertRole::Production, &["analysis"]),
            item("review", ExpertRole::Review, &["draft"]),
        ];
        let contracts = validate_team_plan(parent_id, "Original goal", &plan).expect("valid plan");
        assert_eq!(contracts.len(), 4);
        assert_eq!(
            contracts[0].parent_input_revision,
            parent_input_revision("Original goal")
        );
        assert!(contracts
            .iter()
            .all(|contract| contract.team_id == contracts[0].team_id));
    }

    #[test]
    fn rejects_cycles_duplicate_roles_and_non_production_writes() {
        let parent_id = Uuid::new_v4();
        let cyclic = vec![
            item("research", ExpertRole::Research, &["analysis"]),
            item("analysis", ExpertRole::Analysis, &["research"]),
        ];
        assert!(validate_team_plan(parent_id, "goal", &cyclic)
            .unwrap_err()
            .contains("cycle"));

        let duplicate_role = vec![
            item("one", ExpertRole::Research, &[]),
            item("two", ExpertRole::Research, &[]),
        ];
        assert!(validate_team_plan(parent_id, "goal", &duplicate_role)
            .unwrap_err()
            .contains("unique"));

        let mut invalid_write = item("analysis", ExpertRole::Analysis, &[]);
        invalid_write.resources[0].access = ExpertResourceAccess::Write;
        assert!(validate_team_plan(
            parent_id,
            "goal",
            &[item("research", ExpertRole::Research, &[]), invalid_write]
        )
        .unwrap_err()
        .contains("only production"));
    }

    #[test]
    fn deduplicates_evidence_and_surfaces_claim_conflicts() {
        let evidence = deduplicate_evidence(vec![
            ExpertEvidenceRef {
                id: String::new(),
                kind: "url".to_string(),
                reference: "https://example.com/a".to_string(),
                summary: "first".to_string(),
                verified: false,
            },
            ExpertEvidenceRef {
                id: String::new(),
                kind: "URL".to_string(),
                reference: "https://example.com/a".to_string(),
                summary: "duplicate".to_string(),
                verified: true,
            },
        ]);
        assert_eq!(evidence.len(), 1);
        assert!(evidence[0].verified);

        let claims = vec![
            ExpertClaim {
                key: "occupancy".to_string(),
                statement: "Occupancy improved".to_string(),
                stance: ExpertClaimStance::Supports,
                evidence_refs: vec![evidence[0].id.clone()],
            },
            ExpertClaim {
                key: "occupancy".to_string(),
                statement: "Comparable occupancy declined".to_string(),
                stance: ExpertClaimStance::Contradicts,
                evidence_refs: vec![evidence[0].id.clone()],
            },
        ];
        assert_eq!(unresolved_claim_conflicts(&claims), vec!["occupancy"]);
    }

    #[test]
    fn resource_write_conflicts_but_parallel_reads_do_not() {
        let read = vec![ExpertResourceRequirement {
            key: "source-a".to_string(),
            access: ExpertResourceAccess::Read,
        }];
        let write = vec![ExpertResourceRequirement {
            key: "source-a".to_string(),
            access: ExpertResourceAccess::Write,
        }];
        assert!(!resources_conflict(&read, &read));
        assert!(resources_conflict(&read, &write));
    }

    #[test]
    fn staging_rejects_escape_and_detects_tampering() {
        assert!(safe_staging_relative_path("../escape.txt").is_err());
        assert!(safe_staging_relative_path("C:\\escape.txt").is_err());
        let base = tempfile::tempdir().expect("tempdir");
        let contract = validate_team_plan(
            Uuid::new_v4(),
            "goal",
            &[
                item("research", ExpertRole::Research, &[]),
                item("draft", ExpertRole::Production, &["research"]),
            ],
        )
        .expect("plan")
        .remove(1);
        let manifest = stage_expert_output(base.path(), &contract, "draft.md", "safe draft")
            .expect("stage succeeds");
        let root = base
            .path()
            .join(contract.parent_run_id.to_string())
            .join(format!("{}-{}", contract.key, contract.attempt));
        verify_staging_manifest(&root, &manifest).expect("manifest verifies");
        fs::write(root.join("not-a-directory"), b"file").expect("blocking file fixture");
        assert!(stage_expert_output(
            base.path(),
            &contract,
            "not-a-directory/escape.md",
            "must not be written"
        )
        .unwrap_err()
        .contains("cannot traverse"));
        fs::write(&manifest.absolute_path, b"tampered").expect("tamper fixture");
        assert!(verify_staging_manifest(&root, &manifest)
            .unwrap_err()
            .contains("changed"));
    }

    #[test]
    fn staging_rejects_existing_symlink_or_junction_before_child_write() {
        let base = tempfile::tempdir().expect("base tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let contract = validate_team_plan(
            Uuid::new_v4(),
            "goal",
            &[
                item("research", ExpertRole::Research, &[]),
                item("draft", ExpertRole::Production, &["research"]),
            ],
        )
        .expect("plan")
        .remove(1);
        let root = base
            .path()
            .join(contract.parent_run_id.to_string())
            .join(format!("{}-{}", contract.key, contract.attempt));
        fs::create_dir_all(&root).expect("root fixture");
        let link = root.join("linked");
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(outside.path(), &link).is_ok();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(outside.path(), &link).is_ok();
        if !linked {
            return;
        }
        assert!(stage_expert_output(
            base.path(),
            &contract,
            "linked/escape.md",
            "must stay contained"
        )
        .is_err());
        assert!(!outside.path().join("escape.md").exists());
    }
}
