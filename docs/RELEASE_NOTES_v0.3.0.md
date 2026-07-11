# DS Agent v0.3.0 Release Notes

Status: Windows-first formal feature release. The `v0.3.0` release supersedes
`v0.2.3` for ordinary downloads.

Search aliases: DS Agent, DSAgent, dsagent, DeepSeek Agent OS.

Repository: https://github.com/Lee-take/dsagent

DS Agent v0.3.0 adds an automatic, persistent lifecycle for declarative Skills
and Plugins while keeping installation, updates, activation, and removal inside
the local DS Agent safety boundary.

## Install From Chat

- Send a public GitHub repository URL or Hugging Face repository/Space URL in
  chat. DS Agent resolves an immutable revision, discovers supported declarative
  Skill content, validates it, installs it, enables it, and records provenance
  without asking the user to choose a package format or repeat the instruction.
- GitHub Skill repositories, `SKILL.md` packages, compatible plugin metadata,
  and supported Hugging Face Skill repositories are adapted into the existing
  hash-verified Skill runtime.
- A failed or incompatible package is rejected without replacing a working
  local installation.

## Automatic Use And Updates

- Enabled installed Skills and Plugins are added to the Agent capability
  catalog and selected automatically when the task needs them.
- Every app launch starts a background update sweep for remote installations.
  A newer immutable revision is fetched, validated, and committed under the
  same local identity. The prior working revision remains available as rollback
  metadata.
- Update failures are audited and leave the last working version active. No
  intermediate version, format, conflict, or retry decision is delegated to the
  user.

## Manage And Create

- The Plugins panel now separates System Skills, installed Plugins, installed
  Skills, and Scenario Templates.
- Installed third-party items can be enabled, disabled, or uninstalled directly.
  Protected System Skills cannot be removed.
- The protected `Skill/Plugin Builder` System Skill lets DS Agent create and
  update safe declarative Skills from a direct user instruction. Generated
  Skills are validated, activation-tested, installed, and enabled automatically.
- Operations Briefing is classified as a Scenario Template rather than a
  Plugin.

## Safety Boundary

- The new repository path accepts public HTTPS GitHub and Hugging Face sources
  through strict host and redirect allowlists, size limits, path traversal
  checks, manifest validation, declared permissions, integrity hashes, and
  append-only lifecycle audit events.
- Arbitrary scripts, native binaries, executable payloads, undeclared permission
  expansion, unsafe paths, and unsupported repository contents fail closed.
- Ordinary browser sandbox rules remain unchanged; repository installation uses
  its own narrow source adapter so a local network proxy cannot turn general
  browsing into private-address access.

Bumps the package, desktop, Tauri, Cargo, and updater metadata to `0.3.0` /
`v0.3.0` so installed Windows clients can detect this release as newer than
`v0.2.3`.

## Verification

- Full Rust suite: 528 passed, 2 ignored.
- Production frontend build, source release guard, formatting, whitespace, and
  Skills/Plugins catalog regression checks passed.
- Live adapters were verified independently against a public GitHub Skill
  repository and an official Hugging Face Skill Space.
- The source-built Windows Tauri application passed the WebView2 command-bridge
  smoke and the Skills/Plugins catalog UI smoke, including the protected builder
  and Operations Briefing classification.

Operations Briefing live smoke tests use
`docs/templates/operations-briefing-smoke-evidence` by default. The bundled
smoke files are marked as `SMOKE SAMPLE evidence for local verification only`
and `Replace before operational use`. The desktop seed action uses blank operator templates
under `docs/templates/operations-briefing-evidence` rather than smoke
or business data.

## 中文说明

`v0.3.0` 增加完整的声明式 Skill / 插件生命周期：用户只需在聊天中发送公开的 GitHub
或 Hugging Face 地址，DS Agent 就会自动识别、校验、安装、启用并记录来源，不再让用户
选择包格式或重复确认。今后打开应用时，远程安装项会在后台自动比较不可变版本并升级；
升级失败时保留上一可用版本，不把冲突、重试或回滚选择交给用户。

插件页现已区分系统技能、已安装插件、已安装 Skill 和场景模板。第三方项目可以直接启
用、禁用或卸载；系统自带的 `Skill/Plugin Builder` 受到保护，并让 DS Agent 能根据用户
指令自行制作、校验、测试和安装声明式 Skill。“运营简报”已归入场景模板，不再显示为
插件。

安全边界仍由 DS Agent 本地执行：只允许受控的公开 HTTPS 来源，执行来源、重定向、大小、
路径、manifest、权限和哈希校验；任意脚本、原生二进制、可执行载荷、未声明权限扩张和不
受支持内容都会按 fail-closed 原则拒绝。

The Windows installer remains unsigned, so Windows may show an
unknown-publisher warning.

For historical notes, see `docs/RELEASE_NOTES_v0.2.3.md`.
