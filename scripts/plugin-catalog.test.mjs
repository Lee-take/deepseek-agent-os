#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [app, i18n, commands, skill, styles] = await Promise.all([
  readFile("apps/desktop/src/App.tsx", "utf8"),
  readFile("apps/desktop/src/i18n.ts", "utf8"),
  readFile("apps/desktop/src-tauri/src/commands.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/kernel/skill.rs", "utf8"),
  readFile("apps/desktop/src/styles.css", "utf8"),
]);

assert.match(app, /className="skill-catalog"/);
assert.match(app, /<details className="skill-catalog-item"/);
assert.match(app, /record\.manifest\.description/);
assert.match(app, /copy\.skills\.autoInvoke/);
assert.doesNotMatch(app, /onSubmit=\{installLocalSkillManifest\}/);
assert.doesNotMatch(app, /copy\.skills\.prepareExecution/);
assert.match(app, /setLocalSkillEnabled\(/);
assert.match(app, /uninstallLocalSkill\(/);
assert.match(app, /copy\.skills\.scenarioTemplates/);
assert.match(
  app,
  /<details className="sidebar-tool operations-tool ordinary-user-hidden">[\s\S]*?copy\.skills\.scenarioTemplates/,
);
assert.match(
  app,
  /<details className="sidebar-tool package-tool ordinary-user-hidden">[\s\S]*?copy\.package\.title/,
);
assert.match(styles, /\.ordinary-user-hidden\s*\{\s*display: none;/);
assert.match(app, /run_skill_update_sweep/);

const skillRefreshStart = app.indexOf("const refreshSkillRecords");
const skillRefreshEnd = app.indexOf("const refreshAgentRunRecords", skillRefreshStart);
const skillRefresh = app.slice(skillRefreshStart, skillRefreshEnd);
assert.match(skillRefresh, /invoke<SkillRecord\[]>\("list_skill_records"\)/);
assert.match(skillRefresh, /setSkillRecords\(records\)/);

const durableWorkerStart = app.indexOf("const runNextDurableAgentTask");
const durableWorkerEnd = app.indexOf("const sendAgentPrompt", durableWorkerStart);
const durableWorker = app.slice(durableWorkerStart, durableWorkerEnd);
assert.match(durableWorker, /refreshSkillRecords\(\)/);

const sendAgentPromptStart = app.indexOf("const sendAgentPrompt");
const sendAgentPromptEnd = app.indexOf(
  "const resolveAndResumeAgentActionGroup",
  sendAgentPromptStart,
);
const sendAgentPrompt = app.slice(sendAgentPromptStart, sendAgentPromptEnd);
assert.match(sendAgentPrompt, /refreshSkillRecords\(\)/);

assert.match(i18n, /DS Agent 会在任务需要时自动调用已安装的技能与插件。/);
assert.match(i18n, /DS Agent automatically uses installed skills and plugins when a task needs them\./);

const catalogStart = commands.indexOf("fn load_agent_skill_catalog");
const catalogEnd = commands.indexOf("fn build_agent_skill_catalog_prompt", catalogStart);
const catalog = commands.slice(catalogStart, catalogEnd);
assert.match(catalog, /enablement_status == SkillEnablementStatus::Enabled/);
assert.match(catalog, /trust_level != SkillTrustLevel::Untrusted/);
assert.match(catalog, /record\.entry_available/);

const executionGateStart = skill.indexOf("fn skill_execution_blocked_reason");
const executionGateEnd = skill.indexOf("fn declarative_skill_execution_plan", executionGateStart);
const executionGate = skill.slice(executionGateStart, executionGateEnd);
assert.match(executionGate, /skill is disabled/);
assert.match(executionGate, /skill trust is unverified/);

console.log("plugin catalog tests passed");
