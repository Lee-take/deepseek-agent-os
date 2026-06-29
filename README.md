# DeepSeek Agent OS

Independent local-first desktop Agent OS for DeepSeek model support.

## Project Introduction / 项目介绍

### English

DeepSeek Agent OS, also called DS Agent OS in short, is an independent
open-source desktop project written to help colleagues use DeepSeek large
language models more conveniently in daily work. It focuses on local desktop
agent workflows, permissioned tools, auditable memory, source traceability,
local files, and operations workflow packs.

This project is not an official DeepSeek product, is not affiliated with
DeepSeek, and does not claim any DeepSeek ownership, authorization, or
endorsement. The DeepSeek name is used only to describe compatibility and
support for DeepSeek models.

The project is shared publicly under the Apache-2.0 license. Friends,
colleagues, and anyone who finds it useful are welcome to use it, study it,
fork it, and adapt it within the license terms. When convenient, please open an
issue, discussion, or pull request with suggestions, corrections, criticism, or
other feedback. All advice is welcome.

### 中文

DeepSeek Agent OS，也可以简称 DS Agent OS，是一个独立开源的桌面端项目。写这个项目
的初衷，是为了让同事们在日常工作中更方便地使用 DeepSeek 大模型。项目重点放在本地
桌面 Agent 工作流、可授权工具、可审计记忆、来源追溯、本地文件和经营管理工作流包。

本项目不是 DeepSeek 官方产品，也不隶属于 DeepSeek；项目不主张任何 DeepSeek 的所有
权、授权或官方背书。这里使用 DeepSeek 名称，只是为了说明项目面向 DeepSeek 模型做
兼容和支持。

本项目按照 Apache-2.0 许可证对外开源。有需要的同事、朋友和开发者，都可以在许可证
范围内自由使用、学习、fork 和改造。大家方便或有空的时候，也欢迎通过 issue、讨论或
pull request 提出意见建议；批评指正都非常欢迎。

## Alpha Status

The project is in a v0.1-alpha feature freeze before the first public GitHub
release. Current work is limited to release hygiene, documentation, packaging,
verification, and bug fixes inside the existing scope.

License: Apache-2.0.

The first public v0.1-alpha release is source-only. Windows debug installer
artifacts are build outputs for local validation and are not attached to the
public alpha unless explicitly approved later.

Read first:

- `PROJECT_CONTEXT.md`
- `DECISIONS.md`
- `SESSION_HANDOFF.md`
- `docs/INSTALLATION.md`
- `docs/OPEN_SOURCE_RELEASE.md`
- `docs/RELEASE_NOTES_v0.1-alpha.md`
- `docs/superpowers/specs/2026-06-28-deepseek-agent-os-architecture-design.md`
- `CONTRIBUTING.md`
- `SECURITY.md`
- `.env.example`

## Development

Foundation MVP desktop commands:

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 test
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
npx pnpm@9.15.9 dev
```

`pnpm test` runs the repository secret scan, desktop frontend build, and Rust
tests. The scan covers tracked files plus unignored new files. On Windows, the
test helper automatically keeps Rust build output out of the repository path to
avoid local MinGW path parsing issues when the checkout path contains spaces.
Set `CARGO_TARGET_DIR` yourself only when you need a specific build cache
location.

To run only the repository secret scan before committing or pushing:

```powershell
npx pnpm@9.15.9 test:secrets
```

`test:deepseek` is an optional local smoke test. It reads `DEEPSEEK_API_KEY`
from the local environment, calls DeepSeek Chat Completions, and prints only
secret-safe metadata such as model, finish reason, and token usage. It is not
used by GitHub CI:

```powershell
npx pnpm@9.15.9 test:deepseek
```

`test:deepseek:briefing` is the optional local workflow smoke test. It sends the
Operations Briefing sample evidence manifest to DeepSeek, validates that the
response matches the workflow JSON contract, and prints only counts and token
metadata by default. When a custom evidence directory is an absolute local path,
the script redacts that path from both the prompt and the output:

```powershell
npx pnpm@9.15.9 test:deepseek:briefing
```

Windows builds automatically merge `apps/desktop/src-tauri/tauri.windows.conf.json`
and produce an NSIS installer under the configured Cargo target directory,
for example `debug/bundle/nsis/DeepSeek Agent OS_0.1.0_x64-setup.exe`.

macOS builds have a separate platform config at
`apps/desktop/src-tauri/tauri.macos.conf.json` for `.app` and `.dmg` packaging.
Run the same Tauri build command on a macOS host to produce those bundles.

## Installation And Local Directories

The app is installed like a normal desktop app, but the installation directory is
not used as the workspace or data directory.

- Program files live in the installer-selected application location.
- App state, SQLite events, logs, and local settings live under the OS-provided
  app data directory.
- First run asks for a default workspace, evidence folder, and export folder,
  with native folder picker buttons for each path.
- File, folder, Drive-local, evidence, and export-package paths remain runtime
  user inputs on the user's own machine.
- Local directory settings are stored as `local-directories.json` under the app
  data directory.
- Windows alpha packaging uses a Tauri NSIS installer. macOS packaging uses a
  separate `.app`/`.dmg` config, so Windows installer choices do not block macOS
  distribution.

## Architecture

The app uses a stable Agent OS Kernel with Workflow Packs. The first implementation slice builds the desktop shell, local event store, policy model, and DeepSeek route model.

Phase 2 has started with the capability permission loop: the kernel now declares built-in file, network, browser, email, drive, terminal, and Computer Use capabilities; access requests are evaluated through policy, persisted as append-only events, and resolved through a visible approval queue in the desktop inspector. Browser browse v1, BrowserSubmit boundary v1, NetworkSearch source adapter v1, FileRead v1, FileWrite local-workspace v1, evidence-folder ingestion v1, TerminalRead v1, TerminalWrite boundary v1, EmailRead boundary v1, EmailDraft boundary v1, EmailSend approval boundary v1, DriveRead local-folder v1, DriveWrite export-package v1, Computer Screenshot boundary v1, and ComputerControl boundary v1 are the first adapters: URL browsing fetches page evidence, BrowserSubmit records high-risk form-submission approval/audit requests without filling or submitting forms, NetworkSearch runs the selected free source-backed adapter after policy approval and records source URLs plus the provider/source-adapter label as evidence, local file reading previews UTF-8 evidence files and records encoding/byte-count metadata, FileWrite writes approved UTF-8 content into the configured local workspace and records encoding/byte-count metadata while rejecting paths outside that workspace, folder ingestion creates a bounded UTF-8 text evidence manifest with encoding/byte-count metadata, TerminalRead runs allowlisted read-only diagnostics and records nonzero exits as failed tool results, TerminalWrite records high-risk approval/audit requests without executing mutating commands, EmailRead records mailbox-read approval/audit requests without reading mail, EmailDraft records draft approval/audit requests without creating mailbox drafts, EmailSend records critical outbound-email approval/audit requests without sending mail, DriveRead scans a bounded local folder for matching UTF-8 text evidence and records encoding/byte-count metadata after policy approval, DriveWrite exports the current work-package JSON into a selected local folder after policy approval and shows the local output file name in the audit excerpt, Computer Screenshot captures local screen pixels after policy approval and saves PNG evidence, ComputerControl executes approved structured mouse/keyboard actions through a local input backend, and recent tool output appears in the inspector.

Critical one-shot approvals v1 keeps high-consequence permissions from becoming permanent grants. Non-critical explicit approvals can still be reused where appropriate, but `EmailSend` and `ComputerControl` approvals are consumed by the next same-capability invocation and must be approved again for a later attempt.

Capability grant state v1 makes that permission state visible in the inspector. Capability access records now include a derived `grant_state` so approved grants can be shown as reusable, one-shot available, or consumed without changing the append-only event payloads.

Approval traceability v1 adds an optional `approval_request_id` to capability invocations. Critical EmailSend and ComputerControl retries now link the recorded invocation back to the specific approved request that authorized it, while legacy invocations without the field remain readable.

Recent tool output now surfaces linked approval request IDs when present, so an operator can trace a recorded tool attempt back to the exact approval request from the inspector without opening raw event JSON.

Tool backend settings now record the confirmed Phase 2 direction in `FoundationState` and the runtime inspector: NetworkSearch reports the selected large model's capability, uses a native large-model bridge contract when the provider supports NetworkSearch and the local Codex bridge HTTP runtime is configured, and otherwise requires a selected free source-backed adapter before live search can run; EmailRead, EmailDraft, and EmailSend are architecture-only in this version; DriveRead and DriveWrite point to user-selected local folders and export packages instead of a cloud account; Computer Screenshot and ComputerControl use the Codex bridge contract for ChatGPT/Codex providers and local Windows/macOS backends for other providers. No API key or account credential is stored by this settings slice.

Setup and local directory contract v1 separates installation, app data, and user work directories. The runtime stores local directory settings under the OS app data directory and treats workspace, evidence, and export locations as user-selected paths on that user's machine. The setup panel uses native folder picker buttons for these paths, and the UI intentionally avoids hardcoded developer-machine paths.

Windows installer baseline v1 adds a platform-specific Tauri config that builds an NSIS installer on Windows. A macOS `.app`/`.dmg` config is also committed, but it still needs verification on a macOS host.

DeepSeek credential status v1 reads only whether `DEEPSEEK_API_KEY` is present in the local process environment and shows that status in the runtime inspector with the API base URL. DeepSeek Chat API readiness v1 also reports the derived Chat Completions endpoint, selected Flash/Pro model names, and whether local chat-completion requests are ready based on key presence. The key value is never returned to the UI, never serialized into events, and never included in exported work packages.

DeepSeek Chat executor v1 adds an injectable Chat Completions transport and a reqwest-backed implementation for the official endpoint. Tests use a fake transport and never call the live API; executor errors redact the local API key before returning. NetworkSearch evidence is handled by source-backed search adapters, not by treating plain chat completions as verified web evidence.

DeepSeek cache and telemetry v1 adds an in-memory request cache for Operations Briefing DeepSeek synthesis plus append-only, secret-safe telemetry events. The telemetry records a request hash, model, cache hit/miss status, elapsed milliseconds, token usage when the provider returns it, and estimated cost when local pricing is configured; the runtime inspector shows the latest DeepSeek call status, current in-session cache size, and a clear-cache action. DeepSeek pricing config v1 stores a manual USD / 1M token price table in the user's app data directory and never hardcodes public DeepSeek prices into the open-source project.

NetworkSearch route status v1 makes that product decision visible in the runtime inspector: the selected provider, native/source-model support, selected source model, execution mode, live-network status, and evidence policy are shown before a search runs. When a free source model is selected, the route switches to source-adapter execution with source links required. When a supported large model has the local native bridge configured, the route switches to native bridge contract execution with the same source-link evidence requirement.

NetworkSearch source adapter v1 adds a source-backed HTTP client for the free source-model path. The Tauri command requires a selected source model only when the native bridge route is unavailable, preserves the existing approval gate, avoids network requests while approval is pending, and records the search URL, first result URL, and provider/source-adapter label as durable evidence when search succeeds. In the alpha, reserved free options such as local browser search and source aggregator explicitly disclose that they currently share the source-backed HTTP adapter until separate implementations are confirmed.

Native NetworkSearch bridge contract v1 adds a loopback-only Codex bridge `/network-search` route for supported large-model providers. Bridge responses must include source URLs, and the app maps those URLs into the existing NetworkSearch evidence/audit boundary instead of treating ordinary chat completion text as web evidence.

Computer Use backend status v1 exposes the selected model-driven backend direction in the runtime inspector: ChatGPT/Codex select Codex bridge screen/input contracts, other providers select local Windows/macOS screen/input backends, and ComputerControl remains visibly approval-gated before any mouse/keyboard executor can run. The current desktop build does not silently fall back from an unconnected Codex bridge to local input.

Computer Use OS permission guidance v1 makes local desktop prerequisites visible in the same runtime inspector and exported tool-readiness snapshot. macOS local capture reports the Screen Recording permission requirement, macOS local control reports the Accessibility requirement, and Windows local backends report foreground-desktop and secure-desktop limitations instead of pretending every window can be inspected or controlled.

Codex bridge runtime readiness v1 makes ChatGPT/Codex Computer Use routes explicit before execution. The runtime inspector and exported tool-readiness snapshot show whether a Codex bridge is required, whether `DEEPSEEK_AGENT_OS_CODEX_BRIDGE_TRANSPORT=http` has selected the MVP external HTTP runtime, whether `DEEPSEEK_AGENT_OS_CODEX_BRIDGE_URL` is configured for loopback HTTP routes, and whether the bridge is connected.

Codex bridge contract schema v1 defines a transport-neutral JSON contract with version `deepseek-agent-os.codex-bridge.v1` for health, screenshot, and control messages. Screenshot responses carry display metadata plus PNG base64 rather than local file paths, and control requests still require the same structured action strings used by the local executor.

Codex bridge HTTP runtime v1 implements the approved external HTTP runtime for ChatGPT/Codex Computer Use routes. The desktop client only accepts loopback HTTP endpoints, posts `/health`, `/screenshot`, `/control`, and `/network-search` using the shared contract, marks bridge-routed backends available only when health reports the required capabilities, writes returned screenshot PNG bytes into the local evidence folder, and treats `stdio` sidecar spawning as deferred hardening work rather than an MVP runtime option.

Work-package tool readiness v1 now adds a secret-safe `tool_readiness` snapshot to exported work packages. The snapshot carries DeepSeek API readiness, NetworkSearch route/evidence gate status, Computer Use backend availability flags, local-directory setup readiness, and the model-driven tool strategy without serializing API key values or user machine paths, calling the live DeepSeek API, capturing pixels during export, or controlling the desktop.

ComputerControl structured action v1 limits real desktop input to a small audited protocol: `click:x,y[,button]`, `move:x,y`, `type:text`, `press:key`, `hotkey:key+key`, and `scroll:delta[,axis]`. The local backend uses `enigo` and still requires a one-shot ComputerControl approval plus a local in-memory unlock code before executing any mouse or keyboard action.

ComputerControl local unlock token v1 adds a second local gate around approved desktop input. At app startup the runtime generates a short in-memory challenge code, the inspector lets the local operator unlock ComputerControl for five minutes, and approved execution fails before any invocation is recorded if the local unlock window is not active. The code is not persisted into events or exported work packages.

Computer tool model-aware routing v1 passes the selected large-model provider into screenshot/control commands. ChatGPT/Codex routes use Codex bridge contract clients and record a clear failure when no bridge runtime is connected; DeepSeek/custom routes use the local Windows/macOS screenshot and input clients.

Computer Screenshot evidence-ref v1 separates the human display label from the durable evidence reference. Successful screenshot invocations now use screenshot evidence refs for audit output, while pending and failed invocations keep the existing approval/failure trail.

Computer Screenshot risk-gate v1 reclassifies screen pixel capture as a medium-risk Computer Use read. The default `ask_on_risk` access mode now creates a pending approval before screen inspection, while `limited_auto` can still auto-run medium-risk reads after policy evaluation.

Computer Screenshot local capture backend v1 wires `xcap` for local screen capture. The Tauri command prefers the user-selected evidence directory from first-run setup and falls back to OS app data before setup, then saves PNG files under `computer-screenshots/` and records a relative evidence ref for the audit trail.

The first Operations Management workflow is also wired: Operations Briefing runs through FileRead-backed evidence-folder ingestion, records an append-only workflow run, and shows a draft with summary, anomaly leads, and action items. When the selected large model is DeepSeek and `DEEPSEEK_API_KEY` is configured in the local process environment, the workflow uses the DeepSeek Chat executor for model-backed JSON synthesis after evidence ingestion succeeds; model failures fall back to the deterministic local draft with a warning. Exported work packages carry persisted briefing runs for handoff, workflow template packages for reuse, and pending memory candidates for review; imported runs are replayed as read-only archives, and imported workflow templates are registered locally without writing user folders. The latest run can be exported as Markdown, standalone HTML, or PDF through the DriveWrite approval loop. Markdown and HTML preserve full Unicode; PDF v1 is intentionally lightweight and ASCII-safe until an open CJK font or OS-font discovery strategy is approved. Sample evidence templates live under `docs/templates/operations-briefing-evidence/`, and the desktop UI can seed those templates into the user's local evidence folder through the FileWrite approval loop without overwriting existing files.

Memory Studio now has candidate review plus metadata v1: users can propose candidate memories with type, scope, sensitivity, lifecycle tags, and expiration dates; accept or reject them; and only accepted candidates become long-term memory records with those tags preserved. Candidate conflict surfacing v1 shows likely overlap with existing visible long-term memories before acceptance, and conflict details v1 shows the overlapping memory title, body, metadata, and update time so review is inspectable. Conflict actions are explicit: `Link and accept` saves the candidate as a separate long-term memory and records append-only links; `Merge and accept` writes the previewed merged draft as a new memory and tombstones selected source memories; `Replace and accept` writes the candidate as the replacement memory and tombstones selected target memories. Long-term memory edit v1 appends `memory_record.updated` events and shows the latest version in list/search without rewriting original events. Long-term memory expiration v1 hides expired memories from normal list/search while keeping original events readable. Long-term memory delete v1 appends a `memory_record.deleted` tombstone and hides deleted memories from list/search. Work-package imports now have a read-only preview for new/skipped task records, pending memory candidates, and archived briefing runs; imported candidates stay in local review and do not write long-term memory until accepted. Older task-record auto memory and legacy event payloads remain readable through default metadata.
