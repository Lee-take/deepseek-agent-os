# DS Agent v1.1.0

`v1.1.0` adds the first ordinary-user onboarding and readiness path to the
stable Windows desktop app. Package, desktop, Tauri, Cargo, updater, and
installer metadata are `1.1.0` / `v1.1.0`.

## Onboarding and readiness

- Enter one user-supplied DeepSeek API Key in the app. DS Agent stores it in a
  dedicated Windows DPAPI-protected vault; an environment Key remains an
  explicit compatibility fallback and is never silently copied into the vault.
- Explicit verification checks DeepSeek authentication/balance and confirms
  that `deepseek-v4-flash` and `deepseek-v4-pro` are available before model work
  is reported ready.
- The Kernel exposes only secret-free readiness state and stable repair codes.
  Raw Keys, provider response bodies, balance amounts, account details, and
  absolute app-data or vault paths are excluded from ordinary UI projections,
  events, logs, work packages, and release evidence.
- Workspace setup creates the managed directories inside the selected root and
  uses a bounded create/write/sync/delete probe to verify writability. The
  ordinary UI does not expose internal settings or managed-directory paths.
- Active release-smoke defaults use V4 Flash/Pro. Candidate and installed UI
  smoke use independent temporary `APPDATA`, `LOCALAPPDATA`, WebView2,
  workspace, and report roots and verify cleanup after exit.

## Compatibility

- Existing `local-directories.json` workspace settings remain readable.
- Environment-only operators continue to work after explicit verification.
- Existing conversations are not rewritten, old work-package fields default
  safely, and connector credential vaults are untouched.
- Replacing a Key invalidates the previous verification receipt; removing it
  removes both the local secret and receipt.

## Windows download and integrity

> **Unsigned release:** both `ds-agent.exe` and
> `DS.Agent_1.1.0_x64-setup.exe` are intentionally Authenticode `NotSigned`.
> Windows may display `Unknown publisher` or a Microsoft Defender SmartScreen
> warning. This is not a signed or SignPath-approved release.

Download only over HTTPS from the official
[`v1.1.0` GitHub Release](https://github.com/Lee-take/dsagent/releases/tag/v1.1.0).
Before running the installer, compare its file name, version, exact byte size,
and SHA-256 with the values in that Release. The final Release body records the
exact source commit and asset identity after the exact-main build and fresh
download readback.

The SignPath Foundation application is submitted and approval is pending. If
the project is accepted later, signing starts with a subsequent new version;
the `v1.1.0` tag, Release, and asset will not be moved or replaced.

## Scope and limits

This release does not include GoalEnvelope or any Step 2 work. It adds no new
Office executor, connector, Computer Use, automation, external account, or
external-write capability. Production Microsoft/Google account registration
and live mail/calendar writes remain disabled. DS Agent remains an independent
open-source project, not an official DeepSeek product.

The R1 gate covers focused and full tests, frontend production build, Rust
formatting, migration/recovery regressions, release-source and secret scans,
isolated candidate/installed UI and workflow smoke, PR CI, exact-main CI, and
final version/file-name/byte-size/SHA-256/source/tag/Release/Latest/fresh-
download readback. The cancelled formal 20-run Windows lab was not performed
and is not claimed as release evidence.

## Deterministic briefing templates

`docs/templates/operations-briefing-smoke-evidence` is used only for
deterministic release checks. The bundled smoke files are marked as
`SMOKE SAMPLE evidence for local verification only` and
`Replace before operational use`; replace it before operational use.

The desktop seed action uses the separate
`docs/templates/operations-briefing-evidence` folder. Those files are blank operator templates,
not sample business evidence.
