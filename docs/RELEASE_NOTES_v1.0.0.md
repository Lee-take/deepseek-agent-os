# DS Agent v1.0.0

`v1.0.0` is the first stable DS Agent 1.0 release. It evolves the published
`v0.9.0` foundation without rewriting or moving any earlier commit, tag,
Release, or asset.

Package, desktop, Tauri and Cargo metadata are `1.0.0`, and the updater identity
is `v1.0.0`.

## Highlights

- Kernel-owned lifecycle projections cover Chat, Agent, Automation, Tool,
  Connector, Artifact, Computer Use, and Expert Team work without moving
  business state into Tauri commands or React components.
- Review and Recovery use Kernel-issued opaque actions bound to exact revisions.
  Approved local file changes have durable before/after checkpoints, verified
  one-shot undo, stale-action rejection, and restart-safe no-replay behavior.
- Recurring Automation and Microsoft/Google-shaped connected work share typed
  intent, health, read/sync, private local draft, mutation, reconciliation, and
  crash-repair contracts. Adversarial fake providers prove timeout/restart
  idempotency and a provider apply count of one.
- Chinese and English chat can draft mail or propose a calendar event, present a
  human-readable review card, and approve the exact revision.
- Connected-work approval/start and successful provider completion/private
  projection consumption are atomic. Startup repair handles interrupted work
  without replaying external effects.
- Migration and privacy tests cover legacy schema upgrades, malformed-row
  starvation resistance, stale generations, late results, repeated approvals,
  dynamic secret markers, and private absolute paths.
- Approval cards now appear at the bottom of the auto-scrolling DS Agent chat
  thread with visible **Confirm and run** and **Reject** actions. The right rail
  remains a read-only status surface.
- Scenario Templates and Task Records and Work Packages remain implemented but
  are hidden from the ordinary Plugins sidebar to keep the main experience
  focused.

## Model and authority boundary

DeepSeek owns open-ended understanding, planning, analysis, and synthesis. The
DS Agent Kernel owns deterministic validation, approval, execution, evidence,
audit, verification, and recovery. Credentials and provider-private content do
not become ordinary event, export, model-context, or UI data.

Production Microsoft/Google account registration and external-write authority
remain disabled in this release. Validation uses offline data and adversarial
fake providers; it does not use real accounts, activate a live provider, send
email, or create, modify, or cancel real calendar events.

## Windows download and integrity

- Asset: `DS.Agent_1.0.0_x64-setup.exe`
- Size: `12,714,575 bytes`
- SHA-256: `3B944589C8443A677AF55F8748C06A4D6EBA4ACE86E63E302D5782D5A5E548E4`
- File and product version: `1.0.0`
- Architecture: Windows x64

The installer is currently unsigned, so Windows may show an unknown-publisher
warning. It embeds the Microsoft WebView2 bootstrapper. The final artifact was
built and inspected without launching the installer or changing installed app
data.

The offline release gate passed the 192-file secret scan, TypeScript/Vite
production build, all Node/UI checks, and 852 Rust tests: 845 passed, seven
permission-gated live/GUI tests were intentionally ignored, and zero failed.

## Deterministic briefing fixtures

For deterministic Operations Briefing checks,
`docs/templates/operations-briefing-smoke-evidence` contains the warning
**SMOKE SAMPLE evidence for local verification only** and every file says
**Replace before operational use**. The bundled smoke files are marked as
non-operational test data. Separately,
`docs/templates/operations-briefing-evidence` contains blank operator templates
that the desktop can seed into a user-selected evidence folder.
