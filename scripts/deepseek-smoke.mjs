#!/usr/bin/env node

const apiKey = (process.env.DEEPSEEK_API_KEY ?? "").trim();
const baseUrl = normalizeBaseUrl(
  process.env.DEEPSEEK_API_BASE_URL ?? "https://api.deepseek.com",
);
const model = (process.env.DEEPSEEK_SMOKE_MODEL ?? "deepseek-chat").trim();
const showContent = process.env.DEEPSEEK_SMOKE_SHOW_CONTENT === "1";

if (!apiKey) {
  console.error("DEEPSEEK_API_KEY is required in the local environment.");
  process.exit(1);
}

if (!model) {
  console.error("DEEPSEEK_SMOKE_MODEL cannot be blank.");
  process.exit(1);
}

const endpoint = `${baseUrl}/chat/completions`;
const payload = {
  model,
  messages: [
    {
      role: "user",
      content: "Return the single word: ok",
    },
  ],
  max_tokens: 4,
  temperature: 0,
};

try {
  const startedAt = Date.now();
  const response = await fetch(endpoint, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${apiKey}`,
      "Content-Type": "application/json",
      "User-Agent": "DeepSeek-Agent-OS/0.1 local-smoke-test",
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
          message: redact(responseText, apiKey).slice(0, 500),
        },
        null,
        2,
      ),
    );
    process.exit(1);
  }

  const body = JSON.parse(responseText);
  const firstChoice = body.choices?.[0];
  const content = firstChoice?.message?.content ?? "";
  const result = {
    ok: true,
    endpoint,
    requested_model: model,
    returned_model: body.model ?? null,
    finish_reason: firstChoice?.finish_reason ?? null,
    prompt_tokens: body.usage?.prompt_tokens ?? null,
    completion_tokens: body.usage?.completion_tokens ?? null,
    total_tokens: body.usage?.total_tokens ?? null,
    elapsed_ms: elapsedMs,
    content_present: Boolean(content),
  };

  if (showContent) {
    result.content = content;
  }

  console.log(JSON.stringify(result, null, 2));
} catch (error) {
  console.error(
    JSON.stringify(
      {
        ok: false,
        endpoint,
        error: redact(error?.message ?? String(error), apiKey),
      },
      null,
      2,
    ),
  );
  process.exit(1);
}

function normalizeBaseUrl(value) {
  const normalized = value.trim().replace(/\/+$/, "");
  if (!normalized) {
    throw new Error("DEEPSEEK_API_BASE_URL cannot be blank.");
  }
  return normalized;
}

function redact(value, secret) {
  if (!secret) {
    return value;
  }
  return value.split(secret).join("[REDACTED]");
}
