# Decisions

This file records project decisions that should survive context compression, archive, or new conversations.

## D001: Agent OS Kernel plus Workflow Packs

Decision: Build a stable Agent OS Kernel and keep business scenarios in Workflow Packs.

Reason: If the architecture is rooted in one target user group, later expansion to office, operations, and developer workflows will cause expensive rewrites. Kernel objects must stay generic.

Status: Accepted.

## D002: Local-first with collaboration-ready data model

Decision: MVP is local-first and has no cloud sync, but the data model includes Actor, Role, AuditLog, ShareableRun, and import/export packages.

Reason: Team collaboration will matter, but cloud sync would distract MVP execution. Collaboration-ready local records avoid a future data model rewrite.

Status: Accepted.

## D003: Capability-specific permission engine

Decision: Permissions are not a single full-access switch. File, network, browser, email, drive, terminal, and Computer Use permissions must be separately controlled.

Reason: Ordinary users need clear, task-appropriate control. High-risk capabilities such as email send, file mutation, browser form submission, terminal execution, and Computer Use need separate review.

Status: Accepted.

## D004: Automatic memory with auditability

Decision: Memory is captured automatically, but every memory item needs source, scope, sensitivity, lifecycle, and deletion/edit controls.

Reason: Memory is one of the most important agent features. A hidden or unscoped memory system will create trust, privacy, and correctness problems.

Status: Accepted.

## D005: Template-first extension model

Decision: Workflow Packs are manifest/template/schema/protocol based in MVP. Arbitrary third-party executable code is excluded.

Reason: Third-party code plugins can hide malicious behavior. If code extensions are added later, they require sandboxing, signatures, permissions, review, and stable APIs.

Status: Accepted.

## D006: DeepSeek-first provider strategy

Decision: Build a provider abstraction for multiple mainstream models, but optimize first for DeepSeek.

Reason: DeepSeek-specific thinking mode, Pro/Flash routing, context caching, long context, cost behavior, and reasoning/tool-call handling should shape the first-class experience.

Status: Accepted.

## D007: Import/export is part of collaboration foundation

Decision: Support Workspace Package and Run Archive import/export.

Reason: Without cloud sync, import/export is the safest early collaboration bridge. It also tests whether tasks, evidence, outputs, approvals, and memory candidates are portable.

Status: Accepted.

## D008: Latency Controller is part of the kernel

Decision: The kernel should distinguish Fast Path, Workflow Path, and Deep Work Path.

Reason: OpenClaw-style broad capability loading can feel slow. Ordinary users need fast first results, while complex work still needs deep planning and verification.

Status: Accepted.

## D009: Local unlock and agent token

Decision: The app needs local unlock plus an auto-generated local agent token.

Reason: Browser extensions, local API, Computer Use bridges, and sidecar processes need secure local authentication. Tokens should be generated locally, stored securely, rotatable, and auditable.

Status: Accepted.

## D010: Desktop stack

Decision: Use Tauri + React + TypeScript + Rust sidecar as the desktop stack.

Reason: The project is local-first, permission-heavy, and needs native filesystem/process/security boundaries. Tauri keeps the desktop app lighter than Electron, React/TypeScript supports a polished workbench UI, and Rust is a good fit for the local kernel, event store, policy engine, and sidecar adapters.

Status: Accepted.

## D011: First Operations Management workflow

Decision: The first workflow is an Operations Briefing workflow that turns a local evidence folder into an evidence-backed management brief, anomaly table, action plan, and exportable report package.

Reason: This workflow validates the core Agent OS capabilities without hardcoding an industry into the kernel: file ingestion, memory retrieval, data/document analysis, evidence traceability, DeepSeek routing, verification, artifact generation, permissions, and run archive export.

Status: Accepted.

## D012: MVP import behavior

Decision: MVP supports full export, import preview, template/workflow import, and read-only run archive replay. Imported memories become reviewable memory candidates and are not written automatically.

Reason: Import/export is needed for collaboration, but direct import of data, permissions, or long-term memory is risky. Preview-first import validates package design while keeping the active workspace safe.

Status: Accepted.

## D013: Computer Use rollout

Decision: Computer Use ships behind an experimental high-risk flag in MVP. Screenshot/inspect is allowed with explicit permission; mouse/keyboard control requires per-step approval and cannot run in full auto mode.

Reason: Computer Use is powerful but risky for ordinary users. The first version should prove the policy, audit, and UI controls before enabling broader automation.

Status: Accepted.

## D014: PDF Unicode strategy for MVP

Decision: Keep PDF export lightweight and ASCII-safe in MVP. Markdown and HTML exports are the full-fidelity Unicode report formats.

Reason: Bundling a CJK-capable font affects app size, license review, rendering quality, and cross-platform packaging. Runtime OS font discovery adds platform variability. The current PDF path remains useful for simple ASCII-safe handoff while HTML/Markdown preserve Chinese and other Unicode content.

Status: Accepted.

## D015: Codex bridge runtime lifecycle for MVP

Decision: Codex bridge runs as an external compatible loopback HTTP service in MVP. The desktop app does not spawn, install, or supervise a sidecar process.

Reason: The bridge touches screen pixels, input control, and source-backed search. Keeping it external makes lifecycle, trust, logging, upgrades, and local operator consent explicit while the contract stabilizes. A managed stdio sidecar can be revisited after token hardening, signing, and process supervision are designed.

Status: Accepted.

## D016: Memory Studio conflict actions

Decision: Memory Studio offers three explicit conflict actions: link-and-accept, merge-and-accept, and replace-and-accept. Link keeps all visible memories. Merge writes a new merged memory from the preview and tombstones selected source memories. Replace writes the candidate as the new memory and tombstones selected target memories.

Reason: Memory conflicts should not be silently collapsed. The user needs a visible choice between preserving relationships, consolidating duplicates, and superseding older records. All paths preserve append-only candidate resolutions, memory creation, links, and tombstones for audit.

Status: Accepted.

## D017: v0.0.1 Windows-first open-source preview

Decision: Treat `v0.0.1` as the current public preview version. Stop broad feature expansion and prioritize making the desktop app build, install, launch, and run reliably on Windows, while keeping the existing scope honest and documented.

Reason: The project exists to provide serious DeepSeek-first domestic model support, not to become a feature pile. A credible GitHub preview needs a real Windows running baseline, clear positioning, reproducible builds, safety boundaries, and transparent limitations more than additional features.

Status: Accepted.

## D018: Apache-2.0 license for public release

Decision: Publish DeepSeek Agent OS under Apache-2.0.

Reason: The project is infrastructure-style open source for DeepSeek-first agent support. Apache-2.0 keeps contribution and reuse permissive while adding an explicit patent grant that is useful for agent runtime, desktop, and integration work.

Status: Accepted.

## D019: Source-first 0.0.1 preview

Decision: Publish `v0.0.1` as the current source-first release material. Do not attach unsigned Windows debug installer artifacts unless the maintainer explicitly approves that later.

Reason: The current Windows installer is useful for local validation, but a public open-source alpha should avoid distributing unsigned binaries before signing, artifact provenance, and release packaging policy are mature. Source build instructions are enough for the first credible GitHub baseline.

Status: Accepted.
