#!/usr/bin/env node

import { randomUUID } from "node:crypto";
import { copyFile, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const rawArgs = process.argv.slice(2).filter((arg) => arg !== "--");
const allowedArgs = new Set(["--help", "--self-test"]);
validateArgs(rawArgs, allowedArgs, "test:windows-local");

if (rawArgs.includes("--help")) {
  console.log(
    [
      "Usage: pnpm test:windows-local [-- <flags>]",
      "",
      "Flags:",
      "  --self-test Run deterministic helper checks without calling DeepSeek.",
    ].join("\n"),
  );
  process.exit(0);
}

if (rawArgs.includes("--self-test")) {
  await runSelfTest();
  process.exit(0);
}

const apiKey = (process.env.DEEPSEEK_API_KEY ?? "").trim();
const baseUrl = normalizeBaseUrl(
  process.env.DEEPSEEK_API_BASE_URL ?? "https://api.deepseek.com",
);
const model = (
  process.env.DEEPSEEK_WINDOWS_SMOKE_MODEL ??
  process.env.DEEPSEEK_BRIEFING_SMOKE_MODEL ??
  process.env.DEEPSEEK_SMOKE_MODEL ??
  "deepseek-v4-flash"
).trim();
const appDataDir =
  process.env.DEEPSEEK_AGENT_OS_APP_DATA_DIR ??
  path.join(process.env.APPDATA ?? path.join(os.homedir(), "AppData", "Roaming"), "ai.deepseek-agent-os.desktop");
const hasExplicitLocalDirectoriesFile = Boolean(process.env.DEEPSEEK_AGENT_OS_LOCAL_DIRECTORIES?.trim());
const localDirectoriesFile =
  process.env.DEEPSEEK_AGENT_OS_LOCAL_DIRECTORIES ??
  path.join(appDataDir, "local-directories.json");
const fallbackSmokeRoot =
  process.env.DEEPSEEK_WINDOWS_SMOKE_ROOT ??
  path.join(os.tmpdir(), "deepseek-agent-os-windows-local-smoke");
const templateDir =
  process.env.DEEPSEEK_BRIEFING_TEMPLATE_DIR ??
  path.join("docs", "templates", "operations-briefing-evidence");
const endpoint = `${baseUrl}/chat/completions`;
const maxTokensInput = process.env.DEEPSEEK_WINDOWS_SMOKE_MAX_TOKENS ?? "900";

if (!apiKey) {
  console.error("DEEPSEEK_API_KEY is required in the local environment.");
  process.exit(1);
}

if (!model) {
  console.error("DEEPSEEK_WINDOWS_SMOKE_MODEL cannot be blank.");
  process.exit(1);
}

try {
  const localDirectoryConfig = await readLocalDirectories(localDirectoriesFile);
  const localDirectories = localDirectoryConfig.settings;
  await validateLocalDirectories(localDirectories);
  const templateSeed = await seedEvidenceTemplates(templateDir, localDirectories.evidence_dir);
  const manifest = await buildEvidenceManifest(localDirectories.evidence_dir);
  const synthesis = await synthesizeOperationsBriefing(manifest);
  const run = {
    id: randomUUID(),
    workflow_id: "operations.briefing.v1",
    status: "draft_ready",
    title: "Operations Briefing Draft",
    created_at: new Date().toISOString(),
    evidence_folder_path: localDirectories.evidence_dir,
    ...synthesis,
  };
  const outputs = await writeRunOutputs(run, localDirectories.export_dir);

  console.log(
    JSON.stringify(
      {
        ok: true,
        settings_source: localDirectoryConfig.source,
        settings_file:
          localDirectoryConfig.source === "file"
            ? describeLocalPath(localDirectoriesFile)
            : "[generated smoke directories]",
        evidence_dir: describeLocalPath(localDirectories.evidence_dir),
        export_dir: describeLocalPath(localDirectories.export_dir),
        template_seed: templateSeed,
        evidence_manifest_chars: manifest.length,
        requested_model: model,
        run_id: run.id,
        summary_chars: run.summary.length,
        anomalies_count: run.anomalies.length,
        action_plan_count: run.action_plan.length,
        warnings_count: run.warnings.length,
        outputs: outputs.map((output) => path.basename(output)),
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
        endpoint,
        error: redact(String(error?.message ?? error)),
      },
      null,
      2,
    ),
  );
  process.exit(1);
}

async function readLocalDirectories(filePath) {
  let parsed;
  try {
    parsed = JSON.parse(await readFile(filePath, "utf8"));
  } catch (error) {
    if (error?.code === "ENOENT" && !hasExplicitLocalDirectoriesFile) {
      return {
        source: "generated_fallback",
        settings: await buildFallbackLocalDirectories(),
      };
    }
    throw error;
  }

  for (const field of ["workspace_dir", "evidence_dir", "export_dir"]) {
    if (typeof parsed[field] !== "string" || !parsed[field].trim()) {
      throw new Error(`${field} is missing from local directory settings.`);
    }
  }
  return { source: "file", settings: parsed };
}

async function buildFallbackLocalDirectories() {
  const settings = {
    workspace_dir: path.join(fallbackSmokeRoot, "workspace"),
    evidence_dir: path.join(fallbackSmokeRoot, "evidence"),
    export_dir: path.join(fallbackSmokeRoot, "exports"),
  };

  await Promise.all(Object.values(settings).map((directory) => mkdir(directory, { recursive: true })));
  return settings;
}

async function validateLocalDirectories(localDirectories) {
  for (const [label, dir] of Object.entries(localDirectories)) {
    await mkdir(dir, { recursive: true });
    const info = await stat(dir);
    if (!info.isDirectory()) {
      throw new Error(`${label} is not a directory.`);
    }
  }
}

async function seedEvidenceTemplates(sourceDir, targetDir) {
  const templateNames = [
    "revenue.md",
    "guest-experience.md",
    "risk-and-compliance.md",
    "action-followups.md",
  ];
  await mkdir(targetDir, { recursive: true });
  const written = [];
  const skipped = [];

  for (const name of templateNames) {
    const sourcePath = path.join(sourceDir, name);
    const targetPath = path.join(targetDir, name);
    try {
      await stat(targetPath);
      skipped.push(name);
      continue;
    } catch (error) {
      if (error?.code !== "ENOENT") {
        throw error;
      }
    }
    await copyFile(sourcePath, targetPath);
    written.push(name);
  }

  return { written, skipped };
}

async function buildEvidenceManifest(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = entries
    .filter((entry) => entry.isFile() && /\.(md|txt|csv|json|log|ya?ml)$/i.test(entry.name))
    .map((entry) => entry.name)
    .sort();

  if (files.length === 0) {
    throw new Error(`No supported evidence files found in ${describeLocalPath(directory)}.`);
  }

  const sections = [];
  for (const file of files) {
    const filePath = path.join(directory, file);
    const content = await readFile(filePath, "utf8");
    sections.push(`--- ${file} ---\n${content.trim()}`);
  }

  return sections.join("\n\n").slice(0, 12_000);
}

async function synthesizeOperationsBriefing(manifest) {
  const payload = {
    model,
    messages: [
      {
        role: "system",
        content:
          "You are an operations briefing analyst. Return strict JSON only. The JSON object must contain summary, anomalies, action_plan, and warnings. Do not invent evidence beyond the provided manifest.",
      },
      {
        role: "user",
        content: [
          "Evidence reference: [local evidence directory]",
          "",
          "Evidence manifest excerpt:",
          manifest,
          "",
          'Return JSON with this shape: {"summary":"...","anomalies":[{"area":"...","signal":"...","evidence_ref":"..."}],"action_plan":[{"owner":"...","action":"...","due_hint":"..."}],"warnings":[]}',
        ].join("\n"),
      },
    ],
    max_tokens: readPositiveInteger(maxTokensInput, "DEEPSEEK_WINDOWS_SMOKE_MAX_TOKENS"),
    temperature: 0,
  };

  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${apiKey}`,
      "Content-Type": "application/json",
      "User-Agent": "DeepSeek-Agent-OS/0.1.0 windows-local-smoke-test",
    },
    body: JSON.stringify(payload),
  });
  const responseText = await response.text();

  if (!response.ok) {
    throw new Error(`DeepSeek request failed ${response.status}: ${redact(responseText).slice(0, 500)}`);
  }

  const body = JSON.parse(responseText);
  const content = body.choices?.[0]?.message?.content ?? "";
  const parsed = parseSynthesis(content);
  assertSynthesis(parsed);
  return parsed;
}

async function writeRunOutputs(run, exportDir) {
  await mkdir(exportDir, { recursive: true });
  const baseName = `operations-briefing-${run.id}`;
  const outputs = [
    path.join(exportDir, `${baseName}.json`),
    path.join(exportDir, `${baseName}.md`),
    path.join(exportDir, `${baseName}.html`),
  ];
  await writeFile(outputs[0], `${JSON.stringify(run, null, 2)}\n`, "utf8");
  await writeFile(outputs[1], renderMarkdown(run), "utf8");
  await writeFile(outputs[2], renderHtml(run), "utf8");
  return outputs;
}

function renderMarkdown(run) {
  return [
    `# ${run.title}`,
    "",
    `- Run ID: ${run.id}`,
    `- Workflow: ${run.workflow_id}`,
    `- Status: ${run.status}`,
    `- Created: ${run.created_at}`,
    "- Evidence: [local evidence directory]",
    "",
    "## Summary",
    "",
    run.summary,
    "",
    "## Anomalies",
    "",
    ...listOrEmpty(run.anomalies, (item) => `- **${item.area}**: ${item.signal}`),
    "",
    "## Action Plan",
    "",
    ...listOrEmpty(run.action_plan, (item) => `- **${item.owner}**: ${item.action} _(due: ${item.due_hint})_`),
    "",
    "## Warnings",
    "",
    ...listOrEmpty(run.warnings, (item) => `- ${item}`),
    "",
  ].join("\n");
}

function renderHtml(run) {
  return [
    "<!doctype html>",
    '<html lang="en">',
    "<head>",
    '<meta charset="utf-8">',
    '<meta name="viewport" content="width=device-width, initial-scale=1">',
    `<title>${escapeHtml(run.title)}</title>`,
    "<style>body{font-family:Arial,sans-serif;line-height:1.5;margin:40px;max-width:960px;color:#172033;background:#fff}h1{font-size:28px;margin-bottom:8px}h2{font-size:18px;margin-top:28px;border-bottom:1px solid #d8dee8;padding-bottom:6px}.meta{color:#526071;font-size:13px}li{margin:8px 0}.warning{color:#7a4a00}</style>",
    "</head>",
    "<body>",
    `<h1>${escapeHtml(run.title)}</h1>`,
    '<section class="meta">',
    `<p>Run ID: ${escapeHtml(run.id)}</p>`,
    `<p>Workflow: ${escapeHtml(run.workflow_id)}</p>`,
    `<p>Status: ${escapeHtml(run.status)}</p>`,
    `<p>Created: ${escapeHtml(run.created_at)}</p>`,
    "<p>Evidence: [local evidence directory]</p>",
    "</section>",
    "<h2>Summary</h2>",
    `<p>${escapeHtml(run.summary)}</p>`,
    "<h2>Anomalies</h2>",
    renderHtmlList(run.anomalies, (item) => `<strong>${escapeHtml(item.area)}</strong>: ${escapeHtml(item.signal)}`),
    "<h2>Action Plan</h2>",
    renderHtmlList(run.action_plan, (item) => `<strong>${escapeHtml(item.owner)}</strong>: ${escapeHtml(item.action)} <em>due: ${escapeHtml(item.due_hint)}</em>`),
    "<h2>Warnings</h2>",
    renderHtmlList(run.warnings, (item) => `<span class="warning">${escapeHtml(item)}</span>`),
    "</body>",
    "</html>",
  ].join("\n");
}

function listOrEmpty(values, render) {
  return values.length ? values.map(render) : ["No items recorded."];
}

function renderHtmlList(values, render) {
  if (!values.length) {
    return "<p>No items recorded.</p>";
  }
  return `<ul>${values.map((value) => `<li>${render(value)}</li>`).join("")}</ul>`;
}

function parseSynthesis(value) {
  const trimmed = value.trim();
  const start = trimmed.indexOf("{");
  const end = trimmed.lastIndexOf("}");
  const jsonText = start >= 0 && end > start ? trimmed.slice(start, end + 1) : trimmed;
  return JSON.parse(jsonText);
}

function assertSynthesis(value) {
  if (!value || typeof value !== "object") {
    throw new Error("Operations Briefing synthesis must be a JSON object.");
  }
  if (typeof value.summary !== "string" || value.summary.trim().length === 0) {
    throw new Error("Operations Briefing synthesis requires a non-empty summary.");
  }
  if (!Array.isArray(value.anomalies)) {
    throw new Error("Operations Briefing synthesis requires anomalies array.");
  }
  if (!Array.isArray(value.action_plan)) {
    throw new Error("Operations Briefing synthesis requires action_plan array.");
  }
  if (!Array.isArray(value.warnings)) {
    throw new Error("Operations Briefing synthesis requires warnings array.");
  }
}

function normalizeBaseUrl(value) {
  const normalized = value.trim().replace(/\/+$/, "");
  if (!normalized) {
    throw new Error("DEEPSEEK_API_BASE_URL cannot be blank.");
  }
  return normalized;
}

function readPositiveInteger(value, name) {
  const parsed = Number(value);
  if (!Number.isInteger(parsed) || parsed <= 0) {
    throw new Error(`${name} must be a positive integer.`);
  }
  return parsed;
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function describeLocalPath(value) {
  return path.isAbsolute(value) ? "[local path]" : value.replace(/\\/g, "/");
}

function redact(value) {
  return value
    .split(apiKey).join("[REDACTED]")
    .split(localDirectoriesFile).join("[local settings file]");
}

async function runSelfTest() {
  const testRoot = path.join(os.tmpdir(), "deepseek-agent-os-windows-local-smoke-self-test");
  const sourceDir = path.join(testRoot, "templates");
  const evidenceDir = path.join(testRoot, "evidence");
  await rm(testRoot, { recursive: true, force: true });
  await mkdir(sourceDir, { recursive: true });
  await mkdir(evidenceDir, { recursive: true });
  await Promise.all(
    [
      ["revenue.md", "# Revenue\nRooms revenue test evidence."],
      ["guest-experience.md", "# Guest Experience\nGuest score test evidence."],
      ["risk-and-compliance.md", "# Risk\nNo open compliance issue."],
      ["action-followups.md", "# Follow-ups\nOwner action list."],
    ].map(([name, content]) => writeFile(path.join(sourceDir, name), `${content}\n`, "utf8")),
  );

  const seed = await seedEvidenceTemplates(sourceDir, evidenceDir);
  assertEqual(seed.written.length, 4, "self-test expected four seeded templates");
  const secondSeed = await seedEvidenceTemplates(sourceDir, evidenceDir);
  assertEqual(secondSeed.skipped.length, 4, "self-test expected existing templates to be skipped");

  const settingsDirs = {
    workspace_dir: path.join(testRoot, "settings", "workspace"),
    evidence_dir: path.join(testRoot, "settings", "workspace", "evidence"),
    export_dir: path.join(testRoot, "settings", "workspace", "exports"),
  };
  await validateLocalDirectories(settingsDirs);
  for (const dir of Object.values(settingsDirs)) {
    const info = await stat(dir);
    if (!info.isDirectory()) {
      throw new Error("Self-test expected missing settings directories to be created.");
    }
  }

  const manifest = await buildEvidenceManifest(evidenceDir);
  if (!manifest.includes("--- revenue.md ---") || !manifest.includes("Rooms revenue test evidence.")) {
    throw new Error("Self-test expected evidence manifest to include seeded revenue evidence.");
  }

  const synthesis = parseSynthesis(
    'prefix {"summary":"Local summary","anomalies":[],"action_plan":[],"warnings":["sample"]} suffix',
  );
  assertSynthesis(synthesis);
  assertSelfTestThrows(
    () => assertSynthesis({ summary: "", anomalies: [], action_plan: [], warnings: [] }),
    "non-empty summary",
  );
  assertSelfTestThrows(
    () => readPositiveInteger("0", "TEST_POSITIVE_INTEGER"),
    "positive integer",
  );
  assertEqual(normalizeBaseUrl(" https://api.deepseek.com/// "), "https://api.deepseek.com", "self-test expected base URL normalization");

  const markdown = renderMarkdown({
    id: "00000000-0000-4000-8000-000000000000",
    title: "Operations Briefing Draft",
    workflow_id: "operations.briefing.v1",
    status: "draft_ready",
    created_at: "2026-07-01T00:00:00.000Z",
    summary: "Local summary",
    anomalies: [],
    action_plan: [],
    warnings: [],
  });
  if (!markdown.includes("# Operations Briefing Draft") || !markdown.includes("No items recorded.")) {
    throw new Error("Self-test expected Markdown renderer to include title and empty-state rows.");
  }

  const html = renderHtml({
    id: "00000000-0000-4000-8000-000000000000",
    title: "Operations <Briefing>",
    workflow_id: "operations.briefing.v1",
    status: "draft_ready",
    created_at: "2026-07-01T00:00:00.000Z",
    summary: "Local <summary>",
    anomalies: [],
    action_plan: [],
    warnings: [],
  });
  if (!html.includes("Operations &lt;Briefing&gt;") || !html.includes("Local &lt;summary&gt;")) {
    throw new Error("Self-test expected HTML renderer to escape report text.");
  }

  console.log("windows-local-smoke self-test ok");
}

function assertEqual(actual, expected, message) {
  if (actual !== expected) {
    throw new Error(`${message}: expected ${expected}, got ${actual}`);
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
