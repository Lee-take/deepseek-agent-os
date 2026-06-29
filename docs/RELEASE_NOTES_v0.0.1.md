# DeepSeek Agent OS v0.0.1 Release Notes

Status: early Windows-first open-source preview.

## Positioning

DeepSeek Agent OS, or DS Agent OS, is an independent open-source desktop project
written to help colleagues use DeepSeek large language models more conveniently
in daily work.

This project is not an official DeepSeek product, is not affiliated with
DeepSeek, and does not claim any DeepSeek ownership, authorization, or
endorsement. The DeepSeek name is used only to describe compatibility and
support for DeepSeek models.

## Why 0.0.1

The project is not complete. `v0.0.1` is intentionally defined as an early
preview so the public GitHub release does not overstate product maturity. The
main engineering priority after this release is to make the app reliably build,
install, launch, and run on Windows.

After the Windows version is running reliably, the next platform target is
macOS. The repository already contains a macOS Tauri packaging config, but macOS
validation and release work will come after the Windows baseline is stable.

## Basic Functions In This Preview

- Tauri + React + TypeScript + Rust desktop shell.
- Local-first workspace setup for a workspace folder, evidence folder, and
  export folder.
- DeepSeek route readiness through a local `DEEPSEEK_API_KEY` environment
  variable without storing or showing the key value.
- Optional local DeepSeek smoke tests for Chat Completions and Operations
  Briefing synthesis.
- Permissioned tool surfaces for file, network, browser, terminal, drive,
  email, and Computer Use operations.
- Append-only local audit records for access requests, approvals, tool
  attempts, workflow runs, memory records, and work packages.
- Memory Studio for reviewable memories, edits, deletion, expiration, and
  explicit conflict handling.
- Operations Briefing workflow that reads local evidence, drafts a management
  brief, and can use DeepSeek synthesis when configured.
- Local report and package export paths for Markdown, HTML, lightweight PDF,
  and work-package JSON.
- Windows NSIS debug installer build path for local validation.

## Current Limits

- Real mailbox connectors are not complete.
- Real cloud-drive connectors are not complete.
- Browser form submission and terminal write are approval/audit boundaries, not
  broad automation executors.
- Managed Codex bridge sidecar installation is deferred.
- Hosted sync, account systems, marketplaces, and arbitrary third-party
  executable plugins are not included.
- Public binary distribution is conservative until signing, packaging, and
  provenance are ready.
- PDF export is lightweight and ASCII-safe. Use Markdown or HTML for Chinese or
  other Unicode report content.

## Open Source Acknowledgement

This project benefits from the GitHub open-source ecosystem. Its architecture
and engineering direction were informed by public open-source work in desktop
apps, agent tooling, workflow systems, permission design, auditing, local-first
software, and the Rust/React/Tauri ecosystem.

We sincerely thank the founders, maintainers, and contributors of those
open-source projects. Their work makes projects like this possible.

No private, leaked, or non-authorized source code should be copied into this
repository. Public open-source references are used as learning material and
engineering inspiration, with respect for their licenses and maintainers.

## Local Verification

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 test
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
```

Optional live DeepSeek smoke tests:

```powershell
$env:DEEPSEEK_API_KEY = Read-Host "DeepSeek API key"
npx pnpm@9.15.9 test:deepseek
npx pnpm@9.15.9 test:deepseek:briefing
```

Do not commit API keys, `.env` files, local app data, local evidence folders, or
generated installer artifacts.
