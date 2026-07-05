# DeepSeek Agent OS

Independent local-first desktop Agent OS for DeepSeek model support.

## Project Introduction / 项目介绍

### English

DeepSeek Agent OS is packaged locally as DS Agent. It is an independent
open-source desktop project written to help colleagues use DeepSeek large
language models more conveniently in daily work. It focuses on local desktop
agent workflows, permissioned tools, auditable memory, source traceability,
local files, and operations workflow packs.

This project is not an official DeepSeek product, is not affiliated with
DeepSeek, and does not claim any DeepSeek ownership, authorization, or
endorsement. The DeepSeek name is used only to describe compatibility and
support for DeepSeek models.

The project is shared publicly under the Apache-2.0 license. Friends,
colleagues, and anyone who finds it useful are welcome to use it, study it,
fork it, and adapt it within the license terms. When convenient, please open an
issue, discussion, or pull request with suggestions, corrections, criticism, or
other feedback. All advice is welcome.

This project also benefits from the broader GitHub open-source ecosystem. Its
architecture and engineering direction were informed by public open-source
work in desktop apps, agent tooling, workflow systems, permission design,
auditing, and local-first software. We sincerely thank the founders,
maintainers, and contributors of those open-source projects. Their work makes
projects like this possible.

### 中文

DeepSeek Agent OS 本地安装名为 DS Agent，是一个独立开源的桌面端项目。写这个项目
的初衷，是为了让同事们在日常工作中更方便地使用 DeepSeek 大模型。项目重点放在本地
桌面 Agent 工作流、可授权工具、可审计记忆、来源追溯、本地文件和经营管理工作流包。

本项目不是 DeepSeek 官方产品，也不隶属于 DeepSeek；项目不主张任何 DeepSeek 的所有
权、授权或官方背书。这里使用 DeepSeek 名称，只是为了说明项目面向 DeepSeek 模型做
兼容和支持。

本项目按照 Apache-2.0 许可证对外开源。有需要的同事、朋友和开发者，都可以在许可证
范围内自由使用、学习、fork 和改造。大家方便或有空的时候，也欢迎通过 issue、讨论或
pull request 提出意见建议；批评指正都非常欢迎。

这个项目也受益于 GitHub 上广泛的开源生态。项目在桌面应用、Agent 工具、工作流系
统、权限设计、审计记录和本地优先软件等方面，学习和借鉴了公开开源项目中的工程经验
和架构思想。我们真诚感谢这些开源项目的创建者、维护者和贡献者。正是他们长期开放和
分享，才让类似项目有机会继续向前。

No private, leaked, or non-authorized source code should be copied into this
repository. Public open-source references are used as learning material and
engineering inspiration, with respect for their licenses and maintainers.

本仓库不应复制任何私有、泄露或未授权代码。公开开源项目仅作为学习材料和工程参考，
并尊重原项目许可证和维护者权益。

## 0.1.0 Status / 0.1.0 状态

Version `0.1.0` is the current Windows-first release-candidate line. The
codebase is not a finished agent product yet. The current Windows
build/install/launch/run path is verified through the repeatable release gate
and installed UI workflow smoke before each prerelease. The `v0.1.0` candidate
prereleases include a Windows NSIS installer for ordinary colleagues to
download and test.

After the Windows preview continues to pass local release gates, the next
platform target is macOS. A macOS Tauri packaging config already exists in the
repository, but macOS validation and release work will follow after the Windows
preview continues to pass local release gates.

`0.1.0` 是当前 Windows 优先 release-candidate 版本线，还不是完整成熟的 Agent 产品。
每次 prerelease 前，当前 Windows 构建、安装、启动和运行路径都要通过本地 release gate
与 installed UI workflow smoke 验证。`v0.1.0` candidate prerelease 附带 Windows
NSIS 安装包，方便普通同事直接下载测试。

Windows 预览版持续通过本地 release gates 后，下一步会推进 macOS 版本。仓库里已经保
留了 macOS 的 Tauri 打包配置，macOS 的验证和发布会在 Windows 预览版持续通过本地
release gates 后推进。

License: Apache-2.0.

The public `v0.0.1` release remains unchanged. The `v0.1.0` prerelease line is
the first Windows installer candidate line for colleague testing. The installer
is unsigned, so Windows may show an unknown-publisher warning, but the NSIS
package is built with the Microsoft WebView2 bootstrapper embedded and run
silently so ordinary Windows users do not need a developer toolchain.

## Basic Functions / 基本功能

### English

The current codebase is intended to provide these basic functions:

- Windows desktop shell built with Tauri, React, TypeScript, and Rust.
- Local-first workspace setup for a workspace folder, evidence folder, and
  export folder.
- DeepSeek route readiness through a local `DEEPSEEK_API_KEY` environment
  variable without storing or showing the key value.
- Optional local DeepSeek smoke tests for Chat Completions and Operations
  Briefing synthesis.
- Permissioned tool surfaces for file, network, browser, terminal,
  local-folder read/export, email read/draft/send approval records, and
  Computer Use operations.
- Audited Windows local filesystem mutations from chat for explicit file and
  directory create, update, delete, and rename requests.
- Append-only local audit records for access requests, approvals, tool
  attempts, workflow runs, memory records, and work packages.
- Memory Studio for reviewable memories, edits, deletion, expiration, linked
  memory title/body search, linked memory search match source, linked memory
  relation notes, manual existing-memory links, and explicit conflict handling.
- Operations Briefing workflow:
  - Reads local evidence and drafts a management brief.
  - Can use DeepSeek synthesis when configured.
  - Exports Markdown, HTML, lightweight PDF, and work-package JSON to local paths.
  - Shows context receipts with loop mode, workflow policy, selected evidence,
    validation, and intentional omissions.
  - During work-package imports, previews new/skipped workflow templates,
    new/skipped pending memory candidates, and new/skipped archived briefing runs.
  - Keeps imported archived runs as read-only replay details while redacted
    source-machine evidence handles stay visible as a safety boundary.
  - Uses blank operator templates under
    `docs/templates/operations-briefing-evidence/` for local evidence-folder
    seeding.
- Windows NSIS installer build path for local validation and RC distribution,
  including an embedded Microsoft WebView2 bootstrapper.

Current limits are intentional: real mailbox connectors, real cloud-drive
connectors, automatic local bridge-service management, hosted sync, broad
plugin execution, and polished signed installers are not complete in `0.1.0`.

### 中文

当前代码库的基础功能目标如下：

- 基于 Tauri、React、TypeScript 和 Rust 的 Windows 桌面应用外壳。
- 本地优先的工作目录设置：首次只选择一个工作目录，证据、导出、报告、运行记录和工作包等子目录由 DS Agent 自动维护。
- 通过本地 `DEEPSEEK_API_KEY` 环境变量检测 DeepSeek 可用性，但不保存、不展示密钥
  明文。
- 可选的本地 DeepSeek 联调脚本，用于 Chat Completions 和经营简报合成验证。
- 面向文件、网络、浏览器、终端、本地文件夹读取/导出、邮件读取/草稿/发送审批记录和
  Computer Use 的权限化工具入口。
- 从聊天中执行经过审计的 Windows 本地文件系统变更，包括明确提出的文件/目录创建、
  修改、删除和重命名。
- 本地追加式审计记录，用于记录授权请求、审批、工具调用、工作流运行、记忆记录和工
  作包。
- Memory Studio，用于记忆候选、编辑、删除、过期、关联记忆标题/正文搜索、关联记忆搜索命中来源、
  关联说明、手动关联已有长期记忆和冲突处理。
- Operations Briefing 经营简报工作流：
  - 读取本地证据并生成管理简报。
  - 配置 DeepSeek 后可调用模型合成。
  - 将 Markdown、HTML、轻量 PDF 和 work-package JSON 导出到本地路径。
  - 展示上下文回执，包括循环模式、工作流策略、选入证据、验证结果和有意省略内容。
  - 导入工作包时预览新增/跳过的工作流模板、待审核记忆候选和归档简报运行。
  - 导入归档运行保持只读回放详情，同时保留已清理的源机器证据句柄作为安全边界。
  - 使用 `docs/templates/operations-briefing-evidence/` 下的空白运营模板进行本地证据目录初始化。
- Windows NSIS 安装包构建路径，便于本地验证和 RC 分发，并内置 Microsoft WebView2
  bootstrapper。

当前限制也要说清楚：`0.1.0` 还没有完成真实邮箱连接器、真实云盘连接器、自动安装或管理本地桥接服务、
云同步、广泛插件执行和正式签名安装包。这一版先把 Windows 本地可运行和 DeepSeek 基础支持打牢。

Read first:

- `docs/INSTALLATION.md`
- `docs/OPEN_SOURCE_RELEASE.md`
- `docs/RELEASE_NOTES_v0.1.0.md`
- `CONTRIBUTING.md`
- `SECURITY.md`
- `.env.example`

Maintainer handoff notes, decision logs, and internal planning files are kept as
local-only continuation material and are intentionally excluded from public
source snapshots.

## Development

Desktop source commands:

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 test
npx pnpm@9.15.9 tauri:dev
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --debug
npx pnpm@9.15.9 dev
```

Use `tauri:dev` for the real DS Agent desktop window. The root `dev` command
starts only the Vite web preview and is useful for frontend layout work, but it
does not provide the Tauri command bridge that the chat workflow uses. On
Windows, `tauri:dev` automatically keeps Rust build output under the system
temporary directory when `CARGO_TARGET_DIR` is not set, avoiding MinGW path
parsing failures when the source checkout path contains spaces.

`pnpm test` runs the repository secret scan, desktop frontend build, and Rust
tests. The scan covers tracked files plus unignored new files. On Windows, the
test helper automatically keeps Rust build output out of the repository path to
avoid local MinGW path parsing issues when the checkout path contains spaces.
Set `CARGO_TARGET_DIR` yourself only when you need a specific build cache
location.

To run only the repository secret scan before committing or pushing:

```powershell
npx pnpm@9.15.9 test:secrets
```

Before any publication decision for a new prerelease, run the local
release-candidate gate:

```powershell
npx pnpm@9.15.9 test:release-local
```

This runs the full project test, working-tree and staged diff whitespace checks
(`git diff --check` and `git diff --cached --check`), and the source-only
release guard. The source-only guard checks version/name consistency, required
release docs, generated WebView2 loader ignore coverage, Windows WebView2
bootstrapper packaging config, and currently tracked or unignored files for
accidental installer/binary release artifacts, local runtime artifacts,
generated workflow exports, unexpected binary files, oversized source files,
and stale smoke-test release labels.
The local gate also runs deterministic helper checks for the Windows local
Operations Briefing smoke helper, the installed UI helper, and the release-local
helper itself; the Windows local helper self-test does not call DeepSeek or read
local secrets.
When `DEEPSEEK_API_KEY` is configured locally, the gate also runs the Windows
local Operations Briefing smoke test plus both DeepSeek live smoke tests. Use
`npx pnpm@9.15.9 test:release-local -- --skip-live-deepseek` for an offline
source-only pass. Use
`npx pnpm@9.15.9 test:release-local -- --include-installed-ui` when a Windows
DS Agent install already exists and you also want to smoke-test the installed
WebView2 UI. Use
`npx pnpm@9.15.9 test:release-local -- --include-installed-workflow` for the
stronger installed-app workflow smoke, which exercises the installed app command layer,
Operations Briefing run, and Markdown/HTML/PDF exports through the installed
app.
When a local DeepSeek test key and installed Windows app are available, use the
strongest local gate before that publication decision:

```powershell
npx pnpm@9.15.9 test:release-local -- --require-live-deepseek --include-installed-workflow
```

To run only the source-only release guard:

```powershell
npx pnpm@9.15.9 test:release-source
```

`test:deepseek` is an optional local smoke test. It reads `DEEPSEEK_API_KEY`
from the local environment, calls DeepSeek Chat Completions, and prints only
secret-safe metadata such as model, finish reason, and token usage. It is not
used by GitHub CI:

```powershell
npx pnpm@9.15.9 test:deepseek
```

`test:deepseek:briefing` is the optional local workflow smoke test. It sends the
Operations Briefing smoke sample evidence manifest from
`docs/templates/operations-briefing-smoke-evidence` to DeepSeek, validates that
the response matches the expected workflow result format, and prints only counts and
token metadata by default. The bundled smoke files are marked as
`SMOKE SAMPLE evidence for local verification only` and
`Replace before operational use`. Replace them before pointing the workflow at
real business evidence. When a custom evidence directory is an absolute local
path, the script redacts that path from both the prompt and the output:

```powershell
npx pnpm@9.15.9 test:deepseek:briefing
```

`test:windows-installed-ui` is an optional local Windows smoke test for the
installed app. It launches `ds-agent.exe` with a temporary WebView2 DevTools
port, checks that the installed UI loads at `tauri.localhost`, confirms the
desktop command layer is available, and writes a screenshot under the OS temp directory:

```powershell
npx pnpm@9.15.9 test:windows-installed-ui
```

Add `-- --workflow` to run a stronger installed-app workflow smoke. It saves
temporary local directory settings, seeds Operations Briefing evidence templates,
runs the briefing, exports Markdown/HTML/PDF reports, and restores the original
local directory settings file and app-data event store. When
`DEEPSEEK_API_KEY` is configured, it also requires a newly recorded DeepSeek
telemetry event from the installed app process:

```powershell
npx pnpm@9.15.9 test:windows-installed-ui -- --workflow
```

Windows builds automatically merge `apps/desktop/src-tauri/tauri.windows.conf.json`
and produce an NSIS installer under the configured Cargo target directory, for
example `release/bundle/nsis/DS Agent_0.1.0_x64-setup.exe`. The Windows config
embeds the Microsoft WebView2 bootstrapper and runs it silently during install
when the target machine needs the WebView2 runtime.

macOS builds have a separate platform config at
`apps/desktop/src-tauri/tauri.macos.conf.json` for `.app` and `.dmg` packaging.
Run the same Tauri build command on a macOS host to produce those bundles.

## Installation And Local Directories

The app is installed like a normal desktop app, but the installation directory is
not used as the workspace or data directory.

- Program files live in the installer-selected application location.
- App state, SQLite events, logs, and local settings live under the OS-provided
  app data directory.
- First run asks for one workspace. Evidence, exports, reports, runs, work
  packages, and related artifact folders are managed automatically under that
  workspace.
- File, folder, Drive-local, evidence, and export-package paths remain runtime
  user inputs on the user's own machine.
- Local directory settings are stored as `local-directories.json` under the app
  data directory.
- Windows alpha packaging uses a Tauri NSIS installer. macOS packaging uses a
  separate `.app`/`.dmg` config, so Windows installer choices do not block macOS
  distribution.

## Architecture

Harness architecture v1 runs through a stable Agent OS Kernel plus Workflow
Packs. Loop engineering lives in the product surface: it uses permissioned tool
boundaries, source-linked evidence, bounded workflow runs, selective context
assembly, and token-efficient DeepSeek routing. The runtime keeps context
focused instead of loading every available source into each request, so users
get faster feedback while deeper workflows can still verify their evidence. The
first public preview brings the desktop shell, local event history, policy
model, and DeepSeek route model into one buildable Windows app.

The model boundary is explicit: DeepSeek handles open-ended reasoning,
understanding, planning, and generation, while DS Agent handles deterministic
local preflight, protocol context, permission checks, workspace structure, tool
execution, audit records, and artifacts. Model-returned actions are proposals
until DS Agent validates them against local policy. See
[`docs/AGENT_MODEL_BOUNDARY.md`](docs/AGENT_MODEL_BOUNDARY.md).

Context receipts show loop mode, workflow policy, selected evidence, memory,
model route, token/cache state, validation results, and intentional omissions.
Markdown and HTML report exports carry the same context receipt summary.
Bounded repair loops rerun only the failed step with the smallest useful
context, so DS Agent can keep ordinary tasks responsive while still leaving a
reviewable trail for longer workflow runs.

Central chat tasks now carry a goal loop contract through the run: DS Agent
packages the user's real goal, constraints, done-when criteria, completion
verifier, stop conditions, and near-miss guardrails before asking DeepSeek for
reasoning. Local, browser, file, Office, and tool outcomes are treated as
complete only when DS Agent can observe evidence that matches the user's goal.
If the user adds supplementary guidance during a running task, DS Agent folds it
into the same task at the next small node and keeps the right-side run status in
sync. Completed or partially completed results can include one short,
task-grounded next-better suggestion.

The current 0.1.0 preview includes the permission loop for built-in local
tools. Built-in local tools cover file, network, browser, email approval
records, local folders, terminal diagnostics, and Computer Use surfaces; access
requests are evaluated through policy, persisted as append-only events, and
resolved through a visible approval queue in the desktop inspector. The first
tool paths let users browse URLs for source evidence, run source-linked web
search, preview local text files and evidence folders, create approved
workspace files, run allowlisted read-only terminal diagnostics, record
approval and audit decisions for mutating terminal and browser-form actions,
record approval and audit decisions for email read/draft/send flows, scan local
folders, export work-package JSON to local folders, capture approved screen
evidence, execute approved local mouse/keyboard actions behind an unlock
window, and inspect recent tool output.

Permission review clarity v1 keeps high-impact actions explicit. Outbound email
and desktop control approvals authorize only the next matching attempt, while
lower-risk approvals can stay reusable when the selected access mode allows it.

Permission state visibility v1 shows whether a grant is reusable, ready for one
use, or already spent, so operators can understand current access without
reading audit internals.

Approval decision traceability v1 keeps high-impact retries tied to the approval
that authorized them and keeps earlier audit history readable. Recent tool
output gives operators a clear path from action back to decision when an
approval record is present.

Tool route settings v1 shows the selected model route and available tool paths
in the runtime inspector. Web search follows the selected model route, uses a
configured local bridge service when it can return source-linked results, and
otherwise requires a selected source-linked web-search route before live search
can run. The current preview keeps email read, draft, and send as approval and audit
surfaces. It also keeps local folders and export packages separate from cloud
accounts, while screen inspection and computer control use the configured bridge
or local Windows/macOS route. No API key or account credential is stored by this
settings slice.

Setup directory clarity v2 keeps program files and app data separate from the
user-selected workspace. DS Agent stores that single setup choice in the current
user's app data directory, uses a native folder picker for the workspace, and
automatically manages evidence, exports, reports, runs, sources, work packages,
memory, and logs under that workspace.

Windows packaging clarity v1 builds the local Windows preview as an NSIS
installer. It keeps macOS packaging configured but pending verification on a
macOS host.

DeepSeek credential status v1 reads only whether `DEEPSEEK_API_KEY` is present in the local process environment and shows that status in the runtime inspector with the API base URL. DeepSeek Chat API readiness v1 also reports the derived Chat Completions endpoint, selected Flash/Pro model names, and whether local chat-completion requests are ready based on key presence. The key value is never returned to the UI, never serialized into events, and never included in exported work packages.

DeepSeek model request path v1 calls the official Chat Completions endpoint when a local API key is configured. It keeps automated tests offline, redacts local API keys from request errors, and keeps source-linked web search evidence on dedicated routes that require source URLs instead of treating plain chat completions as verified web evidence.

DeepSeek cache and usage visibility v1 keeps Operations Briefing synthesis responsive with an in-session request cache and secret-safe usage records. The runtime inspector shows the latest DeepSeek call status, cache hit/miss state, elapsed time, token usage when the provider returns it, estimated cost when local pricing is configured, current cache size, and a clear-cache action. Local pricing stays in the user's app data directory, and public source does not hardcode live DeepSeek prices.

Web search evidence clarity v1 shows the selected search route before a web
search runs. The app uses source-linked search when the selected model route
cannot provide verified web results, requires source URLs before search output
is treated as evidence, keeps approval gates in place and avoids live network
requests while approval is pending. Reserved alpha presets disclose when they
share the same local search implementation until separate local-browser or
aggregator routes are ready.

Optional local web-search bridge readiness v1 uses only a configured local loopback
bridge for supported providers and maps returned source URLs into the same
evidence and audit trail, so ordinary chat-completion text is not treated as web
evidence.

Desktop automation route clarity v1 shows whether screen inspection and desktop
control will use the selected local route or a configured local bridge. It keeps
desktop control visibly approval-gated before any mouse or keyboard action can
run, and it does not silently switch routes when a configured bridge is
unavailable.

Desktop prerequisite clarity v1 shows local screen and input prerequisites
before a tool runs. macOS lists Screen Recording and Accessibility requirements,
while Windows calls out foreground-desktop and secure-desktop limits instead of
pretending every window can be inspected or controlled.

Local bridge evidence safety v1 only accepts local loopback bridge endpoints. It keeps
screen evidence, control actions, and source-linked web search inside the same
approval and audit path, stores returned screen evidence in the selected evidence
folder, and does not expose local file paths through bridge responses.

Work-package readiness summary v1 adds a secret-safe readiness snapshot to
exported work packages. It shows whether DeepSeek requests, source-linked web
search, desktop automation, local folders, and selected tool routes are ready.
The snapshot is built without storing API keys, user machine paths, running live
model calls, capturing screens, or controlling the desktop during export.

Audited desktop input safety v1 keeps real desktop input inside a small reviewed action set, keeps desktop control experimental and visibly gated, and requires a one-shot approval plus a local in-memory unlock code before any mouse or keyboard action can run.

Local computer control unlock window v1 adds a short local unlock window after approval. The local operator unlocks computer control for five minutes in the inspector, computer control stays locked when the local unlock window is not active, and the unlock code stays out of audit events and exported work packages.

Computer tool route selection v1 keeps screen inspection and desktop control on
the route users see before approval. It reports an unavailable bridge as a clear
error, keeps DeepSeek and custom-model routes on local Windows/macOS desktop
automation paths, and avoids silent route switching before an approved screen or
control action runs.

Screen evidence clarity v1 keeps approved screenshots as local evidence files
with readable audit references. It keeps pending and failed attempts in the
approval trail so users can see why screen inspection did not run.

Screen inspection consent v1 treats screen capture as a sensitive desktop read.
It runs screen capture without an extra prompt in the default full-access mode,
while medium-risk reads remain policy-evaluated in limited automation mode.

Local screenshot storage clarity v1 saves approved PNG screenshots under
`computer-screenshots/`. It uses the selected evidence folder, or app data before
first-run setup, and records portable relative references for export and audit.

Operations Briefing clarity v1 turns approved local evidence into a management
brief. It drafts a summary, anomaly leads, and action items from approved local
evidence, can use DeepSeek synthesis when configured and falls back to a local
draft with a visible warning.

Report export clarity v1 exports Markdown, standalone HTML, lightweight PDF,
and work-package JSON through an approved local export flow. Markdown and HTML
preserve full Unicode, while the preview PDF stays lightweight and ASCII-safe.

Briefing handoff safety v1 keeps imported briefing runs read-only and
reviewable, keeps source-machine evidence handles redacted in exported work
packages, keeps pending memory candidates reviewable, and keeps resolved memory
candidates out of exported packages. For handoff safety, exported work packages
redact source-machine evidence handles, and resolved memory candidates stay out
of exported work packages. Evidence-template seeding uses blank operator
templates without overwriting existing local evidence files.

Memory review clarity v1 keeps memory writes explicit and reviewable. It lets
users propose, accept, reject, edit, expire, and delete long-term memories.

Memory conflict clarity v1 surfaces likely overlaps before acceptance and
supports link, merge, and replace decisions with inspectable relation notes.
Linked memory title/body search, linked memory search match source, and linked
memory relation notes stay visible so related records are understandable during
review. Manual existing-memory links let users connect already accepted
long-term memories without waiting for a new candidate conflict.

Memory import safety v1 keeps imported memory candidates in local review without
writing long-term memory until accepted. It ensures imported memory candidates
drop source-machine source links, and package-internal duplicate ids are counted
as skipped. Older task-derived memories remain readable without exposing local
source links.
