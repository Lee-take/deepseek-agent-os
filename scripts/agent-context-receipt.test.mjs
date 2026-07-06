#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const receiptModuleUrl = new URL("../apps/desktop/src/agentContextReceipt.ts", import.meta.url);
const appSourceUrl = new URL("../apps/desktop/src/App.tsx", import.meta.url);

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
      "memory_id=memory-1; title=项目记忆运行规则; match_reason=title_terms=记忆,系统",
      "memory_id=memory-2; title=用户默认语气偏好; match_reason=body_terms=语气",
      "memory_id=memory-3; title=lower-ranked memory",
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
    "memory_id=memory-1; title=项目记忆运行规则; match_reason=title_terms=记忆,系统",
    "memory_id=memory-2; title=用户默认语气偏好; match_reason=body_terms=语气",
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
});
