#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, statSync } from "node:fs";
import path from "node:path";

const expectedVersion = "1.0.1";
const maxSourceFileBytes = 2 * 1024 * 1024;
const binaryReleaseExtensions = new Set([
  ".appimage",
  ".deb",
  ".dmg",
  ".dll",
  ".dylib",
  ".exe",
  ".msi",
  ".msix",
  ".node",
  ".nupkg",
  ".pdb",
  ".pkg",
  ".rpm",
  ".so",
]);
const releaseArchiveExtensions = new Set([".7z", ".rar", ".tar", ".tgz", ".zip"]);
const localReleaseArtifactExtensions = new Set([
  ".log",
  ".sqlite3",
  ".sqlite3-journal",
  ".sqlite3-shm",
  ".sqlite3-wal",
  ".tmp",
]);
const localCredentialArtifactExtensions = new Set([
  ".pem",
  ".key",
  ".pfx",
  ".p12",
  ".crt",
  ".cer",
]);
const generatedDirectorySegments = new Set(["node_modules", "dist", "target"]);
const allowedSourceBinaryFiles = new Set([
  "apps/desktop/public/ds-agent-mark.png",
  "apps/desktop/src-tauri/icons/icon.ico",
  "docs/assets/ds-agent-office-workflow.png",
  "docs/assets/ds-agent-social-preview.png",
]);
const requiredDocs = [
  "README.md",
  "README.zh-CN.md",
  "docs/INSTALLATION.md",
  "docs/RELEASE_NOTES_v1.0.1.md",
  "docs/RELEASE_NOTES_v1.0.0.md",
  "docs/DS_AGENT_V1_COMPLETION_AUDIT.md",
  "docs/RELEASE_NOTES_v0.9.0.md",
  "docs/RELEASE_NOTES_v0.8.0.md",
  "docs/RELEASE_NOTES_v0.8.0-rc.1.md",
  "docs/RELEASE_NOTES_v0.5.0.md",
  "docs/RELEASE_NOTES_v0.4.1.md",
  "docs/RELEASE_NOTES_v0.4.0.md",
  "docs/RELEASE_NOTES_v0.3.0.md",
  "docs/RELEASE_NOTES_v0.2.3.md",
  "docs/RELEASE_NOTES_v0.2.2.md",
  "docs/RELEASE_NOTES_v0.2.1.md",
  "docs/RELEASE_NOTES_v0.2.0.md",
  "docs/RELEASE_NOTES_v0.1.2.md",
  "docs/RELEASE_NOTES_v0.1.0.md",
];
const publicReleaseCopyFiles = [
  ".env.example",
  ".github/ISSUE_TEMPLATE/bug_report.yml",
  ".github/ISSUE_TEMPLATE/deepseek_compatibility.yml",
  ".github/pull_request_template.md",
  "CONTRIBUTING.md",
  "README.md",
  "README.zh-CN.md",
  "SECURITY.md",
  "apps/desktop/index.html",
  "apps/desktop/package.json",
  "docs/INSTALLATION.md",
  "docs/OPEN_SOURCE_RELEASE.md",
  "docs/RELEASE_NOTES_v1.0.1.md",
  "docs/RELEASE_NOTES_v1.0.0.md",
  "docs/DS_AGENT_V1_COMPLETION_AUDIT.md",
  "docs/RELEASE_NOTES_v0.9.0.md",
  "docs/RELEASE_NOTES_v0.8.0.md",
  "docs/RELEASE_NOTES_v0.8.0-rc.1.md",
  "docs/RELEASE_NOTES_v0.5.0.md",
  "docs/RELEASE_NOTES_v0.4.1.md",
  "docs/RELEASE_NOTES_v0.4.0.md",
  "docs/RELEASE_NOTES_v0.3.0.md",
  "docs/RELEASE_NOTES_v0.2.3.md",
  "docs/RELEASE_NOTES_v0.2.2.md",
  "docs/RELEASE_NOTES_v0.2.1.md",
  "docs/RELEASE_NOTES_v0.2.0.md",
  "docs/RELEASE_NOTES_v0.1.2.md",
  "docs/RELEASE_NOTES_v0.1.0.md",
  "docs/RELEASE_NOTES_v0.1-alpha.md",
  "package.json",
];
const detailedValidationDocs = requiredDocs.filter((docPath) => docPath.startsWith("docs/"));
const publicUiCopyFiles = ["apps/desktop/src/i18n.ts"];
const publicPeerProductTerms = [
  "ChatGPT",
  "Claude",
  "Claude Code",
  "CodeWhale",
  "Codex",
  "OpenAI",
  "OpenClaw",
];
const staleChineseUiBridgePhrases = [
  "外部代理桥接",
  "外部 HTTP 桥接",
  "标准输入输出旁路进程（暂缓）",
  "来源适配器执行",
  "原生桥接执行",
  "桥接式屏幕像素读取",
  "外部桥接屏幕像素读取",
  "本地 Windows 屏幕像素读取",
  "本地 macOS 屏幕像素读取",
  "桥接式鼠标键盘控制",
  "外部桥接鼠标键盘控制",
  "屏幕后端状态",
  "控制后端状态",
  "外部桥接运行时",
  "外部 Agent bridge",
  "外部 HTTP bridge",
  "stdio sidecar（暂缓）",
  "Bridge 式",
  "外部 Bridge",
  "外部 bridge",
  "需 bridge",
  "无需 bridge",
  "兼容桥接屏幕查看",
  "已配置桥接屏幕查看",
  "兼容桥接鼠标键盘控制",
  "已配置桥接鼠标键盘控制",
  "仅权限与审计",
  "Endpoint 已配置",
  "Endpoint 未配置",
  "Transport 未选择",
  "本地输入后端",
];
const staleEnglishUiBridgePhrases = [
  "External agent bridge",
  "External HTTP bridge",
  "stdio sidecar (deferred)",
  "Native bridge contract",
  "Source adapter execution",
  "Bridge-style screen pixel capture",
  "External bridge screen pixel capture",
  "Local Windows screen pixel capture",
  "Local macOS screen pixel capture",
  "Bridge-style mouse and keyboard control",
  "External bridge mouse and keyboard control",
  "Screen backend status",
  "Control backend status",
  "External bridge runtime",
  "Bridge required",
  "Bridge not required",
  "Endpoint configured",
  "Endpoint missing",
  "Transport missing",
  "Legacy bridge screen inspection",
  "Configured bridge screen inspection",
  "Legacy bridge mouse and keyboard control",
  "Configured bridge mouse and keyboard control",
  "Permission and audit only",
  "local input backend",
];
const staleChineseUiNetworkSearchPhrases = [
  "联网搜索边界",
  "NetworkSearch 来源模型",
  "免费 Web 来源模型",
  "大模型原生 NetworkSearch",
  "来源支撑 NetworkSearch 模型",
  "免费来源适配器执行真实搜索",
  "来源型 HTTP 适配器",
  "来源适配器",
  "已创建 NetworkSearch 待审批请求",
  "NetworkSearch 已执行",
  "NetworkSearch 未完成",
  "NetworkSearch 请求失败",
  "需要 NetworkSearch 来源模型",
  "请先选择 NetworkSearch 来源模型",
  "当前 NetworkSearch 路线尚未启用真实搜索",
];
const staleEnglishUiNetworkSearchPhrases = [
  "NetworkSearch source model",
  "Native large-model NetworkSearch",
  "Source-backed NetworkSearch model",
  "Network Search Boundary",
  "NetworkSearch approval request",
  "NetworkSearch ran",
  "NetworkSearch did not complete",
  "NetworkSearch request failed",
  "NetworkSearch source model required",
  "Choose a NetworkSearch source model first",
  "current NetworkSearch route",
  "free source adapter for live search",
  "source-backed HTTP adapter",
  "local-browser and aggregator presets currently share",
];
const staleChineseUiDrivePhrases = [
  'drive: "Drive"',
  "读取网盘",
  "写入网盘",
  "网盘文件",
  "导出成果到网盘",
  "DriveWrite 待审批请求",
  "Drive 本地文件夹读取",
  "DriveRead 待审批请求",
  "DriveRead 已读取",
  "DriveRead 未完成",
  "DriveRead 请求失败",
  "Drive 导出工作包",
  "DriveWrite 已将",
  "DriveWrite 未完成",
  "DriveWrite 请求失败",
];
const staleEnglishUiDrivePhrases = [
  "A DriveWrite approval request was created. Approve it",
  "Read selected cloud-drive files and folders.",
  "Upload or export artifacts to cloud drive.",
  "Drive Read Local Folder",
  "DriveRead approval request",
  "DriveRead read",
  "DriveRead did not complete",
  "DriveRead request failed",
  "Drive Export Package",
  "DriveWrite approval request",
  "DriveWrite exported",
  "DriveWrite did not complete",
  "DriveWrite request failed",
];
const staleChineseUiFileWritePhrases = [
  "文件写入边界",
  "FileWrite 待审批请求",
  "FileWrite 未执行",
  "FileWrite 请求失败",
];
const staleEnglishUiFileWritePhrases = [
  "File Write Boundary",
  "FileWrite approval request",
  "FileWrite did not execute",
  "FileWrite request failed",
];
const staleChineseUiTerminalWritePhrases = [
  "终端写入边界",
  "TerminalWrite 待审批请求",
  "TerminalWrite v1 仅记录审批与审计",
  "终端写入仅记录审批与审计",
  "TerminalWrite 请求失败",
];
const staleEnglishUiTerminalWritePhrases = [
  "Terminal Write Boundary",
  "TerminalWrite approval request",
  "TerminalWrite v1 only records approval and audit evidence",
  "Terminal write only records approval and audit evidence",
  "TerminalWrite request failed",
];
const staleChineseUiBrowserSubmitPhrases = [
  "浏览器提交边界",
  "BrowserSubmit 待审批请求",
  "BrowserSubmit v1 仅记录审批与审计",
  "浏览器提交仅记录审批与审计",
  "BrowserSubmit 请求失败",
];
const staleEnglishUiBrowserSubmitPhrases = [
  "Browser Submit Boundary",
  "BrowserSubmit approval request",
  "BrowserSubmit v1 only records approval and audit evidence",
  "Browser form submission only records approval and audit evidence",
  "BrowserSubmit request failed",
];
const staleChineseUiEmailPhrases = [
  "邮件发送边界",
  "EmailSend 待审批请求",
  "EmailSend v1 仅记录审批与审计",
  "邮件发送仅记录审批与审计",
  "EmailSend 请求失败",
  "邮件草稿边界",
  "EmailDraft 待审批请求",
  "EmailDraft v1 仅记录审批与审计",
  "邮件草稿仅记录审批与审计",
  "EmailDraft 请求失败",
  "邮件读取边界",
  "EmailRead 待审批请求",
  "EmailRead v1 仅记录审批与审计",
  "邮件读取仅记录审批与审计",
  "EmailRead 请求失败",
];
const staleEnglishUiEmailPhrases = [
  "Email Send Boundary",
  "EmailSend approval request",
  "EmailSend v1 only records approval and audit evidence",
  "Email send only records approval and audit evidence",
  "EmailSend request failed",
  "Email Draft Boundary",
  "EmailDraft approval request",
  "EmailDraft v1 only records approval and audit evidence",
  "Email draft only records approval and audit evidence",
  "EmailDraft request failed",
  "Email Read Boundary",
  "EmailRead approval request",
  "EmailRead v1 only records approval and audit evidence",
  "Email read only records approval and audit evidence",
  "EmailRead request failed",
];
const staleChineseUiComputerControlPhrases = [
  "桌面控制边界",
  "ComputerControl 待审批请求",
  "ComputerControl 已执行",
  "ComputerControl 未执行",
  "ComputerControl 请求失败",
  "电脑控制已执行，并已记录审批与审计",
];
const staleChineseUiScreenshotPhrases = [
  "屏幕截图边界",
];
const staleEnglishUiScreenshotPhrases = [
  "Screenshot Boundary",
];
const staleEnglishUiComputerControlPhrases = [
  "Computer Control Boundary",
  "Local Computer Control Unlock",
  "ComputerControl approval request",
  "ComputerControl executed",
  "ComputerControl was not executed",
  "ComputerControl request failed",
  "Computer control executed and recorded approval and audit evidence",
];
const localOnlyContinuationFiles = [
  "SESSION_HANDOFF.md",
  "PROJECT_CONTEXT.md",
  "DECISIONS.md",
  "docs/superpowers/",
];
const requiredGovernanceDocs = [
  "LICENSE",
  "SECURITY.md",
  "CONTRIBUTING.md",
  "docs/OPEN_SOURCE_RELEASE.md",
];
const requiredGitHubHygieneFiles = [
  ".github/pull_request_template.md",
  ".github/ISSUE_TEMPLATE/bug_report.yml",
  ".github/ISSUE_TEMPLATE/deepseek_compatibility.yml",
  ".github/ISSUE_TEMPLATE/config.yml",
  ".github/workflows/ci.yml",
];
const smokeEvidenceDir = "docs/templates/operations-briefing-smoke-evidence";
const seedEvidenceDir = "docs/templates/operations-briefing-evidence";
const smokeEvidenceSafetyDisclaimer = "SMOKE SAMPLE evidence for local verification only";
const smokeEvidenceReplacementWarning = "Replace before operational use";
const seedEvidenceOperatorTemplatePhrase = "blank operator templates";
const requiredSmokeEvidenceFiles = [
  "revenue.md",
  "guest-experience.md",
  "risk-and-compliance.md",
  "action-followups.md",
];
const requiredSeedEvidenceFiles = [
  "revenue.md",
  "guest-experience.md",
  "risk-and-compliance.md",
  "action-followups.md",
];
const requiredPackageScripts = new Map([
  ["stage:webview2-loader", "node scripts/stage-webview2-loader.mjs"],
  ["test:deepseek", "node scripts/deepseek-smoke.mjs"],
  ["test:deepseek:briefing", "node scripts/deepseek-operations-briefing-smoke.mjs"],
  ["test:builtin-plugins", "node scripts/validate-builtin-plugins.mjs"],
  ["test:frontend-starter-prompts", "node scripts/frontend-starter-prompts-smoke.mjs"],
  ["test:windows-local", "node scripts/windows-local-smoke.mjs"],
  ["test:windows-installed-ui", "node scripts/windows-installed-ui-smoke.mjs"],
  ["test:release-source", "node scripts/release-source-check.mjs"],
  ["test:release-local", "node scripts/release-local-check.mjs"],
]);
const requiredGitignoreEntries = [
  "node_modules/",
  "dist/",
  "target/",
  "tmp/",
  ".DS_Store",
  ".env",
  ".env.*",
  "!.env.example",
  ...localOnlyContinuationFiles,
  "kernel-events.sqlite3",
  "local-directories.json",
  "computer-screenshots/",
  "operations-briefing-*.json",
  "operations-briefing-*.md",
  "operations-briefing-*.html",
  "operations-briefing-*.pdf",
  "deepseek-agent-os-work-package-*.json",
  "*.log",
  "*.tmp",
  "*.sqlite3",
  "*.sqlite3-wal",
  "*.sqlite3-shm",
  "*.sqlite3-journal",
  "*.pem",
  "*.key",
  "*.pfx",
  "*.p12",
  "*.crt",
  "*.cer",
  "*.pdb",
  "*.so",
  "*.dylib",
  "*.node",
  "*.zip",
  "*.7z",
  "*.rar",
  "*.tar",
  "*.tgz",
  "_reference_repos/",
  "apps/desktop/src-tauri/.tauri/",
  "apps/desktop/src-tauri/gen/",
  "apps/desktop/src-tauri/generated/",
  "apps/desktop/src-tauri/target/",
];
const args = process.argv.slice(2).filter((arg) => arg !== "--");
const expectedPackageManager = "pnpm@9.15.9";

validateArgs(args, new Set(["--help"]), "test:release-source");
if (args.includes("--help")) {
  console.log("Usage: pnpm test:release-source");
  process.exit(0);
}

const failures = [];
const checks = [];

checkJsonField("package.json", "version", expectedVersion);
checkJsonField("package.json", "author", "Lee take");
checkJsonField("package.json", "license", "Apache-2.0");
checkJsonField("package.json", "packageManager", expectedPackageManager);
checkJsonField("apps/desktop/package.json", "version", expectedVersion);
checkJsonField("apps/desktop/package.json", "author", "Lee take");
checkJsonField("apps/desktop/package.json", "license", "Apache-2.0");
checkJsonField("apps/desktop/src-tauri/tauri.conf.json", "version", expectedVersion);
checkJsonField("apps/desktop/src-tauri/tauri.conf.json", "productName", "DS Agent");
checkJsonField("apps/desktop/src-tauri/tauri.conf.json", "mainBinaryName", "ds-agent");
checkJsonField(
  "apps/desktop/src-tauri/tauri.conf.json",
  "identifier",
  "ai.deepseek-agent-os.desktop",
);
checkJsonField("apps/desktop/src-tauri/tauri.conf.json", "bundle.active", false);
checkJsonField("apps/desktop/src-tauri/tauri.conf.json", "bundle.publisher", "Lee take");
checkJsonField(
  "apps/desktop/src-tauri/tauri.conf.json",
  "bundle.copyright",
  "Copyright © 2026 Lee take",
);
checkJsonField(
  "apps/desktop/src-tauri/tauri.windows.conf.json",
  "build.beforeBuildCommand",
  "npx pnpm@9.15.9 build && npx pnpm@9.15.9 stage:webview2-loader",
);
checkJsonField(
  "apps/desktop/src-tauri/tauri.windows.conf.json",
  "build.beforeBundleCommand",
  "npx pnpm@9.15.9 stage:webview2-loader",
);
checkJsonField("apps/desktop/src-tauri/tauri.windows.conf.json", "bundle.active", true);
checkJsonField("apps/desktop/src-tauri/tauri.windows.conf.json", "bundle.targets.0", "nsis");
checkJsonField(
  "apps/desktop/src-tauri/tauri.windows.conf.json",
  "bundle.windows.webviewInstallMode.type",
  "embedBootstrapper",
);
checkJsonField(
  "apps/desktop/src-tauri/tauri.windows.conf.json",
  "bundle.windows.webviewInstallMode.silent",
  true,
);
checkJsonField(
  "apps/desktop/src-tauri/tauri.windows.conf.json",
  "bundle.resources.generated/windows/WebView2Loader.dll",
  "WebView2Loader.dll",
);
checkWindowsAppIconPackaging();
checkCargoLicense();
checkPackageScripts();
checkSecretScanHygiene();
checkRootWorkspaceScripts();
checkSmokeScriptReleaseLabels();
checkExternalBridgeUserVisibleErrors();
checkLocalReleaseHelperSelfTests();
checkPackageManagerBaseline();
checkGovernanceDocs();
checkContributingPolicyStatus();
checkOpenSourceReleaseStatus();
checkEnvironmentHygiene();
checkGitHubReleaseHygiene();
checkRequiredDocs();
checkPublicReleaseCopyPositioning();
checkAppFallbackCopyPositioning();
checkRuntimeWebSearchCopyPositioning();
checkRuntimeComputerUseCopyPositioning();
checkRuntimeLocalDesktopRouteFailureCopyPositioning();
checkChatFirstRunStatusUi();
checkChatFirstCenterWorkbenchUi();
checkFrontendStarterPromptsSmoke();
checkHistoricalReleaseNotes();
checkWindowsValidationStatusDocs();
checkReleaseGateDocs();
checkComputerUseDocs();
checkMemoryStudioDocs();
checkSoulProfileSettingsAndBootstrap();
checkOperationsBriefingArchiveReplayUi();
checkWorkPackageImportPreviewUi();
checkOperationsBriefingSmokeEvidence();
checkOperationsBriefingSeedEvidence();
checkBuiltinPluginPackages();
checkGitignoreEntries();
checkLineEndingPolicy();
checkPublicPeerProductTermGuardSelfTest();
checkPublicUiCopyExtractionSelfTest();
checkCiWriteBoundarySelfTest();
checkSourceFileSizeAndBinarySelfTest();
checkSourceOnlyBlocklistSelfTest();
checkLocalOnlyContinuationSelfTest();
checkSourceOnlyFiles();

if (failures.length > 0) {
  console.error(
    JSON.stringify(
      {
        ok: false,
        release: expectedVersion,
        failures,
        checks,
      },
      null,
      2,
    ),
  );
  process.exit(1);
}

console.log(
  JSON.stringify(
    {
      ok: true,
      release: expectedVersion,
      checks,
    },
    null,
    2,
  ),
);

function checkJsonField(filePath, field, expected) {
  const value = getJsonField(readJson(filePath), field);
  if (value !== expected) {
    failures.push(`${filePath} ${field} must be ${expected}`);
    return;
  }
  checks.push(`${filePath} ${field}`);
}

function getJsonField(json, field) {
  if (!field.includes(".")) {
    return json?.[field];
  }
  return getJsonPathValue(json, field.split("."));
}

function getJsonPathValue(value, segments) {
  if (segments.length === 0) {
    return value;
  }
  if (value === null || typeof value !== "object") {
    return undefined;
  }

  const remainingKey = segments.join(".");
  if (Object.prototype.hasOwnProperty.call(value, remainingKey)) {
    return value[remainingKey];
  }

  const [head, ...tail] = segments;
  if (Array.isArray(value) && /^\d+$/.test(head)) {
    return getJsonPathValue(value[Number(head)], tail);
  }
  if (Object.prototype.hasOwnProperty.call(value, head)) {
    return getJsonPathValue(value[head], tail);
  }
  return undefined;
}

function checkWindowsAppIconPackaging() {
  checkJsonField(
    "apps/desktop/src-tauri/tauri.windows.conf.json",
    "bundle.windows.nsis.installerHooks",
    "nsis/shortcut-icons.nsh",
  );
  checkJsonField(
    "apps/desktop/src-tauri/tauri.windows.conf.json",
    "bundle.resources.icons/icon.ico",
    "ds-agent-icon.ico",
  );

  const mainSource = readText("apps/desktop/src-tauri/src/main.rs");
  checkTextIncludes(
    "apps/desktop/src-tauri/src/main.rs",
    mainSource,
    '#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]',
    "desktop app binary uses Windows GUI subsystem for packaged debug and release builds",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/main.rs",
    mainSource,
    'include_bytes!("../icons/icon.ico")',
    "desktop runtime window icon embeds checked-in app icon",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/main.rs",
    mainSource,
    ".set_icon(",
    "desktop runtime window icon is applied at startup",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/main.rs",
    mainSource,
    "apply_windows_window_icons",
    "desktop runtime window icon applies Windows-specific icon handles",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/main.rs",
    mainSource,
    "WM_SETICON",
    "desktop runtime window icon sets Win32 window icon handles",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/main.rs",
    mainSource,
    "ICON_BIG",
    "desktop runtime window icon sets the large taskbar icon",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/main.rs",
    mainSource,
    "ICON_SMALL2",
    "desktop runtime window icon sets the secondary small icon",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/main.rs",
    mainSource,
    "select_ico_resource",
    "desktop runtime window icon selects an ICO resource frame",
  );

  const hooks = readText("apps/desktop/src-tauri/nsis/shortcut-icons.nsh");
  checkTextIncludes(
    "apps/desktop/src-tauri/nsis/shortcut-icons.nsh",
    hooks,
    "!macro NSIS_HOOK_POSTINSTALL",
    "NSIS postinstall hook refreshes shortcut icons",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/nsis/shortcut-icons.nsh",
    hooks,
    'CreateShortcut "$SMPROGRAMS\\${PRODUCTNAME}.lnk" "$INSTDIR\\${MAINBINARYNAME}.exe" "" "$INSTDIR\\ds-agent-icon.ico" 0',
    "start menu shortcut explicitly uses installed app icon file",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/nsis/shortcut-icons.nsh",
    hooks,
    'CreateShortcut "$SMPROGRAMS\\$AppStartMenuFolder\\${PRODUCTNAME}.lnk" "$INSTDIR\\${MAINBINARYNAME}.exe" "" "$INSTDIR\\ds-agent-icon.ico" 0',
    "start menu folder shortcut explicitly uses installed app icon file",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/nsis/shortcut-icons.nsh",
    hooks,
    'CreateShortcut "$DESKTOP\\${PRODUCTNAME}.lnk" "$INSTDIR\\${MAINBINARYNAME}.exe" "" "$INSTDIR\\ds-agent-icon.ico" 0',
    "desktop shortcut explicitly uses installed app icon file",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/nsis/shortcut-icons.nsh",
    hooks,
    'Delete "$DESKTOP\\${PRODUCTNAME}.lnk"',
    "desktop shortcut is deleted before recreation to avoid stale icon metadata",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/nsis/shortcut-icons.nsh",
    hooks,
    "SHChangeNotify",
    "installer notifies Windows shell after refreshing shortcut icons",
  );
}

function checkRequiredDocs() {
  for (const docPath of requiredDocs) {
    if (!existsSync(docPath)) {
      failures.push(`${docPath} is required for source release notes`);
      continue;
    }
    checks.push(`${docPath} present`);
  }

  const readme = readText("README.md");
  const readmeZh = readText("README.zh-CN.md");
  const collapsedReadme = readme.replace(/\s+/g, " ").trim();
  for (const [phrase, label] of [
    ["One Kernel. Modular capabilities. Verifiable execution.", "English README product promise"],
    ["DS Agent is a local Agent Harness optimized for DeepSeek", "English README DeepSeek-first positioning"],
    ["v1.0.1 stable", "English README formal stable status"],
    ["The key difference is that Memory, Automation, Computer Use, parallel Subagents, and Skills are not isolated plugins", "English README unified Kernel thesis"],
    ["contract-first modular Harness architecture", "English README modular architecture"],
    ["Five core capabilities, one engineering philosophy", "English README five-capability framing"],
    ["Memory and Soul", "English README Memory and Soul capability"],
    ["Durable Automation", "English README durable Automation capability"],
    ["Verified Computer Use", "English README verified Computer Use capability"],
    ["Parallel Subagents", "English README parallel Subagent capability"],
    ["Generated and automatic Skills", "English README generated automatic Skills capability"],
    ["goal → done-when contract → context → plan → permission → execution → evidence → verification → recovery", "English README Loop Engineering contract"],
    ["DeepSeek and DS Agent boundary", "English README model boundary"],
    ["Why Rust", "English README Rust rationale"],
    ["Production Microsoft/Google account registration and live external-write authority remain disabled in v1.0.1", "English README connector authority boundary"],
    ["README.zh-CN.md", "English README language switch"],
    ["Search aliases: DS Agent, DSAgent, dsagent, DeepSeek Agent OS.", "English README searchable aliases"],
  ]) {
    checkTextIncludesCollapsed("README.md", readme, phrase, label);
  }
  for (const [phrase, label] of [
    ["一个 Kernel，模块化能力，统一可信执行。", "Chinese README product promise"],
    ["DS Agent 是专门为 DeepSeek 优化的本地 Agent Harness", "Chinese README DeepSeek-first positioning"],
    ["v1.0.1 正式稳定版", "Chinese README formal stable status"],
    ["真正的差异点是：记忆、自动化执行、Computer Use、Subagent 并行协作和技能", "Chinese README unified Kernel thesis"],
    ["契约优先的模块化 Harness 架构", "Chinese README modular architecture"],
    ["五项核心能力，一套工程理念", "Chinese README five-capability framing"],
    ["记忆与 Soul", "Chinese README Memory and Soul capability"],
    ["持久自动化", "Chinese README durable Automation capability"],
    ["可验证的 Computer Use", "Chinese README verified Computer Use capability"],
    ["Subagent 并行协作", "Chinese README parallel Subagent capability"],
    ["技能生成与自动使用", "Chinese README generated automatic Skills capability"],
    ["目标 → 完成条件 → 上下文 → 规划 → 权限 → 执行 → 证据 → 验证 → 恢复", "Chinese README Loop Engineering contract"],
    ["DeepSeek 与 DS Agent 的工作边界", "Chinese README model boundary"],
    ["为什么使用 Rust", "Chinese README Rust rationale"],
    ["v1.0.1 仍未开放生产 Microsoft/Google 账号注册和真实外部写入权限", "Chinese README connector authority boundary"],
    ["README.md", "Chinese README language switch"],
    ["中文搜索别名：DS Agent、DSAgent、dsagent、DeepSeek Agent OS。", "Chinese README searchable aliases"],
  ]) {
    checkTextIncludesCollapsed("README.zh-CN.md", readmeZh, phrase, label);
  }
  for (const [filePath, content] of [["README.md", readme], ["README.zh-CN.md", readmeZh]]) {
    checkTextDoesNotInclude(filePath, content, "Current source candidate", `${filePath} no stale candidate status`);
    checkTextDoesNotInclude(filePath, content, "Latest published stable: [DS Agent v0.9.0]", `${filePath} no stale v0.9 latest status`);
    checkTextDoesNotInclude(filePath, content, "Two Audiences", `${filePath} no split audience introduction`);
    if (content.split(/\r?\n/).length > 260) {
      failures.push(`${filePath} must remain concise at 260 lines or fewer`);
    } else {
      checks.push(`${filePath} concise line budget`);
    }
  }
  checkTextDoesNotInclude(
    "docs/INSTALLATION.md",
    readText("docs/INSTALLATION.md"),
    "The current stable release remains `v0.5.0`",
    "installation current stable status must not regress to v0.5.0",
  );

  for (const phrase of [
    "BrowserSubmit boundary v1, NetworkSearch source adapter v1, FileRead v1",
    "`EmailSend` and `ComputerControl` approvals",
    "Critical EmailSend and ComputerControl retries",
    "Critical one-shot approvals v1 keeps high-consequence permissions from becoming permanent grants",
    "Non-critical explicit approvals can still be reused",
    "outbound email and computer control permissions are consumed by the next matching tool attempt",
    "Permission grant visibility v1 shows whether approvals are reusable, available once, or already consumed",
    "while preserving the append-only audit history",
    "Approval traceability v1 keeps critical retries linked to the exact approval",
    "Critical outbound email and computer control retries stay traceable",
    "Recent tool output shows the approval record that authorized a tool attempt",
    "without opening developer-facing audit storage",
    "NetworkSearch reports the selected large model's capability",
    "FileRead-backed evidence-folder ingestion",
    "DriveWrite approval loop",
    "FileWrite approval loop",
    "ComputerControl remains visibly approval-gated",
    "ComputerControl structured action v1",
    "one-shot ComputerControl approval",
    "ComputerControl local unlock token v1",
    "unlock ComputerControl for five minutes",
    "NetworkSearch evidence is handled",
    "NetworkSearch route status v1",
    "NetworkSearch source adapter v1",
    "Native NetworkSearch bridge contract v1",
    "NetworkSearch evidence/audit boundary",
    "NetworkSearch route/evidence gate status",
    "Computer Screenshot evidence-ref v1",
    "Computer Screenshot risk-gate v1",
    "Computer Screenshot local capture backend v1",
    "screenshot invocations now use screenshot evidence refs",
    "`xcap` for local screen capture",
    "The Tauri command prefers the user-selected evidence directory",
    "Capability grant state v1",
    "Capability access records now include a derived `grant_state`",
    "append-only event payloads",
    "Approval traceability v1 adds an optional `approval_request_id`",
    "capability invocations",
    "legacy invocations without the field",
    "linked approval request IDs",
    "raw event JSON",
    "Tool backend settings now record the confirmed Phase 2 direction",
    "`FoundationState` and the runtime inspector",
    "native large-model bridge contract",
    "free source-backed adapter before live search can run",
    "otherwise requires a selected source-backed search path before live search can run",
    "Reserved alpha presets disclose when they share the same source-backed path",
    "DeepSeek Chat executor v1 adds an injectable Chat Completions transport",
    "reqwest-backed implementation",
    "Tests use a fake transport",
    "executor errors redact the local API key",
    "DeepSeek cache and telemetry v1 adds an in-memory request cache",
    "append-only, secret-safe telemetry events",
    "The telemetry records a request hash",
    "DeepSeek pricing config v1 stores a manual USD / 1M token price table",
    "never hardcodes public DeepSeek prices",
    "Web search route status v1 makes that product decision visible",
    "selected provider, native/source-model support, selected source model, execution mode, live-network status",
    "source-adapter execution",
    "Source-backed web search path v1 adds an HTTP client",
    "The desktop command requires a selected source model",
    "provider/source-adapter label",
    "share the source-backed HTTP adapter",
    "External web-search bridge contract v1 adds a loopback-only `/network-search` route",
    "Bridge responses must include source URLs",
    "Setup and local directory contract v1",
    "The runtime stores local directory settings under the OS app data directory",
    "hardcoded developer-machine paths",
    "Windows installer baseline v1 adds a platform-specific Tauri config",
    "A macOS `.app`/`.dmg` config is also committed",
    "External bridge runtime readiness v1 makes bridge-routed Computer Use paths explicit before execution",
    "`DEEPSEEK_AGENT_OS_BRIDGE_TRANSPORT=http`",
    "`DEEPSEEK_AGENT_OS_BRIDGE_URL` is configured for loopback HTTP routes",
    "External bridge contract schema v1 defines a transport-neutral JSON contract",
    "Screenshot responses carry display metadata plus PNG base64 rather than local file paths",
    "same structured action strings used by the local executor",
    "External bridge HTTP runtime v1 implements the approved external HTTP runtime",
    "posts `/health`, `/screenshot`, `/control`, and `/network-search`",
    "`stdio` sidecar spawning as deferred hardening work",
    "Screen evidence reference v1 separates the human display label from the durable evidence reference",
    "Successful screen captures now use local evidence refs for audit output",
    "Screen inspection approval gate v1 treats screen pixel capture as a medium-risk Computer Use read",
    "The default `ask_on_risk` access mode now creates a pending approval before screen inspection",
    "`limited_auto` can still auto-run medium-risk reads after policy evaluation",
    "Local screen capture evidence v1 stores approved screenshots as local PNG evidence",
    "The runtime uses the user-selected evidence directory",
    "falls back to OS app data before setup",
    "records a relative evidence ref for the audit trail",
    "Computer Use backend status v1 exposes the selected model-driven backend direction in the runtime inspector",
    "external bridge routes use bridge screen/input contracts",
    "local routes use Windows/macOS screen/input backends",
    "External bridge readiness v1 shows whether a supported external desktop bridge is required, configured on loopback, connected, and reporting the screen, input, or web-search capabilities needed before a tool can run",
    "External bridge message boundary v1 keeps screenshots, input actions, and web search inside the same approval/audit model",
    "Bridge health, screen evidence, control actions, and source-linked web search use a versioned local message boundary",
    "External bridge local HTTP runtime v1 accepts only loopback endpoints",
    "checks bridge health before tool execution",
    "leaves managed sidecar startup for future hardening",
    "Computer tool route selection v1 keeps screen inspection and desktop control on the selected tool route",
    "shows a clear failure when the configured external desktop bridge is unavailable",
    "keeps DeepSeek and custom-model desktop routes on local Windows/macOS backends",
    "Work-package tool readiness v1 now adds a secret-safe `tool_readiness` snapshot",
    "web-search evidence route status",
    "Computer Use backend availability",
    "Computer Use backend availability flags",
    "local-directory setup readiness",
    "model-driven tool strategy",
    "without serializing API key values or user machine paths",
    "calling the live DeepSeek API, capturing pixels during export, or controlling the desktop",
    "Audited desktop input protocol v1 limits real desktop input to a small command set",
    "`click:x,y[,button]`",
    "`hotkey:key+key`",
    "The local backend uses `enigo`",
    "before executing any mouse or keyboard action",
    "Local computer control unlock window v1 adds a second local gate around approved desktop input",
    "At app startup the runtime generates a short in-memory challenge code",
    "approved execution fails before any invocation is recorded",
    "The code is not persisted into events or exported work packages",
    "Computer tool model-aware routing v1 passes the selected large-model provider into screen/control commands",
    "External bridge routes use bridge contract clients",
    "DeepSeek/custom routes use the local Windows/macOS screen and input clients",
    "The first Operations Management workflow is also wired",
    "records an append-only workflow run",
    "DeepSeek Chat executor for model-backed JSON synthesis",
    "deterministic local draft",
    "Exported work packages carry persisted briefing runs for handoff",
    "exported work packages redact source-machine evidence handles before serialization",
    "open CJK font or OS-font discovery strategy",
    "Operations Briefing workflow v1 turns approved local evidence into a management brief",
    "It uses an approved local evidence-folder read to draft a summary, anomaly leads, and action items",
    "falls back to a local draft with a visible warning when model synthesis is unavailable",
    "In that same export boundary, exported work packages redact source-machine evidence handles",
    "PDF v1 stays lightweight and ASCII-safe for this preview",
    "The approved local evidence-template seeding flow uses blank operator templates",
    "Long-term memory edit v1 appends `memory_record.updated` events",
    "Long-term memory delete v1 appends a `memory_record.deleted` tombstone",
    "Older task-record auto memory and legacy event payloads remain readable through default metadata",
    "append-only event records",
    "oversized prompt payloads",
    "The first implementation slice",
    "the kernel declares built-in file, network, browser, email, local-folder, terminal, and Computer Use surfaces",
    "The first adapters let users",
    "approval/audit requests for mutating terminal and browser-form actions",
    "approval/audit requests for email read/draft/send flows",
  ]) {
    if (collapsedReadme.includes(phrase)) {
      failures.push(`README.md Architecture overview must not use internal capability wording: ${phrase}`);
    } else {
      checks.push(`README.md Architecture overview no internal capability wording: ${phrase}`);
    }
  }

  const staleReadmeEnglishToolSurface =
    "Permissioned tool surfaces for file, network, browser, terminal, drive, email, and Computer Use operations.";
  const releaseNotes = readText("docs/RELEASE_NOTES_v0.1.0.md");
  const collapsedReleaseNotes = releaseNotes.replace(/\s+/g, " ").trim();
  if (collapsedReleaseNotes.includes(staleReadmeEnglishToolSurface)) {
    failures.push(
      `docs/RELEASE_NOTES_v0.1.0.md must not describe current tool surfaces with stale drive/email connector wording: ${staleReadmeEnglishToolSurface}`,
    );
  } else {
    checks.push("release notes no stale English drive/email tool wording");
  }
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "Permissioned tool surfaces for file, network, browser, terminal, local-folder read/export, email read/draft/send approval records, and Computer Use operations.",
    "release notes current English tool surface wording",
  );
  const releaseScorecard = readText("docs/V0_1_0_RELEASE_SCORECARD.md");
  for (const phrase of [
    "Status: pending",
    "Target: v0.1.0 Windows formal release",
    "Operations Briefing creates local Markdown, HTML, lightweight PDF, and work-package JSON from sample evidence",
    "Memory remains bounded and auditable",
    "npx pnpm@9.15.9 test:frontend-starter-prompts",
    "npx pnpm@9.15.9 test:release-local -- --require-live-deepseek --include-installed-workflow",
    "Do Not Block v0.1.0",
  ]) {
    checkTextIncludesCollapsed(
      "docs/V0_1_0_RELEASE_SCORECARD.md",
      releaseScorecard,
      phrase,
      `v0.1.0 release scorecard: ${phrase}`,
    );
  }

  const installation = readText("docs/INSTALLATION.md");
  const collapsedInstallation = installation.replace(/\s+/g, " ").trim();
  for (const phrase of [
    "NetworkSearch depends on the selected large model:",
    "If the selected model route has native bridge-backed NetworkSearch available",
    "If the selected model does not provide NetworkSearch",
    "Computer Screenshot and ComputerControl are permissioned Computer Use features.",
    "ComputerControl requires an explicit one-shot approval",
    "EmailRead, EmailDraft, and EmailSend are boundary and approval surfaces",
    "DriveRead and DriveWrite use local folders and local export packages",
    "If NetworkSearch is blocked",
    "Default workspace: where FileWrite can create approved local files",
    "uses `/screenshot`, `/control`, and `/network-search` through the shared bridge contract",
    "Managed stdio sidecar spawning is deferred",
    "the app uses the external bridge contract and still requires source links",
    "Alpha free-source presets may share the same source-backed HTTP adapter",
    "External bridge routes use the bridge contract when a local loopback HTTP bridge is configured",
    "External bridge routes use the configured local bridge service when a local loopback HTTP bridge is available",
    "For the MVP bridge runtime",
    "source-model options in the UI",
    "choose a free source model when prompted",
    "DeepSeek and custom routes use local Windows or macOS screenshot/input backends",
    "Tool Backend Strategy inspector",
    "If the selected route has native bridge-backed web search available",
  ]) {
    if (collapsedInstallation.includes(phrase)) {
      failures.push(`docs/INSTALLATION.md must not use internal capability wording in user setup docs: ${phrase}`);
    }
  }
  for (const phrase of [
    "Web search depends on the selected model route",
    "If the selected model route can provide source-linked web search through a configured local bridge service, the app still requires source links",
    "If the selected model route does not provide web search",
    "Screen inspection and computer control are permissioned Computer Use features.",
    "Computer control requires an explicit one-shot approval",
    "Microsoft/Google-shaped mail and calendar flows are implemented and validated offline with adversarial fake providers",
    "Production account registration and live-provider execution remain disabled",
    "If web search is blocked",
    "choose a free source-linked web-search option in the UI before running search",
    "choose a free source-linked web-search option when prompted",
    "At first run, choose one local workspace root",
    "approved local file actions and maintains subdirectories for evidence",
    "Ordinary users should not need to choose separate internal folders",
    "checks bridge readiness before using the local bridge for screen inspection, computer control, and source-linked web search",
    "start and stop the bridge service yourself",
    "DS Agent does not launch or manage the bridge service in this preview",
    "Some early source-linked web-search options may share the same local implementation until separate local-browser or aggregator implementations are confirmed",
    "Optional local bridge routes use the configured local bridge service when a local loopback HTTP bridge is available",
    "DeepSeek and custom routes use local Windows or macOS screen and input paths",
    "For optional local bridge use",
    "Tool Route Strategy inspector",
  ]) {
    checkTextIncludesCollapsed(
      "docs/INSTALLATION.md",
      installation,
      phrase,
      `docs/INSTALLATION.md polished user setup wording: ${phrase}`,
    );
  }

  if (!releaseNotes.includes("not an official DeepSeek product")) {
    failures.push("release notes must keep the non-official DeepSeek disclaimer");
  } else {
    checks.push("release notes disclaimer");
  }

  if (!releaseNotes.includes("Do not commit API keys")) {
    failures.push("release notes must warn against committing API keys");
  } else {
    checks.push("release notes local-secret warning");
  }

  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "local-only continuation material and are intentionally excluded from public source snapshots",
    "release notes local-only continuation docs",
  );

  if (!releaseNotes.includes("npx pnpm@9.15.9 test:release-local")) {
    failures.push("release notes must document the local release gate");
  } else {
    checks.push("release notes local release gate");
  }
  checkTextDoesNotInclude(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "v0.1.0 Candidate Notes",
    "release notes title must use formal release wording",
  );
  checkTextDoesNotInclude(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "tauri build --debug",
    "release notes must not document debug Tauri builds for Windows installer publication",
  );
  checkTextDoesNotInclude(
    "README.md",
    readme,
    "tauri build --debug",
    "README must not document debug Tauri builds for Windows installer publication",
  );
  checkTextDoesNotInclude(
    "docs/INSTALLATION.md",
    readText("docs/INSTALLATION.md"),
    "tauri build --debug",
    "installation docs must not document debug Tauri builds for Windows installer publication",
  );

  checkTextIncludes(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "Windows local helper self-test",
    "release notes Windows local helper self-test",
  );

  checkTextIncludes(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "npx pnpm@9.15.9 test:release-local -- --require-live-deepseek --include-installed-workflow",
    "release notes strongest local release gate",
  );

  if (
    !releaseNotes.includes("Windows installer release") ||
    !releaseNotes.includes("WebView2 bootstrapper")
  ) {
    failures.push("release notes must document Windows installer release packaging");
  } else {
    checks.push("release notes Windows installer release packaging");
  }
}

function checkPublicReleaseCopyPositioning() {
  for (const filePath of publicReleaseCopyFiles) {
    if (!existsSync(filePath)) {
      failures.push(`${filePath} is required for public release copy guard`);
      continue;
    }

    const content = readText(filePath);
    for (const term of publicPeerProductTerms) {
      if (containsPeerProductTerm(content, term)) {
        failures.push(
          `${filePath} must not name peer agent/model products in public release copy: ${term}`,
        );
      }
    }
  }
  checks.push("public release copy avoids peer product names");

  for (const filePath of publicUiCopyFiles) {
    if (!existsSync(filePath)) {
      failures.push(`${filePath} is required for public UI copy guard`);
      continue;
    }

    const visibleCopy = extractPublicUiCopyText(readText(filePath));
    for (const term of publicPeerProductTerms) {
      if (containsPeerProductTerm(visibleCopy, term)) {
        failures.push(`${filePath} must not name peer agent/model products in public UI copy: ${term}`);
      }
    }

    if (/Foundation MVP|基础 MVP/i.test(visibleCopy)) {
      failures.push(`${filePath} must not use stale Foundation MVP wording in public UI copy`);
    } else {
      checks.push(`${filePath} no stale Foundation MVP UI copy`);
    }

    if (filePath === "apps/desktop/src/i18n.ts") {
      for (const phrase of staleChineseUiBridgePhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale mixed-language zh bridge UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "本地桥接路线",
        "本地 HTTP 桥接服务",
        "本地服务启动暂缓",
        "所选模型联网搜索",
        "来源关联路线执行",
        "兼容本地服务屏幕路线",
        "已配置本地服务屏幕路线",
        "本地 Windows 屏幕查看",
        "兼容本地服务鼠标键盘路线",
        "已配置本地服务鼠标键盘路线",
        "屏幕路线状态",
        "控制路线状态",
        "本地桥接服务",
        "需要本地桥接",
        "本地地址已配置",
        "路线类型未选择",
        "仅权限复核",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished zh bridge UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished zh bridge UI copy");

      for (const phrase of staleEnglishUiBridgePhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale English bridge/backend UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "Local bridge route",
        "Local HTTP bridge service",
        "Local service startup deferred",
        "Selected model web search",
        "Source-linked route execution",
        "Legacy local-service screen route",
        "Configured local-service screen route",
        "Local Windows screen inspection",
        "Legacy local-service mouse and keyboard route",
        "Configured local-service mouse and keyboard route",
        "Screen route status",
        "Control route status",
        "Local bridge service",
        "Local bridge required",
        "Local address configured",
        "Route type missing",
        "Permission review only",
        "local input route",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished English bridge/backend UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished English bridge/backend UI copy");

      for (const phrase of staleChineseUiNetworkSearchPhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(
            `${filePath} must not use stale mixed-language zh NetworkSearch UI copy: ${phrase}`,
          );
        }
      }

      for (const phrase of [
        "联网搜索审批",
        "联网搜索来源模型",
        "免费网页来源模型",
        "大模型原生联网搜索",
        "来源支撑联网搜索模型",
        "已创建联网搜索待审批请求",
        "联网搜索已执行",
        "需要联网搜索来源模型",
        "来源关联联网搜索选项",
        "免费来源关联联网搜索选项",
        "同一本地搜索实现",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished zh NetworkSearch UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished zh NetworkSearch UI copy");

      for (const phrase of staleEnglishUiNetworkSearchPhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale NetworkSearch English UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "Web search source model",
        "Native large-model web search",
        "Source-linked web-search option",
        "Web search approval",
        "A web search approval request was created",
        "Web search ran and recorded source links",
        "Web search did not complete",
        "Web search request failed",
        "Web search source model required",
        "uses source-linked web search for live results",
        "Choose a free source-linked web-search option first",
        "same local search implementation",
        "Choose a web search source model first",
        "current web search route is not enabled for live search",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished English NetworkSearch UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished English NetworkSearch UI copy");

      for (const phrase of staleChineseUiDrivePhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale cloud/Drive zh UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "本地文件夹",
        "读取本地文件夹",
        "导出到本地文件夹",
        "已创建本地文件夹读取审批请求",
        "本地文件夹读取已完成",
        "工作包已导出到本地文件夹",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished zh local-folder UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished zh local-folder UI copy");

      for (const phrase of staleEnglishUiDrivePhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale Drive English UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "Local folder read",
        "Read selected local-folder text evidence.",
        "A local folder read approval request was created",
        "Local folder read completed",
        "Local folder read did not complete",
        "Local folder read request failed",
        "Work package export",
        "Export work packages to a selected local folder.",
        "A local folder export approval request was created",
        "Work package exported to the local folder",
        "Work package export did not complete",
        "Work package export request failed",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished English local-folder UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished English local-folder UI copy");

      for (const phrase of staleEnglishUiFileWritePhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale FileWrite English UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "File write approval",
        "A file write approval request was created",
        "File write did not execute",
        "File write request failed",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished English file-write UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished English file-write UI copy");

      for (const phrase of staleChineseUiFileWritePhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale FileWrite zh UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "文件写入审批",
        "文件写入审批请求",
        "文件写入未执行",
        "文件写入请求失败",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished zh file-write UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished zh file-write UI copy");

      for (const phrase of staleChineseUiTerminalWritePhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale TerminalWrite zh UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "终端写入审批",
        "终端写入审批请求",
        "终端写入只留下权限复核记录",
        "终端写入请求失败",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished zh terminal-write UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished zh terminal-write UI copy");

      for (const phrase of staleEnglishUiTerminalWritePhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale TerminalWrite English UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "Terminal write approval",
        "A terminal write approval request was created",
        "Terminal write keeps a permission review record",
        "Terminal write request failed",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(
            `${filePath} must include polished English terminal-write UI copy: ${phrase}`,
          );
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished English terminal-write UI copy");

      for (const phrase of staleChineseUiBrowserSubmitPhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale BrowserSubmit zh UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "浏览器提交审批",
        "浏览器提交审批请求",
        "浏览器提交只留下权限复核记录",
        "浏览器提交请求失败",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished zh browser-submit UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished zh browser-submit UI copy");

      for (const phrase of staleEnglishUiBrowserSubmitPhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale BrowserSubmit English UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "Browser form submission approval",
        "A browser form submission approval request was created",
        "Browser form submission keeps a permission review record",
        "Browser form submission request failed",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(
            `${filePath} must include polished English browser-submit UI copy: ${phrase}`,
          );
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished English browser-submit UI copy");

      for (const phrase of staleChineseUiEmailPhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale email capability zh UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "邮件发送审批",
        "邮件发送审批请求",
        "邮件发送只留下权限复核记录",
        "邮件发送请求失败",
        "邮件草稿审批",
        "邮件草稿审批请求",
        "邮件草稿只留下权限复核记录",
        "邮件草稿请求失败",
        "邮件读取审批",
        "邮件读取审批请求",
        "邮件读取只留下权限复核记录",
        "邮件读取请求失败",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished zh email UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished zh email UI copy");

      for (const phrase of staleEnglishUiEmailPhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale email capability English UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "Email send approval",
        "An email send approval request was created",
        "Email send keeps a permission review record",
        "Email send request failed",
        "Email draft approval",
        "An email draft approval request was created",
        "Email draft keeps a permission review record",
        "Email draft request failed",
        "Email read approval",
        "An email read approval request was created",
        "Email read keeps a permission review record",
        "Email read request failed",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished English email UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished English email UI copy");

      for (const phrase of staleChineseUiScreenshotPhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale screenshot zh UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "屏幕读取审批",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished zh screenshot UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished zh screenshot UI copy");

      for (const phrase of staleEnglishUiScreenshotPhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale screenshot English UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "Screen inspection approval",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished English screenshot UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished English screenshot UI copy");

      for (const phrase of staleChineseUiComputerControlPhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale ComputerControl zh UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "电脑控制审批",
        "电脑控制审批请求",
        "电脑控制已执行",
        "电脑控制已执行，并已保存权限复核记录",
        "电脑控制未执行",
        "电脑控制请求失败",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished zh computer-control UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished zh computer-control UI copy");

      for (const phrase of staleEnglishUiComputerControlPhrases) {
        if (visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must not use stale ComputerControl English UI copy: ${phrase}`);
        }
      }

      for (const phrase of [
        "Computer control approval",
        "Local computer control unlock",
        "A computer control approval request was created",
        "Computer control executed",
        "Computer control executed and saved a permission review record",
        "Computer control was not executed",
        "Computer control request failed",
      ]) {
        if (!visibleCopy.includes(phrase)) {
          failures.push(`${filePath} must include polished English computer-control UI copy: ${phrase}`);
        }
      }
      checks.push("apps/desktop/src/i18n.ts polished English computer-control UI copy");
    }
  }
  checks.push("public UI copy avoids peer product names");

  const readme = readText("README.md");
  const releaseNotes = readText("docs/RELEASE_NOTES_v0.1.0.md");
  checkTextIncludesCollapsed(
    "README.md",
    readme,
    "contract-first modular Harness architecture",
    "README.md modular Harness positioning",
  );
  checkTextIncludesCollapsed(
    "README.md",
    readme,
    "DS Agent treats work as a goal-directed loop, not a single model reply",
    "README.md Loop Engineering positioning",
  );
  checkTextIncludesCollapsed(
    "README.md",
    readme,
    "The Kernel persists run and review state, retries only bounded failed steps, and does not claim completion from model confidence alone",
    "README.md verified completion positioning",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "harness architecture",
    "release notes harness architecture positioning",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "loop engineering implemented in code",
    "release notes loop engineering positioning",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "token-efficient DeepSeek routing",
    "release notes token-efficient DeepSeek routing positioning",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "context receipts show loop mode, workflow policy, selected evidence, memory, route, token/cache state, validation results, and intentional omissions",
    "release notes loop context receipt positioning",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "Markdown and HTML report exports carry the same context receipt summary",
    "release notes report export context receipt positioning",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "bounded repair loops keep failed-step retries small",
    "release notes bounded repair loop positioning",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "v0.1.0-rc.11 Update",
    "release notes rc.11 update heading",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/kernel/app_update.rs",
    readText("apps/desktop/src-tauri/src/kernel/app_update.rs"),
    'APP_UPDATE_USER_AGENT: &str = "DS-Agent-Updater/1.0.1"',
    "app updater User-Agent v1.0.1",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/kernel/app_update.rs",
    readText("apps/desktop/src-tauri/src/kernel/app_update.rs"),
    'APP_UPDATE_CURRENT_RELEASE_TAG: &str = "v1.0.1"',
    "app updater current release tag v1.0.1",
  );
  checkTextDoesNotInclude(
    "apps/desktop/src-tauri/src/kernel/app_update.rs",
    readText("apps/desktop/src-tauri/src/kernel/app_update.rs"),
    'APP_UPDATE_CURRENT_RELEASE_TAG: &str = "v0.9.0"',
    "app updater current release tag must not regress to v0.9.0",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v1.0.1.md",
    readText("docs/RELEASE_NOTES_v1.0.1.md"),
    "Package, desktop, Tauri and Cargo metadata are `1.0.1`, and the updater identity is `v1.0.1`.",
    "v1.0.1 stable release notes updater identity",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v1.0.1.md",
    readText("docs/RELEASE_NOTES_v1.0.1.md"),
    "one Confirm and run / Reject decision pair",
    "v1.0.1 task-bound grouped approval fix",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v1.0.1.md",
    readText("docs/RELEASE_NOTES_v1.0.1.md"),
    "The release artifact was built and inspected without launching the installer or changing the installed DS Agent application or its data.",
    "v1.0.1 release validation boundary",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v1.0.0.md",
    readText("docs/RELEASE_NOTES_v1.0.0.md"),
    "Package, desktop, Tauri and Cargo metadata are `1.0.0`, and the updater identity is `v1.0.0`.",
    "v1.0.0 stable release notes updater identity",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v1.0.0.md",
    readText("docs/RELEASE_NOTES_v1.0.0.md"),
    "The final artifact was built and inspected without launching the installer or changing installed app data.",
    "v1.0.0 stable release validation boundary",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.9.0.md",
    readText("docs/RELEASE_NOTES_v0.9.0.md"),
    "Package, desktop, Tauri and Cargo metadata are `0.9.0`, and the updater identity is `v0.9.0`.",
    "v0.9.0 stable release notes updater identity",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.9.0.md",
    readText("docs/RELEASE_NOTES_v0.9.0.md"),
    "Expert Team",
    "v0.9.0 stable release notes expert team positioning",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.9.0.md",
    readText("docs/RELEASE_NOTES_v0.9.0.md"),
    "Soul",
    "v0.9.0 stable release notes Soul positioning",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.5.0.md",
    readText("docs/RELEASE_NOTES_v0.5.0.md"),
    "Bumps the package, desktop, Tauri, Cargo, and updater metadata to `0.5.0` / `v0.5.0` so installed Windows clients can detect this release as newer than `v0.4.1`.",
    "v0.5.0 release notes updater version bump",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.5.0.md",
    readText("docs/RELEASE_NOTES_v0.5.0.md"),
    "durable Automation and Artifact Engine",
    "v0.5.0 release notes durable automation and artifact positioning",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.4.1.md",
    readText("docs/RELEASE_NOTES_v0.4.1.md"),
    "Bumps the package, desktop, Tauri, Cargo, and updater metadata to `0.4.1` / `v0.4.1` so installed Windows clients can detect this release as newer than `v0.4.0`.",
    "v0.4.1 release notes updater version bump",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.4.1.md",
    readText("docs/RELEASE_NOTES_v0.4.1.md"),
    "This patch removes the tracked `.codegraph` directory marker from the public source tree.",
    "v0.4.1 release notes repository cleanliness positioning",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/app_update_commands.rs",
    readText("apps/desktop/src-tauri/src/app_update_commands.rs"),
    'pub fn download_app_update() -> Result<AppUpdateDownloadResult, String>',
    "app updater has background download command",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/app_update_commands.rs",
    readText("apps/desktop/src-tauri/src/app_update_commands.rs"),
    "download_receipt: String",
    "app updater install command accepts only an opaque receipt",
  );
  checkTextDoesNotInclude(
    "apps/desktop/src-tauri/src/app_update_commands.rs",
    readText("apps/desktop/src-tauri/src/app_update_commands.rs"),
    "installer_path: String",
    "app updater command boundary must not accept an installer path",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/kernel/app_update.rs",
    readText("apps/desktop/src-tauri/src/kernel/app_update.rs"),
    'args: vec!["/S".to_string()]',
    "app updater uses silent NSIS installer flag",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/kernel/app_update.rs",
    readText("apps/desktop/src-tauri/src/kernel/app_update.rs"),
    ".redirect(reqwest::redirect::Policy::none())",
    "app updater disables automatic redirects",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/kernel/app_update_receipt.rs",
    readText("apps/desktop/src-tauri/src/kernel/app_update_receipt.rs"),
    "APP_UPDATE_MAX_BYTES",
    "app updater enforces a streamed byte ceiling",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/kernel/app_update_receipt.rs",
    readText("apps/desktop/src-tauri/src/kernel/app_update_receipt.rs"),
    "install_pending",
    "app updater persists an install crash checkpoint",
  );
  checkTextIncludes(
    "apps/desktop/src/App.tsx",
    readText("apps/desktop/src/App.tsx"),
    'invoke<AppUpdateStatus>("check_app_update")',
    "app update UI checks through deterministic local command",
  );
  checkTextIncludes(
    "apps/desktop/src/App.tsx",
    readText("apps/desktop/src/App.tsx"),
    'invoke<AppUpdateDownloadResult>("download_app_update")',
    "app update UI downloads silently through deterministic local command",
  );
  checkTextIncludes(
    "apps/desktop/src/App.tsx",
    readText("apps/desktop/src/App.tsx"),
    'invoke<AppUpdateInstallResult>("install_app_update"',
    "app update UI installs through deterministic local command",
  );
  checkTextIncludes(
    "apps/desktop/src/App.tsx",
    readText("apps/desktop/src/App.tsx"),
    "downloadReceipt: downloadedAppUpdate.download_receipt",
    "app update UI consumes the opaque download receipt",
  );
  checkTextIncludes(
    "apps/desktop/src/App.tsx",
    readText("apps/desktop/src/App.tsx"),
    "{downloadedAppUpdateReady ? (",
    "app update UI only shows install after download validation",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "Lets users drag local files onto the input box",
    "release notes attachment drag-drop update",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "shows compact attachment cards above the input",
    "release notes attachment composer card update",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "Fixes the Windows desktop shortcut and taskbar icon refresh path",
    "release notes Windows shortcut icon refresh fix",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "goal contract context with the user's real goal, constraints, done-when criteria, completion verifier, stop conditions, and near-miss guardrails",
    "release notes goal loop contract feature",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "one short, task-grounded suggestion for a better next step",
    "release notes completion advice feature",
  );

  const workflow = readText("apps/desktop/src-tauri/src/kernel/workflow.rs");
  const types = readText("apps/desktop/src/types.ts");
  const app = readText("apps/desktop/src/App.tsx");
  const i18n = readText("apps/desktop/src/i18n.ts");
  for (const [file, text, phrase, label] of [
    [
      "apps/desktop/src-tauri/src/kernel/workflow.rs",
      workflow,
      "pub struct OperationsBriefingContextReceipt",
      "Operations Briefing context receipt Rust model",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/workflow.rs",
      workflow,
      "pub context_receipt: OperationsBriefingContextReceipt",
      "Operations Briefing run stores context receipt",
    ],
    [
      "apps/desktop/src/types.ts",
      types,
      "export type OperationsBriefingContextReceipt",
      "Operations Briefing context receipt TypeScript model",
    ],
    [
      "apps/desktop/src/App.tsx",
      app,
      "operationsBriefingRun.context_receipt",
      "Operations Briefing run archive renders context receipt",
    ],
    [
      "apps/desktop/src/i18n.ts",
      i18n,
      "contextReceipt",
      "Operations Briefing copy includes context receipt labels",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/workflow.rs",
      workflow,
      "pub loop_mode: String",
      "Operations Briefing context receipt records loop mode",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/workflow.rs",
      workflow,
      "pub workflow_policy: String",
      "Operations Briefing context receipt records workflow policy",
    ],
    [
      "apps/desktop/src/App.tsx",
      app,
      "operationsBriefingRun.context_receipt.workflow_policy",
      "Operations Briefing UI renders workflow policy",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/workflow.rs",
      workflow,
      "write_operations_briefing_context_receipt_markdown",
      "Operations Briefing Markdown report renders context receipt",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/workflow.rs",
      workflow,
      "write_operations_briefing_context_receipt_html",
      "Operations Briefing HTML report renders context receipt",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/workflow.rs",
      workflow,
      "const OPERATIONS_BRIEFING_REPAIR_RETRY_BUDGET: usize = 1",
      "Operations Briefing bounded repair loop retry budget",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/workflow.rs",
      workflow,
      "operations_briefing_repair_manifest_excerpt",
      "Operations Briefing bounded repair loop compact context",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/workflow.rs",
      workflow,
      "bounded repair loop retried model synthesis once",
      "Operations Briefing bounded repair loop receipt result",
    ],
  ]) {
    checkTextIncludes(file, text, phrase, label);
  }
}

function checkRuntimeWebSearchCopyPositioning() {
  const runtimeCopyFiles = [
    "apps/desktop/src-tauri/src/kernel/tool_strategy.rs",
    "apps/desktop/src-tauri/src/kernel/network_search.rs",
  ];
  const stalePhrases = [
    "free source-backed web-search adapter",
    "shared source-backed HTTP adapter",
    "NetworkSearch will use",
    "NetworkSearch will execute",
    "NetworkSearch can use",
    "source-backed NetworkSearch",
    "source-backed adapter",
    "native bridge contract",
    "selected large model's source-backed route",
    "free-source execution",
    "no executable source-backed adapter",
  ];
  const requiredPhrases = [
    "Use source-linked web search for evidence and citations.",
    "currently shares the same local search implementation",
    "Web search will use the selected source-linked web-search option",
    "Selected model route needs a separate source-linked web-search option before live search can run.",
    "Web search will execute through the selected source-linked web-search option",
    "choose a free source-linked web-search option before running search",
    "Web search has no executable source-linked route selected yet",
  ];

  const runtimeCopy = runtimeCopyFiles
    .map((filePath) => {
      if (!existsSync(filePath)) {
        failures.push(`${filePath} is required for runtime web-search copy guard`);
        return "";
      }
      return readText(filePath);
    })
    .join("\n");

  for (const phrase of stalePhrases) {
    if (runtimeCopy.includes(phrase)) {
      failures.push(`runtime web-search copy must not use internal wording: ${phrase}`);
    } else {
      checks.push(`runtime web-search copy no internal wording: ${phrase}`);
    }
  }

  for (const phrase of requiredPhrases) {
    if (!runtimeCopy.includes(phrase)) {
      failures.push(`runtime web-search copy must include user-facing wording: ${phrase}`);
    } else {
      checks.push(`runtime web-search copy user-facing wording: ${phrase}`);
    }
  }
}

function checkAppFallbackCopyPositioning() {
  const filePath = "apps/desktop/src/App.tsx";
  if (!existsSync(filePath)) {
    failures.push(`${filePath} is required for App fallback copy guard`);
    return;
  }

  const appFallbackCopy = readText(filePath);
  const stalePhrases = [
    "source-backed NetworkSearch",
    "NetworkSearch source model",
    "External bridge runtime",
    "External HTTP bridge",
    "external loopback HTTP service",
    "screen pixels",
    "screen capture library",
    "input library",
    "free source-backed web-search adapter",
    "shared source-backed HTTP adapter",
    "Selected large model needs a separate source-backed NetworkSearch model",
  ];
  const requiredPhrases = [
    "selected model route needs a separate source-linked web-search option before search can run",
    "Screen inspection uses the local Windows screen route.",
    "Mouse and keyboard control uses the local Windows input route.",
    "Local HTTP bridge service",
    "Use a user-started local HTTP service with health, screenshot, control, and web-search endpoints.",
    "Selected local Computer Use route does not need the local bridge service.",
    "Use a free source-linked web-search option for evidence and citations.",
    "currently uses the same local search implementation",
  ];

  for (const phrase of stalePhrases) {
    if (appFallbackCopy.includes(phrase)) {
      failures.push(`${filePath} fallback copy must not use stale internal wording: ${phrase}`);
    } else {
      checks.push(`${filePath} fallback copy no stale internal wording: ${phrase}`);
    }
  }

  for (const phrase of requiredPhrases) {
    if (!appFallbackCopy.includes(phrase)) {
      failures.push(`${filePath} fallback copy must include user-facing wording: ${phrase}`);
    } else {
      checks.push(`${filePath} fallback copy user-facing wording: ${phrase}`);
    }
  }
}

function checkChatFirstRunStatusUi() {
  const appPath = "apps/desktop/src/App.tsx";
  const i18nPath = "apps/desktop/src/i18n.ts";
  const stylesPath = "apps/desktop/src/styles.css";

  for (const filePath of [appPath, i18nPath, stylesPath]) {
    if (!existsSync(filePath)) {
      failures.push(`${filePath} is required for chat-first run status UI`);
      return;
    }
  }

  const app = readText(appPath);
  const i18n = readText(i18nPath);
  const styles = readText(stylesPath);

  const requiredAppSnippets = [
    "runStatusSteps",
    "className=\"run-status-panel\"",
    "className=\"workflow-step-list\"",
    "className=\"inspector-details\"",
    "copy.runStatus.title",
    "copy.runStatus.workflowSteps",
  ];

  for (const snippet of requiredAppSnippets) {
    checkTextIncludes(appPath, app, snippet, `chat-first run status app snippet: ${snippet}`);
  }

  for (const phrase of [
    "任务状态",
    "运行步骤",
    "Run Status",
    "Workflow Steps",
  ]) {
    checkTextIncludes(i18nPath, i18n, phrase, `chat-first run status copy: ${phrase}`);
  }

  for (const snippet of [
    ".run-status-panel",
    ".workflow-step-list",
    ".workflow-step-marker",
    ".inspector-details",
    ".setup-disclosure",
  ]) {
    checkTextIncludes(stylesPath, styles, snippet, `chat-first run status style: ${snippet}`);
  }
}

function checkChatFirstCenterWorkbenchUi() {
  const appPath = "apps/desktop/src/App.tsx";
  const i18nPath = "apps/desktop/src/i18n.ts";
  const stylesPath = "apps/desktop/src/styles.css";

  for (const filePath of [appPath, i18nPath, stylesPath]) {
    if (!existsSync(filePath)) {
      failures.push(`${filePath} is required for chat-first center workbench UI`);
      return;
    }
  }

  const app = readText(appPath);
  const i18n = readText(i18nPath);
  const styles = readText(stylesPath);

  for (const snippet of [
    "className=\"sidebar-controls\"",
    "className=\"sidebar-tool operations-tool\"",
    "className=\"sidebar-tool operations-tool ordinary-user-hidden\"",
    "className=\"sidebar-tool package-tool ordinary-user-hidden\"",
    "className=\"sidebar-tool memory-tool\"",
    "className=\"sidebar-tool settings-tool\"",
    "copy.skills.title",
    "className=\"skill-catalog\"",
    "className=\"skill-catalog-item\"",
    "className=\"brand-row\"",
    "className=\"app-update-slot brand-update-slot\"",
    "className=\"app-update-stack\"",
    "className={`app-update-version${appUpdateError ? \" error\" : \"\"}`}",
    "renderLegacyCenterManagementPanels",
    "className=\"agent-chat-panel\"",
    "aria-label={copy.chatWorkbench.title}",
    "className=\"chat-thread\"",
    "className=\"chat-message assistant pending\"",
    'className={`chat-composer${agentAttachmentDragActive ? " drag-active" : ""}`}',
    "className=\"starter-prompts\"",
    "className=\"starter-prompt\"",
    "copy.chatWorkbench.starterPromptsLabel",
    "className=\"sidebar-record-list\"",
    "onSubmit={sendAgentMessage}",
    "agentMessages.map",
    "run_agent_chat",
    "className=\"setup-modal-backdrop\"",
    "copy.chatWorkbench.deepSeekKeyTitle",
    "copy.chatWorkbench.workspaceTitle",
    "copy.chatWorkbench.networkSearchTitle",
    "copy.chatWorkbench.title",
    "copy.chatWorkbench.composerPlaceholder",
    "className=\"agent-action-approval\"",
    "resolveAndResumeAgentActionGroup",
  ]) {
    checkTextIncludes(appPath, app, snippet, `chat-first center workbench app snippet: ${snippet}`);
  }

  checkTextIncludes(
    stylesPath,
    styles,
    ".ordinary-user-hidden",
    "ordinary-user plugin utilities stay source-retained and display-hidden",
  );

  for (const phrase of [
    "DeepSeek 对话工作台",
    "输入问题、文字或指令",
    "根据我的证据文件夹，生成一份经营简报。",
    "技能与插件",
    "请输入 DeepSeek API key",
    "DeepSeek Chat Workbench",
    "Enter a question, text, or instruction",
    "Create a management briefing from my evidence folder.",
    "Skills & Plugins",
    "Enter a DeepSeek API key",
  ]) {
    checkTextIncludes(i18nPath, i18n, phrase, `chat-first center workbench copy: ${phrase}`);
  }

  for (const snippet of [
    ".sidebar-controls",
    ".brand-row",
    ".brand-update-slot",
    ".app-update-stack",
    ".app-update-version",
    ".sidebar-tools",
    ".sidebar-tool",
    ".sidebar-record-row",
    ".skill-catalog",
    ".skill-catalog-item",
    ".agent-chat-panel",
    ".chat-thread",
    ".chat-message",
    ".chat-message.pending",
    ".chat-composer",
    ".starter-prompts",
    ".starter-prompt",
    ".chat-session-meta",
    ".composer-actions",
    ".setup-modal-backdrop",
    ".setup-modal",
  ]) {
    checkTextIncludes(stylesPath, styles, snippet, `chat-first center workbench style: ${snippet}`);
  }
}

function checkFrontendStarterPromptsSmoke() {
  const scriptPath = "scripts/frontend-starter-prompts-smoke.mjs";
  const script = readText(scriptPath);
  for (const [snippet, checkName] of [
    ["--self-test", "frontend starter prompts smoke has self-test flag"],
    ["starter_prompt_count", "frontend starter prompts smoke reports prompt count"],
    [".starter-prompt", "frontend starter prompts smoke checks starter prompt DOM"],
    ["Runtime.evaluate", "frontend starter prompts smoke uses CDP runtime evaluation"],
    ["Page.captureScreenshot", "frontend starter prompts smoke captures screenshot evidence"],
    ["--remote-debugging-port", "frontend starter prompts smoke launches browser with CDP"],
    ["npx-cli.js", "frontend starter prompts smoke avoids direct Windows npx.cmd spawn"],
    [
      "根据我的证据文件夹，生成一份经营简报。",
      "frontend starter prompts smoke checks Chinese briefing prompt",
    ],
    [
      "Create a management briefing from my evidence folder.",
      "frontend starter prompts smoke checks English briefing prompt",
    ],
  ]) {
    checkTextIncludes(scriptPath, script, snippet, checkName);
  }
}

function checkRuntimeComputerUseCopyPositioning() {
  const filePath = "apps/desktop/src-tauri/src/kernel/computer_use.rs";
  if (!existsSync(filePath)) {
    failures.push(`${filePath} is required for runtime Computer Use copy guard`);
    return;
  }

  const runtimeCopy = readText(filePath);
  const stalePhrases = [
    "External bridge runtime",
    "external bridge runtime",
    "external bridge contract",
    "screen pixels",
    "legacy bridge-style",
    "backend is configured",
    "stdio sidecar",
    "MVP",
    "screen capture library",
    "input library",
  ];
  const requiredPhrases = [
    "Selected route does not need the local bridge service.",
    "Set {BRIDGE_TRANSPORT_ENV_VAR}=http before bridge-routed Computer Use can run.",
    "Local bridge HTTP health check connected to",
    "Screen inspection uses the connected local bridge service for the selected model route",
    "Screen inspection uses the local bridge service for the selected model route, but the service is not connected",
    "Screen inspection uses the local Windows screen route",
    "Connect the local bridge service before requesting bridge-routed screen inspection.",
    "Mouse and keyboard control uses the connected local bridge service for the selected model route",
    "Mouse and keyboard control uses the local bridge service for the selected model route, but the service is not connected",
    "Mouse and keyboard control uses the local Windows input route",
    "Connect the local bridge service before requesting bridge-routed mouse and keyboard control.",
  ];

  for (const phrase of stalePhrases) {
    if (runtimeCopy.includes(phrase)) {
      failures.push(`runtime Computer Use copy must not use internal wording: ${phrase}`);
    } else {
      checks.push(`runtime Computer Use copy no internal wording: ${phrase}`);
    }
  }

  for (const phrase of requiredPhrases) {
    if (!runtimeCopy.includes(phrase)) {
      failures.push(`runtime Computer Use copy must include user-facing wording: ${phrase}`);
    } else {
      checks.push(`runtime Computer Use copy user-facing wording: ${phrase}`);
    }
  }
}

function checkRuntimeLocalDesktopRouteFailureCopyPositioning() {
  const filePath = "apps/desktop/src-tauri/src/kernel/capability.rs";
  if (!existsSync(filePath)) {
    failures.push(`${filePath} is required for local desktop route failure copy guard`);
    return;
  }

  const runtimeCopy = readText(filePath);
  const stalePhrases = [
    "screen capture returned empty dimensions",
    "screen capture returned empty PNG bytes",
    "computer screenshot display enumeration failed",
    "computer screenshot found no display to capture",
    "computer screenshot capture failed",
    "computer screenshot PNG encoding failed",
    "computer control input backend setup failed",
    "capture backend unavailable",
    "input backend unavailable",
  ];
  const requiredPhrases = [
    "local screen inspection returned empty dimensions",
    "local screen inspection returned empty PNG bytes",
    "local screen inspection display enumeration failed",
    "local screen inspection found no display to inspect",
    "local screen inspection failed",
    "local screen inspection PNG encoding failed",
    "local mouse and keyboard control setup failed",
    "local screen inspection route unavailable",
    "local mouse and keyboard control route unavailable",
  ];

  for (const phrase of stalePhrases) {
    if (runtimeCopy.includes(phrase)) {
      failures.push(`local desktop route failure copy must not use internal wording: ${phrase}`);
    } else {
      checks.push(`local desktop route failure copy no internal wording: ${phrase}`);
    }
  }

  for (const phrase of requiredPhrases) {
    if (!runtimeCopy.includes(phrase)) {
      failures.push(`local desktop route failure copy must include user-facing wording: ${phrase}`);
    } else {
      checks.push(`local desktop route failure copy user-facing wording: ${phrase}`);
    }
  }
}

function checkPublicPeerProductTermGuardSelfTest() {
  const blockedSamples = [
    ["openclaw", "OpenClaw"],
    ["CODEX bridge comparison", "Codex"],
    ["Claude code style", "Claude Code"],
  ];
  const allowedSamples = ["DeepSeek-first harness architecture", "open-source desktop agent"];

  for (const [content, term] of blockedSamples) {
    if (!containsPeerProductTerm(content, term)) {
      failures.push(`public peer-name guard self-test must block ${content}`);
      continue;
    }
    checks.push(`public peer-name guard self-test blocks ${content}`);
  }

  for (const content of allowedSamples) {
    const matchedTerm = publicPeerProductTerms.find((term) => containsPeerProductTerm(content, term));
    if (matchedTerm) {
      failures.push(`public peer-name guard self-test must allow ${content}`);
      continue;
    }
    checks.push(`public peer-name guard self-test allows ${content}`);
  }
}

function checkPublicUiCopyExtractionSelfTest() {
  const sample = `
    const labels = {
      chatgpt: "External chat model",
      codex_bridge_screen_capture: "Bridge-style screen pixel capture",
      unsafe: "Claude Code style",
    };
  `;
  const visibleCopy = extractPublicUiCopyText(sample);

  if (visibleCopy.includes("codex_bridge_screen_capture")) {
    failures.push("public UI copy extraction self-test must ignore internal identifiers");
  } else {
    checks.push("public UI copy extraction self-test ignores internal identifiers");
  }

  if (!containsPeerProductTerm(visibleCopy, "Claude Code")) {
    failures.push("public UI copy extraction self-test must inspect visible string values");
  } else {
    checks.push("public UI copy extraction self-test inspects visible string values");
  }
}

function checkCiWriteBoundarySelfTest() {
  const blockedSamples = [
    ["permissions:\n  contents: write\n", "contents write permission"],
    ["permissions:\n  id-token: write\n", "id-token write permission"],
    ["- uses: softprops/action-gh-release@v2", "release action"],
    ["- run: gh release create v0.0.2", "gh release command"],
    ["- uses: actions/upload-artifact@v4", "artifact upload"],
  ];
  const allowedSample = [
    "permissions:",
    "  contents: read",
    "- run: pnpm test:release-source",
  ].join("\n");

  for (const [content, label] of blockedSamples) {
    if (ciWriteBoundaryViolations(content).length === 0) {
      failures.push(`CI write-boundary self-test must block ${label}`);
      continue;
    }
    checks.push(`CI write-boundary self-test blocks ${label}`);
  }

  if (ciWriteBoundaryViolations(allowedSample).length > 0) {
    failures.push("CI write-boundary self-test must allow read-only source guard CI");
  } else {
    checks.push("CI write-boundary self-test allows read-only source guard CI");
  }
}

function ciWriteBoundaryViolations(content) {
  const blockedPatterns = [
    [/^\s*contents:\s*write\s*$/im, "contents write permission"],
    [/^\s*id-token:\s*write\s*$/im, "id-token write permission"],
    [/^\s*actions:\s*write\s*$/im, "actions write permission"],
    [/^\s*packages:\s*write\s*$/im, "packages write permission"],
    [/^\s*pull-requests:\s*write\s*$/im, "pull-requests write permission"],
    [/softprops\/action-gh-release/i, "GitHub release action"],
    [/actions\/upload-artifact/i, "artifact upload action"],
    [/\bgh\s+release\b/i, "gh release command"],
    [/\b(create-release|upload-release-asset)\b/i, "release asset command"],
    [/tauri-apps\/tauri-action/i, "Tauri release upload action"],
  ];

  return blockedPatterns
    .filter(([pattern]) => pattern.test(content))
    .map(([, label]) => label);
}

function containsPeerProductTerm(content, term) {
  return content.toLowerCase().includes(term.toLowerCase());
}

function extractPublicUiCopyText(sourceText) {
  const visibleStrings = [];
  const propertyStringPattern =
    /(?:^|[\s,{])[$A-Z_a-z][$\w]*\s*:\s*("([^"\\]|\\.)*"|'([^'\\]|\\.)*'|`([^`\\]|\\.)*`)/g;

  for (const match of sourceText.matchAll(propertyStringPattern)) {
    visibleStrings.push(stripStringLiteralDelimiters(match[1]));
  }

  return visibleStrings.join("\n");
}

function stripStringLiteralDelimiters(literal) {
  if (literal.length < 2) {
    return literal;
  }
  return literal.slice(1, -1);
}

function checkHistoricalReleaseNotes() {
  const docPath = "docs/RELEASE_NOTES_v0.1-alpha.md";
  if (!existsSync(docPath)) {
    checks.push("historical release notes absent");
    return;
  }

  const releaseNotes = readText(docPath);
  checkTextIncludes(
    docPath,
    releaseNotes,
    "Do not treat `v0.1-alpha` as the current project version.",
    "historical release notes current-version warning",
  );
  checkTextIncludesCollapsed(
    docPath,
    releaseNotes,
    "Status: historical source-only alpha note, superseded by the current v0.1.0 Windows-first preview.",
    "historical release notes superseded status",
  );
  checkTextIncludes(
    docPath,
    releaseNotes,
    "$env:CARGO_TARGET_DIR = Join-Path $env:TEMP",
    "historical release notes temp Cargo target",
  );

  if (/D:\\codex-target/i.test(releaseNotes)) {
    failures.push(`${docPath} must not include machine-specific D:\\codex-target paths`);
  } else {
    checks.push("historical release notes no machine-specific Cargo target");
  }

  const collapsedReleaseNotes = releaseNotes.replace(/\s+/g, " ").trim();
  const staleHistoricalStatus = "Status: first public source-only alpha.";
  if (collapsedReleaseNotes.includes(staleHistoricalStatus)) {
    failures.push(`${docPath} must not present v0.1-alpha as a current alpha status: ${staleHistoricalStatus}`);
  } else {
    checks.push("historical release notes no stale current-alpha status");
  }

  const staleCapabilityPhrases = [
    "EmailSend and ComputerControl",
    "Source-backed NetworkSearch adapter",
    "External loopback HTTP bridge contract for bridge-routed Computer Use and native web-search paths",
    "native NetworkSearch paths",
    "ComputerControl backend routing",
    "Local Windows/macOS screen inspection and computer control backend routing",
    "Managed sidecar spawning is deferred",
    "EmailRead, EmailDraft, and EmailSend",
    "DriveRead and DriveWrite",
    "approval/audit boundaries only",
    "BrowserSubmit records approval/audit boundaries only",
    "TerminalWrite records approval/audit boundaries only",
    "NetworkSearch must preserve source links",
    "Capability permission loop for file, network, browser, email, drive, terminal, and Computer Use surfaces",
    "Source-backed web search adapter with preserved source URLs",
    "External loopback bridge support for screen inspection, computer control, and source-linked web search",
    "External bridge service startup is not included in this historical alpha note",
  ];
  for (const phrase of staleCapabilityPhrases) {
    if (collapsedReleaseNotes.includes(phrase)) {
      failures.push(`${docPath} must not use internal capability wording in historical release notes: ${phrase}`);
    } else {
      checks.push(`historical release notes no internal capability wording: ${phrase}`);
    }
  }

  const requiredUserFacingPhrases = [
    "outbound email and computer control",
    "Optional loopback bridge support for screen inspection, computer control, and source-linked web search",
    "Local Windows/macOS screen inspection and computer control routes",
    "Local bridge service startup is not included in this historical alpha note",
    "Email read, draft, and send tools record approval and audit decisions only",
    "Local folder read and work-package export use local folders and export packages",
    "Browser form submission records approval and audit decisions only",
    "Mutating terminal commands record approval and audit decisions only",
    "Web search must preserve source links",
    "Permissioned local tools for file, network, browser, email, local folders, terminal, and Computer Use surfaces",
    "Source-linked web search with preserved source URLs",
  ];
  for (const phrase of requiredUserFacingPhrases) {
    checkTextIncludesCollapsed(
      docPath,
      releaseNotes,
      phrase,
      `historical release notes user-facing wording: ${phrase}`,
    );
  }
}

function checkWindowsValidationStatusDocs() {
  const readme = readText("README.md");
  const releaseNotes = readText("docs/RELEASE_NOTES_v0.1.0.md");

  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "Windows build/install/launch/run path has been locally validated",
    "release notes Windows local validation status",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "installed UI workflow smoke",
    "release notes installed workflow smoke status",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "macOS validation and release work will follow after the Windows preview continues to pass local release gates",
    "release notes macOS deferral after local gates",
  );

  const staleWindowsPriority = /priority[^.]+reliably build,\s*install,\s*launch,\s*and run on Windows/is;
  if (staleWindowsPriority.test(readme)) {
    failures.push("README.md must not describe Windows build/install/launch/run as the next unproven priority");
  } else {
    checks.push("README.md no stale Windows priority");
  }

  if (staleWindowsPriority.test(releaseNotes)) {
    failures.push(
      "docs/RELEASE_NOTES_v0.1.0.md must not describe Windows build/install/launch/run as the next unproven priority",
    );
  } else {
    checks.push("release notes no stale Windows priority");
  }

  const staleMacosDeferral = "after the Windows baseline is stable";
  if (readme.replace(/\s+/g, " ").trim().includes(staleMacosDeferral)) {
    failures.push(`README.md must not defer macOS with stale wording: ${staleMacosDeferral}`);
  } else {
    checks.push("README.md no stale macOS deferral wording");
  }

  if (releaseNotes.replace(/\s+/g, " ").trim().includes(staleMacosDeferral)) {
    failures.push(
      `docs/RELEASE_NOTES_v0.1.0.md must not defer macOS with stale wording: ${staleMacosDeferral}`,
    );
  } else {
    checks.push("release notes no stale macOS deferral wording");
  }

  const staleMacosDeferralZh = "Windows 基线稳定之后";
  if (readme.replace(/\s+/g, " ").trim().includes(staleMacosDeferralZh)) {
    failures.push(`README.md must not defer macOS with stale Chinese wording: ${staleMacosDeferralZh}`);
  } else {
    checks.push("README.md no stale macOS deferral wording zh");
  }
}

function checkReleaseGateDocs() {
  for (const docPath of ["docs/INSTALLATION.md"]) {
    const content = readText(docPath);
    checkTextIncludesCollapsed(
      docPath,
      content,
      "local runtime artifacts, generated workflow exports, unexpected binary files, oversized source files, and stale smoke-test release labels",
      `${docPath} release guard expanded source hygiene docs`,
    );
    checkTextIncludesCollapsed(
      docPath,
      content,
      "restores the original local directory settings file and app-data event store",
      `${docPath} installed workflow restore docs`,
    );
    checkTextIncludesCollapsed(
      docPath,
      content,
      "Windows local helper self-test",
      `${docPath} Windows local helper self-test docs`,
    );
    checkTextIncludes(
      docPath,
      content,
      "npx pnpm@9.15.9 test:release-local -- --skip-live-deepseek",
      `${docPath} offline release-local flag docs`,
    );
    checkTextIncludesCollapsed(
      docPath,
      content,
      "working-tree and staged diff whitespace checks",
      `${docPath} staged diff whitespace release-local docs`,
    );
    checkTextIncludes(
      docPath,
      content,
      "-- --include-installed-ui",
      `${docPath} installed UI release-local flag docs`,
    );
    checkTextIncludes(
      docPath,
      content,
      "-- --include-installed-workflow",
      `${docPath} installed workflow release-local flag docs`,
    );
    checkTextIncludes(
      docPath,
      content,
      "npx pnpm@9.15.9 test:release-local -- --require-live-deepseek --include-installed-workflow",
      `${docPath} strongest local release gate docs`,
    );
    checkTextIncludes(
      docPath,
      content,
      "npx pnpm@9.15.9 test:windows-installed-ui -- --workflow",
      `${docPath} direct installed workflow command docs`,
    );
    checkTextIncludesCollapsed(
      docPath,
      content,
      "Before any publication decision for a new release, run the local release gate",
      `${docPath} release gate docs`,
    );
    checkTextIncludesCollapsed(
      docPath,
      content,
      "When a local DeepSeek test key and installed Windows app are available, use the strongest local gate before that publication decision",
      `${docPath} strongest gate publication-decision docs`,
    );
    checkTextIncludesCollapsed(
      docPath,
      content,
      "desktop command layer is available",
      `${docPath} installed UI command-layer smoke docs`,
    );

    if (docPath === "README.md") {
      checkTextIncludesCollapsed(
        docPath,
        content,
        "installed app command layer, Operations Briefing run, and Markdown/HTML/PDF exports through the installed app",
        "README.md installed workflow command-layer smoke docs",
      );
      checkTextIncludesCollapsed(
        docPath,
        content,
        "the response matches the expected workflow result format",
        "README.md DeepSeek briefing smoke result-format wording",
      );
    }

    const collapsedContent = content.replace(/\s+/g, " ").trim();
    const staleInternalReleaseGateSnippets = [
      "Tauri bridge",
      "workflow JSON contract",
    ];
    for (const snippet of staleInternalReleaseGateSnippets) {
      if (collapsedContent.includes(snippet)) {
        failures.push(`${docPath} must use user-facing release-gate wording instead of internal test wording: ${snippet}`);
        continue;
      }
      checks.push(`${docPath} no stale internal release-gate wording ${snippet}`);
    }

    const staleReleaseGateSnippets = [
      "Before creating a new source-only prerelease tag or GitHub release target",
      "GitHub release target",
      "Before moving a public release tag",
      "Before moving any public GitHub release tag",
    ];
    for (const snippet of staleReleaseGateSnippets) {
      if (content.includes(snippet)) {
        failures.push(`${docPath} must not imply moving an existing public release tag: ${snippet}`);
        continue;
      }
      checks.push(`${docPath} no stale release tag wording ${snippet}`);
    }

    const staleInstalledWorkflowEventStore = "leaves local audit/run events in the installed app data store";
    if (collapsedContent.includes(staleInstalledWorkflowEventStore)) {
      failures.push(`${docPath} must not say installed workflow ${staleInstalledWorkflowEventStore}`);
    } else {
      checks.push(`${docPath} no stale installed workflow event-store wording`);
    }

    const staleAppendEventStore = "does append local audit and run events to the installed app's app-data event store";
    if (content.replace(/\s+/g, " ").trim().includes(staleAppendEventStore)) {
      failures.push(`${docPath} must not say installed workflow ${staleAppendEventStore}`);
    } else {
      checks.push(`${docPath} no stale installed workflow app-data append wording`);
    }
  }
}

function checkMemoryStudioDocs() {
  const readme = readText("README.md");
  const readmeZh = readText("README.zh-CN.md");
  const releaseNotes = readText("docs/RELEASE_NOTES_v0.1.0.md");
  const app = readText("apps/desktop/src/App.tsx");
  const i18n = readText("apps/desktop/src/i18n.ts");
  const types = readText("apps/desktop/src/types.ts");
  const memoryCommands = readText("apps/desktop/src-tauri/src/commands.rs");

  checkTextIncludesCollapsed(
    "README.md",
    readme,
    "auditable background maintenance can update, merge, or archive memory",
    "README.md auditable memory self-maintenance",
  );
  checkTextIncludesCollapsed(
    "README.zh-CN.md",
    readmeZh,
    "后台维护能够自动更新、合并或归档",
    "README.zh-CN.md auditable memory self-maintenance",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "linked memory title/body search",
    "release notes linked memory title/body search",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "linked memory search match source",
    "release notes linked memory search match source",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "linked memory relation notes",
    "release notes linked memory relation notes",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "manual existing-memory links",
    "release notes manual existing-memory links",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "feedback-informed retrieval scoring",
    "release notes feedback-informed memory retrieval scoring",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "stale/conflicting/should_update feedback surfaces compact review hints",
    "release notes selected-memory feedback review hints",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "automatic Memory maintenance v1",
    "release notes memory maintenance strategy v1",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    memoryCommands,
    ".list_selected_memory_feedback()",
    "runtime context loads selected-memory feedback",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    memoryCommands,
    "feedback:useful+4",
    "runtime context exposes useful feedback score breakdown",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    memoryCommands,
    "memories marked stale by feedback enter background update/archive maintenance",
    "runtime context emits stale feedback review hints",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    memoryCommands,
    "memories flagged conflicting by feedback enter background conflict maintenance",
    "runtime context emits conflicting feedback review hints",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    memoryCommands,
    "memories marked should_update by feedback enter background update maintenance",
    "runtime context emits should_update feedback review hints",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    memoryCommands,
    "memories repeatedly marked irrelevant by feedback enter background retrieval tuning",
    "runtime context emits repeated irrelevant maintenance review hints",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    memoryCommands,
    "memories repeatedly marked stale by feedback enter background archive maintenance",
    "runtime context emits repeated stale maintenance review hints",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    memoryCommands,
    "pub fn list_selected_memory_feedback",
    "desktop command exposes selected-memory feedback review list",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    'invoke<MemorySelectedFeedback[]>("list_selected_memory_feedback")',
    "Memory Studio loads selected-memory feedback records",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "feedbackReviewItems",
    "Memory Studio groups selected-memory feedback for review",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "repeatedIrrelevantFeedback",
    "Memory Studio escalates repeated irrelevant feedback for review",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "repeatedStaleFeedback",
    "Memory Studio escalates repeated stale feedback for review",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "feedbackReviewEmpty",
    "Memory Studio feedback review empty-state copy",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "new/skipped archived briefing runs",
    "release notes archived briefing run import preview counts",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "new/skipped pending memory candidates",
    "release notes pending memory candidate import preview counts",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "new/skipped workflow templates",
    "release notes workflow template import preview counts",
  );
  const collapsedReleaseNotes = releaseNotes.replace(/\s+/g, " ").trim();
  const staleOperationsBriefingRunOnSentence =
    "Operations Briefing workflow that reads local evidence, drafts a management brief, can use DeepSeek synthesis when configured, and previews new/skipped workflow templates, new/skipped pending memory candidates, and new/skipped archived briefing runs during work-package imports";
  if (collapsedReleaseNotes.includes(staleOperationsBriefingRunOnSentence)) {
    failures.push("docs/RELEASE_NOTES_v0.1.0.md must not bury Operations Briefing import/export safety in one long run-on bullet");
  } else {
    checks.push("release notes no run-on Operations Briefing import/export bullet");
  }
  for (const phrase of [
    "Operations Briefing workflow:",
    "Reads local evidence and drafts a management brief.",
    "Can use DeepSeek synthesis when configured.",
    "Exports Markdown, HTML, lightweight PDF, and work-package JSON to local paths.",
    "During work-package imports, previews new/skipped workflow templates, new/skipped pending memory candidates, and new/skipped archived briefing runs.",
    "package-internal duplicate ids are counted as skipped",
    "resolved memory candidates stay out of exported work packages",
    "exported work packages redact source-machine evidence handles",
    "imported memory candidates drop source-machine source links",
    "Uses blank operator templates under",
  ]) {
    checkTextIncludesCollapsed(
      "docs/RELEASE_NOTES_v0.1.0.md",
      releaseNotes,
      phrase,
      `release notes structured Operations Briefing wording: ${phrase}`,
    );
  }
  const collapsedReadme = readme.replace(/\s+/g, " ").trim();
  for (const phrase of [
    "managed bridge sidecars",
    "托管式桥接 sidecar",
    "DS Agent-managed external bridge services",
    "由 DS Agent 自动安装或管理的外部桥接服务",
    "configured external bridge when it can return source-linked results",
    "External web-search bridge readiness v1",
    "Bridge evidence safety v1",
  ]) {
    if (collapsedReadme.includes(phrase)) {
      failures.push(`README.md must describe optional local bridge limits without stale external-bridge wording: ${phrase}`);
    } else {
      checks.push(`README.md no stale external-bridge wording: ${phrase}`);
    }
  }
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "package-internal duplicate ids are counted as skipped",
    "release notes package-internal duplicate import preview ids",
  );
  const eventStore = readText("apps/desktop/src-tauri/src/kernel/event_store.rs");
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/kernel/event_store.rs",
    eventStore,
    "preview_import_counts",
    "work-package import preview uses shared duplicate-aware count helper",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/kernel/event_store.rs",
    eventStore,
    "work_package_import_preview_counts_duplicate_package_ids_as_skipped",
    "work-package import preview regression counts duplicate package ids as skipped",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "resolved memory candidates stay out of exported work packages",
    "release notes resolved memory candidates stay local-only during export",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "imported memory candidates drop source-machine source links",
    "release notes imported memory candidates drop source-machine source links",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/kernel/event_store.rs",
    eventStore,
    "imported_candidate.source_id = None;",
    "memory candidate import clears source-machine source links",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/kernel/event_store.rs",
    eventStore,
    "assert_eq!(imported.candidate.source_id, None);",
    "memory candidate import regression asserts source links are cleared",
  );
  const commands = readText("apps/desktop/src-tauri/src/commands.rs");
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    commands,
    "pending_memory_candidates_for_work_package",
    "work package export filters memory candidates through pending-review helper",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    commands,
    ".list_memory_candidate_records()",
    "work package export uses projected memory candidate status",
  );

  if (app.includes("memoryCandidateRecords.slice(0, 3)")) {
    failures.push(
      "apps/desktop/src/App.tsx must not truncate the memory candidate review queue",
    );
  } else {
    checks.push("memory candidate review UI shows the full review queue");
  }
  if (app.includes("memoryRecords.slice(0, 3)")) {
    failures.push(
      "apps/desktop/src/App.tsx must not truncate the long-term memory management list",
    );
  } else {
    checks.push("long-term memory management UI shows the full editable list");
  }
  checkTextIncludesCollapsed(
    "apps/desktop/src/types.ts",
    types,
    "relation: MemoryRelationKind; note: string; updated_at",
    "linked memory summaries expose relation notes",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "linkedMemory.note",
    "Memory Studio UI shows linked-memory relation notes",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "copy.memory.candidateSuggestedActionOptions[ record.candidate.suggested_action ]",
    "Memory Studio candidate audit shows suggested action labels",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "copy.memory.maintenanceNoUserAction",
    "Memory Studio candidate audit explains background maintenance",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "linkNote",
    "Memory Studio copy includes linked-memory relation note label",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/commands.rs",
    commands,
    "pub fn link_memory_records",
    "Memory Studio command exposes manual existing-memory links",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "linkExistingMemoryRecords",
    "Memory Studio UI submits manual existing-memory links",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "existingLinked",
    "Memory Studio copy confirms manual existing-memory links",
  );
}

function checkWorkPackageImportPreviewUi() {
  const app = readText("apps/desktop/src/App.tsx");
  const i18n = readText("apps/desktop/src/i18n.ts");

  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "importPreview.memory_candidates.new",
    "import preview UI shows new pending memory candidates",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "importPreview.memory_candidates.skipped",
    "import preview UI shows skipped pending memory candidates",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "previewNewMemoryCandidates",
    "import preview copy includes new pending memory candidates",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "previewSkippedMemoryCandidates",
    "import preview copy includes skipped pending memory candidates",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "importPreview.memory_candidates.review_supported",
    "import preview UI shows memory candidate review support",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "importPreview.operations_briefing_runs.replay_supported",
    "import preview UI shows archived briefing replay support",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "importPreview.workflow_templates.import_supported",
    "import preview UI shows workflow template import support",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "previewMemoryCandidateReviewSupported",
    "import preview copy includes memory candidate review support",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "previewArchiveReplaySupported",
    "import preview copy includes archived briefing replay support",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "previewWorkflowTemplateImportSupported",
    "import preview copy includes workflow template import support",
  );
}

function checkSoulProfileSettingsAndBootstrap() {
  const app = readText("apps/desktop/src/App.tsx");

  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "soul_profile_bootstrap: string | null;",
    "Soul bootstrap is stored as conversation-level context",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "function buildAgentSoulBootstrapContextSection",
    "Soul bootstrap has a dedicated prompt section",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "const loadSoulProfileStateForBootstrap = async",
    "new conversation first-send path refreshes Soul profile state",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "await loadSoulProfileStateForBootstrap()",
    "new conversation first send reads current Soul profile",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "Keep this Soul context outside older-turn compression so compression does not erase it.",
    "Soul bootstrap is protected from chat-history compression",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "if (soulBootstrapSection && shouldCompress)",
    "compressed conversation prompts preserve Soul bootstrap",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "createEmptyAgentConversation( agentSoulProfileBootstrapFromState(soulProfileState), )",
    "new conversations capture current Soul profile bootstrap",
  );

  const inlineSoulSettings =
    app.match(
      /<div className="soul-profile-settings">[\s\S]*?<div className="setup-form compact-settings-form">/,
    )?.[0] ?? "";
  if (!inlineSoulSettings) {
    failures.push("apps/desktop/src/App.tsx must include the Soul settings button block");
    return;
  }
  if (
    /soulProfileState\.summary_lines|soulProfileNotice|soulProfileError|soulProfileExists|soulProfileTemplate|soulProfileSummary/.test(
      inlineSoulSettings,
    )
  ) {
    failures.push(
      "apps/desktop/src/App.tsx Soul settings button block must not show saved status or runtime summary",
    );
    return;
  }
  checks.push("Soul settings button block hides runtime summary and saved status");
}

function checkOperationsBriefingArchiveReplayUi() {
  const app = readText("apps/desktop/src/App.tsx");
  const i18n = readText("apps/desktop/src/i18n.ts");
  const releaseNotes = readText("docs/RELEASE_NOTES_v0.1.0.md");
  const workPackageKernel = readText("apps/desktop/src-tauri/src/kernel/work_package.rs");
  const eventStoreKernel = readText("apps/desktop/src-tauri/src/kernel/event_store.rs");

  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "operationsBriefingRuns.map",
    "Operations Briefing UI renders the full run archive",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "runs:",
    "Operations Briefing copy includes run archive heading",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/App.tsx",
    app,
    "operationsBriefingRun.archived_from_package && !operationsBriefingRun.evidence_folder_path",
    "Operations Briefing UI shows redacted imported evidence handles",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "archiveEvidenceRedacted",
    "Operations Briefing copy includes redacted imported evidence handle label",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "redacted source-machine evidence handles",
    "release notes archived briefing evidence handle redaction",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "exported work packages redact source-machine evidence handles",
    "release notes work-package export evidence handle redaction",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/kernel/work_package.rs",
    workPackageKernel,
    "operations_export_package_redacts_local_anomaly_evidence_refs",
    "work-package export redacts local anomaly evidence refs",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/kernel/work_package.rs",
    workPackageKernel,
    "operations_export_package_redacts_local_evidence_path_mentions",
    "work-package export redacts local evidence path text mentions",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/kernel/work_package.rs",
    workPackageKernel,
    "REDACTED_SOURCE_MACHINE_EVIDENCE_HANDLE",
    "work-package export keeps a shared source-machine evidence redaction label",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/kernel/work_package.rs",
    workPackageKernel,
    "redact_source_machine_evidence_text",
    "work-package export sanitizes source-machine evidence handles in text fields",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/kernel/event_store.rs",
    eventStoreKernel,
    "archive_replay_import_redacts_local_evidence_path_mentions",
    "archive replay import redacts local evidence path text mentions",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src-tauri/src/kernel/event_store.rs",
    eventStoreKernel,
    "redact_operations_briefing_run_for_package_export(run.clone())",
    "archive replay import reuses work-package briefing redaction",
  );
}

function checkComputerUseDocs() {
  const releaseNotes = readText("docs/RELEASE_NOTES_v0.1.0.md");
  const collapsedReleaseNotes = releaseNotes.replace(/\s+/g, " ").trim();
  const readme = readText("README.md");
  const i18n = readText("apps/desktop/src/i18n.ts");

  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "Computer Use remains experimental and high-risk",
    "release notes Computer Use experimental high-risk",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "foreground desktop, secure desktop, Screen Recording, and Accessibility limitations",
    "release notes Computer Use OS limitations",
  );
  if (collapsedReleaseNotes.includes("ComputerControl also needs a one-shot ComputerControl approval")) {
    failures.push(
      "docs/RELEASE_NOTES_v0.1.0.md must not repeat internal ComputerControl wording in the user-facing Computer Use bullet",
    );
  } else {
    checks.push("release notes no repeated internal ComputerControl wording");
  }
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "screen capture follows the selected access-mode policy",
    "release notes screen-capture access-mode policy",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "computer control also needs a one-shot approval",
    "release notes user-facing computer-control one-shot wording",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "电脑控制审批请求；批准后可重试一次",
    "ComputerControl zh pending hint discloses one-shot approval",
  );
  checkTextIncludesCollapsed(
    "apps/desktop/src/i18n.ts",
    i18n,
    "A computer control approval request was created. After approval, retry once",
    "Computer control en pending hint discloses one-shot approval",
  );
  checkTextIncludesCollapsed(
    "README.md",
    readme,
    "one-shot approval plus a local in-memory unlock code",
    "README.md user-facing computer-control one-shot approval",
  );
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "one-shot approval",
    "release notes ComputerControl one-shot approval",
  );
  const staleReleaseNoteApprovalBoundaryWording =
    "Browser form submission and terminal write are approval/audit boundaries";
  if (collapsedReleaseNotes.includes(staleReleaseNoteApprovalBoundaryWording)) {
    failures.push(
      `docs/RELEASE_NOTES_v0.1.0.md must describe deferred browser/terminal actions without approval/audit boundary jargon: ${staleReleaseNoteApprovalBoundaryWording}`,
    );
  } else {
    checks.push("release notes no approval/audit boundary jargon");
  }
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "Browser form submission and terminal write keep approval and audit records; they are not broad automation executors",
    "release notes user-facing browser and terminal limit wording",
  );
  const staleReleaseNoteBridgeServiceWording =
    "Managed external bridge sidecar installation is deferred";
  if (collapsedReleaseNotes.includes(staleReleaseNoteBridgeServiceWording)) {
    failures.push(
      `docs/RELEASE_NOTES_v0.1.0.md must describe external bridge limits without sidecar wording: ${staleReleaseNoteBridgeServiceWording}`,
    );
  } else {
    checks.push("release notes no bridge sidecar wording");
  }
  const staleReleaseNoteExternalBridgeServiceWording =
    "DS Agent does not install or manage external bridge services in this preview";
  if (collapsedReleaseNotes.includes(staleReleaseNoteExternalBridgeServiceWording)) {
    failures.push(
      `docs/RELEASE_NOTES_v0.1.0.md must describe bridge-service limits as local user-managed services: ${staleReleaseNoteExternalBridgeServiceWording}`,
    );
  } else {
    checks.push("release notes no stale external bridge service wording");
  }
  checkTextIncludesCollapsed(
    "docs/RELEASE_NOTES_v0.1.0.md",
    releaseNotes,
    "DS Agent does not install or manage local bridge services in this preview",
    "release notes user-facing local bridge service limit",
  );
}

function checkGovernanceDocs() {
  for (const docPath of requiredGovernanceDocs) {
    if (!existsSync(docPath)) {
      failures.push(`${docPath} is required for the open-source release baseline`);
      continue;
    }
    checks.push(`${docPath} present`);
  }

  const license = readText("LICENSE");
  if (!license.includes("Apache License") || !license.includes("Version 2.0")) {
    failures.push("LICENSE must contain the Apache License 2.0 text");
  } else {
    checks.push("LICENSE Apache-2.0 text");
  }

  const openSourceRelease = readText("docs/OPEN_SOURCE_RELEASE.md");
  if (!openSourceRelease.includes("Apache-2.0")) {
    failures.push("docs/OPEN_SOURCE_RELEASE.md must document the Apache-2.0 release policy");
  } else {
    checks.push("open-source release Apache policy");
  }
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "Maintainer handoff notes, decision logs, and `docs/superpowers/` planning files are local-only continuation material",
    "open-source release local-only continuation docs",
  );

  const securityPolicy = readText("SECURITY.md");
  checkTextIncludes(
    "SECURITY.md",
    securityPolicy,
    "0.1.0 preview",
    "SECURITY.md current preview scope",
  );
  checkTextIncludes(
    "SECURITY.md",
    securityPolicy,
    "source-first public preview",
    "SECURITY.md source-first public preview scope",
  );
  checkTextIncludes(
    "SECURITY.md",
    securityPolicy,
    "not an official DeepSeek product",
    "SECURITY.md non-official affiliation",
  );
  checkTextIncludes(
    "SECURITY.md",
    securityPolicy,
    "GitHub Private Vulnerability Reporting",
    "SECURITY.md private vulnerability route",
  );
  checkTextIncludes(
    "SECURITY.md",
    securityPolicy,
    "Do not open a public issue with exploit details, secrets",
    "SECURITY.md public issue secret warning",
  );
  checkTextIncludesCollapsed(
    "SECURITY.md",
    securityPolicy,
    "Computer Use remains experimental and high-risk",
    "SECURITY.md Computer Use high-risk status",
  );
  const collapsedSecurityPolicy = securityPolicy.replace(/\s+/g, " ").trim();
  for (const phrase of [
    "ComputerControl requires explicit approval",
    "NetworkSearch evidence must preserve source URLs",
    "External Computer Use bridge routes require a local loopback HTTP bridge in MVP",
    "managed sidecar spawning is deferred",
    "Managed external bridge sidecar installation or supervision",
    "External desktop bridge use requires a user-started local loopback bridge in this preview",
    "DS Agent does not install, launch, or supervise external bridge services",
  ]) {
    if (collapsedSecurityPolicy.includes(phrase)) {
      failures.push(`SECURITY.md must not use internal capability wording in security boundaries: ${phrase}`);
    } else {
      checks.push(`SECURITY.md no internal capability wording ${phrase}`);
    }
  }
  checkTextIncludesCollapsed(
    "SECURITY.md",
    securityPolicy,
    "Computer control requires explicit approval plus a short local unlock window",
    "SECURITY.md user-facing computer-control security boundary",
  );
  checkTextIncludesCollapsed(
    "SECURITY.md",
    securityPolicy,
    "Web search evidence must preserve source URLs",
    "SECURITY.md user-facing web-search evidence boundary",
  );
  checkTextIncludesCollapsed(
    "SECURITY.md",
    securityPolicy,
    "Optional local desktop bridge use requires a user-started local loopback bridge in this preview",
    "SECURITY.md user-facing local bridge boundary",
  );
  checkTextIncludesCollapsed(
    "SECURITY.md",
    securityPolicy,
    "DS Agent does not install, launch, or supervise local bridge services",
    "SECURITY.md user-managed local bridge service boundary",
  );
}

function checkContributingPolicyStatus() {
  const contributing = readText("CONTRIBUTING.md");

  checkTextIncludesCollapsed(
    "CONTRIBUTING.md",
    contributing,
    "Windows build/install/launch/run is locally verified",
    "CONTRIBUTING.md Windows local validation status",
  );
  checkTextIncludesCollapsed(
    "CONTRIBUTING.md",
    contributing,
    "existing DeepSeek-first workflows, permissions, memory, Windows setup behavior, and Operations Briefing",
    "CONTRIBUTING.md local polish scope",
  );

  const staleSnippets = [
    "until the Windows baseline is genuinely usable",
    "until the Windows baseline is usable",
  ];
  const collapsedContributing = contributing.replace(/\s+/g, " ").trim();

  for (const snippet of staleSnippets) {
    const collapsedSnippet = snippet.replace(/\s+/g, " ").trim();
    if (collapsedContributing.includes(collapsedSnippet)) {
      failures.push(`CONTRIBUTING.md must not include stale Windows baseline wording: ${snippet}`);
      continue;
    }
    checks.push(`CONTRIBUTING.md no stale wording ${snippet}`);
  }

  if (collapsedContributing.includes("NetworkSearch must preserve source links")) {
    failures.push("CONTRIBUTING.md must not use internal NetworkSearch wording in contributor safety boundaries");
  } else {
    checks.push("CONTRIBUTING.md no internal NetworkSearch safety wording");
  }
  checkTextIncludesCollapsed(
    "CONTRIBUTING.md",
    contributing,
    "Web search must preserve source links",
    "CONTRIBUTING.md user-facing web-search safety wording",
  );
}

function checkOpenSourceReleaseStatus() {
  const openSourceRelease = readText("docs/OPEN_SOURCE_RELEASE.md");
  const collapsedOpenSourceRelease = openSourceRelease.replace(/\s+/g, " ").trim();
  const staleOpenSourceWindowsGoal =
    "and a Windows build/install/launch/run path is locally verified through the local release gate";
  if (collapsedOpenSourceRelease.includes(staleOpenSourceWindowsGoal)) {
    failures.push(
      `docs/OPEN_SOURCE_RELEASE.md must describe the Windows validation goal without awkward grammar: ${staleOpenSourceWindowsGoal}`,
    );
  } else {
    checks.push("open-source release no awkward Windows validation goal wording");
  }
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "a locally verified Windows build/install/launch/run path",
    "open-source release Windows local validation status",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "existing DeepSeek-first workflows, permissions, memory, Windows setup behavior, and Operations Briefing",
    "open-source release local polish scope",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "local runtime logs and temporary files",
    "open-source release local runtime artifact policy",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "local `.env` files",
    "open-source release local env file artifact policy",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "credential, private-key, and certificate files",
    "open-source release local credential artifact policy",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "dependency install directories and frontend/Rust build output",
    "open-source release dependency and build output artifact policy",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "reference repositories and generated Tauri state directories",
    "open-source release reference and generated state artifact policy",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "generated Tauri resource directories",
    "open-source release generated Tauri resource artifact policy",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "SQLite database files",
    "open-source release SQLite database artifact policy",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "SQLite sidecar files",
    "open-source release SQLite sidecar artifact policy",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "Tauri bundle directories",
    "open-source release Tauri bundle artifact policy",
  );

  const staleOpenSourceBridgeNonGoal = "No managed external bridge sidecar";
  if (collapsedOpenSourceRelease.includes(staleOpenSourceBridgeNonGoal)) {
    failures.push(
      `docs/OPEN_SOURCE_RELEASE.md must describe external bridge non-goals without sidecar wording: ${staleOpenSourceBridgeNonGoal}`,
    );
  } else {
    checks.push("open-source release no bridge sidecar non-goal wording");
  }
  const staleOpenSourceExternalBridgeNonGoal = "No DS Agent-managed external bridge service";
  if (collapsedOpenSourceRelease.includes(staleOpenSourceExternalBridgeNonGoal)) {
    failures.push(
      `docs/OPEN_SOURCE_RELEASE.md must describe bridge-service non-goals as local user-managed services: ${staleOpenSourceExternalBridgeNonGoal}`,
    );
  } else {
    checks.push("open-source release no stale external bridge service non-goal");
  }
  const staleOpenSourceExternalBridgeEnv =
    ".env.example documents local DeepSeek and external bridge environment variables";
  if (collapsedOpenSourceRelease.includes(staleOpenSourceExternalBridgeEnv)) {
    failures.push(
      `docs/OPEN_SOURCE_RELEASE.md must describe bridge env vars as optional local bridge configuration: ${staleOpenSourceExternalBridgeEnv}`,
    );
  } else {
    checks.push("open-source release no stale external bridge env wording");
  }
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "No DS Agent-managed local bridge service",
    "open-source release user-facing local bridge service non-goal",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "`.env.example` documents local DeepSeek and optional local bridge environment variables",
    "open-source release optional local bridge env wording",
  );
  if (collapsedOpenSourceRelease.includes("approval-boundary only")) {
    failures.push("docs/OPEN_SOURCE_RELEASE.md must not use approval-boundary jargon in preview honesty rules");
  } else {
    checks.push("open-source release no approval-boundary jargon");
  }
  if (collapsedOpenSourceRelease.includes("approval/audit surfaces only")) {
    failures.push("docs/OPEN_SOURCE_RELEASE.md must not use slash-style approval/audit wording in preview honesty rules");
  } else {
    checks.push("open-source release no slash-style approval/audit wording");
  }
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "approval and audit records only",
    "open-source release user-facing approval and audit preview honesty wording",
  );

  const staleSnippets = [
    "clear path toward reliable Windows launch",
    "before the Windows baseline is genuinely usable",
    "before the Windows baseline is usable",
    "Non-Goals Before The Windows 0.0.1 Baseline",
    "Non-Goals Before The Windows 0.1.0 Baseline",
  ];

  for (const snippet of staleSnippets) {
    if (openSourceRelease.includes(snippet)) {
      failures.push(`docs/OPEN_SOURCE_RELEASE.md must not include stale Windows baseline wording: ${snippet}`);
      continue;
    }
    checks.push(`open-source release no stale wording ${snippet}`);
  }

  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "For the formal `v0.1.0` line, publish a GitHub release with the Windows installer attached after final verification passes",
    "open-source release Windows installer release strategy",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "Keep the already-published `v0.0.1` source snapshot, tag, and release unchanged",
    "open-source release immutable v0.0.1 snapshot policy",
  );
  checkTextIncludesCollapsed(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "Run final local release verification on the release branch before any publication decision",
    "open-source release final local verification policy",
  );

  const staleTagPolicySnippets = [
    "## Required Before Public GitHub Release",
    "Publish `v0.1.0` as source-first",
    "moving a public release tag",
    "moving any public GitHub release tag",
    "Run final verification on the release branch.",
  ];

  for (const snippet of staleTagPolicySnippets) {
    if (openSourceRelease.includes(snippet)) {
      failures.push(`docs/OPEN_SOURCE_RELEASE.md must not imply tag mutation or premature 0.1.0 publishing: ${snippet}`);
      continue;
    }
    checks.push(`open-source release no stale tag policy ${snippet}`);
  }
}

function checkEnvironmentHygiene() {
  if (!existsSync(".env.example")) {
    failures.push(".env.example is required for local environment documentation");
    return;
  }

  const envExample = readText(".env.example");
  checks.push(".env.example present");

  if (!/^DEEPSEEK_API_KEY=\s*$/m.test(envExample)) {
    failures.push(".env.example must keep DEEPSEEK_API_KEY blank");
  } else {
    checks.push(".env.example blank DeepSeek key");
  }

  if (!envExample.includes("Do not commit .env")) {
    failures.push(".env.example must warn not to commit .env");
  } else {
    checks.push(".env.example local-secret warning");
  }

  if (!envExample.includes("DEEPSEEK_BRIEFING_EVIDENCE_DIR=docs/templates/operations-briefing-smoke-evidence")) {
    failures.push(".env.example must point briefing smoke evidence at operations-briefing-smoke-evidence");
  } else {
    checks.push(".env.example briefing smoke evidence path");
  }

  if (!envExample.includes(smokeEvidenceSafetyDisclaimer) || !envExample.includes(smokeEvidenceReplacementWarning)) {
    failures.push(".env.example must keep the briefing smoke-sample safety disclaimer");
  } else {
    checks.push(".env.example briefing smoke evidence safety disclaimer");
  }

  const envReadableText = envExample
    .replace(/^\s*#\s?/gm, "")
    .replace(/\s+/g, " ")
    .trim();
  const bridgeRouteWording =
    "Local bridge routes can enable screen inspection, computer control, and source-linked web search when a supported loopback bridge is running.";
  if (!envReadableText.includes(bridgeRouteWording)) {
    failures.push(`.env.example must include ${bridgeRouteWording}`);
  } else {
    checks.push(".env.example user-facing bridge route wording");
  }

  const staleBridgeRouteWording = "External Computer Use and native NetworkSearch bridge routes";
  if (envExample.includes(staleBridgeRouteWording)) {
    failures.push(`.env.example must not use internal bridge route wording: ${staleBridgeRouteWording}`);
  } else {
    checks.push(".env.example no internal bridge route wording");
  }

  const openSourceRelease = readText("docs/OPEN_SOURCE_RELEASE.md");
  if (!openSourceRelease.includes(".env.example")) {
    failures.push("docs/OPEN_SOURCE_RELEASE.md must document .env.example as a release hygiene artifact");
  } else {
    checks.push("open-source release env example policy");
  }
}

function checkGitHubReleaseHygiene() {
  for (const filePath of requiredGitHubHygieneFiles) {
    if (!existsSync(filePath)) {
      failures.push(`${filePath} is required for GitHub release hygiene`);
      continue;
    }
    checks.push(`${filePath} present`);
  }

  const noStaleAlphaFiles = [
    ".env.example",
    ".github/pull_request_template.md",
    ".github/ISSUE_TEMPLATE/bug_report.yml",
  ];
  for (const filePath of noStaleAlphaFiles) {
    if (readText(filePath).includes("v0.1-alpha")) {
      failures.push(`${filePath} must not describe the current release as v0.1-alpha`);
      continue;
    }
    checks.push(`${filePath} current release label`);
  }

  const pullRequestTemplate = readText(".github/pull_request_template.md");
  checkTextIncludes(
    ".github/pull_request_template.md",
    pullRequestTemplate,
    "0.1.0 preview scope",
    "PR template current preview scope",
  );
  checkTextIncludes(
    ".github/pull_request_template.md",
    pullRequestTemplate,
    "does not imply official DeepSeek affiliation",
    "PR template non-official affiliation check",
  );
  checkTextIncludes(
    ".github/pull_request_template.md",
    pullRequestTemplate,
    "No secrets, API keys",
    "PR template secret-safety check",
  );
  if (pullRequestTemplate.includes("NetworkSearch evidence still preserves source links")) {
    failures.push(".github/pull_request_template.md must not use internal NetworkSearch wording in safety checks");
  } else {
    checks.push("PR template no internal NetworkSearch safety wording");
  }
  checkTextIncludes(
    ".github/pull_request_template.md",
    pullRequestTemplate,
    "Web search evidence still preserves source links",
    "PR template user-facing web-search source-link check",
  );

  const bugReportTemplate = readText(".github/ISSUE_TEMPLATE/bug_report.yml");
  checkTextIncludes(
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    bugReportTemplate,
    "current 0.1.0 preview scope",
    "bug report current preview scope",
  );
  checkTextIncludes(
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    bugReportTemplate,
    "GitHub Private Vulnerability Reporting",
    "bug report private vulnerability route",
  );
  checkTextIncludes(
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    bugReportTemplate,
    "API keys",
    "bug report secret warning",
  );
  if (bugReportTemplate.includes("- NetworkSearch")) {
    failures.push(".github/ISSUE_TEMPLATE/bug_report.yml must not expose internal NetworkSearch as a user-facing issue area");
  } else {
    checks.push("bug report no internal NetworkSearch issue area");
  }
  checkTextIncludes(
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    bugReportTemplate,
    "- Web search",
    "bug report user-facing web-search issue area",
  );

  const compatibilityTemplate = readText(".github/ISSUE_TEMPLATE/deepseek_compatibility.yml");
  checkTextIncludes(
    ".github/ISSUE_TEMPLATE/deepseek_compatibility.yml",
    compatibilityTemplate,
    "DeepSeek-first behavior already in scope",
    "DeepSeek compatibility scope",
  );
  checkTextIncludes(
    ".github/ISSUE_TEMPLATE/deepseek_compatibility.yml",
    compatibilityTemplate,
    "Do not paste API keys",
    "DeepSeek compatibility secret warning",
  );

  const issueConfig = readText(".github/ISSUE_TEMPLATE/config.yml");
  checkTextIncludes(
    ".github/ISSUE_TEMPLATE/config.yml",
    issueConfig,
    "blank_issues_enabled: false",
    "issue template disables blank issues",
  );
  checkTextIncludes(
    ".github/ISSUE_TEMPLATE/config.yml",
    issueConfig,
    "security/advisories/new",
    "issue template private security route",
  );

  const ciWorkflow = readText(".github/workflows/ci.yml");
  const requiredCiSnippets = [
    ["runs-on: windows-latest", "CI Windows runner"],
    ["permissions:", "CI declares workflow token permissions"],
    ["contents: read", "CI read-only contents permission"],
    ["pnpm@9.15.9", "CI pinned pnpm"],
    ["node scripts/secret-scan.mjs", "CI secret scan"],
    ["pnpm --filter @deepseek-agent-os/desktop build", "CI desktop frontend build"],
    [
      'TAURI_CONFIG: \'{"bundle":{"active":false,"resources":null}}\'',
      "CI Rust test disables bundle resources",
    ],
    ["cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml", "CI Rust tests"],
    ["pnpm test:release-source", "CI source-only release guard"],
  ];

  for (const [snippet, checkName] of requiredCiSnippets) {
    checkTextIncludes(".github/workflows/ci.yml", ciWorkflow, snippet, checkName);
  }

  if (ciWorkflow.includes("DEEPSEEK_API_KEY")) {
    failures.push(".github/workflows/ci.yml must not require DeepSeek secrets");
  } else {
    checks.push("CI does not require DeepSeek secrets");
  }

  if (ciWorkflow.includes("pnpm stage:webview2-loader")) {
    failures.push(".github/workflows/ci.yml must not stage WebView2Loader before Rust unit tests");
  } else {
    checks.push("CI does not stage WebView2Loader before Rust unit tests");
  }

  const ciWriteBoundaryFindings = ciWriteBoundaryViolations(ciWorkflow);
  if (ciWriteBoundaryFindings.length > 0) {
    failures.push(
      `.github/workflows/ci.yml must stay read-only and source-only: ${ciWriteBoundaryFindings.join(", ")}`,
    );
  } else {
    checks.push("CI has no release upload or write-token steps");
  }

  const openSourceRelease = readText("docs/OPEN_SOURCE_RELEASE.md");
  for (const filePath of requiredGitHubHygieneFiles) {
    if (filePath === ".github/ISSUE_TEMPLATE/config.yml") {
      continue;
    }
    if (!openSourceRelease.includes(filePath)) {
      failures.push(`docs/OPEN_SOURCE_RELEASE.md must document ${filePath}`);
      continue;
    }
    checks.push(`open-source release documents ${filePath}`);
  }
  checkTextIncludes(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "runs `pnpm test:release-source`",
    "open-source release documents CI source-only guard",
  );
  checkTextIncludes(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "`permissions: contents: read`",
    "open-source release documents CI read-only token permission",
  );
  checkTextIncludes(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "does not upload release assets or artifacts",
    "open-source release documents CI no-upload policy",
  );
}

function checkPackageManagerBaseline() {
  if (!existsSync("pnpm-workspace.yaml")) {
    failures.push("pnpm-workspace.yaml is required for the source build workspace");
  } else {
    const workspace = readText("pnpm-workspace.yaml");
    checkTextIncludes(
      "pnpm-workspace.yaml",
      workspace,
      '  - "apps/*"',
      "pnpm workspace includes desktop apps",
    );
  }

  if (!existsSync("pnpm-lock.yaml")) {
    failures.push("pnpm-lock.yaml is required for reproducible source installs");
  } else {
    const lockfile = readText("pnpm-lock.yaml");
    checkTextIncludes("pnpm-lock.yaml", lockfile, "lockfileVersion: '9.0'", "pnpm lockfile v9");
    checkTextIncludes("pnpm-lock.yaml", lockfile, "apps/desktop:", "pnpm lockfile desktop importer");
    checkTextIncludes(
      "pnpm-lock.yaml",
      lockfile,
      "@tauri-apps/api",
      "pnpm lockfile desktop dependencies",
    );
  }

  const ciWorkflow = readText(".github/workflows/ci.yml");
  checkTextIncludes(
    ".github/workflows/ci.yml",
    ciWorkflow,
    "pnpm install --frozen-lockfile",
    "CI frozen pnpm install",
  );

  for (const docPath of ["README.md", "README.zh-CN.md", "docs/INSTALLATION.md"]) {
    const content = readText(docPath);
    checkTextIncludes(docPath, content, "npx pnpm@9.15.9 install", `${docPath} pinned pnpm install`);
    checkTextIncludes(docPath, content, "npx pnpm@9.15.9 test", `${docPath} pinned pnpm test`);
  }
}

function checkTextIncludes(filePath, content, expected, checkName) {
  if (!content.includes(expected)) {
    failures.push(`${filePath} must include ${expected}`);
    return;
  }
  checks.push(checkName);
}

function checkTextDoesNotInclude(filePath, content, rejected, checkName) {
  if (content.includes(rejected)) {
    failures.push(`${filePath} must not include ${rejected}`);
    return;
  }
  checks.push(checkName);
}

function checkTextIncludesCollapsed(filePath, content, expected, checkName) {
  const collapsedContent = content.replace(/\s+/g, " ").trim();
  const collapsedExpected = expected.replace(/\s+/g, " ").trim();
  if (!collapsedContent.includes(collapsedExpected)) {
    failures.push(`${filePath} must include ${expected}`);
    return;
  }
  checks.push(checkName);
}

function checkCargoLicense() {
  const cargoToml = readText("apps/desktop/src-tauri/Cargo.toml");
  if (!/^license\s*=\s*"Apache-2\.0"\s*$/m.test(cargoToml)) {
    failures.push("apps/desktop/src-tauri/Cargo.toml license must be Apache-2.0");
    return;
  }
  checks.push("apps/desktop/src-tauri/Cargo.toml license");
}

function checkPackageScripts() {
  const packageJson = readJson("package.json");
  const scripts = packageJson.scripts ?? {};

  for (const [scriptName, expectedCommand] of requiredPackageScripts) {
    if (scripts[scriptName] !== expectedCommand) {
      failures.push(`package.json script ${scriptName} must be ${expectedCommand}`);
      continue;
    }

    checks.push(`package.json script ${scriptName}`);

    const scriptPath = expectedCommand.replace(/^node\s+/, "");
    if (!existsSync(scriptPath)) {
      failures.push(`${scriptPath} is required by package.json script ${scriptName}`);
      continue;
    }

    checks.push(`${scriptPath} present`);
  }
}

function checkSecretScanHygiene() {
  const scriptPath = "scripts/secret-scan.mjs";
  if (!existsSync(scriptPath)) {
    failures.push(`${scriptPath} is required for local and CI secret scanning`);
    return;
  }

  const secretScan = readText(scriptPath);
  const requiredSnippets = [
    ["\"git\"", "secret scan invokes git"],
    ["\"ls-files\"", "secret scan enumerates git source files"],
    ["\"--cached\"", "secret scan covers tracked files"],
    ["\"--others\"", "secret scan covers untracked source files"],
    ["\"--exclude-standard\"", "secret scan respects ignore rules"],
    ["Candidate values are intentionally not printed.", "secret scan redacts candidate values"],
    ["const envKey = \"DEEPSEEK_API_KEY\";", "secret scan self-test names DeepSeek key safely"],
    ["${envKey}=${fakeKey}", "secret scan self-test detects env-file key assignments"],
    ["Authorization: Bearer ${fakeKey}", "secret scan self-test detects bearer keys"],
    ["$env:${envKey} = \"${fakeKey}\"", "secret scan self-test detects PowerShell env key assignments"],
    ["${envKey}=", "secret scan self-test allows blank env examples"],
  ];

  for (const [snippet, checkName] of requiredSnippets) {
    checkTextIncludes(scriptPath, secretScan, snippet, checkName);
  }
}

function checkRootWorkspaceScripts() {
  const packageJson = readJson("package.json");
  const scripts = packageJson.scripts ?? {};
  const expectedRootScripts = new Map([
    ["dev", "node scripts/require-desktop-workspace.mjs && pnpm --filter @deepseek-agent-os/desktop dev"],
    ["build", "node scripts/require-desktop-workspace.mjs && pnpm --filter @deepseek-agent-os/desktop build"],
    ["tauri", "node scripts/require-desktop-workspace.mjs && pnpm --filter @deepseek-agent-os/desktop tauri"],
  ]);

  for (const [scriptName, expectedCommand] of expectedRootScripts) {
    if (scripts[scriptName] !== expectedCommand) {
      failures.push(`package.json script ${scriptName} must be ${expectedCommand}`);
      continue;
    }
    checks.push(`package.json root ${scriptName} workspace guard`);
  }

  const helperPath = "scripts/require-desktop-workspace.mjs";
  if (!existsSync(helperPath)) {
    failures.push(`${helperPath} is required by root workspace scripts`);
    return;
  }
  checks.push(`${helperPath} present`);

  const helper = readText(helperPath);
  checkTextIncludes(
    helperPath,
    helper,
    "apps/desktop/package.json",
    `${helperPath} names desktop workspace package`,
  );
  checkTextIncludes(
    helperPath,
    helper,
    "source checkout",
    `${helperPath} uses source-checkout wording`,
  );

  if (/Foundation MVP/i.test(helper)) {
    failures.push(`${helperPath} must not use stale Foundation MVP wording`);
  } else {
    checks.push(`${helperPath} no stale Foundation MVP wording`);
  }

  const openSourceRelease = readText("docs/OPEN_SOURCE_RELEASE.md");
  checkTextIncludes(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    helperPath,
    "open-source release documents root workspace helper",
  );
}

function checkSmokeScriptReleaseLabels() {
  const scriptExpectations = [
    [
      "scripts/deepseek-smoke.mjs",
      "DeepSeek-Agent-OS/0.1.0 local-smoke-test",
      "DeepSeek Chat smoke User-Agent release label",
    ],
    [
      "scripts/deepseek-operations-briefing-smoke.mjs",
      "DeepSeek-Agent-OS/0.1.0 operations-briefing-smoke-test",
      "DeepSeek Operations Briefing smoke User-Agent release label",
    ],
    [
      "scripts/windows-local-smoke.mjs",
      "DeepSeek-Agent-OS/0.1.0 windows-local-smoke-test",
      "Windows local smoke User-Agent release label",
    ],
  ];

  for (const [filePath, expected, checkName] of scriptExpectations) {
    const content = readText(filePath);
    checkTextIncludes(filePath, content, expected, checkName);

    if (/DeepSeek-Agent-OS\/0\.1(?:\s|$)/.test(content)) {
      failures.push(`${filePath} must not use stale DeepSeek-Agent-OS/0.1 User-Agent`);
    } else {
      checks.push(`${filePath} no stale User-Agent release label`);
    }
  }

  const rustRuntimeExpectations = [
    [
      "apps/desktop/src-tauri/src/kernel/deepseek.rs",
      "DeepSeek-Agent-OS/0.1.0 deepseek-chat",
      "DeepSeek runtime User-Agent release label",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/capability.rs",
      "DeepSeek-Agent-OS/0.1.0 browser-capability",
      "browser capability runtime User-Agent release label",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/capability.rs",
      "DeepSeek-Agent-OS/0.1.0 network-search",
      "network search runtime User-Agent release label",
    ],
    [
      "apps/desktop/src-tauri/src/kernel/codex_bridge_http.rs",
      "DeepSeek-Agent-OS/0.1.0 external-bridge",
      "external bridge runtime User-Agent release label",
    ],
  ];

  for (const [filePath, expected, checkName] of rustRuntimeExpectations) {
    const content = readText(filePath);
    checkTextIncludes(filePath, content, expected, checkName);

    if (/DeepSeek-Agent-OS\/0\.1(?:\s|$)/.test(content)) {
      failures.push(`${filePath} must not use stale DeepSeek-Agent-OS/0.1 User-Agent`);
    } else {
      checks.push(`${filePath} no stale runtime User-Agent release label`);
    }
  }
}

function checkExternalBridgeUserVisibleErrors() {
  const rustFiles = [
    "apps/desktop/src-tauri/src/kernel/capability.rs",
    "apps/desktop/src-tauri/src/kernel/codex_bridge_contract.rs",
    "apps/desktop/src-tauri/src/kernel/codex_bridge_http.rs",
  ];
  const forbiddenPatterns = [
    /\bErr\(\s*"codex bridge\b/gi,
    /\bformat!\(\s*"codex bridge\b/gi,
    /\bErr\(\s*"external bridge\b/gi,
    /\bformat!\(\s*"external bridge\b/gi,
  ];

  for (const filePath of rustFiles) {
    const content = readText(filePath);
    for (const pattern of forbiddenPatterns) {
      for (const match of content.matchAll(pattern)) {
        const lineNumber = content.slice(0, match.index).split(/\r?\n/).length;
        failures.push(
          `${filePath}:${lineNumber} user-visible bridge errors must say local bridge service, not external/codex bridge`,
        );
      }
    }
  }

  checkTextIncludes(
    "apps/desktop/src-tauri/src/kernel/codex_bridge_http.rs",
    readText("apps/desktop/src-tauri/src/kernel/codex_bridge_http.rs"),
    "local bridge service HTTP endpoint",
    "local bridge service HTTP endpoint user-facing error wording",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/kernel/codex_bridge_contract.rs",
    readText("apps/desktop/src-tauri/src/kernel/codex_bridge_contract.rs"),
    "local bridge service network search response",
    "local bridge service contract validation user-facing error wording",
  );
  checkTextIncludes(
    "apps/desktop/src-tauri/src/kernel/capability.rs",
    readText("apps/desktop/src-tauri/src/kernel/capability.rs"),
    "local bridge service network search returned",
    "local bridge service runtime validation user-facing error wording",
  );
}

function checkLocalReleaseHelperSelfTests() {
  const windowsLocalSmoke = readText("scripts/windows-local-smoke.mjs");
  checkTextIncludes(
    "scripts/windows-local-smoke.mjs",
    windowsLocalSmoke,
    "--self-test",
    "Windows local smoke helper self-test flag",
  );
  checkTextIncludes(
    "scripts/windows-local-smoke.mjs",
    windowsLocalSmoke,
    "windows-local-smoke self-test ok",
    "Windows local smoke helper self-test success message",
  );
  const windowsInstalledUiSmoke = readText("scripts/windows-installed-ui-smoke.mjs");
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "--memory-feedback",
    "Windows installed UI memory-feedback smoke flag",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "--memory-maintenance",
    "Windows installed UI memory-maintenance smoke flag",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "--office",
    "Windows installed UI Office artifact smoke flag",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "runInstalledMemoryFeedbackSmoke",
    "Windows installed UI memory-feedback smoke helper",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "runInstalledMemoryMaintenanceSmoke",
    "Windows installed UI memory-maintenance smoke helper",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "app_data_events_restored: true",
    "Windows installed UI memory-feedback restores app-data event store",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "auto_archives_applied",
    "Windows installed UI memory-maintenance checks automatic archive summary",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "memory_feedback:",
    "Windows installed UI memory-feedback JSON output",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "memory_maintenance:",
    "Windows installed UI memory-maintenance JSON output",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "export_work_package",
    "Windows installed workflow smoke exports work package",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "deepseek-agent-os-work-package-${run.value.id}.json",
    "Windows installed workflow smoke writes work-package JSON evidence",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "work_package_run_count",
    "Windows installed workflow smoke reports work-package run count",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "runInstalledOfficeArtifactSmoke",
    "Windows installed UI Office artifact smoke helper",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "resume_agent_chat_action",
    "Windows installed UI Office smoke uses agent action dispatch path",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "New-Object -ComObject Word.Application",
    "Windows installed UI Office smoke verifies Word can open artifact",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "-EncodedCommand",
    "Windows installed UI Office smoke uses whole-script PowerShell execution",
  );
  checkTextIncludes(
    "scripts/windows-installed-ui-smoke.mjs",
    windowsInstalledUiSmoke,
    "office_artifact:",
    "Windows installed UI Office artifact JSON output",
  );

  const releaseLocal = readText("scripts/release-local-check.mjs");
  checkTextIncludes(
    "scripts/release-local-check.mjs",
    releaseLocal,
    "scripts/windows-local-smoke.mjs",
    "release-local runs Windows local helper self-test",
  );
  checkTextIncludes(
    "scripts/release-local-check.mjs",
    releaseLocal,
    '["git", "diff", "--cached", "--check"]',
    "release-local runs staged diff whitespace check",
  );
  checkTextIncludes(
    "scripts/release-local-check.mjs",
    releaseLocal,
    '["npx", "pnpm@9.15.9", "test:windows-installed-ui"]',
    "release-local self-test pins installed UI smoke command parts",
  );
  checkTextIncludes(
    "scripts/release-local-check.mjs",
    releaseLocal,
    '["npx", "pnpm@9.15.9", "test:windows-installed-ui", "--", "--workflow"]',
    "release-local self-test pins installed workflow smoke command parts",
  );
  checkTextIncludes(
    "scripts/release-local-check.mjs",
    releaseLocal,
    "Self-test expected skip-live command list to exclude live smoke commands.",
    "release-local self-test pins skip-live live-smoke exclusion",
  );
}

function checkOperationsBriefingSmokeEvidence() {
  if (!existsSync(smokeEvidenceDir)) {
    failures.push(`${smokeEvidenceDir} is required for live Operations Briefing smoke tests`);
    return;
  }
  checks.push(`${smokeEvidenceDir} present`);

  for (const fileName of requiredSmokeEvidenceFiles) {
    const filePath = path.join(smokeEvidenceDir, fileName);
    if (!existsSync(filePath)) {
      failures.push(`${filePath} is required for Operations Briefing smoke evidence`);
      continue;
    }

    const content = readText(filePath);
    if (!content.includes(smokeEvidenceSafetyDisclaimer) || !content.includes(smokeEvidenceReplacementWarning)) {
      failures.push(`${filePath} must keep the smoke-sample safety disclaimer`);
      continue;
    }

    checks.push(`${filePath} smoke disclaimer`);
  }

  const briefingSmokeScript = readText("scripts/deepseek-operations-briefing-smoke.mjs");
  if (!briefingSmokeScript.includes("operations-briefing-smoke-evidence")) {
    failures.push("DeepSeek briefing smoke must default to operations-briefing-smoke-evidence");
  } else {
    checks.push("DeepSeek briefing smoke sample default");
  }

  for (const docPath of detailedValidationDocs) {
    const content = readText(docPath);
    if (!content.includes("operations-briefing-smoke-evidence")) {
      failures.push(`${docPath} must document the Operations Briefing smoke evidence folder`);
      continue;
    }
    checks.push(`${docPath} smoke evidence docs`);

    if (!content.includes(smokeEvidenceSafetyDisclaimer) || !content.includes(smokeEvidenceReplacementWarning)) {
      failures.push(`${docPath} must keep the smoke-sample safety disclaimer`);
      continue;
    }
    checks.push(`${docPath} smoke evidence safety disclaimer`);

    if (!content.replace(/\s+/g, " ").trim().includes("The bundled smoke files are marked as")) {
      failures.push(`${docPath} must explain that bundled smoke files are clearly marked before operational use`);
      continue;
    }
    checks.push(`${docPath} smoke evidence marked-file warning`);
  }
}

function checkOperationsBriefingSeedEvidence() {
  if (!existsSync(seedEvidenceDir)) {
    failures.push(`${seedEvidenceDir} is required for desktop evidence template seeding`);
    return;
  }
  checks.push(`${seedEvidenceDir} present`);

  const workflow = readText("apps/desktop/src-tauri/src/kernel/workflow.rs");
  if (!workflow.includes("Blank operator templates for the Operations Briefing workflow.")) {
    failures.push("Operations Briefing workflow package must describe seed files as blank operator templates");
  } else {
    checks.push("Operations Briefing workflow package blank-template description");
  }

  const i18n = readText("apps/desktop/src/i18n.ts");
  if (!i18n.includes("Blank operator templates seeded into the local evidence folder.")) {
    failures.push("Operations Briefing UI seed success copy must describe blank operator templates");
  } else {
    checks.push("Operations Briefing UI seed success copy blank-template wording");
  }
  if (i18n.includes("Sample evidence templates")) {
    failures.push("Operations Briefing UI seed success copy must not call blank templates sample evidence");
  } else {
    checks.push("Operations Briefing UI seed success copy avoids sample-evidence wording");
  }
  if (i18n.includes("样例证据模板")) {
    failures.push("Operations Briefing zh UI seed success copy must not call blank templates sample evidence");
  } else {
    checks.push("Operations Briefing zh UI seed success copy avoids sample-evidence wording");
  }

  const seedReadmePath = path.join(seedEvidenceDir, "README.md");
  if (!existsSync(seedReadmePath)) {
    failures.push(`${seedReadmePath} is required to explain desktop evidence template seeding`);
  } else {
    const seedReadme = readText(seedReadmePath);
    if (!seedReadme.includes(seedEvidenceOperatorTemplatePhrase)) {
      failures.push(`${seedReadmePath} must describe the seed files as blank operator templates`);
    } else {
      checks.push(`${seedReadmePath} blank operator template docs`);
    }
  }

  for (const fileName of requiredSeedEvidenceFiles) {
    const filePath = path.join(seedEvidenceDir, fileName);
    if (!existsSync(filePath)) {
      failures.push(`${filePath} is required for desktop evidence template seeding`);
      continue;
    }
    const content = readText(filePath);
    if (content.includes(smokeEvidenceSafetyDisclaimer) || content.includes(smokeEvidenceReplacementWarning)) {
      failures.push(`${filePath} must stay a blank operator template, not smoke sample evidence`);
      continue;
    }
    checks.push(`${filePath} seed template`);
  }

  const windowsLocalSmoke = readText("scripts/windows-local-smoke.mjs");
  if (!windowsLocalSmoke.includes("operations-briefing-evidence")) {
    failures.push("Windows local smoke must seed the desktop Operations Briefing evidence templates");
  } else {
    checks.push("Windows local smoke desktop evidence template seed");
  }

  for (const docPath of detailedValidationDocs) {
    const content = readText(docPath);
    if (!content.includes(seedEvidenceDir)) {
      failures.push(`${docPath} must document the desktop Operations Briefing seed evidence folder`);
      continue;
    }
    checks.push(`${docPath} seed evidence docs`);

    if (!content.includes(seedEvidenceOperatorTemplatePhrase)) {
      failures.push(`${docPath} must distinguish desktop seed evidence as blank operator templates`);
      continue;
    }
    checks.push(`${docPath} seed evidence blank-template docs`);
  }
}

function checkBuiltinPluginPackages() {
  const result = spawnSync(process.execPath, ["scripts/validate-builtin-plugins.mjs"], {
    encoding: "utf8",
  });

  if (result.status !== 0) {
    failures.push(
      `builtin plugin package validation failed: ${(result.stderr || result.stdout).trim()}`,
    );
    return;
  }

  checks.push("builtin plugin package validation");
}

function checkGitignoreEntries() {
  const gitignore = readText(".gitignore")
    .split(/\r?\n/)
    .map((line) => line.trim());

  for (const entry of requiredGitignoreEntries) {
    if (!gitignore.includes(entry)) {
      failures.push(`.gitignore must include ${entry}`);
      continue;
    }
    checks.push(`${entry} ignored`);
  }
}

function checkLineEndingPolicy() {
  if (!existsSync(".gitattributes")) {
    failures.push(".gitattributes is required to keep source line endings stable");
    return;
  }

  const attributes = readText(".gitattributes");
  checkTextIncludes(".gitattributes", attributes, "* text=auto eol=lf", ".gitattributes text LF default");
  checkTextIncludes(".gitattributes", attributes, "*.ico binary", ".gitattributes icon binary");
  checkTextIncludes(".gitattributes", attributes, "*.png binary", ".gitattributes PNG binary");
  checkTextIncludes(".gitattributes", attributes, "*.pdf binary", ".gitattributes PDF binary");
  checkTextIncludes(".gitattributes", attributes, "*.appimage binary", ".gitattributes AppImage binary");
  checkTextIncludes(".gitattributes", attributes, "*.deb binary", ".gitattributes Debian package binary");
  checkTextIncludes(".gitattributes", attributes, "*.dmg binary", ".gitattributes DMG binary");
  checkTextIncludes(".gitattributes", attributes, "*.msix binary", ".gitattributes MSIX binary");
  checkTextIncludes(".gitattributes", attributes, "*.nupkg binary", ".gitattributes NuGet package binary");
  checkTextIncludes(".gitattributes", attributes, "*.pkg binary", ".gitattributes PKG binary");
  checkTextIncludes(".gitattributes", attributes, "*.rpm binary", ".gitattributes RPM binary");
  checkTextIncludes(".gitattributes", attributes, "*.rar binary", ".gitattributes RAR archive binary");
  checkTextIncludes(".gitattributes", attributes, "*.sqlite3-wal binary", ".gitattributes SQLite WAL binary");
  checkTextIncludes(".gitattributes", attributes, "*.sqlite3-shm binary", ".gitattributes SQLite SHM binary");
  checkTextIncludes(".gitattributes", attributes, "*.sqlite3-journal binary", ".gitattributes SQLite journal binary");
  checkTextIncludes(".gitattributes", attributes, "*.pfx binary", ".gitattributes PKCS#12 PFX binary");
  checkTextIncludes(".gitattributes", attributes, "*.p12 binary", ".gitattributes PKCS#12 P12 binary");
  checkTextIncludes(".gitattributes", attributes, "*.pdb binary", ".gitattributes PDB binary");
  checkTextIncludes(".gitattributes", attributes, "*.so binary", ".gitattributes shared object binary");
  checkTextIncludes(".gitattributes", attributes, "*.dylib binary", ".gitattributes dylib binary");
  checkTextIncludes(".gitattributes", attributes, "*.node binary", ".gitattributes Node native addon binary");

  const openSourceRelease = readText("docs/OPEN_SOURCE_RELEASE.md");
  checkTextIncludes(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    ".gitattributes",
    "open-source release documents line-ending policy",
  );
  checkTextIncludes(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "installer/package artifacts",
    "open-source release documents package artifact binary policy",
  );
  checkTextIncludes(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "unexpected binary files",
    "open-source release documents unexpected binary guard",
  );
  checkTextIncludes(
    "docs/OPEN_SOURCE_RELEASE.md",
    openSourceRelease,
    "oversized source files",
    "open-source release documents oversized source guard",
  );
}

function checkSourceOnlyFiles() {
  const files = gitFiles();
  const blockedFiles = files.filter(isBlockedReleaseArtifact);
  const localOnlyFiles = files.filter(isLocalOnlyContinuationFile);
  const oversizedFiles = files.filter(isOversizedSourceFile);
  const unexpectedBinaryFiles = files.filter(isUnexpectedBinarySourceFile);

  if (blockedFiles.length > 0) {
    failures.push(
      `source-only release must not include local or binary release artifacts: ${blockedFiles.join(", ")}`,
    );
  }

  if (localOnlyFiles.length > 0) {
    failures.push(
      `source-only release must not include local-only continuation docs: ${localOnlyFiles.join(", ")}`,
    );
  }

  if (oversizedFiles.length > 0) {
    failures.push(
      `source-only release must not include oversized source files over ${maxSourceFileBytes} bytes: ${oversizedFiles.join(", ")}`,
    );
  }

  if (unexpectedBinaryFiles.length > 0) {
    failures.push(
      `source-only release must not include unexpected binary files: ${unexpectedBinaryFiles.join(", ")}`,
    );
  }

  if (
    blockedFiles.length > 0 ||
    localOnlyFiles.length > 0 ||
    oversizedFiles.length > 0 ||
    unexpectedBinaryFiles.length > 0
  ) {
    return;
  }

  checks.push(`source-only artifact scan (${files.length} files)`);
}

function checkSourceFileSizeAndBinarySelfTest() {
  const textBuffer = Buffer.from("DeepSeek source release\n", "utf8");
  const binaryBuffer = Buffer.from([0, 0, 1, 0, 1, 0]);

  if (isLikelyBinaryBuffer(textBuffer)) {
    failures.push("source binary self-test must allow ordinary UTF-8 text");
  } else {
    checks.push("source binary self-test allows UTF-8 text");
  }

  if (!isLikelyBinaryBuffer(binaryBuffer)) {
    failures.push("source binary self-test must detect NUL-containing binary content");
  } else {
    checks.push("source binary self-test detects binary content");
  }

  if (!isAllowedSourceBinaryFile("apps/desktop/src-tauri/icons/icon.ico")) {
    failures.push("source binary self-test must allow the checked-in app icon");
  } else {
    checks.push("source binary self-test allows app icon");
  }

  if (isAllowedSourceBinaryFile("docs/report.pdf")) {
    failures.push("source binary self-test must not allow arbitrary PDF binaries");
  } else {
    checks.push("source binary self-test blocks arbitrary binary docs");
  }
}

function checkLocalOnlyContinuationSelfTest() {
  const blockedSamples = [
    "SESSION_HANDOFF.md",
    "PROJECT_CONTEXT.md",
    "DECISIONS.md",
    "docs/superpowers/plans/2026-06-29-open-source-installation-guide-v1.md",
    "docs\\superpowers\\specs\\2026-06-28-deepseek-agent-os-architecture-design.md",
  ];
  const allowedSamples = [
    "README.md",
    "docs/OPEN_SOURCE_RELEASE.md",
    "docs/RELEASE_NOTES_v0.1.0.md",
  ];

  for (const samplePath of blockedSamples) {
    if (!isLocalOnlyContinuationFile(samplePath)) {
      failures.push(`local-only continuation self-test must block ${samplePath}`);
      continue;
    }
    checks.push(`local-only continuation self-test blocks ${samplePath}`);
  }

  for (const samplePath of allowedSamples) {
    if (isLocalOnlyContinuationFile(samplePath)) {
      failures.push(`local-only continuation self-test must allow ${samplePath}`);
      continue;
    }
    checks.push(`local-only continuation self-test allows ${samplePath}`);
  }
}

function checkSourceOnlyBlocklistSelfTest() {
  const blockedSamples = [
    ".env",
    ".env.local",
    "apps/desktop/.env.production",
    "kernel-events.sqlite3",
    "local-directories.json",
    "memory-cache.sqlite3",
    "kernel-events.sqlite3-wal",
    "kernel-events.sqlite3-shm",
    "kernel-events.sqlite3-journal",
    "local-signing-key.pem",
    "localhost.key",
    "windows-codesign.pfx",
    "developer-certificate.p12",
    "test-root-ca.crt",
    "localhost.cer",
    "node_modules/.pnpm/react/index.js",
    "apps/desktop/node_modules/react/index.js",
    "apps/desktop/dist/index.html",
    "apps/desktop/dist/assets/index.js",
    "target/debug/build-script-build",
    "apps/desktop/src-tauri/target/debug/build/ds-agent/out/bindings.rs",
    ".DS_Store",
    "_reference_repos/upstream/src/main.rs",
    "apps/desktop/src-tauri/.tauri/tauri.conf.json",
    "apps/desktop/src-tauri/gen/schemas/desktop-schema.json",
    "apps/desktop/src-tauri/generated/windows/webview2-loader-manifest.json",
    "tauri-dev.err.log",
    "deepseek-agent-os.tmp",
    "computer-screenshots/smoke.png",
    "ds-agent-installed-ui-2026-06-30T05-13-38-803Z.png",
    "apps/desktop/src-tauri/target/debug/ds-agent.pdb",
    "apps/desktop/src-tauri/target/release/libds_agent.so",
    "apps/desktop/src-tauri/target/release/libds_agent.dylib",
    "apps/desktop/src-tauri/target/release/bundle/macos/DS Agent.app/Contents/Info.plist",
    "apps/desktop/src-tauri/target/release/bundle/deb/control",
    "apps/desktop/node_modules/native-addon/build/Release/addon.node",
    "docs/manual.zip",
    "reports/source-package.tar",
    "tmp/local-smoke.7z",
    "tmp/local-smoke.tgz",
    "operations-briefing-00000000-0000-4000-8000-000000000000.html",
    "operations-briefing-local-smoke.md",
    "reports/operations-briefing-manual-export.pdf",
    "deepseek-agent-os-work-package-00000000-0000-4000-8000-000000000000.json",
    "deepseek-agent-os-work-package-local-smoke.json",
  ];
  const allowedSamples = [
    ".env.example",
    "README.md",
    "docs/templates/operations-briefing-smoke-evidence/revenue.md",
    "apps/desktop/src-tauri/icons/icon.ico",
  ];

  for (const samplePath of blockedSamples) {
    if (!isBlockedReleaseArtifact(samplePath)) {
      failures.push(`source-only blocklist self-test must block ${samplePath}`);
      continue;
    }
    checks.push(`source-only blocklist self-test blocks ${samplePath}`);
  }

  for (const samplePath of allowedSamples) {
    if (isBlockedReleaseArtifact(samplePath)) {
      failures.push(`source-only blocklist self-test must allow ${samplePath}`);
      continue;
    }
    checks.push(`source-only blocklist self-test allows ${samplePath}`);
  }
}

function isBlockedReleaseArtifact(filePath) {
  const normalized = filePath.replace(/\\/g, "/").toLowerCase();
  const baseName = path.basename(normalized);
  const extension = path.extname(normalized);
  const localRuntimeFileNames = new Set([
    ".ds_store",
    "kernel-events.sqlite3",
    "local-directories.json",
  ]);

  if (localRuntimeFileNames.has(baseName)) {
    return true;
  }

  if ((baseName === ".env" || baseName.startsWith(".env.")) && baseName !== ".env.example") {
    return true;
  }

  if (hasPathSegment(normalized, generatedDirectorySegments)) {
    return true;
  }

  if (
    normalized.startsWith("_reference_repos/") ||
    normalized.includes("/_reference_repos/") ||
    normalized.startsWith("apps/desktop/src-tauri/.tauri/") ||
    normalized.startsWith("apps/desktop/src-tauri/gen/") ||
    normalized.startsWith("apps/desktop/src-tauri/generated/")
  ) {
    return true;
  }

  if (normalized.includes("/computer-screenshots/") || normalized.startsWith("computer-screenshots/")) {
    return true;
  }

  if (/(^|\/)apps\/desktop\/src-tauri\/target\/(?:debug|release)\/bundle\//.test(normalized)) {
    return true;
  }

  if (/\/?ds-agent-installed-ui-[^/]+\.png$/.test(normalized)) {
    return true;
  }

  if (/^operations-briefing-.+\.(json|md|html|pdf)$/.test(baseName)) {
    return true;
  }

  if (/^deepseek-agent-os-work-package-.+\.json$/.test(baseName)) {
    return true;
  }

  if (binaryReleaseExtensions.has(extension)) {
    return true;
  }

  if (localReleaseArtifactExtensions.has(extension)) {
    return true;
  }

  if (localCredentialArtifactExtensions.has(extension)) {
    return true;
  }

  if (releaseArchiveExtensions.has(extension)) {
    return true;
  }

  return /(bundle\/nsis|setup\.exe|webview2loader\.dll)/.test(normalized);
}

function isOversizedSourceFile(filePath) {
  return statSync(filePath).size > maxSourceFileBytes;
}

function isUnexpectedBinarySourceFile(filePath) {
  if (isAllowedSourceBinaryFile(filePath)) {
    return false;
  }

  return isLikelyBinaryBuffer(readFileSync(filePath));
}

function isAllowedSourceBinaryFile(filePath) {
  return allowedSourceBinaryFiles.has(filePath.replace(/\\/g, "/"));
}

function isLikelyBinaryBuffer(buffer) {
  const sampleLength = Math.min(buffer.length, 4096);
  for (let index = 0; index < sampleLength; index += 1) {
    if (buffer[index] === 0) {
      return true;
    }
  }
  return false;
}

function isLocalOnlyContinuationFile(filePath) {
  const normalized = filePath.replace(/\\/g, "/").toLowerCase();
  return localOnlyContinuationFiles.some((entry) => {
    const normalizedEntry = entry.toLowerCase();
    if (normalizedEntry.endsWith("/")) {
      return normalized.startsWith(normalizedEntry);
    }
    return normalized === normalizedEntry;
  });
}

function hasPathSegment(normalizedPath, blockedSegments) {
  return normalizedPath.split("/").some((segment) => blockedSegments.has(segment));
}

function gitFiles() {
  const result = spawnSync(
    "git",
    ["ls-files", "--cached", "--others", "--exclude-standard", "-z"],
    {
      encoding: "utf8",
    },
  );

  if (result.error) {
    throw result.error;
  }

  if (result.status !== 0) {
    throw new Error(result.stderr || "git ls-files failed");
  }

  return result.stdout.split("\0").filter(Boolean);
}

function readJson(filePath) {
  return JSON.parse(readText(filePath));
}

function readText(filePath) {
  return readFileSync(filePath, "utf8");
}

function validateArgs(values, allowed, commandName) {
  const unknown = values.filter((arg) => !allowed.has(arg));
  if (unknown.length === 0) {
    return;
  }

  console.error(
    JSON.stringify(
      {
        ok: false,
        command: commandName,
        error: `Unknown argument(s): ${unknown.join(", ")}`,
        allowed: Array.from(allowed).sort(),
      },
      null,
      2,
    ),
  );
  process.exit(1);
}
