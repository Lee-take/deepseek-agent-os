#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod app_update_commands;
mod artifact_commands;
mod automation_commands;
mod commands;
mod connector_commands;
mod kernel;

use app_update_commands::{check_app_update, download_app_update, install_app_update};
use artifact_commands::{
    get_artifact_visual_preview, list_artifact_deliveries, spawn_artifact_recovery_worker,
};
use automation_commands::{
    create_once_automation, create_recurring_automation, delete_automation,
    edit_automation_review_item, enqueue_due_automation, list_automation_definitions,
    list_automation_review_items, list_automation_runs, reconcile_automation_runs,
    resolve_automation_review_item, run_automation_now, run_due_automation_sweep,
    set_automation_enabled, update_automation_goal,
};

use commands::{
    archive_memory_candidate_conflicts, archive_memory_from_maintenance_review,
    bind_durable_computer_use_action, browse_url, cancel_durable_computer_use_step,
    capture_computer_screenshot, claim_agent_run_record, claim_next_agent_run_record,
    clear_deepseek_chat_cache, control_computer_boundary, create_email_draft_boundary,
    create_generated_skill, create_task_record, delete_memory_record, enqueue_agent_run_record,
    enqueue_subagent_run_records, ensure_system_skill_builder, execute_agent_tool,
    export_operations_briefing_html_report, export_operations_briefing_pdf_report,
    export_operations_briefing_report, export_work_package, finish_agent_run_record,
    get_agent_soul_profile, get_computer_control_unlock_status, get_computer_use_backend_status,
    get_computer_use_backend_status_for_model, get_deepseek_chat_cache_state,
    get_deepseek_credential_status, get_deepseek_pricing_state, get_deepseek_user_balance,
    get_foundation_state, get_local_directory_state, get_model_driven_tool_strategy,
    get_network_search_route_status, get_network_search_route_status_for_model,
    import_work_package, ingest_evidence_folder, install_local_skill_manifest,
    install_local_skill_zip_package, install_remote_skill_zip_package,
    install_skill_from_repository_url, link_memory_candidate_to_conflicts, link_memory_records,
    list_agent_context_receipts, list_agent_run_records, list_agent_tool_contracts,
    list_agent_tool_invocations, list_capability_access_records, list_capability_catalog,
    list_capability_invocations, list_deepseek_chat_telemetry, list_durable_computer_use_sessions,
    list_durable_computer_use_steps, list_memory_candidate_records,
    list_memory_maintenance_reviews, list_memory_records, list_operations_briefing_runs,
    list_pending_capability_access_records, list_permission_audit_entries,
    list_selected_memory_feedback, list_skill_execution_records, list_skill_records,
    list_task_records, merge_memory_candidate_with_conflicts, prepare_skill_execution_record,
    preview_local_skill_package_manifest, preview_local_skill_zip_package,
    preview_memory_candidate_merge, preview_memory_candidate_replace,
    preview_remote_skill_zip_package, preview_work_package_import, propose_memory_candidate,
    propose_memory_update_candidate_from_feedback, queue_agent_run_guidance_record,
    queue_expert_team_retries, queue_parent_agent_synthesis, read_drive_boundary,
    read_email_boundary, read_local_file, record_agent_run_artifact_record,
    record_agent_run_step_record, record_memory_maintenance_review_action, record_permission_audit,
    record_selected_memory_feedback, reobserve_durable_computer_use_session,
    replace_memory_candidate_conflicts, request_agent_run_cancel_record, request_capability_access,
    reset_skill_trust, resolve_capability_access_request, resolve_memory_candidate,
    resume_agent_chat_action, run_agent_chat, run_durable_computer_use_step,
    run_memory_background_maintenance, run_next_queued_agent_chat_worker, run_operations_briefing,
    run_skill_update_sweep, run_terminal_read, run_terminal_write, save_agent_soul_profile,
    save_deepseek_pricing_settings, save_local_directory_settings, search_memory_records,
    search_network_boundary, seed_operations_briefing_evidence_templates, send_email_boundary,
    set_skill_enabled, stage_agent_attachments, start_agent_run_record,
    start_durable_computer_use_session, submit_browser_boundary,
    take_over_durable_computer_use_step, uninstall_skill, unlock_computer_control,
    update_memory_candidate_conflict, update_memory_record, verify_skill_source,
    write_drive_boundary, write_file_boundary, AppState,
};
use connector_commands::{
    disconnect_connector_account, get_connector_authorization_review,
    inspect_connector_external_result, list_connector_account_summaries,
    list_connector_authorization_reviews, list_connector_provider_catalog,
    list_connector_read_activity, list_connector_recovery_items,
    reconcile_incomplete_connector_attachment_landings, resolve_connector_authorization_review,
    resume_connector_read_sync, retry_connector_attachment_cleanup,
    spawn_connector_attachment_recovery_worker, spawn_connector_read_worker,
    spawn_connector_reconciliation_worker, spawn_connector_sync_recovery_worker,
    start_explicit_connector_calendar_list, start_explicit_connector_mail_search,
};
#[cfg(windows)]
use connector_commands::{
    reconcile_pending_connector_disconnects, recover_pending_connector_authorizations,
};
use kernel::event_store::EventStore;
use tauri::{image::Image, Manager};

const APP_ICON_BYTES: &[u8] = include_bytes!("../icons/icon.ico");
#[cfg(windows)]
const UI_SMOKE_REMOTE_DEBUGGING_PORT_ENV: &str = "DS_AGENT_UI_SMOKE_REMOTE_DEBUGGING_PORT";

#[cfg(windows)]
struct WindowsSingleInstanceGuard(#[allow(dead_code)] std::fs::File);

#[cfg(windows)]
impl WindowsSingleInstanceGuard {
    fn acquire(app_data_dir: &std::path::Path) -> Result<Self, String> {
        use std::os::windows::fs::OpenOptionsExt;

        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .share_mode(0)
            .open(app_data_dir.join("runtime-instance.lock"))
            .map(Self)
            .map_err(|_| "another DS Agent instance already owns the runtime".to_string())
    }
}

#[cfg(windows)]
fn configure_ui_smoke_remote_debugging(context: &mut tauri::Context<tauri::Wry>) {
    let Some(arguments) = ui_smoke_browser_arguments(
        std::env::var(UI_SMOKE_REMOTE_DEBUGGING_PORT_ENV)
            .ok()
            .as_deref(),
    ) else {
        return;
    };

    if let Some(window) = context
        .config_mut()
        .app
        .windows
        .iter_mut()
        .find(|window| window.label == "main")
    {
        window.additional_browser_args = Some(arguments);
    }
}

#[cfg(windows)]
fn ui_smoke_browser_arguments(raw_port: Option<&str>) -> Option<String> {
    let port = raw_port?.trim().parse::<u16>().ok()?;
    if port == 0 {
        return None;
    }

    Some(format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
         --autoplay-policy=no-user-gesture-required \
         --remote-debugging-address=127.0.0.1 \
         --remote-debugging-port={port} \
         --remote-allow-origins=*"
    ))
}

fn apply_main_window_icon(window: &tauri::WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
    window.set_icon(Image::from_bytes(APP_ICON_BYTES)?)?;
    #[cfg(windows)]
    apply_windows_window_icons(window)?;
    Ok(())
}

#[cfg(windows)]
fn apply_windows_window_icons(
    window: &tauri::WebviewWindow,
) -> Result<(), Box<dyn std::error::Error>> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateIconFromResourceEx, GetSystemMetrics, SendMessageW, ICON_BIG, ICON_SMALL,
        ICON_SMALL2, LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON, WM_SETICON,
    };

    let hwnd = window.hwnd()?;
    let big_size = unsafe { GetSystemMetrics(SM_CXICON) }.max(32);
    let small_size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);

    for (slot, desired_size) in [
        (ICON_BIG, big_size),
        (ICON_SMALL, small_size),
        (ICON_SMALL2, small_size),
    ] {
        let icon_resource = select_ico_resource(APP_ICON_BYTES, desired_size as u32)
            .ok_or_else(|| format!("app icon is missing a usable {desired_size}px frame"))?;
        let hicon = unsafe {
            CreateIconFromResourceEx(
                icon_resource,
                true,
                0x0003_0000,
                desired_size,
                desired_size,
                LR_DEFAULTCOLOR,
            )?
        };
        unsafe {
            let _ = SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(slot as usize)),
                Some(LPARAM(hicon.0 as isize)),
            );
        }
    }

    Ok(())
}

fn select_ico_resource(icon_bytes: &[u8], desired_size: u32) -> Option<&[u8]> {
    if read_u16_le(icon_bytes, 0)? != 0 || read_u16_le(icon_bytes, 2)? != 1 {
        return None;
    }
    let count = read_u16_le(icon_bytes, 4)? as usize;
    let mut best: Option<(u32, u32, usize, usize)> = None;

    for index in 0..count {
        let entry_offset = 6 + index * 16;
        let width = match *icon_bytes.get(entry_offset)? {
            0 => 256,
            value => value as u32,
        };
        let height = match *icon_bytes.get(entry_offset + 1)? {
            0 => 256,
            value => value as u32,
        };
        if width != height {
            continue;
        }
        let resource_size = read_u32_le(icon_bytes, entry_offset + 8)? as usize;
        let resource_offset = read_u32_le(icon_bytes, entry_offset + 12)? as usize;
        if resource_offset.checked_add(resource_size)? > icon_bytes.len() {
            continue;
        }
        let distance = width.abs_diff(desired_size);
        let is_downscale = u32::from(width < desired_size);
        let candidate = (distance, is_downscale, resource_offset, resource_size);
        if best.map_or(true, |current| candidate < current) {
            best = Some(candidate);
        }
    }

    let (_, _, resource_offset, resource_size) = best?;
    icon_bytes.get(resource_offset..resource_offset + resource_size)
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn main() {
    let mut context = tauri::generate_context!();
    #[cfg(windows)]
    configure_ui_smoke_remote_debugging(&mut context);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            #[cfg(windows)]
            app.manage(
                WindowsSingleInstanceGuard::acquire(&app_data_dir)
                    .map_err(std::io::Error::other)?,
            );
            let event_store = EventStore::open(app_data_dir.join("kernel-events.sqlite3"))?;
            ensure_system_skill_builder(&event_store).map_err(|error| {
                std::io::Error::other(format!("system Skill setup failed: {error}"))
            })?;
            let state = AppState::new(event_store, app_data_dir.join("connector-vault"))
                .map_err(std::io::Error::other)?;
            #[cfg(windows)]
            {
                let _ = reconcile_pending_connector_disconnects(&state);
                let _ = recover_pending_connector_authorizations(&state);
            }
            let reconciliation_store = state.event_store();
            if let Ok(store) = reconciliation_store.lock() {
                let _ = store.reset_abandoned_connector_revocation_claims(chrono::Utc::now());
                let _ = store.reset_abandoned_connector_reconciliation_claims(chrono::Utc::now());
                let _ = store.reset_expired_connector_sync_recovery_claims(chrono::Utc::now());
                let _ = store.reset_connector_read_executions_after_restart(chrono::Utc::now());
            }
            let _ = reconcile_incomplete_connector_attachment_landings(&state);
            spawn_connector_sync_recovery_worker(state.clone());
            spawn_connector_read_worker(state.clone());
            spawn_connector_reconciliation_worker(state.clone());
            spawn_connector_attachment_recovery_worker(state.clone());
            spawn_artifact_recovery_worker(state.clone());
            app.manage(state);
            if let Some(window) = app.get_webview_window("main") {
                apply_main_window_icon(&window)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_foundation_state,
            check_app_update,
            download_app_update,
            install_app_update,
            list_agent_tool_contracts,
            list_agent_tool_invocations,
            execute_agent_tool,
            get_deepseek_credential_status,
            get_network_search_route_status,
            get_computer_use_backend_status,
            get_network_search_route_status_for_model,
            get_computer_use_backend_status_for_model,
            get_model_driven_tool_strategy,
            get_computer_control_unlock_status,
            get_deepseek_chat_cache_state,
            run_agent_chat,
            run_next_queued_agent_chat_worker,
            resume_agent_chat_action,
            list_agent_run_records,
            list_automation_definitions,
            list_automation_runs,
            list_automation_review_items,
            list_artifact_deliveries,
            get_artifact_visual_preview,
            list_connector_account_summaries,
            list_connector_read_activity,
            list_connector_authorization_reviews,
            get_connector_authorization_review,
            resolve_connector_authorization_review,
            list_connector_provider_catalog,
            list_connector_recovery_items,
            disconnect_connector_account,
            retry_connector_attachment_cleanup,
            resume_connector_read_sync,
            start_explicit_connector_mail_search,
            start_explicit_connector_calendar_list,
            inspect_connector_external_result,
            create_once_automation,
            create_recurring_automation,
            set_automation_enabled,
            update_automation_goal,
            delete_automation,
            run_automation_now,
            edit_automation_review_item,
            resolve_automation_review_item,
            enqueue_due_automation,
            reconcile_automation_runs,
            run_due_automation_sweep,
            start_agent_run_record,
            enqueue_agent_run_record,
            enqueue_subagent_run_records,
            queue_expert_team_retries,
            queue_parent_agent_synthesis,
            claim_next_agent_run_record,
            claim_agent_run_record,
            queue_agent_run_guidance_record,
            request_agent_run_cancel_record,
            record_agent_run_step_record,
            record_agent_run_artifact_record,
            finish_agent_run_record,
            get_deepseek_user_balance,
            clear_deepseek_chat_cache,
            list_deepseek_chat_telemetry,
            list_agent_context_receipts,
            stage_agent_attachments,
            get_deepseek_pricing_state,
            save_deepseek_pricing_settings,
            unlock_computer_control,
            get_local_directory_state,
            get_agent_soul_profile,
            save_agent_soul_profile,
            save_local_directory_settings,
            list_task_records,
            list_memory_records,
            list_memory_candidate_records,
            list_selected_memory_feedback,
            list_memory_maintenance_reviews,
            run_memory_background_maintenance,
            search_memory_records,
            propose_memory_candidate,
            propose_memory_update_candidate_from_feedback,
            resolve_memory_candidate,
            preview_memory_candidate_merge,
            preview_memory_candidate_replace,
            merge_memory_candidate_with_conflicts,
            replace_memory_candidate_conflicts,
            update_memory_candidate_conflict,
            archive_memory_candidate_conflicts,
            record_selected_memory_feedback,
            record_memory_maintenance_review_action,
            archive_memory_from_maintenance_review,
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
            start_durable_computer_use_session,
            reobserve_durable_computer_use_session,
            bind_durable_computer_use_action,
            run_durable_computer_use_step,
            take_over_durable_computer_use_step,
            cancel_durable_computer_use_step,
            list_durable_computer_use_sessions,
            list_durable_computer_use_steps,
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
            list_skill_records,
            list_skill_execution_records,
            prepare_skill_execution_record,
            preview_local_skill_package_manifest,
            preview_local_skill_zip_package,
            preview_remote_skill_zip_package,
            verify_skill_source,
            install_local_skill_manifest,
            install_local_skill_zip_package,
            install_remote_skill_zip_package,
            install_skill_from_repository_url,
            run_skill_update_sweep,
            create_generated_skill,
            reset_skill_trust,
            uninstall_skill,
            set_skill_enabled,
            create_task_record,
            export_work_package,
            import_work_package,
            preview_work_package_import
        ])
        .run(context)
        .expect("failed to run DS Agent desktop app");
}

#[cfg(test)]
mod tests {
    use super::{select_ico_resource, APP_ICON_BYTES};

    #[cfg(windows)]
    use super::{ui_smoke_browser_arguments, WindowsSingleInstanceGuard};

    #[cfg(windows)]
    #[test]
    fn runtime_instance_file_lock_is_exclusive_and_recoverable() {
        let temp = tempfile::tempdir().unwrap();
        let first = WindowsSingleInstanceGuard::acquire(temp.path()).unwrap();
        assert!(WindowsSingleInstanceGuard::acquire(temp.path()).is_err());
        drop(first);
        WindowsSingleInstanceGuard::acquire(temp.path())
            .expect("runtime lock is released when the owning process exits or guard drops");
    }

    #[cfg(windows)]
    #[test]
    fn ui_smoke_remote_debugging_accepts_only_a_local_numeric_port() {
        let arguments = ui_smoke_browser_arguments(Some("49351")).expect("valid port");
        assert!(arguments.contains("--remote-debugging-address=127.0.0.1"));
        assert!(arguments.contains("--remote-debugging-port=49351"));
        assert!(ui_smoke_browser_arguments(Some("0")).is_none());
        assert!(ui_smoke_browser_arguments(Some("not-a-port")).is_none());
        assert!(ui_smoke_browser_arguments(None).is_none());
    }

    #[test]
    fn app_icon_embeds_windows_shell_sizes() {
        for size in [16, 32, 48, 256] {
            assert!(
                select_ico_resource(APP_ICON_BYTES, size).is_some(),
                "app icon should include a usable {size}px frame"
            );
        }
    }

    #[test]
    fn malformed_ico_bytes_do_not_select_resource() {
        assert!(select_ico_resource(b"not an icon", 32).is_none());
    }

    #[test]
    fn startup_resets_recovery_claims_before_spawning_workers() {
        let source = include_str!("main.rs");
        let revocation = source
            .find("reset_abandoned_connector_revocation_claims")
            .expect("revocation reset remains wired");
        let reconciliation = source
            .find("reset_abandoned_connector_reconciliation_claims")
            .expect("reconciliation reset remains wired");
        let sync = source
            .find("reset_expired_connector_sync_recovery_claims")
            .expect("sync recovery reset remains wired");
        let read = source
            .find("reset_connector_read_executions_after_restart")
            .expect("read execution reset remains wired");
        let attachment = source
            .find("reconcile_incomplete_connector_attachment_landings(&state)")
            .expect("attachment recovery remains wired");
        let worker = source
            .find("            spawn_connector_reconciliation_worker(state.clone());")
            .expect("reconciliation worker remains wired");
        let sync_worker = source
            .find("            spawn_connector_sync_recovery_worker(state.clone());")
            .expect("sync recovery worker remains wired");
        let read_worker = source
            .find("            spawn_connector_read_worker(state.clone());")
            .expect("read worker remains wired");

        assert!(revocation < reconciliation);
        assert!(reconciliation < sync);
        assert!(sync < read);
        assert!(read < attachment);
        assert!(attachment < sync_worker);
        assert!(sync_worker < read_worker);
        assert!(read_worker < worker);
    }
}
