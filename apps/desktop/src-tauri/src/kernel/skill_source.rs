use reqwest::header::LOCATION;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

use super::network_sandbox::{ensure_public_remote_addr, validate_public_http_url_syntax};
use super::skill::{
    sha256_hex, SkillInstallationRecord, SkillManifest, SkillPackageKind, SkillSourceIdentity,
};

const MAX_REPOSITORY_FILES: usize = 2_000;
const MAX_REPOSITORY_TEXT_BYTES: usize = 10 * 1024 * 1024;
const MAX_REPOSITORY_ARCHIVE_BYTES: usize = 20 * 1024 * 1024;
const MAX_REPOSITORY_METADATA_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillRepositoryProvider {
    Github,
    HuggingFace,
}

impl SkillRepositoryProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::HuggingFace => "huggingface",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillRepositorySource {
    pub provider: SkillRepositoryProvider,
    pub owner: String,
    pub repository: String,
    pub repository_type: String,
    pub canonical_url: String,
    pub requested_revision: Option<String>,
    pub package_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillRepositoryFile {
    pub path: String,
    pub content: String,
}

impl SkillRepositorySource {
    pub fn parse(value: &str) -> Result<Self, String> {
        let url = Url::parse(value.trim())
            .map_err(|error| format!("skill repository URL is invalid: {error}"))?;
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
            return Err(
                "skill repository URL must be public HTTPS without credentials".to_string(),
            );
        }
        let host = url
            .host_str()
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| "skill repository URL host is required".to_string())?;
        let segments = url
            .path_segments()
            .ok_or_else(|| "skill repository URL path is required".to_string())?
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        let provider = match host.as_str() {
            "github.com" => SkillRepositoryProvider::Github,
            "huggingface.co" => SkillRepositoryProvider::HuggingFace,
            _ => return Err(format!("unsupported skill repository host: {host}")),
        };
        let (repository_type, repository_start) = match provider {
            SkillRepositoryProvider::Github => ("repository".to_string(), 0),
            SkillRepositoryProvider::HuggingFace => match segments.first().map(String::as_str) {
                Some("datasets") => ("dataset".to_string(), 1),
                Some("spaces") => ("space".to_string(), 1),
                _ => ("model".to_string(), 0),
            },
        };
        if segments.len() < repository_start + 2 {
            return Err("skill repository URL must include owner and repository".to_string());
        }
        let owner = required_segment(&segments[repository_start], "repository owner")?;
        let repository = required_segment(
            segments[repository_start + 1].trim_end_matches(".git"),
            "repository name",
        )?;
        let (requested_revision, package_path) =
            parse_repository_tail(&segments[repository_start + 2..])?;
        let hugging_face_prefix = match repository_type.as_str() {
            "dataset" => "datasets/",
            "space" => "spaces/",
            _ => "",
        };
        Ok(Self {
            provider,
            canonical_url: format!(
                "https://{}/{hugging_face_prefix}{owner}/{}",
                match provider {
                    SkillRepositoryProvider::Github => "github.com",
                    SkillRepositoryProvider::HuggingFace => "huggingface.co",
                },
                repository
            ),
            owner,
            repository,
            repository_type,
            requested_revision,
            package_path,
        })
    }

    pub fn repository_id(&self) -> String {
        format!("{}/{}", self.owner, self.repository)
    }

    pub fn source_identity(
        &self,
        resolved_revision: impl Into<String>,
        source_format: impl Into<String>,
    ) -> SkillSourceIdentity {
        SkillSourceIdentity {
            provider: self.provider.as_str().to_string(),
            repository_url: self.canonical_url.clone(),
            requested_revision: self.requested_revision.clone(),
            resolved_revision: resolved_revision.into(),
            package_path: self.package_path.clone(),
            source_format: source_format.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SkillRepositorySnapshot {
    pub resolved_revision: String,
    pub files: Vec<SkillRepositoryFile>,
}

pub fn fetch_repository_skill_installation(
    source_url: &str,
) -> Result<SkillInstallationRecord, String> {
    let source = SkillRepositorySource::parse(source_url)?;
    let snapshot = fetch_repository_snapshot(&source)?;
    build_repository_skill_installation(&source, &snapshot.resolved_revision, &snapshot.files)
}

pub fn fetch_repository_snapshot(
    source: &SkillRepositorySource,
) -> Result<SkillRepositorySnapshot, String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("DS-Agent-Skill-Repository/1.0")
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| format!("skill repository HTTP client could not start: {error}"))?;
    match source.provider {
        SkillRepositoryProvider::Github => fetch_github_snapshot(&client, source),
        SkillRepositoryProvider::HuggingFace => fetch_hugging_face_snapshot(&client, source),
    }
}

fn fetch_github_snapshot(
    client: &reqwest::blocking::Client,
    source: &SkillRepositorySource,
) -> Result<SkillRepositorySnapshot, String> {
    let requested_revision = if let Some(revision) = source.requested_revision.as_ref() {
        revision.clone()
    } else {
        let repository_url = github_api_url(source, &["repos"])?;
        let metadata = fetch_json(client, repository_url.as_str())?;
        json_string(&metadata, "default_branch")
            .ok_or_else(|| "GitHub repository did not report a default branch".to_string())?
    };
    let commit_url = github_api_url(source, &["repos", "commits", &requested_revision])?;
    let commit = fetch_json(client, commit_url.as_str())?;
    let resolved_revision = json_string(&commit, "sha")
        .ok_or_else(|| "GitHub repository did not report a commit revision".to_string())?;
    let archive_url = github_codeload_url(source, &resolved_revision)?;
    let archive_bytes =
        fetch_bounded_bytes(client, archive_url.as_str(), MAX_REPOSITORY_ARCHIVE_BYTES)?;
    let files = repository_files_from_zip(&archive_bytes)?;
    Ok(SkillRepositorySnapshot {
        resolved_revision,
        files,
    })
}

fn fetch_hugging_face_snapshot(
    client: &reqwest::blocking::Client,
    source: &SkillRepositorySource,
) -> Result<SkillRepositorySnapshot, String> {
    let requested_revision = source.requested_revision.as_deref().unwrap_or("main");
    let metadata_url = hugging_face_api_url(source, requested_revision)?;
    let metadata = fetch_json(client, metadata_url.as_str())?;
    let resolved_revision = json_string(&metadata, "sha")
        .ok_or_else(|| "Hugging Face repository did not report a revision".to_string())?;
    let siblings = metadata
        .get("siblings")
        .and_then(Value::as_array)
        .ok_or_else(|| "Hugging Face repository did not report files".to_string())?;
    let mut files = Vec::new();
    for sibling in siblings {
        let Some(path) = sibling.get("rfilename").and_then(Value::as_str) else {
            continue;
        };
        if !repository_file_is_relevant(path)
            || !repository_path_is_in_scope(source.package_path.as_deref(), path)
        {
            continue;
        }
        if files.len() >= MAX_REPOSITORY_FILES {
            return Err(format!(
                "repository Skill file count exceeds {MAX_REPOSITORY_FILES}"
            ));
        }
        let raw_url = hugging_face_raw_url(source, &resolved_revision, path)?;
        let content = fetch_bounded_text(client, raw_url.as_str(), MAX_REPOSITORY_TEXT_BYTES)?;
        files.push(SkillRepositoryFile {
            path: path.to_string(),
            content,
        });
    }
    Ok(SkillRepositorySnapshot {
        resolved_revision,
        files,
    })
}

fn github_api_url(source: &SkillRepositorySource, suffix: &[&str]) -> Result<Url, String> {
    let mut url = Url::parse("https://api.github.com/").map_err(|error| error.to_string())?;
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| "GitHub API URL cannot contain path segments".to_string())?;
        segments.pop_if_empty();
        for segment in suffix {
            segments.push(segment);
            if *segment == "repos" {
                segments.push(&source.owner);
                segments.push(&source.repository);
            }
        }
    }
    Ok(url)
}

fn github_codeload_url(
    source: &SkillRepositorySource,
    resolved_revision: &str,
) -> Result<Url, String> {
    let mut url = Url::parse("https://codeload.github.com/").map_err(|error| error.to_string())?;
    url.path_segments_mut()
        .map_err(|_| "GitHub archive URL cannot contain path segments".to_string())?
        .pop_if_empty()
        .push(&source.owner)
        .push(&source.repository)
        .push("zip")
        .push(resolved_revision);
    Ok(url)
}

fn hugging_face_api_url(
    source: &SkillRepositorySource,
    requested_revision: &str,
) -> Result<Url, String> {
    let collection = match source.repository_type.as_str() {
        "dataset" => "datasets",
        "space" => "spaces",
        _ => "models",
    };
    let mut url = Url::parse("https://huggingface.co/api/").map_err(|error| error.to_string())?;
    url.path_segments_mut()
        .map_err(|_| "Hugging Face API URL cannot contain path segments".to_string())?
        .pop_if_empty()
        .push(collection)
        .push(&source.owner)
        .push(&source.repository)
        .push("revision")
        .push(requested_revision);
    Ok(url)
}

fn hugging_face_raw_url(
    source: &SkillRepositorySource,
    resolved_revision: &str,
    path: &str,
) -> Result<Url, String> {
    let mut url = Url::parse("https://huggingface.co/").map_err(|error| error.to_string())?;
    let mut segments = url
        .path_segments_mut()
        .map_err(|_| "Hugging Face file URL cannot contain path segments".to_string())?;
    segments.pop_if_empty();
    match source.repository_type.as_str() {
        "dataset" => {
            segments.push("datasets");
        }
        "space" => {
            segments.push("spaces");
        }
        _ => {}
    }
    segments
        .push(&source.owner)
        .push(&source.repository)
        .push("resolve")
        .push(resolved_revision);
    for segment in path.split('/') {
        segments.push(segment);
    }
    drop(segments);
    Ok(url)
}

fn fetch_json(client: &reqwest::blocking::Client, url: &str) -> Result<Value, String> {
    let content = fetch_bounded_text(client, url, MAX_REPOSITORY_METADATA_BYTES)?;
    serde_json::from_str(&content)
        .map_err(|error| format!("skill repository metadata is invalid: {error}"))
}

fn fetch_bounded_text(
    client: &reqwest::blocking::Client,
    url: &str,
    limit: usize,
) -> Result<String, String> {
    let bytes = fetch_bounded_bytes(client, url, limit)?;
    String::from_utf8(bytes)
        .map_err(|_| "skill repository file is not valid UTF-8 text".to_string())
}

fn fetch_bounded_bytes(
    client: &reqwest::blocking::Client,
    url: &str,
    limit: usize,
) -> Result<Vec<u8>, String> {
    let response = send_trusted_repository_get(client, url, 5)?
        .error_for_status()
        .map_err(|error| format!("skill repository source returned an error: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(format!("skill repository response exceeds {limit} bytes"));
    }
    let mut bytes = Vec::new();
    response
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("skill repository response could not be read: {error}"))?;
    if bytes.len() > limit {
        return Err(format!("skill repository response exceeds {limit} bytes"));
    }
    Ok(bytes)
}

fn send_trusted_repository_get(
    client: &reqwest::blocking::Client,
    initial_url: &str,
    redirect_limit: usize,
) -> Result<reqwest::blocking::Response, String> {
    let mut url = validate_trusted_repository_url(initial_url)?;
    for redirect_count in 0..=redirect_limit {
        let response = client
            .get(url.clone())
            .send()
            .map_err(|error| format!("trusted repository request failed: {error}"))?;
        if let Err(error) = ensure_public_remote_addr(response.remote_addr()) {
            let local_proxy = response
                .remote_addr()
                .is_some_and(|address| address.ip().is_loopback());
            if !local_proxy {
                return Err(error);
            }
        }
        if !response.status().is_redirection() {
            return Ok(response);
        }
        if redirect_count >= redirect_limit {
            return Err(format!(
                "trusted repository redirect limit of {redirect_limit} was exceeded"
            ));
        }
        let location = response
            .headers()
            .get(LOCATION)
            .ok_or_else(|| "trusted repository redirect omitted Location".to_string())?
            .to_str()
            .map_err(|_| "trusted repository redirect Location is invalid".to_string())?;
        let next_url = url
            .join(location)
            .map_err(|error| format!("trusted repository redirect is invalid: {error}"))?;
        url = validate_trusted_repository_url(next_url.as_str())?;
    }
    Err("trusted repository redirect processing ended unexpectedly".to_string())
}

fn validate_trusted_repository_url(value: &str) -> Result<Url, String> {
    let url = validate_public_http_url_syntax(value)?;
    if url.scheme() != "https" {
        return Err("trusted repository downloads require HTTPS".to_string());
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if matches!(
        host.as_str(),
        "api.github.com"
            | "codeload.github.com"
            | "github.com"
            | "huggingface.co"
            | "cdn-lfs.hf.co"
            | "cas-bridge.xethub.hf.co"
    ) {
        Ok(url)
    } else {
        Err(format!("untrusted repository download host: {host}"))
    }
}

fn repository_files_from_zip(bytes: &[u8]) -> Result<Vec<SkillRepositoryFile>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("GitHub repository archive is invalid: {error}"))?;
    if archive.len() > MAX_REPOSITORY_FILES {
        return Err(format!(
            "repository file count exceeds {MAX_REPOSITORY_FILES}"
        ));
    }
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| format!("GitHub repository archive is invalid: {error}"))?;
        if file.is_dir() {
            continue;
        }
        let enclosed = file
            .enclosed_name()
            .ok_or_else(|| "GitHub repository archive contains an unsafe path".to_string())?;
        let path = enclosed
            .components()
            .skip(1)
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if path.is_empty() || !repository_file_is_relevant(&path) {
            continue;
        }
        if file.size() > MAX_REPOSITORY_TEXT_BYTES as u64 {
            return Err(format!("repository Skill file is too large: {path}"));
        }
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(|_| format!("repository Skill file is not UTF-8 text: {path}"))?;
        files.push(SkillRepositoryFile { path, content });
    }
    Ok(files)
}

fn repository_file_is_relevant(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let file_name = path.rsplit('/').next().unwrap_or(&path);
    file_name.eq_ignore_ascii_case("SKILL.md")
        || file_name.eq_ignore_ascii_case("CLAUDE.md")
        || file_name.eq_ignore_ascii_case("skill.json")
        || file_name.eq_ignore_ascii_case("manifest.json")
        || file_name.eq_ignore_ascii_case("plugin.json")
        || path.eq_ignore_ascii_case(".claude-plugin/plugin.json")
}

fn repository_path_is_in_scope(package_path: Option<&str>, path: &str) -> bool {
    let Some(package_path) = package_path.and_then(normalize_repository_path) else {
        return true;
    };
    let Some(path) = normalize_repository_path(path) else {
        return false;
    };
    path.eq_ignore_ascii_case(&package_path) || path.starts_with(&format!("{package_path}/"))
}

pub fn build_repository_skill_installation(
    source: &SkillRepositorySource,
    resolved_revision: &str,
    files: &[SkillRepositoryFile],
) -> Result<SkillInstallationRecord, String> {
    validate_repository_files(files)?;
    let scoped_files = files
        .iter()
        .filter_map(|file| scoped_repository_file(source.package_path.as_deref(), file))
        .collect::<Vec<_>>();
    if scoped_files.is_empty() {
        return Err("repository path contains no readable Skill content".to_string());
    }

    if let Some(installation) =
        build_native_manifest_installation(source, resolved_revision, &scoped_files)?
    {
        return Ok(installation);
    }

    let plugin_metadata = scoped_files
        .iter()
        .find(|file| file.path.eq_ignore_ascii_case(".claude-plugin/plugin.json"))
        .and_then(|file| serde_json::from_str::<Value>(file.content).ok());
    let mut skill_files = scoped_files
        .iter()
        .filter(|file| {
            file.path.eq_ignore_ascii_case("SKILL.md") || file.path.ends_with("/SKILL.md")
        })
        .collect::<Vec<_>>();
    skill_files.sort_by(|left, right| left.path.cmp(&right.path));
    let source_format = if plugin_metadata.is_some() && !skill_files.is_empty() {
        "claude_plugin"
    } else if !skill_files.is_empty() {
        "skill_md"
    } else {
        let claude = scoped_files
            .iter()
            .find(|file| file.path.eq_ignore_ascii_case("CLAUDE.md"))
            .ok_or_else(|| {
                "repository does not contain a DS Agent manifest, SKILL.md, Claude plugin, or root CLAUDE.md"
                    .to_string()
            })?;
        skill_files.push(claude);
        "claude_md"
    };

    let first_metadata = skill_files
        .first()
        .map(|file| markdown_frontmatter(file.content))
        .unwrap_or_default();
    let name = plugin_metadata
        .as_ref()
        .and_then(|value| json_string(value, "name"))
        .or_else(|| first_metadata.name.clone())
        .unwrap_or_else(|| source.repository.clone());
    let description = plugin_metadata
        .as_ref()
        .and_then(|value| json_string(value, "description"))
        .or_else(|| first_metadata.description.clone())
        .unwrap_or_else(|| format!("Declarative Skill imported from {}.", source.canonical_url));
    let version = plugin_metadata
        .as_ref()
        .and_then(|value| json_string(value, "version"))
        .unwrap_or_else(|| repository_revision_version(resolved_revision));
    let author = plugin_metadata
        .as_ref()
        .and_then(plugin_author)
        .unwrap_or_else(|| source.owner.clone());
    let license = plugin_metadata
        .as_ref()
        .and_then(|value| json_string(value, "license"))
        .or(first_metadata.license)
        .unwrap_or_else(|| "NOASSERTION".to_string());
    let entry_content = if skill_files.len() == 1 {
        skill_files[0].content.to_string()
    } else {
        skill_files
            .iter()
            .map(|file| {
                format!(
                    "## Installed Skill: {}\n\n{}",
                    file.path,
                    file.content.trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    };
    let entry_sha256 = sha256_hex(entry_content.as_bytes());
    let entry_path = if skill_files.len() == 1 {
        skill_files[0].path.clone()
    } else {
        "ds-agent-generated/plugin-bundle.md".to_string()
    };
    let manifest_json = serde_json::json!({
        "schema_version": "ds-agent.skill.v1",
        "name": name,
        "version": version,
        "description": description,
        "author": author,
        "license": license,
        "source": {
            "kind": source.provider.as_str(),
            "url": source.canonical_url,
            "integrity": {
                "algorithm": "sha256",
                "hash": entry_sha256
            }
        },
        "capabilities": ["prompt_guidance"],
        "permissions": [],
        "entry": {
            "kind": "prompt_pack",
            "path": entry_path
        }
    })
    .to_string();
    let manifest = SkillManifest::from_json(&manifest_json).map_err(|error| error.to_string())?;
    let mut installation = SkillInstallationRecord::new(
        manifest,
        format!("repository: {}@{}", source.canonical_url, resolved_revision),
    )?;
    installation.package_kind = if plugin_metadata.is_some() || skill_files.len() > 1 {
        SkillPackageKind::Plugin
    } else {
        SkillPackageKind::Skill
    };
    installation.source_identity = Some(source.source_identity(resolved_revision, source_format));
    installation.entry_content = Some(entry_content);
    installation.entry_sha256 = Some(entry_sha256);
    Ok(installation)
}

fn build_native_manifest_installation(
    source: &SkillRepositorySource,
    resolved_revision: &str,
    files: &[ScopedRepositoryFile<'_>],
) -> Result<Option<SkillInstallationRecord>, String> {
    for file in files {
        if !matches!(
            file.path.as_str(),
            "skill.json" | "manifest.json" | "plugin.json"
        ) {
            continue;
        }
        let value = match serde_json::from_str::<Value>(file.content) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("schema_version").and_then(Value::as_str) != Some("ds-agent.skill.v1") {
            continue;
        }
        let manifest = SkillManifest::from_json(file.content).map_err(|error| error.to_string())?;
        let manifest_dir = file.path.rsplit_once('/').map(|(directory, _)| directory);
        let entry_path = manifest_dir
            .map(|directory| format!("{directory}/{}", manifest.entry.path))
            .unwrap_or_else(|| manifest.entry.path.clone());
        let entry = files
            .iter()
            .find(|candidate| candidate.path.eq_ignore_ascii_case(&entry_path))
            .ok_or_else(|| format!("repository manifest entry is missing: {entry_path}"))?;
        let actual_hash = sha256_hex(entry.content.as_bytes());
        if manifest
            .source
            .integrity
            .as_ref()
            .map(|integrity| integrity.hash.as_str())
            != Some(actual_hash.as_str())
        {
            return Err("repository manifest entry integrity mismatch".to_string());
        }
        let mut installation = SkillInstallationRecord::new(
            manifest,
            format!("repository: {}@{}", source.canonical_url, resolved_revision),
        )?;
        installation.source_identity =
            Some(source.source_identity(resolved_revision, "ds_agent_manifest"));
        installation.entry_content = Some(entry.content.to_string());
        installation.entry_sha256 = Some(actual_hash);
        return Ok(Some(installation));
    }
    Ok(None)
}

#[derive(Clone)]
struct ScopedRepositoryFile<'a> {
    path: String,
    content: &'a str,
}

fn scoped_repository_file<'a>(
    package_path: Option<&str>,
    file: &'a SkillRepositoryFile,
) -> Option<ScopedRepositoryFile<'a>> {
    let path = normalize_repository_path(&file.path)?;
    let Some(package_path) = package_path.and_then(normalize_repository_path) else {
        return Some(ScopedRepositoryFile {
            path,
            content: &file.content,
        });
    };
    if path.eq_ignore_ascii_case(&package_path) {
        let scoped = path.rsplit('/').next().unwrap_or(&path).to_string();
        return Some(ScopedRepositoryFile {
            path: scoped,
            content: &file.content,
        });
    }
    let prefix = format!("{package_path}/");
    path.strip_prefix(&prefix)
        .map(|scoped| ScopedRepositoryFile {
            path: scoped.to_string(),
            content: &file.content,
        })
}

fn validate_repository_files(files: &[SkillRepositoryFile]) -> Result<(), String> {
    if files.is_empty() {
        return Err("repository contains no readable files".to_string());
    }
    if files.len() > MAX_REPOSITORY_FILES {
        return Err(format!(
            "repository file count exceeds {MAX_REPOSITORY_FILES}"
        ));
    }
    let total_bytes = files
        .iter()
        .try_fold(0_usize, |total, file| total.checked_add(file.content.len()))
        .ok_or_else(|| "repository content size overflow".to_string())?;
    if total_bytes > MAX_REPOSITORY_TEXT_BYTES {
        return Err(format!(
            "repository text exceeds {MAX_REPOSITORY_TEXT_BYTES} bytes"
        ));
    }
    for file in files {
        normalize_repository_path(&file.path)
            .ok_or_else(|| format!("repository contains an unsafe path: {}", file.path))?;
    }
    Ok(())
}

fn normalize_repository_path(value: &str) -> Option<String> {
    let path = value.trim().replace('\\', "/");
    if path.is_empty()
        || path.starts_with('/')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return None;
    }
    Some(path)
}

fn parse_repository_tail(tail: &[String]) -> Result<(Option<String>, Option<String>), String> {
    if tail.is_empty() {
        return Ok((None, None));
    }
    if !matches!(tail[0].as_str(), "tree" | "blob" | "resolve") || tail.len() < 2 {
        return Err("unsupported repository URL path".to_string());
    }
    let revision = required_segment(&tail[1], "repository revision")?;
    let package_path = if tail.len() > 2 {
        Some(tail[2..].join("/"))
    } else {
        None
    };
    Ok((Some(revision), package_path))
}

fn required_segment(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || matches!(value, "." | "..") {
        Err(format!("{label} is required"))
    } else {
        Ok(value.to_string())
    }
}

#[derive(Default)]
struct MarkdownMetadata {
    name: Option<String>,
    description: Option<String>,
    license: Option<String>,
}

fn markdown_frontmatter(content: &str) -> MarkdownMetadata {
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return MarkdownMetadata::default();
    }
    let mut metadata = MarkdownMetadata::default();
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches(['\'', '"']).to_string();
        match key.trim() {
            "name" => metadata.name = Some(value),
            "description" => metadata.description = Some(value),
            "license" => metadata.license = Some(value),
            _ => {}
        }
    }
    metadata
}

fn json_string(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn plugin_author(value: &Value) -> Option<String> {
    value
        .get("author")
        .and_then(|author| {
            author.as_str().map(str::to_string).or_else(|| {
                author
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        })
        .map(|author| author.trim().to_string())
        .filter(|author| !author.is_empty())
}

fn repository_revision_version(revision: &str) -> String {
    let revision = revision.trim();
    let short = revision.get(..revision.len().min(12)).unwrap_or(revision);
    format!("0.0.0+{short}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_and_hugging_face_repository_urls() {
        let github =
            SkillRepositorySource::parse("https://github.com/multica-ai/andrej-karpathy-skills")
                .expect("GitHub repository parses");
        assert_eq!(github.provider, SkillRepositoryProvider::Github);
        assert_eq!(github.repository_id(), "multica-ai/andrej-karpathy-skills");
        assert_eq!(github.requested_revision, None);

        let hugging_face = SkillRepositorySource::parse(
            "https://huggingface.co/example/agent-skills/tree/main/skills/reporting",
        )
        .expect("Hugging Face repository path parses");
        assert_eq!(hugging_face.provider, SkillRepositoryProvider::HuggingFace);
        assert_eq!(hugging_face.requested_revision.as_deref(), Some("main"));
        assert_eq!(
            hugging_face.package_path.as_deref(),
            Some("skills/reporting")
        );
    }

    #[test]
    fn adapts_claude_plugin_and_skill_md_without_user_format_decision() {
        let source =
            SkillRepositorySource::parse("https://github.com/multica-ai/andrej-karpathy-skills")
                .expect("source parses");
        let files = vec![
            SkillRepositoryFile {
                path: ".claude-plugin/plugin.json".to_string(),
                content: serde_json::json!({
                    "name": "andrej-karpathy-skills",
                    "description": "Behavioral guidelines for careful coding.",
                    "version": "1.0.0",
                    "author": { "name": "forrestchang" },
                    "license": "MIT",
                    "skills": ["./skills/karpathy-guidelines"]
                })
                .to_string(),
            },
            SkillRepositoryFile {
                path: "skills/karpathy-guidelines/SKILL.md".to_string(),
                content: "---\nname: karpathy-guidelines\ndescription: Keep code changes simple.\nlicense: MIT\n---\n# Karpathy Guidelines\n\nMake surgical changes."
                    .to_string(),
            },
        ];

        let installation = build_repository_skill_installation(&source, "1234567890abcdef", &files)
            .expect("Claude plugin adapts");

        assert_eq!(installation.package_kind, SkillPackageKind::Plugin);
        assert_eq!(installation.manifest.name, "andrej-karpathy-skills");
        assert_eq!(installation.manifest.version, "1.0.0");
        assert_eq!(installation.manifest.entry.kind, "prompt_pack");
        assert!(installation
            .entry_content
            .as_deref()
            .is_some_and(|content| content.contains("Make surgical changes.")));
        assert_eq!(
            installation
                .source_identity
                .as_ref()
                .map(|identity| identity.source_format.as_str()),
            Some("claude_plugin")
        );
    }

    #[test]
    fn rejects_repository_without_declarative_skill_content() {
        let source = SkillRepositorySource::parse("https://github.com/example/script-only")
            .expect("source parses");
        let error = build_repository_skill_installation(
            &source,
            "commit",
            &[SkillRepositoryFile {
                path: "install.ps1".to_string(),
                content: "Write-Host unsafe".to_string(),
            }],
        )
        .expect_err("script-only repository is incompatible");

        assert!(error.contains("does not contain"));
    }

    #[test]
    fn rejects_repository_path_traversal() {
        let source = SkillRepositorySource::parse("https://github.com/example/unsafe")
            .expect("source parses");
        let error = build_repository_skill_installation(
            &source,
            "commit",
            &[SkillRepositoryFile {
                path: "../SKILL.md".to_string(),
                content: "# Unsafe".to_string(),
            }],
        )
        .expect_err("path traversal is blocked");

        assert!(error.contains("unsafe path"));
    }

    #[test]
    fn live_reference_repository_installs_when_explicitly_enabled() {
        if std::env::var("DS_AGENT_LIVE_SKILL_REPO_TEST").as_deref() != Ok("1") {
            return;
        }

        let installation = fetch_repository_skill_installation(
            "https://github.com/multica-ai/andrej-karpathy-skills",
        )
        .expect("live reference repository installs");

        assert_eq!(installation.manifest.name, "andrej-karpathy-skills");
        assert_eq!(installation.package_kind, SkillPackageKind::Plugin);
        assert_eq!(
            installation
                .source_identity
                .as_ref()
                .map(|identity| identity.source_format.as_str()),
            Some("claude_plugin")
        );
        assert!(installation
            .entry_content
            .as_deref()
            .is_some_and(|content| {
                content.contains("Karpathy Guidelines") && content.contains("Surgical Changes")
            }));
    }

    #[test]
    fn live_hugging_face_skill_path_installs_when_explicitly_enabled() {
        if std::env::var("DS_AGENT_LIVE_SKILL_REPO_TEST").as_deref() != Ok("1") {
            return;
        }

        let installation = fetch_repository_skill_installation(
            "https://huggingface.co/spaces/hf-skills/skill-finder",
        )
        .expect("live Hugging Face Skill path installs");

        assert_eq!(installation.package_kind, SkillPackageKind::Skill);
        assert_eq!(
            installation
                .source_identity
                .as_ref()
                .map(|identity| identity.provider.as_str()),
            Some("huggingface")
        );
        assert!(installation
            .entry_content
            .as_deref()
            .is_some_and(|content| content.to_ascii_lowercase().contains("skill")));
    }
}
