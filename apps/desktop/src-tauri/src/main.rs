mod commands;
mod kernel;

use commands::{
    browse_url, capture_computer_screenshot, check_app_update, clear_deepseek_chat_cache,
    control_computer_boundary, create_email_draft_boundary, create_task_record,
    delete_memory_record, export_operations_briefing_html_report,
    export_operations_briefing_pdf_report, export_operations_briefing_report, export_work_package,
    get_computer_control_unlock_status, get_computer_use_backend_status,
    get_computer_use_backend_status_for_model, get_deepseek_chat_cache_state,
    get_deepseek_credential_status, get_deepseek_pricing_state, get_deepseek_user_balance,
    get_foundation_state, get_local_directory_state, get_model_driven_tool_strategy,
    get_network_search_route_status, get_network_search_route_status_for_model,
    import_work_package, ingest_evidence_folder, install_app_update,
    link_memory_candidate_to_conflicts, link_memory_records, list_agent_context_receipts,
    list_capability_access_records, list_capability_catalog, list_capability_invocations,
    list_deepseek_chat_telemetry, list_memory_candidate_records, list_memory_records,
    list_operations_briefing_runs, list_pending_capability_access_records,
    list_permission_audit_entries, list_task_records, merge_memory_candidate_with_conflicts,
    preview_memory_candidate_merge, preview_memory_candidate_replace, preview_work_package_import,
    propose_memory_candidate, read_drive_boundary, read_email_boundary, read_local_file,
    record_permission_audit, replace_memory_candidate_conflicts, request_capability_access,
    resolve_capability_access_request, resolve_memory_candidate, resume_agent_chat_action,
    run_agent_chat, run_operations_briefing, run_terminal_read, run_terminal_write,
    save_deepseek_pricing_settings, save_local_directory_settings, search_memory_records,
    search_network_boundary, seed_operations_briefing_evidence_templates, send_email_boundary,
    submit_browser_boundary, unlock_computer_control, update_memory_record, write_drive_boundary,
    write_file_boundary, AppState,
};
use kernel::event_store::EventStore;
use tauri::{image::Image, Manager};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let event_store = EventStore::open(app_data_dir.join("kernel-events.sqlite3"))?;
            app.manage(AppState::new(event_store));
            if let Some(window) = app.get_webview_window("main") {
                window.set_icon(Image::from_bytes(include_bytes!("../icons/icon.ico"))?)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_foundation_state,
            check_app_update,
            install_app_update,
            get_deepseek_credential_status,
            get_network_search_route_status,
            get_computer_use_backend_status,
            get_network_search_route_status_for_model,
            get_computer_use_backend_status_for_model,
            get_model_driven_tool_strategy,
            get_computer_control_unlock_status,
            get_deepseek_chat_cache_state,
            run_agent_chat,
            resume_agent_chat_action,
            get_deepseek_user_balance,
            clear_deepseek_chat_cache,
            list_deepseek_chat_telemetry,
            list_agent_context_receipts,
            get_deepseek_pricing_state,
            save_deepseek_pricing_settings,
            unlock_computer_control,
            get_local_directory_state,
            save_local_directory_settings,
            list_task_records,
            list_memory_records,
            list_memory_candidate_records,
            search_memory_records,
            propose_memory_candidate,
            resolve_memory_candidate,
            preview_memory_candidate_merge,
            preview_memory_candidate_replace,
            merge_memory_candidate_with_conflicts,
            replace_memory_candidate_conflicts,
            link_memory_candidate_to_conflicts,
            link_memory_records,
            update_memory_record,
            delete_memory_record,
            list_capability_catalog,
            list_capability_access_records,
            list_pending_capability_access_records,
            list_capability_invocations,
            list_operations_briefing_runs,
            request_capability_access,
            resolve_capability_access_request,
            browse_url,
            submit_browser_boundary,
            search_network_boundary,
            read_local_file,
            write_file_boundary,
            ingest_evidence_folder,
            run_terminal_read,
            run_terminal_write,
            capture_computer_screenshot,
            control_computer_boundary,
            read_email_boundary,
            create_email_draft_boundary,
            send_email_boundary,
            read_drive_boundary,
            write_drive_boundary,
            run_operations_briefing,
            export_operations_briefing_report,
            export_operations_briefing_html_report,
            export_operations_briefing_pdf_report,
            seed_operations_briefing_evidence_templates,
            list_permission_audit_entries,
            record_permission_audit,
            create_task_record,
            export_work_package,
            import_work_package,
            preview_work_package_import
        ])
        .run(tauri::generate_context!())
        .expect("failed to run DS Agent desktop app");
}
