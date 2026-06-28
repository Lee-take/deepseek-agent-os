use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum ModelRoute {
    Auto,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ThinkingLevel {
    Auto,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum AccessMode {
    AskOnRisk,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum WorkspaceScope {
    Workspace,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FoundationState {
    app_name: &'static str,
    model_route: ModelRoute,
    thinking_level: ThinkingLevel,
    access_mode: AccessMode,
    workspace_scope: WorkspaceScope,
}

#[tauri::command]
fn get_foundation_state() -> FoundationState {
    FoundationState {
        app_name: "DeepSeek Agent OS",
        model_route: ModelRoute::Auto,
        thinking_level: ThinkingLevel::Auto,
        access_mode: AccessMode::AskOnRisk,
        workspace_scope: WorkspaceScope::Workspace,
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_foundation_state])
        .run(tauri::generate_context!())
        .expect("failed to run DeepSeek Agent OS desktop app");
}
