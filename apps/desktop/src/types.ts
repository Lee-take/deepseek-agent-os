export type ModelRoute = "auto" | "deepseek-v4-flash" | "deepseek-v4-pro";

export type ThinkingLevel = "auto" | "fast" | "standard" | "deep";

export type AccessMode = "ask_every_step" | "ask_on_risk" | "limited_auto" | "full_access";

export type WorkspaceScope = "current_file" | "current_folder" | "workspace";

export type FoundationState = {
  appName: string;
  modelRoute: ModelRoute;
  thinkingLevel: ThinkingLevel;
  accessMode: AccessMode;
  workspaceScope: WorkspaceScope;
};
