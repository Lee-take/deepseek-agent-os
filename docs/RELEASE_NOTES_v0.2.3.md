# DS Agent v0.2.3 Release Notes

Status: Windows-first formal patch release. The `v0.2.3` release supersedes
`v0.2.2` for ordinary downloads.

Search aliases: DS Agent, DSAgent, dsagent, DeepSeek Agent OS.

Repository: https://github.com/Lee-take/dsagent

DS Agent v0.2.3 restores a one-click update experience and makes every real approval actionable where it appears.

## Update Experience

- DS Agent checks for updates at startup, then downloads and validates an
  available Windows installer silently.
- `Install update` appears only after the installer is ready. Clicking it is
  the user's authorization to install; no second Agent permission is required.
- Model-proposed high-risk work still goes through DS Agent's local permission,
  exact-request, resource-lock, evidence, and audit boundaries.

## Visible Approvals

- A pending tool card now shows `Confirm and run` and `Reject` directly.
- The unified pending queue appears before the long capability catalog and is
  brought into view when a request arrives.
- Approval from the right panel resumes the matching visible chat action.

Bumps the package, desktop, Tauri, Cargo, and updater metadata to `0.2.3` / `v0.2.3` so installed Windows clients can detect this release as newer than `v0.2.2`.

## Verification

- Release gates cover production build, source hygiene, update flow, approval
  visibility, durable approval resume, exact one-shot binding, resource locks,
  installer trust, and the full Rust suite.

Operations Briefing live smoke tests use
`docs/templates/operations-briefing-smoke-evidence` by default. The bundled
smoke files are marked as `SMOKE SAMPLE evidence for local verification only`
and `Replace before operational use`. The desktop seed action uses blank operator templates
under `docs/templates/operations-briefing-evidence` rather than smoke or
business data.

## 中文说明

`v0.2.3` 恢复简单更新流程：后台自动下载并校验，准备完成后才显示“安装更新”，点击后
直接安装，不再要求第二次审批。真正需要审批的操作会在右侧“待批准”卡片上直接显示
“确认并执行 / 拒绝”，高风险工具的本地权限、精确请求绑定、资源锁、证据和审计边界保持
不变。

For historical notes, see `docs/RELEASE_NOTES_v0.2.2.md`.
