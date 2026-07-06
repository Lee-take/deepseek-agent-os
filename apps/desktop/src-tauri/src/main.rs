#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod kernel;

use commands::{
    browse_url, capture_computer_screenshot, check_app_update, clear_deepseek_chat_cache,
    control_computer_boundary, create_email_draft_boundary, create_task_record,
    delete_memory_record, download_app_update, export_operations_briefing_html_report,
    export_operations_briefing_pdf_report, export_operations_briefing_report, export_work_package,
    get_agent_soul_profile, get_computer_control_unlock_status, get_computer_use_backend_status,
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
    save_agent_soul_profile, save_deepseek_pricing_settings, save_local_directory_settings,
    search_memory_records, search_network_boundary, seed_operations_briefing_evidence_templates,
    send_email_boundary, stage_agent_attachments, submit_browser_boundary, unlock_computer_control,
    update_memory_record, write_drive_boundary, write_file_boundary, AppState,
};
use kernel::event_store::EventStore;
use tauri::{image::Image, Manager};

const APP_ICON_BYTES: &[u8] = include_bytes!("../icons/icon.ico");

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
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let event_store = EventStore::open(app_data_dir.join("kernel-events.sqlite3"))?;
            app.manage(AppState::new(event_store));
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

#[cfg(test)]
mod tests {
    use super::{select_ico_resource, APP_ICON_BYTES};

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
}
