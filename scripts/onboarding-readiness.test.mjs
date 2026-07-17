#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [app, types, i18n, commands, main, credentialKernel, workspaceKernel] =
  await Promise.all([
    readFile("apps/desktop/src/App.tsx", "utf8"),
    readFile("apps/desktop/src/types.ts", "utf8"),
    readFile("apps/desktop/src/i18n.ts", "utf8"),
    readFile("apps/desktop/src-tauri/src/commands.rs", "utf8"),
    readFile("apps/desktop/src-tauri/src/main.rs", "utf8"),
    readFile("apps/desktop/src-tauri/src/kernel/deepseek_credential.rs", "utf8"),
    readFile("apps/desktop/src-tauri/src/kernel/local_directory.rs", "utf8"),
  ]);

const deepSeekCodes = [
  "ready",
  "not_checked",
  "key_missing",
  "key_format_invalid",
  "authentication_failed",
  "insufficient_balance",
  "rate_limited",
  "network_unavailable",
  "network_timeout",
  "model_unavailable",
  "request_invalid",
  "provider_unavailable",
  "provider_protocol_error",
  "credential_store_unavailable",
];

const workspaceCodes = [
  "ready",
  "workspace_missing",
  "workspace_unavailable",
  "workspace_permission_denied",
  "workspace_probe_cleanup_failed",
  "workspace_settings_invalid",
];

assert.equal(
  (app.match(/const \[deepSeekApiKeyDraft, setDeepSeekApiKeyDraft\] = useState\(""\);/g) ?? [])
    .length,
  1,
  "the UI must keep one ordinary Key draft owner",
);
assert.doesNotMatch(app, /fallbackApiKey|sessionDeepSeekApiKey|deepSeekApiKeyCandidates/);
assert.doesNotMatch(i18n, /Fallback DeepSeek API key|备用 DeepSeek API key/);
assert.match(app, /type="password"[\s\S]*?value=\{deepSeekApiKeyDraft\}/);

const saveKeyStart = app.indexOf("const saveDeepSeekKey = async");
const retryStart = app.indexOf("const retryDeepSeekReadiness", saveKeyStart);
const saveKey = app.slice(saveKeyStart, retryStart);
assert.match(saveKey, /finally\s*\{[\s\S]*?setDeepSeekApiKeyDraft\(""\)/);
assert.doesNotMatch(saveKey, /setDeepSeekReadinessError\(String\(error\)/);
assert.match(saveKey, /readiness\.deepseek\.source === "stored"/);
assert.match(saveKey, /readiness\.deepseek\.code !== "key_format_invalid"/);

const keyModalStart = app.indexOf('agentSetupPrompt === "deepseek_key"');
const workspaceModalStart = app.indexOf('agentSetupPrompt === "workspace"', keyModalStart);
const keyModal = app.slice(keyModalStart, workspaceModalStart);
assert.match(keyModal, /disabled=\{deepSeekReadinessPending \|\| !deepSeekApiKeyDraft\.trim\(\)\}/);
assert.doesNotMatch(keyModal, /!canRetryDeepSeekReadiness/);
assert.match(keyModal, /\{deepSeekReadinessMessage\}/);

const storageLines = app
  .split(/\r?\n/)
  .filter((line) => line.includes("localStorage") || line.includes("sessionStorage"))
  .join("\n");
assert.doesNotMatch(storageLines, /api.?key|credential|deepSeekApiKeyDraft/i);

assert.match(app, /invoke<OnboardingReadinessProjection>\("get_onboarding_readiness"\)/);
assert.match(app, /setOnboardingReadiness\(readiness\)/);
assert.match(app, /copy\.onboarding\.deepseekMessages\[deepSeekCredentialStatus\.code\]/);
assert.match(app, /copy\.onboarding\.workspaceMessages\[workspaceReadiness\.code\]/);
assert.doesNotMatch(app, /get_deepseek_credential_status|get_deepseek_user_balance/);

for (const command of [
  "run_agent_chat",
  "run_next_queued_agent_chat_worker",
  "run_memory_background_maintenance",
  "run_operations_briefing",
]) {
  const block = commandInvocationBlock(app, command);
  assert.ok(block, `expected ${command} invocation`);
  assert.doesNotMatch(block, /apiKey|api_key|credential/i, `${command} must not receive a Key`);
}

const projectionTypes = types.slice(
  types.indexOf("export type DeepSeekReadinessProjection"),
  types.indexOf("export type AppUpdateStatus"),
);
assert.doesNotMatch(
  projectionTypes,
  /base_url|endpoint|api_key|key_hash|fingerprint|account|currency|amount|total_balance|app_data|settings_file|vault|workspace_dir|evidence_dir|export_dir/i,
);
assert.match(projectionTypes, /flash_model: "deepseek-v4-flash"/);
assert.match(projectionTypes, /pro_model: "deepseek-v4-pro"/);
const pricingProjection = types.slice(
  types.indexOf("export type DeepSeekPricingState"),
  types.indexOf("export type NetworkSearchRouteStatus"),
);
assert.doesNotMatch(pricingProjection, /app_data_dir|settings_file|vault/i);
assert.doesNotMatch(app, /deepSeekPricingState\.(?:app_data_dir|settings_file)/);

for (const code of [...new Set([...deepSeekCodes, ...workspaceCodes])]) {
  const copies = i18n.match(new RegExp(`\\b${escapeRegex(code)}\\s*:`, "g")) ?? [];
  assert.ok(copies.length >= 2, `expected Chinese and English copy for ${code}`);
}
for (const action of [
  "saveAndCheck",
  "replaceKey",
  "removeKey",
  "retryCheck",
  "chooseWorkspace",
]) {
  const copies = i18n.match(new RegExp(`\\b${action}\\s*:`, "g")) ?? [];
  assert.ok(copies.length >= 3, `expected typed Chinese and English copy for ${action}`);
}
assert.match(i18n, /contacts DeepSeek|联系 DeepSeek/);

for (const command of [
  "get_onboarding_readiness",
  "save_deepseek_api_key",
  "verify_deepseek_api_key",
  "remove_deepseek_api_key",
]) {
  assert.match(commands, new RegExp(`pub fn ${command}\\b`));
  assert.match(main, new RegExp(`\\b${command},`));
}
assert.doesNotMatch(main, /get_deepseek_credential_status|get_deepseek_user_balance/);

for (const consumer of [
  "pub fn run_agent_chat(",
  "pub async fn run_next_queued_agent_chat_worker(",
  "pub fn run_memory_background_maintenance(",
  "pub fn run_operations_briefing(",
]) {
  const block = rustFunctionBlock(commands, consumer);
  assert.match(block, /deepseek_credentials/);
}
assert.doesNotMatch(commands, /session_api_key|fallback_api_key|api_key_override/);

assert.match(credentialKernel, /CryptProtectData/);
assert.match(credentialKernel, /CryptUnprotectData/);
assert.match(credentialKernel, /MoveFileExW/);
assert.match(credentialKernel, /stored[\s\S]*environment[\s\S]*missing/i);
assert.match(credentialKernel, /fetch_user_balance[\s\S]*fetch_models/);
assert.match(credentialKernel, /impl Drop for DeepSeekSecret[\s\S]*?self\.0\.zeroize\(\)/);
assert.match(workspaceKernel, /workspace_write_probe/);
assert.match(workspaceKernel, /remove_file/);
assert.match(workspaceKernel, /ManagedDirectoryEscape/);
assert.match(commands, /workspace_readiness_projection_from_setup_error/);

console.log("onboarding readiness tests passed");

function commandInvocationBlock(source, command) {
  const start = source.indexOf(`"${command}"`);
  if (start < 0) {
    return "";
  }
  const end = source.indexOf("});", start);
  return source.slice(start, end < 0 ? start + 1_500 : end + 3);
}

function rustFunctionBlock(source, marker) {
  const start = source.indexOf(marker);
  assert.ok(start >= 0, `expected Rust function ${marker}`);
  const nextCommand = source.indexOf("#[tauri::command]", start + marker.length);
  return source.slice(start, nextCommand < 0 ? source.length : nextCommand);
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
