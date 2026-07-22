# DS Agent v1.4.0

`v1.4.0` is a backward-compatible minor release that packages the Step 4 local
T1 Office verification engine and persistent goal-continuation checkpoints.
Package, desktop, Tauri, Cargo, updater, and installer metadata are
`1.4.0` / `v1.4.0`.

## Verified local T1 engine

- `operations.reconcile_excel` accepts one exact workspace-relative source
  directory, scans the bounded T1 XLSX/DOCX/PDF source set, records byte counts,
  media types and SHA-256 identities, reconciles every key figure, and writes a
  new XLSX without overwriting an existing artifact.
- `operations.generate_powerpoint` re-verifies the exact reconciliation receipt,
  creates a new one-page PPTX, renders it through local Microsoft Office, and
  permits at most three non-overwriting sibling revisions before completion.
- Both tools remain behind Kernel ToolContracts, exact grouped authorization,
  workspace boundaries, resource ownership, persisted artifact identity,
  required evidence kinds, and post-write verification.
- Goal continuation now persists revision-bound gaps, budgets, evidence,
  resources, artifacts, and terminal source identities across restart. DeepSeek
  remains advisory and cannot approve actions or mint authorization, evidence,
  or completion receipts.

## Reachability boundary

The installed binary contains the production T1 ToolContracts, executors, and
Tauri command dispatch. Ordinary chat does not yet automatically select or
sequence `operations.reconcile_excel` and `operations.generate_powerpoint`.
Accordingly, v1.4.0 is not represented as a complete one-sentence Office
workflow in the React chat UI. Existing general Office create/open/update
actions remain separate.

This release does not add Step 5 Computer Use work, connector or production
tenant expansion, background external writes, TaskCheckpoint/exact undo, batch
concurrency, Headless/ACP, or any C5A or later capability.

## C4C verification basis

The accepted deterministic matrix used 50 isolated local T1 groups: 43 `A` and
7 expected fail-closed `F`, for VOCR 86.00%. It detected both injected numeric
conflicts and both damaged formulas. Office open/render, formula, garbling,
clipping and overflow failures produced zero false completion. Every successful
group retained all eight traceable key figures; authorization-budget compliance
was 50/50; unauthorized path writes and DeepSeek-issued authority or completion
receipts were both zero.

A separate installed Microsoft Office/render case passed with one rendered
page and is excluded from the deterministic 50-group denominator. The release
gate reruns the focused matrix, complete Rust and Node suites, frontend build,
format, secret scan, release-source, migration/recovery, isolated candidate and
installed checks, and an absolute-zero Clippy command before publication.

## Deterministic briefing templates

`docs/templates/operations-briefing-smoke-evidence` remains deterministic local
test material. The bundled smoke files are marked as
`SMOKE SAMPLE evidence for local verification only` and
`Replace before operational use`; replace them before operational use. The
desktop seed action continues to use the blank operator templates under
`docs/templates/operations-briefing-evidence`.

## Reproducible Windows package

The final release gate compares the application and installer from two fresh,
distinct `CARGO_TARGET_DIR` builds byte-for-byte. The GitHub Release records the
exact main/tag commit, file name, byte size, SHA-256, version, and truthful
signature state, then independently downloads and re-verifies the published
asset.

## Unsigned release

Both `ds-agent.exe` and `DS.Agent_1.4.0_x64-setup.exe` are intentionally
Authenticode `NotSigned`; there is no signer. Windows may show `Unknown
publisher` or Microsoft Defender SmartScreen. Download only over HTTPS from the
official [`v1.4.0` GitHub Release](https://github.com/Lee-take/dsagent/releases/tag/v1.4.0)
and verify the exact byte size and SHA-256 published there before running it.

The SignPath Foundation application remains submitted and approval is pending.
This release is not represented as signed or SignPath-approved. If signing
becomes available later, it begins with a subsequent new version and does not
replace the immutable v1.1.0, v1.2.0, v1.3.0, or v1.4.0 tag, Release, or asset.

No real API key, production account, paid API, production tenant, installed DS
Agent overwrite, current user AppData mutation, or external target is required
for this release verification.
