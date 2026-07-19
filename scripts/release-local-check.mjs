#!/usr/bin/env node

import { spawnSync } from "node:child_process";

const rawArgs = process.argv.slice(2).filter((arg) => arg !== "--");
const allowedArgs = new Set([
  "--help",
  "--include-installed-ui",
  "--include-installed-workflow",
  "--require-live-deepseek",
  "--self-test",
  "--skip-live-deepseek",
]);
validateArgs(rawArgs, allowedArgs, "test:release-local");

if (rawArgs.includes("--help")) {
  console.log(
    [
      "Usage: pnpm test:release-local [-- <flags>]",
      "",
      "Flags:",
      "  --self-test                 Run deterministic release-local helper checks.",
      "  --skip-live-deepseek        Skip live DeepSeek and Windows local smoke checks.",
      "  --require-live-deepseek     Fail if DEEPSEEK_API_KEY is not configured.",
      "  --include-installed-ui      Include installed DS Agent UI smoke.",
      "  --include-installed-workflow Include installed DS Agent workflow smoke.",
    ].join("\n"),
  );
  process.exit(0);
}

if (rawArgs.includes("--self-test")) {
  runSelfTest();
  process.exit(0);
}

const args = new Set(rawArgs);
const skipLiveDeepSeek = args.has("--skip-live-deepseek");
const requireLiveDeepSeek = args.has("--require-live-deepseek");
try {
  validateFlagCombination({ skipLiveDeepSeek, requireLiveDeepSeek });
} catch (error) {
  console.error(String(error?.message ?? error));
  process.exit(1);
}
const includeInstalledUi =
  args.has("--include-installed-ui") ||
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_UI_SMOKE === "1";
const includeInstalledWorkflow =
  args.has("--include-installed-workflow") ||
  process.env.DEEPSEEK_AGENT_OS_INSTALLED_UI_WORKFLOW_SMOKE === "1";
const hasDeepSeekKey = Boolean(process.env.DEEPSEEK_API_KEY?.trim());

if (requireLiveDeepSeek && !hasDeepSeekKey) {
  console.error(
    "DEEPSEEK_API_KEY is required when --require-live-deepseek is used.",
  );
  process.exit(1);
}

const releaseLocalPlan = buildReleaseLocalPlan({
  skipLiveDeepSeek,
  requireLiveDeepSeek,
  hasDeepSeekKey,
  includeInstalledUi,
  includeInstalledWorkflow,
  env: process.env,
});

if (!releaseLocalPlan.shouldRunLiveDeepSeek) {
  console.log(liveDeepSeekSkipMessage({ skipLiveDeepSeek, hasDeepSeekKey }));
}

const results = [];
for (const command of releaseLocalPlan.commands) {
  console.log(`\n== ${command.name} ==`);
  run(command.parts, command.env);
  results.push(command.name);
}

console.log(
  JSON.stringify(
      {
        ok: true,
        live_deepseek_checks: releaseLocalPlan.shouldRunLiveDeepSeek
          ? "ran"
          : "skipped",
        installed_ui_check:
          includeInstalledUi || includeInstalledWorkflow ? "ran" : "skipped",
        installed_workflow_check: includeInstalledWorkflow ? "ran" : "skipped",
        checks: results,
      },
    null,
    2,
  ),
);

function buildReleaseLocalPlan({
  skipLiveDeepSeek,
  requireLiveDeepSeek,
  hasDeepSeekKey,
  includeInstalledUi,
  includeInstalledWorkflow,
  env,
}) {
  const commands = buildBaseCommands();
  const shouldRunLiveDeepSeek =
    !skipLiveDeepSeek && (hasDeepSeekKey || requireLiveDeepSeek);

  if (shouldRunLiveDeepSeek) {
    commands.push(...buildLiveDeepSeekCommands());
  }

  if (includeInstalledWorkflow) {
    commands.push(
      buildInstalledUiCommand({
        includeInstalledWorkflow: true,
        skipLiveDeepSeek,
        env,
      }),
      buildInstalledMemoryMaintenanceCommand({
        skipLiveDeepSeek,
        env,
      }),
    );
  } else if (includeInstalledUi) {
    commands.push(
      buildInstalledUiCommand({
        includeInstalledWorkflow: false,
        skipLiveDeepSeek,
        env,
      }),
    );
  }

  return {
    commands,
    shouldRunLiveDeepSeek,
  };
}

function buildLiveDeepSeekCommands() {
  return [
    {
      name: "Windows local Operations Briefing smoke",
      parts: ["npx", "pnpm@9.15.9", "test:windows-local"],
    },
    {
      name: "DeepSeek Chat Completions smoke",
      parts: ["npx", "pnpm@9.15.9", "test:deepseek"],
    },
    {
      name: "DeepSeek Operations Briefing smoke",
      parts: ["npx", "pnpm@9.15.9", "test:deepseek:briefing"],
    },
  ];
}

function buildInstalledUiCommand({
  includeInstalledWorkflow,
  skipLiveDeepSeek,
  env,
}) {
  const parts = [
    "npx",
    "pnpm@9.15.9",
    "test:windows-installed-ui",
    "--",
    "--isolated-profile",
  ];
  const command = {
    name: includeInstalledWorkflow
      ? "Windows installed UI workflow smoke"
      : "Windows installed UI smoke",
    parts,
  };

  if (includeInstalledWorkflow) {
    parts.push("--workflow");
  }

  if (includeInstalledWorkflow && skipLiveDeepSeek) {
    command.env = { ...env };
    delete command.env.DEEPSEEK_API_KEY;
  }

  return command;
}

function buildInstalledMemoryMaintenanceCommand({ skipLiveDeepSeek, env }) {
  const command = {
    name: "Windows installed UI memory maintenance smoke",
    parts: [
      "npx",
      "pnpm@9.15.9",
      "test:windows-installed-ui",
      "--",
      "--isolated-profile",
      "--memory-maintenance",
    ],
  };

  if (skipLiveDeepSeek) {
    command.env = { ...env };
    delete command.env.DEEPSEEK_API_KEY;
  }

  return command;
}

function buildBaseCommands() {
  return [
    {
      name: "full project test",
      parts: ["npx", "pnpm@9.15.9", "test"],
    },
    {
      name: "diff whitespace check",
      parts: ["git", "diff", "--check"],
    },
    {
      name: "staged diff whitespace check",
      parts: ["git", "diff", "--cached", "--check"],
    },
    {
      name: "source-only release guard",
      parts: ["npx", "pnpm@9.15.9", "test:release-source"],
    },
    {
      name: "Windows local helper self-test",
      parts: ["node", "scripts/windows-local-smoke.mjs", "--self-test"],
    },
    {
      name: "installed UI helper self-test",
      parts: ["node", "scripts/windows-installed-ui-smoke.mjs", "--self-test"],
    },
    {
      name: "release-local helper self-test",
      parts: ["node", "scripts/release-local-check.mjs", "--self-test"],
    },
  ];
}

function liveDeepSeekSkipMessage({ skipLiveDeepSeek, hasDeepSeekKey }) {
  if (skipLiveDeepSeek) {
    return "Skipping live DeepSeek checks because --skip-live-deepseek was provided.";
  }

  if (!hasDeepSeekKey) {
    return "Skipping live DeepSeek checks because DEEPSEEK_API_KEY is not configured.";
  }

  return "Skipping live DeepSeek checks.";
}

function validateFlagCombination({ skipLiveDeepSeek, requireLiveDeepSeek }) {
  if (skipLiveDeepSeek && requireLiveDeepSeek) {
    throw new Error(
      "--skip-live-deepseek and --require-live-deepseek cannot be combined.",
    );
  }
}

function run(parts, env = process.env) {
  const result = spawnSync(parts.map(quoteShellPart).join(" "), {
    env,
    shell: true,
    stdio: "inherit",
  });

  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function quoteShellPart(value) {
  if (/^[A-Za-z0-9_@./:=+-]+$/.test(value)) {
    return value;
  }
  return `"${value.replaceAll('"', '\\"')}"`;
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

function runSelfTest() {
  const baseCommands = buildBaseCommands();
  const baseCommandNames = baseCommands.map((command) => command.name);
  assertSelfTestIncludes(
    baseCommandNames,
    "staged diff whitespace check",
    "Self-test expected release-local base commands to include staged diff whitespace check.",
  );
  assertSelfTestCommandParts(
    baseCommands,
    "staged diff whitespace check",
    ["git", "diff", "--cached", "--check"],
  );
  const skipLivePlan = buildReleaseLocalPlan({
    skipLiveDeepSeek: true,
    requireLiveDeepSeek: false,
    hasDeepSeekKey: true,
    includeInstalledUi: false,
    includeInstalledWorkflow: false,
    env: {},
  });
  const skipLiveCommandNames = skipLivePlan.commands.map((command) => command.name);
  if (skipLivePlan.shouldRunLiveDeepSeek !== false) {
    throw new Error(
      "Self-test expected skip-live command list to exclude live smoke commands.",
    );
  }
  assertSelfTestExcludes(
    skipLiveCommandNames,
    "Windows local Operations Briefing smoke",
    "Self-test expected skip-live command list to exclude live smoke commands.",
  );
  assertSelfTestExcludes(
    skipLiveCommandNames,
    "DeepSeek Chat Completions smoke",
    "Self-test expected skip-live command list to exclude live smoke commands.",
  );
  assertSelfTestExcludes(
    skipLiveCommandNames,
    "DeepSeek Operations Briefing smoke",
    "Self-test expected skip-live command list to exclude live smoke commands.",
  );

  const installedUiCommand = buildInstalledUiCommand({
    includeInstalledWorkflow: false,
    skipLiveDeepSeek: false,
    env: {},
  });
  assertSelfTestCommandParts(
    [installedUiCommand],
    "Windows installed UI smoke",
    [
      "npx",
      "pnpm@9.15.9",
      "test:windows-installed-ui",
      "--",
      "--isolated-profile",
    ],
  );
  const installedWorkflowCommand = buildInstalledUiCommand({
    includeInstalledWorkflow: true,
    skipLiveDeepSeek: false,
    env: {},
  });
  assertSelfTestCommandParts(
    [installedWorkflowCommand],
    "Windows installed UI workflow smoke",
    [
      "npx",
      "pnpm@9.15.9",
      "test:windows-installed-ui",
      "--",
      "--isolated-profile",
      "--workflow",
    ],
  );
  const installedWorkflowPlan = buildReleaseLocalPlan({
    skipLiveDeepSeek: false,
    requireLiveDeepSeek: true,
    hasDeepSeekKey: true,
    includeInstalledUi: false,
    includeInstalledWorkflow: true,
    env: {},
  });
  assertSelfTestCommandParts(
    installedWorkflowPlan.commands,
    "Windows installed UI memory maintenance smoke",
    [
      "npx",
      "pnpm@9.15.9",
      "test:windows-installed-ui",
      "--",
      "--isolated-profile",
      "--memory-maintenance",
    ],
  );

  const command = buildInstalledUiCommand({
    includeInstalledWorkflow: true,
    skipLiveDeepSeek: true,
    env: {
      DEEPSEEK_API_KEY: "test-secret",
    },
  });
  if (command.env?.DEEPSEEK_API_KEY !== undefined) {
    throw new Error(
      "Self-test expected skip-live installed workflow command to clear DEEPSEEK_API_KEY.",
    );
  }
  const skipReason = liveDeepSeekSkipMessage({
    skipLiveDeepSeek: true,
    hasDeepSeekKey: true,
  });
  if (!skipReason.includes("--skip-live-deepseek")) {
    throw new Error(
      "Self-test expected explicit skip-live message to mention --skip-live-deepseek.",
    );
  }
  assertSelfTestThrows(
    () =>
      validateFlagCombination({
        skipLiveDeepSeek: true,
        requireLiveDeepSeek: true,
      }),
    "cannot be combined",
  );
  console.log("release-local-check self-test ok");
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

function assertSelfTestIncludes(values, expected, message) {
  if (!values.includes(expected)) {
    throw new Error(message);
  }
}

function assertSelfTestExcludes(values, unexpected, message) {
  if (values.includes(unexpected)) {
    throw new Error(message);
  }
}

function assertSelfTestCommandParts(commands, commandName, expectedParts) {
  const command = commands.find((candidate) => candidate.name === commandName);
  if (!command) {
    throw new Error(`Self-test expected command: ${commandName}`);
  }

  if (JSON.stringify(command.parts) !== JSON.stringify(expectedParts)) {
    throw new Error(
      `Self-test expected ${commandName} parts to be ${expectedParts.join(" ")}`,
    );
  }
}
