# Installation And First Run

This guide is for users who download or build DeepSeek Agent OS on their own
machine. Local paths are intentionally chosen at install or first run time; the
project does not depend on the maintainer's local directories.

## Windows Installer

The Windows build produces a normal NSIS setup executable:

```text
DS Agent_0.1.0_x64-setup.exe
```

The `v0.1.0` release-candidate prereleases attach this Windows installer for
colleague testing. It is unsigned, so Windows may show an unknown-publisher
warning. The installer embeds the Microsoft WebView2 bootstrapper and runs it
silently when the target machine needs the WebView2 runtime; users do not need
Node.js, pnpm, Rust, or a source checkout to run the installed app.

The installer-selected application directory is only for program files. It is
not the workspace, evidence folder, export folder, or event database location.

At first run, choose three local folders in the setup panel:

- Default workspace: where approved file writes can create local files.
- Default evidence folder: where evidence templates and screenshot evidence can
  be written.
- Default export folder: where reports and work packages can be exported.

The app stores these choices in `local-directories.json` under the OS app data
directory. The setup panel shows the exact app data and settings-file paths for
the current machine.

## Build From Source

Install dependencies and verify the desktop package:

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 test
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
```

`pnpm test` runs the repository secret scan, desktop frontend build, and Rust
tests. The scan covers tracked files plus unignored new files. On Windows, the
test helper sets a temporary Cargo target directory when `CARGO_TARGET_DIR` is
not already configured, which avoids local MinGW path parsing issues when the
checkout path contains spaces.

To run only the repository secret scan before committing or pushing:

```powershell
npx pnpm@9.15.9 test:secrets
```

Before any publication decision for a new prerelease, run the local
release-candidate gate:

```powershell
npx pnpm@9.15.9 test:release-local
```

This gate runs the full project test, working-tree and staged diff whitespace
checks (`git diff --check` and `git diff --cached --check`), and the
source-only release guard. The source-only guard checks version/name
consistency, required release docs, generated WebView2 loader ignore coverage,
Windows WebView2 bootstrapper packaging config, and currently tracked or
unignored files for accidental installer/binary release artifacts, local runtime
artifacts, generated workflow exports, unexpected binary files, oversized
source files, and stale smoke-test release labels. The local gate also runs
deterministic helper checks for the Windows
local Operations Briefing smoke helper, the installed UI helper, and the
release-local helper itself; the Windows local helper self-test does not call
DeepSeek or read local secrets. If `DEEPSEEK_API_KEY` is configured locally, the
gate also runs the Windows local Operations Briefing smoke test and both
DeepSeek live smoke tests. These live checks are local maintainer checks and are
not run in GitHub CI.
For an offline source-only pass, run:

```powershell
npx pnpm@9.15.9 test:release-local -- --skip-live-deepseek
```

To run only the source-only release guard:

```powershell
npx pnpm@9.15.9 test:release-source
```

To include the installed Windows UI in this gate, add
`-- --include-installed-ui`. To include the stronger installed-app workflow
smoke, add `-- --include-installed-workflow`.
When a local DeepSeek test key and installed Windows app are available, use the
strongest local gate before that publication decision:

```powershell
npx pnpm@9.15.9 test:release-local -- --require-live-deepseek --include-installed-workflow
```

When a Windows DS Agent install already exists, the installed WebView2 UI can be
smoke-tested locally:

```powershell
npx pnpm@9.15.9 test:windows-installed-ui
```

This launches the installed `ds-agent.exe` with a temporary WebView2 DevTools
port, checks that the UI renders at `tauri.localhost`, verifies that the
desktop command layer is available, and saves a screenshot under the OS temp
directory. It is not run in GitHub CI.

For a fuller installed-app workflow check, run:

```powershell
npx pnpm@9.15.9 test:windows-installed-ui -- --workflow
```

The workflow smoke uses temporary local directories, seeds Operations Briefing
templates, runs the briefing, exports Markdown/HTML/PDF reports, and restores
the original local directory settings file and app-data event store. When
`DEEPSEEK_API_KEY` is configured, it also requires a newly recorded DeepSeek
telemetry event from the installed app process.

macOS packaging has a committed Tauri config for `.app` and `.dmg`, but it still
needs verification on a macOS host.

## DeepSeek API Key

DeepSeek model-backed Operations Briefing synthesis is enabled only when the
selected large-model provider is DeepSeek and the desktop process can read a
non-empty `DEEPSEEK_API_KEY` environment variable.

See `.env.example` for local environment variable names. Do not commit `.env`
files or API keys.

For a persistent Windows user environment variable:

```powershell
[Environment]::SetEnvironmentVariable("DEEPSEEK_API_KEY", "your-key-here", "User")
```

Restart the desktop app after setting the variable. The runtime inspector only
shows whether the key is configured; it never displays, stores, exports, or
serializes the key value.

To verify the local key without launching the app, run:

```powershell
npx pnpm@9.15.9 test:deepseek
```

The smoke test reads only local environment variables, sends a minimal Chat
Completions request, and prints secret-safe metadata such as model, finish
reason, token usage, and elapsed time. It does not print the API key and is not
run in GitHub CI.

To verify the local Operations Briefing synthesis path against the sample
evidence templates, run:

```powershell
npx pnpm@9.15.9 test:deepseek:briefing
```

This workflow smoke test uses
`docs/templates/operations-briefing-smoke-evidence` by default, validates the
returned JSON shape, and prints counts plus token metadata. The desktop app's
seed-template button still uses the blank operator templates under
`docs/templates/operations-briefing-evidence`. The bundled smoke files are
marked as `SMOKE SAMPLE evidence for local verification only` and
`Replace before operational use`. Replace them before pointing the workflow at
real business evidence. Set
`DEEPSEEK_BRIEFING_EVIDENCE_DIR` to point at another local evidence folder.
Absolute local evidence paths are redacted from the model prompt and script
output. Keep private evidence local and do not commit it.

## DeepSeek Pricing

Cost estimates are optional. The app does not hardcode public DeepSeek prices.

Use the DeepSeek Pricing panel to enter a local manual price table in USD per
1M tokens:

- Flash input
- Flash output
- Pro input
- Pro output

These settings are saved as `deepseek-pricing.json` under the OS app data
directory. If pricing is disabled, missing, or does not match the model used by
a telemetry event, cost remains empty while latency, cache status, and token
counts still work.

DeepSeek Chat responses used by Operations Briefing synthesis are cached only in
the current desktop session. Use the Tool Route Strategy inspector to see the
current cache entry count or clear cached responses. Clearing the cache does not
delete telemetry events.

## Report Exports

Operations Briefing runs can be exported as Markdown, standalone HTML, or PDF.
Use Markdown or HTML when the report contains Chinese or other Unicode text. PDF
v1 uses lightweight built-in PDF fonts and keeps output ASCII-safe until a
bundled open CJK font or runtime OS-font discovery strategy is approved.

## Network Search

Web search depends on the selected model route:

- If the selected model route can provide source-linked web search through a
  configured local bridge service, the app still requires source links.
- If the selected model route does not provide web search, choose a free
  source-linked web-search option in the UI before running search.
- Some early source-linked web-search options may share the same local
  implementation until separate local-browser or aggregator implementations are
  confirmed.
- DeepSeek Chat completions are not treated as verified web evidence by
  themselves.

## Computer Use

Screen inspection and computer control are permissioned Computer Use features.

- Optional local bridge routes use the configured local bridge service when a
  local loopback HTTP bridge is available.
- DeepSeek and custom routes use local Windows or macOS screen and input paths.
- Computer control requires an explicit one-shot approval and a short local
  unlock token before real mouse or keyboard input executes.
- macOS local screenshot/control paths require Screen Recording and
  Accessibility permissions. Windows local paths can be limited by secure
  desktop prompts or elevated target windows.

For optional local bridge use, start a compatible local HTTP service yourself
and then launch the desktop app with:

```powershell
$env:DEEPSEEK_AGENT_OS_BRIDGE_TRANSPORT = "http"
$env:DEEPSEEK_AGENT_OS_BRIDGE_URL = "http://127.0.1.0:47329"
```

The desktop app only accepts loopback bridge URLs. It checks bridge readiness
before using the local bridge for screen inspection, computer control, and
source-linked web search. In this preview, start and stop the bridge service
yourself; DS Agent does not launch or manage the bridge service in this preview.

## Current Deferred Connectors

Email read, draft, and send tools are approval and audit surfaces in this
version. They record permission decisions but do not read, draft, or send real
mail yet.

Local folder read and work-package export use local folders and local export
packages in this version. Cloud-drive connectors are deferred.

## Troubleshooting

- If the app asks for setup again, verify that the app data directory is
  writable and that `local-directories.json` still exists.
- If DeepSeek synthesis stays unavailable, restart the app after setting
  `DEEPSEEK_API_KEY` and check the runtime inspector.
- If web search is blocked, choose a free source-linked web-search option when
  prompted.
- If screenshot or control fails, check OS permission notes in the Tool Route
  Strategy inspector.
