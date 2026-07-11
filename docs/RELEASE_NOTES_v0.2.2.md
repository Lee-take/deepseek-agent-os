# DS Agent v0.2.2 Release Notes

Status: Windows-first formal patch release. The `v0.2.2` release supersedes
`v0.2.1` for ordinary downloads.

Search aliases: DS Agent, DSAgent, dsagent, DeepSeek Agent OS.

Repository: https://github.com/Lee-take/dsagent

## Ordinary-User Replies

DS Agent v0.2.2 makes ordinary replies and installed capabilities easier to use without weakening the local execution boundary.

- Successful local actions now produce short, action-aware result sentences
  instead of exposing protocol fields, tool identifiers, target/evidence/output
  labels, English verification receipts, or raw JSON in the chat body.
- The DeepSeek reply contract now requires the user's language and
  conclusion-first wording unless technical detail is explicitly requested.
- Full evidence, verification, and audit data remain available in the task
  inspector and durable local records.

## Automatic Skills And Plugins

- The ordinary-user plugins panel is a read-only installed-capability catalog.
  It shows names first and expands a selected name to its description.
- Install, enable/disable, manual-run, trust-reset, uninstall, and execution
  audit controls are no longer exposed in this ordinary-user surface.
- DeepSeek selects an installed capability when it matches the task. DS Agent
  still validates trust, verified entry content, permissions, resource locks,
  policy, execution evidence, and audit records locally.
- Legacy disabled state no longer excludes an otherwise safe installed
  declarative Skill from the automatic catalog. Untrusted or incomplete Skills
  still fail closed.

Bumps the package, desktop, Tauri, Cargo, and updater metadata to `0.2.2` / `v0.2.2` so installed Windows clients can detect this release as newer than `v0.2.1`.

## Verification

- Production frontend build, secret scan, JavaScript UI regressions, Rust tests,
  source release guard, and Windows installer build are release gates.
- Focused regressions cover ordinary completion wording, automatic Skill
  catalog inclusion, safe execution despite legacy disabled state, and a full
  queued Skill-backed run.

Operations Briefing live smoke tests use
`docs/templates/operations-briefing-smoke-evidence` by default. The bundled
smoke files are marked as `SMOKE SAMPLE evidence for local verification only`
and `Replace before operational use`. The desktop seed action uses blank operator templates
under `docs/templates/operations-briefing-evidence` rather than smoke or
business data.

## 中文说明

`v0.2.2` 重点修复普通用户体验：聊天结果不再显示工具标识、协议字段、原始 JSON 或英文
验证回执；完整技术证据仍保留在运行检查器和本地审计记录中。

插件面板改为只读能力目录，只显示已安装技能/插件名称，点击名称才显示简介。DeepSeek
根据任务选择能力，DS Agent 继续在本地校验信任、入口、权限、资源锁、执行证据和审计。
旧的禁用状态不再阻止安全的已安装声明式 Skill 自动参与任务；不可信或入口不完整的 Skill
仍按 fail-closed 原则阻止。

For historical notes, see `docs/RELEASE_NOTES_v0.2.1.md`.
