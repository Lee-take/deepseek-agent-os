#!/usr/bin/env node

import { readdir, readFile } from "node:fs/promises";
import path from "node:path";

const apiKey = (process.env.DEEPSEEK_API_KEY ?? "").trim();
const baseUrl = normalizeBaseUrl(
  process.env.DEEPSEEK_API_BASE_URL ?? "https://api.deepseek.com",
);
const model = (
  process.env.DEEPSEEK_BRIEFING_SMOKE_MODEL ??
  process.env.DEEPSEEK_SMOKE_MODEL ??
  "deepseek-chat"
).trim();
const evidenceDir =
  process.env.DEEPSEEK_BRIEFING_EVIDENCE_DIR ??
  path.join("docs", "templates", "operations-briefing-evidence");
const evidenceLabel = describeEvidenceDir(evidenceDir);
const maxTokensInput = process.env.DEEPSEEK_BRIEFING_SMOKE_MAX_TOKENS ?? "900";
const showContent = process.env.DEEPSEEK_SMOKE_SHOW_CONTENT === "1";

if (!apiKey) {
  console.error("DEEPSEEK_API_KEY is required in the local environment.");
  process.exit(1);
}

if (!model) {
  console.error("DEEPSEEK_BRIEFING_SMOKE_MODEL cannot be blank.");
  process.exit(1);
}

const endpoint = `${baseUrl}/chat/completions`;

try {
  const manifest = await buildEvidenceManifest(evidenceDir);
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
          `Evidence reference: ${evidenceLabel}`,
          "",
          "Evidence manifest excerpt:",
          manifest,
          "",
          'Return JSON with this shape: {"summary":"...","anomalies":[{"area":"...","signal":"...","evidence_ref":"..."}],"action_plan":[{"owner":"...","action":"...","due_hint":"..."}],"warnings":[]}',
        ].join("\n"),
      },
    ],
    max_tokens: readPositiveInteger(
      maxTokensInput,
      "DEEPSEEK_BRIEFING_SMOKE_MAX_TOKENS",
    ),
    temperature: 0,
  };

  const startedAt = Date.now();
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${apiKey}`,
      "Content-Type": "application/json",
      "User-Agent": "DeepSeek-Agent-OS/0.1 operations-briefing-smoke-test",
    },
    body: JSON.stringify(payload),
  });
  const elapsedMs = Date.now() - startedAt;
  const responseText = await response.text();

  if (!response.ok) {
    console.error(
      JSON.stringify(
        {
          ok: false,
          endpoint,
          status: response.status,
          message: redact(responseText, apiKey, evidenceDir).slice(0, 500),
        },
        null,
        2,
      ),
    );
    process.exit(1);
  }

  const body = JSON.parse(responseText);
  const content = body.choices?.[0]?.message?.content ?? "";
  const parsed = parseSynthesis(content);
  assertSynthesis(parsed);

  const result = {
    ok: true,
    endpoint,
    evidence_dir: evidenceLabel,
    evidence_manifest_chars: manifest.length,
    requested_model: model,
    returned_model: body.model ?? null,
    finish_reason: body.choices?.[0]?.finish_reason ?? null,
    prompt_tokens: body.usage?.prompt_tokens ?? null,
    completion_tokens: body.usage?.completion_tokens ?? null,
    total_tokens: body.usage?.total_tokens ?? null,
    elapsed_ms: elapsedMs,
    summary_chars: parsed.summary.length,
    anomalies_count: parsed.anomalies.length,
    action_plan_count: parsed.action_plan.length,
    warnings_count: parsed.warnings.length,
  };

  if (showContent) {
    result.synthesis = parsed;
  }

  console.log(JSON.stringify(result, null, 2));
} catch (error) {
  console.error(
    JSON.stringify(
      {
        ok: false,
        endpoint,
        evidence_dir: evidenceLabel,
        error: redact(error?.message ?? String(error), apiKey, evidenceDir),
      },
      null,
      2,
    ),
  );
  process.exit(1);
}

async function buildEvidenceManifest(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = entries
    .filter((entry) => entry.isFile() && /\.(md|txt|csv|json|log|ya?ml)$/i.test(entry.name))
    .map((entry) => entry.name)
    .sort();

  if (files.length === 0) {
    throw new Error(`No supported evidence files found in ${directory}.`);
  }

  const sections = [];
  for (const file of files) {
    const filePath = path.join(directory, file);
    const content = await readFile(filePath, "utf8");
    sections.push(`--- ${file} ---\n${content.trim()}`);
  }

  return sections.join("\n\n").slice(0, 12_000);
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

function describeEvidenceDir(value) {
  const normalized = value.trim();
  if (path.isAbsolute(normalized)) {
    return "[local evidence directory]";
  }
  return normalized.replace(/\\/g, "/");
}

function redact(value, secret, localPath) {
  let redacted = value;
  if (secret) {
    redacted = redacted.split(secret).join("[REDACTED]");
  }
  if (localPath && path.isAbsolute(localPath)) {
    redacted = redacted.split(localPath).join("[local evidence directory]");
  }
  return redacted;
}
