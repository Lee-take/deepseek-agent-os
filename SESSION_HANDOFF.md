# Session Handoff

Last updated: 2026-06-29

## Current Goal

Design an open-source desktop Agent OS optimized for DeepSeek, with a beautiful modern UI, serious harness architecture, strong memory, granular permissions, loop engineering, and a first Operations Management workflow pack.

## Current Stage

Foundation MVP implementation has started and is completed through the current approved slice:

- Repository baseline
- Tauri/React desktop shell
- Rust kernel models
- DeepSeek route defaults
- SQLite event store
- Policy foundation
- UI/workbench state wiring

Phase 2 has started and the first tool-access/permission-closure slice is now implemented:

- Built-in capability catalog for file, network, browser, email, drive, terminal, and Computer Use.
- Policy-backed capability access requests with `auto_approved`, `pending_approval`, `approved`, `rejected`, and `denied` states.
- Append-only event-store records for capability access requests and user resolutions.
- Tauri commands for listing capabilities, requesting access, listing access records, and approving/rejecting pending requests.
- React inspector panel showing capability cards, pending approvals, and recent permission audit entries.

Critical one-shot approvals v1 is implemented:

- Non-critical explicit approvals can still act as reusable grants where existing flows depend on them.
- `EmailSend` and `ComputerControl` approvals are one-shot: an approved request is available for the next same-capability invocation only.
- Once a same-capability invocation is recorded after the approval timestamp, the approval is considered consumed.
- Append-only approval and invocation records are preserved for audit.
- UI pending hints for EmailSend and ComputerControl now say the approved retry is one-shot.

Capability grant state v1 is implemented:

- `CapabilityAccessRecord` now includes derived `grant_state`.
- Grant states are `not_granted`, `reusable`, `one_shot_available`, and `one_shot_consumed`.
- `has_user_approved_capability` now uses the same derived grant state used by the UI.
- The inspector shows grant badges for reusable, one-shot available, and consumed grants; `not_granted` stays hidden to reduce noise.
- Persisted event payloads are unchanged.

Approval traceability v1 is implemented:

- `CapabilityInvocation` now includes serde-defaulted optional `approval_request_id`.
- Critical EmailSend and ComputerControl retries attach the concrete approved request ID to the recorded invocation.
- Newly created EmailSend/ComputerControl pending invocations also point at their pending access request.
- One-shot consumption prefers explicit `approval_request_id`; legacy invocations without it still consume by same capability and timestamp.
- TypeScript mirrors `approval_request_id` as nullable.
- Recent tool output shows linked approval request IDs when present.

Browser capability adapter v1 is implemented:

- `browse_url` Tauri command runs `BrowserBrowse` through policy before fetching.
- Default `ask_on_risk` creates a pending approval and does not fetch until the user approves.
- `limited_auto` can fetch directly; historical `auto_approved` entries are not treated as reusable grants.
- Explicit user approvals allow a later browser browse retry to execute.
- The adapter uses `reqwest` blocking HTTP with Rustls, follows up to 5 redirects, extracts a basic title/text excerpt, records `capability_invocation.recorded`, and shows recent tool output in the inspector.
- Limit: this is browse/open URL only; real public web search provider execution remains separate.

BrowserSubmit boundary v1 is implemented:

- `run_browser_submit_boundary` in `apps/desktop/src-tauri/src/kernel/capability.rs` validates a target URL and submission summary, then runs `BrowserSubmit` through the existing high-risk policy path.
- `submit_browser_boundary` Tauri command records access requests, permission audits, and capability invocations.
- `ask_on_risk` and `limited_auto` create a pending approval because BrowserSubmit is high risk.
- If policy allows or approval is present, the invocation is recorded as blocked/failed with a warning that browser form submission is not enabled and no form was submitted.
- The React inspector now has a Browser Submit Boundary form under the Browser Tool.
- Limit: this is an approval/audit boundary only; real browser filling/submission still requires a separate executor design and explicit enablement.

NetworkSearch source adapter v1 is implemented:

- `run_network_search_boundary` in `apps/desktop/src-tauri/src/kernel/capability.rs` validates a search query, applies a default `public web` scope when none is supplied, and runs `NetworkSearch` through policy.
- The runner now accepts a `NetworkSearchClient`; pending approval returns before the client is called, so approval gates still prevent live network access.
- `HttpNetworkSearchClient` executes the selected free source-backed adapter path and currently uses DuckDuckGo HTML as the free web source.
- Reserved free options now disclose in UI notes that they are alpha presets and currently share the source-backed HTTP adapter until separate local-browser or aggregator implementations are confirmed.
- Search success records `requested_url` as the search URL and `evidence_url`/`evidence_ref` as the first source result URL.
- Successful NetworkSearch invocation titles now include the provider/source-adapter label, so audit output distinguishes native bridge results from selected free source adapters.
- Empty result sets are recorded as failed because source-backed search must preserve source links.
- `search_network_boundary` Tauri command now receives the selected large-model provider, uses the native bridge route when available, otherwise requires `network_search_source_model`, builds the HTTP source client, and records access requests, permission audits, and capability invocations.
- `ask_every_step` creates a pending approval; `ask_on_risk` can execute after policy allows because `NetworkSearch` is low risk and source model is selected.
- The React inspector now has a Network Search Boundary form under the Browser Tool.
- ChatGPT can use native NetworkSearch only through the local Codex bridge `/network-search` contract when the bridge HTTP runtime is configured.
- Plain DeepSeek/Chat Completions responses are not treated as verified web evidence.

Native NetworkSearch bridge contract v1 is implemented:

- `CodexBridgeCapability` now includes `network_search`, and the default bridge health request advertises the requested NetworkSearch capability alongside screenshot/control.
- `CodexBridgeNetworkSearchRequest` carries contract version, capability, selected large-model provider, query, and scope.
- `CodexBridgeNetworkSearchResponse` requires provider, query, scope, search URL, and at least one HTTP(S) source-link item.
- `CodexBridgeHttpClient::network_search` posts to the loopback-only `/network-search` route.
- `CodexBridgeNetworkSearchClient` implements the existing `NetworkSearchClient` trait, validates bridge contract version/capability/source links, and maps bridge results into the existing `NetworkSearch` evidence fields.
- `search_network_boundary` routes to `CodexBridgeNetworkSearchClient::from_env` when model strategy selects `native_large_model`; otherwise it keeps the selected free source-backed adapter path.

FileRead capability adapter v1 is implemented:

- `read_local_file` Tauri command runs `FileRead` through policy before reading.
- `ask_on_risk` can read directly because FileRead is low risk; `ask_every_step` creates a pending approval and does not read until approved.
- Reads local UTF-8 text files up to 512 KiB through `LocalFileContentClient`.
- The invocation records generic `requested_resource` and `evidence_ref` fields so tool output can represent URLs and files.
- Successful reads add a metadata warning with `utf-8` encoding and byte count so the inspector shows what kind of evidence was read.
- The React inspector now has a File Tool path input and shows file reads in the same recent tool output stream.
- Limit: binary/PDF/Office parsing is not implemented yet.

FileWrite boundary v1 is implemented:

- `run_file_write_boundary` in `apps/desktop/src-tauri/src/kernel/capability.rs` validates a target path, change summary, and content, then runs `FileWrite` through the existing high-risk policy path.
- `write_file_boundary` Tauri command records access requests, permission audits, and capability invocations.
- `ask_on_risk` and `limited_auto` create a pending approval because FileWrite is high risk.
- If policy allows or approval is present, `LocalWorkspaceFileWriteClient` writes UTF-8 content into the configured local workspace, creating parent directories as needed.
- Successful FileWrite results carry `encoding` and `bytes` metadata, and inspector excerpts show `utf-8 text` plus bytes written.
- The writer rejects parent-directory traversal and absolute paths outside the configured workspace.
- Before first-run setup, the command falls back to an app-data `workspace` folder instead of arbitrary developer-machine paths.
- The React inspector now has a File Write Boundary form under the File Tool with path, summary, and content fields.
- Current limit: file write content is capped at 512 KiB.

Evidence folder ingestion v1 is implemented:

- `ingest_evidence_folder` Tauri command runs through the same `FileRead` permission model before scanning a local folder.
- `ask_on_risk` can ingest directly because this is a low-risk local read; `ask_every_step` creates a pending approval and does not scan until approved.
- The local folder client scans only the top-level folder for bounded UTF-8 text evidence: `.txt`, `.md`, `.csv`, `.json`, `.log`, `.yaml`, and `.yml`.
- Current limits are 20 files and 512 KiB per file.
- Accepted evidence files now carry `encoding` and `bytes` metadata, and successful inspector excerpts show `utf-8 text` plus byte counts for each accepted file.
- The resulting capability invocation stores a manifest-style excerpt with file count, total bytes, accepted file names, and metadata; `requested_resource` and `evidence_ref` point to the folder path.
- The React inspector now has an Evidence Folder path input under the File Tool surface and shows folder ingests in the shared recent tool output stream.

TerminalRead capability adapter v1 is implemented:

- `run_terminal_read` Tauri command runs `TerminalRead` through policy before running a command.
- `ask_on_risk` can run directly because TerminalRead is low risk; `ask_every_step` creates a pending approval and does not execute until approved.
- The runtime rejects commands outside the TerminalRead allowlist before execution.
- Current allowlist: `pwd`, `git status --short`, `git diff --stat`, and `git branch --show-current`.
- Local execution captures stdout, stderr, and exit code, truncates command output to 4,000 characters, and records a `capability_invocation.recorded` event.
- A TerminalRead command that starts but exits nonzero is recorded as a failed invocation with stderr in the excerpt and the exit code in warnings.
- The React inspector now has a Terminal Read Tool preset selector and shows command results in the shared recent tool output stream.
- Limit: TerminalRead v1 deliberately does not accept arbitrary shell input.

TerminalWrite approval boundary v1 is implemented:

- `run_terminal_write` Tauri command runs `TerminalWrite` through policy and records access requests, audit entries, and invocations.
- `run_terminal_write_boundary` validates and normalizes the requested command but does not spawn a shell process.
- `ask_on_risk` and `limited_auto` create a pending approval because TerminalWrite is high risk.
- If policy is approved or auto-allowed, the invocation is recorded as blocked/failed with a warning that TerminalWrite execution is not enabled and no command was run.
- The React inspector now has a Terminal Write Boundary form under Terminal Read.
- Limit: this is an approval/audit boundary only; a real mutating command executor still requires a separate design and explicit confirmation.

EmailRead boundary v1 is implemented:

- `run_email_read_boundary` in `apps/desktop/src-tauri/src/kernel/capability.rs` validates mailbox/folder and query text, then runs `EmailRead` through the existing policy path.
- `read_email_boundary` Tauri command records access requests, permission audits, and capability invocations.
- Because `EmailRead` is medium risk, `ask_on_risk` creates a pending approval while `full_access` records the blocked/failed boundary invocation directly.
- If policy allows or approval is present, the invocation is recorded as blocked/failed with a warning that mailbox reading is not enabled and no email was read.
- The React inspector now has an Email Read Boundary form with mailbox/folder and query fields.
- Limit: this is an approval/audit boundary only; real mailbox reading still requires selecting and wiring a mailbox connector.

EmailDraft boundary v1 is implemented:

- `run_email_draft_boundary` in `apps/desktop/src-tauri/src/kernel/capability.rs` validates recipient, subject, and body, then runs `EmailDraft` through the existing policy path.
- `create_email_draft_boundary` Tauri command records access requests, permission audits, and capability invocations.
- Because `EmailDraft` is low risk, `ask_on_risk` and `full_access` record the blocked/failed boundary invocation directly; `ask_every_step` creates a pending approval first.
- If policy allows or approval is present, the invocation is recorded as blocked/failed with a warning that draft creation is not enabled and no mailbox draft was created.
- The React inspector now has an Email Draft Boundary form with recipient, subject, and body fields.
- Limit: this is an approval/audit boundary only; real mailbox draft creation still requires selecting and wiring a mailbox connector.

EmailSend approval boundary v1 is implemented:

- `run_email_send_boundary` in `apps/desktop/src-tauri/src/kernel/capability.rs` validates recipient, subject, and body, then runs `EmailSend` through the existing critical-risk policy path.
- `send_email_boundary` Tauri command records access requests, permission audits, and capability invocations.
- `full_access` still creates a pending approval until the user explicitly approves EmailSend because email sending is critical risk.
- If approved, the invocation is recorded as blocked/failed with a warning that EmailSend execution is not enabled and no email was sent.
- The React inspector now has an Email Send Boundary form with recipient, subject, and body fields.
- Limit: this is an approval/audit boundary only; real outbound sending still requires selecting and wiring a mailbox connector.

DriveRead local-folder v1 is implemented:

- `run_drive_read_boundary` in `apps/desktop/src-tauri/src/kernel/capability.rs` validates local folder location and query text, then runs `DriveRead` through the existing policy path.
- `read_drive_boundary` Tauri command records access requests, permission audits, and capability invocations.
- Because `DriveRead` is low risk, `ask_on_risk` and `full_access` can read directly; `ask_every_step` creates a pending approval first and does not touch the filesystem until approved.
- If policy allows or approval is present, `LocalDriveFolderClient` scans only top-level supported text files in the selected local folder, bounded by file count and file size, and filters matches by file name or text content.
- Matching DriveRead entries now carry `encoding` and `bytes` metadata, and successful inspector excerpts show `utf-8 text` plus byte counts for each matched file.
- The React inspector now has a Drive local folder form with local folder path and keyword fields.
- Limit: real cloud-drive reading is still deferred; this version intentionally uses local folders.

DriveWrite export-package v1 is implemented:

- `run_drive_write_boundary` in `apps/desktop/src-tauri/src/kernel/capability.rs` validates local export folder and write summary, then runs `DriveWrite` through the existing high-risk policy path.
- `write_drive_boundary` Tauri command records access requests, permission audits, and capability invocations.
- `ask_on_risk` and `limited_auto` create a pending approval because DriveWrite is high risk; `full_access` can export after policy evaluation.
- If policy allows or approval is present, the command builds the current work package JSON from the event store and writes it to `deepseek-agent-os-work-package-<uuid>.json` in the selected local folder.
- Successful DriveWrite excerpts now include the local output file name while `evidence_ref` keeps the full written path for audit.
- The React inspector now has a Drive Export Package form with target local folder and export-summary fields.
- Limit: real cloud-drive uploading or modification is still deferred; this version writes local export packages only.

Computer Screenshot boundary v1 is implemented:

- `run_computer_screenshot` in `apps/desktop/src-tauri/src/kernel/capability.rs` uses the existing policy flow for `ComputerScreenshot`.
- A `ComputerScreenshotClient` trait defines the platform capture contract; tests use a fake client for success, pending approval, and capture failure paths.
- `capture_computer_screenshot` Tauri command records access requests, permission audits, and capability invocations.
- The React inspector now has a Screenshot Boundary control under the tool surfaces.

Computer Screenshot evidence-ref v1 is implemented:

- `ComputerScreenshot` now carries both `display_label` and `evidence_ref`.
- Successful screenshot invocations use the screenshot `evidence_ref` rather than the display label, so real screenshots point at saved local evidence.
- Pending and failed screenshot invocations are unchanged.

Computer Screenshot risk-gate v1 is implemented:

- `capability_risk(ComputerScreenshot)` is now `medium`, not `low`.
- `ask_on_risk` creates a pending approval for `ComputerScreenshot` and does not call the screenshot client until approved.
- `limited_auto` still allows medium-risk screenshot reads after policy evaluation.
- Tests cover policy and boundary behavior for this approval gate.

Computer Screenshot local capture backend v1 is implemented:

- `LocalScreenshotCaptureBackend` and `CapturedScreenshotImage` split OS capture from evidence-file persistence.
- `XcapLocalScreenshotCaptureBackend` uses `xcap::Monitor::all()`, prefers the primary display, captures an RGBA image, encodes it as PNG through `xcap::image`, and returns screen metadata plus PNG bytes.
- `LocalComputerScreenshotClient` saves PNG evidence under `<evidence_dir>/computer-screenshots/` when first-run directory settings exist, or `<app_data_dir>/computer-screenshots/` before setup.
- Evidence refs are relative strings such as `computer-screenshots/<uuid>-primary-display.png`, so exported audit records remain portable across machines.
- Tests cover PNG evidence writing, empty capture rejection, and evidence-base directory selection.
- Real capture can still fail at runtime if the OS denies screen-capture permission or no display is available; those failures are recorded as invocation warnings.

ComputerControl boundary v1 is implemented:

- `run_computer_control_boundary` in `apps/desktop/src-tauri/src/kernel/capability.rs` validates target/context and a structured action, then runs `ComputerControl` through the existing critical-risk policy path.
- `control_computer_boundary` Tauri command records access requests, permission audits, and capability invocations.
- `full_access` still creates a pending approval until the user explicitly approves ComputerControl because desktop control is critical risk.
- If approved, the invocation executes through the injected `ComputerControlClient` and records success or executor failure with warnings.
- The React inspector now has a Computer Control Boundary form under the Screenshot Boundary control.

ComputerControl structured action v1 is implemented:

- The action field accepts only `click:x,y[,button]`, `move:x,y`, `type:text`, `press:key`, `hotkey:key+key`, and `scroll:delta[,axis]`.
- Natural-language actions such as `Click the Submit button` are rejected before policy execution.
- `ComputerControlClient` and `LocalComputerControlInputBackend` split policy/audit from low-level input simulation.
- `LocalComputerControlClient` translates structured actions to backend operations. Hotkeys press keys in order and release them in reverse order.
- `EnigoLocalComputerControlInputBackend` uses `enigo` for local mouse/keyboard execution on Windows/macOS-capable builds.
- Tests cover action parsing, approval gating without executor calls, approved execution, executor failure recording, click translation, and hotkey release order.
- Codex bridge execution is still represented by model strategy, but not wired to an external bridge call in this slice.

ComputerControl local unlock token v1 is implemented:

- `AppState` now keeps a `ComputerControlUnlockState` in memory next to the event store. It generates a short local challenge code on app startup.
- New Tauri commands `get_computer_control_unlock_status` and `unlock_computer_control` expose only the challenge, lock state, and expiry timestamp to the inspector.
- A matching token unlocks ComputerControl for five minutes; wrong tokens return `invalid computer control unlock token` and do not consume approval.
- `control_computer_boundary` checks the unlock only after a one-shot ComputerControl approval is available, and before it calls the executor or appends a `CapabilityInvocation`.
- The React inspector now shows a local unlock form next to the ComputerControl form. The unlock code is runtime state only; it is not written to append-only events or exported work packages.

Computer tool model-aware routing v1 is implemented:

- `capture_computer_screenshot` and `control_computer_boundary` now accept `large_model_provider` and optional `network_search_source_model`.
- The React inspector passes the current model/provider state into both commands.
- Commands use `computer_tool_strategy_for_command` to pick local Windows/macOS clients for DeepSeek/custom routes and Codex bridge clients for ChatGPT/Codex routes.
- `CodexBridgeComputerScreenshotClient` and `CodexBridgeComputerControlClient` intentionally return a clear unconnected-bridge failure in this desktop build.
- The app does not silently fall back from ChatGPT/Codex bridge selection to local screen/input backends.
- Tests cover command strategy routing plus bridge client failure behavior.

Tool backend settings v1 is implemented:

- `FoundationState` now includes `large_model_provider`, optional `network_search_source_model`, and `tool_backends`.
- Confirmed settings are now model-driven:
  - NetworkSearch reports the selected large model's native capability, uses the native bridge contract when a supported provider has the local Codex bridge HTTP runtime configured, and otherwise uses a selected free source-backed adapter.
  - The UI prompts for a NetworkSearch source model and offers free preset options only when the selected route needs a separate source-backed adapter.
  - EmailRead, EmailDraft, and EmailSend stay as boundary/architecture surfaces in this version.
  - DriveRead scans bounded local folders and DriveWrite exports current work-package JSON to a local folder; cloud account connectors are deferred.
  - ChatGPT and Codex select the Codex bridge contract for ComputerScreenshot and ComputerControl.
  - Other large models select local Windows screenshot/input backends on Windows and local macOS screenshot/input backends on macOS.
- The React inspector renders the backend strategy in both Chinese and English.
- Limit: this slice stores no DeepSeek API key, mailbox credential, cloud-drive credential, or OS-control token.

Model-driven tool strategy v1 is implemented:

- `apps/desktop/src-tauri/src/kernel/tool_strategy.rs` derives `ModelDrivenToolStrategy` from large-model provider, optional NetworkSearch source model, and runtime platform.
- DeepSeek and custom models require a separate source-backed NetworkSearch model.
- ChatGPT is marked as capable of native NetworkSearch and uses `native_large_model` only when `DEEPSEEK_AGENT_OS_CODEX_BRIDGE_TRANSPORT=http` and `DEEPSEEK_AGENT_OS_CODEX_BRIDGE_URL` point to a local bridge runtime.
- Codex uses the Codex bridge for Computer Use but still requires a separate source-backed NetworkSearch model.
- The desktop toolbar now has a large-model provider selector for DeepSeek, ChatGPT, Codex, and Custom.
- The NetworkSearch boundary form shows a required source-model dropdown when the selected route lacks native bridge-backed NetworkSearch.
- The form is blocked until the source model is selected.
- Free preset source-model options are `free_web_source`, `free_local_browser`, and `free_source_aggregator`.

DeepSeek credential status v1 is implemented:

- `apps/desktop/src-tauri/src/kernel/deepseek.rs` now exposes `DeepSeekCredentialStatus`.
- `get_deepseek_credential_status` reads only whether `DEEPSEEK_API_KEY` is present in the local process environment.
- The returned status includes `base_url=https://api.deepseek.com`, `chat_completions_url=https://api.deepseek.com/chat/completions`, the env var name, selected Flash/Pro model names, and local readiness booleans.
- The key value is never returned to the UI, serialized into events, or included in exported work packages.
- The React inspector renders DeepSeek API status under Tool Backend Strategy.

DeepSeek Chat API readiness v1 is implemented:

- `build_deepseek_chat_completion_request` constructs a non-streaming Chat Completions request with system/user messages, selected model route, and thinking payload.
- Fast thinking maps to the Flash model with thinking disabled; Deep maps to the Pro model with `reasoning_effort=max`.
- This slice does not execute NetworkSearch through DeepSeek; source-backed search evidence is handled by the NetworkSearch adapter.
- The React inspector renders Chat API readiness, endpoint, and selected model names under Tool Backend Strategy.

DeepSeek Chat executor v1 is implemented:

- `DeepSeekChatCompletionTransport` allows tests and future adapters to inject the transport.
- `HttpDeepSeekChatCompletionTransport` posts JSON to the official Chat Completions endpoint with bearer auth.
- `execute_deepseek_chat_completion` validates `DEEPSEEK_API_KEY`, calls the transport, and redacts the key from returned errors.
- Tests use a fake transport only; no live DeepSeek API call is run in CI/local verification.
- NetworkSearch is not wired to DeepSeek chat execution; the confirmed evidence route is source-backed search with preserved URLs.

DeepSeek cache and telemetry v1 is implemented:

- `DeepSeekChatCompletionUsage`, `DeepSeekChatCacheStatus`, `DeepSeekChatTelemetry`, and `DeepSeekChatCompletionExecution` capture response usage, cache state, elapsed milliseconds, and a request hash.
- `DeepSeekChatCompletionCache` and `DeepSeekMemoryChatCompletionCache` provide an in-memory request cache for repeated DeepSeek Chat requests in the same desktop session.
- `execute_deepseek_chat_completion_with_cache` hashes the serialized request with SHA-256, records cache hit/miss telemetry, and avoids a second transport call for identical cached requests.
- `EventStore` appends and lists `deepseek_chat.telemetry_recorded` events.
- `run_operations_briefing` routes DeepSeek synthesis through the app cache and persists telemetry after the workflow run.
- The React Tool Backend Strategy inspector shows the latest DeepSeek telemetry: cache status, elapsed milliseconds, and total token count when available.
- The telemetry does not serialize prompt text or API keys.

DeepSeek cache controls v1 is implemented:

- `DeepSeekChatCacheState` reports the current in-session cache entry count.
- `DeepSeekMemoryChatCompletionCache::clear` removes in-memory cached responses and returns the removed-entry count.
- Tauri commands `get_deepseek_chat_cache_state` and `clear_deepseek_chat_cache` expose cache state and clearing to React.
- The React Tool Backend Strategy inspector shows cache entries and a clear-cache action; clearing responses does not delete telemetry events.

DeepSeek pricing config v1 is implemented:

- `deepseek-pricing.json` is stored under the OS app data directory, separate from the local directory setup file.
- `DeepSeekPricingSettings` stores manual USD / 1M token prices for Flash input, Flash output, Pro input, and Pro output.
- Price parsing converts decimal USD values into integer micro-USD values before cost math.
- `estimate_deepseek_chat_cost_micro_usd` returns a cost only when pricing is enabled, the telemetry model matches a configured rate, and prompt/completion token counts are present.
- `get_deepseek_pricing_state` and `save_deepseek_pricing_settings` expose the config to the desktop UI.
- The React workbench has a DeepSeek Pricing panel beside local setup, and the latest telemetry line shows formatted cost when `estimated_cost_micro_usd` is populated.
- This slice does not fetch live DeepSeek prices and does not hardcode default prices into the open-source project.

Operations Briefing DeepSeek synthesis v1 is implemented:

- `OperationsBriefingSynthesizer` is now an injectable workflow trait, with deterministic drafting kept as the default path.
- `run_operations_briefing_with_synthesizer` runs model synthesis only after evidence-folder ingestion succeeds.
- `DeepSeekOperationsBriefingSynthesizer` builds a DeepSeek Chat request from the bounded evidence manifest and parses strict JSON into summary, anomalies, action plan, and warnings.
- The Tauri `run_operations_briefing` command receives the selected large-model provider, model route, and thinking level from React.
- The command uses DeepSeek synthesis only when the selected provider is DeepSeek and the local process has a non-empty `DEEPSEEK_API_KEY`; otherwise it keeps the deterministic local draft.
- Model synthesis errors do not fail the workflow: the run falls back to the deterministic draft and records a warning.
- The API key value is not sent to the UI, serialized to events, or included in the model prompt.

Operations Briefing report export v1 is implemented:

- `render_operations_briefing_report` turns a stored Operations Briefing run into Markdown with traceable metadata, summary, anomalies, action plan, warnings, evidence folder, and evidence invocation ID.
- `DriveWriteRequest` now accepts either the existing `package_json` payload or a named `export_file` payload.
- `LocalDriveFolderClient::write_export_file` writes the named file inside the selected export folder and rejects file names that contain directories.
- `export_operations_briefing_report` finds a stored briefing run by ID, renders Markdown, and writes it through the same high-risk DriveWrite approval/audit path used by local export packages.
- The React workflow card now exposes an `Export report` action next to the existing work-package export action.
- Before first-run setup, report export falls back to the OS app data directory instead of a developer-machine path; after setup it uses the user's configured export directory.
- Sample evidence templates are committed under `docs/templates/operations-briefing-evidence/`.

Operations Briefing HTML report export v1 is implemented:

- `render_operations_briefing_html_report` turns a stored Operations Briefing run into standalone static HTML with traceable metadata, summary, anomalies, action plan, warnings, evidence folder, and evidence invocation ID.
- The HTML renderer escapes user and evidence text and does not embed scripts or external assets.
- `operations_briefing_html_report_file_name` emits `operations-briefing-<run-id>.html`.
- `export_operations_briefing_html_report` finds a stored briefing run by ID, renders HTML, and writes it through the same high-risk DriveWrite approval/audit path used by Markdown reports and local export packages.
- The React workflow card now exposes an `Export HTML` action next to the existing Markdown report export action.

Operations Briefing PDF report export v1 is implemented:

- `DriveWriteExportFile` now supports optional `content_base64`, so binary exports can use the same high-risk DriveWrite approval/audit path as text reports and work packages.
- `render_operations_briefing_pdf_report` turns a stored Operations Briefing run into static PDF bytes with report metadata, summary, anomalies, action plan, warnings, evidence folder, evidence invocation ID, pagination, and footer page numbers.
- `operations_briefing_pdf_report_file_name` emits `operations-briefing-<run-id>.pdf`.
- `export_operations_briefing_pdf_report` finds a stored briefing run by ID, renders PDF bytes, base64-encodes them for the DriveWrite request, and writes the decoded bytes locally after approval.
- The React workflow card now exposes an `Export PDF` action next to the Markdown and HTML report exports.
- Limit: the v1 PDF renderer uses built-in PDF core fonts and ASCII-safe text fallback; Markdown and HTML remain the full-fidelity Unicode exports until a bundled/open font strategy is chosen.

Operations Briefing sample evidence templates v1 is implemented:

- `docs/templates/operations-briefing-evidence/README.md` explains how to use the sample evidence set.
- `revenue.md`, `guest-experience.md`, `risk-and-compliance.md`, and `action-followups.md` provide text templates aligned to the first briefing workflow.
- Templates are repository docs only and do not encode any local user path or secret.

Operations Briefing evidence template seed v1 is implemented:

- `run_operations_briefing_template_seed` uses `CapabilityKind::FileWrite`, so `ask_on_risk` creates a pending approval and does not touch the filesystem until approved.
- `LocalOperationsBriefingTemplateSeeder` writes the committed sample templates into the target evidence folder and skips existing files instead of overwriting them.
- The installed app can seed templates because template content is compiled in with `include_str!`; it does not depend on a source checkout being present at runtime.
- `seed_operations_briefing_evidence_templates` chooses the user's configured evidence directory, or `<app_data>/operations-briefing-evidence` before first-run setup.
- The React Operations Briefing panel now has a `Seed templates` action that fills the evidence-folder input from the recorded invocation evidence ref.

NetworkSearch route status v1 is implemented:

- `apps/desktop/src-tauri/src/kernel/network_search.rs` exposes `NetworkSearchRouteStatus`.
- `get_network_search_route_status_for_model` returns a read-only status derived from the selected model strategy and current DeepSeek readiness.
- With no selected source model, the status records `execution_mode=permission_audit_only`, `evidence_policy=pending_user_confirmation`, and `network_requests_enabled=false`.
- With a selected free source model, the status records `execution_mode=source_backed_adapter`, `evidence_policy=source_links_required`, and `network_requests_enabled=true`.
- With a supported native bridge route, the status records `backend=native_large_model`, `execution_mode=native_bridge_contract`, `evidence_policy=source_links_required`, and `network_requests_enabled=true`.
- `requires_user_confirmation` is true only when the selected provider needs a separate source-backed NetworkSearch model and none has been selected.
- The React inspector renders the selected provider, native/source-model support, source model, route, execution mode, evidence policy, real-network status, DeepSeek orchestration readiness, and confirmation gate under Tool Backend Strategy.
- `search_network_boundary` preserves approval/audit behavior and executes either the native bridge route or the free source adapter only after policy allows.

Computer Use backend status v1 is implemented:

- `apps/desktop/src-tauri/src/kernel/computer_use.rs` exposes `ComputerUseBackendStatus`.
- `get_computer_use_backend_status_for_model` returns the selected Computer Use backend direction and availability flags without capturing pixels or controlling input during status checks.
- ChatGPT and Codex report `codex_bridge_screen_capture` and `codex_bridge_input_control`, with availability false until a bridge runtime is connected.
- Non-bridge providers report `local_windows_screen_capture` and `local_windows_input_control` on Windows, with a macOS path represented as `local_macos_screen_capture` and `local_macos_input_control`.
- Status now includes explicit screenshot/control permission requirement flags and notes. macOS local screen capture reports Screen Recording, macOS local control reports Accessibility, and Windows local backends report foreground desktop / secure desktop limitations.
- Status also includes `codex_bridge` readiness: whether the selected route requires a bridge runtime, whether `DEEPSEEK_AGENT_OS_CODEX_BRIDGE_TRANSPORT` has selected `http` or `stdio`, whether `DEEPSEEK_AGENT_OS_CODEX_BRIDGE_URL` is configured for HTTP routes, and whether the bridge is connected.
- For `http`, status posts `/health` to the loopback bridge endpoint and marks screenshot/control available only when the bridge reports the required capabilities.
- `control_requires_approval=true` keeps real mouse/keyboard execution visibly approval-gated.
- The React inspector renders screen/control backend status and OS permission notes under Tool Backend Strategy in Chinese and English.

Codex bridge contract schema v1 is implemented:

- `apps/desktop/src-tauri/src/kernel/codex_bridge_contract.rs` defines transport-neutral serde structs for health, screenshot, and control messages.
- Contract version is `deepseek-agent-os.codex-bridge.v1`.
- Capability names serialize as `computer_screenshot` and `computer_control`.
- Screenshot responses carry display label, dimensions, capture timestamp, and `png_base64`; they intentionally do not carry local evidence paths such as `computer-screenshots/...`.
- Control requests still require structured action strings such as `click:120,340` and reject natural-language actions.

Codex bridge HTTP runtime v1 is implemented:

- `apps/desktop/src-tauri/src/kernel/codex_bridge_http.rs` implements the loopback-only HTTP bridge transport.
- `DEEPSEEK_AGENT_OS_CODEX_BRIDGE_TRANSPORT=http` plus `DEEPSEEK_AGENT_OS_CODEX_BRIDGE_URL=http://127.0.0.1:<port>` enables HTTP bridge execution for ChatGPT/Codex Computer Use routes.
- The HTTP client posts `/health`, `/screenshot`, and `/control` using contract version `deepseek-agent-os.codex-bridge.v1`.
- Bridge screenshot responses return `png_base64`; the desktop client decodes it and writes a local `computer-screenshots/...` evidence file under the user-selected evidence directory.
- Bridge control requests now include the user-approved target plus the structured action string, for example `click:120,340,left`.
- Remote HTTP endpoints are rejected; only loopback hosts such as `127.0.0.1` and `localhost` are accepted.
- `stdio` remains represented in status/options as a future sidecar transport, but it is not executable yet.

Work-package tool readiness v1 is implemented:

- Exported work packages now include a serde-defaulted `tool_readiness` snapshot.
- The snapshot includes `DeepSeekCredentialStatus`, `NetworkSearchRouteStatus`, `ComputerUseBackendStatus`, local-directory readiness, and `ModelDrivenToolStrategy`.
- Manual work-package export and DriveWrite local export-package both use the same readiness builder.
- `DEEPSEEK_API_KEY` may appear only as the environment variable name; the API key value is not serialized.
- Local directory readiness reports setup booleans for workspace/evidence/export directories and keeps actual user paths redacted.
- This readiness export does not call the live DeepSeek API, capture screen pixels during export, control mouse/keyboard input during export, or serialize user machine paths.
- Legacy work packages without `tool_readiness` still parse with default not-ready strategy/status.

Operations Briefing workflow skeleton v1 is implemented:

- New Rust workflow runtime lives in `apps/desktop/src-tauri/src/kernel/workflow.rs`.
- `run_operations_briefing` calls evidence-folder ingestion through `FileRead`, preserves the evidence invocation ID, and returns an `OperationsBriefingRun`.
- Workflow run statuses are `pending_approval`, `draft_ready`, and `failed`.
- Successful runs create a deterministic draft scaffold: summary, anomaly leads, action plan, warnings, evidence folder path, and workflow ID `operations.briefing.v1`.
- Runs are persisted as append-only `operations_briefing.run_recorded` events and exposed through `list_operations_briefing_runs`.
- The React workbench now has an Operations Briefing Workflow panel that accepts an evidence folder path and displays the latest run.
- Exported work packages now include `operations_briefing_runs` with serde-defaulted legacy compatibility.
- Imported Operations Briefing runs are appended as read-only archived runs with `archived_from_package=true`; duplicate run IDs are skipped.
- The latest run card has an export action that fills the existing work-package JSON output.

Memory Candidate review v1 is implemented:

- New memory candidate model types live in `apps/desktop/src-tauri/src/kernel/models.rs`: `MemoryCandidate`, `MemoryCandidateSource`, `MemoryCandidateStatus`, `MemoryCandidateResolution`, and `MemoryCandidateRecord`.
- New append-only event types are `memory_candidate.proposed` and `memory_candidate.resolved`.
- `resolve_memory_candidate(..., accepted=true)` appends a normal `MemoryRecord` with source `memory_candidate`; rejection does not write long-term memory.
- Tauri commands: `propose_memory_candidate`, `list_memory_candidate_records`, and `resolve_memory_candidate`.
- The React Memory panel now has a Memory Candidates review section with propose/accept/reject controls.

Memory Studio metadata v1 is implemented:

- `MemoryCandidate` and `MemoryRecord` now carry `memory_type`, `scope`, `sensitivity`, and `lifecycle`.
- Metadata enums live in `apps/desktop/src-tauri/src/kernel/models.rs`: `MemoryType`, `MemoryScope`, `MemorySensitivity`, and `MemoryLifecycle`.
- Serde defaults keep older candidate and memory event payloads readable: manual candidates default to `preference`, older long-term memories default to `project_context`, and scope/sensitivity/lifecycle default to `workspace`/`normal`/`active`.
- `propose_memory_candidate` accepts explicit metadata from the UI, and accepted candidates preserve those tags when written as long-term `MemoryRecord`s.
- The React Memory panel now has compact metadata selectors and renders metadata badges on candidate and long-term memory rows.
- Limit: conflict resolution and merge actions are still next.

Memory Studio conflict surfacing v1 is implemented:

- `MemoryCandidateRecord` now includes `conflicting_memory_ids` with a serde default for older payload compatibility.
- `EventStore::list_memory_candidate_records` computes conflicts against visible long-term memories only, so deleted memories are ignored.
- Conflict detection treats matching normalized titles as overlap, and also treats same type/scope with exact or clear body containment as overlap.
- Memories created from the same accepted candidate are ignored so accepted candidates do not conflict with themselves.
- The React Memory Candidates list shows a compact overlap warning without blocking accept/reject actions.

Memory Studio conflict details v1 is implemented:

- `MemoryConflictSummary` exposes a compact title/body/metadata/update-time snapshot for each visible long-term memory that overlaps a candidate.
- `MemoryCandidateRecord` keeps `conflicting_memory_ids` for compatibility and adds serde-defaulted `conflicting_memories` for richer review.
- `EventStore::list_memory_candidate_records` populates conflict summaries from the same visible-memory set used by the existing conflict detector.
- The React Memory Candidates list now expands overlapping memory details under the conflict warning, while leaving accept/reject semantics unchanged.
- Limit: this slice does not add merge or replace actions.

Memory Studio conflict link v1 is implemented:

- `MemoryRecordLink` and append-only event type `memory_record.linked` represent explicit relationships between long-term memories.
- `MemoryRecord` now projects serde-defaulted `linked_memory_ids` and compact `linked_memories` summaries when listed.
- `EventStore::link_memory_candidate_to_conflicts` accepts a pending candidate, writes it as a separate long-term memory, and links it to selected visible overlapping memories without rewriting or deleting either side.
- Tauri command `link_memory_candidate_to_conflicts` exposes the link path to the desktop UI.
- The React Memory Candidates list now shows `Link and accept` for candidates with overlaps, and long-term memory rows show related memory summaries.
- Limit: merge and replace conflict actions remain a product decision.

Memory Studio merge preview v1 is implemented:

- `MemoryCandidateMergePreview` carries a pure-read merged draft for one pending candidate and selected visible long-term memories.
- `EventStore::preview_memory_candidate_merge` builds the draft body from selected memory bodies plus the candidate body, removes duplicate bodies, and preserves candidate metadata.
- Previewing a merge does not append resolution, memory, update, delete, or link events; the candidate remains pending.
- Tauri command `preview_memory_candidate_merge` exposes the preview path.
- The React Memory Candidates list now shows `Preview merge` for candidates with overlaps and renders the draft inline.
- Limit: saving a merged draft and replacing existing memories still require product confirmation.

Memory Studio replace preview v1 is implemented:

- `MemoryCandidateReplacePreview` carries a pure-read replacement draft for one pending candidate and selected visible long-term target memories.
- `EventStore::preview_memory_candidate_replace` returns the candidate title/body/metadata as the replacement draft and target memory summaries as the would-replace list.
- Previewing a replacement does not append resolution, memory, update, delete, or link events; the candidate remains pending.
- Tauri command `preview_memory_candidate_replace` exposes the preview path.
- The React Memory Candidates list now shows `Preview replace` for candidates with overlaps and renders target memories inline.
- Limit: confirming replacement and tombstoning old memories still require product confirmation.

Memory Studio expiration v1 is implemented:

- `MemoryCandidate`, `MemoryRecord`, and `MemoryRecordUpdate` now carry optional `expires_at` timestamps.
- New candidate/update writes require `expires_at` when lifecycle is `expires`; legacy events without `expires_at` remain readable.
- Accepted future-expiring candidates preserve their expiration timestamp on the long-term memory record.
- `EventStore::list_memory_records_at` and `search_memory_records_at` provide deterministic expiration filtering for tests; normal list/search use current UTC time.
- Expired memories are hidden from normal list/search without deleting or rewriting original events.
- The React Memory panel shows date inputs when candidate/edit lifecycle is `expires` and renders expiration badges for unexpired timed memories.

Memory Studio edit v1 is implemented:

- `MemoryRecordUpdate` and append-only event type `memory_record.updated` represent long-term memory revisions.
- `EventStore::update_memory_record` appends an update event and returns `NotFound` for missing or deleted memory IDs.
- `list_memory_records` and `search_memory_records` merge the latest update into the visible memory version while preserving original `id`, `source`, `source_id`, and `created_at`.
- Tauri command `update_memory_record` exposes the edit path.
- The React Memory panel now has edit/save/cancel actions for long-term memory rows and refreshes list/search results after saving.

Memory Studio delete v1 is implemented:

- `MemoryRecordDeletion` and append-only event type `memory_record.deleted` represent long-term memory deletion tombstones.
- `EventStore::delete_memory_record` appends a deletion event and returns `NotFound` for missing or already-deleted memory IDs.
- `list_memory_records` and `search_memory_records` filter deleted memory IDs without rewriting or deleting the original `memory_record.created` event.
- Tauri command `delete_memory_record` exposes the deletion path.
- The React Memory panel now has a delete action on long-term memory rows and refreshes list/search results after deletion.

Work-package import preview v1 is implemented:

- New preview structs live in `apps/desktop/src-tauri/src/kernel/work_package.rs`: `WorkPackageImportPreview`, `WorkPackageTaskImportPreview`, `WorkPackageMemoryCandidateImportPreview`, `WorkPackageOperationsBriefingImportPreview`, and `WorkPackageWorkflowTemplateImportPreview`.
- `EventStore::preview_work_package_import` computes total/new/skipped task records and memory candidates without appending any events.
- The preview reports memory candidate count with `review_supported=true`; candidates import into local review, not long-term memory.
- The preview reports archived Operations Briefing run count with `replay_supported=true`.
- Tauri command `preview_work_package_import` parses the same package JSON and returns the read-only preview.
- The React import area now has a Preview button and compact preview summary before Import.

Workflow template package import v1 is implemented:

- `WorkflowTemplatePackage` and `WorkflowTemplateFile` live in `apps/desktop/src-tauri/src/kernel/workflow.rs`.
- Exported work packages now include the compiled Operations Briefing evidence template package in `workflow_templates`.
- Legacy work packages without `workflow_templates` parse with an empty list.
- Append-only event type `workflow_template_package.imported` registers imported template packages locally without writing user folders.
- `EventStore::preview_work_package_import` reports workflow template package total/new/skipped counts.
- `EventStore::import_workflow_template_packages` imports new template package IDs and skips existing IDs.
- `import_work_package` now returns workflow-template import/skip counts, and the React import preview/result copy shows those counts.

Imported memory candidate review v1 is implemented:

- Work packages now include serde-defaulted `memory_candidates` for legacy compatibility.
- `export_work_package` includes local memory candidate proposals in handoff packages.
- `EventStore::import_memory_candidates` imports new candidate IDs, skips existing IDs, forces imported candidates to `MemoryCandidateSource::Import`, and leaves them pending.
- Candidate import does not call `append_memory_record`; long-term memory is written only after local acceptance through Memory Studio.
- `import_work_package` now returns task, memory candidate, and briefing-run import/skip counts.
- Package import refreshes Memory Studio candidate records after import.

Operations Briefing run archive replay v1 is implemented:

- `OperationsBriefingRun` now has serde-defaulted `archived_from_package`; legacy run events default to local runs.
- `EventStore::import_operations_briefing_runs` imports new run IDs, skips existing IDs, and marks imported runs as archived.
- `import_work_package` now imports task records, memory candidates, and archived briefing runs, returning all import/skip counts.
- The React run card shows a compact archive badge for imported runs, and package import refreshes the run list.
- Limit: archive replay is read-only; it does not rerun evidence tools or synthesize a new briefing.

## Must Read First in a New Conversation

1. `PROJECT_CONTEXT.md`
2. `DECISIONS.md`
3. `docs/superpowers/specs/2026-06-28-deepseek-agent-os-architecture-design.md`
4. `SESSION_HANDOFF.md`

## Reference Sources Already Pulled Locally

All under `D:\deepseek UI\_reference_repos`:

- `codex` from `openai/codex`
- `CodeWhale` from `Hmbown/CodeWhale`
- `openclaw` from `openclaw/openclaw`
- `hermes-agent-desktop` from `Felix-Forever/hermes-agent-desktop`
- `learn-claude-code` from `shareAI-lab/learn-claude-code`

Local ECC reference found at:

- `D:\Codex\Codex\2026-05-31\affaan-m-ecc-https-github-com\work\ECC`

Do not use leaked Claude Code source repositories.

## CodeGraph Evidence

CodeGraph version: `1.1.1`.

Indexed projects:

- Codex: 3,268 files, 105,439 nodes, 375,872 edges.
- CodeWhale: 616 files, 27,992 nodes, 101,952 edges.
- OpenClaw: 18,315 files, 339,278 nodes, 1,582,280 edges.
- learn-claude-code: 108 files, 2,693 nodes, 6,015 edges.
- Hermes Agent Desktop: only 1 indexed source file, useful mainly as a lightweight UI/reference artifact.

## Foundation MVP Commands

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'deepseek_ui_cargo_target'
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
npx pnpm@9.15.9 dev
```

Set `CARGO_TARGET_DIR` before Rust/Tauri verification on Windows to avoid the local MinGW `dlltool` path-with-space issue under `D:\deepseek UI`.

On Windows, `apps/desktop/src-tauri/tauri.windows.conf.json` is merged automatically
and builds an NSIS installer at `debug/bundle/nsis/DeepSeek Agent OS_0.1.0_x64-setup.exe`
under the configured Cargo target directory.

On macOS, `apps/desktop/src-tauri/tauri.macos.conf.json` enables `.app` and `.dmg`
packaging. This config is committed but still needs verification on a macOS host.

Open-source installation guide v1 is implemented:

- `docs/INSTALLATION.md` explains Windows installer usage, source builds, first-run workspace/evidence/export directory setup, `DEEPSEEK_API_KEY`, manual DeepSeek pricing, NetworkSearch routing, Computer Use permissions, deferred email connectors, local Drive behavior, and troubleshooting.
- `README.md` now links to the installation guide in the "Read first" section.
- The guide explicitly separates installer program files from user data, app data, local workspace paths, evidence folders, and export folders.
- The guide does not claim macOS packaging verification and does not decide the unresolved PDF font or Codex bridge sidecar questions.

## Latest Verification

2026-06-29 FileWrite metadata legacy compatibility v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml file_write_result_legacy_json_defaults_utf8_encoding -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused FileWrite legacy metadata test passed; Rust tests passed with 214 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Legacy FileWrite result JSON without `encoding` now has regression coverage confirming it deserializes with the `utf-8` default.

2026-06-29 TerminalRead exit-code coverage v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml terminal_read_records_failed_invocation_for_nonzero_exit_code -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused TerminalRead exit-code test passed; Rust tests passed with 213 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. TerminalRead commands that start but exit nonzero are now explicitly covered as failed invocations with stderr in the excerpt and exit code warnings.

2026-06-29 DriveWrite output name v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml drive_write_local_export_package_writes_json_after_policy_allows -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused DriveWrite output-name test passed; Rust tests passed with 212 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Successful DriveWrite exports now include the local output file name in the inspector excerpt while the full output path remains in `evidence_ref`.

2026-06-29 FileWrite metadata v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml file_write_boundary_writes_workspace_file_after_policy_allows -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused FileWrite metadata test passed; Rust tests passed with 212 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Successful local FileWrite invocations now record UTF-8 encoding and byte-count metadata in the structured result and inspector excerpt while binary writes and outside-workspace paths remain out of scope.

2026-06-29 text evidence metadata legacy compatibility v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml legacy_json_defaults_utf8_encoding -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused legacy metadata tests passed with 2 tests; Rust tests passed with 212 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Legacy DriveRead and evidence-folder JSON payloads without `encoding` now have explicit regression coverage confirming they deserialize with the `utf-8` default.

2026-06-29 EvidenceFolder metadata v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml evidence_folder_ingest_returns_manifest_tool_result -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused EvidenceFolder metadata test passed; Rust tests passed with 210 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Successful evidence-folder ingests now record UTF-8 encoding and byte-count metadata for accepted local text evidence in the structured result and inspector excerpt while nested folders and binary/PDF/Office parsing remain deferred.

2026-06-29 DriveRead metadata v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml drive_read_local_folder_returns_matching_manifest_after_policy_allows -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused DriveRead metadata test passed; Rust tests passed with 210 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Successful local DriveRead entries now record UTF-8 encoding and byte-count metadata in the structured result and inspector excerpt while cloud drive connectors and binary/PDF/Office parsing remain deferred.

2026-06-29 FileRead metadata v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml file_read_returns_structured_tool_result -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused FileRead metadata test passed; Rust tests passed with 210 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Successful UTF-8 FileRead invocations now record encoding and byte-count metadata in the inspector warnings while PDF/Office parsing remains unimplemented.

2026-06-29 Memory Studio replace preview v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml previewing_memory_candidate_replace_does_not_write_events -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused Memory Studio replace-preview test passed; Rust tests passed with 210 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Pending memory candidates with overlaps can now show a pure-read replacement draft and target-memory list without resolving the candidate, deleting old memories, or writing memory/link events.

2026-06-29 Memory Studio merge preview v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml previewing_memory_candidate_merge_does_not_write_events -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused Memory Studio merge-preview test passed; Rust tests passed with 209 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Pending memory candidates with overlaps can now show a pure-read merge draft without resolving the candidate or writing memory/link events.

2026-06-29 Memory Studio conflict link v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml linking_memory_candidate_accepts_candidate_and_keeps_related_memories -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused Memory Studio link test passed; Rust tests passed with 208 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Pending memory candidates with overlaps can now be accepted as separate long-term memories while recording append-only links to the overlapping memories.

2026-06-29 DeepSeek cache controls v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 207 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. DeepSeek session cache size is now visible in the runtime inspector and cached responses can be cleared without deleting telemetry events.

2026-06-29 open-source installation guide v1:

```powershell
git diff --check
```

Result: documentation diff check passed with only LF-to-CRLF warnings. GitHub users now have a first-run guide for installing/building the app, selecting local directories on their own machine, configuring `DEEPSEEK_API_KEY`, and using local DeepSeek pricing without hardcoded maintainer paths.

2026-06-29 Memory Studio conflict details v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 206 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Memory Studio candidate conflicts now include inspectable overlapping memory summaries in the UI while leaving merge/replace actions as a separate product decision.

2026-06-29 DeepSeek pricing config v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 206 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. DeepSeek cost telemetry can now be populated from a local manual USD / 1M token pricing table stored under OS app data, without hardcoded public DeepSeek prices.

2026-06-29 DeepSeek cache and telemetry v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 200 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Operations Briefing DeepSeek synthesis now uses an in-session request cache and records secret-safe DeepSeek telemetry with request hash, model, cache status, elapsed milliseconds, and token counts when returned.

2026-06-29 Workflow template package import v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 197 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Work packages now carry workflow template packages, import preview shows template counts, and importing registers new template packages locally without writing user folders.

2026-06-29 Memory Studio expiration v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 194 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Memory Studio can now set expiration dates on candidates and edited memories; expired long-term memories are hidden from normal list/search without rewriting original events.

2026-06-29 Memory Studio conflict surfacing v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 192 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Pending Memory Studio candidates now show likely overlap with visible long-term memories while leaving accept/reject under operator control.

2026-06-29 Memory Studio edit v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 190 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Long-term Memory Studio rows can now be edited through append-only `memory_record.updated` events; list/search show the latest visible version while deletion tombstones remain authoritative.

2026-06-29 Memory Studio delete v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 188 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Long-term Memory Studio rows can now be deleted through an append-only tombstone event and are hidden from list/search without rewriting original memory events.

2026-06-29 Native NetworkSearch bridge contract v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 186 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. NetworkSearch can now use a native large-model bridge contract for supported providers when the local Codex bridge HTTP runtime is configured, while preserving source-link evidence and the approval/audit boundary.

2026-06-29 Operations Briefing PDF report export v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 180 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Operations Briefing runs can now be rendered to PDF bytes and exported through the DriveWrite approval loop. The v1 PDF path is intentionally lightweight and ASCII-safe; full Chinese/CJK PDF rendering needs a confirmed font strategy.

2026-06-29 Operations Briefing HTML report export v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 178 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Operations Briefing runs can now be rendered to static escaped HTML and exported through the DriveWrite approval loop.

2026-06-29 Operations Briefing template seed v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 176 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. The Operations Briefing panel can now seed compiled sample evidence templates into the user's configured evidence folder through the FileWrite approval loop, skipping existing files instead of overwriting them.

2026-06-29 Operations Briefing report export v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 172 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Operations Briefing runs can now be rendered to Markdown and exported through the DriveWrite approval loop, and sample evidence templates are committed under `docs/templates/operations-briefing-evidence/`.

2026-06-29 Operations Briefing DeepSeek synthesis v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 168 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Operations Briefing now uses DeepSeek Chat model synthesis when the selected provider is DeepSeek and `DEEPSEEK_API_KEY` is available, while preserving deterministic fallback and permission-gated evidence ingestion.

2026-06-29 Local directory readiness export v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 163 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Exported work packages now include redacted local-directory readiness without serializing user machine paths.

2026-06-29 FileWrite local workspace v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml file_write_boundary -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused FileWrite tests passed with 4 tests; full Rust tests passed with 162 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. FileWrite now writes approved UTF-8 content inside the configured local workspace and rejects outside-workspace paths.

2026-06-29 Codex bridge HTTP runtime v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 161 tests and no Rust warnings; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer at `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. HTTP bridge health, screenshot, and control routes are wired for loopback endpoints; `stdio` remains future work.

2026-06-29 Codex bridge contract schema v1:

```powershell
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 157 tests; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer without Rust warnings; `git diff --check` passed with only LF-to-CRLF warnings. Shared bridge JSON schema exists, but no HTTP or stdio transport is wired yet.

2026-06-29 Codex bridge transport selection gate:

```powershell
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 153 tests; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer without Rust warnings; `git diff --check` passed with only LF-to-CRLF warnings. Real bridge transport selection still needs product confirmation: HTTP local service vs stdio sidecar.

2026-06-29 Codex bridge runtime readiness v1:

```powershell
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 151 tests; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer; `git diff --check` passed with only LF-to-CRLF warnings. Bridge health checks and execution transport are still not implemented.

2026-06-29 Computer Use OS permission readiness v1:

```powershell
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 148 tests; desktop TypeScript/Vite build passed; Tauri debug build produced the Windows NSIS installer; `git diff --check` passed with only LF-to-CRLF warnings. The committed macOS `.app`/`.dmg` config still needs verification on a macOS host.

2026-06-29 model-driven tool strategy slice:

```powershell
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
git diff --check
```

Result: Rust tests passed with 122 tests; desktop TypeScript/Vite build passed; `git diff --check` passed with only existing LF-to-CRLF warnings.

2026-06-29 setup folder picker and Tauri packaging verification:

```powershell
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: Rust tests passed with 125 tests; desktop TypeScript/Vite build passed; Tauri debug build produced `D:\codex-target\deepseek-ui-tauri\debug\deepseek-agent-os-desktop.exe` and the NSIS installer `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings.

2026-06-29 NetworkSearch source adapter v1:

```powershell
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml network_search
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused NetworkSearch tests passed with 11 tests; full Rust tests passed with 128 tests; desktop TypeScript/Vite build passed; Tauri debug build produced the NSIS installer; `git diff --check` passed with only LF-to-CRLF warnings.

2026-06-29 local-path portability polish:

- Runtime UI placeholders no longer include developer-machine examples such as `D:\evidence`.
- File, folder, evidence-folder, Drive local-folder, and export-package inputs now read as user-local runtime paths.
- Rust tests use `fixtures/evidence` style sample paths instead of developer-machine drive paths.
- README states that local paths are runtime user inputs on the user's own machine.

2026-06-29 setup and local directory contract v1:

- `apps/desktop/src-tauri/src/kernel/local_directory.rs` stores local directory settings in `local-directories.json` under the OS app data directory.
- Settings include default workspace, evidence folder, and export folder.
- Missing settings produce `needs_setup=true`, which the React workbench shows as a first-run setup panel.
- The setup panel uses the Tauri dialog plugin to provide native folder picker buttons for each local directory field.
- Saving settings hydrates Operations Briefing/evidence folder defaults from the evidence directory, DriveRead from the workspace, and DriveWrite from the export directory.
- Installation directory is intentionally not treated as workspace or user-data storage.

2026-06-29 Windows installer baseline v1:

- `apps/desktop/src-tauri/tauri.windows.conf.json` enables Tauri bundling on Windows with `targets=["nsis"]`.
- Normal `tauri build --debug` now produces a Windows NSIS setup exe when `CARGO_TARGET_DIR` is set to a path without spaces.
- Windows installer scope is intentionally separated from first-run workspace/evidence/export directory selection.
- `apps/desktop/src-tauri/tauri.macos.conf.json` now enables future macOS `.app` and `.dmg` packaging; it still needs a macOS runner verification pass.

2026-06-29 NetworkSearch provider label v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml network_search_boundary_runs_source_client_after_policy_allows -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused NetworkSearch provider-label and blank-provider fallback tests passed; full Rust tests passed with 215 tests; desktop TypeScript/Vite build passed; Tauri debug build produced `D:\codex-target\deepseek-ui-tauri\debug\deepseek-agent-os-desktop.exe` and `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Successful NetworkSearch invocations now include the provider/source-adapter label in the title while preserving source URL evidence, with `source-backed search` as the blank-provider fallback.

2026-06-29 NetworkSearch source option transparency v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml reserved_free_network_search_options_disclose_shared_alpha_adapter -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused free-source option disclosure test passed; full Rust tests passed with 216 tests; desktop TypeScript/Vite build passed; Tauri debug build produced `D:\codex-target\deepseek-ui-tauri\debug\deepseek-agent-os-desktop.exe` and `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. Reserved free NetworkSearch options now disclose through backend option metadata, React fallback metadata, visible Chinese/English labels, and the source-model prompt that they are alpha presets sharing the source-backed HTTP adapter until separate local-browser or aggregator implementations are confirmed; public install docs use user-machine-safe target-dir examples.

2026-06-29 Phase 2 confirmation closeout v1:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml memory_candidate_accepts -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml codex_bridge_status -- --nocapture
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Result: focused Memory Studio merge/replace tests passed; focused Codex bridge runtime status tests passed; full Rust tests passed with 218 tests; desktop TypeScript/Vite build passed; Tauri debug build produced `D:\codex-target\deepseek-ui-tauri\debug\deepseek-agent-os-desktop.exe` and `D:\codex-target\deepseek-ui-tauri\debug\bundle\nsis\DeepSeek Agent OS_0.1.0_x64-setup.exe`; `git diff --check` passed with only LF-to-CRLF warnings. PDF export strategy is now explicitly ASCII-safe in MVP while Markdown/HTML remain Unicode-safe. Codex bridge runtime is now decided as an external loopback HTTP service for MVP; stdio sidecar spawning is deferred and no longer shown as a current runtime option. Memory Studio now has confirmed merge-and-accept and replace-and-accept actions that preserve append-only audit events while tombstoning superseded memories.

2026-06-29 Open-source release hygiene v1:

- User confirmed the project should stop feature development before the first GitHub open-source release.
- Added a v0.1-alpha feature-freeze decision to `DECISIONS.md`.
- Added `CONTRIBUTING.md`, `SECURITY.md`, `docs/OPEN_SOURCE_RELEASE.md`, a pull request template, and focused issue templates for bugs and DeepSeek compatibility.
- README and project context now say the project is independent and not an official DeepSeek repository.
- User confirmed Apache-2.0, the GitHub repository name `deepseek-agent-os`, and a source-only first alpha.
- Added Apache-2.0 license metadata and source-only release policy docs.

2026-06-29 Public GitHub release v0.1-alpha:

- Public repository is `https://github.com/Lee-take/deepseek-agent-os` under the `Lee-take` account.
- Default branch is `main`.
- Public release `v0.1-alpha` is a prerelease with no binary assets attached.
- Release policy remains source-only for the first alpha.
- Final GitHub CI for `main` completed successfully on commit `8b5f377`.

2026-06-29 Local DeepSeek smoke test v1:

- User confirmed the DeepSeek API key is for local `D:\deepseek UI` project testing only.
- Store and use the key only through local environment variables such as `DEEPSEEK_API_KEY`; do not write the key into source, docs, `.env`, logs, commits, GitHub Actions, or release assets.
- Added `scripts/deepseek-smoke.mjs` and `pnpm test:deepseek` to run a local Chat Completions smoke test that prints only secret-safe metadata.
- Added `pnpm test` as the local desktop verification command for frontend build plus Rust tests.

2026-06-29 Local DeepSeek Operations Briefing smoke test v1:

- Added `scripts/deepseek-operations-briefing-smoke.mjs` and `pnpm test:deepseek:briefing`.
- The script reads `DEEPSEEK_API_KEY` from the local environment, sends the Operations Briefing sample evidence manifest to DeepSeek, validates the returned JSON contract, and prints only secret-safe counts/token metadata by default.
- Absolute local evidence directory paths are redacted from the model prompt and script output.
- The workflow smoke test is local-only and is not run in GitHub CI.
- `DEEPSEEK_BRIEFING_EVIDENCE_DIR` can point at another local evidence folder for maintainer testing, but private evidence must not be committed or uploaded.

Verification:

```powershell
npx pnpm@9.15.9 test
npx pnpm@9.15.9 test:deepseek
npx pnpm@9.15.9 test:deepseek:briefing
$env:DEEPSEEK_BRIEFING_EVIDENCE_DIR=(Resolve-Path 'docs/templates/operations-briefing-evidence').Path; npx pnpm@9.15.9 test:deepseek:briefing; Remove-Item Env:DEEPSEEK_BRIEFING_EVIDENCE_DIR
rg -n --pcre2 '(?<![A-Za-z0-9])sk-[A-Za-z0-9]{16,}(?![A-Za-z0-9])' . -g '!node_modules' -g '!target' -g '!dist' -g '!src-tauri/target'
rg -n 'DEEPSEEK_API_KEY\s*=\s*["''][^"'']+["'']' . -g '!node_modules' -g '!target' -g '!dist' -g '!src-tauri/target'
git diff --check
```

Result: desktop build and all 218 Rust tests passed; both DeepSeek live smoke tests returned `ok=true`; absolute evidence path output was redacted as `[local evidence directory]`; both secret scans had no matches; `git diff --check` passed with only LF-to-CRLF warnings.

## Confirmed Architecture Direction

- Build Agent OS Kernel plus Workflow Packs.
- Use Tauri + React + TypeScript + Rust sidecar.
- Use local-first desktop architecture.
- Team collaboration data model exists from day one; no cloud sync in MVP.
- First version supports email, drive, browser, and Computer Use through permissioned capabilities.
- Current provider direction: NetworkSearch reports the selected large model's search capability, uses the native bridge contract for supported providers when the external loopback HTTP Codex bridge runtime is configured, and otherwise requires a separate NetworkSearch source model. Email remains architecture-only for this version. Drive uses local folders/export packages. Computer Screenshot and ComputerControl use the Codex bridge contract when the selected provider is ChatGPT or Codex, otherwise local Windows/macOS libraries.
- Use granular permission controls similar to Codex access dropdown, but more understandable for office users.
- Add thinking/model controls in the main composer.
- Use automatic memory with Memory Studio, source traceability, scopes, lifecycle, explicit conflict actions, and import/export.
- Use DeepSeek-first optimization layer with Pro/Flash/Auto routing, thinking control, in-session request caching, latency/token telemetry, and local manual pricing-table cost estimates.
- Use the Operations Briefing workflow as the first Operations Management Pack flow.
- Keep Operations Briefing PDF export lightweight and ASCII-safe in MVP; Markdown and HTML are the full Unicode report formats.
- Workflow template packages import into local registration; read-only run archive replay is available for Operations Briefing runs, and memory candidates import into local review.
- Treat imported memories as reviewable candidates, not automatic writes.
- Put Computer Use behind an experimental high-risk flag in MVP.
- Local short-window unlock is implemented for ComputerControl; a stronger local agent token or OS credential prompt remains future hardening.
- v0.1-alpha is feature-frozen for GitHub open-source release preparation; do not add new capabilities before release.
- Public release policy is Apache-2.0 and source-only for the first alpha.

## Next Actions

1. Keep implementing within the confirmed DeepSeek-first scope; avoid broad new product lanes unless the maintainer confirms them.
2. Strengthen test coverage and local verification first, especially live DeepSeek API and Operations Briefing paths that must remain secret-safe.
3. Continue improving the existing desktop Agent OS workflows, permissions, memory, and Operations Briefing pack without uploading local secrets.
4. Before public pushes, run local verification and secret scans.

## Open Questions

- Public repository owner and security contact channel can be filled from the authenticated GitHub account unless the maintainer gives a dedicated contact.
- The maintainer has approved storing the DeepSeek test API key locally for this project. It must stay local-only and must not be uploaded.
