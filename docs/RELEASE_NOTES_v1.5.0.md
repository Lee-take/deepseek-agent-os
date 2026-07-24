# DS Agent v1.5.0

`v1.5.0` is a backward-compatible minor release that packages the Step 5
durable, verified Computer Use golden slice. Package, desktop, Tauri, Cargo,
updater, and installer metadata are `1.5.0` / `v1.5.0`.

## Production-reachable durable Computer Use

- The installed app exposes one Kernel-owned Computer Use session and step
  lifecycle through the existing right-rail UI and Tauri commands. A user can
  start from a recent successful screenshot, bind one exact action and
  postcondition, approve it through the existing capability policy, re-observe,
  run once, take over, cancel, or re-observe after a safe stop.
- Every action binds the exact application, process, window, title, frame,
  target, semantic state, approval request, action fingerprint, screenshot
  evidence, and postcondition. The Kernel revalidates those identities
  immediately before execution.
- `ActionStarted` is persisted before the external effect and can be recorded
  only once. A control error, timeout, screenshot failure, process or window
  closure, restart, takeover, corrupt evidence, or ambiguous post-action state
  becomes fail-closed `EffectUnknown` or recovery inspection and is never
  replayed automatically.
- A screenshot, click, navigation, or mutation cannot prove completion by
  itself. Verification requires the exact post-action semantic receipt bound to
  the same action revision. DeepSeek remains advisory and cannot approve an
  action or mint authority, evidence, verification, or completion.

## Exact installed-test boundary

The Step 5 reliability evidence is deliberately narrower than general desktop
or browser automation. It covers generated, isolated targets only:

- one exact generated File Explorer folder and item;
- one exact generated Excel workbook, worksheet, cell, value, and sentinels;
- one installed Edge window using a fresh profile and one test-declared,
  ephemeral loopback portal with an exact tab, URL, origin, document, field,
  decoy, action, and semantic receipt.

The Edge DOM helper is compiled only for Windows tests. This release does not
claim arbitrary website control, stored-login reuse, a production portal or
tenant, or general browser completion. The named File Explorer, Excel, and
Edge/local-portal cases are exact installed-test evidence; they are not a
promise that every application, workbook, website, or desktop layout is
supported.

The final Step 5 matrix records 30 installed application runs: 10 File Explorer,
10 Excel, and 10 Edge/local-portal. All 30 complete, all 30 recover after the
declared window move/resize/focus change, wrong-target writes are zero, and
false completions are zero. A separate deterministic matrix covers 40 fault
cases, including stale identities, redirects, target/action drift, control and
screenshot failures, missing or corrupt receipts, closure, restart, takeover,
and recovery inspection.

## Preserved boundaries

`v1.5.0` preserves the v1.4.0 T1 Office engine, exact-task grouped
authorization, Goal completion receipts, Event Store invariants, migration and
recovery compatibility, updater integrity, and source-only release guards.

`docs/templates/operations-briefing-smoke-evidence` remains deterministic local
test material. The bundled smoke files are marked as
`SMOKE SAMPLE evidence for local verification only` and
`Replace before operational use`; replace them before operational use. The
desktop seed action continues to use blank operator templates under
`docs/templates/operations-briefing-evidence`.

This release does not add Step 6 persistent automation/recovery work, production
Microsoft or Google accounts, live mail/calendar writes, a production portal,
stored browser login reuse, broad browser automation, secure/UAC desktop
control, CAPTCHA, payment or administrator authentication, VM/profile claims,
Headless/ACP, or arbitrary executable extensions.

## Reproducible Windows package

The final release gate compares the application and installer from two fresh,
distinct `CARGO_TARGET_DIR` builds byte-for-byte. The GitHub Release records the
exact main/tag commit, file name, byte size, SHA-256, version, and truthful
signature state, then independently downloads and re-verifies the published
asset.

## Unsigned release

Both `ds-agent.exe` and `DS.Agent_1.5.0_x64-setup.exe` are intentionally
Authenticode `NotSigned`; there is no signer. Windows may show `Unknown
publisher` or Microsoft Defender SmartScreen. Download only over HTTPS from the
official [`v1.5.0` GitHub Release](https://github.com/Lee-take/dsagent/releases/tag/v1.5.0)
and verify the exact byte size and SHA-256 published there before running it.

The SignPath Foundation application remains submitted and approval is pending.
This release is not represented as signed or SignPath-approved. If signing
becomes available later, it begins with a subsequent new version and does not
replace the immutable v1.1.0, v1.2.0, v1.3.0, v1.4.0, or v1.5.0 tag, Release,
or asset.

No real API key, production account, paid API, production tenant, installed DS
Agent overwrite, current user AppData mutation, or external production target
is required for this release verification.
