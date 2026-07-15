# DS Agent v1.0.1

`v1.0.1` is a focused patch release for the first stable DS Agent 1.0 line. It
fixes an approval-surface regression found in the published `v1.0.0` desktop
app and adds visible creator/maintainer attribution. The `v1.0.0` commit, tag,
Release, notes, and installer remain immutable.

Package, desktop, Tauri and Cargo metadata are `1.0.1`, and the updater identity
is `v1.0.1`.

## Approval UI fix

- Historical pending capability records are no longer inserted as one global
  approval block beneath an unrelated active conversation. An ordinary
  knowledge question therefore cannot inherit old browser, file, update, or
  other approval rows merely because they remain pending in local history.
- Chat approval controls now belong to the assistant message and exact
  `permission_request_id` values that proposed the actions. A task with no
  action awaiting confirmation shows no approval controls.
- When one task needs several permissions, DS Agent shows one explanatory
  sentence and one Confirm and run / Reject decision pair. One click
  resolves every permission record for that task before its actions resume in
  order. The Kernel still retains separate capability decisions and audit
  records underneath that single user decision.
- Rejecting once blocks every action awaiting approval for that task. Partial
  failures refresh current permission state so a retry does not blindly repeat
  already completed decisions.
- Legacy manual capability requests remain actionable only in their own
  capability card. Task-bound and exact-tool approvals are excluded from those
  generic cards, preventing duplicate approval surfaces.

The patch does not silently delete, approve, or reject historical local
records. Existing `v1.0.0` app data remains under the user's control.

## Attribution and release identity

- `Lee take` is visible as the creator and maintainer in Settings.
- Root, desktop, Rust package, and Windows application metadata consistently
  identify version `1.0.1` and the supported publisher attribution.
- This attribution is not a digital signature. The Windows installer and
  application remain unsigned and may show an unknown-publisher warning.

## Model and authority boundary

A valid DeepSeek API Key supplied by each user remains required. DS Agent does
not bundle a shared key or bypass DeepSeek access requirements. DeepSeek owns
open-ended understanding, planning, analysis, and synthesis; the DS Agent
Kernel owns deterministic validation, approval, execution, evidence, audit,
verification, and recovery.

Production Microsoft/Google account registration and live external-write
authority remain disabled. This patch does not sign in to real accounts, send
real email, or create, change, or cancel real calendar events.

## Windows download and integrity

- Asset: `DS.Agent_1.0.1_x64-setup.exe`
- Size: `12,716,857 bytes`
- SHA-256: `469C4EFA54F4C94A6E37D28C9C88D331B26E1770C6792DC93D02B451640E2A6F`
- File and product version: `1.0.1`
- Architecture: Windows x64
- Authenticode status: unsigned (`NotSigned`)

The installer embeds the Microsoft WebView2 bootstrapper. The release artifact
was built and inspected without launching the installer or changing the
installed DS Agent application or its data.

The offline release gate covers the source secret scan, TypeScript/Vite
production build, focused approval interaction checks, all Node/UI checks, and
852 Rust tests: 845 passed, seven permission-gated live/GUI tests were
intentionally ignored, and zero failed.

## Deterministic briefing fixtures

For deterministic Operations Briefing checks,
`docs/templates/operations-briefing-smoke-evidence` contains the warning
**SMOKE SAMPLE evidence for local verification only** and every file says
**Replace before operational use**. The bundled smoke files are marked as
non-operational test data. Separately,
`docs/templates/operations-briefing-evidence` contains blank operator templates
that the desktop can seed into a user-selected evidence folder.
