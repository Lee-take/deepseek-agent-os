import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import ts from "../apps/desktop/node_modules/typescript/lib/typescript.js";

const source = await readFile(
  new URL("../apps/desktop/src/expert-team.ts", import.meta.url),
  "utf8",
);
const transpiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ES2020,
    target: ts.ScriptTarget.ES2020,
  },
}).outputText;
const helpers = await import(
  `data:text/javascript;base64,${Buffer.from(transpiled).toString("base64")}`
);

function record({
  id,
  parent = "parent",
  key,
  role,
  attempt = 1,
  status = "queued",
  dependsOn = [],
  access = "read",
  result = null,
}) {
  return {
    id,
    role: "subagent",
    status,
    parent_run_id: parent,
    started_at: `2026-07-15T00:00:0${attempt}Z`,
    expert_contract: {
      team_id: "team",
      parent_run_id: parent,
      parent_input_revision: "revision",
      key,
      role,
      attempt,
      previous_attempt_run_id: attempt > 1 ? `${key}-1` : null,
      prompt: key,
      depends_on: dependsOn,
      capabilities: ["file_read"],
      resources: [{ key: "shared", access }],
      budget: {
        max_elapsed_ms: 1000,
        max_tool_calls: 4,
        max_tokens: 1000,
        max_output_bytes: 4096,
        max_staged_bytes: 4096,
      },
      output_contract: {
        min_evidence_sources: 0,
        require_claims: false,
        require_staged_output: false,
        require_review: false,
        fail_on_unresolved_conflict: false,
      },
      retry_policy: { max_attempts: 2, substitute_role: null },
    },
    expert_result: result,
  };
}

const passedResult = {
  quality_gates: [{ code: "fake", passed: true, detail: "passed" }],
  evidence: [{ verified: true }],
  unresolved_conflicts: [],
  usage: { tokens: 200, tool_calls: 1 },
};

test("schedules only dependency-ready latest expert attempts", () => {
  const research = record({
    id: "research-1",
    key: "research",
    role: "research",
    status: "completed",
    result: passedResult,
  });
  const analysis = record({
    id: "analysis-1",
    key: "analysis",
    role: "analysis",
    dependsOn: ["research"],
  });
  const review = record({
    id: "review-1",
    key: "review",
    role: "review",
    dependsOn: ["production"],
  });
  assert.deepEqual(
    helpers.readyExpertAttempts([research, analysis, review]).map((item) => item.id),
    ["analysis-1"],
  );
});

test("latest retry supersedes failed attempt and write conflicts serialize", () => {
  const failed = record({
    id: "research-1",
    key: "research",
    role: "research",
    status: "failed",
  });
  const retry = record({
    id: "research-2",
    key: "research",
    role: "analysis",
    attempt: 2,
  });
  assert.equal(helpers.latestExpertAttempts([failed, retry], "parent")[0].id, "research-2");

  const writer = record({
    id: "production-1",
    key: "production",
    role: "production",
    status: "running",
    access: "write",
  });
  assert.equal(helpers.isExpertAttemptReady(retry, [failed, retry, writer]), false);
});

test("parent synthesis stays closed until every latest result passes", () => {
  const parent = { id: "parent", role: "parent", status: "blocked" };
  const passed = record({
    id: "research-1",
    key: "research",
    role: "research",
    status: "completed",
    result: passedResult,
  });
  const failed = record({
    id: "analysis-1",
    key: "analysis",
    role: "analysis",
    status: "failed",
    result: { ...passedResult, quality_gates: [{ code: "fake", passed: false }] },
  });
  assert.deepEqual(helpers.blockedParentsWithTerminalChildren([parent, passed, failed]), [parent]);
  assert.equal(helpers.expertTeamCanSynthesize([parent, passed, failed], "parent"), false);
  failed.status = "completed";
  failed.expert_result = passedResult;
  assert.equal(helpers.expertTeamCanSynthesize([parent, passed, failed], "parent"), true);
});
