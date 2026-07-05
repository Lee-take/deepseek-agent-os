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

export type AppUpdateInstallResult = {
  latest_version: string;
  asset_name: string;
  installer_path: string;
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
  model: string;
  cache_status: DeepSeekChatCacheStatus;
  elapsed_ms: number;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  total_tokens: number | null;
  estimated_cost_micro_usd: number | null;
  created_at: string;
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
  | "drive_read"
  | "drive_write"
  | "terminal_read"
  | "terminal_write"
  | "computer_screenshot"
  | "computer_control";

export type CapabilityFamily =
  | "file"
  | "network"
  | "browser"
  | "email"
  | "drive"
  | "terminal"
  | "computer_use";

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
  created_at: string;
};

export type PermissionResolution = {
  id: string;
  request_id: string;
  approved: boolean;
  note: string;
  created_at: string;
};

export type CapabilityAccessRecord = {
  request: CapabilityAccessRequest;
  resolution: PermissionResolution | null;
  effective_status: CapabilityAccessStatus;
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
