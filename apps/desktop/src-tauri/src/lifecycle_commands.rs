use tauri::State;

use crate::commands::AppState;
use crate::kernel::task_lifecycle::TaskLifecycleSnapshot;

#[tauri::command]
pub fn list_task_lifecycle(state: State<'_, AppState>) -> Result<TaskLifecycleSnapshot, String> {
    let event_store = state.event_store();
    let store = event_store
        .lock()
        .map_err(|_| "event store lock failed".to_string())?;
    store
        .task_lifecycle_snapshot()
        .map_err(|error| error.to_string())
}
