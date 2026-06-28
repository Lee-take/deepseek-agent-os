# DeepSeek Agent OS

Local-first open-source desktop Agent OS optimized for DeepSeek.

Read first:

- `PROJECT_CONTEXT.md`
- `DECISIONS.md`
- `SESSION_HANDOFF.md`
- `docs/superpowers/specs/2026-06-28-deepseek-agent-os-architecture-design.md`

## Development

Foundation MVP desktop commands:

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'deepseek_ui_cargo_target'
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 dev
```

On Windows, `CARGO_TARGET_DIR` keeps Rust build output out of the repo path with a space, which avoids the local MinGW `dlltool` issue.

## Architecture

The app uses a stable Agent OS Kernel with Workflow Packs. The first implementation slice builds the desktop shell, local event store, policy model, and DeepSeek route model.
