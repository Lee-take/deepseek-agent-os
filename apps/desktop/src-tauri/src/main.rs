mod commands;
mod kernel;

use commands::{
    create_task_record, export_work_package, get_foundation_state, import_work_package,
    list_memory_records, list_permission_audit_entries, list_task_records, record_permission_audit,
    AppState,
};
use kernel::event_store::EventStore;
use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let event_store = EventStore::open(app_data_dir.join("kernel-events.sqlite3"))?;
            app.manage(AppState::new(event_store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_foundation_state,
            list_task_records,
            list_memory_records,
            list_permission_audit_entries,
            record_permission_audit,
            create_task_record,
            export_work_package,
            import_work_package
        ])
        .run(tauri::generate_context!())
        .expect("failed to run DeepSeek Agent OS desktop app");
}
