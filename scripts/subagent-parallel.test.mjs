import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const appSource = await readFile(new URL("../apps/desktop/src/App.tsx", import.meta.url), "utf8");
const commandSource = await readFile(
  new URL("../apps/desktop/src-tauri/src/commands.rs", import.meta.url),
  "utf8",
);
const runSource = await readFile(
  new URL("../apps/desktop/src-tauri/src/kernel/agent_run.rs", import.meta.url),
  "utf8",
);

test("runs at most three queued Subagents concurrently without adding their replies to chat", () => {
  assert.match(appSource, /readyExpertAttempts\(currentRuns\)/);
  assert.match(appSource, /queue_expert_team_retries/);
  assert.match(appSource, /Promise\.all\([\s\S]*queuedSubagents\.map/);
  assert.match(appSource, /desktop-subagent-worker-\$\{index \+ 1\}/);
  assert.doesNotMatch(
    appSource.match(/if \(queuedSubagents\.length > 0\) \{[\s\S]*?return;\n        \}/)?.[0] ?? "",
    /updateActiveAgentMessages/,
  );
});

test("queues one parent synthesis after all Subagents terminate", () => {
  assert.match(appSource, /queue_parent_agent_synthesis/);
  assert.match(appSource, /\["completed", "failed", "cancelled"\]\.includes\(child\.status\)/);
  assert.match(commandSource, /All latest Expert Team attempts passed their gates/);
  assert.match(commandSource, /do not claim failed work succeeded/i);
});

test("enforces bounded one-level read-only Subagent execution", () => {
  assert.match(runSource, /AGENT_RUN_MAX_PARALLEL_SUBAGENTS: usize = 3/);
  assert.match(commandSource, /Background Subagents are read-only/);
  assert.match(commandSource, /ExpertCapability::FileRead/);
  assert.doesNotMatch(
    commandSource.match(/fn block_subagent_mutating_actions[\s\S]*?\n\}/)?.[0] ?? "",
    /browser_open/,
  );
  assert.match(commandSource, /Never create nested subagents/);
});
