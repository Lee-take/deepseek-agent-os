# DS Agent v0.8.0 Release Notes / 正式版说明

Status: Windows-first stable release. Repository:
https://github.com/Lee-take/dsagent

状态：Windows 优先正式版本。项目地址：
https://github.com/Lee-take/dsagent

## Release Identity / 发布身份

Package, desktop, Tauri and Cargo metadata remain `0.8.0`, and the updater
identity is `v0.8.0`. Installed `v0.5.0` and `v0.8.0-rc.1` clients can detect
this stable release. A stable `v0.8.0` client does not treat the historical RC,
an equal version or a lower version as an update.

package、desktop、Tauri 和 Cargo 元数据继续保持 `0.8.0`，updater 当前身份为
`v0.8.0`。已安装的 `v0.5.0` 和 `v0.8.0-rc.1` 可以发现本正式版；`v0.8.0`
正式版不会把历史 RC、相同版本或更低版本当作可用更新。

The v0.6 Automation connector and v0.7 Artifact Engine names were internal
roadmap milestones already consolidated into the public `v0.5.0` release. No
retroactive v0.6 or v0.7 tags are created. The published `v0.8.0-rc.1` tag,
Release and notes remain immutable historical update-test evidence.

v0.6 Automation connector 与 v0.7 Artifact Engine 是内部路线图里程碑，相关能力
已合并进入公开的 `v0.5.0`，因此不会补建 v0.6 或 v0.7 tag。已经发布的
`v0.8.0-rc.1` tag、Release 和说明保持不可变，作为升级测试历史证据。

## Durable Verified Computer Use / 持久、可验证的 Computer Use

v0.8.0 delivers the first complete Windows-first Durable Verified Computer Use
step through one evidence-driven loop:

`observe -> approve -> revalidate -> record ActionStarted -> act once -> observe -> verify`

v0.8.0 交付首个 Windows 优先的持久、可验证 Computer Use 完整步骤，采用同一条
证据驱动闭环：

`观察 -> 批准 -> 再校验 -> 持久记录 ActionStarted -> 只执行一次 -> 再观察 -> 验证`

- Sessions and steps persist in SQLite with revision compare-and-swap, bounded
  recovery and malformed-row quarantine.
- The exact action, one-shot approval, stable window identity, title hash,
  accessibility target, semantic state and checkpoint are bound together.
- DS Agent persists `ActionStarted` before the external input effect. A restart
  across that boundary becomes `EffectUnknown` and is never replayed
  automatically.
- The foreground window and target are revalidated immediately before acting;
  stale or changed bindings fail closed with zero control calls.
- Post-action screenshot and UI Automation evidence are captured automatically.
  Screenshot-only evidence cannot claim semantic success; the deterministic
  postcondition must pass.
- User takeover stops later control, records durable state and requires a fresh
  observation before work can continue.
- The right rail exposes bounded create, bind, approve, run, takeover,
  re-observe and cancel controls without exposing raw private evidence.
- Local loopback bridge requests bypass system proxies so local capability calls
  are not diverted through unrelated proxy configuration.

- Session 与 step 使用 SQLite 持久化，并通过 revision compare-and-swap、有限恢复和
  畸形记录隔离保护状态一致性。
- 准确动作、一次性批准、稳定窗口身份、标题摘要、无障碍目标、语义状态和 checkpoint
  被绑定为同一份执行证据。
- DS Agent 在产生外部输入副作用前先持久记录 `ActionStarted`；如果跨越该边界重启，
  状态转为 `EffectUnknown`，绝不自动重放。
- 执行前立即重新校验前台窗口和目标；陈旧或变化的绑定会 fail closed，控制调用为零。
- 动作后自动采集截图与 UI Automation 证据；只有截图不能证明语义成功，必须通过确定性
  后置条件。
- 用户接管会停止后续控制、持久记录状态，继续前必须重新观察。
- 右侧状态栏提供有界的创建、绑定、批准、运行、接管、重新观察和取消入口，不暴露原始
  私密证据。
- 本地 loopback bridge 请求绕过系统代理，避免本地能力调用被无关代理配置转发。

## Safety And Privacy Boundaries / 安全与隐私边界

- The verified Computer Use scenario is an isolated, low-risk Notepad-like
  Windows application.
- Secure/UAC desktops, privileged targets, managed browser login-state reuse,
  broad cross-application coverage and general undo are not claimed.
- Raw screenshots, typed text, window titles, accessibility text and sensitive
  local paths stay local. Public DTOs and UI surfaces carry only bounded labels,
  summaries, evidence handles and fingerprints.
- DeepSeek may propose content and one exact action. DS Agent owns schema and
  policy validation, authorization, execution, evidence, verification,
  recovery, takeover and replay prevention.
- Computer control remains high risk, requires an exact one-shot approval and
  local unlock, and cannot be silently authorized by model output.

- 已验证的 Computer Use 场景是隔离、低风险、类似记事本的 Windows 应用。
- 本版本不宣称支持安全/UAC 桌面、特权目标、受管浏览器登录态复用、广泛跨应用覆盖或
  通用撤销。
- 原始截图、输入文本、窗口标题、无障碍文本和敏感本地路径只留在本机；公开 DTO 与 UI
  仅显示有界标签、摘要、证据句柄和指纹。
- DeepSeek 可以提议内容与一个准确动作；DS Agent 负责 schema/policy 校验、授权、执行、
  证据、验证、恢复、接管和防重放。
- Computer control 始终属于高风险能力，需要准确的一次性批准和本地解锁，模型输出不能
  静默自我授权。

## Upgrade Guidance / 升级说明

1. In DS Agent, check for updates from an installed `v0.5.0` or
   `v0.8.0-rc.1` client, or download the stable installer from the GitHub
   Release.
2. Verify the installer filename, byte length and SHA-256 against the values in
   the published Release before running an unsigned binary.
3. The built-in updater downloads the installer, runs the NSIS silent update
   and restarts DS Agent. Do not interrupt the process while installation is in
   progress.
4. Workspace choices, settings and durable run state live under OS app-data and
   workspace locations rather than the program installation directory.
5. After updating, confirm the installed version is `0.8.0` and that the
   expected workspace/settings remain available.

1. 在已安装的 `v0.5.0` 或 `v0.8.0-rc.1` 中使用 DS Agent 检查更新，也可以从
   GitHub 正式 Release 下载稳定版安装包。
2. 运行未签名安装包前，按 Release 公布值核对文件名、字节数和 SHA-256。
3. 内置 updater 会下载安装包、执行 NSIS 静默升级并重启 DS Agent；安装过程中不要
   中断进程。
4. 工作区选择、设置和持久运行状态位于 OS app-data 与工作区，而不是程序安装目录。
5. 升级后确认安装版本为 `0.8.0`，并检查原有工作区和设置仍可使用。

## Validation Evidence / 验证证据

- The published RC commit completed GitHub Actions with the production frontend
  build, secret scan, source-only release guard and 778 passing Rust tests, zero
  failures and seven environment-only ignored tests.
- The public RC installer was downloaded again and matched its published byte
  length and SHA-256.
- The maintainer completed the built-in updater transition from installed
  `v0.5.0` to `v0.8.0-rc.1`; registry, executable and embedded updater identity
  were independently rechecked before stable promotion.
- Stable updater fixtures passed, including old-stable discovery and
  RC/equal/downgrade fail-closed coverage. The complete offline local release
  gate passed with a 1,600-module frontend production build, 779 passing Rust
  tests, zero failures, seven environment-only ignored tests, secret scan,
  source-only guard, formatting and diff checks.
- A fresh stable installer candidate was built without installation. Its
  ProductVersion and FileVersion are `0.8.0`; the binary contains the stable
  updater identity and no `v0.8.0-rc.1` updater identity.
- Publication remains gated on exact-commit remote CI, an immutable annotated
  tag and a post-publication asset re-download.

Operations Briefing live smoke tests use
`docs/templates/operations-briefing-smoke-evidence` by default. The bundled
smoke files are marked as `SMOKE SAMPLE evidence for local verification only`
and `Replace before operational use`. The desktop seed action continues to use
blank operator templates under
`docs/templates/operations-briefing-evidence`, not smoke or business data.

- 已发布 RC commit 的 GitHub Actions 已通过 production frontend build、secret
  scan、source-only release guard，以及 778 个 Rust 通过测试、0 失败、7 个仅环境原因
  ignored 测试。
- 公开 RC 安装包已重新下载，字节数与 SHA-256 均与发布值一致。
- 维护者已完成从已安装 `v0.5.0` 到 `v0.8.0-rc.1` 的内置 updater 升级；正式版
  提升前又独立复核了注册表、可执行文件和内嵌 updater 身份。
- 正式版 updater fixture 已通过，覆盖旧稳定版发现更新以及 RC/相同版本/降级全部
  fail closed。完整离线本地 release gate 已通过：前端 production build 为 1,600 modules，
  Rust 为 779 passed、0 failed、7 个仅环境原因 ignored，并通过 secret scan、source-only
  guard、format 与 diff 检查。
- 已构建但未安装全新稳定版 candidate；ProductVersion 与 FileVersion 都是 `0.8.0`，
  二进制包含 stable updater 身份且不含 `v0.8.0-rc.1` updater 身份。
- 发布仍需等待精确 commit 的远程 CI、不可变 annotated tag 与发布后资产回下载核验。

## Known Limits / 已知限制

- The Windows installer is unsigned and may show an unknown-publisher warning.
- Durable Verified Computer Use remains a bounded Windows-first capability, not
  a claim of safe arbitrary desktop automation.
- macOS packaging configuration exists but still requires verification on a
  macOS host.
- Email read/draft/send remains an approval and audit surface without live mail
  delivery; cloud-drive connectors remain deferred.
- PDF v1 is ASCII-safe; use Markdown or HTML for full-fidelity Chinese and other
  Unicode report output.

- Windows 安装包未签名，可能出现“未知发布者”提示。
- 持久、可验证 Computer Use 仍是有界的 Windows 优先能力，不代表可以安全地自动化任意
  桌面应用。
- macOS 打包配置已经存在，但仍需在 macOS 主机上验证。
- 邮件读取/草稿/发送目前仍是批准与审计表面，不会真实投递邮件；云盘连接器继续延期。
- PDF v1 保持 ASCII-safe；需要完整中文或其他 Unicode 报告时请使用 Markdown 或 HTML。

## Installer Integrity / 安装包完整性

- Filename / 文件名: `DS.Agent_0.8.0_x64-setup.exe`
- Byte length / 字节数: `12,354,607`
- SHA-256: `AF22E6D28C20BF8C61967421AAEB9DFDAAA9E2729CED18AE0D43A68E331E49BF`
