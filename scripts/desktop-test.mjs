#!/usr/bin/env node

import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";

if (!existsSync(path.join("apps", "desktop", "package.json"))) {
  console.error(
    "Desktop workspace has not been created yet. Missing apps/desktop/package.json.",
  );
  process.exit(1);
}

const env = {
  ...process.env,
  CARGO_TARGET_DIR:
    process.env.CARGO_TARGET_DIR ??
    path.join(os.tmpdir(), "deepseek_agent_os_cargo_target"),
};

run([
  "npx",
  "pnpm@9.15.9",
  "--filter",
  "@deepseek-agent-os/desktop",
  "build",
]);
run(["node", "scripts/conversation-title.test.mjs"]);
run(["node", "scripts/agent-chat-run-state.test.mjs"]);
run(["node", "scripts/subagent-parallel.test.mjs"]);
run(["node", "scripts/expert-team.test.mjs"]);
run(["node", "scripts/automation-center.test.mjs"]);
run(["node", "scripts/agent-attachments.test.mjs"]);
run(["node", "scripts/agent-chat-pending.test.mjs"]);
run(["node", "scripts/agent-context-receipt.test.mjs"]);
run(["node", "scripts/settings-panel.test.mjs"]);
run(["node", "scripts/plugin-catalog.test.mjs"]);
run(["node", "scripts/app-update-flow.test.mjs"]);
run(["node", "scripts/approval-visibility.test.mjs"]);
run(["node", "scripts/durable-computer-use-ui.test.mjs"]);
run(["node", "scripts/memory-candidate-gate.test.mjs"]);
run(["cargo", "test", "--manifest-path", "apps/desktop/src-tauri/Cargo.toml"]);

function run(parts) {
  const result = spawnSync(parts.map(quoteShellPart).join(" "), {
    env,
    shell: true,
    stdio: "inherit",
  });

  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function quoteShellPart(value) {
  if (/^[A-Za-z0-9_@./:=+-]+$/.test(value)) {
    return value;
  }
  return `"${value.replaceAll('"', '\\"')}"`;
}
