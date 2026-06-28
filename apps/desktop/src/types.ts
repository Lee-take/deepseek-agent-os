export type ModelRoute = "auto" | "flash" | "pro";

export type ThinkingLevel = "auto" | "fast" | "standard" | "deep";

export type AccessMode = "ask_every_step" | "ask_on_risk" | "limited_auto" | "full_access";

export type WorkspaceScope = "workspace";

export type Language = "zh" | "en";

export type ThemeStyle = "deep" | "ink" | "porcelain";

export type TaskRecordStatus = "active" | "done" | "blocked";

export type MemoryRecordSource = "task_record";

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

export type RiskLevel = "low" | "medium" | "high" | "critical";

export type PolicyDecision = "allow" | "ask" | "deny";

export type FoundationState = {
  app_name: string;
  model_route: ModelRoute;
  thinking_level: ThinkingLevel;
  access_mode: AccessMode;
  workspace_scope: WorkspaceScope;
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
  source: MemoryRecordSource;
  source_id: string | null;
  pinned: boolean;
  created_at: string;
  updated_at: string;
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

export type WorkPackage = {
  version: string;
  exported_at: string;
  foundation_state: FoundationState;
  task_records: TaskRecord[];
};

export type WorkPackageImportSummary = {
  imported: number;
  skipped: number;
};
