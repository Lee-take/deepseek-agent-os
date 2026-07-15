use tauri::State;
use uuid::Uuid;

use crate::commands::AppState;
use crate::kernel::workspace_undo::{apply_workspace_undo, WorkspaceUndoResult, WorkspaceUndoView};

#[tauri::command]
pub fn list_workspace_undo_items(
    state: State<'_, AppState>,
) -> Result<Vec<WorkspaceUndoView>, String> {
    let event_store = state.event_store();
    let store = event_store.lock().map_err(|_| lock_error())?;
    store
        .list_workspace_undo_views()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn undo_workspace_mutation(
    checkpoint_id: Uuid,
    action_revision: String,
    state: State<'_, AppState>,
) -> Result<WorkspaceUndoResult, String> {
    let event_store = state.event_store();
    let store = event_store.lock().map_err(|_| lock_error())?;
    apply_workspace_undo(&store, checkpoint_id, action_revision.trim())
}

fn lock_error() -> String {
    "workspace recovery state is unavailable".to_string()
}
