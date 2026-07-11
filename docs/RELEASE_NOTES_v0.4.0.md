# DS Agent v0.4.0 Release Notes

Status: Windows-first formal feature release. The `v0.4.0` release supersedes
`v0.3.0` for ordinary downloads.

Search aliases: DS Agent, DSAgent, dsagent, DeepSeek Agent OS.

Repository: https://github.com/Lee-take/dsagent

DS Agent v0.4.0 adds native Subagent parallel work with bounded fan-out,
isolated child execution, and one parent synthesis while keeping the ordinary
chat composer usable.

## Native Parallel Work

- A parent Agent run can create a bounded group of independent child runs and
  execute them concurrently through the native DS Agent runtime.
- Each child receives isolated task context and persists its own claim,
  completion, failure, and audit evidence instead of sharing mutable chat state.
- Child results stay out of ordinary chat. After every child reaches a terminal
  state, the parent is re-queued exactly once and produces the user-facing
  synthesis.
- The composer remains available while parallel work is active, so a user can
  continue working without waiting for the entire group to finish.

## Recovery And Safety

- Durable group and child state make parallel work reviewable and restart-safe.
- Bounded fan-out and explicit parent/child ownership prevent recursive task
  explosions and accidental cross-run result mixing.
- Existing permissions, tool contracts, resource coordination, evidence, and
  audit boundaries continue to apply to child execution.

## Skill Catalog Refresh

- The installed capability catalog refreshes after Skill and Plugin lifecycle
  changes, including install, update, enable, disable, and uninstall actions.
- Newly available capabilities can be selected without restarting DS Agent.

Bumps the package, desktop, Tauri, Cargo, and updater metadata to `0.4.0` /
`v0.4.0` so installed Windows clients can detect this release as newer than
`v0.3.0`.

## Verification

- Full Rust suite: 534 passed, 2 ignored.
- Production frontend build, source release guard, secret scan, formatting,
  whitespace, Agent-context receipt, Skill/Plugin catalog, and Subagent parallel
  regression checks passed.
- An identifier-isolated Windows Tauri runtime audit proved three child workers
  were claimed concurrently, all child runs completed, the parent was re-queued
  once for synthesis, and the composer remained usable.

Operations Briefing live smoke tests use
`docs/templates/operations-briefing-smoke-evidence` by default. The bundled
smoke files are marked as `SMOKE SAMPLE evidence for local verification only`
and `Replace before operational use`. The desktop seed action uses blank operator templates
under `docs/templates/operations-briefing-evidence` rather
than smoke or business data.

## 中文说明

`v0.4.0` 加入 DS Agent 原生 Subagent 并行工作。父任务可以把相互独立的工作拆成数量受控、
上下文隔离的子任务并发执行；每个子任务独立留存领取、完成、失败和审计证据。所有子任务
结束后，父任务只会重新入队一次并统一生成面向用户的综合结果，中间结果不会刷进普通聊天。

并行执行期间，聊天输入框保持可用。持久化的任务组和父子关系让重启恢复与事后复核有明确
依据，现有权限、工具契约、资源协调、证据和审计边界继续适用于每个子任务。

本版同时让 Skill 与插件目录在安装、更新、启用、禁用和卸载后自动刷新，无需重启应用。

The Windows installer remains unsigned, so Windows may show an
unknown-publisher warning.

For historical notes, see `docs/RELEASE_NOTES_v0.3.0.md`.
