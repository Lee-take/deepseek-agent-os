# DS Agent v0.9.0 Release Notes / 正式版说明

Status: Windows-first stable release. Repository:
https://github.com/Lee-take/dsagent

状态：Windows 优先正式版本。项目地址：
https://github.com/Lee-take/dsagent

## Release Identity / 发布身份

Package, desktop, Tauri and Cargo metadata are `0.9.0`, and the updater
identity is `v0.9.0`. Installed `v0.5.0`, `v0.8.0-rc.1` and `v0.8.0` clients
can detect this stable release. A `v0.9.0` client does not treat an equal or
lower version as an update.

package、desktop、Tauri 和 Cargo 元数据均为 `0.9.0`，updater 当前身份为
`v0.9.0`。已安装的 `v0.5.0`、`v0.8.0-rc.1` 和 `v0.8.0` 可以发现本正式版；
`v0.9.0` 不会把相同版本或更低版本当作更新。

Published historical tags and Releases remain immutable. v0.9.0 is a new
commit, annotated tag and installer asset rather than a moved release label.

既有公开 tag 与 Release 保持不可变。v0.9.0 使用新的 commit、annotated tag 与
安装包资产，不移动任何历史发布标签。

## Expert Team / 专家团队

Complex tasks can now run as one bounded Expert Team while ordinary chat still
shows one parent task and one final answer.

复杂任务现在可以作为一个有界“专家团队”运行，同时普通对话仍只呈现一个父任务和一份
最终答案。

- DeepSeek plans two to four distinct Research, Analysis, Production and Review
  roles when specialist collaboration materially helps the task.
- Research and Analysis are read-only. Production writes only to isolated,
  run-scoped staging. Review is read-only and must accept or reject the exact
  Production revision.
- DS Agent persists immutable attempt contracts, dependencies, evidence,
  budgets, retry lineage, conflicts, quality gates and merge receipts.
- At most three dependency-ready experts run concurrently. Read/write resource
  conflicts are serialized and nested expert teams are rejected.
- Failed work creates a bounded new attempt instead of rewriting history. The
  parent cannot synthesize success until every latest deterministic gate passes.
- Final merge authority remains with DS Agent. Staged bytes and hashes are
  reverified immediately before one exact merge receipt is recorded.

- 当专家协作确实有帮助时，DeepSeek 可以规划二到四个不同的研究、分析、制作与审核角色。
- 研究与分析保持只读；制作只能写入每次运行隔离的暂存区；审核保持只读，并且必须针对
  准确的制作版本给出接受或拒绝结论。
- DS Agent 持久记录不可变任务约定、依赖、证据、预算、重试谱系、冲突、质量闸门与
  合并回执。
- 最多三个依赖已满足的专家并发运行；读写资源冲突会串行化，嵌套专家团队会被拒绝。
- 失败工作通过有界的新 attempt 继续，不改写历史；所有最新确定性闸门通过前，父任务
  不能合成成功结论。
- 最终合并权仍属于 DS Agent；合并前会重新校验暂存内容与摘要，只记录一次准确回执。

## Durable Soul Across Conversations / 跨对话 Soul

Explicit identity and collaboration settings now persist in the same chat turn
and are reloaded for new conversations.

用户明确设定或确认的身份与协作信息现在会在同一轮对话中持久化，并在新对话中重新载入。

- Defines exact roles for the user's own name, how DS Agent addresses the user,
  the user's name for DS Agent and DS Agent's self-reference.
- Explicit definitions, changes and immediately bound short confirmations can
  write Soul without a second confirmation.
- DS Agent requires exact current-message evidence, an allowed field, bounded
  content and sensitivity checks before writing.
- A visible success receipt appears only after both `memory/soul.md` and its
  append-only audit event are durable. Audit failure rolls the file back.
- Unknown, sensitive, unbound, read-only expert or storage-unavailable updates
  fail closed and explicitly report that nothing was saved.
- New and already-created empty conversations refresh the latest Soul before
  their first message; chat-history compression cannot erase the profile.

- 明确区分用户姓名、DS Agent 对用户的称呼、用户对 DS Agent 的称呼以及 DS Agent 自称。
- 明确的设定、修改和与上一条助手提议准确绑定的短确认，可以直接写入 Soul，无需二次确认。
- 写入前必须通过当前消息原文证据、字段白名单、内容长度与敏感信息校验。
- 只有 `memory/soul.md` 与 append-only 审计事件都持久化后才显示成功回执；审计失败会
  回滚文件。
- 未知、敏感、证据未绑定、只读专家或存储不可用的更新会 fail closed，并明确说明未写入。
- 新建和已提前创建的空对话会在首次发送前刷新最新 Soul；对话压缩不会擦除该设定。

## Preserved Safety Boundaries / 保持不变的安全边界

- DeepSeek owns open-ended reasoning, role planning, drafting, review judgment
  and synthesis proposals. DS Agent owns schemas, persistence, permissions,
  isolation, budgets, evidence, deterministic gates, recovery and final merge.
- Durable Verified Computer Use keeps the v0.8.0
  `observe -> approve -> revalidate -> act once -> observe -> verify` boundary.
- High-risk computer control still requires exact one-shot approval and local
  unlock. Model output cannot silently authorize local effects.
- Secrets, raw screenshots, typed text, window titles, accessibility text and
  sensitive local paths remain local and are excluded from public release data.

- DeepSeek 负责开放式推理、角色规划、起草、审核判断与综合提议；DS Agent 负责 schema、
  持久化、权限、隔离、预算、证据、确定性闸门、恢复与最终合并。
- 持久、可验证 Computer Use 延续 v0.8.0 的
  `观察 -> 批准 -> 再校验 -> 只执行一次 -> 再观察 -> 验证` 边界。
- 高风险电脑控制仍要求准确的一次性批准与本地解锁，模型输出不能静默授权本地副作用。
- 密钥、原始截图、输入文本、窗口标题、无障碍文本和敏感本地路径继续只留在本机，不进入
  公开发布数据。

## Upgrade Guidance / 升级说明

1. Use the built-in updater from an installed v0.5.0, v0.8.0-rc.1 or v0.8.0
   client, or download the v0.9.0 installer from the GitHub Release.
2. Verify the filename, byte length and SHA-256 published on the Release before
   running the unsigned installer.
3. Do not interrupt the NSIS update while installation is in progress.
4. Workspace choices, settings, Soul and durable run state live under OS
   app-data and workspace locations rather than the program directory.
5. After updating, confirm the installed version is 0.9.0 and start a new
   conversation to verify the expected Soul identity settings.

1. 在已安装的 v0.5.0、v0.8.0-rc.1 或 v0.8.0 中使用内置 updater，也可以从 GitHub
   Release 下载 v0.9.0 安装包。
2. 运行未签名安装包前，按 Release 公布值核对文件名、字节数与 SHA-256。
3. NSIS 升级过程中不要中断安装。
4. 工作区、设置、Soul 与持久运行状态位于 OS app-data 和工作区，而不是程序安装目录。
5. 升级后确认安装版本为 0.9.0，并新建对话核验预期 Soul 身份设置。

## Validation Evidence / 验证证据

- Test-first regression coverage reproduces the cross-conversation Soul failure
  and verifies exact field repair, persistence, receipt ordering and restart.
- Expert Team tests cover hostile staging paths, symlink or junction rejection,
  content tampering, evidence conflicts, resource scheduling, restart recovery,
  bounded retries and exact-revision review/merge.
- The complete local release gate includes secret scanning, TypeScript checking,
  the production frontend build, all Node suites, Rust tests, formatting, diff
  checks and the source-only release guard.
- A fresh Windows installer is built without installation and independently
  checked before publication. Exact final byte length and SHA-256 are published
  with the GitHub Release asset.
- Publication is gated on GitHub Actions for the exact release commit, an
  immutable annotated tag and a post-publication asset re-download.

- 测试优先回归覆盖复现了跨对话 Soul 故障，并验证字段纠错、持久化、回执顺序与重启恢复。
- 专家团队测试覆盖恶意暂存路径、符号链接或 junction 拒绝、内容篡改、证据冲突、资源调度、
  重启恢复、有限重试与准确版本审核/合并。
- 完整本地发布闸门包括秘密扫描、TypeScript 检查、前端 production build、全部 Node
  套件、Rust 测试、格式检查、diff 检查与 source-only 发布检查。
- 全新 Windows 安装包会在不安装的情况下构建并独立核验；最终字节数与 SHA-256 随
  GitHub Release 资产发布。
- 发布必须等待准确 release commit 的 GitHub Actions、不可变 annotated tag 与发布后
  资产回下载核验。

## Known Limits / 已知限制

Operations Briefing live smoke tests use
`docs/templates/operations-briefing-smoke-evidence` by default. The bundled
smoke files are marked as `SMOKE SAMPLE evidence for local verification only`
and `Replace before operational use`. The desktop seed action continues to use
blank operator templates under
`docs/templates/operations-briefing-evidence`, not smoke or business data.

- The Windows installer is unsigned and may show an unknown-publisher warning.
- Expert Team is intentionally one level deep, has bounded attempts and does
  not grant experts arbitrary destination writes or desktop control.
- Soul chat writes require explicit user definition, change or confirmation;
  inferred general memories remain separately reviewable.
- No live DeepSeek call or installed-app migration is part of the deterministic
  source test suite.
- macOS packaging still requires verification on a macOS host.
- Live email delivery and cloud-drive connectors remain deferred.
- PDF v1 remains ASCII-safe; use Markdown or HTML for full-fidelity CJK output.

- Windows 安装包未签名，可能显示“未知发布者”警告。
- 专家团队刻意限制为一层并采用有限 attempt，不允许专家任意写入最终目标或控制桌面。
- Soul 对话写入要求用户明确设定、修改或确认；一般推断记忆仍保持单独可审核。
- 确定性源码测试不包含真实 DeepSeek 调用或已安装应用迁移。
- macOS 安装包仍需在 macOS 主机验证。
- 真实邮件投递与云盘连接器继续延期。
- PDF v1 保持 ASCII-safe；完整中日韩文本输出请使用 Markdown 或 HTML。

## Installer Integrity / 安装包完整性

- Filename: `DS.Agent_0.9.0_x64-setup.exe`
- Size: `12,473,860 bytes`
- SHA-256: `F9A822E267E7591C5AC2B7D5F5A67C89DCE6834221F30DD0B30AD7D4C999ADA4`
- Windows signature: unsigned (`NotSigned`)

The values above come from the fresh, uninstalled release candidate and are
repeated on GitHub Release for download verification.

- 文件名：`DS.Agent_0.9.0_x64-setup.exe`
- 大小：`12,473,860 字节`
- SHA-256：`F9A822E267E7591C5AC2B7D5F5A67C89DCE6834221F30DD0B30AD7D4C999ADA4`
- Windows 签名：未签名（`NotSigned`）

以上数值来自全新构建且未安装的 release candidate，并会在 GitHub Release 中重复公布，
供下载后核验。
