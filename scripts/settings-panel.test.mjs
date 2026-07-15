#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const appSourceUrl = new URL("../apps/desktop/src/App.tsx", import.meta.url);
const i18nSourceUrl = new URL("../apps/desktop/src/i18n.ts", import.meta.url);
const stylesSourceUrl = new URL("../apps/desktop/src/styles.css", import.meta.url);
const settingsModuleUrl = new URL("../apps/desktop/src/settingsPanel.ts", import.meta.url);
const tauriDefaultCapabilityUrl = new URL(
  "../apps/desktop/src-tauri/capabilities/default.json",
  import.meta.url,
);

const {
  deepSeekApiKeyCandidates,
  settingsPanelItems,
  shouldExposePluginsSidebarEntry,
} = await import(settingsModuleUrl);

test("settings panel exposes only the ordinary user configuration items", () => {
  assert.deepEqual(
    settingsPanelItems.map((item) => item.id),
    [
      "deepseek_api_key",
      "deepseek_fallback_api_key",
      "deepseek_model",
      "deepseek_thinking",
      "interface_style",
      "soul_profile",
      "workspace_directory",
      "deepseek_balance",
    ],
  );

  assert.deepEqual(
    settingsPanelItems.map((item) => item.control),
    [
      "password",
      "password",
      "select",
      "select",
      "select",
      "modal_button",
      "directory_picker",
      "balance_reader",
    ],
  );
});

test("settings panel shows the creator and maintainer attribution", () => {
  const appSource = readFileSync(appSourceUrl, "utf8");
  const i18nSource = readFileSync(i18nSourceUrl, "utf8");
  const styles = readFileSync(stylesSourceUrl, "utf8");

  assert.match(appSource, /className="product-attribution"/);
  assert.match(appSource, /copy\.settingsPanel\.attribution/);
  assert.match(i18nSource, /attribution:\s*"DS Agent 由 Lee take 创建并维护。"/);
  assert.match(
    i18nSource,
    /attribution:\s*"DS Agent is created and maintained by Lee take\."/,
  );
  assert.match(styles, /\.product-attribution\s*\{/);
});

test("model selector uses ordinary model type language and concise options", () => {
  const i18nSource = readFileSync(i18nSourceUrl, "utf8");

  assert.match(i18nSource, /modelRoute:\s*"模型类型"/);
  assert.doesNotMatch(i18nSource, /模型路线/);
  assert.match(i18nSource, /auto:\s*"DeepSeek 自动"/);
  assert.match(i18nSource, /flash:\s*"Flash"/);
  assert.match(i18nSource, /pro:\s*"Pro"/);
  assert.doesNotMatch(i18nSource, /DeepSeek 快速|DeepSeek 专业|DeepSeek Flash|DeepSeek Pro/);
});

test("interface style defaults to porcelain and hides the dark default route", () => {
  const appSource = readFileSync(appSourceUrl, "utf8");
  const i18nSource = readFileSync(i18nSourceUrl, "utf8");

  assert.match(appSource, /return "porcelain";/);
  assert.doesNotMatch(appSource, /copy\.themeOptions\.deep/);
  assert.doesNotMatch(i18nSource, /深色默认|Deep default/);
});

test("default access mode gives local computer full control", () => {
  const appSource = readFileSync(appSourceUrl, "utf8");

  assert.match(appSource, /access_mode:\s*"full_access"/);
  assert.match(appSource, /computer_control:\s*"local_windows_input_control"/);
});

test("workspace directory setting auto-saves without a separate save command", () => {
  const workspaceItem = settingsPanelItems.find((item) => item.id === "workspace_directory");
  const appSource = readFileSync(appSourceUrl, "utf8");

  assert.equal(workspaceItem?.autoSaveOnChange, true);
  assert.match(appSource, /chooseLocalDirectory\(\{ autoSave: true \}\)/);
  assert.doesNotMatch(appSource, /copy\.settingsPanel\.saveWorkspace/);
});

test("primary DeepSeek API key shows a masked configured placeholder", () => {
  const appSource = readFileSync(appSourceUrl, "utf8");
  const i18nSource = readFileSync(i18nSourceUrl, "utf8");

  assert.match(i18nSource, /apiKeyConfiguredPlaceholder:\s*"••••••••••••••••（已配置）"/);
  assert.match(
    appSource,
    /primaryDeepSeekApiKeyPlaceholder\s*=\s*sessionDeepSeekApiKey\s*\?\s*copy\.settingsPanel\.apiKeyPlaceholder\s*:\s*deepSeekCredentialStatus\.api_key_configured\s*\?\s*copy\.settingsPanel\.apiKeyConfiguredPlaceholder\s*:\s*copy\.settingsPanel\.apiKeyPlaceholder/,
  );
  assert.match(appSource, /placeholder=\{primaryDeepSeekApiKeyPlaceholder\}/);
  assert.match(appSource, /placeholder=\{copy\.settingsPanel\.fallbackApiKeyPlaceholder\}/);
});

test("verified DeepSeek API keys show a compact green check indicator", () => {
  const appSource = readFileSync(appSourceUrl, "utf8");
  const i18nSource = readFileSync(i18nSourceUrl, "utf8");
  const styles = readFileSync(stylesSourceUrl, "utf8");

  assert.match(i18nSource, /apiKeyReady:\s*"API key 已通过启动检测"/);
  assert.match(
    appSource,
    /const primaryDeepSeekApiKeyReady\s*=\s*deepSeekCredentialStatus\.chat_completion_ready;/,
  );
  assert.match(appSource, /className="api-key-input-row"/);
  assert.match(appSource, /primaryDeepSeekApiKeyReady \? \(/);
  assert.match(appSource, /className="api-key-ready-indicator"/);
  assert.match(appSource, /aria-label=\{copy\.settingsPanel\.apiKeyReady\}/);
  assert.match(styles, /--success:\s*#[0-9a-fA-F]{6};/);
  assert.match(styles, /\.api-key-input-row[\s\S]*grid-template-columns:\s*minmax\(0,\s*1fr\)\s*18px;/);
  assert.match(styles, /\.api-key-ready-indicator[\s\S]*color:\s*var\(--success\);/);
});

test("compact settings controls use smaller typography than full setup forms", () => {
  const styles = readFileSync(stylesSourceUrl, "utf8");

  assert.match(
    styles,
    /\.user-settings-controls\s+(?:input,\s*)?\.user-settings-controls\s+select[\s\S]*font-size:\s*13px;/,
  );
  assert.match(styles, /\.compact-settings-form\s+\.setup-field\s+button[\s\S]*min-height:\s*32px;/);
});

test("settings panel exposes Soul as a modal button with annotated editor", () => {
  const soulItem = settingsPanelItems.find((item) => item.id === "soul_profile");
  const appSource = readFileSync(appSourceUrl, "utf8");
  const i18nSource = readFileSync(i18nSourceUrl, "utf8");
  const styles = readFileSync(stylesSourceUrl, "utf8");

  assert.equal(soulItem?.control, "modal_button");
  assert.match(appSource, /invoke<AgentSoulProfileState>\("get_agent_soul_profile"\)/);
  assert.match(appSource, /invoke<AgentSoulProfileState>\("save_agent_soul_profile",\s*\{/);
  assert.match(appSource, /const \[soulProfileModalOpen,\s*setSoulProfileModalOpen\]/);
  assert.match(appSource, /onClick=\{\(\) => setSoulProfileModalOpen\(true\)\}/);
  assert.match(appSource, /role="dialog"[\s\S]*aria-labelledby="soul-profile-modal-title"/);
  assert.match(appSource, /className="soul-profile-guide"/);
  assert.match(appSource, /copy\.settingsPanel\.soulProfileGuides\.map/);
  assert.match(appSource, /value=\{soulProfileDraft\}/);
  assert.match(appSource, /aria-label=\{copy\.settingsPanel\.soulProfile\}/);
  const inlineSoulSettings =
    appSource.match(
      /<div className="soul-profile-settings">[\s\S]*?<div className="setup-form compact-settings-form">/,
    )?.[0] ?? "";
  assert.match(inlineSoulSettings, /copy\.settingsPanel\.soulProfile/);
  assert.doesNotMatch(inlineSoulSettings, /<textarea/);
  assert.doesNotMatch(
    inlineSoulSettings,
    /soulProfileState\.summary_lines|soulProfileNotice|soulProfileError|soulProfileExists|soulProfileTemplate|soulProfileSummary/,
  );
  assert.match(i18nSource, /soulProfile:\s*"Soul"/);
  assert.match(i18nSource, /soulProfileModalTitle:\s*"Soul Profile"/);
  assert.match(i18nSource, /soulProfileGuides:\s*\[/);
  assert.match(i18nSource, /soulProfileSave:\s*"Save Soul Profile"/);
  assert.match(styles, /\.soul-profile-modal/);
  assert.match(
    styles,
    /\.soul-profile-modal \.setup-modal-actions \{[\s\S]*position:\s*sticky/,
  );
  assert.match(styles, /\.soul-profile-guide/);
});

test("new conversations keep Soul bootstrap outside compressed chat history", () => {
  const appSource = readFileSync(appSourceUrl, "utf8");

  assert.match(appSource, /soul_profile_bootstrap:\s*string \| null/);
  assert.match(appSource, /function buildAgentSoulBootstrapContextSection/);
  assert.match(appSource, /const loadSoulProfileStateForBootstrap = async/);
  assert.match(
    appSource,
    /if \(priorMessages\.length === 0\) \{[\s\S]*?await loadSoulProfileStateForBootstrap\(\)/,
  );
  assert.match(appSource, /const refreshSoulProfileAfterAgentResponse = async/);
  assert.match(appSource, /response\.soul_profile_update\?\.status !== "applied"/);
  assert.match(appSource, /await refreshSoulProfileAfterAgentResponse\(response\)/);
  assert.match(
    appSource,
    /currentConversations\.map\(\(conversation\) => \(\{[\s\S]*?soul_profile_bootstrap: soulProfileBootstrap/,
  );
  assert.match(appSource, /soul_profile_update: response\.soul_profile_update/);
  assert.match(
    appSource,
    /function buildAgentConversationContextPrompt\(\s*prompt: string,\s*messages: AgentConversationMessage\[\],\s*soulProfileBootstrap: string \| null = null,/,
  );
  assert.match(appSource, /messages\.length === 0[\s\S]*soulBootstrapSection/);
  assert.match(appSource, /if \(soulBootstrapSection && shouldCompress\)/);
  assert.match(
    appSource,
    /createEmptyAgentConversation\(\s*agentSoulProfileBootstrapFromState\(soulProfileState\),?\s*\)/,
  );
});

test("plugins are exposed as a conservative local skill registry entry", () => {
  assert.equal(shouldExposePluginsSidebarEntry(), true);
});

test("workspace directory picker has Tauri dialog open permission", () => {
  const capability = JSON.parse(readFileSync(tauriDefaultCapabilityUrl, "utf8"));
  const permissions = capability.permissions ?? [];

  assert.equal(capability.windows.includes("main"), true);
  assert.equal(
    permissions.includes("dialog:default") || permissions.includes("dialog:allow-open"),
    true,
  );
});

test("DeepSeek API key candidates trim blanks and keep fallback order", () => {
  assert.deepEqual(deepSeekApiKeyCandidates(" primary ", " fallback "), [
    "primary",
    "fallback",
  ]);
  assert.deepEqual(deepSeekApiKeyCandidates("same", " same "), ["same"]);
  assert.deepEqual(deepSeekApiKeyCandidates("", " fallback "), ["fallback"]);
});
