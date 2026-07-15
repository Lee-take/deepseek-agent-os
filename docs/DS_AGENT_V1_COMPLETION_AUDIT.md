# DS Agent v1.0 Completion Audit

Date: 2026-07-15

Release: `1.0.0` / updater identity `v1.0.0`

Scope: local source, offline/fake-provider behavior, migrations, UI, recovery,
and an uninstalled Windows release artifact

## Authority boundary

Local validation did not use real Microsoft or Google accounts, activate live
providers, write to external systems, install or overwrite DS Agent, or modify
installed application data. Publication authorization is limited to committing
and pushing the audited source, creating the new immutable `v1.0.0` tag and
GitHub Release, and uploading the audited installer. The published `v0.9.0`
commit, tag, Release, and assets remain unchanged.

## Product completion evidence

- Phase 1 inspected actual `v0.9.0` source, CodeGraph, Git state, Release, and CI
  before implementation and preserved the existing Kernel and history.
- Phase 2 made updater downloads bounded, origin-locked, receipt-bound,
  atomically landed, hash/size/Windows-file-identity checked, exactly consumed,
  revalidated before launch, and restart-repaired.
- Phase 3 made lifecycle, exact Review, Recovery, workspace checkpoints,
  verified one-shot undo, stale-action rejection, and crash no-replay behavior
  Kernel-owned and test-covered.
- Phase 4 gave Automation plus Microsoft/Google-shaped connected work typed
  contracts, credential isolation, private projections, exact approval,
  reconciliation, and adversarial fake-provider one-effect validation.
- Phase 5 verified Chinese and English chat-first connected-work review,
  ordinary-user UI cards, fake/no-provider execution boundaries, and
  cross-feature UI regressions in an isolated desktop run.
- Phase 6 covered atomic approval/start, atomic completion/private-consumption,
  interrupted-work repair, schema migration, malformed-row handling, dynamic
  secret/path privacy, late generations, and a broad failure matrix.
- Final UI refinement places the interactive approval queue at the bottom of
  the auto-scrolling chat thread, removes duplicate right-rail actions, and
  keeps Scenario Templates plus Task Records and Work Packages source-retained
  but hidden from the ordinary Plugins sidebar.

## Final gate evidence

- The final offline `release-local -- --skip-live-deepseek` gate passed after
  the UI refinement. It covered the 192-file secret scan, TypeScript/Vite
  production build, all Node/UI checks, the complete 852-test Rust suite (845
  passed, seven intentionally ignored live/GUI tests, zero failed), unstaged and
  staged diff whitespace checks, the source-only release guard, and all three
  local helper self-tests.
- Focused approval visibility, plugin catalog, installed-UI helper self-test,
  TypeScript, and production build checks passed. An isolated fake-approval
  browser run visually confirmed the central approval card and hidden plugin
  utilities without using installed app data.
- `cargo fmt --check`, `git diff --check`, and `git diff --cached --check` are
  required again immediately before publication.
- Live DeepSeek, installed UI/workflow, real-account, and external-write checks
  were explicitly skipped because they cross the stated authority boundary; no
  locally completable product work was skipped.

## Release artifact integrity

- Build result: one Windows x64 NSIS bundle completed from the dedicated
  no-space Cargo target; no installer was launched.
- Release binary: `release/ds-agent.exe`, `44,313,153 bytes`, SHA-256
  `A6F37A2967866FC41B5F3C1997657CC5B3FA92EDE84EA6DD14FE638E64B516DC`.
- Installer: `DS.Agent_1.0.0_x64-setup.exe`, `12,714,575 bytes`, SHA-256
  `3B944589C8443A677AF55F8748C06A4D6EBA4ACE86E63E302D5782D5A5E548E4`.
- Both PE files report file/product version `1.0.0`, product and description
  `DS Agent`; the app is x86-64 Windows GUI. Both DS Agent files are unsigned.
- Static NSIS inspection reports NSIS 3 Unicode and includes the app executable,
  embedded Microsoft WebView2 bootstrapper, `WebView2Loader.dll`, and
  `ds-agent-icon.ico`.
- The embedded WebView2 bootstrapper is `1,688,792 bytes`, version
  `1.3.241.15`, SHA-256
  `F91077E2C116DCF6377E555D0D4A3A564D242351AD6718B6954658D4F74819C1`,
  and has a valid Microsoft signature.
- Packaged Loader and icon bytes match their staged/source files: Loader
  `160,320 bytes`, SHA-256
  `8427B1FC58EC707813E5C0A51EB5D69397BB333250A7B891BE4D3B123F1E0F1C`;
  icon `134,590 bytes`, SHA-256
  `D0C076A19C076E515639EDFB5D475493D3671B0A4A51837AF7102A5AFA175CE8`.
- The source-only release guard excludes generated targets, bundles,
  credentials, private runtime data, and local handoff plans.

No installer was launched as part of this audit.

## Remaining explicit permission gates

External runtime validation remains permission-gated: real Microsoft/Google
account authorization and live reads; real email/calendar writes;
installed-app upgrade validation; and code signing. These are not converted
into local product gaps.

## Deterministic briefing fixtures

`docs/templates/operations-briefing-smoke-evidence` uses the warning
**SMOKE SAMPLE evidence for local verification only** and each fixture says
**Replace before operational use**. The bundled smoke files are marked as
non-operational fixtures. The separate
`docs/templates/operations-briefing-evidence` folder contains blank operator templates
for safe desktop seeding; it is not sample operational evidence.
