#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [app, toolRuntime, main, commandAdapter, appUpdateKernel] = await Promise.all([
  readFile("apps/desktop/src/App.tsx", "utf8"),
  readFile("apps/desktop/src-tauri/src/kernel/tool_runtime.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/main.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/app_update_commands.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/kernel/app_update.rs", "utf8"),
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
assert.match(commandAdapter, /download_receipt: String/);
assert.doesNotMatch(commandAdapter, /installer_path: String/);

const startupStart = app.indexOf('void invoke<FoundationState>("get_foundation_state")');
const startupEnd = app.indexOf(
  'void invoke<OnboardingReadinessProjection>("get_onboarding_readiness")',
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
assert.doesNotMatch(download, /installer_path/);
assert.match(download, /\^dsur1\\\.\[0-9a-f\]\{32\}\\\.\[0-9\]\+\$/);
assert.match(download, /\^\[0-9a-f\]\{64\}\$/);
assert.match(download, /Number\.isSafeInteger\(result\.byte_size\)/);

const installEnd = app.indexOf("const loadMemoryRecords", installStart);
const install = app.slice(installStart, installEnd);
assert.match(install, /invoke<AppUpdateInstallResult>\("install_app_update"/);
assert.match(install, /downloadReceipt: downloadedAppUpdate\.download_receipt/);
assert.doesNotMatch(install, /installerPath|installer_path/);
assert.doesNotMatch(install, /invokeAgentTool/);
assert.doesNotMatch(install, /APP_UPDATE_DOWNLOAD_TOOL_ID/);

assert.doesNotMatch(app, /const APP_UPDATE_CHECK_TOOL_ID/);
assert.doesNotMatch(app, /const APP_UPDATE_DOWNLOAD_TOOL_ID/);
assert.doesNotMatch(app, /const APP_UPDATE_INSTALL_TOOL_ID/);
assert.match(app, /\{downloadedAppUpdateReady \? \(/);

assert.match(appUpdateKernel, /redirect\(reqwest::redirect::Policy::none\(\)\)/);
assert.match(appUpdateKernel, /APP_UPDATE_MAX_BYTES/);
assert.match(appUpdateKernel, /download_receipt/);
assert.match(appUpdateKernel, /sha256/);
assert.match(appUpdateKernel, /byte_size/);
assert.doesNotMatch(appUpdateKernel, /pub installer_path: String/);
assert.doesNotMatch(appUpdateKernel, /pub\(crate\) fn schedule_install\(installer_path/);

const policyTestStart = toolRuntime.indexOf(
  "fn tool_policy_keeps_update_install_confirmation_mandatory",
);
const policyTestEnd = toolRuntime.indexOf("#[test]", policyTestStart);
const policyTest = toolRuntime.slice(policyTestStart, policyTestEnd);
assert.match(policyTest, /assert_eq!\(install\.policy_decision, PolicyDecision::Ask\)/);

console.log("app update flow tests passed");
