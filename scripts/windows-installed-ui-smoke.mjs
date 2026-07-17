#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import {
  lstat,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  realpath,
  rm,
  writeFile,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import net from "node:net";

const isWindows = process.platform === "win32";
const uiSmokeRemoteDebuggingPortEnv =
  "DS_AGENT_UI_SMOKE_REMOTE_DEBUGGING_PORT";
const rawArgs = process.argv.slice(2).filter((arg) => arg !== "--");
const allowedArgs = new Set([
  "--agent-chat",
  "--help",
  "--isolated-profile",
  "--memory-feedback",
  "--memory-maintenance",
  "--office",
  "--self-test",
  "--skill-lifecycle",
  "--workflow",
]);
validateArgs(rawArgs, allowedArgs, "test:windows-installed-ui");
validateInstalledUiSmokeFlagCombination(rawArgs);

if (rawArgs.includes("--help")) {
  console.log(
    [
      "Usage: pnpm test:windows-installed-ui [-- <flags>]",
      "",
      "Flags:",
      "  --agent-chat Exercise the installed Tauri agent chat command bridge.",
      "  --isolated-profile Run with new verified temp APPDATA and LOCALAPPDATA roots (required).",
      "  --memory-feedback Exercise installed memory candidate + selected-memory feedback bridge.",
      "  --memory-maintenance Exercise installed background memory update/archive maintenance.",
      "  --office Exercise installed Office artifact creation and Word open verification.",
      "  --self-test Run deterministic helper checks without launching DS Agent.",
      "  --skill-lifecycle Verify the installed Skills and Plugins catalog UI.",
      "  --workflow  Exercise the installed Tauri workflow and report exports.",
    ].join("\n"),
  );
  process.exit(0);
}

const args = new Set(rawArgs);
const selfTestMode = args.has("--self-test");
const executablePath = selfTestMode ? null : resolveExecutablePath();
const isolatedProfileMode = args.has("--isolated-profile");
const includeWorkflowSmoke =
  args.has("--workflow") ||
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_UI_WORKFLOW_SMOKE === "1";
const includeAgentChatSmoke =
  args.has("--agent-chat") ||
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_AGENT_CHAT_SMOKE === "1";
const includeMemoryFeedbackSmoke =
  args.has("--memory-feedback") ||
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_MEMORY_FEEDBACK_SMOKE === "1";
const includeMemoryMaintenanceSmoke =
  args.has("--memory-maintenance") ||
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_MEMORY_MAINTENANCE_SMOKE === "1";
const includeOfficeArtifactSmoke =
  args.has("--office") ||
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_OFFICE_ARTIFACT_SMOKE === "1";
const includeSkillLifecycleSmoke =
  args.has("--skill-lifecycle") ||
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_SKILL_LIFECYCLE_SMOKE === "1";
const expectModelTelemetry = Boolean(process.env.DEEPSEEK_API_KEY?.trim());
const timeoutMs = readPositiveInteger(
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_UI_TIMEOUT_MS ?? "20000",
  "DEEPSEEK_AGENT_OS_INSTALLED_UI_TIMEOUT_MS",
);
const workflowTimeoutMs = readPositiveInteger(
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_WORKFLOW_TIMEOUT_MS ?? "120000",
  "DEEPSEEK_AGENT_OS_INSTALLED_WORKFLOW_TIMEOUT_MS",
);
let screenshotDir =
  process.env.DEEPSEEK_AGENT_OS_UI_SMOKE_SCREENSHOT_DIR ??
  path.join(os.tmpdir(), "deepseek-agent-os-ui-smoke");
let workflowRootDir =
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_WORKFLOW_DIR ??
  path.join(os.tmpdir(), "deepseek-agent-os-installed-workflow-smoke");

if (!selfTestMode && !isWindows) {
  console.error("test:windows-installed-ui only runs on Windows.");
  process.exit(1);
}

if (!selfTestMode && !isolatedProfileMode) {
  console.error("test:windows-installed-ui requires --isolated-profile.");
  process.exit(1);
}

if (!selfTestMode && (!executablePath || !existsSync(executablePath))) {
  console.error(
    [
      "DS Agent executable was not found.",
      "Set DEEPSEEK_AGENT_OS_INSTALLED_EXE or install the app first.",
      `Default checked: ${defaultInstalledExecutablePath()}`,
    ].join("\n"),
  );
  process.exit(1);
}

if (!selfTestMode && typeof WebSocket !== "function") {
  console.error("This script requires a Node.js runtime with global WebSocket.");
  process.exit(1);
}

let child;
let cdp;
let isolatedProfile;

async function main() {
  try {
    isolatedProfile = await createIsolatedProfile();
    screenshotDir = isolatedProfile.screenshotDir;
    workflowRootDir = isolatedProfile.workspaceDir;
    const port = await findFreePort();
    ensureNoExternalWebViewProfileOverride(process.env);
    const env = isolatedProfileEnvironment(process.env, isolatedProfile, {
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: webView2ArgsForPort(port),
      [uiSmokeRemoteDebuggingPortEnv]: String(port),
    });
    child = spawn(executablePath, [], {
      env,
      stdio: ["ignore", "ignore", "pipe"],
      windowsHide: false,
    });

    child.stderr?.on("data", () => undefined);

    const target = await waitForWebViewTarget(port, timeoutMs);
    cdp = await CdpClient.connect(target.webSocketDebuggerUrl);
    await cdp.send("Runtime.enable");
    await cdp.send("Page.enable");
    await cdp.send("Page.bringToFront").catch(() => undefined);
    await waitForReadyState(cdp, timeoutMs);
    const bodyText = await waitForMeaningfulBodyText(cdp, timeoutMs);

    const title = await evaluate(cdp, "document.title");
    const url = await evaluate(cdp, "location.href");
    const hasTauriInternals = await evaluate(
      cdp,
      "Boolean(window.__TAURI_INTERNALS__ || window.__TAURI__)",
    );
    const onboarding = await runInstalledOnboardingSmoke(cdp, bodyText);
    const screenshotPath = await captureScreenshot(cdp, screenshotDir);
    const agentChat = includeAgentChatSmoke
      ? await runInstalledAgentChatSmoke(cdp)
      : null;
    const memoryFeedback = includeMemoryFeedbackSmoke
      ? await runInstalledMemoryFeedbackSmoke(cdp)
      : null;
    const memoryMaintenance = includeMemoryMaintenanceSmoke
      ? await runInstalledMemoryMaintenanceSmoke(cdp)
      : null;
    const workflow = includeWorkflowSmoke
      ? await runInstalledWorkflowSmoke(cdp)
      : null;
    const officeArtifact = includeOfficeArtifactSmoke
      ? await runInstalledOfficeArtifactSmoke(cdp)
      : null;
    const skillLifecycle = includeSkillLifecycleSmoke
      ? await runInstalledSkillLifecycleSmoke(cdp)
      : null;

    const checks = {
      title: title === "DS Agent",
      url: typeof url === "string" && url.startsWith("http://tauri.localhost"),
      not_blank: meaningfulBodyText(bodyText),
      tauri_bridge_present: hasTauriInternals === true,
      no_framework_overlay: !hasFrameworkOverlay(bodyText),
    };
    const failedChecks = Object.entries(checks)
      .filter(([, passed]) => !passed)
      .map(([name]) => name);

    if (failedChecks.length > 0) {
      throw new Error(`Installed UI smoke failed checks: ${failedChecks.join(", ")}`);
    }

    console.log(
      JSON.stringify(
        {
          ok: true,
          executable: describeLocalExecutable(executablePath),
          title,
          url,
          body_chars: String(bodyText).length,
          checks,
          isolated_profile: true,
          onboarding,
          screenshot: screenshotPath,
          agent_chat: agentChat ?? "skipped",
          memory_feedback: memoryFeedback ?? "skipped",
          memory_maintenance: memoryMaintenance ?? "skipped",
          office_artifact: officeArtifact ?? "skipped",
          skill_lifecycle: skillLifecycle ?? "skipped",
          workflow: workflow ?? "skipped",
        },
        null,
        2,
      ),
    );
  } catch (error) {
    console.error(
      JSON.stringify(
        {
          ok: false,
          executable: executablePath
            ? describeLocalExecutable(executablePath)
            : "[not found]",
          error: String(error?.message ?? error),
        },
        null,
        2,
      ),
    );
    process.exitCode = 1;
  } finally {
    if (cdp) {
      await cdp.send("Browser.close").catch(() => undefined);
      cdp.close();
    }
    terminateProcessTree(child);
    if (isolatedProfile) {
      try {
        await removeIsolatedProfile(isolatedProfile);
        console.log(JSON.stringify({ isolated_profile_cleanup: "verified" }));
      } catch {
        console.error(
          JSON.stringify({
            ok: false,
            error: "Isolated profile cleanup could not be verified.",
          }),
        );
        process.exitCode = 1;
      }
    }
  }
}

function resolveExecutablePath() {
  return (
    process.env.DEEPSEEK_AGENT_OS_INSTALLED_EXE?.trim() ||
    defaultInstalledExecutablePath()
  );
}

function defaultInstalledExecutablePath() {
  return path.join(
    process.env.LOCALAPPDATA ?? path.join(os.homedir(), "AppData", "Local"),
    "DS Agent",
    "ds-agent.exe",
  );
}

async function createIsolatedProfile() {
  const tempRoot = await realpath(os.tmpdir());
  const root = await mkdtemp(path.join(tempRoot, "ds-agent-ui-profile-"));
  try {
    const verifiedRoot = await verifyIsolatedProfileRoot(root, tempRoot);
    const profile = {
      root: verifiedRoot,
      tempRoot,
      appDataDir: path.join(verifiedRoot, "appdata"),
      localAppDataDir: path.join(verifiedRoot, "localappdata"),
      workspaceDir: path.join(verifiedRoot, "workspace"),
      reportDir: path.join(verifiedRoot, "reports"),
      screenshotDir: path.join(verifiedRoot, "reports", "screenshots"),
    };
    await Promise.all(
      [
        profile.appDataDir,
        profile.localAppDataDir,
        profile.workspaceDir,
        profile.reportDir,
        profile.screenshotDir,
      ].map((directory) => mkdir(directory, { recursive: true })),
    );
    return profile;
  } catch {
    const verifiedRoot = await verifyIsolatedProfileRoot(root, tempRoot).catch(() => null);
    if (verifiedRoot) {
      await rm(verifiedRoot, { recursive: true, force: true }).catch(() => undefined);
    }
    throw new Error("Isolated profile could not be initialized.");
  }
}

async function verifyIsolatedProfileRoot(root, expectedTempRoot = os.tmpdir()) {
  const [resolvedRoot, resolvedTempRoot] = await Promise.all([
    realpath(root),
    realpath(expectedTempRoot),
  ]);
  const stat = await lstat(resolvedRoot);
  if (
    !stat.isDirectory() ||
    stat.isSymbolicLink() ||
    path.dirname(resolvedRoot).toLowerCase() !== resolvedTempRoot.toLowerCase() ||
    !path.basename(resolvedRoot).startsWith("ds-agent-ui-profile-")
  ) {
    throw new Error("Isolated profile root validation failed.");
  }
  return resolvedRoot;
}

function isolatedProfileEnvironment(baseEnv, profile, overrides = {}) {
  return {
    ...baseEnv,
    ...overrides,
    APPDATA: profile.appDataDir,
    LOCALAPPDATA: profile.localAppDataDir,
    WEBVIEW2_USER_DATA_FOLDER: path.join(profile.localAppDataDir, "webview2"),
    DS_AGENT_UI_SMOKE_PROFILE_MODE: "isolated-clean",
  };
}

function ensureNoExternalWebViewProfileOverride(env) {
  const browserArgs = String(env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS ?? "");
  if (/--(?:user-data-dir|profile-directory|disk-cache-dir)(?:=|\s)/i.test(browserArgs)) {
    throw new Error("External WebView2 profile arguments are not allowed in isolated mode.");
  }
}

async function removeIsolatedProfile(profile) {
  const verifiedRoot = await verifyIsolatedProfileRoot(profile.root, profile.tempRoot);
  await rm(verifiedRoot, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  if (existsSync(verifiedRoot)) {
    throw new Error("Isolated profile cleanup failed.");
  }
}

function webView2ArgsForPort(port) {
  const existing = process.env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS?.trim();
  return [
    existing,
    `--remote-debugging-port=${port}`,
    "--remote-allow-origins=*",
  ]
    .filter(Boolean)
    .join(" ");
}

async function findFreePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.unref();
    server.on("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : null;
      server.close(() => {
        if (port) {
          resolve(port);
        } else {
          reject(new Error("Could not allocate a local debugging port."));
        }
      });
    });
  });
}

async function waitForWebViewTarget(port, timeout) {
  const deadline = Date.now() + timeout;
  let lastError = "";

  while (Date.now() < deadline) {
    for (const host of ["127.0.0.1", "localhost"]) {
      try {
        const response = await fetch(`http://${host}:${port}/json/list`);
        if (response.ok) {
          const targets = await response.json();
          const page = targets.find(
            (target) =>
              target.type === "page" &&
              typeof target.webSocketDebuggerUrl === "string" &&
              (String(target.url).startsWith("http://tauri.localhost") ||
                String(target.title).includes("DS Agent")),
          );
          if (page) {
            return page;
          }
        }
      } catch (error) {
        lastError = `${host}: ${String(error?.message ?? error)}`;
      }
    }
    await delay(250);
  }

  throw new Error(
    [
      "Timed out waiting for WebView2 remote debugging target.",
      "Close any already-running DS Agent window and retry if this persists.",
      lastError,
    ].join(" "),
  );
}

async function waitForReadyState(client, timeout) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const readyState = await evaluate(client, "document.readyState").catch(
      () => "",
    );
    if (readyState === "interactive" || readyState === "complete") {
      return;
    }
    await delay(150);
  }
  throw new Error("Timed out waiting for installed UI document readiness.");
}

async function waitForMeaningfulBodyText(client, timeout) {
  const deadline = Date.now() + timeout;
  let lastText = "";
  while (Date.now() < deadline) {
    lastText = String(
      (await evaluate(
        client,
        "document.body ? document.body.innerText.slice(0, 6000) : ''",
      ).catch(() => "")) ?? "",
    );
    if (meaningfulBodyText(lastText)) {
      return lastText;
    }
    await delay(150);
  }
  throw new Error(
    `Timed out waiting for installed UI meaningful body text; last body chars: ${lastText.length}.`,
  );
}

async function evaluate(client, expression) {
  const result = await client.send("Runtime.evaluate", {
    expression,
    returnByValue: true,
    awaitPromise: true,
  });

  if (result.exceptionDetails) {
    const detail =
      result.exceptionDetails.exception?.description ??
      result.exceptionDetails.text ??
      "unknown error";
    throw new Error(`CDP evaluation failed for: ${expression}. ${detail}`);
  }
  return result.result?.value;
}

async function invokeTauri(client, command, params = {}, timeout = workflowTimeoutMs) {
  const expression = `
    (async () => {
      const command = ${JSON.stringify(command)};
      const params = ${JSON.stringify(params)};
      const invoke =
        window.__TAURI_INTERNALS__?.invoke ??
        window.__TAURI__?.core?.invoke ??
        window.__TAURI__?.invoke;
      if (typeof invoke !== "function") {
        throw new Error("Tauri invoke bridge is not available.");
      }
      try {
        return await invoke(command, params);
      } catch (error) {
        throw new Error(String(error?.message ?? error));
      }
    })()
  `;

  return withTimeout(
    evaluate(client, expression),
    timeout,
    `Timed out waiting for Tauri command ${command}.`,
  );
}

async function runInstalledOnboardingSmoke(client, bodyText) {
  const readiness = await invokeTauri(client, "get_onboarding_readiness", {});
  const serialized = JSON.stringify(readiness);
  const expectedSource = process.env.DEEPSEEK_API_KEY?.trim()
    ? "environment"
    : "missing";
  const expectedCode = expectedSource === "environment" ? "not_checked" : "key_missing";
  if (
    readiness?.schema_version !== 1 ||
    readiness?.deepseek?.source !== expectedSource ||
    readiness?.deepseek?.code !== expectedCode ||
    readiness?.deepseek?.chat_completion_ready !== false ||
    readiness?.workspace?.code !== "workspace_missing"
  ) {
    throw new Error("Isolated onboarding projection was not a clean first-run state.");
  }
  if (
    /api_key|key_hash|fingerprint|account|currency|amount|total_balance|app_data|settings_file|vault|workspace_dir|evidence_dir|export_dir/i.test(
      serialized,
    )
  ) {
    throw new Error("Isolated onboarding projection exposed a forbidden field.");
  }
  const copySignals = onboardingCopySignals(bodyText);
  if (expectedSource === "missing" && (!copySignals.missingKey || !copySignals.contactsDeepSeek)) {
    throw new Error("Isolated onboarding UI did not show missing-Key contact disclosure.");
  }
  if (expectedSource === "environment" && !copySignals.retry) {
    throw new Error("Isolated onboarding UI did not show environment-Key retry copy.");
  }
  return {
    source: readiness.deepseek.source,
    deepseek_code: readiness.deepseek.code,
    workspace_code: readiness.workspace.code,
    secret_free_projection: true,
    copy_signals: copySignals,
  };
}

function onboardingCopySignals(value) {
  const text = String(value ?? "");
  return {
    missingKey:
      text.includes("请输入你自己的 DeepSeek API Key") ||
      text.includes("Enter your own DeepSeek API Key"),
    contactsDeepSeek:
      text.includes("联系 DeepSeek") || text.includes("contacts DeepSeek"),
    workspace:
      text.includes("请选择一个工作目录") || text.includes("Choose one workspace"),
    retry:
      text.includes("重试检查") || text.includes("Retry check"),
  };
}

async function runInstalledSkillLifecycleSmoke(client) {
  const clicked = await evaluate(
    client,
    `(() => {
      const labels = new Set(["插件", "Plugins"]);
      const target = Array.from(document.querySelectorAll("summary, button")).find((element) =>
        labels.has((element.textContent ?? "").trim()),
      );
      if (!target) return false;
      target.click();
      return true;
    })()`,
  );
  if (clicked !== true) {
    throw new Error("Installed Skills and Plugins smoke could not find the catalog button.");
  }
  const deadline = Date.now() + timeoutMs;
  let catalogText = "";
  while (Date.now() < deadline) {
    catalogText = String(
      (await evaluate(
        client,
        "document.body ? document.body.innerText.slice(0, 12000) : ''",
      ).catch(() => "")) ?? "",
    );
    if (
      ["技能与插件", "Skills and Plugins"].some((token) => catalogText.includes(token)) &&
      ["系统技能", "System Skills"].some((token) => catalogText.includes(token))
    ) {
      break;
    }
    await delay(150);
  }

  const catalogDom = await evaluate(
    client,
    `(() => ({
      hubOpen: document.querySelector("details.plugins-hub")?.open === true,
      headings: Array.from(document.querySelectorAll(".plugins-hub .skill-catalog-group h4"))
        .map((element) => (element.textContent ?? "").trim()),
      hiddenUtilityLabels: Array.from(document.querySelectorAll(".plugins-hub .ordinary-user-hidden"))
        .map((element) => (element.querySelector(":scope > summary")?.textContent ?? "").trim()),
    }))()`,
  );
  const headings = Array.isArray(catalogDom?.headings) ? catalogDom.headings : [];

  const checks = {
    catalog_heading: ["技能与插件", "Skills and Plugins"].some((token) =>
      catalogText.includes(token),
    ),
    catalog_open: catalogDom?.hubOpen === true,
    system_skills: ["系统技能", "System Skills"].some((token) => headings.includes(token)),
    installed_plugins: ["已安装插件", "Installed Plugins"].some((token) =>
      headings.includes(token),
    ),
    installed_skills: ["已安装 Skill", "已安装技能", "Installed Skills"].some((token) =>
      headings.includes(token),
    ),
    scenario_templates_hidden:
      !["场景模板", "Scenario Templates"].some((token) => catalogText.includes(token)) &&
      ["场景模板", "Scenario Templates"].some((token) =>
        catalogDom?.hiddenUtilityLabels?.includes(token),
      ),
    work_packages_hidden:
      !["任务记录与工作包", "Task Records and Work Packages"].some((token) =>
        catalogText.includes(token),
      ) &&
      ["任务记录与工作包", "Task Records and Work Packages"].some((token) =>
        catalogDom?.hiddenUtilityLabels?.includes(token),
      ),
    protected_builder: catalogText.includes("Skill/Plugin Builder"),
  };
  const failedChecks = Object.entries(checks)
    .filter(([, passed]) => !passed)
    .map(([name]) => name);
  if (failedChecks.length > 0) {
    throw new Error(
      `Installed Skills and Plugins smoke failed checks: ${failedChecks.join(", ")}`,
    );
  }

  return {
    checks,
    screenshot: await captureScreenshot(client, screenshotDir, "skill-lifecycle"),
  };
}

async function runInstalledAgentChatSmoke(client) {
  if (!process.env.DEEPSEEK_API_KEY?.trim()) {
    throw new Error("DEEPSEEK_API_KEY is required for --agent-chat smoke.");
  }

  await ensureInstalledDeepSeekReady(client);
  const telemetryBefore = await listDeepSeekTelemetry(client);
  const response = await invokeTauri(
    client,
    "run_agent_chat",
    {
      prompt: "请用一句中文回答：DS Agent 的桌面对话桥已经连到 DeepSeek 了吗？",
      largeModelProvider: "deepseek",
      modelRoute: "flash",
      thinkingLevel: "fast",
      accessMode: "ask_on_risk",
      networkSearchSourceModel: null,
    },
    workflowTimeoutMs,
  );
  const content = String(response?.content ?? "").trim();
  if (!content) {
    throw new Error("Installed agent chat smoke expected non-empty assistant content.");
  }

  const telemetryAfter = await listDeepSeekTelemetry(client);
  const newEntries = newTelemetryEntries(telemetryBefore, telemetryAfter);
  const latest = newEntries[0];
  if (!latest) {
    throw new Error("Installed agent chat smoke expected a new DeepSeek telemetry event.");
  }

  return {
    ok: true,
    content_chars: content.length,
    response_model: response?.model ?? null,
    protocol_version: response?.protocol_version ?? null,
    proposed_actions: Array.isArray(response?.proposed_actions)
      ? response.proposed_actions.length
      : null,
    missing_prerequisites: Array.isArray(response?.missing_prerequisites)
      ? response.missing_prerequisites.length
      : null,
    model_telemetry: {
      new_entries: newEntries.length,
      latest_model: latest.model ?? null,
      latest_cache_status: latest.cache_status ?? null,
      latest_total_tokens: latest.total_tokens ?? null,
    },
  };
}

async function ensureInstalledDeepSeekReady(client) {
  const readiness = await invokeTauri(client, "verify_deepseek_api_key", {});
  if (
    readiness?.deepseek?.source !== "environment" ||
    readiness?.deepseek?.chat_completion_ready !== true ||
    readiness?.deepseek?.code !== "ready"
  ) {
    throw new Error(
      `DeepSeek environment Key did not reach ready state: ${readiness?.deepseek?.code ?? "unknown"}`,
    );
  }
  return readiness;
}

async function runInstalledMemoryFeedbackSmoke(client) {
  const directoryState = await invokeTauri(client, "get_local_directory_state", {});
  const appDataEventsBackup = await backupAppDataEventsFile(
    directoryState?.settings_file,
  );
  let smokeResult = null;
  let appDataEventsRestored = false;

  try {
    const beforeRecords = await invokeTauri(client, "list_memory_records", {});
    const stamp = new Date().toISOString().replaceAll(":", "-").replaceAll(".", "-");
    const title = `Installed memory feedback smoke ${stamp}`;
    const body =
      "Temporary memory used only to verify installed selected-memory feedback bridge.";
    const candidateRecord = await invokeTauri(client, "propose_memory_candidate", {
      title,
      body,
      memoryType: "project_context",
      scope: "workspace",
      sensitivity: "normal",
      lifecycle: "active",
      expiresAt: null,
    });
    const candidateId = candidateRecord?.candidate?.id;
    if (!candidateId) {
      throw new Error("Installed memory feedback smoke did not create a candidate id.");
    }

    await invokeTauri(client, "resolve_memory_candidate", {
      candidateId,
      accepted: true,
      note: "Installed memory feedback smoke accepted temporary candidate.",
    });
    const afterAcceptRecords = await invokeTauri(client, "list_memory_records", {});
    const acceptedMemory = afterAcceptRecords.find(
      (memory) => memory?.source_id === candidateId,
    );
    if (!acceptedMemory?.id) {
      throw new Error("Installed memory feedback smoke did not find accepted memory.");
    }

    const feedback = await invokeTauri(client, "record_selected_memory_feedback", {
      memoryId: acceptedMemory.id,
      contextReceiptId: null,
      feedback: "useful",
      note: "Installed UI smoke selected-memory feedback.",
    });
    if (feedback?.memory_id !== acceptedMemory.id || feedback?.feedback !== "useful") {
      throw new Error("Installed memory feedback smoke received an unexpected feedback record.");
    }

    const afterFeedbackRecords = await invokeTauri(client, "list_memory_records", {});
    const afterFeedbackMemory = afterFeedbackRecords.find(
      (memory) => memory?.id === acceptedMemory.id,
    );
    if (!afterFeedbackMemory) {
      throw new Error("Installed memory feedback smoke lost the target memory.");
    }
    if (
      afterFeedbackMemory.title !== acceptedMemory.title ||
      afterFeedbackMemory.body !== acceptedMemory.body ||
      afterFeedbackRecords.length !== afterAcceptRecords.length
    ) {
      throw new Error("Installed memory feedback smoke mutated memory records.");
    }

    smokeResult = {
      ok: true,
      mode: "memory_feedback",
      created_candidate: Boolean(candidateId),
      target_memory_id: acceptedMemory.id,
      feedback_kind: feedback.feedback,
      memory_count_before: Array.isArray(beforeRecords) ? beforeRecords.length : null,
      memory_count_after_accept: Array.isArray(afterAcceptRecords)
        ? afterAcceptRecords.length
        : null,
      memory_count_after_feedback: Array.isArray(afterFeedbackRecords)
        ? afterFeedbackRecords.length
        : null,
      app_data_events: "pending_restore",
      app_data_events_restored: false,
    };
  } finally {
    await closeInstalledAppForAppDataRestore();
    appDataEventsRestored = await restoreAppDataEventsFile(appDataEventsBackup);
  }

  if (appDataEventsRestored !== true) {
    throw new Error(
      "Installed memory feedback smoke could not verify restored app-data event store.",
    );
  }

  return {
    ...smokeResult,
    app_data_events: "restored",
    app_data_events_restored: true,
  };
}

async function runInstalledMemoryMaintenanceSmoke(client) {
  const directoryState = await invokeTauri(client, "get_local_directory_state", {});
  const appDataEventsBackup = await backupAppDataEventsFile(
    directoryState?.settings_file,
  );
  let smokeResult = null;
  let appDataEventsRestored = false;

  try {
    const beforeRecords = await invokeTauri(client, "list_memory_records", {});
    const stamp = new Date().toISOString().replaceAll(":", "-").replaceAll(".", "-");
    const originalUpdateBody =
      "Use the old memory maintenance wording that requires users to process candidates.";
    const updateMemory = await createAcceptedMemory(client, {
      title: `Installed maintenance update smoke ${stamp}`,
      body: originalUpdateBody,
      note: "Installed maintenance smoke accepted update target.",
    });
    const archiveMemory = await createAcceptedMemory(client, {
      title: `Installed maintenance stale smoke ${stamp}`,
      body: "This stale maintenance memory should be archived by repeated stale feedback.",
      note: "Installed maintenance smoke accepted archive target.",
    });
    const updateBody =
      "Use automatic background maintenance for memory upkeep; users only review audit and correction hints.";

    await invokeTauri(client, "record_selected_memory_feedback", {
      memoryId: updateMemory.id,
      contextReceiptId: null,
      feedback: "should_update",
      note: updateBody,
    });
    for (let i = 0; i < 2; i += 1) {
      await invokeTauri(client, "record_selected_memory_feedback", {
        memoryId: archiveMemory.id,
        contextReceiptId: null,
        feedback: "stale",
        note: "This temporary installed maintenance memory is stale.",
      });
    }

    const firstSummary = await invokeTauri(
      client,
      "run_memory_background_maintenance",
      {},
    );
    const secondSummary = await invokeTauri(
      client,
      "run_memory_background_maintenance",
      {},
    );
    const afterRecords = await invokeTauri(client, "list_memory_records", {});
    const candidates = await invokeTauri(client, "list_memory_candidate_records", {});
    const reviews = await invokeTauri(client, "list_memory_maintenance_reviews", {});
    const updatedMemory = afterRecords.find((memory) => memory?.id === updateMemory.id);
    const archivedMemory = afterRecords.find((memory) => memory?.id === archiveMemory.id);
    const updateCandidate = candidates.find(
      (record) =>
        record?.candidate?.source_id === updateMemory.id &&
        record?.candidate?.suggested_action === "update",
    );
    const updateReview = reviews.find((review) => review?.memory?.id === updateMemory.id);

    if (!updatedMemory) {
      throw new Error("Installed memory maintenance smoke lost the update target.");
    }
    const updatedBody = String(updatedMemory.body ?? "").trim();
    if (!updatedBody || updatedBody === originalUpdateBody) {
      throw new Error(
        `Installed memory maintenance smoke did not change the should_update body. ${JSON.stringify({
          body_chars: updatedBody.length,
          update_candidate_status: updateCandidate?.effective_status ?? null,
          first_summary: firstSummary,
        })}`,
      );
    }
    if (archivedMemory) {
      throw new Error("Installed memory maintenance smoke did not archive repeated stale memory.");
    }
    if (firstSummary?.update_candidates_created !== 1) {
      throw new Error("Installed memory maintenance smoke expected one update candidate.");
    }
    if (firstSummary?.auto_updates_applied !== 1) {
      throw new Error("Installed memory maintenance smoke expected one automatic update.");
    }
    if (firstSummary?.auto_archives_applied !== 1) {
      throw new Error("Installed memory maintenance smoke expected one automatic archive.");
    }
    if (
      secondSummary?.update_candidates_created !== 0 ||
      secondSummary?.auto_updates_applied !== 0 ||
      secondSummary?.auto_archives_applied !== 0
    ) {
      throw new Error("Installed memory maintenance smoke expected idempotent rerun counts.");
    }
    if (updateCandidate?.effective_status !== "accepted") {
      throw new Error("Installed memory maintenance smoke expected accepted update candidate audit.");
    }
    if (updateReview?.review_needed !== false) {
      throw new Error("Installed memory maintenance smoke expected audit-only resolved update review.");
    }

    smokeResult = {
      ok: true,
      mode: "memory_maintenance",
      update_memory_id: updateMemory.id,
      archived_memory_id: archiveMemory.id,
      update_candidate_status: updateCandidate.effective_status,
      update_review_needed: updateReview?.review_needed ?? null,
      first_summary: firstSummary,
      second_summary: secondSummary,
      memory_count_before: Array.isArray(beforeRecords) ? beforeRecords.length : null,
      memory_count_after_maintenance: Array.isArray(afterRecords)
        ? afterRecords.length
        : null,
      app_data_events: "pending_restore",
      app_data_events_restored: false,
    };
  } finally {
    await closeInstalledAppForAppDataRestore();
    appDataEventsRestored = await restoreAppDataEventsFile(appDataEventsBackup);
  }

  if (appDataEventsRestored !== true) {
    throw new Error(
      "Installed memory maintenance smoke could not verify restored app-data event store.",
    );
  }

  return {
    ...smokeResult,
    app_data_events: "restored",
    app_data_events_restored: true,
  };
}

async function createAcceptedMemory(client, { title, body, note }) {
  const candidateRecord = await invokeTauri(client, "propose_memory_candidate", {
    title,
    body,
    memoryType: "project_context",
    scope: "workspace",
    sensitivity: "normal",
    lifecycle: "active",
    expiresAt: null,
  });
  const candidateId = candidateRecord?.candidate?.id;
  if (!candidateId) {
    throw new Error("Installed memory smoke did not create a candidate id.");
  }

  await invokeTauri(client, "resolve_memory_candidate", {
    candidateId,
    accepted: true,
    note,
  });
  const records = await invokeTauri(client, "list_memory_records", {});
  const acceptedMemory = records.find((memory) => memory?.source_id === candidateId);
  if (!acceptedMemory?.id) {
    throw new Error("Installed memory smoke did not find accepted memory.");
  }
  return acceptedMemory;
}

async function runInstalledWorkflowSmoke(client) {
  const startedAt = new Date();
  const runRoot = path.join(
    workflowRootDir,
    startedAt.toISOString().replaceAll(":", "-").replaceAll(".", "-"),
  );
  const workspaceDir = path.join(runRoot, "workspace");
  const evidenceDir = path.join(workspaceDir, "evidence");
  const exportDir = path.join(workspaceDir, "exports");
  await mkdir(workspaceDir, { recursive: true });
  await mkdir(evidenceDir, { recursive: true });
  await mkdir(exportDir, { recursive: true });

  const directoryState = await invokeTauri(client, "get_local_directory_state", {});
  const settingsBackup = await backupSettingsFile(directoryState?.settings_file);
  const appDataEventsBackup = await backupAppDataEventsFile(
    directoryState?.settings_file,
  );
  const approvals = [];
  let workflowResult = null;
  let restoreVerified = false;
  let appDataEventsRestored = false;

  try {
    if (expectModelTelemetry) {
      await ensureInstalledDeepSeekReady(client);
    }
    const savedDirectoryState = await invokeTauri(
      client,
      "save_local_directory_settings",
      {
        workspaceDir,
        workspaceName: "Installed Workflow Smoke",
        evidenceDir,
        exportDir,
      },
    );
    if (savedDirectoryState?.workspace?.code !== "ready") {
      throw new Error("Temporary installed workflow directories were not accepted.");
    }

    const seed = await invokeWithApproval(
      client,
      "seed_operations_briefing_evidence_templates",
      { accessMode: "full_access" },
      "file_write",
      approvals,
    );
    assertInvocationSucceeded(seed.value, "template seed");
    const evidenceFolderPath = seed.value.evidence_ref ?? evidenceDir;
    const telemetryBefore = expectModelTelemetry
      ? await listDeepSeekTelemetry(client)
      : [];

    const run = await invokeWithApproval(
      client,
      "run_operations_briefing",
      {
        accessMode: "full_access",
        evidenceFolderPath,
        largeModelProvider: "deepseek",
        modelRoute: "auto",
        thinkingLevel: "fast",
      },
      "file_read",
      approvals,
    );
    if (run.value?.status !== "draft_ready") {
      throw new Error(
        `Operations Briefing run did not reach draft_ready: ${run.value?.status ?? "unknown"}`,
      );
    }
    const modelTelemetry = await summarizeModelTelemetry(client, telemetryBefore);

    const markdown = await exportOperationsBriefingReport(
      client,
      "export_operations_briefing_report",
      run.value.id,
      approvals,
    );
    const html = await exportOperationsBriefingReport(
      client,
      "export_operations_briefing_html_report",
      run.value.id,
      approvals,
    );
    const pdf = await exportOperationsBriefingReport(
      client,
      "export_operations_briefing_pdf_report",
      run.value.id,
      approvals,
    );
    const workPackage = await invokeTauri(client, "export_work_package", {});
    const workPackagePath = path.join(
      exportDir,
      `deepseek-agent-os-work-package-${run.value.id}.json`,
    );
    await writeFile(workPackagePath, `${JSON.stringify(workPackage, null, 2)}\n`, "utf8");
    const workPackageBriefingRuns = Array.isArray(workPackage?.operations_briefing_runs)
      ? workPackage.operations_briefing_runs
      : [];
    if (!workPackageBriefingRuns.some((item) => item?.id === run.value.id)) {
      throw new Error("Exported work package did not include the installed workflow run.");
    }

    const exportedFiles = await readdir(exportDir);
    const exportedRefs = [
      markdown.value?.evidence_ref,
      html.value?.evidence_ref,
      pdf.value?.evidence_ref,
      workPackagePath,
    ].filter(Boolean);

    for (const filePath of exportedRefs) {
      if (!existsSync(filePath)) {
        throw new Error(`Expected exported report was not found: ${filePath}`);
      }
    }

    workflowResult = {
      ok: true,
      mode: "workflow",
      run_id: run.value.id,
      run_status: run.value.status,
      evidence_ref: describeLocalPath(evidenceFolderPath),
      export_dir: describeLocalPath(exportDir),
      exported_files: exportedFiles,
      work_package_file: path.basename(workPackagePath),
      work_package_run_count: workPackageBriefingRuns.length,
      approvals_resolved: approvals.length,
      model_telemetry: modelTelemetry,
      settings_file_restored: false,
      app_data_events: "pending_restore",
      app_data_events_restored: false,
    };
  } finally {
    await closeInstalledAppForAppDataRestore();
    restoreVerified = await restoreSettingsFile(settingsBackup);
    appDataEventsRestored = await restoreAppDataEventsFile(appDataEventsBackup);
  }

  assertWorkflowRestoresVerified({
    settingsRestored: restoreVerified,
    appDataEventsRestored,
  });

  return {
    ...workflowResult,
    settings_file_restored: restoreVerified,
    app_data_events: appDataEventsRestored ? "restored" : "restore_failed",
    app_data_events_restored: appDataEventsRestored,
  };
}

async function runInstalledOfficeArtifactSmoke(client) {
  const startedAt = new Date();
  const runRoot = path.join(
    workflowRootDir,
    `office-${startedAt.toISOString().replaceAll(":", "-").replaceAll(".", "-")}`,
  );
  const workspaceDir = path.join(runRoot, "workspace");
  const evidenceDir = path.join(workspaceDir, "evidence");
  const exportDir = path.join(workspaceDir, "exports");
  await mkdir(workspaceDir, { recursive: true });
  await mkdir(evidenceDir, { recursive: true });
  await mkdir(exportDir, { recursive: true });

  const directoryState = await invokeTauri(client, "get_local_directory_state", {});
  const settingsBackup = await backupSettingsFile(directoryState?.settings_file);
  const appDataEventsBackup = await backupAppDataEventsFile(directoryState?.settings_file);
  const approvals = [];
  let officeResult = null;
  let restoreVerified = false;
  let appDataEventsRestored = false;

  try {
    const savedDirectoryState = await invokeTauri(client, "save_local_directory_settings", {
      workspaceDir,
      workspaceName: "Installed Office Artifact Smoke",
      evidenceDir,
      exportDir,
    });
    if (savedDirectoryState?.workspace?.code !== "ready") {
      throw new Error("Temporary installed office artifact directories were not accepted.");
    }

    const target = "office/office-artifact-smoke.docx";
    const createAction = installedOfficeSmokeAction({
      actionType: "office_create",
      target,
      title: "DS Agent Office Artifact Smoke",
      reason: "Create a Word document to verify release Office artifact packaging.",
      content: JSON.stringify({
        app: "word",
        title: "DS Agent Office Artifact Smoke",
        body: [
          "DS Agent Office Artifact Smoke",
          "This document verifies that DS Agent can create a Word artifact that opens in Microsoft Word without repair.",
        ].join("\n"),
        target_location: "workspace",
      }),
    });
    const create = await resumeAgentActionWithApproval(client, createAction, "file_write", approvals);
    if (create.action?.execution_state !== "succeeded") {
      throw new Error(
        `office_create did not succeed: ${create.action?.execution_state ?? "unknown"} ${
          create.action?.blocked_reason ?? ""
        }`.trim(),
      );
    }

    const relativeTarget = String(create.action.target ?? target).replaceAll("\\", "/");
    const createdPath = path.join(workspaceDir, relativeTarget);
    if (!existsSync(createdPath)) {
      throw new Error(`Expected Office artifact was not found: ${createdPath}`);
    }

    const wordOpen = verifyWordCanOpenDocument(createdPath);
    officeResult = {
      ok: true,
      mode: "office_artifact",
      app: "word",
      target: relativeTarget,
      created_file: describeLocalPath(createdPath),
      create_state: create.action.execution_state,
      create_dispatch_note: create.action.dispatch_note ?? null,
      approvals_resolved: approvals.length,
      word_open_ok: wordOpen.ok === true,
      word_paragraphs: wordOpen.paragraphs ?? null,
      word_text_chars: wordOpen.text_chars ?? null,
      settings_file_restored: false,
      app_data_events: "pending_restore",
      app_data_events_restored: false,
    };
  } finally {
    await closeInstalledAppForAppDataRestore();
    restoreVerified = await restoreSettingsFile(settingsBackup);
    appDataEventsRestored = await restoreAppDataEventsFile(appDataEventsBackup);
  }

  assertWorkflowRestoresVerified({
    settingsRestored: restoreVerified,
    appDataEventsRestored,
  });

  return {
    ...officeResult,
    settings_file_restored: restoreVerified,
    app_data_events: appDataEventsRestored ? "restored" : "restore_failed",
    app_data_events_restored: appDataEventsRestored,
  };
}

function installedOfficeSmokeAction({ actionType, target, title, reason, content }) {
  return {
    action_type: actionType,
    title,
    reason,
    risk: "medium",
    requires_confirmation: true,
    target,
    target_location: "workspace",
    destination: null,
    preferred_browser: null,
    content,
    capability: null,
    policy_decision: null,
    execution_state: "proposed",
    dispatch_note: null,
    permission_request_id: null,
    capability_invocation_id: null,
    workflow_run_id: null,
    blocked_reason: null,
  };
}

async function resumeAgentActionWithApproval(client, action, capability, approvals) {
  const firstAction = await invokeTauri(client, "resume_agent_chat_action", {
    accessMode: "full_access",
    largeModelProvider: "deepseek",
    networkSearchSourceModel: null,
    action,
  });
  if (firstAction?.execution_state !== "needs_confirmation") {
    return { action: firstAction, retried: false };
  }

  const requestId = await approveNewestPendingCapability(client, capability);
  approvals.push(requestId);
  const retryAction = {
    ...firstAction,
    execution_state: "proposed",
  };
  return {
    action: await invokeTauri(client, "resume_agent_chat_action", {
      accessMode: "full_access",
      largeModelProvider: "deepseek",
      networkSearchSourceModel: null,
      action: retryAction,
    }),
    retried: true,
  };
}

function verifyWordCanOpenDocument(filePath) {
  const shell = resolvePowerShellExecutable();
  const script = `
$ErrorActionPreference = 'Stop'
$path = $env:DS_AGENT_OFFICE_SMOKE_DOCX
if (-not (Test-Path -LiteralPath $path)) {
  throw "Office smoke file does not exist: $path"
}
$before = @(Get-Process WINWORD -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
$word = $null
$doc = $null
try {
  $word = New-Object -ComObject Word.Application
  $word.Visible = $false
  $word.DisplayAlerts = 0
  $readOnly = $true
  $confirmConversions = $false
  $addToRecentFiles = $false
  $doc = $word.Documents.Open([ref] $path, [ref] $confirmConversions, [ref] $readOnly, [ref] $addToRecentFiles)
  $text = [string]$doc.Content.Text
  if ($text -notlike '*DS Agent Office Artifact Smoke*') {
    throw "Word opened the document but expected smoke text was not found."
  }
  $result = [ordered]@{
    ok = $true
    app = 'Word'
    paragraphs = [int]$doc.Paragraphs.Count
    text_chars = [int]$text.Length
  }
  $result | ConvertTo-Json -Compress
} finally {
  if ($doc -ne $null) {
    try { $doc.Close([ref] $false) | Out-Null } catch {}
  }
  if ($word -ne $null) {
    try { $word.Quit() | Out-Null } catch {}
    try { [System.Runtime.InteropServices.Marshal]::ReleaseComObject($word) | Out-Null } catch {}
  }
  [GC]::Collect()
  [GC]::WaitForPendingFinalizers()
  $after = @(Get-Process WINWORD -ErrorAction SilentlyContinue)
  foreach ($process in $after) {
    if ($before -notcontains $process.Id) {
      try { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue } catch {}
    }
  }
}
`;
  const encodedScript = Buffer.from(script, "utf16le").toString("base64");
  const result = spawnSync(shell, ["-NoProfile", "-ExecutionPolicy", "Bypass", "-EncodedCommand", encodedScript], {
    encoding: "utf8",
    env: {
      ...process.env,
      DS_AGENT_OFFICE_SMOKE_DOCX: filePath,
    },
    timeout: 45_000,
    windowsHide: true,
  });

  if (result.error) {
    throw new Error(`Word open verification failed: ${result.error.message}`);
  }
  if (result.status !== 0) {
    throw new Error(
      `Word open verification failed: ${(result.stderr || result.stdout).trim()}`,
    );
  }
  const stdout = result.stdout.trim();
  try {
    return JSON.parse(stdout);
  } catch (error) {
    throw new Error(`Word open verification returned invalid JSON: ${stdout || error.message}`);
  }
}

function resolvePowerShellExecutable() {
  const candidates = [
    process.env.PWSH_EXE,
    "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
    "pwsh.exe",
    "powershell.exe",
  ].filter(Boolean);
  for (const candidate of candidates) {
    const probe = spawnSync(candidate, ["-NoProfile", "-Command", "$PSVersionTable.PSVersion.Major"], {
      encoding: "utf8",
      windowsHide: true,
    });
    if (probe.status === 0) {
      return candidate;
    }
  }
  throw new Error("PowerShell is required for Word COM Office artifact verification.");
}

async function invokeWithApproval(client, command, params, capability, approvals) {
  const firstValue = await invokeTauri(client, command, params);
  if (firstValue?.status !== "pending_approval") {
    return { value: firstValue, retried: false };
  }

  const requestId = await approveNewestPendingCapability(client, capability);
  approvals.push(requestId);
  const retryValue = await invokeTauri(client, command, params);
  return { value: retryValue, retried: true };
}

async function approveNewestPendingCapability(client, capability) {
  const records = await invokeTauri(
    client,
    "list_pending_capability_access_records",
    {},
  );
  const matches = records
    .filter((record) => record?.request?.capability === capability)
    .sort((left, right) =>
      String(right.request.created_at).localeCompare(String(left.request.created_at)),
    );
  const requestId = matches[0]?.request?.id;

  if (!requestId) {
    throw new Error(`No pending ${capability} approval was found.`);
  }

  await invokeTauri(client, "resolve_capability_access_request", {
    requestId,
    approved: true,
    note: "Installed workflow smoke approval",
  });
  return requestId;
}

async function exportOperationsBriefingReport(client, command, runId, approvals) {
  const result = await invokeWithApproval(
    client,
    command,
    {
      accessMode: "full_access",
      runId,
    },
    "drive_write",
    approvals,
  );
  assertInvocationSucceeded(result.value, command);
  return result;
}

async function listDeepSeekTelemetry(client) {
  const telemetry = await invokeTauri(client, "list_deepseek_chat_telemetry", {});
  return Array.isArray(telemetry) ? telemetry : [];
}

async function summarizeModelTelemetry(client, telemetryBefore) {
  if (!expectModelTelemetry) {
    return {
      expected: false,
      observed: false,
      reason: "DEEPSEEK_API_KEY not configured",
    };
  }

  const beforeIds = new Set(telemetryBefore.map((entry) => entry?.id));
  const telemetryAfter = await listDeepSeekTelemetry(client);
  const newEntries = newTelemetryEntries(telemetryBefore, telemetryAfter);
  const latest = newEntries[0];

  if (!latest) {
    throw new Error(
      "Installed workflow smoke expected a DeepSeek telemetry event, but none was recorded.",
    );
  }

  return {
    expected: true,
    observed: true,
    new_entries: newEntries.length,
    latest_model: latest.model ?? null,
    latest_cache_status: latest.cache_status ?? null,
    latest_total_tokens: latest.total_tokens ?? null,
  };
}

function newTelemetryEntries(telemetryBefore, telemetryAfter) {
  const beforeIds = new Set((telemetryBefore ?? []).map((entry) => entry?.id));
  return (telemetryAfter ?? []).filter((entry) => !beforeIds.has(entry?.id));
}

function assertInvocationSucceeded(invocation, label) {
  if (invocation?.status !== "succeeded") {
    throw new Error(
      `${label} invocation did not succeed: ${invocation?.status ?? "unknown"}`,
    );
  }
}

async function closeInstalledAppForAppDataRestore() {
  if (cdp) {
    await cdp.send("Browser.close").catch(() => undefined);
    cdp.close();
    cdp = null;
  }
  terminateProcessTree(child);
  child = undefined;
  await delay(500);
}

async function backupSettingsFile(settingsFile) {
  return backupLocalFile(settingsFile);
}

async function restoreSettingsFile(backup) {
  return restoreLocalFile(backup);
}

async function backupAppDataEventsFile(settingsFile) {
  if (!settingsFile) {
    return null;
  }

  return backupLocalFile(path.join(path.dirname(settingsFile), "kernel-events.sqlite3"));
}

async function restoreAppDataEventsFile(backup) {
  return restoreLocalFile(backup);
}

async function backupLocalFile(filePath) {
  if (!filePath) {
    return null;
  }

  const verifiedFilePath = await verifyIsolatedLocalFilePath(filePath);

  try {
    return {
      filePath: verifiedFilePath,
      existed: true,
      content: await readFile(verifiedFilePath),
    };
  } catch (error) {
    if (error?.code === "ENOENT") {
      return {
        filePath,
        existed: false,
        content: null,
      };
    }
    throw error;
  }
}

async function restoreLocalFile(backup) {
  if (!backup?.filePath) {
    return true;
  }

  const verifiedFilePath = await verifyIsolatedLocalFilePath(backup.filePath);

  if (backup.existed) {
    await mkdir(path.dirname(verifiedFilePath), { recursive: true });
    await writeFile(verifiedFilePath, backup.content);
    const restoredContent = await readFile(verifiedFilePath);
    return restoredContent.equals(backup.content);
  }

  await rm(verifiedFilePath, { force: true });
  return !existsSync(verifiedFilePath);
}

async function verifyIsolatedLocalFilePath(filePath) {
  if (!isolatedProfile?.root || !isolatedProfile?.tempRoot) {
    throw new Error("Local smoke file access requires an active isolated profile.");
  }
  const verifiedRoot = await verifyIsolatedProfileRoot(
    isolatedProfile.root,
    isolatedProfile.tempRoot,
  );
  const resolvedPath = path.resolve(filePath);
  const parent = await realpath(path.dirname(resolvedPath));
  if (!pathIsInsideRoot(resolvedPath, verifiedRoot) || !pathIsInsideRoot(parent, verifiedRoot)) {
    throw new Error("Local smoke file path escaped the isolated profile.");
  }
  try {
    const metadata = await lstat(resolvedPath);
    if (metadata.isSymbolicLink() || !metadata.isFile()) {
      throw new Error("Local smoke file path is unsafe.");
    }
    const canonicalFile = await realpath(resolvedPath);
    if (!pathIsInsideRoot(canonicalFile, verifiedRoot)) {
      throw new Error("Local smoke file path escaped the isolated profile.");
    }
  } catch (error) {
    if (error?.code !== "ENOENT") {
      throw error;
    }
  }
  return resolvedPath;
}

function pathIsInsideRoot(candidate, root) {
  const relative = path.relative(root, candidate);
  return (
    relative === "" ||
    (!relative.startsWith(`..${path.sep}`) && relative !== ".." && !path.isAbsolute(relative))
  );
}

function assertWorkflowRestoresVerified({
  settingsRestored,
  appDataEventsRestored,
}) {
  const failedRestores = [];
  if (settingsRestored !== true) {
    failedRestores.push("settings file");
  }
  if (appDataEventsRestored !== true) {
    failedRestores.push("app-data event store");
  }

  if (failedRestores.length > 0) {
    throw new Error(
      `Installed workflow smoke could not verify restored ${failedRestores.join(
        " and ",
      )}.`,
    );
  }
}

async function captureScreenshot(client, directory, label = "ui") {
  await mkdir(directory, { recursive: true });
  const response = await client.send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
  });
  const filePath = path.join(
    directory,
    `ds-agent-installed-${label}-${new Date()
      .toISOString()
      .replaceAll(":", "-")
      .replaceAll(".", "-")}.png`,
  );
  await writeFile(filePath, Buffer.from(response.data, "base64"));
  return filePath;
}

function meaningfulBodyText(value) {
  const text = String(value ?? "");
  if (text.length > 200) {
    return ["DS Agent", "工作台", "Workbench", "Operations", "记忆", "审批"].some(
      (token) => text.includes(token),
    );
  }

  const compactChatFirstTokens = [
    "新对话",
    "运行步骤",
    "理解任务",
    "调用 DeepSeek",
    "生成与导出",
    "New chat",
    "Run steps",
    "Understand task",
    "Call DeepSeek",
    "Generate and export",
  ];
  const matchedCompactTokens = compactChatFirstTokens.filter((token) =>
    text.includes(token),
  ).length;
  return text.length > 80 && matchedCompactTokens >= 3;
}

function hasFrameworkOverlay(value) {
  const text = String(value ?? "");
  return [
    "Vite",
    "React Refresh",
    "Unhandled Runtime Error",
    "Build Error",
  ].some((token) => text.includes(token));
}

function describeLocalExecutable(value) {
  return path.isAbsolute(value)
    ? `[local executable]/${path.basename(value)}`
    : value.replace(/\\/g, "/");
}

function describeLocalPath(value) {
  const normalized = path.resolve(String(value));
  const tempRoot = path.resolve(os.tmpdir());
  if (normalized.toLowerCase().startsWith(tempRoot.toLowerCase())) {
    return `[temp]/${path.relative(tempRoot, normalized).replace(/\\/g, "/")}`;
  }

  return path.isAbsolute(value)
    ? `[local path]/${path.basename(value)}`
    : String(value).replace(/\\/g, "/");
}

function readPositiveInteger(value, name) {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed <= 0) {
    throw new Error(`${name} must be a positive integer.`);
  }
  return parsed;
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function withTimeout(promise, timeout, message) {
  let timeoutId;
  const timeoutPromise = new Promise((_, reject) => {
    timeoutId = setTimeout(() => reject(new Error(message)), timeout);
  });

  return Promise.race([promise, timeoutPromise]).finally(() => {
    clearTimeout(timeoutId);
  });
}

function terminateProcessTree(process) {
  if (!process || process.exitCode !== null || !process.pid) {
    return;
  }

  if (isWindows) {
    spawnSync("taskkill", ["/PID", String(process.pid), "/T", "/F"], {
      stdio: "ignore",
    });
    return;
  }

  process.kill();
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

class CdpClient {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 1;
    this.pending = new Map();

    socket.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (!message.id || !this.pending.has(message.id)) {
        return;
      }
      const { resolve, reject } = this.pending.get(message.id);
      this.pending.delete(message.id);
      if (message.error) {
        reject(new Error(message.error.message));
      } else {
        resolve(message.result ?? {});
      }
    });
  }

  static connect(url) {
    return new Promise((resolve, reject) => {
      const socket = new WebSocket(url);
      socket.addEventListener("open", () => resolve(new CdpClient(socket)), {
        once: true,
      });
      socket.addEventListener(
        "error",
        () => reject(new Error("Could not connect to WebView2 CDP target.")),
        { once: true },
      );
    });
  }

  send(method, params = {}) {
    const id = this.nextId++;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
    });
  }

  close() {
    this.socket.close();
  }
}

async function runSelfTest() {
  if (uiSmokeRemoteDebuggingPortEnv !== "DS_AGENT_UI_SMOKE_REMOTE_DEBUGGING_PORT") {
    throw new Error("Self-test expected the installed app smoke port contract.");
  }
  if (!allowedArgs.has("--memory-feedback")) {
    throw new Error("Self-test expected --memory-feedback to be a supported installed UI smoke flag.");
  }
  if (!allowedArgs.has("--memory-maintenance")) {
    throw new Error("Self-test expected --memory-maintenance to be a supported installed UI smoke flag.");
  }
  if (!allowedArgs.has("--office")) {
    throw new Error("Self-test expected --office to be a supported installed UI smoke flag.");
  }
  if (!allowedArgs.has("--isolated-profile")) {
    throw new Error("Self-test expected --isolated-profile to be supported.");
  }
  if (!allowedArgs.has("--skill-lifecycle")) {
    throw new Error("Self-test expected --skill-lifecycle to be a supported installed UI smoke flag.");
  }
  validateInstalledUiSmokeFlagCombination(["--memory-feedback", "--agent-chat"]);
  validateInstalledUiSmokeFlagCombination(["--office"]);
  assertSelfTestThrows(
    () => validateInstalledUiSmokeFlagCombination(["--memory-feedback", "--workflow"]),
    "cannot be combined",
  );
  assertSelfTestThrows(
    () => validateInstalledUiSmokeFlagCombination(["--office", "--workflow"]),
    "cannot be combined",
  );
  assertSelfTestThrows(
    () => validateInstalledUiSmokeFlagCombination(["--office", "--memory-feedback"]),
    "cannot be combined",
  );
  assertSelfTestThrows(
    () => validateInstalledUiSmokeFlagCombination(["--memory-maintenance", "--workflow"]),
    "cannot be combined",
  );
  assertSelfTestThrows(
    () => validateInstalledUiSmokeFlagCombination(["--memory-maintenance", "--memory-feedback"]),
    "cannot be combined",
  );
  if (meaningfulBodyText("中\nEN\n设置")) {
    throw new Error("Self-test expected short settings-only text to stay blank.");
  }
  if (!meaningfulBodyText("DS Agent 工作台 Operations 记忆 审批 ".repeat(16))) {
    throw new Error("Self-test expected legacy workbench text to stay meaningful.");
  }
  const missingKeySignals = onboardingCopySignals(
    "请输入你自己的 DeepSeek API Key。保存后会联系 DeepSeek，核验认证。",
  );
  if (!missingKeySignals.missingKey || !missingKeySignals.contactsDeepSeek) {
    throw new Error("Self-test expected deterministic missing-Key onboarding copy.");
  }
  const repairSignals = onboardingCopySignals("请选择一个工作目录。重试检查");
  if (!repairSignals.workspace || !repairSignals.retry) {
    throw new Error("Self-test expected deterministic workspace and retry copy.");
  }

  const isolatedProfileTest = await createIsolatedProfile();
  isolatedProfile = isolatedProfileTest;
  const isolatedEnv = isolatedProfileEnvironment(
    { APPDATA: "real-appdata", LOCALAPPDATA: "real-localappdata" },
    isolatedProfileTest,
  );
  if (
    isolatedEnv.APPDATA !== isolatedProfileTest.appDataDir ||
    isolatedEnv.LOCALAPPDATA !== isolatedProfileTest.localAppDataDir ||
    isolatedEnv.DS_AGENT_UI_SMOKE_PROFILE_MODE !== "isolated-clean"
  ) {
    throw new Error("Self-test expected isolated APPDATA and LOCALAPPDATA overrides.");
  }
  assertSelfTestThrows(
    () =>
      ensureNoExternalWebViewProfileOverride({
        WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--user-data-dir=C:\\real-profile",
      }),
    "not allowed",
  );
  ensureNoExternalWebViewProfileOverride({
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--disable-features=Example",
  });
  const unsafeRoot = path.join(os.tmpdir(), "deepseek-agent-os-ui-smoke-self-test");
  await mkdir(unsafeRoot, { recursive: true });
  await assertAsyncSelfTestThrows(
    () => verifyIsolatedProfileRoot(unsafeRoot, os.tmpdir()),
    "validation failed",
  );
  await assertAsyncSelfTestThrows(
    () => backupLocalFile(path.join(unsafeRoot, "outside.bin")),
    "escaped the isolated profile",
  );
  await removeSelfTestDirectory(unsafeRoot, "deepseek-agent-os-ui-smoke-self-test");
  await removeIsolatedProfile(isolatedProfileTest);
  isolatedProfile = undefined;
  if (existsSync(isolatedProfileTest.root)) {
    throw new Error("Self-test expected isolated profile cleanup.");
  }

  const attempts = [
    "",
    "中\nEN\n新对话\n对话\n1\n未命名对话\n7月5日 11:25\n设置\n发送\n运行步骤\n6\n1\n理解任务\n已完成\n2\n读取证据\n已完成\n3\n选择记忆\n已完成\n4\n调用 DeepSeek\n已完成\n5\n校验结果\n已完成\n6\n生成与导出\n已完成",
  ];
  const fakeClient = {
    async send(method) {
      if (method !== "Runtime.evaluate") {
        throw new Error(`Unexpected method in self-test: ${method}`);
      }
      return {
        result: {
          value: attempts.shift() ?? "",
        },
      };
    },
  };
  const bodyText = await waitForMeaningfulBodyText(fakeClient, 1_000);
  if (!meaningfulBodyText(bodyText)) {
    throw new Error("Self-test expected meaningful body text.");
  }
  const restoreTestDir = await mkdtemp(
    path.join(await realpath(os.tmpdir()), "ds-agent-ui-profile-self-test-"),
  );
  const restoreTestProfile = {
    root: restoreTestDir,
    tempRoot: await realpath(os.tmpdir()),
  };
  isolatedProfile = restoreTestProfile;
  const missingSettingsFile = path.join(restoreTestDir, "missing-local-directories.json");
  await rm(missingSettingsFile, { force: true });
  const missingBackup = await backupSettingsFile(missingSettingsFile);
  await writeFile(missingSettingsFile, '{"workspace_dir":"temporary"}', "utf8");
  if ((await restoreSettingsFile(missingBackup)) !== true) {
    throw new Error("Self-test expected missing settings restore to report true.");
  }
  if (existsSync(missingSettingsFile)) {
    throw new Error("Self-test expected missing settings file to be removed.");
  }

  const existingSettingsFile = path.join(restoreTestDir, "existing-local-directories.json");
  await writeFile(existingSettingsFile, '{"workspace_dir":"original"}', "utf8");
  const existingBackup = await backupSettingsFile(existingSettingsFile);
  await writeFile(existingSettingsFile, '{"workspace_dir":"temporary"}', "utf8");
  if ((await restoreSettingsFile(existingBackup)) !== true) {
    throw new Error("Self-test expected existing settings restore to report true.");
  }
  const restoredContent = await readFile(existingSettingsFile, "utf8");
  if (restoredContent !== '{"workspace_dir":"original"}') {
    throw new Error("Self-test expected existing settings content to be restored.");
  }
  const missingEventsFile = path.join(restoreTestDir, "missing-kernel-events.sqlite3");
  await rm(missingEventsFile, { force: true });
  const missingEventsBackup = await backupLocalFile(missingEventsFile);
  await writeFile(missingEventsFile, Buffer.from([1, 2, 3]));
  if ((await restoreLocalFile(missingEventsBackup)) !== true) {
    throw new Error("Self-test expected missing event store restore to report true.");
  }
  if (existsSync(missingEventsFile)) {
    throw new Error("Self-test expected missing event store file to be removed.");
  }

  const existingEventsFile = path.join(restoreTestDir, "existing-kernel-events.sqlite3");
  const originalEventsContent = Buffer.from([4, 5, 6, 7]);
  await writeFile(existingEventsFile, originalEventsContent);
  const existingEventsBackup = await backupLocalFile(existingEventsFile);
  await writeFile(existingEventsFile, Buffer.from([8, 9]));
  if ((await restoreLocalFile(existingEventsBackup)) !== true) {
    throw new Error("Self-test expected existing event store restore to report true.");
  }
  const restoredEventsContent = await readFile(existingEventsFile);
  if (!restoredEventsContent.equals(originalEventsContent)) {
    throw new Error("Self-test expected existing event store content to be restored.");
  }
  await removeIsolatedProfile(restoreTestProfile);
  isolatedProfile = undefined;
  assertWorkflowRestoresVerified({
    settingsRestored: true,
    appDataEventsRestored: true,
  });
  assertSelfTestThrows(
    () =>
      assertWorkflowRestoresVerified({
        settingsRestored: false,
        appDataEventsRestored: true,
      }),
    "settings file",
  );
  assertSelfTestThrows(
    () =>
      assertWorkflowRestoresVerified({
        settingsRestored: true,
        appDataEventsRestored: false,
      }),
    "app-data event store",
  );
  console.log("windows-installed-ui-smoke self-test ok");
}

async function removeSelfTestDirectory(root, expectedName) {
  const [resolvedRoot, resolvedTempRoot] = await Promise.all([
    realpath(root),
    realpath(os.tmpdir()),
  ]);
  const stat = await lstat(resolvedRoot);
  if (
    !stat.isDirectory() ||
    stat.isSymbolicLink() ||
    path.dirname(resolvedRoot).toLowerCase() !== resolvedTempRoot.toLowerCase() ||
    path.basename(resolvedRoot) !== expectedName
  ) {
    throw new Error("Self-test cleanup root validation failed.");
  }
  await rm(resolvedRoot, { recursive: true, force: true });
  if (existsSync(resolvedRoot)) {
    throw new Error("Self-test cleanup failed.");
  }
}

function assertSelfTestThrows(action, expectedMessage) {
  try {
    action();
  } catch (error) {
    if (String(error?.message ?? error).includes(expectedMessage)) {
      return;
    }
    throw error;
  }

  throw new Error(`Self-test expected error containing: ${expectedMessage}`);
}

async function assertAsyncSelfTestThrows(action, expectedMessage) {
  try {
    await action();
  } catch (error) {
    if (String(error?.message ?? error).includes(expectedMessage)) {
      return;
    }
    throw error;
  }

  throw new Error(`Self-test expected error containing: ${expectedMessage}`);
}

function validateInstalledUiSmokeFlagCombination(values) {
  const valueSet = new Set(values);
  if (valueSet.has("--office") && valueSet.has("--workflow")) {
    throw new Error("--office cannot be combined with --workflow.");
  }
  if (valueSet.has("--office") && valueSet.has("--memory-feedback")) {
    throw new Error("--office cannot be combined with --memory-feedback.");
  }
  if (valueSet.has("--office") && valueSet.has("--memory-maintenance")) {
    throw new Error("--office cannot be combined with --memory-maintenance.");
  }
  if (valueSet.has("--memory-feedback") && valueSet.has("--workflow")) {
    throw new Error("--memory-feedback cannot be combined with --workflow.");
  }
  if (valueSet.has("--memory-maintenance") && valueSet.has("--workflow")) {
    throw new Error("--memory-maintenance cannot be combined with --workflow.");
  }
  if (valueSet.has("--memory-maintenance") && valueSet.has("--memory-feedback")) {
    throw new Error("--memory-maintenance cannot be combined with --memory-feedback.");
  }
}

if (args.has("--self-test")) {
  await runSelfTest();
} else {
  await main();
}
