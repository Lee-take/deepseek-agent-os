#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [panel, automationCenter, main, lifecycleCommands, lifecycleKernel] = await Promise.all([
  readFile("apps/desktop/src/TaskLifecyclePanel.tsx", "utf8"),
  readFile("apps/desktop/src/AutomationCenter.tsx", "utf8"),
  readFile("apps/desktop/src-tauri/src/main.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/lifecycle_commands.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/kernel/task_lifecycle.rs", "utf8"),
]);

assert.match(panel, /invoke<TaskLifecycleSnapshot>\("list_task_lifecycle"\)/);
assert.match(panel, /结果未知时不会自动重复执行/);
assert.match(panel, /remote_uncertain/);
assert.doesNotMatch(panel, /credential_handle|refresh_token|access_token|provider error body/i);
assert.match(automationCenter, /<TaskLifecyclePanel language=\{language\} \/>/);
assert.match(main, /list_task_lifecycle,/);
assert.match(lifecycleCommands, /task_lifecycle_snapshot\(\)/);
assert.match(lifecycleKernel, /TaskLifecycleSource::ExpertAttempt/);
assert.match(lifecycleKernel, /TaskLifecycleSource::ConnectorRecovery/);
assert.match(lifecycleKernel, /TaskLifecycleSource::ComputerUse/);
assert.match(lifecycleKernel, /TaskLifecyclePhase::EffectUnknown/);
assert.match(lifecycleKernel, /TaskEffectState::CompensationRequired/);
assert.doesNotMatch(lifecycleKernel, /\.prompt\.clone\(\)|input_summary\.clone\(\)/);

console.log("task lifecycle projection UI checks passed");
