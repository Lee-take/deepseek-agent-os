# Session Handoff

Last updated: 2026-06-28

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
pnpm install
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'deepseek_ui_cargo_target'
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
pnpm dev
```

Set `CARGO_TARGET_DIR` before Rust verification on Windows to avoid the local MinGW `dlltool` path-with-space issue under `D:\deepseek UI`.

## Confirmed Architecture Direction

- Build Agent OS Kernel plus Workflow Packs.
- Use Tauri + React + TypeScript + Rust sidecar.
- Use local-first desktop architecture.
- Team collaboration data model exists from day one; no cloud sync in MVP.
- First version supports email, drive, browser, and Computer Use through permissioned capabilities.
- Use granular permission controls similar to Codex access dropdown, but more understandable for office users.
- Add thinking/model controls in the main composer.
- Use automatic memory with Memory Studio, source traceability, scopes, lifecycle, conflict handling, and import/export.
- Use DeepSeek-first optimization layer with Pro/Flash/Auto routing, thinking control, context caching, and cost/latency telemetry.
- Use the Operations Briefing workflow as the first Operations Management Pack flow.
- Build full export, import preview, template/workflow import, and read-only run archive replay.
- Treat imported memories as reviewable candidates, not automatic writes.
- Put Computer Use behind an experimental high-risk flag in MVP.
- Use local password/unlock plus local agent token.

## Next Actions

1. Implement the memory kernel and Memory Studio plan.
2. Add connector capability adapters for email, drive, browser, and computer-use behind policy.
3. Implement the Operations Briefing workflow pack.
4. Add import/export packages and read-only run archives.
5. Integrate the DeepSeek provider, caching, and latency controller.

## Open Questions

- Decide the exact sample input files for the Operations Briefing workflow.
- Confirm connector implementation priority across email, drive, browser, and computer-use.
- Define the Memory Studio review UX for candidate memories, conflicts, and approvals.
