use std::path::{Path, PathBuf};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::process::Command;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::kernel::app_update_receipt::{
    land_update_reader_at, recover_update_receipts_at, schedule_install_at, APP_UPDATE_MAX_BYTES,
};

pub(crate) const APP_UPDATE_RELEASES_API_URL: &str =
    "https://api.github.com/repos/Lee-take/dsagent/releases";
const APP_UPDATE_RELEASE_DOWNLOAD_PATH_PREFIX: &str = "/Lee-take/dsagent/releases/download/";
const APP_UPDATE_LEGACY_RELEASE_DOWNLOAD_PATH_PREFIX: &str =
    "/Lee-take/deepseek-agent-os/releases/download/";
const APP_UPDATE_USER_AGENT: &str = "DS-Agent-Updater/1.4.0";
const APP_UPDATE_CURRENT_RELEASE_TAG: &str = "v1.4.0";
#[cfg(windows)]
const WINDOWS_CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppUpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub asset_name: Option<String>,
    pub release_url: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppUpdateDownloadResult {
    pub latest_version: String,
    pub asset_name: String,
    pub download_receipt: String,
    pub sha256: String,
    pub byte_size: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AppUpdateInstallResult {
    pub restart_scheduled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SilentUpdateInstallCommand {
    program: PathBuf,
    args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct GithubReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
struct GithubRelease {
    tag_name: String,
    html_url: String,
    assets: Vec<GithubReleaseAsset>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ParsedReleaseVersion {
    core: Vec<u64>,
    prerelease: Option<ParsedPrereleaseVersion>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ParsedPrereleaseVersion {
    rank: u8,
    number: u64,
    label: String,
}

pub(crate) fn check_update() -> Result<AppUpdateStatus, String> {
    fetch_github_releases()
        .map(|releases| update_status_from_releases(releases, app_update_current_version()))
}

pub(crate) fn download_update() -> Result<AppUpdateDownloadResult, String> {
    let releases = fetch_github_releases()?;
    let release = latest_installable_update_release(&releases, app_update_current_version())
        .ok_or_else(|| "DS Agent is already up to date".to_string())?;
    let latest_version = normalize_release_version(&release.tag_name);
    let asset = release_installable_asset(&release)
        .ok_or_else(|| "latest release has no Windows installer asset".to_string())?;
    let response = download_release_asset(asset)?;
    let content_length = response.content_length();
    let landed = land_update_reader_at(
        &app_update_dir(),
        &latest_version,
        &asset.name,
        content_length,
        response,
    )?;

    Ok(AppUpdateDownloadResult {
        latest_version,
        asset_name: asset.name.clone(),
        download_receipt: landed.download_receipt,
        sha256: landed.sha256,
        byte_size: landed.byte_size,
    })
}

pub(crate) fn schedule_install(download_receipt: &str) -> Result<AppUpdateInstallResult, String> {
    let scheduled = schedule_install_at(
        &app_update_dir(),
        download_receipt,
        spawn_silent_update_runner,
    )?;
    Ok(AppUpdateInstallResult {
        restart_scheduled: scheduled.restart_scheduled,
    })
}

pub(crate) fn recover_app_update_state() -> Result<(), String> {
    recover_update_receipts_at(&app_update_dir())
}

fn normalize_release_version(version: &str) -> String {
    version
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

fn app_update_current_version() -> &'static str {
    match option_env!("DS_AGENT_RELEASE_TAG") {
        Some(value) if !value.is_empty() => value,
        _ => APP_UPDATE_CURRENT_RELEASE_TAG,
    }
}

fn parse_release_version(version: &str) -> Option<ParsedReleaseVersion> {
    let normalized = normalize_release_version(version).to_ascii_lowercase();
    let mut version_parts = normalized.splitn(2, '-');
    let core_text = version_parts.next()?.trim();
    if core_text.is_empty() {
        return None;
    }

    let mut core = Vec::new();
    for part in core_text.split('.') {
        if part.trim().is_empty() {
            core.push(0);
            continue;
        }
        core.push(part.parse::<u64>().ok()?);
    }
    while core.len() < 3 {
        core.push(0);
    }

    Some(ParsedReleaseVersion {
        core,
        prerelease: version_parts.next().and_then(parse_prerelease_version),
    })
}

fn parse_prerelease_version(value: &str) -> Option<ParsedPrereleaseVersion> {
    let tokens = value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>();
    let label = tokens
        .iter()
        .copied()
        .find(|token| {
            token
                .chars()
                .any(|character| character.is_ascii_alphabetic())
        })?
        .to_string();
    let number = tokens
        .iter()
        .copied()
        .find_map(|token| token.parse::<u64>().ok())
        .unwrap_or(0);
    let rank = match label.as_str() {
        "alpha" | "a" => 0,
        "beta" | "b" => 1,
        "rc" | "candidate" => 2,
        _ => 1,
    };

    Some(ParsedPrereleaseVersion {
        rank,
        number,
        label,
    })
}

fn compare_release_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let Some(left) = parse_release_version(left) else {
        return std::cmp::Ordering::Equal;
    };
    let Some(right) = parse_release_version(right) else {
        return std::cmp::Ordering::Equal;
    };

    let part_count = left.core.len().max(right.core.len());
    for index in 0..part_count {
        let left_part = *left.core.get(index).unwrap_or(&0);
        let right_part = *right.core.get(index).unwrap_or(&0);
        match left_part.cmp(&right_part) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }

    match (&left.prerelease, &right.prerelease) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (Some(left), Some(right)) => left.cmp(right),
    }
}

fn is_newer_version(latest_version: &str, current_version: &str) -> bool {
    compare_release_versions(latest_version, current_version).is_gt()
}

fn is_windows_installer_asset(asset_name: &str) -> bool {
    let normalized = asset_name.to_ascii_lowercase();
    (normalized.ends_with(".exe") || normalized.ends_with(".msi"))
        && !normalized.contains("debug")
        && !normalized.contains("symbols")
}

fn release_asset_is_trusted(download_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(download_url) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some("github.com")
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && (url
            .path()
            .starts_with(APP_UPDATE_RELEASE_DOWNLOAD_PATH_PREFIX)
            || url
                .path()
                .starts_with(APP_UPDATE_LEGACY_RELEASE_DOWNLOAD_PATH_PREFIX))
}

fn release_installable_asset(release: &GithubRelease) -> Option<&GithubReleaseAsset> {
    release.assets.iter().find(|asset| {
        is_windows_installer_asset(&asset.name)
            && release_asset_is_trusted(&asset.browser_download_url)
    })
}

fn app_update_http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(APP_UPDATE_USER_AGENT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("failed to build update client: {error}"))
}

fn fetch_github_releases() -> Result<Vec<GithubRelease>, String> {
    app_update_http_client()?
        .get(APP_UPDATE_RELEASES_API_URL)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .map_err(|error| format!("failed to check GitHub releases: {error}"))?
        .error_for_status()
        .map_err(|error| format!("GitHub releases check failed: {error}"))?
        .json::<Vec<GithubRelease>>()
        .map_err(|error| format!("failed to parse GitHub releases: {error}"))
}

fn sorted_releases_by_version(mut releases: Vec<GithubRelease>) -> Vec<GithubRelease> {
    releases.sort_by(|left, right| {
        compare_release_versions(&right.tag_name, &left.tag_name)
            .then_with(|| right.tag_name.cmp(&left.tag_name))
    });
    releases
}

fn update_status_from_releases(
    releases: Vec<GithubRelease>,
    current_version: &str,
) -> AppUpdateStatus {
    let releases = sorted_releases_by_version(releases);
    let latest_version = releases
        .first()
        .map(|release| normalize_release_version(&release.tag_name));
    let latest_update_release = releases.iter().find(|release| {
        is_newer_version(&release.tag_name, current_version)
            && release_installable_asset(release).is_some()
    });

    if let Some(release) = latest_update_release {
        let asset_name = release_installable_asset(release).map(|asset| asset.name.clone());
        return AppUpdateStatus {
            current_version: current_version.to_string(),
            latest_version: Some(normalize_release_version(&release.tag_name)),
            update_available: asset_name.is_some(),
            asset_name,
            release_url: Some(release.html_url.clone()),
            message: None,
        };
    }

    let has_newer_release = releases
        .iter()
        .any(|release| is_newer_version(&release.tag_name, current_version));
    AppUpdateStatus {
        current_version: current_version.to_string(),
        latest_version,
        update_available: false,
        asset_name: None,
        release_url: releases.first().map(|release| release.html_url.clone()),
        message: if has_newer_release {
            Some("latest release has no Windows installer asset".to_string())
        } else {
            None
        },
    }
}

#[cfg(test)]
fn update_status_from_release(release: GithubRelease) -> AppUpdateStatus {
    update_status_from_releases(vec![release], app_update_current_version())
}

fn latest_installable_update_release(
    releases: &[GithubRelease],
    current_version: &str,
) -> Option<GithubRelease> {
    let releases = sorted_releases_by_version(releases.to_vec());
    releases.into_iter().find(|release| {
        is_newer_version(&release.tag_name, current_version)
            && release_installable_asset(release).is_some()
    })
}

fn app_update_dir() -> PathBuf {
    std::env::temp_dir().join("ds-agent-updates")
}

fn download_release_asset(
    asset: &GithubReleaseAsset,
) -> Result<reqwest::blocking::Response, String> {
    if !release_asset_is_trusted(&asset.browser_download_url) {
        return Err("update asset URL is not trusted".to_string());
    }

    let client = app_update_http_client()?;
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .map_err(|_| "failed to resolve the update installer download".to_string())?;
    if response.status().is_success() {
        return validate_terminal_download_response(response);
    }
    if response.status() != reqwest::StatusCode::FOUND {
        return Err("update installer download endpoint returned an invalid status".to_string());
    }
    let location = response
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "update installer redirect is missing".to_string())?;
    let resolved = validated_release_asset_location(location)?;
    let response = client
        .get(resolved)
        .send()
        .map_err(|_| "failed to download the update installer".to_string())?;
    validate_terminal_download_response(response)
}

fn validate_terminal_download_response(
    response: reqwest::blocking::Response,
) -> Result<reqwest::blocking::Response, String> {
    if response.status().is_redirection()
        || response.headers().contains_key(reqwest::header::LOCATION)
        || !response.status().is_success()
    {
        return Err("update installer byte request did not terminate safely".to_string());
    }
    if response.content_length().is_none()
        || response
            .content_length()
            .is_some_and(|size| size == 0 || size > APP_UPDATE_MAX_BYTES)
    {
        return Err("update installer response is not bounded".to_string());
    }
    Ok(response)
}

fn validated_release_asset_location(value: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| "update installer redirect is invalid".to_string())?;
    if url.scheme() != "https"
        || url.host_str() != Some("release-assets.githubusercontent.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || url.fragment().is_some()
        || !url.path().starts_with("/github-production-release-asset/")
    {
        return Err("update installer redirect is not trusted".to_string());
    }
    Ok(url)
}

fn silent_update_install_command(installer_path: &Path) -> SilentUpdateInstallCommand {
    let extension = installer_path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if extension == "msi" {
        return SilentUpdateInstallCommand {
            program: PathBuf::from("msiexec.exe"),
            args: vec![
                "/i".to_string(),
                installer_path.display().to_string(),
                "/quiet".to_string(),
                "/norestart".to_string(),
            ],
        };
    }

    SilentUpdateInstallCommand {
        program: installer_path.to_path_buf(),
        args: vec!["/S".to_string()],
    }
}

fn powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn app_update_runner_script(
    installer_path: &Path,
    app_path: &Path,
    current_process_id: u32,
    expected_sha256: &str,
    expected_size: u64,
) -> String {
    let install_command = silent_update_install_command(installer_path);
    let install_args = install_command
        .args
        .iter()
        .map(|argument| powershell_single_quoted(argument))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        concat!(
            "$ErrorActionPreference = 'Stop'\n",
            "$parentPid = {current_process_id}\n",
            "$installerPath = {installer_path}\n",
            "$stream = [System.IO.FileStream]::new($installerPath, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)\n",
            "try {{\n",
            "  if ($stream.Length -ne {expected_size}) {{ exit 41 }}\n",
            "  $stream.Position = 0\n",
            "  $actualHash = (Get-FileHash -InputStream $stream -Algorithm SHA256).Hash\n",
            "  if ($actualHash -ne {expected_sha256}) {{ exit 42 }}\n",
            "  try {{ Wait-Process -Id $parentPid -Timeout 30 -ErrorAction SilentlyContinue }} catch {{ }}\n",
            "  $process = Start-Process -FilePath {installer_program} -ArgumentList @({install_args}) -Wait -PassThru -WindowStyle Hidden\n",
            "  if ($process.ExitCode -eq 0) {{\n",
            "    Start-Process -FilePath {app_path}\n",
            "  }}\n",
            "}} finally {{\n",
            "  $stream.Dispose()\n",
            "}}\n"
        ),
        current_process_id = current_process_id,
        installer_path = powershell_single_quoted(&installer_path.display().to_string()),
        expected_sha256 = powershell_single_quoted(&expected_sha256.to_ascii_uppercase()),
        expected_size = expected_size,
        installer_program = powershell_single_quoted(&install_command.program.display().to_string()),
        install_args = install_args,
        app_path = powershell_single_quoted(&app_path.display().to_string()),
    )
}

#[cfg(windows)]
fn spawn_silent_update_runner(
    installer_path: &Path,
    expected_sha256: &str,
    expected_size: u64,
) -> Result<(), String> {
    let app_path =
        std::env::current_exe().map_err(|error| format!("failed to locate DS Agent: {error}"))?;
    let script = app_update_runner_script(
        installer_path,
        &app_path,
        std::process::id(),
        expected_sha256,
        expected_size,
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(
        script
            .encode_utf16()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<_>>(),
    );

    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-EncodedCommand",
        ])
        .arg(encoded)
        .creation_flags(WINDOWS_CREATE_NO_WINDOW);
    command
        .spawn()
        .map_err(|error| format!("failed to start silent update runner: {error}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn spawn_silent_update_runner(
    _installer_path: &Path,
    _expected_sha256: &str,
    _expected_size: u64,
) -> Result<(), String> {
    Err("silent app updates are only supported on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        app_update_current_version, app_update_runner_script, is_newer_version,
        is_windows_installer_asset, release_asset_is_trusted, release_installable_asset,
        silent_update_install_command, update_status_from_release, update_status_from_releases,
        validated_release_asset_location, GithubRelease, GithubReleaseAsset,
    };

    #[test]
    fn app_update_version_compare_accepts_newer_release_tags() {
        assert!(is_newer_version("v0.1.2", "0.1.1"));
        assert!(is_newer_version("0.2.1", "0.1.9"));
        assert!(is_newer_version("v0.1.0-rc.3", "v0.1.0-rc.1"));
        assert!(is_newer_version("v0.1.0", "v0.1.0-rc.3"));
        assert!(!is_newer_version("v0.1.0-rc.3", "v0.1.0"));
        assert!(!is_newer_version("v0.1.0", "0.1.0"));
        assert!(!is_newer_version("v0.0.9", "0.1.0"));

        assert!(is_newer_version("v0.9.0", "v0.8.0"));
        assert!(!is_newer_version("v0.8.0", "v0.9.0"));
        assert!(!is_newer_version("v0.9.0", "v0.9.0"));
        assert!(!is_newer_version("v0.9.0", "v0.9.1"));
    }

    #[test]
    fn app_update_status_selects_newer_prerelease_installer_from_release_list() {
        let releases = vec![
            GithubRelease {
                tag_name: "v0.1.0-rc.3".to_string(),
                html_url: "https://github.com/Lee-take/dsagent/releases/tag/v0.1.0-rc.3"
                    .to_string(),
                assets: vec![GithubReleaseAsset {
                    name: "DS Agent_0.1.0_x64-setup.exe".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/dsagent/releases/download/v0.1.0-rc.3/DS.Agent_0.1.0_x64-setup.exe"
                            .to_string(),
                }],
            },
            GithubRelease {
                tag_name: "v0.1.0-rc.1".to_string(),
                html_url: "https://github.com/Lee-take/dsagent/releases/tag/v0.1.0-rc.1"
                    .to_string(),
                assets: vec![GithubReleaseAsset {
                    name: "DS Agent_0.1.0_x64-setup.exe".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/dsagent/releases/download/v0.1.0-rc.1/DS.Agent_0.1.0_x64-setup.exe"
                            .to_string(),
                }],
            },
        ];

        let status = update_status_from_releases(releases, "v0.1.0-rc.1");

        assert!(status.update_available);
        assert_eq!(status.current_version, "v0.1.0-rc.1");
        assert_eq!(status.latest_version.as_deref(), Some("0.1.0-rc.3"));
        assert_eq!(
            status.asset_name.as_deref(),
            Some("DS Agent_0.1.0_x64-setup.exe")
        );
    }

    #[test]
    fn app_update_status_keeps_current_prerelease_quiet_from_release_list() {
        let releases = vec![GithubRelease {
            tag_name: "v0.1.0-rc.3".to_string(),
            html_url: "https://github.com/Lee-take/dsagent/releases/tag/v0.1.0-rc.3".to_string(),
            assets: vec![GithubReleaseAsset {
                name: "DS Agent_0.1.0_x64-setup.exe".to_string(),
                browser_download_url:
                    "https://github.com/Lee-take/dsagent/releases/download/v0.1.0-rc.3/DS.Agent_0.1.0_x64-setup.exe"
                        .to_string(),
            }],
        }];

        let status = update_status_from_releases(releases, "v0.1.0-rc.3");

        assert!(!status.update_available);
        assert_eq!(status.current_version, "v0.1.0-rc.3");
        assert_eq!(status.latest_version.as_deref(), Some("0.1.0-rc.3"));
        assert!(status.asset_name.is_none());
    }

    #[test]
    fn app_update_status_keeps_current_formal_release_quiet_from_release_list() {
        let releases = vec![
            GithubRelease {
                tag_name: "v0.3.0".to_string(),
                html_url: "https://github.com/Lee-take/dsagent/releases/tag/v0.3.0".to_string(),
                assets: vec![GithubReleaseAsset {
                    name: "DS Agent_0.3.0_x64-setup.exe".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/dsagent/releases/download/v0.3.0/DS.Agent_0.3.0_x64-setup.exe"
                            .to_string(),
                }],
            },
            GithubRelease {
                tag_name: "v0.1.0".to_string(),
                html_url: "https://github.com/Lee-take/dsagent/releases/tag/v0.1.0".to_string(),
                assets: vec![GithubReleaseAsset {
                    name: "DS Agent_0.1.0_x64-setup.exe".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/dsagent/releases/download/v0.1.0/DS.Agent_0.1.0_x64-setup.exe"
                            .to_string(),
                }],
            },
        ];

        let status = update_status_from_releases(releases, app_update_current_version());

        assert!(!status.update_available);
        assert_eq!(status.current_version, "v1.4.0");
        assert_eq!(status.latest_version.as_deref(), Some("0.3.0"));
        assert!(status.asset_name.is_none());
    }

    #[test]
    fn app_update_status_promotes_v080_to_v090_and_fails_closed_afterward() {
        let stable = GithubRelease {
            tag_name: "v0.9.0".to_string(),
            html_url: "https://github.com/Lee-take/dsagent/releases/tag/v0.9.0".to_string(),
            assets: vec![GithubReleaseAsset {
                name: "DS.Agent_0.9.0_x64-setup.exe".to_string(),
                browser_download_url:
                    "https://github.com/Lee-take/dsagent/releases/download/v0.9.0/DS.Agent_0.9.0_x64-setup.exe"
                        .to_string(),
            }],
        };
        let previous_stable = GithubRelease {
            tag_name: "v0.8.0".to_string(),
            html_url: "https://github.com/Lee-take/dsagent/releases/tag/v0.8.0".to_string(),
            assets: vec![GithubReleaseAsset {
                name: "DS.Agent_0.8.0_x64-setup.exe".to_string(),
                browser_download_url:
                    "https://github.com/Lee-take/dsagent/releases/download/v0.8.0/DS.Agent_0.8.0_x64-setup.exe"
                        .to_string(),
            }],
        };
        let old_stable = GithubRelease {
            tag_name: "v0.5.0".to_string(),
            html_url: "https://github.com/Lee-take/dsagent/releases/tag/v0.5.0".to_string(),
            assets: vec![GithubReleaseAsset {
                name: "DS.Agent_0.5.0_x64-setup.exe".to_string(),
                browser_download_url:
                    "https://github.com/Lee-take/dsagent/releases/download/v0.5.0/DS.Agent_0.5.0_x64-setup.exe"
                        .to_string(),
            }],
        };
        let releases = vec![previous_stable, old_stable, stable];

        let old_client = update_status_from_releases(releases.clone(), "v0.8.0");
        assert!(old_client.update_available);
        assert_eq!(old_client.latest_version.as_deref(), Some("0.9.0"));
        assert_eq!(
            old_client.asset_name.as_deref(),
            Some("DS.Agent_0.9.0_x64-setup.exe")
        );

        let stable_client = update_status_from_releases(releases.clone(), "v0.9.0");
        assert!(!stable_client.update_available);
        assert_eq!(stable_client.latest_version.as_deref(), Some("0.9.0"));
        assert!(stable_client.asset_name.is_none());

        let newer_client = update_status_from_releases(releases, "v0.9.1");
        assert!(!newer_client.update_available);
        assert_eq!(newer_client.latest_version.as_deref(), Some("0.9.0"));
        assert!(newer_client.asset_name.is_none());
    }

    #[test]
    fn app_update_status_selects_v012_installer_for_v011_client() {
        let releases = vec![
            GithubRelease {
                tag_name: "v0.1.2".to_string(),
                html_url: "https://github.com/Lee-take/dsagent/releases/tag/v0.1.2".to_string(),
                assets: vec![GithubReleaseAsset {
                    name: "DS.Agent_0.1.2_x64-setup.exe".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/dsagent/releases/download/v0.1.2/DS.Agent_0.1.2_x64-setup.exe"
                            .to_string(),
                }],
            },
            GithubRelease {
                tag_name: "v0.1.1".to_string(),
                html_url: "https://github.com/Lee-take/dsagent/releases/tag/v0.1.1".to_string(),
                assets: vec![GithubReleaseAsset {
                    name: "DS.Agent_0.1.1_x64-setup.exe".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/dsagent/releases/download/v0.1.1/DS.Agent_0.1.1_x64-setup.exe"
                            .to_string(),
                }],
            },
        ];

        let status = update_status_from_releases(releases, "v0.1.1");

        assert!(status.update_available);
        assert_eq!(status.current_version, "v0.1.1");
        assert_eq!(status.latest_version.as_deref(), Some("0.1.2"));
        assert_eq!(
            status.asset_name.as_deref(),
            Some("DS.Agent_0.1.2_x64-setup.exe")
        );
        assert_eq!(
            status.release_url.as_deref(),
            Some("https://github.com/Lee-take/dsagent/releases/tag/v0.1.2")
        );
    }

    #[test]
    fn app_update_asset_filter_accepts_windows_installers_only() {
        assert!(is_windows_installer_asset("DS Agent_0.1.2_x64-setup.exe"));
        assert!(is_windows_installer_asset("DS-Agent-0.1.2.msi"));
        assert!(!is_windows_installer_asset("Source code.zip"));
        assert!(!is_windows_installer_asset("DS-Agent-0.1.2-debug.exe"));
        assert!(!is_windows_installer_asset("DS-Agent-0.1.2-symbols.exe"));
    }

    #[test]
    fn app_update_asset_trust_accepts_canonical_and_legacy_release_urls() {
        assert!(release_asset_is_trusted(
            "https://github.com/Lee-take/dsagent/releases/download/v0.1.2/DS.Agent_0.1.2_x64-setup.exe"
        ));
        assert!(release_asset_is_trusted(
            "https://github.com/Lee-take/deepseek-agent-os/releases/download/v0.1.2/DS.Agent_0.1.2_x64-setup.exe"
        ));
        assert!(!release_asset_is_trusted(
            "https://github.com/SomeoneElse/dsagent/releases/download/v0.1.2/DS.Agent_0.1.2_x64-setup.exe"
        ));
        for url in [
            "http://github.com/Lee-take/dsagent/releases/download/v0.1.2/update.exe",
            "https://github.com.evil.invalid/Lee-take/dsagent/releases/download/v0.1.2/update.exe",
            "https://user@github.com/Lee-take/dsagent/releases/download/v0.1.2/update.exe",
            "https://github.com/Lee-take/dsagent/releases/download/v0.1.2/update.exe?redirect=evil",
            "https://github.com/Lee-take/dsagent/releases/download/v0.1.2/update.exe#fragment",
        ] {
            assert!(
                !release_asset_is_trusted(url),
                "unexpected trusted URL: {url}"
            );
        }
    }

    #[test]
    fn app_update_status_hides_source_only_newer_release() {
        let release = GithubRelease {
            tag_name: "v9.9.9".to_string(),
            html_url: "https://github.com/Lee-take/dsagent/releases/tag/v9.9.9".to_string(),
            assets: vec![GithubReleaseAsset {
                name: "source.zip".to_string(),
                browser_download_url:
                    "https://github.com/Lee-take/dsagent/releases/download/v9.9.9/source.zip"
                        .to_string(),
            }],
        };

        let status = update_status_from_release(release);
        assert!(!status.update_available);
        assert_eq!(
            status.message.as_deref(),
            Some("latest release has no Windows installer asset")
        );
    }

    #[test]
    fn app_update_selects_trusted_windows_installer_asset() {
        let release = GithubRelease {
            tag_name: "v9.9.9".to_string(),
            html_url: "https://github.com/Lee-take/dsagent/releases/tag/v9.9.9".to_string(),
            assets: vec![
                GithubReleaseAsset {
                    name: "source.zip".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/dsagent/releases/download/v9.9.9/source.zip"
                            .to_string(),
                },
                GithubReleaseAsset {
                    name: "DS Agent_9.9.9_x64-setup.exe".to_string(),
                    browser_download_url:
                        "https://github.com/Lee-take/dsagent/releases/download/v9.9.9/DS.Agent.exe"
                            .to_string(),
                },
            ],
        };

        let asset = release_installable_asset(&release).expect("installer asset");
        assert_eq!(asset.name, "DS Agent_9.9.9_x64-setup.exe");
    }

    #[test]
    fn app_update_silent_installer_command_uses_nsis_s_arg() {
        let command = silent_update_install_command(std::path::Path::new(
            r"C:\Users\tester\AppData\Local\Temp\ds-agent-updates\DS.Agent_0.1.0_rc7_x64-setup.exe",
        ));

        assert_eq!(
            command.program,
            std::path::PathBuf::from(
                r"C:\Users\tester\AppData\Local\Temp\ds-agent-updates\DS.Agent_0.1.0_rc7_x64-setup.exe",
            )
        );
        assert_eq!(command.args, vec!["/S"]);
    }

    #[test]
    fn app_update_redirect_accepts_only_the_exact_github_asset_host() {
        assert!(validated_release_asset_location(
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/file?sig=opaque"
        )
        .is_ok());
        for location in [
            "http://release-assets.githubusercontent.com/github-production-release-asset/1/file",
            "https://release-assets.githubusercontent.com.evil.invalid/github-production-release-asset/1/file",
            "https://user@release-assets.githubusercontent.com/github-production-release-asset/1/file",
            "https://release-assets.githubusercontent.com/another-prefix/1/file",
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/file#fragment",
        ] {
            assert!(validated_release_asset_location(location).is_err());
        }
    }

    #[test]
    fn app_update_runner_holds_and_revalidates_the_exact_installer_before_waiting() {
        let script = app_update_runner_script(
            std::path::Path::new(r"C:\managed\update.exe"),
            std::path::Path::new(r"C:\Program Files\DS Agent\ds-agent.exe"),
            42,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            12_345,
        );
        let open = script.find("[System.IO.FileStream]::new").unwrap();
        let length = script.find("$stream.Length -ne 12345").unwrap();
        let hash = script.find("Get-FileHash -InputStream $stream").unwrap();
        let wait = script.find("Wait-Process -Id $parentPid").unwrap();
        let install = script.find("Start-Process -FilePath").unwrap();
        assert!(open < length && length < hash && hash < wait && wait < install);
        assert!(script.contains("[System.IO.FileShare]::Read"));
        assert!(
            script.contains("'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'")
        );
        assert!(script.contains("$stream.Dispose()"));
    }
}
