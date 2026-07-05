# DS Agent and DeepSeek Boundary

This document defines the product and runtime boundary between DS Agent and
DeepSeek.

DS Agent is a local execution layer for DeepSeek-backed work. It should not be
a blind text relay, and it should not become a second local reasoning model.
DeepSeek owns open-ended reasoning. DS Agent owns deterministic readiness,
context packaging, permission checks, local execution, auditability, and
artifact management.

## Maintainer Summary (Chinese)

- DS Agent is not a local reasoning model. It should not locally reimplement
  DeepSeek's understanding, planning, drafting, summarization, classification,
  or judgment.
- DS Agent is also not a blind relay. It adds value by packaging context,
  checking readiness, enforcing permissions, managing the workspace, executing
  validated actions, recording evidence, and making work resumable.
- For ordinary chat, questions, and task instructions, send DeepSeek the full
  user message plus DS Agent protocol context unless the request is clearly a
  fully local deterministic command.
- DeepSeek returns separated intent: user-visible text, missing prerequisites,
  proposed actions, workflow or plugin calls, artifact targets, and memory
  candidates.
- DS Agent treats every model action as a proposal. It executes only after local
  schema checks, allowlist checks, capability policy, risk checks, workspace
  path checks, source requirements, and confirmation rules pass.

## Ownership

| Layer | Owns | Does not own |
| --- | --- | --- |
| DS Agent | Local setup checks, workspace structure, protocol context, capability policy, path and risk validation, user confirmations, tool and workflow execution, telemetry, audit records, artifacts, work packages, memory candidate review. | Natural-language understanding, planning, drafting, summarization, classification, judgment, or other model-native reasoning. |
| DeepSeek | Understanding the user's message, deciding what response or plan is useful, generating user-facing text, drafting structured action proposals, asking for missing prerequisites, and proposing workflow/tool/artifact/memory steps. | Direct local file, browser, terminal, email, drive, or desktop access; permission approval; path trust; secret handling; or final execution authority. |

The model can propose. DS Agent decides whether the proposal is allowed to run.
The model can ask for local context. DS Agent decides what context is safe and
useful to provide.

## Request Routing Rules

| User request type | DeepSeek first? | DS Agent responsibility |
| --- | --- | --- |
| Ordinary question, drafting, analysis, translation, summarization, planning, judgment, or ambiguous instruction | Yes | Run deterministic preflight, send the full user message with protocol context, parse the envelope, and validate any returned local actions. |
| Local readiness or settings action fully specified by code | No, unless explanation is needed | Show or change local state, such as API key readiness, workspace setup, access mode, permission records, import/export, UI navigation, release/source/secret guard checks, or work-package operations. |
| User asks for current web or source-linked information | Yes, after checking search readiness | Tell DeepSeek which search routes are available. If no source-linked route exists, ask for the missing search model/source configuration before claiming current facts. |
| User asks DS Agent to read, write, browse, search, run a workflow, export an artifact, or use a connector | Usually yes | Let DeepSeek decide whether the action is useful, then validate the proposed action locally before execution. Direct manual tool surfaces may still execute deterministic commands chosen by the user. |
| High-risk or mutating action, including file write, terminal write, email send, browser submit, or computer control | Yes when reasoning is needed; never self-approved by the model | Require local policy approval or explicit user confirmation, then record the approval and invocation. |
| Workflow pack, skill, or plugin request | Usually yes | Treat the workflow as a capability behind chat. DeepSeek may propose it; DS Agent validates inputs, prerequisites, policy, run state, artifacts, and replay records. |

## Default Chat Workflow

1. The user types naturally in the central chat composer.
2. DS Agent runs deterministic local preflight:
   - check whether a DeepSeek API key is available when model reasoning is
     needed;
   - check whether a work root exists when local files, artifacts, or workflow
     state are needed;
   - check whether a source-linked search route exists when the user asks for
     current web information;
   - collect current access mode, allowed capabilities, active workspace state,
     recent relevant run state, and the expected structured response schema.
3. If the request is a fully local deterministic command, DS Agent may execute
   it without calling DeepSeek. Examples include settings changes, readiness
   checks, permission-state display, import/export, release/source/secret
   guards, UI navigation, and commands whose behavior is fully specified by
   local code.
4. Otherwise DS Agent sends DeepSeek the full user message plus compact DS
   Agent protocol context. DS Agent should not replace model reasoning with a
   local heuristic classifier.
5. DeepSeek returns a structured agent envelope:
   - `user_reply`: text that can be shown in the conversation;
   - `missing_prerequisites`: keys, workspace, source routes, or user inputs
     needed before execution;
   - `proposed_actions`: local actions DS Agent may validate and run;
   - `workflow_calls`: workflow-pack or skill/plugin calls;
   - `artifact_targets`: files, reports, exports, or work packages to create;
   - `memory_candidates`: reviewable memory proposals, not automatic writes.
6. DS Agent validates the envelope before doing anything local:
   - schema and version;
   - action allowlist;
   - local capability policy;
   - access mode;
   - risk level and confirmation rule;
   - workspace path boundaries;
   - source-link requirements for search and evidence;
   - secret and release guards.
7. DS Agent shows the user-facing reply in chat, asks for missing prerequisites
   just in time, records proposed/blocked/approved/executed actions, and shows
   step state in the right sidebar.
8. If a validated action produces tool evidence that requires more reasoning,
   DS Agent sends the smallest useful follow-up context back to DeepSeek rather
   than replaying the whole conversation.

## Goal Loop Completion Contract

DS Agent should treat a user instruction as a goal-directed run, not as a single
model response. Before sending work to DeepSeek, DS Agent packages the user's
message with a lightweight loop contract:

- `user_goal`: the concrete outcome the user is asking DS Agent to achieve.
- `constraints`: safety, workspace, source, format, permission, and product
  boundaries that must be respected.
- `done_when`: observable conditions that must be true before DS Agent can call
  the task complete.
- `completion_verifier`: the local state, tool evidence, source evidence, test
  output, opened artifact, or other concrete observation needed to verify the
  outcome.
- `disallowed_near_misses`: answers that are related but do not satisfy the
  user's real goal, such as explaining how to create a file when the user asked
  DS Agent to create it.

Completion is not based on model confidence. A local/browser/file/Office/tool
task is complete only when DS Agent has observable evidence that satisfies the
`done_when` criteria. If verification fails, DS Agent should repair the smallest
failed step or switch strategy. If the same failure repeats or no new evidence
is produced, DS Agent should stop and report the blocker instead of looping.

When a run finishes or partially finishes, DS Agent may add one short
`completion_advice` sentence that helps the user improve the result. The advice
must be secondary to the completed result, grounded in the current task, and
must not imply that extra work has already run.

## Structured Envelope Contract

DeepSeek should return separated fields so DS Agent can display, validate, and
execute safely:

```json
{
  "version": "ds-agent-envelope/v1",
  "user_reply": "Text that can be shown to the user.",
  "missing_prerequisites": [],
  "proposed_actions": [],
  "workflow_calls": [],
  "artifact_targets": [],
  "memory_candidates": []
}
```

Required interpretation rules:

- `user_reply` is conversation text. It is not permission to execute anything.
- `missing_prerequisites` pauses all dependent actions until the user supplies
  the needed key, workspace, connector, search route, file, or confirmation.
- `proposed_actions` are local action requests. They must include an action
  type, a human-readable reason, risk metadata, and the minimal validated input
  needed by DS Agent. File writes must include exact UTF-8 content and a target
  path relative to the workspace unless another destination is explicitly
  approved.
- `workflow_calls` are workflow-pack, skill, or plugin proposals. They must be
  routed through the same local prerequisite, policy, path, source, and artifact
  validation as tool actions.
- `artifact_targets` describe what should be produced, but DS Agent chooses the
  safe workspace path and records the artifact reference.
- `memory_candidates` are review items only. They are not long-term memory until
  the user or review workflow accepts them.
- If the envelope is malformed or its schema is unsupported, DS Agent may show
  safe plain text but must execute no local action from that response.

## Execution Rules

- A model-returned action is always a proposal until DS Agent validates it.
- DeepSeek cannot self-approve high-risk actions.
- Unknown action types are blocked.
- High-risk or mutating actions require explicit confirmation unless a local
  policy path already allows them.
- File and artifact writes must stay inside the configured work root or another
  explicitly approved local destination.
- Ordinary users configure one work root. DS Agent creates and extends
  subdirectories for evidence, reports, exports, runs, sources, work packages,
  memory, logs, and future artifact types as needed.
- Memory is proposed for review. DS Agent should not silently write long-term
  memory from a model response.

## Action Lifecycle

Every model-returned local action follows the same state machine:

1. `proposed`: the model suggested an action and DS Agent normalized it.
2. `waiting_prerequisite`: a required key, workspace, source route, connector,
   file, or user input is missing.
3. `needs_confirmation`: local policy requires user approval before execution.
4. `blocked`: schema, allowlist, path, source, secret, release, or policy checks
   rejected the action.
5. `succeeded`: DS Agent executed the validated action and recorded the audit
   event, invocation, evidence reference, artifact, or workflow run.
6. `failed`: DS Agent attempted the allowed action but the local executor,
   connector, network route, model route, or workflow returned an error.

The right sidebar should render this lifecycle as execution status. It should
not become a second settings surface.

## Missing Prerequisites

Prerequisites should appear only when they are actually needed:

- If chat or model reasoning needs DeepSeek, ask for the DeepSeek API key.
- If local artifacts, workflow state, or file operations need a place to live,
  ask for one work root and a human-readable workspace name.
- If the user asks for current web information, use an available source-linked
  search route. If no route exists, ask for the search model/source credentials
  or another configured search route.
- If a workflow pack needs a connector or plugin, ask only for that connector or
  plugin at the moment the workflow needs it.

## Workflow Packs, Skills, and Plugins

Workflow packs, skills, and plugins are capabilities behind the conversation.
They should not replace the central chat with fixed mandatory flows. The user
should be able to ask naturally; DeepSeek can decide that a workflow/tool is
useful; DS Agent then validates and runs that workflow/tool through local policy.

## UI Implications

- The center of the app is the chat. It should not require the user to choose a
  fixed workflow before typing.
- The left sidebar groups navigation, workflow packs, plugins, memory, and
  settings. Advanced internal setup belongs under settings or debug surfaces,
  not on the first screen.
- The first-run workspace prompt asks for one work root and a workspace name.
  Evidence, reports, exports, runs, sources, work packages, memory, and logs are
  DS Agent-managed subdirectories.
- Missing DeepSeek, workspace, search, connector, or permission prerequisites
  appear as just-in-time dialogs or approval prompts tied to the user's current
  request.
- The right sidebar shows step state, permissions, evidence references,
  workflow run status, and recent execution output.

## Anti-Goals

- Do not build a local natural-language planner that duplicates DeepSeek.
- Do not send every user message directly to DeepSeek without DS Agent protocol,
  readiness checks, permissions, and execution control.
- Do not expose internal evidence/export/report directories as first-run setup
  decisions.
- Do not let workflow forms become the primary UI for ordinary chat.
- Do not treat plain model text as verified web evidence.
