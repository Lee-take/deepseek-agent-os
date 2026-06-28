export type ModelRoute = "auto" | "flash" | "pro";

export type ThinkingLevel = "auto" | "fast" | "standard" | "deep";

export type AccessMode = "ask_every_step" | "ask_on_risk" | "limited_auto" | "full_access";

export type WorkspaceScope = "workspace";

export type Language = "zh" | "en";

export type ThemeStyle = "deep" | "ink" | "porcelain";

export type TaskRecordStatus = "active" | "done" | "blocked";

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
