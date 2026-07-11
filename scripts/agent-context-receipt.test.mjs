#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const receiptModuleUrl = new URL("../apps/desktop/src/agentContextReceipt.ts", import.meta.url);
const appSourceUrl = new URL("../apps/desktop/src/App.tsx", import.meta.url);
const commandsSourceUrl = new URL("../apps/desktop/src-tauri/src/commands.rs", import.meta.url);
const mainSourceUrl = new URL("../apps/desktop/src-tauri/src/main.rs", import.meta.url);

const { summarizeAgentContextReceipt } = await import(receiptModuleUrl);

test("summarizes agent context receipts for compact inspector display", () => {
  const summary = summarizeAgentContextReceipt({
    id: "receipt-1",
    user_intent: "Central chat action requested by the user.",
    loop_mode: "evidence_gathering",
    action_type: "file_read",
    execution_state: "succeeded",
    capability: "file_read",
    policy_decision: "allow",
    capability_invocation_id: "invocation-1",
    workflow_run_id: null,
    selected_evidence: ["capability_invocation:invocation-1", "target:reports/source.md"],
    selected_memories: [
      "memory_retrieval=memory_runtime/v1; query_terms_count=6; considered_records=4; candidate_count=3; selected_count=2; max_records=3; max_bytes=1200; used_bytes=512; filtered_sensitive=1; filtered_archived=1; omitted_by_budget=1",
      "memory_id=memory-1; rank=1; title=项目记忆运行规则; type=workflow_rule; scope=project; score=18; score_breakdown=title_terms:2*6 body_terms:2*3 linked_terms:0*1 pinned:0; match_reason=title_terms=记忆,系统; inclusion_mode=compact_snippet; snippet=用户要求 DS Agent 记忆系统要对标 Codex 和 Claude Code。",
      "memory_id=memory-2; rank=2; title=用户默认语气偏好; type=preference; scope=user; score=11; score_breakdown=title_terms:1*6 body_terms:1*3 linked_terms:0*1 pinned:+2; match_reason=body_terms=语气; inclusion_mode=compact_snippet; snippet=默认用简洁、温暖、直接的中文语气。",
      "soul_profile=memory/soul.md; reason=identity_profile; bytes=35/800; lines=user preferred address: 李总",
    ],
    memory_candidate_gate: [
      "proposed=3; kept=1; dropped=2; reasons=sensitive=1,transient=1,archived=0,invalid=0,over_limit=0",
      "kept title=Default response tone; suggested_action=new; privacy_review=normal",
    ],
    model_route: "auto",
    thinking_level: "fast",
    token_cache_state: "cache: miss",
    allowed_tools: ["browser_browse", "file_read", "network_search"],
    validators: ["schema_normalized", "capability_policy_checked"],
    stop_conditions: ["evidence_observed", "blocked_or_failed"],
    matched_stop_conditions: ["evidence_observed"],
    confirmation_rule: "follow capability policy before tool dispatch",
    policy_constraints: [
      "access_mode=full_access",
      "requires_confirmation=false",
      "policy_decision=allow",
    ],
    validation_results: [
      "model proposal parsed and normalized",
      "capability invocation recorded",
      "policy_decision=allow",
    ],
    intentional_omissions: [
      "raw user prompt not stored",
      "API keys and local secrets omitted",
    ],
    created_at: "2026-07-05T08:00:00Z",
  });

  assert.equal(summary.title, "evidence_gathering / file_read");
  assert.equal(summary.status, "succeeded");
  assert.deepEqual(summary.meta, ["auto", "fast", "cache: miss"]);
  assert.deepEqual(summary.evidence, [
    "capability_invocation:invocation-1",
    "target:reports/source.md",
  ]);
  assert.deepEqual(summary.memories, [
    "项目记忆运行规则",
    "用户默认语气偏好",
  ]);
  assert.deepEqual(summary.memoryRetrieval, [
    "retrieval v1: selected 2/3 candidates from 4 reviewed memories",
    "budget: records 2/3, bytes 512/1200, query terms 6",
  ]);
  assert.deepEqual(summary.memoryScores, [
    "rank 1 score 18: 项目记忆运行规则 (workflow_rule/project; title_terms=记忆,系统; compact_snippet; title_terms:2*6 body_terms:2*3 linked_terms:0*1 pinned:0)",
    "rank 2 score 11: 用户默认语气偏好 (preference/user; body_terms=语气; compact_snippet; title_terms:1*6 body_terms:1*3 linked_terms:0*1 pinned:+2)",
  ]);
  assert.deepEqual(summary.memoryFeedbackTargets, [
    {
      memoryId: "memory-1",
      title: "项目记忆运行规则",
      rank: "1",
      score: "18",
      memoryType: "workflow_rule",
      scope: "project",
      matchReason: "title_terms=记忆,系统",
      scoreBreakdown: "title_terms:2*6 body_terms:2*3 linked_terms:0*1 pinned:0",
      inclusionMode: "compact_snippet",
    },
    {
      memoryId: "memory-2",
      title: "用户默认语气偏好",
      rank: "2",
      score: "11",
      memoryType: "preference",
      scope: "user",
      matchReason: "body_terms=语气",
      scoreBreakdown: "title_terms:1*6 body_terms:1*3 linked_terms:0*1 pinned:+2",
      inclusionMode: "compact_snippet",
    },
  ]);
  assert.deepEqual(summary.memoryConflictHints, [
    "1 sensitive memory omitted from prompt context",
    "1 archived or stale memory omitted from prompt context",
    "1 lower-ranked memory omitted by retrieval budget",
  ]);
  assert.deepEqual(summary.memoryCandidateGate, [
    "proposed=3; kept=1; dropped=2; reasons=sensitive=1,transient=1,archived=0,invalid=0,over_limit=0",
    "kept title=Default response tone; suggested_action=new; privacy_review=normal",
  ]);
  assert.deepEqual(summary.validation, [
    "model proposal parsed and normalized",
    "capability invocation recorded",
  ]);
  assert.deepEqual(summary.policy, [
    "constraints: access_mode=full_access, requires_confirmation=false",
    "tools: browser_browse, file_read",
    "validators: schema_normalized, capability_policy_checked",
    "stop: evidence_observed, blocked_or_failed",
    "matched: evidence_observed",
    "confirm: follow capability policy before tool dispatch",
  ]);
  assert.deepEqual(summary.omissions, [
    "raw user prompt not stored",
    "API keys and local secrets omitted",
  ]);
});

test("App wires the context receipt list into the capability inspector", () => {
  const appSource = readFileSync(appSourceUrl, "utf8");

  assert.match(appSource, /invoke<AgentContextReceipt\[\]>\("list_agent_context_receipts"\)/);
  assert.match(appSource, /agentContextReceipts\.slice\(0,\s*3\)\.map/);
  assert.match(appSource, /summarizeAgentContextReceipt\(receipt\)/);
  assert.match(appSource, /summary\.memories\.length > 0/);
  assert.match(appSource, /summary\.memoryRetrieval\.length > 0/);
  assert.match(appSource, /summary\.memoryScores\.length > 0/);
  assert.match(appSource, /summary\.memoryConflictHints\.length > 0/);
  assert.match(appSource, /summary\.memoryCandidateGate\.length > 0/);
  assert.match(appSource, /summary\.memoryFeedbackTargets\.length > 0/);
  assert.match(appSource, /recordSelectedMemoryFeedback/);
  assert.match(appSource, /"record_selected_memory_feedback"/);
  assert.match(appSource, /copy\.memoryFeedback\.shouldUpdate/);
});

test("Memory Studio exposes selected-memory feedback review", () => {
  const appSource = readFileSync(appSourceUrl, "utf8");
  const commandsSource = readFileSync(commandsSourceUrl, "utf8");
  const mainSource = readFileSync(mainSourceUrl, "utf8");

  assert.match(commandsSource, /pub fn list_selected_memory_feedback/);
  assert.match(mainSource, /list_selected_memory_feedback/);
  assert.match(appSource, /invoke<MemorySelectedFeedback\[\]>\("list_selected_memory_feedback"\)/);
  assert.match(appSource, /selectedMemoryFeedbackRecords/);
  assert.match(appSource, /feedbackReviewItems/);
  assert.match(appSource, /copy\.memory\.feedbackReview/);
  assert.match(appSource, /copy\.memory\.needsFeedbackReview/);
  assert.match(appSource, /copy\.memory\.feedbackReviewEmpty/);
});

test("Memory Studio exposes background maintenance audit with filtering and sorting", () => {
  const appSource = readFileSync(appSourceUrl, "utf8");
  const commandsSource = readFileSync(commandsSourceUrl, "utf8");
  const mainSource = readFileSync(mainSourceUrl, "utf8");

  assert.match(commandsSource, /pub fn list_memory_maintenance_reviews/);
  assert.match(commandsSource, /pub fn run_memory_background_maintenance/);
  assert.match(commandsSource, /auto_candidate_decisions_applied/);
  assert.match(commandsSource, /MemoryCandidateSuggestedAction::Update/);
  assert.match(commandsSource, /MemoryCandidateSuggestedAction::Archive/);
  assert.match(mainSource, /list_memory_maintenance_reviews/);
  assert.match(mainSource, /run_memory_background_maintenance/);
  assert.match(appSource, /invoke<MemoryMaintenanceReviewItem\[\]>\("list_memory_maintenance_reviews"\)/);
  assert.match(appSource, /invoke<MemoryBackgroundMaintenanceSummary>\(\s*"run_memory_background_maintenance"/);
  assert.match(
    appSource,
    /await Promise\.all\(\[\s*refreshCapabilityState\(\),\s*refreshSkillRecords\(\),\s*runMemoryBackgroundMaintenance\(\),\s*\]\)/,
  );
  assert.match(
    appSource,
    /await runMemoryBackgroundMaintenance\(\);\s*const \[records, memories, memoryCandidates, briefingRuns\]/s,
  );
  assert.doesNotMatch(appSource, /invoke\("resolve_memory_candidate"/);
  assert.doesNotMatch(appSource, /invoke\("merge_memory_candidate_with_conflicts"/);
  assert.doesNotMatch(appSource, /invoke\("replace_memory_candidate_conflicts"/);
  assert.doesNotMatch(appSource, /invoke\("update_memory_candidate_conflict"/);
  assert.doesNotMatch(appSource, /invoke\("archive_memory_candidate_conflicts"/);
  assert.match(appSource, /memoryMaintenanceFilter/);
  assert.match(appSource, /memoryMaintenanceSort/);
  assert.match(appSource, /filteredMemoryMaintenanceReviews/);
  assert.match(appSource, /copy\.memory\.maintenanceReview/);
  assert.match(appSource, /copy\.memory\.maintenanceAutomatic/);
  assert.match(appSource, /copy\.memory\.maintenanceNoUserAction/);
  assert.match(appSource, /copy\.memory\.maintenanceFilterOptions/);
  assert.match(appSource, /copy\.memory\.maintenanceSortOptions/);
});

test("Memory Studio escalates repeated selected-memory feedback for maintenance review", () => {
  const appSource = readFileSync(appSourceUrl, "utf8");

  assert.match(
    appSource,
    /const repeatedIrrelevantFeedback = \(counts\.irrelevant \?\? 0\) >= 2/,
  );
  assert.match(appSource, /const repeatedStaleFeedback = \(counts\.stale \?\? 0\) >= 2/);
  assert.match(appSource, /repeatedIrrelevantFeedback \|\|/);
  assert.match(appSource, /repeatedStaleFeedback \|\|/);
});
