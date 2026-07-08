# DS Agent v0.1.0 Release Scorecard

## Release Decision

Status: pending
Target: v0.1.0 Windows formal release
Release window: 2026-07-11 to 2026-07-12

## Product Claim

DS Agent v0.1.0 should be released as a Windows-first local desktop agent for
DeepSeek users who need practical office help: summarize local evidence, draft
management briefings, create inspectable local artifacts, continue project
context with auditable memory, and keep local file actions behind visible
permission and audit boundaries.

The public introduction should state DS Agent's strength positively: it turns
local evidence into reviewable office outputs, keeps context receipts and
memory feedback visible, and gives users local artifacts they can inspect,
continue, package, and correct.

## Must Pass

- Windows installer downloads, installs, launches, and opens the DS Agent shell.
- DeepSeek readiness is visible without exposing the API key.
- Operations Briefing creates local Markdown, HTML, lightweight PDF, and
  work-package JSON from sample evidence.
- Office artifact creation/opening does not trigger Office repair prompts in
  the validated path.
- Context Receipt shows selected evidence, memory, route, token/cache state,
  validation, and omissions.
- Memory remains bounded and auditable: selected-memory receipts, feedback,
  quality scoring, background update/archive/merge, and no silent model-owned
  writes.
- Local mutating actions stay behind capability policy, path validation,
  confirmation rules, and append-only audit records.
- Release notes disclose unsigned installer status and current limits.
- GitHub release has installer plus SHA-256 checksum.

## Should Pass

- A first-time user can complete workspace setup without reading developer
  docs.
- The main README gives three copy-paste office task examples.
- The first-run chat workbench shows office starter prompts and clicking one
  fills the composer in a real browser smoke test.
- The installed-app smoke covers the final release tag.
- The release-local gate passes with live DeepSeek and installed workflow
  checks.

## Hero Scenario Evidence

Record the final evidence before publishing `v0.1.0`.

| Scenario | Evidence Required | Status | Notes |
| --- | --- | --- | --- |
| Operations Briefing | Local Markdown, HTML, PDF, and work-package JSON paths plus release-local output | passed 2026-07-09 | Installed workflow smoke passed standalone with run `424294d0-60af-490a-9366-e5b28213239c`; exported Markdown, HTML, PDF, and `deepseek-agent-os-work-package-424294d0-60af-490a-9366-e5b28213239c.json`; DeepSeek telemetry observed `deepseek-v4-flash`, cache `miss`, 393 tokens. Final strong `test:release-local -- --require-live-deepseek --include-installed-workflow` also passed with installed workflow run `d46e54e0-c4c2-46f4-af58-33f99f63efdd`, work-package JSON, live DeepSeek smoke, and installed memory-maintenance smoke. |
| Office Artifact Assistant | At least one opened office artifact without repair prompts, or an explicit release-note limitation | passed 2026-07-09 | Installed Office artifact smoke passed through `resume_agent_chat_action`: created `office/office-artifact-smoke.docx`, Word COM opened it successfully, `word_open_ok: true`, 2 paragraphs, 140 text chars, settings and app-data events restored. Rust Office-focused tests also passed: 23 passed, 0 failed with `CARGO_TARGET_DIR=D:\codex-target\ds-agent-office-test`. |
| Memory-Aware Continuation | Context Receipt memory reasons, feedback controls, and maintenance smoke evidence | passed 2026-07-09 | `node --test scripts/agent-context-receipt.test.mjs` passed 5 checks for compact receipts, inspector wiring, selected-memory feedback review, and maintenance audit. Installed `--memory-feedback` smoke created one accepted memory and recorded `useful` feedback without mutating memory records. Installed `--memory-maintenance` smoke auto-created/applied 1 update, auto-archived 1 stale memory, and second run was idempotent with app-data events restored. |
| First-Run Office Prompts | `test:frontend-starter-prompts` output with prompt count, click result, and screenshot path | passed 2026-07-09 | Real Edge/CDP smoke loaded `DS Agent`, found 3 zh office prompts, clicked the briefing prompt, filled the composer with `根据我的证据文件夹，生成一份经营简报。`, and emitted a temp screenshot path. |

## Formal Release Gate

Run these before committing the final release checkpoint:

```powershell
npx pnpm@9.15.9 test:release-source
npx pnpm@9.15.9 test:frontend-starter-prompts
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
git diff --check
npx pnpm@9.15.9 test:release-local -- --require-live-deepseek --include-installed-workflow
```

Latest pre-publish gate status on 2026-07-09: all commands above passed. The
strong release-local gate ran the full project test, whitespace checks,
source-only release guard, helper self-tests, Windows local Operations Briefing
smoke, DeepSeek Chat smoke, DeepSeek Operations Briefing smoke, installed UI
workflow smoke, and installed UI memory-maintenance smoke.

Build the final Windows installer with:

```powershell
$env:CARGO_TARGET_DIR = 'D:\codex-target\ds-agent-v0.1.0-release'
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --config src-tauri/tauri.windows.conf.json
```

## Do Not Block v0.1.0

- Real email connector.
- Real cloud-drive connector.
- macOS release.
- Code signing.
- New memory features beyond bug fixes.
- Broad Computer Use automation.

## Post-Release Backlog

- macOS validation and packaging.
- Code signing strategy.
- Real connector strategy for email and cloud drive.
- Additional workflow packs after the Windows office-work path is stable.
- Broader Computer Use automation.
- Optional public benchmark comparing DS Agent's memory and office-work flows
  against selected open-source agent patterns.
