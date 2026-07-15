# DeepSeek Agent OS (DS Agent / DSAgent)

Local-first DeepSeek AI work platform for background tasks, permissioned tools,
auditable evidence, and verifiable local execution.

Latest stable: [DS Agent v0.9.0](https://github.com/Lee-take/dsagent/releases/tag/v0.9.0)

Previous stable: [DS Agent v0.8.0](https://github.com/Lee-take/dsagent/releases/tag/v0.8.0)

Historical prerelease: [DS Agent v0.8.0-rc.1](https://github.com/Lee-take/dsagent/releases/tag/v0.8.0-rc.1)

Search aliases: DS Agent, DSAgent, dsagent, DeepSeek Agent OS.

中文搜索别名：DS Agent、DSAgent、dsagent、DeepSeek Agent OS。

## Two Audiences / 两类读者入口

### For Everyday Users / 面向普通用户

DS Agent helps DeepSeek users finish practical office work on a local Windows
desktop. You can choose a workspace, point DS Agent at local evidence, and ask
in chat for work that normally takes repeated copying, checking, formatting,
and rewriting.

Typical tasks include:

- Turn a folder of evidence into a management briefing, HTML/PDF report, or
  reusable work package.
- Start longer Agent work as a background run while keeping the composer
  available for follow-up instructions and queued next steps.
- Summarize meeting notes into action items, owners, deadlines, risks, and
  follow-up drafts.
- Create local Office-style artifacts from local files while keeping output
  paths visible.
- Continue a project from prior context and show which memories were used.
- Review run state, cancellation state, and audit records instead of waiting
  inside one blocked chat turn.
- Keep local file actions behind visible permission, validation, and audit
  records.

After installing DS Agent, the necessary setup is intentionally simple: set a
local `DEEPSEEK_API_KEY` and choose one local workspace folder. If you want DS
Agent to use live network search, add one more search-capable route: either a
large-model key/provider that can return source-linked web results, or a free
source-linked search option when available.

DS Agent 面向普通 DeepSeek 用户，目标不是让用户学习复杂开发工具，而是帮助他们把日
常办公任务做完：选择一个本地工作目录，把证据文件放进去，然后直接用聊天提出任务。

常见场景包括：

- 把一组本地证据整理成经营简报、HTML/PDF 报告或可复用工作包。
- 让较长 Agent 任务在后台运行，同时继续输入后续要求，并把后续指令排队。
- 把会议纪要整理成行动项、责任人、截止时间、风险提示和后续草稿。
- 从本地文件生成可检查的 Office 类办公产物，并清楚显示输出路径。
- 延续上次项目，并说明本次用了哪些记忆。
- 查看任务状态、取消状态和审计记录，而不是被单次阻塞聊天卡住。
- 对本地文件动作保持可见授权、校验和审计记录。

安装 DS Agent 后，必要设置刻意保持简单：设置本地 `DEEPSEEK_API_KEY`，再选择一个本
地工作目录即可。如果要使用实时网络搜索功能，再额外配置一条具备搜索能力的路线：可
以是支持返回来源链接的大模型 key/服务，也可以是在可用时选择免费的来源型搜索选项。

### For Practitioners / 面向专业人员

DS Agent is a local agent harness around DeepSeek. It brings model replies,
structured actions, local execution, evidence receipts, artifacts, and recovery
into one inspectable workflow. The model proposes structured work; DS Agent
owns context assembly, policy checks, local execution, receipts, artifacts, and
recovery.

Technical highlights:

- Agent harness: structured envelopes separate user reply, proposed actions,
  missing prerequisites, confirmations, artifact targets, and memory
  candidates.
- Loop engineering: bounded loops for preflight, context assembly, model
  routing, permission gates, local tool execution, validation, retries, and
  resumable work packages.
- Background Agent runs: user instructions are persisted as queued runs, then
  claimed by a worker that records state, steps, artifacts, cancellation, and
  terminal outcome.
- Safe skill ecosystem foundation: install and execution prepare paths are
  governed by manifest validation, permission declarations, source checks,
  trust state, local audit records, and disable/uninstall controls.
- Auditable memory: selected memories have reasons and receipts; feedback,
  quality scoring, maintenance history, expiration, deletion, and conflict
  review stay inspectable.
- Context receipts: each workflow can show selected evidence, selected memory,
  route decisions, validations, omissions, costs, and output paths.
- Local-first execution: Windows/Tauri desktop runtime keeps workspaces,
  reports, logs, and artifacts inside a user-controlled local boundary.
- DeepSeek-first routing: context is compacted and selected to fit practical
  DeepSeek usage while preserving reviewable evidence.

DS Agent 对专业人员来说，是围绕 DeepSeek 构建的本地 Agent harness。它把模型回复、
结构化动作、本地执行、证据回执、产物落地和失败恢复纳入同一个可检查工作流。模型负
责提出结构化工作建议；DS Agent 负责上下文组装、策略校验、本地执行、证据回执、产物
落地和失败恢复。

技术亮点包括：

- Agent harness：把用户回复、待执行动作、缺失前置条件、确认项、产物目标和记忆候
  选拆成结构化 envelope。
- Loop engineering：把 preflight、上下文组装、模型路由、权限门、本地工具执行、校
  验、重试和可恢复工作包纳入有边界的任务循环。
- 后台 Agent run：用户指令会先作为 queued run 持久化，再由 worker 领取执行，并记录
  状态、步骤、产物、取消和最终结果。
- 安全 skill 生态基础：安装和执行准备会经过 manifest 校验、权限声明、来源检查、信
  任状态、本地审计记录以及禁用/卸载控制。
- 可审计记忆系统：已选记忆有理由和回执，反馈、质量评分、维护历史、过期、删除和冲
  突处理都可检查。
- Context Receipt：工作流可以展示证据、记忆、路线选择、校验、省略项、成本和输出
  路径。
- 本地优先执行：Windows/Tauri 桌面运行时让工作目录、报告、日志和产物留在用户可控
  的本地边界内。
- DeepSeek-first 路由：面向 DeepSeek 的实际上下文预算做选择和压缩，同时保留可复核
  的证据链。

## Project Introduction / 项目介绍

### English

DeepSeek Agent OS is packaged locally as DS Agent and can also be searched as
DSAgent or dsagent. It is an independent open-source desktop project written to
help colleagues use DeepSeek large language models more conveniently in daily
work. It focuses on local desktop agent workflows, permissioned tools,
auditable memory, source traceability, local files, and operations workflow
packs.

Its practical strength is turning local evidence into reviewable office
outputs. DS Agent can read an evidence folder, draft an operations briefing,
export Markdown/HTML/PDF and work-package JSON, create local Office artifacts,
and then show what evidence, memory, route, validation, omissions, and output
paths were used. The goal is to make everyday office work easier to finish,
inspect, continue, and correct.

The first-run setup is deliberately lightweight. For normal DeepSeek-backed
office work, install DS Agent, set `DEEPSEEK_API_KEY`, and choose one local
workspace folder. Network search is optional; when current web information is
needed, configure one additional source-linked search route, such as a
search-capable model/provider key or an available free web-search option.

This project is not an official DeepSeek product, is not affiliated with
DeepSeek, and does not claim any DeepSeek ownership, authorization, or
endorsement. The DeepSeek name is used only to describe compatibility and
support for DeepSeek models.

The project is shared publicly under the Apache-2.0 license. Friends,
colleagues, and anyone who finds it useful are welcome to use it for learning,
research, evaluation, internal pilots, and adaptation within the license terms.
The published source is not presented as a turnkey codebase to repackage
directly as a commercial product or hosted service. If you are evaluating any
commercial use, please respect the Apache-2.0 license, preserve required
notices, avoid implying DS Agent or DeepSeek endorsement, and complete your own
security review, compliance review, testing, signing, and user-support
responsibilities. When convenient, please open an issue, discussion, or pull
request with suggestions, corrections, criticism, or other feedback. All advice
is welcome.

This project also benefits from the broader GitHub open-source ecosystem.
Public open-source work has provided valuable reference points for DS Agent in
desktop apps, agent harnesses, workflow systems, permission design, auditing,
local-first software, and release engineering. We sincerely thank the founders,
maintainers, and contributors of those projects for their generous sharing and
long-term dedication. Their open work gives projects like DS Agent a stronger
foundation to learn from and build on.

### 中文

DeepSeek Agent OS 本地安装名为 DS Agent，也可以用 DSAgent 或 dsagent 搜索到。本项
目是一个独立开源的桌面端项目。写这个项目的初衷，是为了让同事们在日常工作中更方便
地使用 DeepSeek 大模型。项目重点放在本地桌面 Agent 工作流、可授权工具、可审计记
忆、来源追溯、本地文件和经营管理工作流包。

DS Agent 的强项，是把本地证据变成可检查、可继续、可交付的办公产物。它可以读取证
据文件夹，生成经营简报，导出 Markdown/HTML/PDF 和工作包 JSON，创建本地 Office
文档，并说明本次任务用了哪些证据、记忆、模型路线、校验、省略项和输出路径。我们的
定位是让日常办公任务真的能被完成、复核、延续和纠偏。

首次使用的设置也尽量简单。普通 DeepSeek 办公任务只需要安装 DS Agent，设置本地
`DEEPSEEK_API_KEY`，再选择一个本地工作目录。网络搜索是可选增强；如果任务需要实时
联网信息，再额外配置一条能够返回来源链接的搜索路线，例如具备搜索能力的大模型
key/服务，或在可用时选择免费的来源型搜索选项。

本项目不是 DeepSeek 官方产品，也不隶属于 DeepSeek；项目不主张任何 DeepSeek 的所有
权、授权或官方背书。这里使用 DeepSeek 名称，只是为了说明项目面向 DeepSeek 模型做
兼容和支持。

本项目按照 Apache-2.0 许可证对外开源。我们欢迎有需要的同事、朋友和开发者在许可证
范围内用于学习、研究、评估、内部试用和合规二次开发参考。但当前公开源代码不是作为
可以直接包装成商业产品或托管服务的成品代码库来发布；我们也不希望下载用户未经独立
评审、合规处理和工程加固，就直接拿当前源代码做商业化交付。如果评估任何商业场景，
请遵守 Apache-2.0 许可证，保留必要声明，不暗示 DS Agent 或 DeepSeek 官方背书，并自
行完成安全审查、合规审查、测试、签名和用户支持责任。大家方便或有空的时候，也欢迎
通过 issue、讨论或 pull request 提出意见建议；批评指正都非常欢迎。

这个项目也受益于 GitHub 上广泛的开源生态。公开开源项目在桌面应用、Agent harness、
工作流系统、权限设计、审计记录、本地优先软件和发布工程等方面，为 DS Agent 提供了
大量参考价值、工程经验和架构启发。我们真诚感谢这些开源项目的创建者、维护者和贡献
者，感谢他们长期开放、无私分享和持续奉献。正是这些开源工作，让 DS Agent 这样的项
目能够站在更扎实的基础上继续学习和建设。

No private, leaked, or non-authorized source code should be copied into this
repository. Public open-source references are used as learning material and
engineering inspiration, with respect for their licenses and maintainers.

本仓库不应复制任何私有、泄露或未授权代码。公开开源项目仅作为学习材料和工程参考，
并尊重原项目许可证和维护者权益。

## 0.9.0 Stable Status / 0.9.0 正式版状态

Version `0.9.0` is the current Windows-first stable release. It adds a bounded
Expert Team for complex work: DeepSeek can plan Research, Analysis, Production
and Review roles while DS Agent owns durable attempts, permissions, budgets,
evidence, isolated staging, retries, exact-revision review and the final merge.
It also makes explicit Soul identity and collaboration settings durable in the
same chat turn, with visible receipts and automatic reuse in new conversations.

`0.9.0` 是当前 Windows 优先正式版本。它新增有界“专家团队”：DeepSeek 可以规划
研究、分析、制作与审核分工，DS Agent 负责持久任务、权限、预算、证据、隔离暂存、有限
重试、准确版本审核与最终合并。同时，用户在对话中明确设定或确认的 Soul 身份与协作偏好
会在同一轮安全写入，并通过可见回执在新对话中继续生效。

## 0.8.0 Historical Status / 0.8.0 历史状态

Version `0.8.0` is the previous Windows-first stable release. It added Durable
Verified Computer Use: DS Agent observes and binds the
exact window, target, action and approval; revalidates immediately before one
input effect; captures post-action evidence; and claims success only when a
deterministic semantic postcondition passes. Restart uncertainty becomes
`EffectUnknown` and is never replayed automatically. User takeover stops control
and requires a fresh observation before work can continue.

`0.8.0` 是上一 Windows 优先正式版本。它新增持久、可验证的
Computer Use 闭环：DS Agent 绑定准确窗口、目标、动作和批准，执行前再次校验，只产生
一次输入副作用，随后自动取证，并且只有确定性语义后置条件成立时才声明成功。重启后的
不确定副作用会进入 `EffectUnknown`，不会自动重放；用户接管会立即停止控制，继续前必须
重新观察。

## 0.5.0 Historical Status / 0.5.0 历史状态

Version `0.5.0` is the previous stable Windows release. It introduced the
durable Automation and Artifact Engine foundation. The v0.6 and v0.7 labels were
internal roadmap milestones already consolidated into v0.5.0; no retroactive
public tags are created for them.

`0.5.0` 是上一正式版本，包含持久化 Automation 和 Artifact Engine 基础。
v0.6、v0.7 是已合并进入 v0.5.0 的内部路线图里程碑，不会补建公开标签。

## 0.4.1 Historical Status / 0.4.1 历史状态

Version `0.4.1` was the preceding Windows-first formal release. It includes native
Subagent parallel work: a parent run can split independent work into bounded,
isolated child runs, execute them concurrently, and produce one parent synthesis
without flooding ordinary chat with intermediate child results. The composer
remains usable while the group is running, and durable events preserve claims,
completion, recovery, and audit evidence.

The Skills and Plugins catalog also refreshes automatically after lifecycle
changes, so newly installed, updated, enabled, disabled, or removed capabilities
appear without requiring an app restart.

`0.4.1` 是上一版 Windows 优先正式发布版本。它包含原生 Subagent 并行工作：父任务可以
把相互独立的工作拆成数量受控、上下文隔离的子任务并发执行，完成后只由父任务统一综合，
不会把中间子任务结果刷进普通聊天；并行期间输入框仍可继续使用，领取、完成、恢复和审计
证据都会持久化。

Skill 与插件目录也会在安装、更新、启用、禁用或卸载后自动刷新，无需重启应用。

## 0.3.0 Historical Status / 0.3.0 历史状态

Version `0.3.0` was the preceding Windows-first formal release. It added an
automatic lifecycle for declarative Skills and Plugins: send a public GitHub or
Hugging Face repository URL in chat and DS Agent resolves, validates, installs,
enables, persists, and later selects the capability automatically without asking
the user to choose a package format or repeat the instruction.

Remote installations are compared with their upstream immutable revision every
time DS Agent starts. Compatible updates are validated and committed under the
same local identity; an incompatible or failed update leaves the last working
version active and records the failure for audit. Installed third-party items
can be enabled, disabled, or uninstalled from the Plugins panel.

The Plugins panel separates System Skills, installed Plugins, installed Skills,
and Scenario Templates. The protected `Skill/Plugin Builder` System Skill lets
DS Agent create, validate, activation-test, install, and enable safe declarative
Skills from one user instruction. Operations Briefing is a Scenario Template,
not a Plugin.

Repository installation is limited to controlled public HTTPS GitHub and
Hugging Face sources. Strict redirects, size and path checks, manifest and
permission validation, integrity hashes, declarative-only entry points, and
append-only lifecycle events keep arbitrary scripts, native binaries, unsafe
paths, and undeclared permission expansion outside the runtime.

`0.3.0` 是上一版 Windows 优先正式发布版本。用户只需在聊天中发送公开的 GitHub 或
Hugging Face 仓库地址，DS Agent 会自动完成版本解析、内容识别、安全校验、安装、启用
和持久化，以后再根据任务自动调用，不要求用户选择包格式或重复确认。

每次打开 DS Agent 都会在后台对比远端不可变版本；兼容的新版本自动升级并保持同一本地
身份，失败时继续使用上一可用版本并记录审计。插件页区分系统技能、已安装插件、已安装
Skill 和场景模板，第三方项目可直接启用、禁用或卸载。受保护的 `Skill/Plugin Builder`
系统技能可以根据一次用户指令自动制作、校验、测试和安装声明式 Skill。“运营简报”属于
场景模板，不属于插件。

## 0.2.3 Historical Status / 0.2.3 历史状态

Version `0.2.3` was the preceding Windows-first formal release. It turned DS Agent
from a blocking chat tool into the foundation of a local AI work platform:
DeepSeek handles open-ended understanding and planning, while DS Agent owns
background execution, tool contracts, permissions, sandbox boundaries, state,
evidence, audit, verification, and recovery.

The Windows build/install/launch/run path is verified through the repeatable
release gate and installed UI workflow smoke before publication. After the
Windows preview continues to pass local release gates, the next platform target
is macOS. A macOS Tauri packaging config already exists in the repository, but
macOS validation and release work will follow after the Windows preview
continues to pass local release gates.

Background Agent runs keep the composer usable while work is queued, claimed,
executed, cancelled, and audited. New input can become a separate task, durable
guidance for the active run, or an explicit cancellation request. Run state,
steps, artifacts, guidance, cancellation, and terminal outcomes survive the
chat turn that created them.

`v0.2.3` includes the first restart-safe slice: after an app or process restart,
an expired run can be reclaimed, durable guidance or cancellation is honored,
and an already verified write is not executed twice.

The generic tool runtime gives file, Office, browser, network, Computer Use,
application-update, workflow, and trusted declarative skill actions one
execution contract. Policy checks, exact approval fingerprints, resource locks,
heartbeats, evidence receipts, verification, audit events, replay, and recovery
are handled by DS Agent instead of being left to model text. Conflicting writes
to the same declared resource are serialized rather than hidden.

Skill manifests declare permissions, source identity, integrity, trust state,
execution plans, audit events, and disable or uninstall controls. Trusted
declarative skill entry points can now execute through the same permissioned
runtime as built-in tools. Unsupported native hooks, arbitrary scripts,
undeclared permissions, unsafe paths, and disallowed network destinations fail
closed.

Chat completion summaries now keep internal action identifiers, protocol
fields, raw JSON, and audit receipts out of ordinary user replies. The full
technical evidence remains available in the run inspector and durable audit.

The plugins panel is now a read-only installed-capability catalog: it shows
skill and plugin names, expands a selected name to its description, and leaves
capability selection to DeepSeek. DS Agent continues to validate trust,
verified entry content, permissions, resources, and execution evidence locally.

App updates are downloaded and validated silently. The install button appears
only when the installer is ready, and clicking it installs without a second
permission step. Real Agent approvals remain visible and actionable on the
right-side pending card.

The package version, Tauri version, Cargo version, installer naming, and
updater current-release tag are bumped together to `0.2.3` / `v0.2.3`. This
matters for desktop upgrades: installed Windows clients compare against the
GitHub release tag and installer asset, so a new release must use a new
public tag instead of reusing `v0.2.2`.

`0.2.3` 是当前 Windows 优先正式发布版本。这一版把 DS Agent 从“单次阻塞聊天工具”推
进为本地 AI 工作平台的地基：DeepSeek 负责开放式理解和规划，DS Agent 负责后台执行、
工具契约、权限、沙箱边界、状态、证据、审计、验证和恢复。

正式发布前，当前 Windows 构建、安装、启动和运行路径都要通过本地 release gate 与
installed UI workflow smoke 验证。Windows 预览版持续通过本地 release gates 后，下一步
会推进 macOS 版本。仓库里已经保留了 macOS 的 Tauri 打包配置，macOS 的验证和发布会在
Windows 预览版持续通过本地 release gates 后推进。

后台 Agent run 让输入框和主要操作在任务执行期间继续可用。新输入可以明确成为独立任
务、追加给当前 run 的持久化 guidance，或取消当前任务。运行状态、步骤、产物、追加指
令、取消请求和最终结果不再依附于发起任务的那一次聊天回合。

`v0.2.3` 包含第一段重启安全链路：应用或进程重启后，过期任务可以重新领取，持久化
guidance 或取消请求会被处理，已经验证成功的写入不会重复执行。

通用工具 runtime 为文件、Office、浏览器、网络、Computer Use、应用更新、工作流和受
信任声明式 skill 提供统一执行契约。策略检查、精确审批指纹、资源锁、心跳、证据回执、
结果验证、审计、回放和恢复由 DS Agent 负责；声明写入同一资源的任务会被串行化，不允
许隐藏并发写。

skill manifest 会声明权限、来源身份、完整性、信任状态、执行计划、审计事件以及禁用
或卸载控制。受信任的声明式 skill 入口已经可以通过与内置工具相同的权限 runtime 执
行；不支持的 native hook、任意脚本、未声明权限、不安全路径和不允许的网络目标会按
fail-closed 原则阻断。

普通聊天回复不再暴露动作标识、协议字段、原始 JSON 或审计回执；完整技术证据继续保留
在运行检查器和持久审计中。插件面板改为只读能力目录，只显示已安装技能/插件名称，点击
名称才显示简介；DeepSeek 根据任务选择能力，DS Agent 继续负责本地信任、入口、权限、
资源和执行证据校验。

应用更新会在后台自动下载并校验，准备完成后才显示“安装更新”；点击即安装，不再要求
第二次审批。真正需要审批的 Agent 操作会在右侧“待批准”卡片上直接显示操作按钮。

这次也把 package、Tauri、Cargo、安装包命名和 updater 当前 release tag 一起升级到
`0.2.3` / `v0.2.3`。这对桌面端自动升级很关键：已安装 Windows 客户端会对比 GitHub
release tag 和安装包资产，所以补丁发布必须使用新的公开 tag，不能继续沿用 `v0.2.2`。

License: Apache-2.0.

The public `v0.0.1` through `v0.8.0` releases remain unchanged. `v0.9.0` is the
current Windows stable line. The published `v0.8.0` stable release and
`v0.8.0-rc.1` prerelease remain immutable historical evidence.
The installer is unsigned, so Windows may show an unknown-publisher warning,
but the NSIS package is built with the Microsoft WebView2 bootstrapper embedded
and run silently so ordinary Windows users do not need a developer toolchain.

`v0.9.0` keeps the verifiable Windows Agent foundation and adds bounded expert
collaboration plus explicit cross-conversation Soul persistence. Install DS
Agent, connect DeepSeek through a local environment variable, choose a
workspace, keep tasks moving in the background, and let a permissioned runtime
execute and verify local work with evidence.

`v0.9.0` 延续可验证的 Windows Agent 工作，并新增有界专家协作与明确的跨对话 Soul
持久化：安装 DS Agent，通过本地环境变量连接 DeepSeek，选择一个本地工作目录，让任务
在后台推进，并由权限化 runtime 执行、留证和验证本地工作。

## Why DS Agent / DS Agent 亮点

DS Agent is built for local office work: it packages context for DeepSeek,
validates proposed actions before touching local files, records audit events,
shows receipts for evidence and memory, and exports artifacts under a local
workspace.

DS Agent 是一个 Windows-first 的 DeepSeek 本地桌面 Agent，面向真实办公任务：
读取本地证据、整理会议和行动项、生成经营简报、创建可检查的本地文件，并在项目延
续时使用可审计的记忆。

Highlights:

- DeepSeek-first local desktop experience for Windows users.
- Chat-first office workflows for evidence summaries, management briefings, and
  local artifacts.
- Permissioned local execution: the model proposes, DS Agent validates and
  records.
- Context Receipts that show selected evidence, memory, route, validation,
  omissions, and output paths.
- Auditable memory with feedback, quality scoring, maintenance history, and no
  silent model-owned writes.
- Local-first workspace for files, reports, work packages, logs, and replayable
  artifacts.

亮点：

- 面向 DeepSeek 用户的 Windows 本地桌面体验。
- 从聊天直接进入办公任务：证据整理、经营简报、本地办公产物。
- 权限化本地执行：模型提出动作，DS Agent 校验、执行并记录。
- Context Receipt 展示证据、记忆、路线、验证、省略项和输出路径。
- 可审计记忆：反馈、质量评分、维护历史清楚可查，不做模型黑箱式静默写入。
- 本地优先工作目录：文件、报告、工作包、日志和可回放产物都落在本地边界内。

Try these tasks:

- `根据我的证据文件夹，生成一份经营简报，并导出 HTML 和 PDF。`
- `把这段会议纪要整理成行动项、责任部门、截止时间和风险提示。`
- `继续上次的项目，先说明你用了哪些记忆，再给我下一步建议。`

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
  relation notes, manual existing-memory links, explicit conflict handling, and
  feedback-informed retrieval scoring. Selected-memory feedback stays
  append-only, while DS Agent automatically records retrieval tuning, applies
  audited updates, and archives repeated stale memories in the background.
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
marketplace-style skill execution, stronger third-party sandboxing, and
polished signed installers are not complete in `0.2.3`.

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
- Memory Studio，用于记忆审计、编辑、删除、过期、关联记忆标题/正文搜索、关联记忆搜索命中来源、
  关联说明、手动关联已有长期记忆、冲突审计和反馈驱动检索。Context Receipt 里的已选记忆反馈会参与后续检索评分，
  stale/conflicting/should_update 反馈会压缩成后台维护线索；DS Agent 会自动记录检索调优、应用审计更新，
  并在重复 stale 反馈下自动归档过时记忆。
- 后台 Agent run：用户可以在任务执行期间继续输入，后续指令会进入队列，运行状态、
  取消请求、步骤、产物和最终结果会写入本地审计记录。
- 安全 skill 生态基础：skill 安装和执行准备会校验 manifest、权限声明、来源身份、完
  整性、信任状态和执行计划，并提供信任重置、启用/禁用、卸载和审计记录。
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

当前限制也要说清楚：`0.9.0` 还没有完成真实邮箱连接器、真实云盘连接器、自动安装或管理本地桥接服务、
云同步、公共 skill marketplace 发现与签名分发、任意第三方 native/script 代码的强进程隔离和正式签名
安装包。这一版先把后台 run、通用工具 runtime、权限沙箱、Computer Use、可信声明式 skill 和可验证
执行地基打牢。

Read first:

- `docs/INSTALLATION.md`
- `docs/OPEN_SOURCE_RELEASE.md`
- `docs/RELEASE_NOTES_v0.9.0.md`
- `docs/RELEASE_NOTES_v0.8.0.md`
- `docs/RELEASE_NOTES_v0.8.0-rc.1.md`
- `docs/RELEASE_NOTES_v0.5.0.md`
- `docs/RELEASE_NOTES_v0.4.1.md`
- `docs/RELEASE_NOTES_v0.4.0.md`
- `docs/RELEASE_NOTES_v0.3.0.md`
- `docs/RELEASE_NOTES_v0.2.2.md`
- `docs/RELEASE_NOTES_v0.2.1.md`
- `docs/RELEASE_NOTES_v0.2.0.md`
- `docs/RELEASE_NOTES_v0.1.2.md`
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
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri build --config src-tauri/tauri.windows.conf.json
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

Before any publication decision for a new release, run the local release gate:

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
example `release/bundle/nsis/DS Agent_0.9.0_x64-setup.exe`. The Windows config
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
If the user adds follow-up guidance while a task is running, DS Agent can keep
the composer available and enqueue the next Agent run instead of blocking the
whole workspace. Completed or partially completed results can include one
short, task-grounded next-better suggestion.

The current 0.9.0 stable release includes the permission loop for built-in local tools,
durable background Agent runs, the generic tool runtime, resource coordination,
and trusted declarative skill execution.
Built-in local tools cover file, network, browser, email approval records,
local folders, terminal diagnostics, and Computer Use surfaces; access requests
are evaluated through policy, persisted as append-only events, and resolved
through a visible approval queue in the desktop inspector. Agent run records
follow the same local-first discipline: queued work is persisted, worker claims
are explicit, status transitions are inspectable, cancellation is recorded, and
artifacts are attached to the run history. The first
tool paths let users browse URLs for source evidence, run source-linked web
search, preview local text files and evidence folders, create approved
workspace files, run allowlisted read-only terminal diagnostics, record
approval and audit decisions for mutating terminal and browser-form actions,
record approval and audit decisions for email read/draft/send flows, scan local
folders, export work-package JSON to local folders, capture approved screen
evidence, execute approved local mouse/keyboard actions behind an unlock
window, and inspect recent tool output.

The safe skill ecosystem is deliberately conservative. A skill package must
describe its manifest, source, permissions, integrity, trust state, and
execution plan before DS Agent prepares it for use. Unsupported native hooks,
unsafe scripts, missing source identity, or undeclared permissions are rejected
or held behind review. This is not yet a broad marketplace, but it is the
minimum architecture needed before GitHub, Hugging Face, or other open-source
skill sources can be trusted by ordinary users.

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
packages, keeps memory candidate history local and auditable, and keeps resolved
memory candidates out of exported packages. For handoff safety, exported work packages
redact source-machine evidence handles, and resolved memory candidates stay out
of exported work packages. Evidence-template seeding uses blank operator
templates without overwriting existing local evidence files.

Memory review clarity v1 keeps memory writes explicit and auditable while
moving routine candidate decisions into DS Agent background maintenance. Users
can still propose corrections, edit, expire, and delete long-term memories when
they need to fix the audit trail.
Selected-memory feedback now supports feedback-informed retrieval scoring:
useful feedback can lift later recall, irrelevant, stale, conflicting, or
should_update feedback can lower recall, and stale/conflicting/should_update
feedback surfaces compact review hints without dumping full memory bodies into
receipts.
Automatic Memory maintenance v1 keeps routine memory care out of the user's
workflow. Repeated irrelevant feedback records retrieval tuning, should_update
feedback can create and apply an audited update, and repeated stale feedback can
archive stale memories in the background while leaving an inspectable audit
trail.

Memory conflict clarity v1 surfaces likely overlaps in the audit view and lets
DS Agent apply link, merge, replace, update, archive, or reject decisions in the
background with inspectable relation notes.
Linked memory title/body search, linked memory search match source, and linked
memory relation notes stay visible so related records are understandable during
review. Manual existing-memory links let users connect already accepted
long-term memories without waiting for a new candidate conflict.

Memory import safety v1 keeps imported memory candidates local and auditable
before DS Agent background maintenance resolves them. It ensures imported
memory candidates drop source-machine source links, and package-internal
duplicate ids are counted as skipped. Older task-derived memories remain
readable without exposing local source links.
