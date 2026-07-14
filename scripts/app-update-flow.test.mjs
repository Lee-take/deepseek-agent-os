#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [app, toolRuntime, main, commandAdapter] = await Promise.all([
  readFile("apps/desktop/src/App.tsx", "utf8"),
  readFile("apps/desktop/src-tauri/src/kernel/tool_runtime.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/main.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/app_update_commands.rs", "utf8"),
]);

assert.match(main, /mod app_update_commands;/);
assert.match(
  main,
  /use app_update_commands::\{check_app_update, download_app_update, install_app_update\};/,
);
const handlerStart = main.indexOf("tauri::generate_handler![");
const handlerEnd = main.indexOf("]);", handlerStart);
const handler = main.slice(handlerStart, handlerEnd);
assert.match(handler, /check_app_update,/);
assert.match(handler, /download_app_update,/);
assert.match(handler, /install_app_update,/);
assert.match(commandAdapter, /pub fn check_app_update\(\)/);
assert.match(commandAdapter, /pub fn download_app_update\(\)/);
assert.match(commandAdapter, /installer_path: String/);

const startupStart = app.indexOf('void invoke<FoundationState>("get_foundation_state")');
const startupEnd = app.indexOf(
  'void invoke<DeepSeekCredentialStatus>("get_deepseek_credential_status")',
  startupStart,
);
const startup = app.slice(startupStart, startupEnd);
assert.match(startup, /invoke<AppUpdateStatus>\("check_app_update"\)/);
assert.match(startup, /void downloadAvailableAppUpdate\(status\)/);
assert.doesNotMatch(startup, /invokeAgentTool/);

const downloadStart = app.indexOf("async function downloadAvailableAppUpdate");
const installStart = app.indexOf("const installAvailableAppUpdate", downloadStart);
const download = app.slice(downloadStart, installStart);
assert.match(download, /invoke<AppUpdateDownloadResult>\("download_app_update"\)/);
assert.doesNotMatch(download, /invokeAgentTool/);

const installEnd = app.indexOf("const loadMemoryRecords", installStart);
const install = app.slice(installStart, installEnd);
assert.match(install, /invoke<AppUpdateInstallResult>\("install_app_update"/);
assert.doesNotMatch(install, /invokeAgentTool/);
assert.doesNotMatch(install, /APP_UPDATE_DOWNLOAD_TOOL_ID/);

assert.doesNotMatch(app, /const APP_UPDATE_CHECK_TOOL_ID/);
assert.doesNotMatch(app, /const APP_UPDATE_DOWNLOAD_TOOL_ID/);
assert.doesNotMatch(app, /const APP_UPDATE_INSTALL_TOOL_ID/);
assert.match(app, /\{downloadedAppUpdateReady \? \(/);

const policyTestStart = toolRuntime.indexOf(
  "fn tool_policy_keeps_update_install_confirmation_mandatory",
);
const policyTestEnd = toolRuntime.indexOf("#[test]", policyTestStart);
const policyTest = toolRuntime.slice(policyTestStart, policyTestEnd);
assert.match(policyTest, /assert_eq!\(install\.policy_decision, PolicyDecision::Ask\)/);

console.log("app update flow tests passed");
