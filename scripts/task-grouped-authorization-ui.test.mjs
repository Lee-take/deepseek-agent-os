#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [app, types, commands, domain, groupedStore] = await Promise.all([
  readFile("apps/desktop/src/App.tsx", "utf8"),
  readFile("apps/desktop/src/types.ts", "utf8"),
  readFile("apps/desktop/src-tauri/src/commands.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/kernel/task_grouped_approval.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/kernel/event_store/grouped_approval.rs", "utf8"),
]);

for (const status of [
  "pending",
  "approved",
  "rejected",
  "revoked",
  "expired",
  "scope_changed",
]) {
  assert.match(types, new RegExp(`\\|? \\"${status}\\"`));
  assert.match(app, new RegExp(`${status.replace("_", "\\_")}`));
}

for (const binding of [
  "group_id",
  "task_id",
  "expected_projection_revision",
  "manifest_revision",
  "manifest_fingerprint",
  "preview_schema_revision",
  "preview_renderer_revision",
  "preview_hash",
]) {
  assert.match(types, new RegExp(`${binding}:`));
  assert.match(domain, new RegExp(`pub ${binding}:`));
}

const cardStart = app.indexOf("function TaskGroupedAuthorizationCard");
const cardEnd = app.indexOf("function capabilityFamilyIcon", cardStart);
assert.ok(cardStart >= 0 && cardEnd > cardStart);
const card = app.slice(cardStart, cardEnd);
for (const field of [
  "authorization.goal",
  "authorization.applications",
  "authorization.paths",
  "authorization.accounts",
  "authorization.recipients",
  "authorization.time_windows",
  "authorization.external_targets",
  "authorization.expires_at",
  "authorization.risk_level",
  "authorization.verifiers",
  "authorization.capability_audits",
]) {
  assert.match(card, new RegExp(field.replaceAll(".", "\\.")));
}
assert.match(card, /onResolve\(authorization\.intent, true\)/);
assert.match(card, /onResolve\(authorization\.intent, false\)/);
assert.match(card, /onRevoke\(authorization\.intent\)/);
assert.doesNotMatch(card, /invoke\s*\(/);
assert.doesNotMatch(card, /resolve_capability_access_request/);

assert.match(app, /invoke<TaskGroupedAuthorizationView>\("resolve_task_grouped_authorization"/);
assert.match(app, /invoke<TaskGroupedAuthorizationView>\("revoke_task_grouped_authorization"/);
assert.match(app, /await refreshTaskGroupedAuthorizationState\(\)/);
assert.match(app, /setTaskGroupedAuthorizations\(\[\]\)/);
assert.match(app, /!taskGroupedAuthorization\s*&&\s*messageApprovalActions\.length > 0/);
assert.match(
  app,
  /taskGroupedAuthorizations\.find\([\s\S]{0,240}authorization\.intent\.task_id === message\.goal_projection\?\.goal_id/,
);
const queuedWorkerStart = app.indexOf('"run_next_queued_agent_chat_worker"');
const queuedWorkerEnd = app.indexOf("const refreshMemoryCandidateRecords", queuedWorkerStart);
assert.ok(queuedWorkerStart >= 0 && queuedWorkerEnd > queuedWorkerStart);
const queuedWorker = app.slice(queuedWorkerStart, queuedWorkerEnd);
assert.match(queuedWorker, /goal_projection: workerResult\.response\.goal_projection/);
assert.match(queuedWorker, /refreshCapabilityState\(\)/);
assert.doesNotMatch(app, /prepare_task_grouped_approval(?:_from_proposal)?/);
assert.doesNotMatch(
  app,
  /resolve_task_grouped_authorization"[\s\S]{0,300}(capability|risk|scope|target|authority|preview|grant|actor|claim|token)\s*:/i,
);

assert.match(domain, /pub struct TaskGroupedAuthorizationIntent/);
assert.match(domain, /#\[serde\(deny_unknown_fields\)\]\s*pub struct TaskGroupedAuthorizationIntent/);
assert.match(domain, /actor: TaskGroupedApprovalActor::User/);
assert.doesNotMatch(
  domain.slice(
    domain.indexOf("pub struct TaskGroupedAuthorizationIntent"),
    domain.indexOf("impl TaskGroupedAuthorizationIntent"),
  ),
  /capability|risk|scope|target|authority|preview:|grant|actor|claim|token/i,
);

for (const phrase of [
  "list_task_grouped_authorizations",
  "resolve_task_grouped_authorization",
  "revoke_task_grouped_authorization",
  "refresh_task_grouped_approval_state",
  "c3c_exact_ui_intent_rejects_tamper_replay_and_frontend_authority_fields",
  "c3c_ui_read_refreshes_expiry_and_survives_restart_without_new_authority",
]) {
  assert.match(groupedStore, new RegExp(phrase));
}

assert.match(commands, /pub fn list_task_grouped_authorizations/);
assert.match(commands, /pub fn resolve_task_grouped_authorization/);
assert.match(commands, /pub fn revoke_task_grouped_authorization/);
assert.doesNotMatch(commands, /pub fn prepare_task_grouped_approval/);
assert.doesNotMatch(commands, /TaskGroupedApprovalActor::(DeepSeekModel|FrontendPayload)/);

console.log("Grouped authorization UI and IPC authority boundary checks passed");
