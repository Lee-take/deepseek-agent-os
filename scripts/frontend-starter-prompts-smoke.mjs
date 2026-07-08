#!/usr/bin/env node

import { spawn, spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdir, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const args = process.argv.slice(2).filter((arg) => arg !== "--");
const allowedArgs = new Set(["--help", "--self-test"]);

const appHost = process.env.DS_AGENT_FRONTEND_SMOKE_HOST ?? "127.0.1.0";
const appPort = Number(process.env.DS_AGENT_FRONTEND_SMOKE_PORT ?? "1420");
const appUrl = process.env.DS_AGENT_FRONTEND_SMOKE_URL ?? `http://${appHost}:${appPort}/`;
const timeoutMs = Number(process.env.DS_AGENT_FRONTEND_SMOKE_TIMEOUT_MS ?? "45000");
const screenshotDir =
  process.env.DS_AGENT_FRONTEND_SMOKE_SCREENSHOT_DIR ??
  path.join(os.tmpdir(), "ds-agent-ui-smoke");

const localizedPromptSets = [
  {
    locale: "zh",
    prompts: [
      "根据我的证据文件夹，生成一份经营简报。",
      "把这段会议纪要整理成行动项、责任部门、截止时间和风险提示。",
      "继续上次的项目，先说明你用了哪些记忆，再给我下一步建议。",
    ],
  },
  {
    locale: "en",
    prompts: [
      "Create a management briefing from my evidence folder.",
      "Turn these meeting notes into actions, owners, deadlines, and risks.",
      "Continue the previous project, first explain which memories you used.",
    ],
  },
];

validateArgs(args);

if (args.includes("--help")) {
  console.log("Usage: pnpm test:frontend-starter-prompts [-- --self-test]");
  process.exit(0);
}

if (args.includes("--self-test")) {
  runSelfTest();
  process.exit(0);
}

if (typeof fetch !== "function" || typeof WebSocket !== "function") {
  throw new Error("Node.js with global fetch and WebSocket is required for this smoke test.");
}

let devServer;
let browser;
let browserUserDataDir;
let cdp;

async function main() {
  try {
    const startedDevServer = !(await httpOk(appUrl));
    let devServerLog = () => "";

    if (startedDevServer) {
      const started = startViteDevServer();
      devServer = started.child;
      devServerLog = started.getLog;
      await waitForCondition(
        async () => {
          if (devServer.exitCode !== null) {
            throw new Error(`Vite dev server exited early with code ${devServer.exitCode}.`);
          }
          return httpOk(appUrl);
        },
        timeoutMs,
        () => `Vite dev server did not become ready at ${appUrl}.\n${devServerLog()}`,
      );
    }

    const browserPath = resolveBrowserPath();
    const debugPort = await findFreePort();
    browserUserDataDir = path.join(
      os.tmpdir(),
      `ds-agent-starter-prompts-${process.pid}-${Date.now()}`,
    );
    await mkdir(browserUserDataDir, { recursive: true });

    browser = spawnBrowser(browserPath, debugPort, browserUserDataDir, appUrl);
    const target = await waitForBrowserTarget(debugPort, appUrl, timeoutMs);
    cdp = await CdpClient.connect(target.webSocketDebuggerUrl);
    await cdp.send("Runtime.enable");
    await cdp.send("Page.enable");
    await cdp.send("Page.bringToFront").catch(() => undefined);
    await cdp.send("Emulation.setDeviceMetricsOverride", {
      width: 1440,
      height: 1000,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await cdp.send("Page.navigate", { url: appUrl });
    await waitForReadyState(cdp, timeoutMs);

    const beforeClick = await waitForCondition(
      async () => {
        const state = await readStarterPromptState(cdp);
        if (state.framework_overlay) {
          throw new Error(`Framework overlay is visible: ${state.body_text_sample}`);
        }
        return state.composer_present && state.starter_prompt_count >= 3 ? state : false;
      },
      timeoutMs,
      "Starter prompts did not render on the first meaningful DS Agent screen.",
    );

    const matchedPromptSet = findMatchingPromptSet(beforeClick.starter_prompts);
    if (!matchedPromptSet) {
      throw new Error(
        `Starter prompts did not match expected office-work prompts: ${JSON.stringify(
          beforeClick.starter_prompts,
        )}`,
      );
    }

    const clicked = await evaluate(
    cdp,
    `(() => {
        const firstPrompt = document.querySelectorAll(".chat-composer .starter-prompt")[0];
        if (!firstPrompt) {
          return false;
        }
        firstPrompt.click();
        return true;
      })()`,
    );
    if (!clicked) {
      throw new Error("Could not click the first starter prompt.");
    }

    const afterClick = await waitForCondition(
      async () => {
        const state = await readStarterPromptState(cdp);
        return state.textarea_value === beforeClick.starter_prompts[0] ? state : false;
      },
      Math.min(timeoutMs, 10000),
      "Clicking the first starter prompt did not populate the chat composer.",
    );

    const screenshot = await captureScreenshot(cdp, screenshotDir);
    const result = {
      ok: true,
      url: beforeClick.url,
      title: beforeClick.title,
      browser: describeLocalExecutable(browserPath),
      dev_server: startedDevServer ? "started by smoke test" : "reused existing server",
      locale: matchedPromptSet.locale,
      starter_prompt_count: beforeClick.starter_prompt_count,
      starter_prompts: beforeClick.starter_prompts,
      textarea_after_click: afterClick.textarea_value,
      setup_modal_visible: beforeClick.setup_modal_visible,
      screenshot,
    };
    console.log(JSON.stringify(result, null, 2));
  } finally {
    if (cdp) {
      await cdp.send("Browser.close").catch(() => undefined);
      cdp.close();
    } else if (browser) {
      terminateProcessTree(browser);
    }

    if (devServer) {
      terminateProcessTree(devServer);
    }

    if (browserUserDataDir) {
      await delay(300);
      await rm(browserUserDataDir, { recursive: true, force: true }).catch(() => undefined);
    }
  }
}

function startViteDevServer() {
  const npx = resolveNpxInvocation();
  const child = spawn(
    npx.command,
    [
      ...npx.args,
      "pnpm@9.15.9",
      "--filter",
      "@deepseek-agent-os/desktop",
      "dev",
      "--",
      "--host",
      appHost,
    ],
    {
      cwd: repoRoot,
      env: { ...process.env, BROWSER: "none" },
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    },
  );

  const getLog = collectProcessOutput(child);
  return { child, getLog };
}

function resolveNpxInvocation() {
  const npxCliPath = path.join(
    path.dirname(process.execPath),
    "node_modules",
    "npm",
    "bin",
    "npx-cli.js",
  );
  if (existsSync(npxCliPath)) {
    return { command: process.execPath, args: [npxCliPath] };
  }

  if (process.platform === "win32") {
    throw new Error(`Could not locate npm npx-cli.js at ${npxCliPath}.`);
  }

  return { command: "npx", args: [] };
}

function spawnBrowser(browserPath, debugPort, userDataDir, url) {
  return spawn(
    browserPath,
    [
      `--remote-debugging-port=${debugPort}`,
      `--user-data-dir=${userDataDir}`,
      "--no-first-run",
      "--no-default-browser-check",
      "--disable-background-networking",
      "--disable-extensions",
      "--disable-sync",
      "--new-window",
      url,
    ],
    {
      cwd: repoRoot,
      stdio: ["ignore", "ignore", "ignore"],
      windowsHide: true,
    },
  );
}

async function waitForBrowserTarget(port, expectedUrl, timeout) {
  let lastTargets = [];
  return waitForCondition(
    async () => {
      lastTargets = await fetchJson(`http://127.0.0.1:${port}/json/list`, 1500).catch(
        () => [],
      );
      const pages = lastTargets.filter((target) => {
        return target.type === "page" && target.webSocketDebuggerUrl;
      });
      return (
        pages.find((target) => String(target.url ?? "").startsWith(expectedUrl)) ??
        pages.find((target) => !String(target.url ?? "").startsWith("devtools://")) ??
        false
      );
    },
    timeout,
    () =>
      `Could not find a browser CDP page target for ${expectedUrl}. Last targets: ${JSON.stringify(
        lastTargets,
      ).slice(0, 1000)}`,
  );
}

async function waitForReadyState(client, timeout) {
  await waitForCondition(
    async () => {
      const readyState = await evaluate(client, "document.readyState");
      return readyState === "interactive" || readyState === "complete";
    },
    timeout,
    "Browser page did not reach an interactive ready state.",
  );
}

async function readStarterPromptState(client) {
  return evaluate(
    client,
    `(() => {
      const bodyText = document.body?.innerText ?? "";
      const prompts = Array.from(document.querySelectorAll(".chat-composer .starter-prompt")).map(
        (button) => button.innerText.trim()
      );
      const textarea = document.querySelector(".chat-composer textarea");
      return {
        title: document.title,
        url: window.location.href,
        body_text_length: bodyText.length,
        body_text_sample: bodyText.slice(0, 800),
        framework_overlay:
          Boolean(document.querySelector("vite-error-overlay")) ||
          /Internal server error|Unhandled Runtime Error|Build Error|React Refresh/i.test(bodyText),
        composer_present: Boolean(textarea),
        starter_prompt_count: prompts.length,
        starter_prompts: prompts,
        textarea_value: textarea?.value ?? "",
        setup_modal_visible: Boolean(document.querySelector(".setup-modal-backdrop")),
      };
    })()`,
  );
}

function findMatchingPromptSet(actualPrompts) {
  return localizedPromptSets.find((promptSet) => {
    return promptSet.prompts.every((expected, index) => actualPrompts[index] === expected);
  });
}

async function captureScreenshot(client, directory) {
  await mkdir(directory, { recursive: true });
  const response = await client.send("Page.captureScreenshot", {
    format: "png",
    captureBeyondViewport: false,
  });
  const filePath = path.join(
    directory,
    `ds-agent-starter-prompts-${new Date()
      .toISOString()
      .replaceAll(":", "-")
      .replaceAll(".", "-")}.png`,
  );
  await writeFile(filePath, Buffer.from(response.data, "base64"));
  return filePath;
}

async function evaluate(client, expression) {
  const response = await client.send("Runtime.evaluate", {
    expression,
    awaitPromise: true,
    returnByValue: true,
  });
  if (response.exceptionDetails) {
    throw new Error(
      response.exceptionDetails.exception?.description ??
        response.exceptionDetails.text ??
        "Runtime.evaluate failed.",
    );
  }
  return response.result?.value;
}

async function httpOk(url) {
  try {
    const response = await fetchWithTimeout(url, 1500);
    return response.ok;
  } catch {
    return false;
  }
}

async function fetchJson(url, timeout) {
  const response = await fetchWithTimeout(url, timeout);
  if (!response.ok) {
    throw new Error(`${url} returned HTTP ${response.status}`);
  }
  return response.json();
}

async function fetchWithTimeout(url, timeout) {
  const controller = new AbortController();
  const timeoutId = setTimeout(() => controller.abort(), timeout);
  try {
    return await fetch(url, { signal: controller.signal });
  } finally {
    clearTimeout(timeoutId);
  }
}

async function waitForCondition(check, timeout, message) {
  const start = Date.now();
  let lastError;
  while (Date.now() - start < timeout) {
    try {
      const value = await check();
      if (value) {
        return value;
      }
    } catch (error) {
      lastError = error;
      if (/exited early|Framework overlay/.test(String(error.message))) {
        throw error;
      }
    }
    await delay(200);
  }

  const renderedMessage = typeof message === "function" ? message() : message;
  const suffix = lastError ? ` Last error: ${lastError.message}` : "";
  throw new Error(`${renderedMessage}${suffix}`);
}

function resolveBrowserPath() {
  for (const candidate of browserCandidates()) {
    if (candidate && existsSync(candidate)) {
      return candidate;
    }
  }
  throw new Error(
    `Could not find Microsoft Edge or Google Chrome. Checked: ${browserCandidates().join(", ")}`,
  );
}

function browserCandidates() {
  return [
    process.env.DS_AGENT_BROWSER_PATH,
    "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    path.join(process.env.LOCALAPPDATA ?? "", "Microsoft\\Edge\\Application\\msedge.exe"),
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
  ].filter(Boolean);
}

async function findFreePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
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

function collectProcessOutput(child) {
  const chunks = [];
  const maxLength = 8000;
  const push = (data) => {
    chunks.push(String(data));
    while (chunks.join("").length > maxLength) {
      chunks.shift();
    }
  };
  child.stdout?.on("data", push);
  child.stderr?.on("data", push);
  return () => chunks.join("");
}

function terminateProcessTree(child) {
  if (!child || child.exitCode !== null || !child.pid) {
    return;
  }

  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(child.pid), "/t", "/f"], { stdio: "ignore" });
    return;
  }

  child.kill("SIGTERM");
}

function describeLocalExecutable(value) {
  return path.isAbsolute(value) ? `[local executable]/${path.basename(value)}` : value;
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function validateArgs(values) {
  const unknown = values.filter((arg) => !allowedArgs.has(arg));
  if (unknown.length > 0) {
    throw new Error(`Unknown frontend starter prompts smoke argument(s): ${unknown.join(", ")}`);
  }
}

function runSelfTest() {
  const failures = [];
  assert(
    browserCandidates().some((candidate) => candidate.endsWith("msedge.exe")),
    "browser candidates include Microsoft Edge",
    failures,
  );
  assert(
    browserCandidates().some((candidate) => candidate.endsWith("chrome.exe")),
    "browser candidates include Google Chrome",
    failures,
  );
  assert(
    findMatchingPromptSet(localizedPromptSets[0].prompts)?.locale === "zh",
    "Chinese starter prompts match expected set",
    failures,
  );
  assert(
    findMatchingPromptSet(localizedPromptSets[1].prompts)?.locale === "en",
    "English starter prompts match expected set",
    failures,
  );
  assert(
    !findMatchingPromptSet(["Create a management briefing from my evidence folder."]),
    "partial starter prompt sets are rejected",
    failures,
  );
  const npx = resolveNpxInvocation();
  assert(Boolean(npx.command) && Array.isArray(npx.args), "npx invocation resolves", failures);

  if (failures.length > 0) {
    console.error(JSON.stringify({ ok: false, failures }, null, 2));
    process.exit(1);
  }

  console.log(
    JSON.stringify(
      {
        ok: true,
        self_tests: 6,
        app_url: appUrl,
        starter_prompt_sets: localizedPromptSets.map((promptSet) => promptSet.locale),
      },
      null,
      2,
    ),
  );
}

function assert(condition, message, failures) {
  if (!condition) {
    failures.push(message);
  }
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

    socket.addEventListener("close", () => {
      for (const { reject } of this.pending.values()) {
        reject(new Error("CDP socket closed."));
      }
      this.pending.clear();
    });
  }

  static connect(url) {
    return new Promise((resolve, reject) => {
      const socket = new WebSocket(url);
      socket.addEventListener("open", () => resolve(new CdpClient(socket)), { once: true });
      socket.addEventListener("error", () => reject(new Error("Could not connect to CDP target.")), {
        once: true,
      });
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

await main();
