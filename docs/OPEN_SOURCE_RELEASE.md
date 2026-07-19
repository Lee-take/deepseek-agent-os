# Open Source Release Plan

This document defines the `0.1.0` release checkpoint so the project can keep a
credible GitHub open-source baseline for DeepSeek-first desktop agent support
that can run on Windows.

## Release Goal

Ship a buildable local-first desktop Agent OS preview that demonstrates:

- DeepSeek-first model routing, credential readiness, cache telemetry, and manual
  pricing configuration;
- permissioned local tools with audit records;
- Memory Studio with explicit review and conflict actions;
- an Operations Briefing workflow pack using local evidence and DeepSeek
  synthesis when configured;
- Windows installer packaging, source build instructions, and a locally
  verified Windows build/install/launch/run path through the local release gate
  and installed UI workflow smoke.
- A platform roadmap that keeps the Windows checkpoint first, then validates
  and releases macOS after the Windows checkpoint remains stable.

## Non-Goals For The 0.1.0 Release

- No new workflow packs.
- No new model providers beyond the existing abstraction.
- No real email connector.
- No real cloud-drive connector.
- No DS Agent-managed local bridge service.
- No PDF v2 CJK font work.
- No broader Computer Use automation.
- No arbitrary third-party executable plugin system.

## Required Before Publication

- Confirm Apache-2.0 license metadata is present in the repository.
- Confirm repository visibility and project owner name.
- Keep the already-published `v0.0.1` source snapshot, tag, and release
  unchanged.
- For the formal `v0.1.0` line, publish a GitHub release with the Windows
  installer attached after final verification passes.
- The Windows installer should embed the Microsoft WebView2 bootstrapper so
  ordinary users do not need a developer toolchain or a separate WebView2 setup
  step.
- Run final local release verification on the release branch before any
  publication decision.
- Prepare release notes that call out preview limits plainly.

## Post-Release Maintenance

- Do not move an already published release tag. Keep public source snapshots
  reproducible for users who downloaded the generated source archive. The older
  `v0.1-alpha` tag is historical and should not be treated as the current
  project version.
- If post-release hardening commits should become a released snapshot, create a
  new patch or prerelease tag instead of rewriting an old tag.
- The older source-only publication decision is superseded by the
  `v0.1.0` Windows installer release plan.
- Keep patch releases focused on release hygiene, security checks,
  documentation corrections, Windows run reliability, or DeepSeek compatibility
  verification. Do not use patch releases to add broad new product capabilities
  outside the existing DeepSeek-first workflows, permissions, memory, Windows
  setup behavior, and Operations Briefing scope.
- If an unsigned installer is attached, disclose the unsigned status in the
  release notes and provide a SHA-256 checksum.
- Windows GNU release builds strip the target-path-bearing COFF symbol table and
  disable linker-generated PE timestamps. NSIS stores payload bytes without
  source-file modification times. A stable installer release requires two
  fresh, distinct `CARGO_TARGET_DIR` builds with identical application and
  installer byte sizes and SHA-256 values.
- The immutable `v1.0.2` installer remains unsigned. `v1.1.0`, `v1.2.0`, and
  `v1.3.0` are explicitly disclosed unsigned exceptions: both the application executable and
  NSIS installer must read back as `NotSigned`, and each Release must warn about
  `Unknown publisher` and Microsoft Defender SmartScreen while binding the
  HTTPS asset to its exact source, version, byte size, and SHA-256. A later
  release must not be represented as signed unless both files satisfy
  `CODE_SIGNING_POLICY.md` and independently verify as Authenticode `Valid`.

## Release Hygiene Artifacts

- `README.md` explains the independent project positioning, basic functions,
  current limits, and open-source acknowledgements.
- `CONTRIBUTING.md` explains the `0.1.0` Windows-first preview policy.
- `SECURITY.md` documents current security boundaries and private reporting
  expectations.
- `PRIVACY.md` documents local data handling and every current category of
  user-triggered network activity.
- `CODE_SIGNING_POLICY.md` documents signing roles, manual approval,
  source-to-binary provenance, two-stage application/installer signing, and
  independent Authenticode verification. Until SignPath approval provides real
  identifiers, CI must not contain guessed organization, project, policy, or
  artifact slugs and the repository must not contain a signing token.
- GitHub Private Vulnerability Reporting is enabled for sensitive security
  reports.
- `.github/pull_request_template.md` keeps PRs scoped to existing preview work.
- `.github/ISSUE_TEMPLATE/bug_report.yml` and
  `.github/ISSUE_TEMPLATE/deepseek_compatibility.yml` collect useful reports
  without encouraging broad feature requests outside the current preview scope.
- `.github/workflows/ci.yml` verifies the repository secret scan, desktop
  frontend build, Rust tests, and runs `pnpm test:release-source` on Windows
  without requiring secrets; it also keeps the workflow token scoped to
  `permissions: contents: read` and does not upload release assets or artifacts.
- `scripts/require-desktop-workspace.mjs` keeps root `dev`, `build`, and
  `tauri` scripts pointed at the desktop workspace in a source checkout instead
  of silently running against a partial tree.
- `.gitattributes` keeps source line endings stable across Windows and Unix
  workstations while preserving binary assets, including local Windows debug
  symbols, native dynamic libraries, and installer/package artifacts, as
  binary files.
- `.gitignore` and the source-only release guard keep local `.env` files,
  credential, private-key, and certificate files, dependency install
  directories and frontend/Rust build output, reference repositories and
  generated Tauri state directories, generated Tauri resource directories,
  local runtime logs and temporary files, generated app data, SQLite database
  files, SQLite sidecar files, screenshots, reports, work packages, Tauri bundle
  directories, release binaries, maintainer handoff notes, decision logs, and
  internal planning files out of the source-first preview. The same guard also
  rejects unexpected binary files and oversized source files so accidental local
  exports or packaged assets do not enter generated source archives.
- `.env.example` documents local DeepSeek and optional local bridge environment
  variables without storing secret values.
- `docs/RELEASE_NOTES_v1.3.0.md` is the current stable release note.
  `docs/RELEASE_NOTES_v1.2.0.md` preserves the immutable Step 2 release evidence.
  `docs/RELEASE_NOTES_v1.1.0.md`, `docs/RELEASE_NOTES_v1.0.2.md`, `docs/RELEASE_NOTES_v1.0.1.md`,
  `docs/RELEASE_NOTES_v1.0.0.md`, and
  `docs/DS_AGENT_V1_COMPLETION_AUDIT.md` preserve the immutable first stable
  release evidence. Historical release notes, including `v0.9.0`, `v0.8.0`,
  and `v0.8.0-rc.1`, remain unchanged.

Maintainer handoff notes, decision logs, and `docs/superpowers/` planning files
are local-only continuation material. They are useful for project handoff on the
maintainer machine, but they are not public release docs and must not enter a
source-only release archive.

## Open Source Acknowledgement

This project is informed by public open-source work on GitHub in desktop apps,
agent tooling, workflow systems, permission design, auditing, local-first
software, and the broader Rust/React/Tauri ecosystem. We thank the founders,
maintainers, and contributors of those projects.

Public open-source references are learning material and engineering
inspiration. Private, leaked, or non-authorized source code must not be copied
into this repository.

## Recommended License Discussion

The project uses Apache-2.0. This was chosen for infrastructure-style open
source and its explicit patent grant.

Public copy should not describe Apache-2.0 as a hard ban on commercial use.
Instead, it should state the intended boundary: DS Agent is published for
learning, research, evaluation, internal pilots, and adaptation within the
license terms, while the current source is not presented as a turnkey codebase
to repackage directly as a commercial product or hosted service. Any commercial
evaluation should preserve notices, avoid implying DS Agent or DeepSeek
endorsement, and require independent security review, compliance review,
testing, signing, and user support.

## Preview Honesty Rules

- Do not imply official DeepSeek affiliation.
- Do not claim live web evidence from plain chat-completion text.
- Do not claim cloud connectors where the implementation is local-folder or
  approval and audit records only.
- Do not hide high-risk Computer Use limitations.
- Do not add broad feature work outside the existing DeepSeek-first workflows,
  permissions, memory, Windows setup behavior, and Operations Briefing scope.
