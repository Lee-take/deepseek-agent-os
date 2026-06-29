# Contributing

DeepSeek Agent OS is in a `0.0.1` Windows-first preview stage. The project is
public, but it is not a finished agent product yet.

## Project Direction

This is an independent open-source desktop Agent OS optimized for DeepSeek. The
goal is to make DeepSeek useful in local-first agent workflows with explicit
permissions, auditable memory, tool boundaries, and operations workflow packs.

This project is not an official DeepSeek product and is not affiliated with
DeepSeek.

## Current Contribution Policy

During the `0.0.1` preview, accepted changes should prioritize:

- build, packaging, and installation fixes;
- documentation and examples;
- security, privacy, and permission-boundary fixes;
- test coverage for existing behavior;
- bug fixes that preserve the current product scope;
- Windows launch, installer, and first-run reliability.

Please avoid opening PRs for new capabilities, new workflow packs, new provider
integrations, or broader automation until the Windows baseline is genuinely
usable.

## Development Setup

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop build
$env:CARGO_TARGET_DIR = Join-Path $env:TEMP 'deepseek_agent_os_cargo_target'
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
```

On Windows, keep `CARGO_TARGET_DIR` outside a repository path with spaces.

## Pull Request Expectations

- Keep changes scoped and explain the user-facing impact.
- Do not serialize API keys, local user paths, screen contents, mailbox content,
  or personal data in tests or fixtures.
- Add or update tests for behavior changes.
- Run the relevant verification commands before requesting review.
- State any skipped verification clearly in the PR description.

## Safety Boundaries

Computer Use, browser submission, terminal write, email send, file write, and
drive write behavior must remain permission-gated and auditable. Do not bypass
the policy engine for convenience.

NetworkSearch must preserve source links. Plain model text should not be treated
as verified web evidence.
