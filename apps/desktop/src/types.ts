export type ModelRoute = "auto" | "flash" | "pro";

export type LargeModelProvider = "deepseek" | "chatgpt" | "codex" | "custom";

export type ThinkingLevel = "auto" | "fast" | "standard" | "deep";

export type AccessMode = "ask_every_step" | "ask_on_risk" | "limited_auto" | "full_access";

export type WorkspaceScope = "workspace";

export type Language = "zh" | "en";

export type ThemeStyle = "ink" | "porcelain";

export type TaskRecordStatus = "active" | "done" | "blocked";

export type NetworkSearchSourceModel =
  | "free_web_source"
  | "free_local_browser"
  | "free_source_aggregator";

export type NetworkSearchBackend =
  | "deepseek"
  | "native_large_model"
  | "source_backed_model";

export type NetworkSearchExecutionMode =
  | "permission_audit_only"
  | "source_backed_adapter"
  | "native_bridge_contract";

export type NetworkSearchEvidencePolicy =
  | "pending_user_confirmation"
  | "source_links_required";

export type EmailBackend = "architecture_only";

export type DriveBackend = "local_folder_export_package";

export type ComputerScreenshotBackend =
  | "codex_style_screen_capture"
  | "codex_bridge_screen_capture"
  | "local_windows_screen_capture"
  | "local_macos_screen_capture";

export type ComputerControlBackend =
  | "codex_style_input_control"
  | "codex_bridge_input_control"
  | "local_windows_input_control"
  | "local_macos_input_control";

export type CodexBridgeTransport = "http" | "stdio";

export type RuntimePlatform = "windows" | "macos" | "other";

export type SkillTrustLevel = "untrusted" | "local_declarative" | "remote_declarative";

export type SkillEnablementStatus = "enabled" | "disabled";

export type AgentRunStatus =
  | "queued"
  | "running"
  | "waiting_for_prerequisite"
  | "waiting_for_confirmation"
  | "blocked"
  | "cancel_requested"
  | "completed"
  | "failed"
  | "cancelled";

export type AgentRunRole = "parent" | "subagent";

export type ExpertRole = "research" | "analysis" | "production" | "review";

export type ExpertCapability =
  | "file_read"
  | "network_search"
  | "browser_browse"
  | "managed_staging_write";

export type ExpertResourceRequirement = {
  key: string;
  access: "read" | "write";
};

export type ExpertBudget = {
  max_elapsed_ms: number;
  max_tool_calls: number;
  max_tokens: number;
  max_output_bytes: number;
  max_staged_bytes: number;
};

export type ExpertOutputContract = {
  min_evidence_sources: number;
  require_claims: boolean;
  require_staged_output: boolean;
  require_review: boolean;
  fail_on_unresolved_conflict: boolean;
};

export type ExpertRetryPolicy = {
  max_attempts: number;
  substitute_role: ExpertRole | null;
};

export type AgentSubtaskPlanItem = {
  key: string;
  role: ExpertRole;
  prompt: string;
  depends_on: string[];
  capabilities: ExpertCapability[];
  resources: ExpertResourceRequirement[];
  budget: ExpertBudget;
  output_contract: ExpertOutputContract;
  retry_policy: ExpertRetryPolicy;
};

export type ExpertAttemptContract = AgentSubtaskPlanItem & {
  team_id: string;
  parent_run_id: string;
  parent_input_revision: string;
  attempt: number;
  previous_attempt_run_id: string | null;
};

export type ExpertQualityGate = {
  code: string;
  passed: boolean;
  detail: string;
};

export type ExpertAttemptResult = {
  id: string;
  run_id: string;
  parent_run_id: string;
  key: string;
  role: ExpertRole;
  attempt: number;
  parent_input_revision: string;
  output_revision: string;
  summary: string;
  claims: Array<{
    key: string;
    statement: string;
    stance: "supports" | "contradicts" | "uncertain";
    evidence_refs: string[];
  }>;
  evidence: Array<{
    id: string;
    kind: string;
    reference: string;
    summary: string;
    verified: boolean;
  }>;
  unresolved_conflicts: string[];
  missing_evidence: string[];
  usage: {
    elapsed_ms: number;
    tool_calls: number;
    tokens: number;
    output_bytes: number;
    staged_bytes: number;
  };
  quality_gates: ExpertQualityGate[];
  staging: {
    relative_path: string;
    absolute_path: string;
    sha256: string;
    bytes: number;
  } | null;
  review: {
    target_revision: string;
    decision: "accept" | "reject" | "needs_revision";
    findings: string[];
  } | null;
  external_effect_state: "none" | "verified_read_only" | "managed_staging_only" | "uncertain";
  retry_eligible: boolean;
  recorded_at: string;
};

export type ExpertMergeReceipt = {
  id: string;
  parent_run_id: string;
  parent_input_revision: string;
  production_run_id: string;
  production_revision: string;
  review_run_id: string;
  merged_at: string;
};

export type AgentRunStepStatus = "pending" | "running" | "completed" | "failed";

export type AgentRunQueuedGuidance = {
  id: string;
  run_id: string;
  guidance: string;
  queued_at: string;
  applied_at: string | null;
};

export type AgentRunStepRecord = {
  id: string;
  run_id: string;
  sequence: number;
  status: AgentRunStepStatus;
  label: string;
  detail: string;
  recorded_at: string;
};

export type AgentRunArtifactRecord = {
  id: string;
  run_id: string;
  kind: string;
  title: string;
  path: string;
  created_at: string;
};

export type AgentRunRecord = {
  id: string;
  conversation_id: string;
  prompt: string;
  execution_prompt: string | null;
  execution_context_recorded_at: string | null;
  attachment_count: number;
  role: AgentRunRole;
  parent_run_id: string | null;
  subtask_key: string | null;
  expert_contract: ExpertAttemptContract | null;
  expert_result: ExpertAttemptResult | null;
  expert_merge_receipt: ExpertMergeReceipt | null;
  status: AgentRunStatus;
  worker_id: string | null;
  lease_expires_at: string | null;
  recovery_count: number;
  last_recovered_at: string | null;
  recovery_reason: string | null;
  continuation_count: number;
  continuation_queued_at: string | null;
  continuation_tool_invocation_id: string | null;
  queued_guidance: AgentRunQueuedGuidance[];
  steps: AgentRunStepRecord[];
  artifacts: AgentRunArtifactRecord[];
  cancel_requested: boolean;
  cancel_reason: string | null;
  status_reason: string | null;
  waiting_tool_invocation_id: string | null;
  started_at: string;
  updated_at: string;
  finished_at: string | null;
  finish_summary: string | null;
  finish_error: string | null;
};

export type AutomationDefinition = {
  id: string;
  revision: number;
  goal: string;
  timezone: string;
  schedule:
    | { kind: "once"; run_at: string }
    | { kind: "daily"; hour: number; minute: number }
    | { kind: "weekly"; weekday: number; hour: number; minute: number }
    | { kind: "monthly"; day: number; hour: number; minute: number }
    | { kind: "restricted_cron"; weekdays: number[]; hour: number; minute: number };
  status: "enabled" | "paused" | "deleted";
  missed_run_policy: "skip" | "run_once";
  retry_limit: number;
  missed_after_seconds: number;
  created_at: string;
  updated_at: string;
};

export type AutomationRun = {
  id: string;
  definition_id: string;
  definition_revision: number;
  trigger_window_key: string;
  scheduled_for: string;
  status:
    | "queued"
    | "running"
    | "waiting_review"
    | "waiting_approval"
    | "completed"
    | "failed"
    | "cancelled";
  attempt: number;
  agent_run_id: string | null;
  review_queue_item_id: string | null;
  last_error: string | null;
  claimed_by: string | null;
  claimed_at: string | null;
  created_at: string;
  updated_at: string;
};

export type ReviewQueueItem = {
  id: string;
  automation_run_id: string;
  agent_run_id: string | null;
  tool_invocation_id: string | null;
  status: "pending_review" | "pending_approval" | "accepted" | "rejected";
  preview_fingerprint: string | null;
  revision: number;
  title: string;
  evidence_ref: string | null;
  created_at: string;
  updated_at: string;
};

export type ConnectorAccountSummary = {
  id: string;
  display_name: string;
  provider_label: "microsoft365" | "google_workspace" | "workspace_connector";
  abilities: Array<
    | "mail_read"
    | "mail_attachments"
    | "mail_draft"
    | "mail_send"
    | "mail_sync"
    | "calendar_read"
    | "calendar_change"
    | "calendar_sync"
  >;
  health:
    | "connected"
    | "needs_repair"
    | "disconnect_pending"
    | "disconnected"
    | "revocation_pending";
  health_reason?:
    | "authorization_expired"
    | "disconnect_finishing"
    | "revocation_unconfirmed"
    | null;
  sync_state: "not_enabled" | "never_synced" | "healthy" | "delayed" | "stopped";
  last_successful_sync_at?: string | null;
  repair_action_available: boolean;
  connected_at: string;
  updated_at: string;
};

export type ConnectorAuthorizationReview = {
  review_id: string;
  provider_label: ConnectorAccountSummary["provider_label"];
  abilities: ConnectorAccountSummary["abilities"];
  status:
    | "awaiting_confirmation"
    | "connecting"
    | "connected"
    | "cancelling"
    | "cancelled"
    | "repair_required";
  expires_at: string;
  account?: ConnectorAccountSummary | null;
};

export type ConnectorReadActivity = {
  id: string;
  kind: "mail" | "calendar";
  phase: "queued" | "running" | "completed" | "needs_attention" | "cancelled";
  item_count?: number;
  evidence_ref?: string;
  error_code?:
    | "connection_needs_attention"
    | "provider_temporarily_unavailable"
    | "external_result_uncertain"
    | "evidence_unavailable"
    | "execution_record_unavailable"
    | "read_could_not_complete";
  updated_at: string;
};

export type ArtifactDelivery = {
  id: string;
  format: "word" | "excel" | "power_point" | "pdf";
  phase:
    | "generated"
    | "structure_checked"
    | "visual_checked"
    | "revision_required"
    | "revision_prepared"
    | "ready_for_delivery"
    | "completed"
    | "failed";
  status_code:
    | "generated_check_pending"
    | "structure_passed_visual_pending"
    | "checks_passed_delivery_pending"
    | "revision_in_progress"
    | "completed"
    | "needs_attention";
  structure_checked: boolean;
  visual_checked: boolean;
  revision_attempts: number;
  preview_available: boolean;
  rendered_page_count: number;
  updated_at: string;
};

export type ConnectorRecoveryStatus =
  | "repair_required"
  | "needs_repair"
  | "disconnect_pending"
  | "revocation_pending"
  | "sync_exhausted"
  | "reconciliation_required";

export type ConnectorRecoveryReasonCode =
  | "attachment_legacy_workspace_unbound"
  | "attachment_legacy_receipt_incomplete"
  | "attachment_retention_identity_changed"
  | "attachment_stored_identity_changed"
  | "attachment_execution_record_incomplete"
  | "attachment_recovery_required"
  | "account_needs_repair"
  | "account_disconnect_pending"
  | "account_revocation_pending"
  | "sync_retry_exhausted"
  | "reconciliation_required";

export type ConnectorRecoveryExternalEffectState =
  | "local_file_preserved"
  | "no_external_write"
  | "local_credential_removal_pending"
  | "external_result_uncertain";

export type ConnectorRecoveryNextStepCode =
  | "retry_local_cleanup"
  | "inspect_file_manually"
  | "review_account_connection"
  | "wait_for_local_disconnect_recovery"
  | "repair_account_connection"
  | "verify_provider_state";

export type ConnectorRecoveryAction =
  | { kind: "retry_attachment_cleanup"; action_revision: string }
  | { kind: "resume_sync"; action_revision: string }
  | { kind: "inspect_external_result"; action_revision: string };

export type ConnectorRecoveryItem = {
  id: string;
  kind: "attachment" | "account" | "sync" | "reconciliation";
  status: ConnectorRecoveryStatus;
  title: string;
  reason_code: ConnectorRecoveryReasonCode;
  external_effect_state: ConnectorRecoveryExternalEffectState;
  next_step_code: ConnectorRecoveryNextStepCode;
  sync_capability?: "mail" | "calendar";
  action?: ConnectorRecoveryAction;
  updated_at: string;
};

export type ConnectorRecoveryCommandResult = {
  acceptance: "accepted" | "already_accepted";
  items: ConnectorRecoveryItem[];
};

export type SkillSourceIntegrity = {
  algorithm: string;
  hash: string;
};

export type SkillSource = {
  kind: string;
  url: string;
  integrity: SkillSourceIntegrity | null;
};

export type SkillPackageKind = "skill" | "plugin" | "system_skill";
export type SkillUpdatePolicy = "automatic" | "pinned";
export type SkillUpdateState = "current" | "update_available" | "failed";

export type SkillSourceIdentity = {
  provider: string;
  repository_url: string;
  requested_revision: string | null;
  resolved_revision: string;
  package_path: string | null;
  source_format: string;
};

export type SkillPermissionDeclaration = {
  kind: string;
  scope: string;
  reason: string;
};

export type SkillEntry = {
  kind: string;
  path: string;
};

export type SkillManifest = {
  schema_version: string;
  name: string;
  version: string;
  description: string;
  author: string;
  license: string;
  source: SkillSource;
  capabilities: string[];
  permissions: SkillPermissionDeclaration[];
  entry: SkillEntry;
  trust_level: SkillTrustLevel;
  risk_warnings: string[];
};

export type SkillRecord = {
  id: string;
  manifest: SkillManifest;
  installed_from: string;
  installed_at: string;
  enablement_status: SkillEnablementStatus;
  last_audit_note: string | null;
  updated_at: string;
  package_kind: SkillPackageKind;
  system_protected: boolean;
  source_identity: SkillSourceIdentity | null;
  update_policy: SkillUpdatePolicy;
  update_state: SkillUpdateState;
  last_update_checked_at: string | null;
  last_update_failure: string | null;
  rollback_version: string | null;
  rollback_revision: string | null;
  entry_available: boolean;
  entry_sha256: string | null;
};

export type SkillUpdateSweepResult = {
  checked: number;
  updated: number;
  current: number;
  failed: number;
  failures: string[];
  records: SkillRecord[];
};

export type SkillPackagePreflight = {
  manifest: SkillManifest;
  package_files: string[];
  blocked_files: string[];
  warnings: string[];
  audit_summary: string;
  entry_sha256: string | null;
};

export type SkillSourceVerification = {
  verified: boolean;
  source_kind: string;
  source_url: string;
  integrity_algorithm: string | null;
  integrity_hash: string | null;
  provenance: string;
  checked_at: string;
};

export type SkillExecutionStatus = "planned" | "blocked" | "activated";

export type SkillExecutionRecord = {
  id: string;
  skill_id: string;
  skill_name: string;
  status: SkillExecutionStatus;
  entry_kind: string;
  entry_path: string;
  input_summary: string;
  execution_plan: string;
  blocked_reason: string | null;
  requested_at: string;
  tool_invocation_id: string | null;
  run_id: string | null;
  evidence_ref: string | null;
  completed_at: string | null;
};

export type LocalDirectorySettings = {
  workspace_dir: string;
  workspace_name: string;
  evidence_dir: string;
  export_dir: string;
};

export type LocalDirectoryState = {
  app_data_dir: string;
  settings_file: string;
  settings: LocalDirectorySettings | null;
  needs_setup: boolean;
};

export type AppUpdateStatus = {
  current_version: string;
  latest_version: string | null;
  update_available: boolean;
  asset_name: string | null;
  release_url: string | null;
  message: string | null;
};

export type AppUpdateDownloadResult = {
  latest_version: string;
  asset_name: string;
  installer_path: string;
};

export type AppUpdateInstallResult = {
  installer_path: string;
  restart_scheduled: boolean;
};

export type ToolBackendSettings = {
  network_search: NetworkSearchBackend;
  email: EmailBackend;
  drive: DriveBackend;
  computer_screenshot: ComputerScreenshotBackend;
  computer_control: ComputerControlBackend;
};

export type DeepSeekCredentialStatus = {
  base_url: string;
  chat_completions_url: string;
  api_key_env_var: string;
  api_key_configured: boolean;
  chat_completion_ready: boolean;
  flash_model: string;
  pro_model: string;
  readiness_note: string;
};

export type DeepSeekChatCacheStatus = "disabled" | "hit" | "miss";

export type DeepSeekChatTelemetry = {
  id: string;
  request_hash: string;
  model: string;
  cache_status: DeepSeekChatCacheStatus;
  elapsed_ms: number;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  total_tokens: number | null;
  estimated_cost_micro_usd: number | null;
  created_at: string;
};

export type DeepSeekChatCacheState = {
  entries: number;
};

export type AgentSoulProfileUpdateReceipt = {
  update_id: string;
  status: "applied" | "unchanged" | "blocked";
  summary: string;
  changed_fields: string[];
  undo_available: boolean;
  applied_at: string;
};

export type DeepSeekUserBalanceInfo = {
  currency: string;
  total_balance: string;
  granted_balance: string;
  topped_up_balance: string;
};

export type DeepSeekUserBalanceResponse = {
  is_available: boolean;
  balance_infos: DeepSeekUserBalanceInfo[];
};

export type AgentChatResponse = {
  id: string;
  role: "assistant";
  content: string;
  protocol_version: string;
  proposed_actions: AgentChatActionProposal[];
  missing_prerequisites: AgentChatMissingPrerequisite[];
  memory_candidates: MemoryCandidate[];
  soul_profile_update: AgentSoulProfileUpdateReceipt | null;
  subagent_plan: AgentSubtaskPlanItem[];
  expert_output?: unknown;
  model: string;
  cache_status: DeepSeekChatCacheStatus;
  elapsed_ms: number;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  total_tokens: number | null;
  estimated_cost_micro_usd: number | null;
  created_at: string;
};

export type AgentRunWorkerResult = {
  record: AgentRunRecord;
  response: AgentChatResponse;
};

export type AgentActionExecutionState =
  | "proposed"
  | "waiting_prerequisite"
  | "needs_confirmation"
  | "blocked"
  | "succeeded"
  | "failed";

export type AgentChatActionProposal = {
  action_type: string;
  title: string | null;
  reason: string | null;
  risk: string | null;
  requires_confirmation: boolean;
  target: string | null;
  target_location: string | null;
  destination: string | null;
  preferred_browser: string | null;
  content: string | null;
  capability: CapabilityKind | null;
  policy_decision: PolicyDecision | null;
  execution_state: AgentActionExecutionState;
  dispatch_note: string | null;
  permission_request_id: string | null;
  capability_invocation_id: string | null;
  workflow_run_id: string | null;
  blocked_reason: string | null;
};

export type AgentChatMissingPrerequisite = {
  kind: string;
  message: string;
};

export type DeepSeekPricingSettings = {
  enabled: boolean;
  flash_prompt_usd_per_million_tokens: string;
  flash_completion_usd_per_million_tokens: string;
  pro_prompt_usd_per_million_tokens: string;
  pro_completion_usd_per_million_tokens: string;
};

export type DeepSeekPricingState = {
  app_data_dir: string;
  settings_file: string;
  settings: DeepSeekPricingSettings;
  pricing_configured: boolean;
  note: string;
};

export type NetworkSearchRouteStatus = {
  backend: NetworkSearchBackend;
  execution_mode: NetworkSearchExecutionMode;
  evidence_policy: NetworkSearchEvidencePolicy;
  network_requests_enabled: boolean;
  deepseek_orchestration_ready: boolean;
  requires_user_confirmation: boolean;
  note: string;
};

export type ComputerUseBackendStatus = {
  screenshot_backend: ComputerScreenshotBackend;
  screenshot_available: boolean;
  screenshot_note: string;
  screenshot_permission_required: boolean;
  screenshot_permission_note: string;
  control_backend: ComputerControlBackend;
  control_available: boolean;
  control_requires_approval: boolean;
  control_note: string;
  control_permission_required: boolean;
  control_permission_note: string;
  codex_bridge: CodexBridgeRuntimeStatus;
};

export type CodexBridgeRuntimeStatus = {
  required: boolean;
  transport_env_var: string;
  transport: CodexBridgeTransport | null;
  transport_decision_required: boolean;
  transport_options: CodexBridgeTransportOption[];
  endpoint_env_var: string;
  endpoint_configured: boolean;
  connected: boolean;
  note: string;
};

export type CodexBridgeTransportOption = {
  value: CodexBridgeTransport;
  label: string;
  note: string;
};

export type ComputerControlUnlockStatus = {
  challenge: string;
  unlocked: boolean;
  unlocked_until: string | null;
};

export type ComputerUseStepStatus =
  | "observed"
  | "awaiting_approval"
  | "ready"
  | "action_started"
  | "awaiting_verification"
  | "verified"
  | "needs_replan"
  | "user_taken_over"
  | "effect_unknown"
  | "verification_failed"
  | "cancelled";

export type ComputerUseUndoCapability = "none" | "compensation_required";

export type ComputerUseVerificationOutcome = "verified" | "evidence_only" | "failed";

export type ComputerUseSessionView = {
  id: string;
  run_id: string | null;
  safe_goal_summary: string;
  active_step_id: string | null;
  revision: number;
  created_at: string;
  updated_at: string;
};

export type ComputerUseStepView = {
  id: string;
  session_id: string;
  sequence: number;
  status: ComputerUseStepStatus;
  revision: number;
  pre_observation_fingerprint: string;
  window_fingerprint: string;
  target_fingerprint: string | null;
  pre_semantic_fingerprint: string | null;
  pre_screenshot_evidence_ref: string;
  pre_safe_summary: string;
  action_display: string | null;
  action_safe_summary: string | null;
  action_fingerprint: string | null;
  approval_request_id: string | null;
  post_observation_fingerprint: string | null;
  post_semantic_fingerprint: string | null;
  post_screenshot_evidence_ref: string | null;
  verification_outcome: ComputerUseVerificationOutcome | null;
  verification_safe_summary: string | null;
  undo_capability: ComputerUseUndoCapability;
  status_reason: string | null;
  created_at: string;
  updated_at: string;
};

export type ComputerUseSessionStartResult = {
  session: ComputerUseSessionView;
  step: ComputerUseStepView;
};

export type ComputerUseRunResult = {
  step: ComputerUseStepView;
  capability_invocation: CapabilityInvocation | null;
  execution_summary: string | null;
  safe_error: string | null;
};

export type NetworkSearchSourceModelOption = {
  value: NetworkSearchSourceModel;
  label: string;
  note: string;
};

export type ModelDrivenToolStrategy = {
  large_model_provider: LargeModelProvider;
  large_model_supports_network_search: boolean;
  network_search_source_model_required: boolean;
  network_search_source_model: NetworkSearchSourceModel | null;
  free_network_search_source_model_options: NetworkSearchSourceModelOption[];
  network_search_backend: NetworkSearchBackend;
  computer_screenshot_backend: ComputerScreenshotBackend;
  computer_control_backend: ComputerControlBackend;
  runtime_platform: RuntimePlatform;
  macos_supported: boolean;
  note: string;
};

export type WorkPackageToolReadiness = {
  deepseek: DeepSeekCredentialStatus;
  network_search: NetworkSearchRouteStatus;
  computer_use: ComputerUseBackendStatus;
  local_directories: LocalDirectoryReadinessStatus;
  tool_strategy: ModelDrivenToolStrategy;
};

export type LocalDirectoryReadinessStatus = {
  needs_setup: boolean;
  workspace_configured: boolean;
  evidence_configured: boolean;
  export_configured: boolean;
  paths_redacted: boolean;
  note: string;
};

export type MemoryRecordSource = "task_record" | "memory_candidate";

export type MemoryCandidateSource =
  | "manual"
  | "task_record"
  | "import"
  | "workflow_reflection";

export type MemoryCandidateStatus = "pending" | "accepted" | "rejected";

export type MemorySelectedFeedbackKind =
  | "useful"
  | "irrelevant"
  | "stale"
  | "conflicting"
  | "should_update";

export type MemoryMaintenanceReviewKind = "retrieval" | "update_archive" | "conflict";

export type MemoryMaintenanceActionKind =
  | "mark_reviewed"
  | "snooze"
  | "retrieval_reviewed"
  | "update_candidate_created"
  | "archived";

export type MemoryCandidateSuggestedAction =
  | "new"
  | "update"
  | "merge"
  | "replace"
  | "archive"
  | "link"
  | "reject_hint";

export type MemoryType =
  | "preference"
  | "project_context"
  | "workflow_rule"
  | "artifact"
  | "failure_pattern";

export type MemoryScope = "workspace" | "project" | "organization" | "user";

export type MemorySensitivity = "normal" | "sensitive";

export type MemoryLifecycle = "active" | "archived" | "expires";

export type MemoryRelationKind = "related" | "updates" | "extends" | "derives";

export type MemorySearchMatchSource =
  | "direct"
  | "linked_memory_title"
  | "linked_memory_body";

export type CapabilityKind =
  | "file_read"
  | "file_write"
  | "network_search"
  | "browser_browse"
  | "browser_submit"
  | "email_read"
  | "email_draft"
  | "email_send"
  | "connector_attachment_read"
  | "connector_write"
  | "drive_read"
  | "drive_write"
  | "terminal_read"
  | "terminal_write"
  | "computer_screenshot"
  | "computer_control"
  | "app_update_check"
  | "app_update_download"
  | "app_update_install"
  | "skill_use";

export type CapabilityFamily =
  | "file"
  | "network"
  | "browser"
  | "email"
  | "drive"
  | "terminal"
  | "computer_use"
  | "app_update"
  | "skill";

export type RiskLevel = "low" | "medium" | "high" | "critical";

export type PolicyDecision = "allow" | "ask" | "deny";

export type CapabilityAccessStatus =
  | "auto_approved"
  | "pending_approval"
  | "approved"
  | "rejected"
  | "denied";

export type CapabilityGrantState =
  | "not_granted"
  | "reusable"
  | "one_shot_available"
  | "one_shot_consumed";

export type CapabilityInvocationStatus = "succeeded" | "pending_approval" | "failed";

export type OperationsBriefingRunStatus = "pending_approval" | "draft_ready" | "failed";

export type TerminalReadCommand =
  | "pwd"
  | "git status --short"
  | "git diff --stat"
  | "git branch --show-current";

export type FoundationState = {
  app_name: string;
  large_model_provider: LargeModelProvider;
  model_route: ModelRoute;
  thinking_level: ThinkingLevel;
  access_mode: AccessMode;
  workspace_scope: WorkspaceScope;
  network_search_source_model: NetworkSearchSourceModel | null;
  tool_backends: ToolBackendSettings;
};

export type TaskRecord = {
  id: string;
  title: string;
  summary: string;
  status: TaskRecordStatus;
  created_at: string;
  updated_at: string;
};

export type MemoryRecord = {
  id: string;
  title: string;
  body: string;
  memory_type: MemoryType;
  scope: MemoryScope;
  sensitivity: MemorySensitivity;
  lifecycle: MemoryLifecycle;
  source: MemoryRecordSource;
  source_id: string | null;
  pinned: boolean;
  expires_at: string | null;
  linked_memory_ids: string[];
  linked_memories: MemoryRecordLinkSummary[];
  search_match?: MemorySearchMatch;
  created_at: string;
  updated_at: string;
};

export type MemorySearchMatch = {
  source: MemorySearchMatchSource;
  linked_memory_id: string | null;
  relation: MemoryRelationKind | null;
};

export type MemoryRecordLinkSummary = {
  id: string;
  title: string;
  memory_type: MemoryType;
  scope: MemoryScope;
  relation: MemoryRelationKind;
  note: string;
  updated_at: string;
};

export type MemoryRecordDeletion = {
  id: string;
  memory_id: string;
  note: string;
  deleted_at: string;
};

export type MemorySelectedFeedback = {
  id: string;
  memory_id: string;
  context_receipt_id: string | null;
  feedback: MemorySelectedFeedbackKind;
  note: string;
  created_at: string;
};

export type MemoryMaintenanceFeedbackCounts = Record<MemorySelectedFeedbackKind, number>;

export type MemoryMaintenanceReviewAction = {
  id: string;
  memory_id: string;
  action: MemoryMaintenanceActionKind;
  note: string;
  snoozed_until: string | null;
  created_at: string;
};

export type MemoryMaintenanceReviewItem = {
  memory: MemoryRecord;
  feedback_counts: MemoryMaintenanceFeedbackCounts;
  feedback_count: number;
  quality_score: number;
  quality_signals: string[];
  latest_feedback: MemorySelectedFeedback | null;
  review_kinds: MemoryMaintenanceReviewKind[];
  recommended_actions: MemoryMaintenanceActionKind[];
  review_needed: boolean;
  snoozed_until: string | null;
  last_action: MemoryMaintenanceReviewAction | null;
};

export type MemoryBackgroundMaintenanceActionSummary = {
  memory_id: string | null;
  memory_title: string;
  action: string;
  outcome: string;
  reason: string;
  feedback: MemorySelectedFeedbackKind | null;
  model_used: boolean;
  audit_note: string;
};

export type MemoryBackgroundMaintenanceSummary = {
  retrieval_reviews_marked: number;
  update_candidates_created: number;
  merge_candidates_created: number;
  auto_candidate_decisions_applied: number;
  auto_updates_applied: number;
  auto_merges_applied: number;
  auto_archives_applied: number;
  model_update_rewrites_used: number;
  actions: MemoryBackgroundMaintenanceActionSummary[];
};

export type MemoryRecordUpdate = {
  id: string;
  memory_id: string;
  title: string;
  body: string;
  memory_type: MemoryType;
  scope: MemoryScope;
  sensitivity: MemorySensitivity;
  lifecycle: MemoryLifecycle;
  pinned: boolean;
  expires_at: string | null;
  note: string;
  updated_at: string;
};

export type MemoryCandidate = {
  id: string;
  title: string;
  body: string;
  memory_type: MemoryType;
  scope: MemoryScope;
  sensitivity: MemorySensitivity;
  lifecycle: MemoryLifecycle;
  source: MemoryCandidateSource;
  source_id: string | null;
  rationale: string;
  evidence_excerpt: string;
  privacy_review: string;
  suggested_action: MemoryCandidateSuggestedAction;
  expires_at: string | null;
  created_at: string;
  updated_at: string;
};

export type MemoryCandidateResolution = {
  id: string;
  candidate_id: string;
  accepted: boolean;
  note: string;
  created_at: string;
};

export type MemoryConflictSummary = {
  id: string;
  title: string;
  body: string;
  memory_type: MemoryType;
  scope: MemoryScope;
  sensitivity: MemorySensitivity;
  lifecycle: MemoryLifecycle;
  source: MemoryRecordSource;
  source_id: string | null;
  expires_at: string | null;
  updated_at: string;
};

export type MemoryCandidateRecord = {
  candidate: MemoryCandidate;
  resolution: MemoryCandidateResolution | null;
  effective_status: MemoryCandidateStatus;
  conflicting_memory_ids: string[];
  conflicting_memories: MemoryConflictSummary[];
};

export type MemoryCandidateMergePreview = {
  candidate_id: string;
  source_memory_ids: string[];
  title: string;
  body: string;
  memory_type: MemoryType;
  scope: MemoryScope;
  sensitivity: MemorySensitivity;
  lifecycle: MemoryLifecycle;
  expires_at: string | null;
};

export type MemoryCandidateReplacePreview = {
  candidate_id: string;
  target_memory_ids: string[];
  replacement_title: string;
  replacement_body: string;
  memory_type: MemoryType;
  scope: MemoryScope;
  sensitivity: MemorySensitivity;
  lifecycle: MemoryLifecycle;
  expires_at: string | null;
  target_memories: MemoryConflictSummary[];
};

export type PermissionAuditEntry = {
  id: string;
  access_mode: AccessMode;
  capability: CapabilityKind;
  risk_level: RiskLevel;
  decision: PolicyDecision;
  reason: string;
  created_at: string;
};

export type CapabilityDescriptor = {
  family: CapabilityFamily;
  capability: CapabilityKind;
  title: string;
  summary: string;
  risk_level: RiskLevel;
  default_scope: string;
  experimental: boolean;
};

export type CapabilityAccessRequest = {
  id: string;
  access_mode: AccessMode;
  family: CapabilityFamily;
  capability: CapabilityKind;
  title: string;
  summary: string;
  risk_level: RiskLevel;
  decision: PolicyDecision;
  status: CapabilityAccessStatus;
  reason: string;
  exact_tool: {
    tool_id: string;
    request_fingerprint: string;
    preview: string;
    preview_revision: number;
    preview_hash: string;
  } | null;
  created_at: string;
};

export type PermissionResolution = {
  id: string;
  request_id: string;
  approved: boolean;
  note: string;
  expected_request_revision: number | null;
  exact_preview_revision: number | null;
  exact_preview_hash: string | null;
  created_at: string;
};

export type CapabilityAccessRecord = {
  request: CapabilityAccessRequest;
  resolution: PermissionResolution | null;
  effective_status: CapabilityAccessStatus;
  projection_revision: number;
  grant_state: CapabilityGrantState;
};

export type CapabilityInvocation = {
  id: string;
  capability: CapabilityKind;
  status: CapabilityInvocationStatus;
  policy_decision: PolicyDecision;
  approval_request_id: string | null;
  requested_resource: string | null;
  evidence_ref: string | null;
  requested_url: string | null;
  evidence_url: string | null;
  title: string | null;
  excerpt: string | null;
  warnings: string[];
  elapsed_ms: number;
  created_at: string;
};

export type ToolValueType = "string" | "boolean" | "number" | "object" | "array";

export type ToolFieldSchema = {
  name: string;
  value_type: ToolValueType;
  nullable: boolean;
  description: string;
};

export type ToolObjectSchema = {
  properties: ToolFieldSchema[];
  required: string[];
  allow_additional: boolean;
};

export type ToolPathScope =
  | "none"
  | "workspace"
  | "local_filesystem"
  | "app_evidence_directory"
  | "app_update_directory"
  | "installed_skill_store";

export type ToolResourceAccess = "read" | "write";

export type ToolResourceRequirement = {
  key: string;
  access: ToolResourceAccess;
  lease_seconds: number;
};

export type ToolConstraints = {
  allowed_network_hosts: string[];
  path_scope: ToolPathScope;
  mutates_machine_state: boolean;
  protected_path_policy: string;
  resource: ToolResourceRequirement | null;
};

export type ToolVerificationContract = {
  recipe_id: string;
  description: string;
  required_evidence_kinds: string[];
};

export type AgentToolContract = {
  id: string;
  version: string;
  title: string;
  description: string;
  capability: CapabilityKind;
  risk_level: RiskLevel;
  executor_id: string;
  input_schema: ToolObjectSchema;
  output_schema: ToolObjectSchema;
  constraints: ToolConstraints;
  verification: ToolVerificationContract;
  recovery_hint: string;
};

export type ToolExecutionStatus =
  | "waiting_for_confirmation"
  | "running"
  | "succeeded"
  | "failed"
  | "blocked";

export type ToolEvidence = {
  kind: string;
  reference: string;
  summary: string;
};

export type ToolVerificationResult = {
  passed: boolean;
  summary: string;
  checked_at: string;
};

export type ToolInvocationRecord = {
  id: string;
  run_id: string | null;
  tool_id: string;
  tool_version: string;
  capability: CapabilityKind;
  status: ToolExecutionStatus;
  policy_decision: PolicyDecision;
  approval_request_id: string | null;
  input_summary: string;
  request_fingerprint: string;
  output: Record<string, unknown> | null;
  evidence: ToolEvidence[];
  verification: ToolVerificationResult;
  error: string | null;
  recovery_hint: string;
  elapsed_ms: number;
  created_at: string;
  finished_at: string | null;
};

export type AgentContextReceipt = {
  id: string;
  user_intent: string;
  loop_mode: string;
  action_type: string;
  execution_state: string;
  capability: string | null;
  policy_decision: string | null;
  capability_invocation_id: string | null;
  workflow_run_id: string | null;
  selected_evidence: string[];
  selected_memories: string[];
  memory_candidate_gate: string[];
  model_route: string;
  thinking_level: string;
  token_cache_state: string;
  allowed_tools: string[];
  validators: string[];
  stop_conditions: string[];
  matched_stop_conditions: string[];
  confirmation_rule: string;
  policy_constraints: string[];
  validation_results: string[];
  intentional_omissions: string[];
  created_at: string;
};

export type AgentSoulProfileState = {
  exists: boolean;
  content: string;
  summary_lines: string[];
  used_bytes: number;
  max_bytes: number;
};

export type OperationsBriefingAnomaly = {
  area: string;
  signal: string;
  evidence_ref: string | null;
};

export type OperationsBriefingAction = {
  owner: string;
  action: string;
  due_hint: string;
};

export type OperationsBriefingContextReceipt = {
  user_intent: string;
  loop_mode: string;
  workflow_policy: string;
  selected_evidence: string[];
  selected_memories: string[];
  model_route: string;
  thinking_level: string;
  token_cache_state: string;
  validation_results: string[];
  intentional_omissions: string[];
};

export type OperationsBriefingRun = {
  id: string;
  workflow_id: string;
  status: OperationsBriefingRunStatus;
  archived_from_package: boolean;
  evidence_folder_path: string | null;
  evidence_invocation_id: string | null;
  title: string;
  summary: string;
  anomalies: OperationsBriefingAnomaly[];
  action_plan: OperationsBriefingAction[];
  warnings: string[];
  context_receipt: OperationsBriefingContextReceipt;
  created_at: string;
};

export type WorkPackage = {
  version: string;
  exported_at: string;
  foundation_state: FoundationState;
  tool_readiness: WorkPackageToolReadiness;
  task_records: TaskRecord[];
  memory_candidates: MemoryCandidate[];
  operations_briefing_runs: OperationsBriefingRun[];
  workflow_templates: WorkflowTemplatePackage[];
};

export type WorkflowTemplateFile = {
  path: string;
  content: string;
};

export type WorkflowTemplatePackage = {
  id: string;
  workflow_id: string;
  title: string;
  description: string;
  files: WorkflowTemplateFile[];
};

export type WorkPackageImportSummary = {
  imported: number;
  skipped: number;
  memory_candidates: WorkPackageMemoryCandidateImportSummary;
  operations_briefing_runs: WorkPackageOperationsBriefingImportSummary;
  workflow_templates: WorkPackageWorkflowTemplateImportSummary;
};

export type WorkPackageMemoryCandidateImportSummary = {
  imported: number;
  skipped: number;
};

export type WorkPackageOperationsBriefingImportSummary = {
  imported: number;
  skipped: number;
};

export type WorkPackageWorkflowTemplateImportSummary = {
  imported: number;
  skipped: number;
};

export type WorkPackageTaskImportPreview = {
  total: number;
  new: number;
  skipped: number;
};

export type WorkPackageOperationsBriefingImportPreview = {
  total: number;
  new: number;
  skipped: number;
  replay_supported: boolean;
};

export type WorkPackageWorkflowTemplateImportPreview = {
  total: number;
  new: number;
  skipped: number;
  import_supported: boolean;
};

export type WorkPackageMemoryCandidateImportPreview = {
  total: number;
  new: number;
  skipped: number;
  review_supported: boolean;
};

export type WorkPackageImportPreview = {
  task_records: WorkPackageTaskImportPreview;
  memory_candidates: WorkPackageMemoryCandidateImportPreview;
  operations_briefing_runs: WorkPackageOperationsBriefingImportPreview;
  workflow_templates: WorkPackageWorkflowTemplateImportPreview;
};
