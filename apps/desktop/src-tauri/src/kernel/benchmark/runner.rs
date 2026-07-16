use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::kernel::models::AccessMode;

use super::t1::baseline;
use super::{
    aggregate_benchmark_runs, classify_benchmark_run, BenchmarkAggregate, BenchmarkOutcomeClass,
    BenchmarkRunResult, BenchmarkTaskSpec, BenchmarkVerifierStatus,
};

pub const STEP0_BASELINE_REPORT_VERSION: &str = "ds-agent.step-0-t1-baseline/v1";
pub const STEP0_RUNNER_VERSION: &str = "ds-agent.step-0-offline-runner/v1";
pub const JSON_REPORT_FILE: &str = "t1-baseline-v1.json";
pub const MARKDOWN_REPORT_FILE: &str = "t1-baseline-v1.md";
const RUN_COUNT: u32 = 5;

#[derive(Clone, Debug)]
pub struct BenchmarkRunnerConfig {
    pub workspace_root: PathBuf,
    pub fixture_root: PathBuf,
    pub output_root: PathBuf,
    pub report_root: PathBuf,
    pub app_version: String,
    pub source_commit: String,
    pub release_tag: Option<String>,
    pub environment_profile: String,
    pub access_mode: AccessMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineCompletionDecision {
    Completed,
    FailedClosed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineEnvironment {
    pub runner_version: String,
    pub app_version: String,
    pub source_commit: String,
    pub release_tag: Option<String>,
    pub source_state: String,
    pub operating_system: String,
    pub architecture: String,
    pub environment_profile: String,
    pub access_mode: AccessMode,
    pub fixture_mode: String,
    pub renderer_mode: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineFixtureManifestBinding {
    pub fixture_set_id: String,
    pub generator_id: String,
    pub checked_manifest_sha256: String,
    pub generated_fixture_count_per_run: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineClassification {
    pub outcome_class: BenchmarkOutcomeClass,
    pub verifier_gate_passed: bool,
    pub false_completion: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineOfflineUsage {
    pub api_call_count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub estimated_cost_micro_usd: u64,
    pub availability: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineRunMetrics {
    pub outcome_class: BenchmarkOutcomeClass,
    pub clarification_count: u32,
    pub authorization_count: u32,
    pub human_intervention_count: u32,
    pub logical_duration_ms: u64,
    pub evidence_receipt_count: u32,
    pub failure_stage: Option<String>,
    pub offline_usage: BaselineOfflineUsage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineArtifactHash {
    pub kind: String,
    pub relative_ref: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineRunRecord {
    pub run_id: String,
    pub repetition_index: u32,
    pub clean_state_id: String,
    pub clean_root_binding: String,
    pub task_contract: BenchmarkTaskSpec,
    pub fixture_manifest_sha256: String,
    pub run_result: BenchmarkRunResult,
    pub classification: BaselineClassification,
    pub completion_decision: BaselineCompletionDecision,
    pub metrics: BaselineRunMetrics,
    pub artifact_hashes: Vec<BaselineArtifactHash>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineAggregateRecord {
    pub c0b_metrics: BenchmarkAggregate,
    pub completed_run_count: u32,
    pub required_verifier_pass_count: u32,
    pub required_verifier_total: u32,
    pub evidence_receipt_count: u32,
    pub total_human_intervention_count: u32,
    pub offline_usage_total: BaselineOfflineUsage,
    pub completion_decision: BaselineCompletionDecision,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineCanonicalReport {
    pub version: String,
    pub logical_snapshot_at: String,
    pub environment: BaselineEnvironment,
    pub fixture_manifest: BaselineFixtureManifestBinding,
    pub task_spec: BenchmarkTaskSpec,
    pub task_spec_fingerprint: String,
    pub runs: Vec<BaselineRunRecord>,
    pub aggregate: BaselineAggregateRecord,
    pub failures: Vec<String>,
    pub ignored_by_scope: Vec<String>,
    pub scope_disclosure: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineWallClockObservation {
    pub run_id: String,
    pub wall_clock_ms: f64,
    pub normative: bool,
    pub note: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineObservationSummary {
    pub observation_count: u32,
    pub total_wall_clock_ms: f64,
    pub median_wall_clock_ms: Option<f64>,
    pub p95_wall_clock_ms: Option<f64>,
    pub normative: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineObservations {
    pub runs: Vec<BaselineWallClockObservation>,
    pub summary: BaselineObservationSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineReportIntegrity {
    pub canonical_payload_algorithm: String,
    pub canonical_payload_sha256: String,
    pub markdown_body_algorithm: String,
    pub markdown_body_sha256: String,
    pub json_report_name: String,
    pub markdown_report_name: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step0BaselineReport {
    pub canonical: BaselineCanonicalReport,
    pub observations: BaselineObservations,
    pub integrity: BaselineReportIntegrity,
}

impl BaselineOfflineUsage {
    pub(super) fn none() -> Self {
        Self {
            api_call_count: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_micro_usd: 0,
            availability: "none".to_string(),
            reason: "Offline deterministic runner made no model or provider call".to_string(),
        }
    }

    fn is_offline_zero(&self) -> bool {
        self.api_call_count == 0
            && self.prompt_tokens == 0
            && self.completion_tokens == 0
            && self.total_tokens == 0
            && self.estimated_cost_micro_usd == 0
            && self.availability == "none"
    }
}

impl Step0BaselineReport {
    pub fn canonical_json(&self) -> Result<String, String> {
        canonical_json(&self.canonical)
    }

    pub fn validate(&self) -> Result<(), String> {
        let canonical = &self.canonical;
        if canonical.version != STEP0_BASELINE_REPORT_VERSION {
            return Err("unsupported Step 0 baseline report version".to_string());
        }
        canonical.task_spec.validate()?;
        if canonical.task_spec_fingerprint != canonical.task_spec.fingerprint()? {
            return Err("Step 0 report task fingerprint mismatch".to_string());
        }
        if canonical.fixture_manifest.fixture_set_id != canonical.task_spec.fixture_set_id
            || canonical.fixture_manifest.generated_fixture_count_per_run
                != canonical.task_spec.fixtures.len() as u32
            || !is_sha256(&canonical.fixture_manifest.checked_manifest_sha256)
        {
            return Err("Step 0 fixture manifest binding is invalid".to_string());
        }
        if canonical.runs.len() != RUN_COUNT as usize {
            return Err("Step 0 baseline requires exactly five runs".to_string());
        }

        let mut run_ids = BTreeSet::new();
        let mut clean_ids = BTreeSet::new();
        let mut clean_bindings = BTreeSet::new();
        let mut verifier_pass_count = 0_u32;
        let mut evidence_count = 0_u32;
        let mut human_count = 0_u32;
        for (offset, run) in canonical.runs.iter().enumerate() {
            let expected_index = offset as u32 + 1;
            if run.repetition_index != expected_index
                || run.run_result.repetition_index != expected_index
                || run.run_id != format!("t1-run-{expected_index:04}")
                || run.clean_state_id != format!("clean-state-{expected_index:02}")
                || run.clean_root_binding != format!("run-{expected_index:02}")
            {
                return Err("Step 0 run order or clean-state binding is invalid".to_string());
            }
            if !run_ids.insert(run.run_id.as_str())
                || !clean_ids.insert(run.clean_state_id.as_str())
                || !clean_bindings.insert(run.clean_root_binding.as_str())
            {
                return Err("Step 0 run or clean root was reused".to_string());
            }
            if run.task_contract != canonical.task_spec
                || run.fixture_manifest_sha256 != canonical.fixture_manifest.checked_manifest_sha256
                || run.run_result.run_id != run.run_id
                || run.run_result.subject.clean_state_id != run.clean_state_id
            {
                return Err("Step 0 per-run contract binding drifted".to_string());
            }
            run.run_result.validate(&canonical.task_spec)?;
            let classification = classify_benchmark_run(&canonical.task_spec, &run.run_result)?;
            if run.classification.outcome_class != classification.outcome_class
                || run.classification.verifier_gate_passed != classification.verifier_gate_passed
                || run.classification.false_completion != classification.false_completion
            {
                return Err("Step 0 stored classification is not recomputable".to_string());
            }
            let completed = classification.verifier_gate_passed
                && !classification.false_completion
                && classification.outcome_class == BenchmarkOutcomeClass::A
                && run
                    .run_result
                    .verifier_results
                    .iter()
                    .all(|result| result.status == BenchmarkVerifierStatus::Passed);
            if run.completion_decision
                != if completed {
                    BaselineCompletionDecision::Completed
                } else {
                    BaselineCompletionDecision::FailedClosed
                }
                || !completed
            {
                return Err("Step 0 run completion did not fail closed".to_string());
            }
            let mut verifier_ids = BTreeSet::new();
            let mut evidence_refs = BTreeSet::new();
            for result in &run.run_result.verifier_results {
                if !verifier_ids.insert(result.verifier_id.as_str()) {
                    return Err("Step 0 run contains a duplicate verifier".to_string());
                }
                verifier_pass_count = verifier_pass_count
                    .checked_add(1)
                    .ok_or_else(|| "verifier count overflow".to_string())?;
                for evidence in &result.evidence {
                    if !evidence_refs.insert(evidence.relative_or_opaque_ref.as_str()) {
                        return Err("Step 0 run contains duplicate evidence".to_string());
                    }
                    evidence_count = evidence_count
                        .checked_add(1)
                        .ok_or_else(|| "evidence count overflow".to_string())?;
                }
            }
            if verifier_ids.len() != canonical.task_spec.done_when.len() {
                return Err("Step 0 verifier set is missing or unexpected".to_string());
            }
            let expected_human = run.run_result.interactions.manual_interventions.len() as u32;
            if run.metrics.outcome_class != classification.outcome_class
                || run.metrics.clarification_count
                    != run.run_result.interactions.clarification_count
                || run.metrics.authorization_count
                    != run.run_result.interactions.authorization_count
                || run.metrics.human_intervention_count != expected_human
                || run.metrics.logical_duration_ms != run.run_result.elapsed_ms
                || run.metrics.evidence_receipt_count
                    != run
                        .run_result
                        .verifier_results
                        .iter()
                        .map(|result| result.evidence.len() as u32)
                        .sum::<u32>()
                || run.metrics.failure_stage != run.run_result.failure_stage
                || !run.metrics.offline_usage.is_offline_zero()
                || !run.run_result.deepseek_usage.is_empty()
            {
                return Err("Step 0 per-run metrics are incomplete or inconsistent".to_string());
            }
            human_count = human_count
                .checked_add(expected_human)
                .ok_or_else(|| "human intervention count overflow".to_string())?;
            validate_artifact_hashes(&run.artifact_hashes)?;
        }

        let pairs = canonical
            .runs
            .iter()
            .map(|run| (&canonical.task_spec, &run.run_result))
            .collect::<Vec<_>>();
        let recomputed = aggregate_benchmark_runs(&pairs)?;
        let aggregate = &canonical.aggregate;
        if aggregate.c0b_metrics != recomputed
            || aggregate.completed_run_count != RUN_COUNT
            || aggregate.required_verifier_pass_count != verifier_pass_count
            || aggregate.required_verifier_total
                != RUN_COUNT * canonical.task_spec.done_when.len() as u32
            || aggregate.evidence_receipt_count != evidence_count
            || aggregate.total_human_intervention_count != human_count
            || !aggregate.offline_usage_total.is_offline_zero()
            || aggregate.completion_decision != BaselineCompletionDecision::Completed
            || !canonical.failures.is_empty()
        {
            return Err("Step 0 aggregate metrics or completion decision are invalid".to_string());
        }
        if aggregate.c0b_metrics.outcomes.a != u64::from(RUN_COUNT)
            || aggregate.c0b_metrics.outcomes.q != 0
            || aggregate.c0b_metrics.outcomes.h != 0
            || aggregate.c0b_metrics.outcomes.s != 0
            || aggregate.c0b_metrics.outcomes.f != 0
            || aggregate.c0b_metrics.vocr.numerator != u64::from(RUN_COUNT)
            || aggregate.c0b_metrics.vocr.denominator != u64::from(RUN_COUNT)
            || aggregate.c0b_metrics.false_completion_rate.numerator != 0
        {
            return Err("Step 0 VOCR or false-completion result is invalid".to_string());
        }

        validate_observations(&self.observations, &run_ids)?;
        let canonical_hash = sha256(self.canonical_json()?.as_bytes());
        if self.integrity.canonical_payload_algorithm != "sha256"
            || self.integrity.canonical_payload_sha256 != canonical_hash
            || self.integrity.markdown_body_algorithm != "sha256"
            || self.integrity.json_report_name != JSON_REPORT_FILE
            || self.integrity.markdown_report_name != MARKDOWN_REPORT_FILE
        {
            return Err("Step 0 report integrity binding is invalid".to_string());
        }
        let markdown_body = render_markdown_body(self)?;
        if self.integrity.markdown_body_sha256 != sha256(markdown_body.as_bytes()) {
            return Err("Step 0 Markdown body hash mismatch".to_string());
        }
        validate_secret_safe_report(self)?;
        Ok(())
    }
}

pub fn run_t1_step0_baseline(
    config: &BenchmarkRunnerConfig,
) -> Result<Step0BaselineReport, String> {
    validate_config(config, true)?;
    for root in config_roots(config) {
        fs::create_dir_all(root)
            .map_err(|error| format!("create fresh benchmark root: {error}"))?;
        ensure_empty_directory(root)?;
    }

    let task_spec = super::t1::task_spec()?;
    let fixture_manifest_sha256 = baseline::checked_fixture_manifest_sha256();
    let mut runs = Vec::with_capacity(RUN_COUNT as usize);
    let mut observations = Vec::with_capacity(RUN_COUNT as usize);
    for index in 1..=RUN_COUNT {
        let run_dir = format!("run-{index:02}");
        let workspace_run_root = config.workspace_root.join(&run_dir);
        let fixture_run_root = config.fixture_root.join(&run_dir);
        let output_run_root = config.output_root.join(&run_dir);
        for root in [&workspace_run_root, &fixture_run_root, &output_run_root] {
            create_fresh_run_root(root)?;
        }
        let (run, observation) = baseline::execute_t1_run(
            config,
            &task_spec,
            &fixture_manifest_sha256,
            index,
            &workspace_run_root,
            &fixture_run_root,
            &output_run_root,
        )?;
        runs.push(run);
        observations.push(observation);
    }

    validate_runtime_file_scope(config, &task_spec, false)?;
    let c0b_metrics = aggregate_benchmark_runs(
        &runs
            .iter()
            .map(|run| (&task_spec, &run.run_result))
            .collect::<Vec<_>>(),
    )?;
    let evidence_receipt_count = runs
        .iter()
        .flat_map(|run| &run.run_result.verifier_results)
        .map(|result| result.evidence.len() as u32)
        .sum();
    let canonical = BaselineCanonicalReport {
        version: STEP0_BASELINE_REPORT_VERSION.to_string(),
        logical_snapshot_at: "2026-07-16T00:00:00Z".to_string(),
        environment: BaselineEnvironment {
            runner_version: STEP0_RUNNER_VERSION.to_string(),
            app_version: config.app_version.clone(),
            source_commit: config.source_commit.clone(),
            release_tag: config.release_tag.clone(),
            source_state: "c0d-working-tree-based-on-source-commit".to_string(),
            operating_system: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
            environment_profile: config.environment_profile.clone(),
            access_mode: config.access_mode,
            fixture_mode: "synthetic-deterministic-offline".to_string(),
            renderer_mode: "deterministic-receipt-fixture-no-office-or-poppler".to_string(),
        },
        fixture_manifest: BaselineFixtureManifestBinding {
            fixture_set_id: task_spec.fixture_set_id.clone(),
            generator_id: super::t1::FIXTURE_GENERATOR_ID.to_string(),
            checked_manifest_sha256: fixture_manifest_sha256,
            generated_fixture_count_per_run: task_spec.fixtures.len() as u32,
        },
        task_spec: task_spec.clone(),
        task_spec_fingerprint: task_spec.fingerprint()?,
        aggregate: BaselineAggregateRecord {
            c0b_metrics,
            completed_run_count: runs.len() as u32,
            required_verifier_pass_count: runs
                .iter()
                .flat_map(|run| &run.run_result.verifier_results)
                .filter(|result| result.status == BenchmarkVerifierStatus::Passed)
                .count() as u32,
            required_verifier_total: runs.len() as u32 * task_spec.done_when.len() as u32,
            evidence_receipt_count,
            total_human_intervention_count: runs
                .iter()
                .map(|run| run.metrics.human_intervention_count)
                .sum(),
            offline_usage_total: BaselineOfflineUsage::none(),
            completion_decision: BaselineCompletionDecision::Completed,
        },
        runs,
        failures: Vec::new(),
        ignored_by_scope: vec![
            "Live DeepSeek and token or price telemetry were excluded; all usage and cost values are explicit zero with availability none".to_string(),
            "Installed DS Agent, real Office, Poppler, real accounts, VM, connectors, and external writes were excluded by C0D scope".to_string(),
            "Observed wall-clock timing is non-normative and excluded from canonical hashing and PASS decisions".to_string(),
        ],
        scope_disclosure: "Step 0 only: five offline synthetic T1 runs used fresh explicit roots. Files alone never imply completion; receipts and all six deterministic verifiers jointly gate completion. No later product capability was started or claimed".to_string(),
    };
    let observations = BaselineObservations {
        summary: summarize_observations(&observations)?,
        runs: observations,
    };
    let canonical_hash = sha256(canonical_json(&canonical)?.as_bytes());
    let report = Step0BaselineReport {
        canonical,
        observations,
        integrity: BaselineReportIntegrity {
            canonical_payload_algorithm: "sha256".to_string(),
            canonical_payload_sha256: canonical_hash,
            markdown_body_algorithm: "sha256".to_string(),
            markdown_body_sha256: String::new(),
            json_report_name: JSON_REPORT_FILE.to_string(),
            markdown_report_name: MARKDOWN_REPORT_FILE.to_string(),
        },
    };
    let normalized = serde_json::to_string(&report)
        .map_err(|error| format!("normalize Step 0 report: {error}"))?;
    let mut report: Step0BaselineReport = serde_json::from_str(&normalized)
        .map_err(|error| format!("read normalized Step 0 report: {error}"))?;
    report.integrity.markdown_body_sha256 = sha256(render_markdown_body(&report)?.as_bytes());
    report.validate()?;
    write_reports(config, &report)?;
    validate_runtime_file_scope(config, &task_spec, true)?;
    let readback = readback_t1_step0_baseline(config)?;
    if !reports_match(&readback, &report) {
        return Err("Step 0 report changed during disk readback".to_string());
    }
    Ok(report)
}

pub fn readback_t1_step0_baseline(
    config: &BenchmarkRunnerConfig,
) -> Result<Step0BaselineReport, String> {
    validate_config(config, false)?;
    let task_spec = super::t1::task_spec()?;
    validate_runtime_file_scope(config, &task_spec, true)?;
    let json = fs::read_to_string(config.report_root.join(JSON_REPORT_FILE))
        .map_err(|error| format!("read Step 0 JSON report: {error}"))?;
    let report: Step0BaselineReport = serde_json::from_str(&json)
        .map_err(|error| format!("parse Step 0 JSON report: {error}"))?;
    report.validate()?;
    let markdown = fs::read_to_string(config.report_root.join(MARKDOWN_REPORT_FILE))
        .map_err(|error| format!("read Step 0 Markdown report: {error}"))?;
    if markdown != render_markdown(&report)? {
        return Err("Step 0 Markdown readback does not match the JSON report".to_string());
    }
    for (offset, expected) in report.canonical.runs.iter().enumerate() {
        let index = offset as u32 + 1;
        let run_dir = format!("run-{index:02}");
        let actual = baseline::readback_t1_run(
            config,
            &task_spec,
            &report.canonical.fixture_manifest.checked_manifest_sha256,
            index,
            &config.workspace_root.join(&run_dir),
            &config.fixture_root.join(&run_dir),
            &config.output_root.join(&run_dir),
        )?;
        if &actual != expected {
            return Err(format!("Step 0 run {index} changed during disk readback"));
        }
    }
    Ok(report)
}

fn validate_config(config: &BenchmarkRunnerConfig, require_fresh: bool) -> Result<(), String> {
    let roots = config_roots(config);
    for (index, root) in roots.iter().enumerate() {
        if !root.is_absolute()
            || root
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
        {
            return Err(format!(
                "benchmark root {index} must be an absolute normalized path"
            ));
        }
        if require_fresh && root.exists() {
            return Err(format!("benchmark root {index} is not clean"));
        }
        if !require_fresh
            && (!root.is_dir()
                || fs::symlink_metadata(root)
                    .map_err(|error| error.to_string())?
                    .file_type()
                    .is_symlink())
        {
            return Err(format!("benchmark root {index} is missing or unsafe"));
        }
    }
    for (index, left) in roots.iter().enumerate() {
        for right in roots.iter().skip(index + 1) {
            if left.starts_with(right) || right.starts_with(left) {
                return Err("benchmark roots must be pairwise disjoint".to_string());
            }
        }
    }
    if config.app_version.is_empty()
        || config.environment_profile.is_empty()
        || config.source_commit.len() != 40
        || !config
            .source_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("benchmark version or source identity is invalid".to_string());
    }
    Ok(())
}

fn config_roots(config: &BenchmarkRunnerConfig) -> [&Path; 4] {
    [
        &config.workspace_root,
        &config.fixture_root,
        &config.output_root,
        &config.report_root,
    ]
}

fn create_fresh_run_root(root: &Path) -> Result<(), String> {
    if root.exists() {
        return Err("benchmark run root is not clean".to_string());
    }
    fs::create_dir(root).map_err(|error| format!("create benchmark run root: {error}"))?;
    ensure_empty_directory(root)
}

fn ensure_empty_directory(root: &Path) -> Result<(), String> {
    if fs::symlink_metadata(root)
        .map_err(|error| format!("inspect benchmark root: {error}"))?
        .file_type()
        .is_symlink()
    {
        return Err("benchmark root symlink is blocked".to_string());
    }
    if fs::read_dir(root)
        .map_err(|error| format!("read benchmark root: {error}"))?
        .next()
        .is_some()
    {
        return Err("benchmark root is not empty".to_string());
    }
    Ok(())
}

fn validate_runtime_file_scope(
    config: &BenchmarkRunnerConfig,
    task_spec: &BenchmarkTaskSpec,
    include_reports: bool,
) -> Result<(), String> {
    let mut workspace = BTreeSet::new();
    let mut fixtures = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    for index in 1..=RUN_COUNT {
        let run_dir = format!("run-{index:02}");
        workspace.insert(format!("{run_dir}/{}", baseline::RUN_BINDING_FILE));
        for fixture in &task_spec.fixtures {
            fixtures.insert(format!("{run_dir}/{}", fixture.relative_path));
        }
        for output in baseline::expected_output_files() {
            outputs.insert(format!("{run_dir}/{output}"));
        }
    }
    ensure_exact_files(&config.workspace_root, &workspace)?;
    ensure_exact_files(&config.fixture_root, &fixtures)?;
    ensure_exact_files(&config.output_root, &outputs)?;
    let reports = if include_reports {
        BTreeSet::from([
            JSON_REPORT_FILE.to_string(),
            MARKDOWN_REPORT_FILE.to_string(),
        ])
    } else {
        BTreeSet::new()
    };
    ensure_exact_files(&config.report_root, &reports)
}

fn ensure_exact_files(root: &Path, expected: &BTreeSet<String>) -> Result<(), String> {
    let mut actual = BTreeSet::new();
    collect_files(root, root, &mut actual)?;
    if &actual != expected {
        return Err("benchmark output scope contains missing or unexpected files".to_string());
    }
    Ok(())
}

fn collect_files(root: &Path, current: &Path, output: &mut BTreeSet<String>) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|error| format!("read benchmark scope: {error}"))? {
        let entry = entry.map_err(|error| format!("read benchmark scope entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("inspect benchmark scope entry: {error}"))?;
        if file_type.is_symlink() {
            return Err("benchmark scope symlink is blocked".to_string());
        }
        if file_type.is_dir() {
            collect_files(root, &entry.path(), output)?;
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| "benchmark file escaped its root".to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if !output.insert(relative) {
                return Err("benchmark scope contains a duplicate file".to_string());
            }
        } else {
            return Err("benchmark scope contains an unsupported entry".to_string());
        }
    }
    Ok(())
}

fn write_reports(
    config: &BenchmarkRunnerConfig,
    report: &Step0BaselineReport,
) -> Result<(), String> {
    ensure_empty_directory(&config.report_root)?;
    let mut json = serde_json::to_string_pretty(report)
        .map_err(|error| format!("serialize Step 0 JSON report: {error}"))?;
    json.push('\n');
    write_new(&config.report_root.join(JSON_REPORT_FILE), json.as_bytes())?;
    let markdown = render_markdown(report)?;
    write_new(
        &config.report_root.join(MARKDOWN_REPORT_FILE),
        markdown.as_bytes(),
    )?;
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("create benchmark report: {error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("write benchmark report: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync benchmark report: {error}"))
}

fn validate_artifact_hashes(artifacts: &[BaselineArtifactHash]) -> Result<(), String> {
    if artifacts.is_empty() {
        return Err("Step 0 artifact hash list is empty".to_string());
    }
    let mut refs = BTreeSet::new();
    for artifact in artifacts {
        if artifact.kind.is_empty()
            || artifact.relative_ref.is_empty()
            || artifact.relative_ref.starts_with('/')
            || artifact.relative_ref.contains('\\')
            || artifact.relative_ref.contains(':')
            || artifact
                .relative_ref
                .split('/')
                .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
            || !is_sha256(&artifact.sha256)
            || !refs.insert(artifact.relative_ref.as_str())
        {
            return Err("Step 0 artifact hash binding is unsafe or duplicated".to_string());
        }
    }
    Ok(())
}

fn validate_observations(
    observations: &BaselineObservations,
    expected_run_ids: &BTreeSet<&str>,
) -> Result<(), String> {
    if observations.runs.len() != RUN_COUNT as usize
        || observations.summary.observation_count != RUN_COUNT
        || observations.summary.normative
    {
        return Err("Step 0 wall-clock observation shape is invalid".to_string());
    }
    let mut ids = BTreeSet::new();
    for observation in &observations.runs {
        if observation.normative
            || !observation.wall_clock_ms.is_finite()
            || observation.wall_clock_ms < 0.0
            || !expected_run_ids.contains(observation.run_id.as_str())
            || !ids.insert(observation.run_id.as_str())
        {
            return Err("Step 0 wall-clock observation is invalid or non-finite".to_string());
        }
    }
    let recomputed = summarize_observations(&observations.runs)?;
    if recomputed.observation_count != observations.summary.observation_count
        || recomputed.normative != observations.summary.normative
        || !float_matches(
            recomputed.total_wall_clock_ms,
            observations.summary.total_wall_clock_ms,
        )
        || !optional_float_matches(
            recomputed.median_wall_clock_ms,
            observations.summary.median_wall_clock_ms,
        )
        || !optional_float_matches(
            recomputed.p95_wall_clock_ms,
            observations.summary.p95_wall_clock_ms,
        )
    {
        return Err("Step 0 wall-clock statistics are inconsistent".to_string());
    }
    Ok(())
}

fn optional_float_matches(left: Option<f64>, right: Option<f64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => float_matches(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn float_matches(left: f64, right: f64) -> bool {
    left.is_finite()
        && right.is_finite()
        && (left - right).abs() <= 0.000_001_f64.max(left.abs().max(right.abs()) * 1e-12)
}

fn reports_match(left: &Step0BaselineReport, right: &Step0BaselineReport) -> bool {
    left.canonical == right.canonical
        && left.integrity == right.integrity
        && left.observations.runs.len() == right.observations.runs.len()
        && left
            .observations
            .runs
            .iter()
            .zip(&right.observations.runs)
            .all(|(left, right)| {
                left.run_id == right.run_id
                    && left.normative == right.normative
                    && left.note == right.note
                    && float_matches(left.wall_clock_ms, right.wall_clock_ms)
            })
        && left.observations.summary.observation_count
            == right.observations.summary.observation_count
        && left.observations.summary.normative == right.observations.summary.normative
        && float_matches(
            left.observations.summary.total_wall_clock_ms,
            right.observations.summary.total_wall_clock_ms,
        )
        && optional_float_matches(
            left.observations.summary.median_wall_clock_ms,
            right.observations.summary.median_wall_clock_ms,
        )
        && optional_float_matches(
            left.observations.summary.p95_wall_clock_ms,
            right.observations.summary.p95_wall_clock_ms,
        )
}

fn summarize_observations(
    observations: &[BaselineWallClockObservation],
) -> Result<BaselineObservationSummary, String> {
    let mut values = observations
        .iter()
        .map(|value| value.wall_clock_ms)
        .collect::<Vec<_>>();
    if values
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err("non-finite benchmark observation".to_string());
    }
    values.sort_by(f64::total_cmp);
    let total = values.iter().sum::<f64>();
    let median = if values.is_empty() {
        None
    } else if values.len() % 2 == 1 {
        Some(values[values.len() / 2])
    } else {
        Some((values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0)
    };
    let p95 = if values.is_empty() {
        None
    } else {
        let rank = (values.len() * 95).div_ceil(100);
        Some(values[rank.saturating_sub(1)])
    };
    Ok(BaselineObservationSummary {
        observation_count: observations.len() as u32,
        total_wall_clock_ms: total,
        median_wall_clock_ms: median,
        p95_wall_clock_ms: p95,
        normative: false,
    })
}

fn validate_secret_safe_report(report: &Step0BaselineReport) -> Result<(), String> {
    let value = serde_json::to_value(report)
        .map_err(|error| format!("serialize report for privacy validation: {error}"))?;
    validate_value_strings(&value)?;
    let markdown = render_markdown(report)?;
    validate_report_string(&markdown)
}

fn validate_value_strings(value: &Value) -> Result<(), String> {
    match value {
        Value::String(value) => validate_report_string(value),
        Value::Array(values) => values.iter().try_for_each(validate_value_strings),
        Value::Object(values) => values.values().try_for_each(validate_value_strings),
        _ => Ok(()),
    }
}

fn validate_report_string(value: &str) -> Result<(), String> {
    let lower = value.to_ascii_lowercase();
    let normalized = lower.replace([' ', '-', '.'], "_");
    let forbidden = [
        "bearer ",
        "api_key",
        "apikey",
        "provider_raw_body",
        "provider raw body",
        "chain_of_thought",
        "reasoning_content",
        "test_secret",
        "secret_marker",
        "production_data",
        "personal_data",
        "file://",
    ];
    let drive_path = value.as_bytes().windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'/' | b'\\')
    });
    let api_key_shape = lower.match_indices("sk-").any(|(index, _)| {
        lower[index + 3..]
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b'-'))
            .count()
            >= 12
    });
    if forbidden
        .iter()
        .any(|marker| lower.contains(marker) || normalized.contains(marker))
        || drive_path
        || value.contains("\\\\")
        || value.contains("../")
        || value.contains("..\\")
        || api_key_shape
    {
        return Err("Step 0 report contains a secret-like value or local path".to_string());
    }
    Ok(())
}

fn render_markdown(report: &Step0BaselineReport) -> Result<String, String> {
    let mut markdown = render_markdown_body(report)?;
    markdown.push_str("\n## Integrity\n\n");
    markdown.push_str(&format!(
        "- Canonical payload SHA-256: `{}`\n- Markdown body SHA-256: `{}`\n- Machine report: `{}`\n- Human report: `{}`\n",
        report.integrity.canonical_payload_sha256,
        report.integrity.markdown_body_sha256,
        report.integrity.json_report_name,
        report.integrity.markdown_report_name,
    ));
    Ok(markdown)
}

fn render_markdown_body(report: &Step0BaselineReport) -> Result<String, String> {
    let canonical = &report.canonical;
    let aggregate = &canonical.aggregate.c0b_metrics;
    let mut output = String::new();
    output.push_str("# Step 0 T1 Offline Baseline\n\n");
    output.push_str("Status: **COMPLETED** — five independent clean-state runs passed all six deterministic verifiers and the C0B completion gate.\n\n");
    output.push_str("## Scope and environment\n\n");
    output.push_str(&format!(
        "- Runner: `{}`\n- App version: `{}`\n- Source base commit: `{}`\n- Release tag: `{}`\n- Environment: `{}` / `{}` / `{}`\n- Fixture mode: `{}`\n- Renderer mode: `{}`\n- Scope disclosure: {}\n",
        canonical.environment.runner_version,
        canonical.environment.app_version,
        canonical.environment.source_commit,
        canonical.environment.release_tag.as_deref().unwrap_or("none"),
        canonical.environment.operating_system,
        canonical.environment.architecture,
        canonical.environment.environment_profile,
        canonical.environment.fixture_mode,
        canonical.environment.renderer_mode,
        canonical.scope_disclosure,
    ));
    output.push_str("\n## Contract binding\n\n");
    output.push_str(&format!(
        "- Task: `{}` revision {}\n- Prompt: {}\n- TaskSpec fingerprint: `{}`\n- Fixture set: `{}`\n- Fixture manifest SHA-256: `{}`\n- Allowed capabilities: `{}`\n- Expected risk: `{}`\n- Authorization budget: {}\n- Required done_when/verifiers: {}\n",
        canonical.task_spec.task_id,
        canonical.task_spec.task_revision,
        canonical.task_spec.prompt,
        canonical.task_spec_fingerprint,
        canonical.fixture_manifest.fixture_set_id,
        canonical.fixture_manifest.checked_manifest_sha256,
        serde_json::to_string(&canonical.task_spec.allowed_capabilities).map_err(|error| error.to_string())?,
        serde_json::to_string(&canonical.task_spec.expected_risk).map_err(|error| error.to_string())?,
        canonical.task_spec.authorization_budget,
        canonical.task_spec.done_when.len(),
    ));
    output.push_str("\n## Five-run result\n\n");
    output.push_str("| Run | Clean state | A/Q/H/S/F | Verifiers | Gate | Completion | Clarifications | Authorizations | Human interventions | Logical ms | Observed wall ms* | Tokens | Cost micro-USD | Failure stage |\n");
    output.push_str("| ---: | --- | --- | ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n");
    let observed = report
        .observations
        .runs
        .iter()
        .map(|item| (item.run_id.as_str(), item.wall_clock_ms))
        .collect::<BTreeMap<_, _>>();
    for run in &canonical.runs {
        let passed = run
            .run_result
            .verifier_results
            .iter()
            .filter(|result| result.status == BenchmarkVerifierStatus::Passed)
            .count();
        output.push_str(&format!(
            "| {} | `{}` | {} | {}/{} | {} | {} | {} | {} | {} | {} | {:.3} | {} | {} | {} |\n",
            run.repetition_index,
            run.clean_state_id,
            outcome_label(run.metrics.outcome_class),
            passed,
            run.run_result.verifier_results.len(),
            if run.classification.verifier_gate_passed {
                "PASS"
            } else {
                "FAIL"
            },
            completion_label(run.completion_decision),
            run.metrics.clarification_count,
            run.metrics.authorization_count,
            run.metrics.human_intervention_count,
            run.metrics.logical_duration_ms,
            observed.get(run.run_id.as_str()).copied().unwrap_or(0.0),
            run.metrics.offline_usage.total_tokens,
            run.metrics.offline_usage.estimated_cost_micro_usd,
            run.metrics.failure_stage.as_deref().unwrap_or("none"),
        ));
    }
    output.push_str("\n* Observed wall-clock values are non-normative, excluded from canonical hashing, and never affect PASS.\n");
    output.push_str("\n## Verifier outcomes and evidence\n\n");
    output.push_str("| Run | done_when | Verifier | Status | Evidence kind | Evidence ref | Evidence SHA-256 |\n");
    output.push_str("| ---: | --- | --- | --- | --- | --- | --- |\n");
    for run in &canonical.runs {
        for result in &run.run_result.verifier_results {
            for evidence in &result.evidence {
                output.push_str(&format!(
                    "| {} | `{}` | `{}` | {:?} | `{}` | `{}` | `{}` |\n",
                    run.repetition_index,
                    result.done_when_id,
                    result.verifier_id,
                    result.status,
                    evidence.kind,
                    evidence.relative_or_opaque_ref,
                    evidence.sha256.as_deref().unwrap_or("none"),
                ));
            }
        }
    }
    output.push_str("\n## Artifact, manifest, receipt, and run hashes\n\n");
    output.push_str("| Run | Kind | Relative reference | Bytes | SHA-256 |\n| ---: | --- | --- | ---: | --- |\n");
    for run in &canonical.runs {
        for artifact in &run.artifact_hashes {
            output.push_str(&format!(
                "| {} | `{}` | `{}` | {} | `{}` |\n",
                run.repetition_index,
                artifact.kind,
                artifact.relative_ref,
                artifact.bytes,
                artifact.sha256,
            ));
        }
    }
    output.push_str("\n## Aggregate metrics\n\n");
    output.push_str(&format!(
        "- A/Q/H/S/F: **{}/{}/{}/{}/{}**\n- VOCR: **{}/{} ({})**\n- False completion: **{}/{} ({})**\n- Verifier pass ratio: **{}/{} ({})**\n- Evidence completeness: **{}/{} ({})**\n- Authorization budget compliance: **{}/{} ({})**\n- Clarifications: {} total; {} runs with clarification\n- Human intervention: {} total; {} runs\n- Logical duration median/p95: `{}` / `{}` ms\n- Observed wall-clock total/median/p95*: `{:.3}` / `{}` / `{}` ms\n- API calls/tokens/cost: **0 / 0 / 0**; availability `none` because this runner is offline\n- Guardrail violations: unauthorized {}, duplicate external write {}, authority drift {}, approval replay {}, refusal bypass {}\n- Completion decision: **{}**\n",
        aggregate.outcomes.a,
        aggregate.outcomes.q,
        aggregate.outcomes.h,
        aggregate.outcomes.s,
        aggregate.outcomes.f,
        aggregate.vocr.numerator,
        aggregate.vocr.denominator,
        ratio_label(&aggregate.vocr),
        aggregate.false_completion_rate.numerator,
        aggregate.false_completion_rate.denominator,
        ratio_label(&aggregate.false_completion_rate),
        aggregate.verifier_pass_ratio.numerator,
        aggregate.verifier_pass_ratio.denominator,
        ratio_label(&aggregate.verifier_pass_ratio),
        aggregate.evidence_complete_ratio.numerator,
        aggregate.evidence_complete_ratio.denominator,
        ratio_label(&aggregate.evidence_complete_ratio),
        aggregate.authorization_budget_compliance.numerator,
        aggregate.authorization_budget_compliance.denominator,
        ratio_label(&aggregate.authorization_budget_compliance),
        aggregate.total_clarifications,
        aggregate.clarification_run_ratio.numerator,
        canonical.aggregate.total_human_intervention_count,
        aggregate.manual_intervention_ratio.numerator,
        aggregate.median_elapsed_ms.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
        aggregate.p95_elapsed_ms.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
        report.observations.summary.total_wall_clock_ms,
        report.observations.summary.median_wall_clock_ms.map(|value| format!("{value:.3}")).unwrap_or_else(|| "none".to_string()),
        report.observations.summary.p95_wall_clock_ms.map(|value| format!("{value:.3}")).unwrap_or_else(|| "none".to_string()),
        aggregate.guardrails.unauthorized_action,
        aggregate.guardrails.duplicate_external_write,
        aggregate.guardrails.authority_drift,
        aggregate.guardrails.cross_task_approval_replay,
        aggregate.guardrails.refusal_bypass,
        completion_label(canonical.aggregate.completion_decision),
    ));
    output.push_str("\n## Failures, exclusions, and interpretation\n\n");
    output.push_str("- Failures: none.\n");
    for ignored in &canonical.ignored_by_scope {
        output.push_str(&format!("- Scope exclusion: {ignored}.\n"));
    }
    output.push_str("- Interpretation: generating files is insufficient. Each run completed only after receipts, hashes, provenance, all six deterministic verifiers, and the C0B completion gate passed.\n");
    Ok(output)
}

fn ratio_label(ratio: &super::BenchmarkRatio) -> String {
    ratio
        .basis_points
        .map(|value| format!("{:.2}%", f64::from(value) / 100.0))
        .unwrap_or_else(|| "n/a".to_string())
}

fn outcome_label(outcome: BenchmarkOutcomeClass) -> &'static str {
    match outcome {
        BenchmarkOutcomeClass::A => "A",
        BenchmarkOutcomeClass::Q => "Q",
        BenchmarkOutcomeClass::H => "H",
        BenchmarkOutcomeClass::S => "S",
        BenchmarkOutcomeClass::F => "F",
    }
}

fn completion_label(decision: BaselineCompletionDecision) -> &'static str {
    match decision {
        BaselineCompletionDecision::Completed => "completed",
        BaselineCompletionDecision::FailedClosed => "failed_closed",
    }
}

fn canonical_json<T: Serialize>(value: &T) -> Result<String, String> {
    let value = serde_json::to_value(value)
        .map_err(|error| format!("serialize canonical report value: {error}"))?;
    serde_json::to_string(&canonical_value(&value))
        .map_err(|error| format!("serialize canonical report: {error}"))
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonical_value).collect()),
        Value::Object(values) => {
            let mut ordered = serde_json::Map::new();
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for key in keys {
                ordered.insert(key.clone(), canonical_value(&values[key]));
            }
            Value::Object(ordered)
        }
        _ => value.clone(),
    }
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::env;

    use tempfile::TempDir;

    use super::*;

    fn config_under(root: &Path, suffix: &str) -> BenchmarkRunnerConfig {
        BenchmarkRunnerConfig {
            workspace_root: root.join(format!("workspace-{suffix}")),
            fixture_root: root.join(format!("fixtures-{suffix}")),
            output_root: root.join(format!("outputs-{suffix}")),
            report_root: root.join(format!("reports-{suffix}")),
            app_version: "1.0.2".to_string(),
            source_commit: "c".repeat(40),
            release_tag: Some("v1.0.2".to_string()),
            environment_profile: "windows-offline-synthetic".to_string(),
            access_mode: AccessMode::AskOnRisk,
        }
    }

    fn explicit_or_temp_config(temp: &TempDir) -> BenchmarkRunnerConfig {
        let names = [
            "DS_AGENT_C0D_WORKSPACE_ROOT",
            "DS_AGENT_C0D_FIXTURE_ROOT",
            "DS_AGENT_C0D_OUTPUT_ROOT",
            "DS_AGENT_C0D_REPORT_ROOT",
        ];
        let values = names.iter().map(env::var_os).collect::<Vec<_>>();
        if values.iter().all(Option::is_none) {
            return config_under(temp.path(), "default");
        }
        assert!(
            values.iter().all(Option::is_some),
            "all explicit C0D roots are required"
        );
        BenchmarkRunnerConfig {
            workspace_root: PathBuf::from(values[0].clone().unwrap()),
            fixture_root: PathBuf::from(values[1].clone().unwrap()),
            output_root: PathBuf::from(values[2].clone().unwrap()),
            report_root: PathBuf::from(values[3].clone().unwrap()),
            app_version: env::var("DS_AGENT_C0D_APP_VERSION")
                .unwrap_or_else(|_| "1.0.2".to_string()),
            source_commit: env::var("DS_AGENT_C0D_SOURCE_COMMIT")
                .unwrap_or_else(|_| "c".repeat(40)),
            release_tag: Some(
                env::var("DS_AGENT_C0D_RELEASE_TAG").unwrap_or_else(|_| "v1.0.2".to_string()),
            ),
            environment_profile: "windows-offline-synthetic".to_string(),
            access_mode: AccessMode::AskOnRisk,
        }
    }

    #[test]
    fn offline_runner_accepts_explicit_roots_and_readbacks_five_clean_runs() {
        let temp = tempfile::tempdir().unwrap();
        let config = explicit_or_temp_config(&temp);
        let report = run_t1_step0_baseline(&config).unwrap();
        assert_eq!(report.canonical.runs.len(), 5);
        assert_eq!(report.canonical.aggregate.required_verifier_pass_count, 30);
        assert_eq!(report.canonical.aggregate.c0b_metrics.outcomes.a, 5);
        assert_eq!(report.canonical.aggregate.c0b_metrics.outcomes.f, 0);
        let readback = readback_t1_step0_baseline(&config).unwrap();
        assert!(reports_match(&readback, &report));
    }

    #[test]
    fn canonical_report_is_identical_across_fresh_repetitions() {
        let temp = tempfile::tempdir().unwrap();
        let first = run_t1_step0_baseline(&config_under(temp.path(), "one")).unwrap();
        let second = run_t1_step0_baseline(&config_under(temp.path(), "two")).unwrap();
        assert_eq!(
            first.canonical_json().unwrap(),
            second.canonical_json().unwrap()
        );
        assert_eq!(
            first.integrity.canonical_payload_sha256,
            second.integrity.canonical_payload_sha256
        );
    }

    #[test]
    fn report_missing_duplicate_reused_nonfinite_secret_and_fake_pass_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let report = run_t1_step0_baseline(&config_under(temp.path(), "negative")).unwrap();

        let mut missing = report.clone();
        missing.canonical.runs[0].run_result.verifier_results.pop();
        assert!(missing.validate().is_err());

        let mut duplicate = report.clone();
        let evidence =
            duplicate.canonical.runs[0].run_result.verifier_results[0].evidence[0].clone();
        duplicate.canonical.runs[0].run_result.verifier_results[1]
            .evidence
            .push(evidence);
        assert!(duplicate.validate().is_err());

        let mut duplicate_verifier = report.clone();
        duplicate_verifier.canonical.runs[0]
            .run_result
            .verifier_results[1]
            .done_when_id = duplicate_verifier.canonical.runs[0]
            .run_result
            .verifier_results[0]
            .done_when_id
            .clone();
        duplicate_verifier.canonical.runs[0]
            .run_result
            .verifier_results[1]
            .verifier_id = duplicate_verifier.canonical.runs[0]
            .run_result
            .verifier_results[0]
            .verifier_id
            .clone();
        assert!(duplicate_verifier.validate().is_err());

        let mut unexpected_verifier = report.clone();
        unexpected_verifier.canonical.runs[0]
            .run_result
            .verifier_results[0]
            .verifier_id = "t1.unexpected/v1".to_string();
        assert!(unexpected_verifier.validate().is_err());

        let mut reused = report.clone();
        reused.canonical.runs[1].clean_root_binding =
            reused.canonical.runs[0].clean_root_binding.clone();
        assert!(reused.validate().is_err());

        let mut nonfinite = report.clone();
        nonfinite.observations.runs[0].wall_clock_ms = f64::NAN;
        assert!(nonfinite.validate().is_err());

        let mut secret = report.clone();
        secret.canonical.scope_disclosure = format!("{}{}", "sk-", "abcdefghijklmnop");
        assert!(secret.validate().is_err());

        let mut fake_pass = report.clone();
        fake_pass.canonical.runs[0].run_result.verifier_results[0].status =
            BenchmarkVerifierStatus::Failed;
        fake_pass.canonical.runs[0].completion_decision = BaselineCompletionDecision::Completed;
        assert!(fake_pass.validate().is_err());

        let mut tampered_hash = report;
        tampered_hash.integrity.canonical_payload_sha256 = "a".repeat(64);
        assert!(tampered_hash.validate().is_err());
    }

    #[test]
    fn non_clean_and_nested_roots_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let dirty = config_under(temp.path(), "dirty");
        fs::create_dir_all(&dirty.workspace_root).unwrap();
        fs::write(dirty.workspace_root.join("marker"), b"dirty").unwrap();
        assert!(run_t1_step0_baseline(&dirty).is_err());

        let mut nested = config_under(temp.path(), "nested");
        nested.output_root = nested.workspace_root.join("escape");
        assert!(run_t1_step0_baseline(&nested).is_err());
    }

    #[test]
    fn tampered_fixture_output_and_receipt_fail_disk_readback() {
        let temp = tempfile::tempdir().unwrap();
        let config = config_under(temp.path(), "disk-tamper");
        run_t1_step0_baseline(&config).unwrap();

        let fixture = config
            .fixture_root
            .join("run-01/inputs/01-monthly-revenue.xlsx");
        let original_fixture = fs::read(&fixture).unwrap();
        fs::write(&fixture, b"tampered fixture").unwrap();
        assert!(readback_t1_step0_baseline(&config).is_err());
        fs::write(&fixture, original_fixture).unwrap();

        let output = config
            .output_root
            .join("run-01/outputs/t1-reconciliation.xlsx");
        let original_output = fs::read(&output).unwrap();
        fs::write(&output, b"tampered output").unwrap();
        assert!(readback_t1_step0_baseline(&config).is_err());
        fs::write(&output, original_output).unwrap();

        let receipt = config
            .output_root
            .join("run-01/receipts/result-receipt.json");
        let original_receipt = fs::read(&receipt).unwrap();
        fs::write(&receipt, b"{}").unwrap();
        assert!(readback_t1_step0_baseline(&config).is_err());
        fs::write(&receipt, original_receipt).unwrap();

        let json_report = config.report_root.join(JSON_REPORT_FILE);
        let mut json = fs::read_to_string(&json_report).unwrap();
        json = json.replacen("canonical_payload_sha256", "canonical_payload_sha25x", 1);
        fs::write(&json_report, json).unwrap();
        assert!(readback_t1_step0_baseline(&config).is_err());
    }
}
