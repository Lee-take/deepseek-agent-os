#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const component = readFileSync("apps/desktop/src/AutomationCenter.tsx", "utf8");
const connectorHealth = readFileSync("apps/desktop/src/ConnectorHealthPanel.tsx", "utf8");
const recoveryCenter = readFileSync("apps/desktop/src/RecoveryCenter.tsx", "utf8");
const artifactDelivery = readFileSync("apps/desktop/src/ArtifactDeliveryPanel.tsx", "utf8");
const artifactCommands = readFileSync("apps/desktop/src-tauri/src/artifact_commands.rs", "utf8");
const artifactPublicCommands = artifactCommands.slice(
  artifactCommands.indexOf("#[tauri::command]"),
  artifactCommands.indexOf("#[cfg(test)]"),
);
const artifactKernel = readFileSync("apps/desktop/src-tauri/src/kernel/artifacts.rs", "utf8");
const desktopTypes = readFileSync("apps/desktop/src/types.ts", "utf8");
const connectorCommands = readFileSync("apps/desktop/src-tauri/src/connector_commands.rs", "utf8");
const connectorCommandsProduction = connectorCommands.split("#[cfg(test)]")[0];
const appCommands = readFileSync("apps/desktop/src-tauri/src/commands.rs", "utf8");
const reconciliation = readFileSync(
  "apps/desktop/src-tauri/src/kernel/connectors/reconciliation.rs",
  "utf8",
);
const eventStore = readFileSync("apps/desktop/src-tauri/src/kernel/event_store.rs", "utf8");
const commands = readFileSync("apps/desktop/src-tauri/src/automation_commands.rs", "utf8");
const connector = readFileSync("apps/desktop/src-tauri/src/kernel/connectors.rs", "utf8");
const connectorCatalog = readFileSync(
  "apps/desktop/src-tauri/src/kernel/connectors/catalog.rs",
  "utf8",
);
const connectorRuntimeRegistry = readFileSync(
  "apps/desktop/src-tauri/src/kernel/connectors/runtime_registry.rs",
  "utf8",
);
const connectorOauth = readFileSync(
  "apps/desktop/src-tauri/src/kernel/connectors/oauth.rs",
  "utf8",
);
const toolRuntime = readFileSync(
  "apps/desktop/src-tauri/src/kernel/tool_runtime.rs",
  "utf8",
);
const types = readFileSync("apps/desktop/src/types.ts", "utf8");
const main = readFileSync("apps/desktop/src-tauri/src/main.rs", "utf8");

assert.match(
  connectorOauth,
  /#\[cfg\(test\)\]\s+pub fn complete_authorization\(/,
  "state-only OAuth completion must remain test-only",
);
assert.match(
  eventStore,
  /#\[cfg\(test\)\]\s+pub fn claim_connector_authorization_session\(/,
  "OAuth state must not be a production authorization claim authority",
);
assert.match(
  eventStore,
  /pub\(crate\) fn finish_connector_authorization_with_account\(/,
  "phased OAuth completion requires an atomic account and session finalizer",
);
assert.match(
  eventStore,
  /DELETE FROM connector_authorization_actions\s+WHERE authorization_id = \?1 AND review_id IS NULL/,
  "account finalization must retain durable review tombstones for authority cleanup",
);
const providerExecution =
  connectorOauth.match(
    /pub\(crate\) fn execute_claimed_authorization_with_runtime<[\s\S]*?\n\}/,
  )?.[0] ?? "";
const phasedFinalization =
  connectorOauth.match(
    /pub\(crate\) fn finalize_claimed_authorization_with_runtime<[\s\S]*?\n\}/,
  )?.[0] ?? "";
const sharedFinalization =
  connectorOauth.match(
    /pub\(crate\) fn finalize_claimed_authorization_with_shared_runtime<[\s\S]*?\n\}/,
  )?.[0] ?? "";
assert.match(
  providerExecution,
  /exchange_code/,
  "provider execution must contain the exchange call",
);
assert.doesNotMatch(
  providerExecution,
  /with_authorization_fence|validate_connector_authorization_exchange_claim/,
  "provider execution must run outside the authorization fence and EventStore claim check",
);
assert.doesNotMatch(
  phasedFinalization,
  /exchange_code|complete_review/,
  "fenced finalization must never call a provider",
);
assert.match(
  sharedFinalization,
  /with_authorization_fence[\s\S]*\.lock\(\)[\s\S]*validate_connector_authorization_exchange_claim[\s\S]*put_authorization_result[\s\S]*\.lock\(\)[\s\S]*finish_connector_authorization_with_account/,
  "shared finalization must acquire the fence before short Store validation, vault mutation, and short Store commit",
);
assert.doesNotMatch(
  sharedFinalization,
  /exchange_code|complete_review/,
  "shared fenced finalization must never call a provider",
);
for (const fakeType of [
  "FakeConnectorCredentialStore",
  "FakeConnectorRemoteState",
  "FakeConnectorProvider",
]) {
  assert.match(
    connector,
    new RegExp(`#\\[cfg\\(test\\)\\]\\s+(?:#\\[[^\\]]+\\]\\s+)?pub struct ${fakeType}`),
    `${fakeType} must not exist in the production connector binary`,
  );
}
const authorizationReviewView =
  connectorCommands.match(
    /pub struct ConnectorAuthorizationReviewView \{[\s\S]*?\n\}/,
  )?.[0] ?? "";
assert.match(authorizationReviewView, /review_id: Uuid/);
assert.match(authorizationReviewView, /account: Option<ConnectorAccountHealthView>/);
assert.doesNotMatch(
  authorizationReviewView,
  /authorization_id|provider_id|tenant|secret|token|ticket|claim|attempt|credential|handle|scope|revision|generation|cleanup_required|error|detail/,
  "authorization review IPC view must contain only allowlisted safe fields",
);
const authorizationResolveRequest =
  connectorCommands.match(
    /pub struct ConnectorAuthorizationResolveRequest \{[\s\S]*?\n\}/,
  )?.[0] ?? "";
assert.match(authorizationResolveRequest, /review_id: Uuid/);
assert.match(authorizationResolveRequest, /intent: ConnectorAuthorizationReviewIntent/);
assert.doesNotMatch(
  authorizationResolveRequest,
  /authorization_id|provider_id|state:|code:|secret|token|ticket|claim|attempt|credential|handle|scope|revision|generation|now:/,
  "authorization resolve IPC input must be exactly review id plus typed intent",
);
const productionAuthorizationResolve =
  connectorCommandsProduction.match(
    /pub fn resolve_connector_authorization_review\([\s\S]*?\n\}/,
  )?.[0] ?? "";
assert.match(
  productionAuthorizationResolve,
  /connector_oauth_providers[\s\S]*resolve_connector_authorization_review_with_registry/,
  "production approve must resolve only through the immutable runtime registry",
);
assert.doesNotMatch(
  productionAuthorizationResolve,
  /Fake|LocalDemo|MicrosoftOAuth|Google|Arc::new/,
  "production authorization resolve must not construct a provider",
);
const authorizationResolveService =
  connectorCommandsProduction.match(
    /fn resolve_connector_authorization_review_with_registry<[\s\S]*?\n\}/,
  )?.[0] ?? "";
assert.match(
  authorizationResolveService,
  /registry\s*\.provider\(&provider_id\)[\s\S]*connector_authorization_active_review[\s\S]*resolve_connector_authorization_review/,
  "provider availability must be confirmed before authority read and durable review consumption",
);
assert.match(
  authorizationResolveService,
  /execute_review_authorization_with_runtime[\s\S]*let completion_now = Utc::now\(\)[\s\S]*finalize_claimed_authorization_with_shared_runtime/,
  "production resolve must use a fresh provider-return clock and the shared lock-order-safe finalizer",
);
assert.doesNotMatch(
  authorizationResolveService,
  /let store = event_store[\s\S]*finalize_claimed_authorization_with_shared_runtime/,
  "production resolve must not hold an EventStore guard while entering the authorization fence",
);
assert.doesNotMatch(
  connectorCommandsProduction,
  /ConnectorAuthorizationSession|ConnectorAuthorizationResult|ConnectorAuthorizationExchangeClaim|ConnectorAuthorizationCleanupClaim/,
  "Tauri connector commands must not return private OAuth session or claim types",
);
const builtinToolCatalog =
  toolRuntime.match(/pub fn builtin_tool_catalog\(\)[\s\S]*?\n\}/)?.[0] ?? "";
assert.doesNotMatch(
  builtinToolCatalog,
  /connector_authorization|oauth|review_id/i,
  "DeepSeek-visible built-in tools must not expose connector authorization review authority",
);
const authorizationReviewType =
  desktopTypes.match(
    /export type ConnectorAuthorizationReview = \{[\s\S]*?\n\};/,
  )?.[0] ?? "";
assert.match(
  authorizationReviewType,
  /awaiting_confirmation[\s\S]*connecting[\s\S]*connected[\s\S]*cancelling[\s\S]*cancelled[\s\S]*repair_required/,
);
assert.doesNotMatch(
  authorizationReviewType,
  /authorization_id|provider_id|tenant|secret|token|ticket|claim|attempt|credential|handle|scope|revision|generation|cleanup_required|error|detail/,
  "frontend authorization type must remain ownership-free and secret-free",
);
assert.match(connectorHealth, /list_connector_authorization_reviews/);
assert.match(connectorHealth, /resolve_connector_authorization_review/);
assert.match(connectorHealth, /正在安全清理连接信息/);
assert.doesNotMatch(
  connectorHealth,
  /intent:\s*["']approve["']/,
  "production UI must not offer approve while no live provider is registered",
);
assert.match(
  phasedFinalization,
  /with_authorization_fence[\s\S]*validate_connector_authorization_exchange_claim[\s\S]*put_authorization_result/,
  "phased completion must recheck the exact durable claim before any result credential write",
);
for (const legacyActionMethod of [
  "issue_connector_authorization_action",
  "claim_connector_authorization_action",
  "resolve_connector_authorization_action",
]) {
  assert.match(
    eventStore,
    new RegExp(`#\\[cfg\\(test\\)\\]\\s+pub\\(crate\\) fn ${legacyActionMethod}\\(`),
    `${legacyActionMethod} must remain test-only after durable review authority activation`,
  );
}

assert.match(component, /create_once_automation/);
assert.match(component, /set_automation_enabled/);
assert.match(component, /update_automation_goal/);
assert.match(component, /delete_automation/);
assert.match(component, /run_automation_now/);
assert.match(component, /resolve_automation_review_item/);
assert.match(component, /list_automation_review_items/);
assert.match(component, /需要审阅或对外修改时仍会等待你确认/);
assert.doesNotMatch(component, /cron|OAuth|PKCE|refresh_token|access_token/);
assert.match(connectorHealth, /list_connector_account_summaries/);
assert.match(connectorHealth, /disconnect_connector_account/);
assert.doesNotMatch(connectorHealth, /credential_handle|refresh_token|access_token|client_secret/);
assert.doesNotMatch(connectorHealth, /provider_id|granted_capabilities/);
assert.match(connectorHealth, /provider_label/);
assert.match(connectorHealth, /last_successful_sync_at/);
assert.match(connectorCommands, /list_connector_provider_catalog/);
assert.match(connectorCatalog, /ConnectorProviderAvailability::Unavailable/);
assert.match(appCommands, /ConnectorRuntimeRegistries::empty\(\)/);
assert.match(connectorRuntimeRegistry, /trait ConnectorOAuthRegistry/);
assert.match(connectorRuntimeRegistry, /trait ConnectorReadRegistry/);
assert.match(connectorRuntimeRegistry, /trait ConnectorSyncRegistry/);
assert.match(connectorRuntimeRegistry, /EmptyConnectorReconcilerRegistry/);
assert.match(connectorRuntimeRegistry, /EmptyConnectorRevocationRegistry/);
assert.doesNotMatch(
  connectorRuntimeRegistry,
  /ConnectorMutationProvider|ConnectorDraftProvider|BrowserUrlOpener|WindowsConnectorCredentialStore|MicrosoftOAuth|MicrosoftGraph|Google/,
);
assert.doesNotMatch(
  main,
  /ConnectorRuntimeRegistries|FakeConnectorProvider|MicrosoftOAuthProvider|MicrosoftGraphAdapter/,
);
assert.doesNotMatch(
  connectorCommands.match(/pub fn list_connector_provider_catalog[\s\S]*?\n\}/)?.[0] ?? "",
  /MicrosoftGraphAdapter|MicrosoftOAuthTokenClient|ConnectorRuntime|credential/i,
);
assert.match(eventStore, /CREATE TABLE IF NOT EXISTS connector_authorization_actions/);
assert.match(eventStore, /claim_connector_authorization_action/);
assert.match(connectorOauth, /complete_claimed_authorization/);
const authorizationSessionStruct =
  connectorOauth.match(/pub struct ConnectorAuthorizationSession \{[\s\S]*?\n\}/)?.[0] ?? "";
assert.doesNotMatch(
  authorizationSessionStruct,
  /exchange_claim|cleanup_claim|claim_id|claim_expires|lease|owner/,
);
const exchangeClaimStruct =
  eventStore.match(/pub\(crate\) struct ConnectorAuthorizationExchangeClaim \{[\s\S]*?\n\}/)?.[0] ?? "";
const cleanupClaimStruct =
  eventStore.match(/pub\(crate\) struct ConnectorAuthorizationCleanupClaim \{[\s\S]*?\n\}/)?.[0] ?? "";
const reviewProvisionStruct =
  eventStore.match(/pub\(crate\) struct ConnectorAuthorizationActionProvision \{[\s\S]*?\n\}/)?.[0] ?? "";
const activeReviewStruct =
  eventStore.match(/pub\(crate\) struct ConnectorAuthorizationActiveReview \{[\s\S]*?\n\}/)?.[0] ?? "";
const authorityCleanupClaimStruct =
  eventStore.match(/pub\(crate\) struct ConnectorAuthorizationAuthorityCleanupClaim \{[\s\S]*?\n\}/)?.[0] ?? "";
assert.match(exchangeClaimStruct, /claim_id: Uuid/);
assert.match(cleanupClaimStruct, /claim_id: Uuid/);
assert.doesNotMatch(exchangeClaimStruct, /Serialize|Deserialize|Clone/);
assert.doesNotMatch(cleanupClaimStruct, /Serialize|Deserialize|Clone/);
for (const privateReviewStruct of [
  reviewProvisionStruct,
  activeReviewStruct,
  authorityCleanupClaimStruct,
]) {
  assert.notEqual(privateReviewStruct, "");
  assert.doesNotMatch(privateReviewStruct, /Serialize|Deserialize|Clone/);
}
assert.match(eventStore, /action_status = 'consumed'/);
assert.match(eventStore, /authority_cleanup_required = 1/);
assert.match(eventStore, /action_status = 'resolved'/);
assert.doesNotMatch(
  connectorCommandsProduction,
  /ConnectorAuthorizationSession|ConnectorAuthorizationResult|returned_state|pkce_challenge|verifier_handle|result_credential_handle|redirect_uri|granted_scopes/,
);
assert.doesNotMatch(
  types,
  /ConnectorAuthorizationSession|ConnectorAuthorizationResult|returned_state|pkce_challenge|verifier_handle|result_credential_handle|redirect_uri|granted_scopes/,
);
assert.match(component, /RecoveryCenter/);
assert.match(component, /ArtifactDeliveryPanel/);
assert.match(artifactDelivery, /list_artifact_deliveries/);
assert.match(artifactDelivery, /结构检查/);
assert.match(artifactDelivery, /实际渲染/);
assert.match(artifactDelivery, /get_artifact_visual_preview/);
assert.match(artifactDelivery, /查看实际渲染预览/);
assert.match(artifactDelivery, /artifact-preview-pagination/);
assert.match(artifactDelivery, /previewErrors/);
assert.doesNotMatch(
  artifactDelivery,
  /storage_ref|artifact_hash|input_fingerprint|template_hash|validator_version|render_hash|claim|path/i,
  "artifact delivery UI must use only the public status projection",
);
assert.match(artifactCommands, /ArtifactDeliveryView/);
assert.doesNotMatch(
  artifactPublicCommands,
  /ArtifactRecord|storage_ref|artifact_hash|input_fingerprint|template_hash|path|content/i,
  "artifact IPC command must return only the public delivery view",
);
assert.match(artifactKernel, /MAX_ARTIFACT_REVISIONS: u32 = 3/);
assert.match(artifactKernel, /actual_office_or_pdf_render/);
assert.match(artifactKernel, /write_revision_if_authorized/);
assert.doesNotMatch(builtinToolCatalog, /artifact_storage|artifact_revision_provider|complete_artifact/);
assert.match(recoveryCenter, /list_connector_recovery_items/);
assert.match(recoveryCenter, /list_connector_read_activity/);
assert.match(recoveryCenter, /Promise\.allSettled/);
assert.match(recoveryCenter, /恢复项目仍可正常查看和处理/);
assert.match(recoveryCenter, /最近 50 项邮件或日历只读执行/);
assert.doesNotMatch(
  recoveryCenter,
  /query:|provider_id|tenant_ref|credential_handle|source_invocation_id|claim_id|plan_fingerprint|authority_fingerprint/,
  "read activity UI must remain a strict public projection",
);
assert.match(recoveryCenter, /retry_connector_attachment_cleanup/);
assert.match(recoveryCenter, /resume_connector_read_sync/);
assert.match(recoveryCenter, /inspect_connector_external_result/);
assert.match(recoveryCenter, /item\.action\?\.kind !== "resume_sync"/);
assert.match(recoveryCenter, /恢复只读同步/);
assert.match(recoveryCenter, /Resume read-only sync/);
assert.match(recoveryCenter, /核对外部结果/);
assert.match(recoveryCenter, /Check external result/);
assert.match(recoveryCenter, /action_revision: item\.action\.action_revision/);
assert.match(recoveryCenter, /这里只显示脱敏的恢复状态/);
assert.match(recoveryCenter, /发生了什么：/);
assert.match(recoveryCenter, /外部操作：/);
assert.match(recoveryCenter, /下一步：/);
assert.match(recoveryCenter, /reasonCopy\[language\]\[item\.reason_code\]/);
assert.match(recoveryCenter, /externalEffectCopy\[language\]\[item\.external_effect_state\]/);
assert.match(recoveryCenter, /nextStepCopy\[language\]\[item\.next_step_code\]/);
assert.doesNotMatch(recoveryCenter, /item\.summary|String\(reason\)|>\s*\{item\.status\}\s*</);
assert.doesNotMatch(recoveryCenter, /workspace_root|storage_identity|remote_ref|credential_handle|refresh_token|access_token|client_secret/);
assert.match(connectorCommands, /retry_connector_attachment_recovery/);
assert.match(connectorCommands, /recovery items could not be loaded safely/);
assert.match(eventStore, /FROM connector_sync_streams/);
assert.match(eventStore, /state\.stopped\(\)/);
assert.match(eventStore, /ConnectorRecoveryStatus::SyncExhausted/);
assert.doesNotMatch(
  eventStore,
  /fn issue_connector_(?:sync|reconciliation)_recovery_action/,
  "Recovery listing must not issue or rotate action authority",
);
assert.doesNotMatch(
  eventStore.match(/CONNECTOR_RECOVERY_RETRY_QUEUED_EVENT[\s\S]*?Self::insert_kernel_event\(&transaction, &event\)\?/g)?.at(-1) ?? "",
  /action_fingerprint|action_revision|token|credential_handle/,
  "Recovery retry events must not persist public revisions or private binding material",
);
assert.match(types, /kind: "attachment" \| "account" \| "sync" \| "reconciliation"/);
assert.match(types, /kind: "retry_attachment_cleanup"; action_revision: string/);
assert.match(types, /kind: "resume_sync"; action_revision: string/);
assert.match(types, /kind: "inspect_external_result"; action_revision: string/);
assert.match(types, /acceptance: "accepted" \| "already_accepted"/);
assert.match(types, /items: ConnectorRecoveryItem\[\]/);
assert.match(recoveryCenter, /result\.acceptance === "already_accepted"/);
assert.match(recoveryCenter, /background work is not yet claimed complete/);
const recoveryActionType =
  types.match(/export type ConnectorRecoveryAction =[\s\S]*?;\n/)?.[0] ?? "";
assert.doesNotMatch(
  recoveryActionType,
  /token|fingerprint|ticket|claim|attempt|credential|handle|secret/i,
  "Recovery actions must expose only a typed action and public revision",
);
for (const requestName of [
  "RetryConnectorAttachmentCleanupRequest",
  "ResumeConnectorReadSyncRequest",
  "InspectConnectorExternalResultRequest",
]) {
  const requestType =
    connectorCommands.match(
      new RegExp(`pub struct ${requestName} \\{[\\s\\S]*?\\n\\}`),
    )?.[0] ?? "";
  assert.match(requestType, /item_id: Uuid/);
  assert.match(requestType, /action_revision: String/);
  assert.doesNotMatch(
    requestType,
    /token|fingerprint|kind:|account_id|provider_id|capability|ticket|claim|attempt|credential|handle|secret|now:/i,
    `${requestName} must remain a strict public locator allowlist`,
  );
}
assert.match(connectorCommands, /resume_connector_read_sync_from_recovery/);
assert.doesNotMatch(
  connectorCommands.match(/pub fn resume_connector_read_sync[\s\S]*?\n\}/)?.[0] ?? "",
  /connector_runtime|provider\(/,
);
const inspectCommand =
  connectorCommands.match(/pub fn inspect_connector_external_result[\s\S]*?\n\}/)?.[0] ?? "";
assert.match(
  inspectCommand,
  /acceptance == ConnectorRecoveryAcceptance::Accepted[\s\S]*?wake_connector_reconciliation_worker/,
  "Only a newly accepted inspection may wake the shared worker",
);
assert.match(inspectCommand, /schedule_connector_reconciliation_from_recovery/);
assert.doesNotMatch(
  inspectCommand,
  /reconcile_due_connector_mutations|reconcile_mutation|ConnectorMutationProvider|connector_runtime|credential/i,
);
const inspectUi =
  recoveryCenter.match(/const inspectExternalResult[\s\S]*?\n  \};/)?.[0] ?? "";
assert.match(inspectUi, /item_id: item\.id/);
assert.match(inspectUi, /action_revision: item\.action\.action_revision/);
assert.doesNotMatch(
  inspectUi,
  /provider|accountId|generation|capability|requestFingerprint|idempotency|target/i,
);
assert.match(appCommands, /connector_registries: Arc<ConnectorRuntimeRegistries>/);
assert.match(appCommands, /ConnectorRuntimeRegistries::empty\(\)/);
assert.doesNotMatch(
  appCommands.match(/pub fn new\([\s\S]*?\n    \}/)?.[0] ?? "",
  /FakeConnectorProvider|Microsoft|Google/,
);
const productionReconciliation = reconciliation.split("#[cfg(test)]")[0];
assert.doesNotMatch(productionReconciliation, /ConnectorMutationProvider/);
assert.doesNotMatch(productionReconciliation, /\.apply_mutation\(/);
assert.doesNotMatch(recoveryCenter, /execute_agent_tool|prepare_connector_attachment|approve_and_reserve|microsoft/i);

assert.match(commands, /enqueue_due_automation_agent_run/);
assert.match(commands, /reconcile_automation_agent_runs/);
assert.match(main, /run_due_automation_sweep/);
assert.match(main, /resume_connector_read_sync,/);
assert.match(main, /inspect_connector_external_result,/);
assert.match(main, /list_connector_read_activity,/);
assert.match(main, /start_explicit_connector_mail_search,/);
assert.match(main, /start_explicit_connector_calendar_list,/);
assert.match(main, /spawn_connector_read_worker\(state\.clone\(\)\)/);
assert.doesNotMatch(main, /reconcile_due_connector_mutations/);
const startupRecoveryCalls = [
  "let _ = reconcile_pending_connector_disconnects(&state);",
  "let _ = recover_pending_connector_authorizations(&state);",
  "let _ = store.reset_abandoned_connector_revocation_claims(chrono::Utc::now());",
  "let _ = store.reset_abandoned_connector_reconciliation_claims(chrono::Utc::now());",
  "let _ = store.reset_expired_connector_sync_recovery_claims(chrono::Utc::now());",
  "let _ = store.reset_connector_read_executions_after_restart(chrono::Utc::now());",
  "let _ = reconcile_incomplete_connector_attachment_landings(&state);",
  "spawn_connector_sync_recovery_worker(state.clone());",
  "spawn_connector_read_worker(state.clone());",
  "spawn_connector_reconciliation_worker(state.clone());",
  "spawn_connector_attachment_recovery_worker(state.clone());",
];
assert.doesNotMatch(
  builtinToolCatalog,
  /start_explicit_connector_(?:mail_search|calendar_list)|connector_read_source|source_invocation_id/,
  "DeepSeek-visible tools must not construct explicit connector read receipts",
);
const authorizationRecovery =
  connectorCommands.match(/pub fn recover_pending_connector_authorizations[\s\S]*?\n\}/)?.[0] ?? "";
assert.match(authorizationRecovery, /recover_due_connector_authorizations/);
assert.doesNotMatch(
  authorizationRecovery,
  /connector_registries|ConnectorOAuthRegistry|\.provider\(|exchange_code|authorization_url|browser|Microsoft|Google|FakeConnectorProvider/,
);
const startupRecoveryOffsets = startupRecoveryCalls.map((call) => main.indexOf(call));
assert.ok(startupRecoveryOffsets.every((offset) => offset >= 0));
assert.ok(
  startupRecoveryOffsets.every(
    (offset, index) => index === 0 || startupRecoveryOffsets[index - 1] < offset,
  ),
);
const reconciliationWorker =
  connectorCommands.match(/pub fn spawn_connector_reconciliation_worker[\s\S]*?\n\}/)?.[0] ?? "";
assert.match(reconciliationWorker, /execution_enabled/);
assert.match(reconciliationWorker, /run_connector_reconciliation_worker_once/);
assert.doesNotMatch(
  reconciliationWorker,
  /Microsoft|Google|FakeConnectorProvider|ConnectorMutationProvider|apply_mutation/,
  "Recovery worker must remain provider-neutral and read-only",
);
const app = readFileSync("apps/desktop/src/App.tsx", "utf8");
assert.match(app, /setInterval\(\(\) => \{[\s\S]*sweepAutomations/);

assert.match(connector, /ConnectorCredentialHandle/);
assert.match(connector, /external connector mutation requires exact tool approval/);
assert.doesNotMatch(connector, /pub\s+struct\s+ConnectorSecret\s*\([^)]*pub/);

console.log("Automation Center and connector boundary checks passed.");
