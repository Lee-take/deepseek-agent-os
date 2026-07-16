# DS Agent v1.0.2

`v1.0.2` is an urgent compatibility and trust/UI patch for the stable DS Agent
1.0 line. It fixes two user-visible regressions reported against the published
`v1.0.1` desktop app. The `v1.0.1` commit, tag, Release, notes, and installer
remain immutable.

Package, desktop, Tauri and Cargo metadata are `1.0.2`, and the updater identity
is `v1.0.2`.

## User-facing reply isolation

- Fenced DeepSeek protocol responses that use `subagent_plan: null` are parsed
  as an empty optional plan instead of falling back to raw text.
- Protocol JSON is never shown in place of the user-facing reply when another
  optional envelope field has an unexpected shape. DS Agent extracts only
  `reply_to_user`; malformed or incomplete actions remain fail-closed and are
  not executed.
- Previously saved affected messages are cleaned at presentation time after the
  updated app starts. Conversation storage is not silently rewritten.

## Run-step terminal state

- The right-side workflow inspector now selects the latest parent run from the
  active conversation. Once that durable run is complete, the six fixed steps
  converge to Done / 已完成 instead of retaining stale waiting states.
- Calling DeepSeek with the user's configured API Key is not a per-run approval
  action. A missing or unusable DeepSeek configuration is shown as blocked, not
  as a permission confirmation request.
- This patch does not weaken approval policy for local writes, Computer Use,
  external effects, or other risk-bearing capabilities. The Kernel remains the
  authority for permission, execution, evidence, audit, verification, and
  recovery.

## Scope and compatibility

This patch adds no new connector, Computer Use, automation, memory, Subagent,
or Skill feature. A valid DeepSeek API Key supplied by each user remains
required. Production Microsoft/Google account registration and live external
writes remain disabled.

## Windows download and integrity

- Asset: `DS.Agent_1.0.2_x64-setup.exe`
- File and product version: `1.0.2`
- Architecture: Windows x64
- Authenticode status: unsigned (`NotSigned`)

Verify the final byte size and SHA-256 against the published GitHub Release
before running the installer. The installer embeds the Microsoft WebView2
bootstrapper.

The release artifact was built and inspected without launching the installer
or changing the installed DS Agent application or its data.

The final offline release gate covers the source secret scan, TypeScript/Vite
production build, focused reply/run-state regressions, all Node/UI checks, the
source-only release guard, and the complete Rust suite. Live DeepSeek and
installed Office/rendering checks remain separately permission-gated.

## Deterministic briefing fixtures

For deterministic Operations Briefing checks,
`docs/templates/operations-briefing-smoke-evidence` contains the warning
**SMOKE SAMPLE evidence for local verification only** and every file says
**Replace before operational use**. The bundled smoke files are marked as
non-operational test data. Separately,
`docs/templates/operations-briefing-evidence` contains blank operator templates
that the desktop can seed into a user-selected evidence folder.
