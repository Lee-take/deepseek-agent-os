use std::path::{Path, PathBuf};

use tauri::Manager;

const ISOLATED_PROFILE_MODE_ENV: &str = "DS_AGENT_UI_SMOKE_PROFILE_MODE";
const ISOLATED_PROFILE_MODE: &str = "isolated-clean";
const ISOLATED_APP_DATA_DIR_ENV: &str = "DS_AGENT_UI_SMOKE_APP_DATA_DIR";
const ISOLATED_PROFILE_PREFIX: &str = "ds-agent-ui-profile-";
const ISOLATED_APP_DATA_DIR_NAME: &str = "appdata";

pub fn resolve_app_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if std::env::var(ISOLATED_PROFILE_MODE_ENV).ok().as_deref() != Some(ISOLATED_PROFILE_MODE) {
        return app.path().app_data_dir().map_err(|error| error.to_string());
    }

    let requested = std::env::var(ISOLATED_APP_DATA_DIR_ENV)
        .map(PathBuf::from)
        .map_err(|_| "isolated app-data override is missing".to_string())?;
    validate_isolated_app_data_dir(&requested, &std::env::temp_dir())
}

pub trait AppDataDirExt {
    fn resolved_app_data_dir(&self) -> Result<PathBuf, String>;
}

impl AppDataDirExt for tauri::AppHandle {
    fn resolved_app_data_dir(&self) -> Result<PathBuf, String> {
        resolve_app_data_dir(self)
    }
}

fn validate_isolated_app_data_dir(requested: &Path, temp_root: &Path) -> Result<PathBuf, String> {
    if !requested.is_absolute() {
        return Err("isolated app-data override must be absolute".to_string());
    }

    let requested_metadata = std::fs::symlink_metadata(requested)
        .map_err(|_| "isolated app-data override is unavailable".to_string())?;
    if !requested_metadata.is_dir() || requested_metadata.file_type().is_symlink() {
        return Err("isolated app-data override must be a real directory".to_string());
    }

    let profile_root = requested
        .parent()
        .ok_or_else(|| "isolated app-data override has no profile root".to_string())?;
    let profile_metadata = std::fs::symlink_metadata(profile_root)
        .map_err(|_| "isolated profile root is unavailable".to_string())?;
    if !profile_metadata.is_dir() || profile_metadata.file_type().is_symlink() {
        return Err("isolated profile root must be a real directory".to_string());
    }

    let canonical_app_data = std::fs::canonicalize(requested)
        .map_err(|_| "isolated app-data override could not be verified".to_string())?;
    let canonical_profile_root = std::fs::canonicalize(profile_root)
        .map_err(|_| "isolated profile root could not be verified".to_string())?;
    let canonical_temp_root = std::fs::canonicalize(temp_root)
        .map_err(|_| "system temp root could not be verified".to_string())?;

    if canonical_app_data.parent() != Some(canonical_profile_root.as_path())
        || canonical_profile_root.parent() != Some(canonical_temp_root.as_path())
        || canonical_app_data
            .file_name()
            .and_then(|name| name.to_str())
            != Some(ISOLATED_APP_DATA_DIR_NAME)
        || !canonical_profile_root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(ISOLATED_PROFILE_PREFIX))
    {
        return Err("isolated app-data override escaped the verified temp profile".to_string());
    }

    Ok(canonical_app_data)
}

#[cfg(test)]
mod tests {
    use super::validate_isolated_app_data_dir;

    #[test]
    fn isolated_app_data_accepts_only_the_verified_temp_profile_shape() {
        let temp = tempfile::tempdir().expect("temp root");
        let profile = temp.path().join("ds-agent-ui-profile-test");
        let app_data = profile.join("appdata");
        std::fs::create_dir_all(&app_data).expect("create isolated app data");

        assert_eq!(
            validate_isolated_app_data_dir(&app_data, temp.path()).expect("valid override"),
            std::fs::canonicalize(app_data).expect("canonical app data")
        );
    }

    #[test]
    fn isolated_app_data_rejects_a_non_profile_directory() {
        let temp = tempfile::tempdir().expect("temp root");
        let app_data = temp.path().join("not-a-profile").join("appdata");
        std::fs::create_dir_all(&app_data).expect("create app data");

        assert!(validate_isolated_app_data_dir(&app_data, temp.path()).is_err());
    }
}
