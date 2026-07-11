# DS Agent v0.2.1 Release Notes

Status: Windows-first formal patch release. The `v0.2.1` release supersedes
`v0.2.0` for ordinary downloads.

Search aliases: DS Agent, DSAgent, dsagent, DeepSeek Agent OS.

Repository: https://github.com/Lee-take/dsagent

## Restart-Safe Durable Runs

DS Agent v0.2.1 closes the first restart-safe recovery slice for durable background runs.

- A long-running tool-backed run can renew its worker and resource leases while
  local execution is in progress.
- After the desktop process restarts and the old lease expires, DS Agent can
  reclaim the same run from its persisted event history.
- A previously verified tool invocation is reused by its canonical request
  fingerprint, evidence, and verification receipt instead of repeating a
  possible file write.
- Durable queued guidance is included at the recovered worker boundary and is
  recorded as applied once.
- A cancellation request made while the worker is offline ends in one durable
  cancelled state without applying later queued guidance.
- The task inspector shows recovery count and recovery reason while the main
  composer remains available.

The recovery contract remains fail-closed: a tool invocation with an
indeterminate side-effect outcome is blocked for review rather than replayed
automatically.

Bumps the package, desktop, Tauri, Cargo, and updater metadata to `0.2.1` / `v0.2.1` so installed Windows clients can detect this release as newer than `v0.2.0`.

## Verification

- Disk-backed restart regressions cover a real workspace file write, lease
  heartbeat, recovery, duplicate-write prevention, durable guidance,
  cancellation, resource state, audit evidence, and terminal state.
- The desktop test suite, TypeScript production build, Rust tests, source
  release guard, and Windows installed-UI workflow remain release gates.

Operations Briefing live smoke tests use
`docs/templates/operations-briefing-smoke-evidence` by default. The bundled
smoke files are marked as `SMOKE SAMPLE evidence for local verification only`
and `Replace before operational use`. The desktop seed action uses blank operator templates
under `docs/templates/operations-briefing-evidence` rather than smoke or
business data.

## Current Limits

- DS Agent is not yet an always-on operating-system service.
- A tool whose process stopped before its outcome was durably verified still
  requires review instead of automatic replay.
- The Windows installer remains unsigned, so Windows may show an
  unknown-publisher warning.
- macOS packaging is configured but is not validated or published by this
  Windows release.

## 中文说明

`v0.2.1` 是面向 Windows 的正式补丁版本，重点补齐后台任务的重启恢复证据。长耗时工具执行
期间会续租 worker 与资源锁；应用或进程重启后，租约过期的任务可以从持久事件记录中恢复并
由新 worker 重新领取。已经带有证据和验证结果的同一工具请求会直接复用，不会再次写入文件。

重启前排队的 guidance 会在恢复后的 worker 边界加入任务上下文并只记录一次已应用状态；离线
期间收到的取消请求会形成唯一的 cancelled 终态，不会继续执行后续 guidance。任务检查器会显
示恢复次数和原因，主输入框继续可用。

安全边界保持不变：如果工具副作用结果不确定，DS Agent 会阻断并要求复核，不会自动重放。

For historical notes, see `docs/RELEASE_NOTES_v0.2.0.md` and
`docs/RELEASE_NOTES_v0.1.2.md`.
