<p align="center">
  <img src="apps/desktop/public/ds-agent-mark.png" width="96" alt="DS Agent logo" />
</p>

<h1 align="center">DS Agent</h1>

<p align="center"><strong>One Kernel. Modular capabilities. Verifiable execution.</strong></p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh-CN.md">中文</a>
</p>

<p align="center">
  <a href="https://github.com/Lee-take/dsagent/releases/tag/v1.5.0">v1.5.0 stable</a> ·
  <a href="https://github.com/Lee-take/dsagent/releases/download/v1.5.0/DS.Agent_1.5.0_x64-setup.exe">Download for Windows</a> ·
  <a href="LICENSE">Apache-2.0</a>
</p>

<p align="center">Created and maintained by <strong>Lee take</strong>.</p>

DS Agent is a DeepSeek-first local Windows agent for everyday work. Describe the result you need in chat: it can organize office material, research with sources,
work with local files, run durable automations, and control desktop apps when you
approve—while keeping steps, evidence, permissions, verification, and recovery
visible.

Under the hood, DS Agent is a local Agent Harness optimized for DeepSeek. The
Harness gives model-backed work a durable, permissioned, and verifiable
execution boundary on the local machine.

DS Agent began from a practical need: more colleagues were using DeepSeek in daily work, yet it was difficult to find an Agent optimized for DeepSeek with a trustworthy local execution boundary.

<p align="center">
  <img src="docs/assets/ds-agent-office-workflow.png" alt="A real DS Agent v1.0.0 run turning meeting notes into a team execution checklist, with six completed verification steps" />
</p>

<p align="center"><em>A real local v1.0.0 run: meeting notes become a ready-to-send execution checklist, with every run step visible and completed.</em></p>

## What you can do today

- **Turn office input into action.** Convert meeting notes, requirements, or
  rough material into structured checklists, plans, reports, and handoffs.
- **Build evidence-backed briefings.** Read selected local files or public web
  sources, preserve source links, and produce reviewable summaries.
- **Keep recurring work running.** Create one-time, daily, weekly, or monthly
  automations with persisted schedules, bounded retries, and recovery state.
- **Use the desktop with control.** Let DS Agent operate supported local apps
  only through explicit permissions, pre/post observation, and verification.

## One Kernel, modular capabilities

The key difference is that Memory, Automation, Computer Use, parallel
Subagents, and Skills are not isolated plugins. They form one complete loop
coordinated by the same DS Agent Kernel.

DS Agent uses a contract-first modular Harness architecture. New tools,
connectors, workflows, Skills, and executors enter through shared contracts for
permissions, resource locking, idempotency, evidence, audit, verification, and
recovery. A module does not bypass the Kernel, and it does not need to rebuild a
separate state machine or safety system.

```mermaid
flowchart TB
    U["User goal"] --> P["DS Agent preflight and context"]
    P --> D["DeepSeek<br/>understand · plan · analyze · synthesize"]
    D --> K["DS Agent Kernel<br/>validate · approve · execute · verify · recover"]
    K --> M["Memory and Soul"]
    K --> A["Automation"]
    K --> C["Computer Use"]
    K --> S["Subagents"]
    K --> SK["Skills and Connectors"]
    K --> O["Evidence and Artifacts"]
    M --> R["Reviewable result"]
    A --> R
    C --> R
    S --> R
    SK --> R
    O --> R
```

## Five core capabilities, one engineering philosophy

### Memory and Soul

DS Agent selects useful long-term memory instead of saving every conversation.
Memory receipts explain what was used and why; feedback influences later
retrieval; auditable background maintenance can update, merge, or archive
memory. Soul keeps a user-approved identity, communication style, and
collaboration preferences consistent across conversations.

### Durable Automation

Natural-language goals can run once, daily, weekly, or monthly. Schedules,
trigger windows, run state, missed-run policy, and results are persisted so work
can recover after restart. Resource locks, bounded retries, and idempotency
checks prevent duplicate execution. A scheduled task never receives broader
authority than an interactive task.

### Verified Computer Use

Desktop control follows a complete safety loop:
`pre-observe → approve → revalidate → act once → post-observe → verify`.
User takeover stops control, uncertain effects are not replayed automatically,
and computer control requires a one-shot approval plus a local in-memory unlock
code.

### Parallel Subagents

Complex work can be divided into bounded Research, Analysis, Production, and
Review roles. Subagents use isolated context, resources, budgets, and staging;
Review binds its decision to the exact production revision; the parent produces
one final synthesis instead of flooding the main conversation.

### Generated and automatic Skills

DS Agent can turn suitable instructions into declarative Skills, validate and
activation-test them, and keep a stable identity when they are updated.
Installed Skills can be selected automatically when a task needs them. Source,
integrity, permissions, trust state, execution plans, audit events, and
disable/uninstall controls remain visible and enforceable.

## Loop Engineering

DS Agent treats work as a goal-directed loop, not a single model reply:

`goal → done-when contract → context → plan → permission → execution → evidence → verification → recovery`

The Kernel persists run and review state, retries only bounded failed steps, and
does not claim completion from model confidence alone. Local files, browser
actions, Office artifacts, and Computer Use are complete only when observable
evidence satisfies the task's completion criteria.

In v1.5.0, DeepSeek may propose a bounded `GoalEnvelope`, but only the local
Kernel can validate and freeze it. For the same queued task, the Kernel alone
derives the capability manifest, risk, and preview shown in one exact-task
authorization card. Approval creates only exact authority; it does not execute
a Tool, resume the task, or mark the Goal complete. Completion remains blocked
until locally authoritative verifier evidence covers every frozen `done_when`
condition and required artifact identity.

The v1.5.0 binary preserves the T1 Excel/PowerPoint verification engine and adds a production-reachable durable Computer Use session/step UI and Kernel loop.
Exact installed evidence is limited to generated File Explorer and Excel targets plus installed Edge with a fresh profile and an ephemeral loopback test portal; it does not claim general browser automation, stored-login reuse, or production-site completion.
The release remains honest: ordinary chat does not yet automatically select or sequence the two T1 tools or the complete T1 Office workflow.

## DeepSeek and DS Agent boundary

| Layer | Responsibility |
| --- | --- |
| **DeepSeek** | Open-ended understanding, planning, analysis, drafting, and synthesis. |
| **DS Agent Kernel** | Deterministic readiness, context packaging, policy, approval, execution, evidence, audit, verification, and recovery. |

DeepSeek proposes actions; DS Agent decides whether they are safe and allowed to
run. The model never receives direct local authority and cannot approve its own
high-risk action. See the full [model boundary](docs/AGENT_MODEL_BOUNDARY.md).

## More built-in capabilities

- Local files, folders, source-linked web research, browser actions, terminal
  diagnostics, screenshots, and permissioned desktop input.
- Markdown, HTML, lightweight PDF, Office-style artifacts, evidence receipts,
  reports, and portable work packages.
- Review, Recovery, durable checkpoints, stale-action rejection, and verified
  one-shot undo for supported local file changes.
- Microsoft/Google-shaped mail, calendar, sync, draft, mutation, and
  reconciliation contracts validated with offline adversarial fake providers.

Production Microsoft/Google account registration and live external-write
authority remain disabled in v1.5.0. The release does not sign in to real
accounts, send real email, or create, change, or cancel real calendar events.

## Why Rust

The DS Agent Kernel and desktop command layer use Rust to keep local execution
predictable, strongly typed, memory-safe, and low-overhead. Rust is especially
valuable for concurrency, resource ownership, durable state transitions,
credential isolation, and exact error handling. Tauri commands and the React UI
remain thin; the Kernel and persistent projections own business state.

## Quick start

1. Download the [Windows x64 installer](https://github.com/Lee-take/dsagent/releases/download/v1.5.0/DS.Agent_1.5.0_x64-setup.exe).
2. Enter your own valid DeepSeek API key in onboarding and run the explicit
   balance/model verification. The key is stored locally with Windows DPAPI.
3. Choose one local workspace and let the readiness doctor verify its managed
   directories and writability.
4. Describe the result you want in chat. DS Agent requests additional
   permissions or prerequisites only when the task needs them.

A user-supplied DeepSeek API key is a required prerequisite. DS Agent does not
bundle a shared key or bypass DeepSeek access requirements; use remains subject
to DeepSeek's terms and account policies.

The v1.5.0 application executable and installer are Authenticode `NotSigned`.
Windows may display `Unknown publisher` or a Microsoft Defender SmartScreen
warning. Download only over HTTPS from this repository, verify the SHA-256 in
the GitHub Release, and read the [installation guide](docs/INSTALLATION.md)
before running the installer.

## Code signing policy

DS Agent `v1.5.0` is intentionally unsigned. The SignPath Foundation application
is submitted and approval is pending; no release is represented as signed.
If the project is accepted, signing starts with a later new version and does not
replace the immutable v1.1.0, v1.2.0, v1.3.0, v1.4.0, or v1.5.0 tag or asset. For releases accepted into the program:
**Free code signing provided by SignPath.io, certificate by SignPath
Foundation.** See the full [code signing policy](CODE_SIGNING_POLICY.md) and
[privacy policy](PRIVACY.md).

### Build from source

```powershell
npx pnpm@9.15.9 install
npx pnpm@9.15.9 test
npx pnpm@9.15.9 --filter @deepseek-agent-os/desktop tauri:dev
```

On Windows, use a `CARGO_TARGET_DIR` without spaces for release builds, for
example `D:\build-target\ds-agent-v1-release`.

## Stable release

- Release: [DS Agent v1.5.0](https://github.com/Lee-take/dsagent/releases/tag/v1.5.0)
- Installer: `DS.Agent_1.5.0_x64-setup.exe`
- Integrity: verify the final byte size and SHA-256 published in the GitHub
  Release before running the installer.
- Onboarding: one user-supplied Key, Windows DPAPI storage, explicit DeepSeek
  balance/V4 model verification, secret-free readiness, and workspace doctor.
- Compatibility: existing workspace settings remain readable; environment-key
  operators require explicit verification; conversations and connector vaults
  are not rewritten.
- Goal contract: bounded model proposal, Kernel validation/freeze, read-only UI
  state, and fail-closed evidence coverage for every completion condition and
  required artifact.
- Task authorization: one Kernel-derived exact-task card with manifest/risk/
  preview binding, one user decision, per-capability audit, and exact revocation;
  approval does not execute or resume the task.
- Computer Use: production-reachable durable lifecycle and fail-closed execution; exact installed proof is limited to generated Explorer/Excel targets and an isolated Edge/loopback test portal, not general browsing.
- T1 verification engine: exact source identities and provenance, non-overwriting XLSX/PPTX artifacts, actual local render evidence, bounded revisions, and persisted continuation checkpoints; ordinary chat does not yet automatically select or sequence this complete T1 path.

## Documentation

- [Installation](docs/INSTALLATION.md)
- [DS Agent and DeepSeek boundary](docs/AGENT_MODEL_BOUNDARY.md)
- [v1 architecture](docs/architecture/DS_AGENT_V1_ARCHITECTURE_PLAN.md)
- [v1.5.0 release notes](docs/RELEASE_NOTES_v1.5.0.md)
- [v1.4.0 release notes](docs/RELEASE_NOTES_v1.4.0.md)
- [v1.3.0 release notes](docs/RELEASE_NOTES_v1.3.0.md)
- [v1.2.0 release notes](docs/RELEASE_NOTES_v1.2.0.md)
- [v1.1.0 release notes](docs/RELEASE_NOTES_v1.1.0.md)
- [v1 completion audit](docs/DS_AGENT_V1_COMPLETION_AUDIT.md)
- [Security](SECURITY.md) · [Privacy](PRIVACY.md) ·
  [Code signing policy](CODE_SIGNING_POLICY.md) ·
  [Contributing](CONTRIBUTING.md) · [License](LICENSE)

DS Agent is an independent open-source project. It is not an official DeepSeek
product and does not claim DeepSeek ownership, authorization, or endorsement.
The DeepSeek name is used only to describe model compatibility and the project's
DeepSeek-first design.

Search aliases: DS Agent, DSAgent, dsagent, DeepSeek Agent OS.
