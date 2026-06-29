# Open Source Release Plan

This document defines the `v0.0.1` release goal so the project can become a
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
- Windows debug installer packaging, source build instructions, and a clear
  path toward reliable Windows launch and first-run behavior.
- A platform roadmap that completes Windows first, then validates and releases
  macOS after the Windows baseline is stable.

## Non-Goals Before The Windows 0.0.1 Baseline

- No new workflow packs.
- No new model providers beyond the existing abstraction.
- No real email connector.
- No real cloud-drive connector.
- No managed Codex bridge sidecar.
- No PDF v2 CJK font work.
- No broader Computer Use automation.
- No arbitrary third-party executable plugin system.

## Required Before Public GitHub Release

- Confirm Apache-2.0 license metadata is present in the repository.
- Confirm repository visibility and project owner name.
- Publish `v0.0.1` as source-first unless the maintainer later approves
  unsigned installer artifacts explicitly.
- Run final verification on the release branch.
- Prepare release notes that call out preview limits plainly.

## Post-Release Maintenance

- Do not move an already published release tag. Keep public source snapshots
  reproducible for users who downloaded the generated source archive. The older
  `v0.1-alpha` tag is historical and should not be treated as the current
  project version.
- If post-release hardening commits should become a released source snapshot,
  create a new source-only prerelease tag instead of rewriting an old tag.
- Keep patch prereleases focused on release hygiene, security checks,
  documentation corrections, Windows run reliability, or DeepSeek compatibility
  verification. Do not use patch releases to add broad new product capabilities
  before the Windows baseline is genuinely usable.
- Do not attach unsigned installer binaries unless the maintainer explicitly
  approves binary distribution for that release.

## Release Hygiene Artifacts

- `README.md` explains the independent project positioning, basic functions,
  current limits, and open-source acknowledgements.
- `CONTRIBUTING.md` explains the `0.0.1` Windows-first preview policy.
- `SECURITY.md` documents current security boundaries and private reporting
  expectations.
- GitHub Private Vulnerability Reporting is enabled for sensitive security
  reports.
- `.github/pull_request_template.md` keeps PRs scoped to existing preview work.
- `.github/ISSUE_TEMPLATE/bug_report.yml` and
  `.github/ISSUE_TEMPLATE/deepseek_compatibility.yml` collect useful reports
  without encouraging broad feature requests before the Windows baseline is
  usable.
- `.github/workflows/ci.yml` verifies the repository secret scan, desktop
  frontend build, and Rust tests on Windows without requiring secrets.
- `.env.example` documents local DeepSeek and external bridge environment
  variables without storing secret values.
- `docs/RELEASE_NOTES_v0.0.1.md` is the current release note source.

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

## Preview Honesty Rules

- Do not imply official DeepSeek affiliation.
- Do not claim live web evidence from plain chat-completion text.
- Do not claim cloud connectors where the implementation is local-folder or
  approval-boundary only.
- Do not hide high-risk Computer Use limitations.
- Do not add broad feature work before the Windows baseline is genuinely usable.
