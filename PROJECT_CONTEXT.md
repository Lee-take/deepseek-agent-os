# DeepSeek Agent OS Project Context

## Project Positioning

This project is an open-source desktop Agent OS optimized for DeepSeek, designed for ordinary office and operations users first, while preserving a serious agent harness suitable for coding, analysis, automation, and industry workflows.

This is an independent project and is not an official DeepSeek product or an affiliated DeepSeek repository.

The product is not a DeepSeek clone of Codex, Claude Code, OpenClaw, or CodeWhale. It should combine the best ideas from lawful open-source and public references without becoming a feature pile.

## Core Thesis

The future agent is not only a chat interface. It is a controlled execution system that combines memory, tools, permissions, audit logs, rollback, scheduling, verification, and collaboration-ready task records.

The product should let users bring experience, documents, processes, and judgement into reusable agent workflows.

## Architecture Principle

Use a stable Agent OS Kernel with pluggable Workflow Packs.

The kernel must not be tied to one user group or one scenario. Enterprise operations, personal office work, and developer workflows should share the same kernel objects:

- Workspace
- Actor
- Task
- Run
- Step
- Artifact
- Capability
- Policy
- Memory
- Verification
- AuditLog

Workflow Packs provide scenario-specific forms, prompts, templates, tool permissions, validation recipes, and output formats.

## Confirmed Product Constraints

- Local-first desktop app.
- Team collaboration data structures must exist from the beginning, but cloud sync is not part of MVP.
- First version should support email, cloud drive, browser, and Computer Use through capability adapters.
- Each capability must have independent authorization.
- Memory is automatic, but visible, auditable, scoped, and reversible.
- Third-party extension is configuration/template/protocol first. Arbitrary third-party code execution is not allowed in MVP.
- Provider framework should allow major models later, but first product experience is DeepSeek-first.
- Desktop app and local kernel are open-source first. Hosted/team/commercial services are deferred.
- First sample workflow pack is Operations Management, but operations logic must not leak into the kernel.
- Login/unlock should use a local password plus an auto-generated local agent token.
- Desktop stack is Tauri + React + TypeScript + Rust sidecar.
- First Operations Management workflow is Operations Briefing: local evidence folder to management brief, anomaly table, action plan, and report package.
- MVP import is preview-first: full export, import preview, template/workflow import, read-only run archive replay, and memory candidates requiring review.
- Computer Use is behind an experimental high-risk flag in MVP.
- v0.0.1 is the current GitHub open-source preview version. The immediate priority is to make the project reliably build, install, launch, and run on Windows before adding more product surface.
- Public release policy: Apache-2.0 license, GitHub repository name `deepseek-agent-os`, and source-only first alpha.

## Important Boundaries

- Do not use leaked Claude Code source code.
- Do not copy private or questionable implementations.
- Use lawful public references only: OpenAI Codex, CodeWhale, OpenClaw, Hermes Agent Desktop, learn-claude-code, ECC, official docs, and papers.
- Treat reference projects as architecture inputs, not code to paste.

## Current Workspace

- Root: `D:\deepseek UI`
- Reference repositories: `D:\deepseek UI\_reference_repos`
- CodeGraph version used for analysis: `1.1.1`
