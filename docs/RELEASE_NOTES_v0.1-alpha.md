# DeepSeek Agent OS v0.1-alpha Release Notes

Status: first public source-only alpha.

## Positioning

DeepSeek Agent OS is an independent local-first desktop Agent OS optimized for
DeepSeek. It is not an official DeepSeek product and is not affiliated with
DeepSeek.

The v0.1-alpha release is feature-frozen. The goal is a credible open-source
baseline for DeepSeek-first desktop agent support, not a broad feature launch.

## What Is Included

- Tauri + React + TypeScript + Rust desktop shell.
- Local SQLite append-only event store.
- DeepSeek-first model route defaults, credential readiness, in-session cache
  telemetry, and manual local pricing configuration.
- Capability permission loop for file, network, browser, email, drive, terminal,
  and Computer Use surfaces.
- One-shot approvals for high-consequence actions such as EmailSend and
  ComputerControl.
- Source-backed NetworkSearch adapter with preserved source URLs.
- External loopback HTTP Codex bridge contract for ChatGPT/Codex routed
  Computer Use and native NetworkSearch paths.
- Local Windows/macOS Computer Screenshot and ComputerControl backend routing,
  with explicit approval and unlock boundaries.
- Memory Studio candidate review, edit/delete/expiration, and explicit
  conflict actions: link, merge, and replace.
- Operations Briefing workflow pack using local evidence folders and optional
  DeepSeek synthesis when `DEEPSEEK_API_KEY` is configured.
- Work package export/import preview, workflow template import, and read-only
  Operations Briefing archive replay.
- Windows debug NSIS build path and source build instructions.

## Important Alpha Limits

- EmailRead, EmailDraft, and EmailSend are approval/audit boundaries only; they
  do not connect to a mailbox.
- DriveRead and DriveWrite use local folders and export packages; they do not
  connect to cloud-drive accounts.
- BrowserSubmit records approval/audit boundaries only; it does not submit web
  forms.
- TerminalWrite records approval/audit boundaries only; it does not execute
  mutating shell commands.
- PDF export is lightweight and ASCII-safe. Use Markdown or HTML for full
  Unicode and Chinese report content.
- The Codex bridge runtime is external loopback HTTP only in v0.1-alpha.
  Managed sidecar spawning is deferred.
- NetworkSearch must preserve source links. Plain DeepSeek or ChatGPT text is
  not treated as verified web evidence.
- No hosted sync, account system, marketplace, or arbitrary third-party
  executable plugins are included.

## Verification Before Release

Run on the release branch:

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
$env:CARGO_TARGET_DIR='D:\codex-target\deepseek-ui-tauri'; npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Optional live DeepSeek smoke test:

```powershell
$env:DEEPSEEK_API_KEY = Read-Host "DeepSeek API key"
npx pnpm@9.15.9 test:deepseek
npx pnpm@9.15.9 test:deepseek:briefing
```

Do not commit API keys, `.env` files, local app data, local evidence folders, or
generated installer artifacts.
