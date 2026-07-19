# Installation And First Run

This guide is for users who download or build DeepSeek Agent OS on their own
machine. Local paths are intentionally chosen at install or first run time; the
project does not depend on the maintainer's local directories.

## Windows Installer

The published Windows release provides a normal NSIS setup executable:

```text
DS.Agent_1.3.0_x64-setup.exe
```

Version `1.3.0` is the current published stable release. Earlier commits, tags,
Releases, and assets remain immutable. Both `ds-agent.exe` and the installer are
Authenticode `NotSigned`, so Windows may show `Unknown publisher` or a Microsoft
Defender SmartScreen warning. Download only over HTTPS from the official GitHub
Release and verify its byte size and SHA-256 against the `v1.3.0` Release before
running it. The installer
embeds the Microsoft WebView2 bootstrapper and runs it silently when the target
machine needs the WebView2 runtime; users do not need Node.js, pnpm, Rust, or a
source checkout to run the installed app.

The installer-selected application directory is only for program files. It is
not the workspace, evidence folder, export folder, or event database location.

## Code signing policy and verification

DS Agent `v1.3.0` is intentionally unsigned. Its HTTPS source, immutable tag,
exact byte size, and SHA-256 are the verification route for this asset. The
SignPath Foundation application is submitted and approval is pending; no
publisher, certificate, or signed status is claimed for v1.3.0. If signing is
approved later, it starts with a new version and does not replace this tag or
asset or any immutable v1.1.0 or v1.2.0 publication.

For releases accepted into the open-source signing program: **Free code signing
provided by SignPath.io, certificate by SignPath Foundation.** The complete
[Code signing policy](../CODE_SIGNING_POLICY.md) defines provenance, approval,
verification, and incident handling. The [privacy policy](../PRIVACY.md)
describes current local data and user-triggered network behavior.

On Windows, inspect a downloaded installer without launching it:

```powershell
Get-AuthenticodeSignature .\DS.Agent_1.3.0_x64-setup.exe |
  Select-Object Status, StatusMessage, SignerCertificate, TimeStamperCertificate
Get-FileHash .\DS.Agent_1.3.0_x64-setup.exe -Algorithm SHA256
```

For `v1.3.0`, `Status` is expected to be `NotSigned`. An unexpected signer,
signature identity, hash, version, source, or asset mismatch is a stop
condition. For a future release explicitly documented as signed, any status
other than `Valid` is also a stop condition.

At first run, choose one local workspace root. DS Agent uses that workspace for
approved local file actions and maintains subdirectories for evidence, exports,
reports, workflow runs, work packages, memory, logs, and future artifact types
as needed. Ordinary users should not need to choose separate internal folders
before starting their first chat task.

The required first-run setup is intentionally small: enter one user-supplied
DeepSeek API Key, explicitly verify balance and availability of
`deepseek-v4-flash` and `deepseek-v4-pro`, and choose one local workspace root.
The Key is stored in a dedicated Windows DPAPI vault; readiness receipts do not
contain the Key, provider response body, balance amount, account details, or an
absolute vault path. An environment Key remains an explicit operator
compatibility fallback and is never silently copied into the vault.

The app stores this local directory choice in `local-directories.json` under
the OS app data directory. The ordinary setup projection shows a compact
workspace display name and readiness state, not internal app-data, settings,
managed-directory, or vault paths.

When DeepSeek proposes a v1 GoalEnvelope, DS Agent locally validates and
freezes the bounded contract. The chat surface is read-only: it can show the
goal state, stable reason codes, revision/fingerprint, and verifier/artifact
coverage counts, but it cannot create local validation, evidence, or completion
authority.

For a queued task whose Goal is frozen, the Kernel may derive one exact-task
authorization card from a private descriptive capability proposal. Approve,
reject, and revoke operate only on the Kernel-issued task/group/manifest/preview
intent. Approval grants exact authority only; it does not execute a Tool, resume
the task, or mark the Goal complete.

Good first tasks to try:

- `根据我的证据文件夹，生成一份经营简报，并导出 HTML 和 PDF。`
- `把这段会议纪要整理成行动项、责任部门、截止时间和风险提示。`
- `继续上次的项目，先说明你用了哪些记忆，再给我下一步建议。`

## Build From Source

Install dependencies and verify the desktop package:

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 test
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --config src-tauri/tauri.windows.conf.json
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

Before any publication decision for a new release, run the local release gate:

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

When a Windows DS Agent install already exists, its executable can be launched
against a fresh isolated profile without reading or changing the user's normal
DS Agent AppData:

```powershell
npx pnpm@9.15.9 test:windows-installed-ui -- --isolated-profile
```

This creates independent temporary `APPDATA`, `LOCALAPPDATA`, WebView2,
workspace, and report roots, checks that the UI renders at `tauri.localhost`,
verifies the desktop command layer, and verifies cleanup after exit. It does not
back up, read, or restore the user's normal DS Agent profile. It is not run in
GitHub CI.

For a fuller installed-app workflow check, run:

```powershell
npx pnpm@9.15.9 test:windows-installed-ui -- --isolated-profile --workflow
```

The workflow smoke uses temporary local directories, seeds Operations Briefing
templates, runs the briefing, exports Markdown/HTML/PDF reports, and verifies
the isolated profile is removed after exit. When
`DEEPSEEK_API_KEY` is configured, it also requires a newly recorded DeepSeek
telemetry event from the installed app process.

macOS packaging has a committed Tauri config for `.app` and `.dmg`, but it still
needs verification on a macOS host.

## DeepSeek API Key

DeepSeek model-backed work is enabled only after the Kernel readiness
projection reports a verified user-supplied Key, available balance, and both
required V4 models. Use the in-app onboarding screen to save, verify, replace,
retry, or remove that Key. The ordinary UI never displays the stored value.

See `.env.example` for local environment variable names. Do not commit `.env`
files or API keys.

For development/operator compatibility, an environment Key can be used instead
of the local vault. It is not copied into the vault and still requires explicit
verification. For a persistent Windows user environment variable:

```powershell
[Environment]::SetEnvironmentVariable("DEEPSEEK_API_KEY", "your-key-here", "User")
```

Restart the desktop app after setting the variable, then run the in-app
verification. The readiness projection shows only source, stable status codes,
required model availability, and retry/repair actions; it never displays,
stores, exports, or serializes the key value.

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

## Current Connector Boundary

Microsoft/Google-shaped mail and calendar flows are implemented and validated
offline with adversarial fake providers. Production account registration and
live-provider execution remain disabled. The release therefore does not use a
real account, read live mail/calendar data, send mail, or create, modify, or
cancel real calendar events unless those separate boundaries are deliberately
enabled and authorized in a future release.

## Troubleshooting

- If the app asks for setup again, verify that the app data directory is
  writable and that `local-directories.json` still exists.
- If DeepSeek stays unavailable, use onboarding or Settings to retry the
  explicit Key check and follow the stable authentication, balance, network,
  model, or credential-store repair message.
- If web search is blocked, choose a free source-linked web-search option when
  prompted.
- If screenshot or control fails, check OS permission notes in the Tool Route
  Strategy inspector.
