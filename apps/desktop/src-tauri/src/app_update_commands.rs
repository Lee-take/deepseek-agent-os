use tauri::AppHandle;

use crate::kernel::app_update::{
    check_update, download_update, schedule_install, AppUpdateDownloadResult,
    AppUpdateInstallResult, AppUpdateStatus,
};

#[tauri::command]
pub fn check_app_update() -> Result<AppUpdateStatus, String> {
    check_update()
}

#[tauri::command]
pub fn download_app_update() -> Result<AppUpdateDownloadResult, String> {
    download_update()
}

#[tauri::command]
pub fn install_app_update(
    app: AppHandle,
    installer_path: String,
) -> Result<AppUpdateInstallResult, String> {
    let result = schedule_install(&installer_path)?;
    app.exit(0);
    Ok(result)
}
