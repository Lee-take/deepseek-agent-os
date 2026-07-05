# DeepSeek Agent OS v0.1.0 Candidate Notes

Status: Windows-first release candidate. The `v0.1.0-rc.6` prerelease is
intended for colleague testing through a GitHub release asset after the final
local gates pass.

Packaging: Windows installer prerelease. The GitHub prerelease should attach
the NSIS setup executable and its SHA-256 checksum. The installer is unsigned,
so Windows may show an unknown-publisher warning, but it embeds the Microsoft
WebView2 bootstrapper and runs it silently when the target machine needs the
WebView2 runtime. Ordinary users do not need Node.js, pnpm, Rust, or a source
checkout to run the installed app.

## v0.1.0-rc.6 Update

- Adds task-scoped file and image attachments to the DS Agent chat composer.
- Lets users drag local files onto the input box, keeps the dropped file order,
  removes duplicate paths, and shows compact attachment cards above the input.
- Adds small image thumbnails or file icons for attachments, plus an `x` control
  to remove mistaken files before sending.
- Sends bounded attachment context with the current instruction, including text
  snippets when safe and metadata-only image/file evidence when content is not
  included in DeepSeek context.
- Surfaces attachment evidence in the right-side run status so users can see
  ready, metadata-only, and blocked attachments.

## v0.1.0-rc.5 Update

- Fixes the Windows desktop shortcut and taskbar icon refresh path so installed
  builds use the DS Agent app icon after upgrades instead of keeping a stale
  cached shortcut icon.
- Sets both the normal app icon and the Windows large/small window icons at
  startup, improving taskbar and Alt-Tab icon consistency.

## v0.1.0-rc.4 Update

- Upgrades the central chat run loop so DS Agent sends DeepSeek a goal contract
  context with the user's real goal, constraints, done-when criteria,
  completion verifier, stop conditions, and near-miss guardrails.
- Strengthens completion semantics: local, browser, file, Office, and tool work
  is treated as complete only after DS Agent has observable evidence that
  matches the user's goal, rather than a merely similar model answer.
- Adds bounded loop behavior for ordinary tasks: verification success stops the
  loop quickly, repeated failures switch strategy or report a blocker, and
  missing prerequisites pause the run instead of guessing.
- Supports in-run supplementary guidance as part of the same task. When the user
  adds more detail during a running task, DS Agent queues it for the next small
  node and keeps the right-side run status in sync.
- Adds completion advice: after a completed or partially completed result, DS
  Agent can add one short, task-grounded suggestion for a better next step
  without implying extra work was already performed.
- Updates the right-side run status for chat tasks to show a lightweight goal
  loop: understand the task, call DeepSeek or run local actions, apply any
  supplementary guidance, and validate the result.

## v0.1.0-rc.3 Update

- Adds audited Windows local file actions from chat: create file, update file,
  delete file, and rename file with absolute local paths.
- Adds audited Windows local directory actions from chat: create directory,
  rename directory, and delete directory with absolute local paths.
- Routes these file and directory mutations through the `FileWrite` capability
  boundary so access policy, permission audit records, and capability
  invocations stay visible.
- Fixes the local directory listing path that could leave the UI waiting for a
  second terminal-output step after DeepSeek had already planned a bounded
  directory read.

Maintainer handoff notes, decision logs, and internal planning files are kept as
local-only continuation material and are intentionally excluded from public
source snapshots.

## Positioning

DeepSeek Agent OS is packaged locally as DS Agent. It is an independent open-source desktop project
written to help colleagues use DeepSeek large language models more conveniently
in daily work.

The preview uses a harness architecture with loop engineering implemented in
code: permissioned tool boundaries, append-only audit events, selective context
assembly, source-linked evidence, bounded workflow runs, and token-efficient
DeepSeek routing.

In this preview, context receipts show loop mode, workflow policy, selected
evidence, memory, route, token/cache state, validation results, and intentional
omissions; bounded repair loops keep failed-step retries small so longer work
can stay reviewable without loading the full conversation into every model
request.

Markdown and HTML report exports carry the same context receipt summary.

This project is not an official DeepSeek product, is not affiliated with
DeepSeek, and does not claim any DeepSeek ownership, authorization, or
endorsement. The DeepSeek name is used only to describe compatibility and
support for DeepSeek models.

## Why 0.1.0

The project is not complete. `0.1.0` is intentionally defined as a
Windows-first test candidate so the source tree can move forward without
overstating product maturity. The Windows build/install/launch/run path has
been locally validated through the repeatable release gate and installed UI
workflow smoke.

After the Windows preview continues to pass local release gates, the next
platform target is macOS. The repository already contains a macOS Tauri
packaging config, but macOS validation and release work will follow after the
Windows preview continues to pass local release gates.

## Basic Functions In This Preview

- Tauri + React + TypeScript + Rust desktop shell.
- Local-first workspace setup for a workspace folder, evidence folder, and
  export folder.
- DeepSeek route readiness through a local `DEEPSEEK_API_KEY` environment
  variable without storing or showing the key value.
- Optional local DeepSeek smoke tests for Chat Completions and Operations
  Briefing synthesis.
- Permissioned tool surfaces for file, network, browser, terminal,
  local-folder read/export, email read/draft/send approval records, and
  Computer Use operations.
- Audited Windows local filesystem mutations from chat for explicit create,
  update, delete, and rename requests on files and directories.
- Append-only local audit records for access requests, approvals, tool
  attempts, workflow runs, memory records, and work packages.
- Computer Use remains experimental and high-risk: screen capture follows the
  selected access-mode policy, computer control also needs a one-shot approval
  plus a local unlock code, and desktop automation is still subject to
  foreground desktop, secure desktop, Screen Recording, and Accessibility
  limitations.
- Memory Studio for reviewable memories, edits, deletion, expiration, linked
  memory title/body search, linked memory search match source, linked memory
  relation notes, manual existing-memory links, and explicit conflict handling.
- Operations Briefing workflow:
  - Reads local evidence and drafts a management brief.
  - Can use DeepSeek synthesis when configured.
  - Exports Markdown, HTML, lightweight PDF, and work-package JSON to local
    paths.
  - During work-package imports, previews new/skipped workflow templates,
    new/skipped pending memory candidates, and new/skipped archived briefing
    runs.
  - Import preview: package-internal duplicate ids are counted as skipped.
  - Export safety: resolved memory candidates stay out of exported work
    packages.
  - Export safety: exported work packages redact source-machine evidence
    handles.
  - Import safety: imported memory candidates drop source-machine source links
    before local review.
  - Keeps imported archived runs as read-only replay details while redacted
    source-machine evidence handles stay visible as a safety boundary.
  - Uses blank operator templates under
    `docs/templates/operations-briefing-evidence` for the desktop
    seed-template button and a separate smoke sample folder for live smoke
    tests.
- Local report and package export paths for Markdown, HTML, lightweight PDF,
  and work-package JSON.
- Windows NSIS installer build path for local validation and RC distribution,
  including an embedded Microsoft WebView2 bootstrapper.

## Current Limits

- Real mailbox connectors are not complete.
- Real cloud-drive connectors are not complete.
- Browser form submission and terminal write keep approval and audit records;
  they are not broad automation executors.
- DS Agent does not install or manage local bridge services in this preview.
- Hosted sync, account systems, marketplaces, and arbitrary third-party
  executable plugins are not included.
- The Windows installer is unsigned in this RC, so users may see an
  unknown-publisher warning until signing is added.
- PDF export is lightweight and ASCII-safe. Use Markdown or HTML for Chinese or
  other Unicode report content.

## Open Source Acknowledgement

This project benefits from the GitHub open-source ecosystem. Its architecture
and engineering direction were informed by public open-source work in desktop
apps, agent tooling, workflow systems, permission design, auditing, local-first
software, and the Rust/React/Tauri ecosystem.

We sincerely thank the founders, maintainers, and contributors of those
open-source projects. Their work makes projects like this possible.

No private, leaked, or non-authorized source code should be copied into this
repository. Public open-source references are used as learning material and
engineering inspiration, with respect for their licenses and maintainers.

## Local Verification

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 test
npx pnpm@9.15.9 test:release-local
npx pnpm@9.15.9 test:release-local -- --require-live-deepseek --include-installed-workflow
npx pnpm@9.15.9 test:release-source
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
git diff --check
git diff --cached --check
```

The local release gate also runs deterministic helper checks, including the
Windows local helper self-test, installed UI helper self-test, release-local
helper self-test, and working-tree and staged diff whitespace checks. The
Windows local helper self-test does not call DeepSeek or read local secrets.

Optional live DeepSeek smoke tests:

```powershell
$env:DEEPSEEK_API_KEY = Read-Host "DeepSeek API key"
npx pnpm@9.15.9 test:deepseek
npx pnpm@9.15.9 test:deepseek:briefing
```

The briefing smoke uses the non-sensitive sample evidence under
`docs/templates/operations-briefing-smoke-evidence` by default. Use
`DEEPSEEK_BRIEFING_EVIDENCE_DIR` for another local evidence folder. The bundled
smoke files are marked as `SMOKE SAMPLE evidence for local verification only`
and `Replace before operational use`. Replace them before pointing the workflow
at real business evidence.

Do not commit API keys, `.env` files, local app data, local evidence folders, or
generated installer artifacts.
