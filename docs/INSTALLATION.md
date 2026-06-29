# Installation And First Run

This guide is for users who download or build DeepSeek Agent OS on their own
machine. Local paths are intentionally chosen at install or first run time; the
project does not depend on the maintainer's local directories.

## Windows Installer

The Windows build produces a normal NSIS setup executable:

```text
DeepSeek Agent OS_0.0.1_x64-setup.exe
```

The installer-selected application directory is only for program files. It is
not the workspace, evidence folder, export folder, or event database location.

At first run, choose three local folders in the setup panel:

- Default workspace: where FileWrite can create approved local files.
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

This workflow smoke test uses `docs/templates/operations-briefing-evidence` by
default, validates the returned JSON shape, and prints counts plus token
metadata. Set `DEEPSEEK_BRIEFING_EVIDENCE_DIR` to point at another local
evidence folder. Absolute local evidence paths are redacted from the model prompt
and script output. Keep private evidence local and do not commit it.

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
the current desktop session. Use the Tool Backend Strategy inspector to see the
current cache entry count or clear cached responses. Clearing the cache does not
delete telemetry events.

## Report Exports

Operations Briefing runs can be exported as Markdown, standalone HTML, or PDF.
Use Markdown or HTML when the report contains Chinese or other Unicode text. PDF
v1 uses lightweight built-in PDF fonts and keeps output ASCII-safe until a
bundled open CJK font or runtime OS-font discovery strategy is approved.

## Network Search

NetworkSearch depends on the selected large model:

- If the selected model route has native bridge-backed NetworkSearch available,
  the app uses the Codex bridge contract and still requires source links.
- If the selected model does not provide NetworkSearch, choose one of the free
  source-model options in the UI before running search.
- Alpha free-source presets may share the same source-backed HTTP adapter until
  separate local-browser or aggregator implementations are confirmed.
- DeepSeek Chat completions are not treated as verified web evidence by
  themselves.

## Computer Use

Computer Screenshot and ComputerControl are permissioned Computer Use features.

- ChatGPT and Codex routes use the Codex bridge contract when an external
  loopback HTTP bridge is configured.
- DeepSeek and custom routes use local Windows or macOS screenshot/input
  backends.
- ComputerControl requires an explicit one-shot approval and a short local
  unlock token before real mouse or keyboard input executes.
- macOS local screenshot/control paths require Screen Recording and
  Accessibility permissions. Windows local paths can be limited by secure
  desktop prompts or elevated target windows.

For the MVP bridge runtime, start a compatible local HTTP service yourself and
then launch the desktop app with:

```powershell
$env:DEEPSEEK_AGENT_OS_CODEX_BRIDGE_TRANSPORT = "http"
$env:DEEPSEEK_AGENT_OS_CODEX_BRIDGE_URL = "http://127.0.0.1:47329"
```

The desktop app only accepts loopback bridge URLs. It checks `/health` and then
uses `/screenshot`, `/control`, and `/network-search` through the shared bridge
contract. Managed stdio sidecar spawning is deferred.

## Current Deferred Connectors

EmailRead, EmailDraft, and EmailSend are boundary and approval surfaces in this
version. They record permission decisions but do not read, draft, or send real
mail yet.

DriveRead and DriveWrite use local folders and local export packages in this
version. Cloud-drive connectors are deferred.

## Troubleshooting

- If the app asks for setup again, verify that the app data directory is
  writable and that `local-directories.json` still exists.
- If DeepSeek synthesis stays unavailable, restart the app after setting
  `DEEPSEEK_API_KEY` and check the runtime inspector.
- If NetworkSearch is blocked, choose a free source model when prompted.
- If screenshot or control fails, check OS permission notes in the Tool Backend
  Strategy inspector.
