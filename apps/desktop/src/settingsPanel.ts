export type SettingsPanelItemControl =
  | "password"
  | "select"
  | "directory_picker"
  | "balance_reader"
  | "profile_editor";

export type SettingsPanelItemId =
  | "deepseek_api_key"
  | "deepseek_fallback_api_key"
  | "deepseek_model"
  | "deepseek_thinking"
  | "interface_style"
  | "soul_profile"
  | "workspace_directory"
  | "deepseek_balance";

export type SettingsPanelItem = {
  id: SettingsPanelItemId;
  control: SettingsPanelItemControl;
  autoSaveOnChange?: boolean;
};

export const settingsPanelItems: SettingsPanelItem[] = [
  { id: "deepseek_api_key", control: "password" },
  { id: "deepseek_fallback_api_key", control: "password" },
  { id: "deepseek_model", control: "select" },
  { id: "deepseek_thinking", control: "select" },
  { id: "interface_style", control: "select" },
  { id: "soul_profile", control: "profile_editor" },
  { id: "workspace_directory", control: "directory_picker", autoSaveOnChange: true },
  { id: "deepseek_balance", control: "balance_reader" },
];

export function shouldExposePluginsSidebarEntry(): boolean {
  return false;
}

export function deepSeekApiKeyCandidates(primaryApiKey: string, fallbackApiKey: string): string[] {
  const candidates = [primaryApiKey, fallbackApiKey]
    .map((value) => value.trim())
    .filter(Boolean);

  return Array.from(new Set(candidates));
}
