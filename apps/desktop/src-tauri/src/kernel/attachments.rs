use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const AGENT_ATTACHMENT_COUNT_LIMIT: usize = 6;
pub const AGENT_ATTACHMENT_TOTAL_BYTES_LIMIT: u64 = 20 * 1024 * 1024;
const AGENT_ATTACHMENT_TEXT_CONTEXT_BYTES_LIMIT: u64 = 256 * 1024;
const AGENT_ATTACHMENT_TEXT_SNIPPET_CHARS: usize = 12_000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAttachmentKind {
    Text,
    Image,
    File,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAttachmentStatus {
    Ready,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentAttachment {
    pub id: String,
    pub name: String,
    pub kind: AgentAttachmentKind,
    pub mime_type: String,
    pub byte_size: u64,
    pub local_path: String,
    pub content_included: bool,
    pub text_snippet: Option<String>,
    pub blocked_reason: Option<String>,
    pub status: AgentAttachmentStatus,
}

pub fn stage_agent_attachment_paths(
    paths: Vec<String>,
    existing_count: usize,
    existing_total_bytes: u64,
) -> Vec<AgentAttachment> {
    let mut staged = Vec::with_capacity(paths.len());
    let mut accepted_count = existing_count;
    let mut accepted_total_bytes = existing_total_bytes;

    for path in paths {
        let mut attachment = stage_agent_attachment_path(&path);

        if attachment.status == AgentAttachmentStatus::Ready {
            if accepted_count >= AGENT_ATTACHMENT_COUNT_LIMIT {
                attachment = blocked_attachment_from_ready(
                    attachment,
                    format!("Too many attachments for one task. Limit is {AGENT_ATTACHMENT_COUNT_LIMIT}."),
                );
            } else if accepted_total_bytes.saturating_add(attachment.byte_size)
                > AGENT_ATTACHMENT_TOTAL_BYTES_LIMIT
            {
                attachment = blocked_attachment_from_ready(
                    attachment,
                    "Attachment total size exceeds the current task limit.".to_string(),
                );
            } else {
                accepted_count += 1;
                accepted_total_bytes = accepted_total_bytes.saturating_add(attachment.byte_size);
            }
        }

        staged.push(attachment);
    }

    staged
}

fn stage_agent_attachment_path(path: &str) -> AgentAttachment {
    let path_buf = PathBuf::from(path);
    let name = attachment_name(&path_buf, path);
    let local_path = path_buf.to_string_lossy().to_string();
    let id = Uuid::new_v4().to_string();

    let metadata = match std::fs::metadata(&path_buf) {
        Ok(metadata) => metadata,
        Err(error) => {
            return AgentAttachment {
                id,
                name,
                kind: AgentAttachmentKind::File,
                mime_type: "application/octet-stream".to_string(),
                byte_size: 0,
                local_path,
                content_included: false,
                text_snippet: None,
                blocked_reason: Some(format!("Attachment metadata could not be read: {error}")),
                status: AgentAttachmentStatus::Blocked,
            };
        }
    };

    if !metadata.is_file() {
        return AgentAttachment {
            id,
            name,
            kind: AgentAttachmentKind::File,
            mime_type: "application/octet-stream".to_string(),
            byte_size: 0,
            local_path,
            content_included: false,
            text_snippet: None,
            blocked_reason: Some("Selected path is not a file.".to_string()),
            status: AgentAttachmentStatus::Blocked,
        };
    }

    let byte_size = metadata.len();
    let (kind, mime_type) = infer_attachment_kind_and_mime(&path_buf);
    let mut attachment = AgentAttachment {
        id,
        name,
        kind,
        mime_type,
        byte_size,
        local_path,
        content_included: false,
        text_snippet: None,
        blocked_reason: None,
        status: AgentAttachmentStatus::Ready,
    };

    match attachment.kind {
        AgentAttachmentKind::Text => {
            if byte_size > AGENT_ATTACHMENT_TEXT_CONTEXT_BYTES_LIMIT {
                attachment.blocked_reason =
                    Some("Text file is too large for model context; metadata only.".to_string());
                return attachment;
            }

            match std::fs::read_to_string(&path_buf) {
                Ok(text) => {
                    attachment.text_snippet = Some(truncate_chars(
                        text.trim(),
                        AGENT_ATTACHMENT_TEXT_SNIPPET_CHARS,
                    ));
                    attachment.content_included = attachment
                        .text_snippet
                        .as_deref()
                        .is_some_and(|snippet| !snippet.is_empty());
                }
                Err(error) => {
                    attachment.blocked_reason = Some(format!(
                        "Text content could not be read as UTF-8; metadata only. {error}"
                    ));
                }
            }
        }
        AgentAttachmentKind::Image => {
            attachment.blocked_reason =
                Some("DeepSeek V4 is text-only; image pixels were not sent.".to_string());
        }
        AgentAttachmentKind::File => {
            attachment.blocked_reason =
                Some("File type is not text or image; metadata only.".to_string());
        }
    }

    attachment
}

fn blocked_attachment_from_ready(
    mut attachment: AgentAttachment,
    blocked_reason: String,
) -> AgentAttachment {
    attachment.content_included = false;
    attachment.text_snippet = None;
    attachment.blocked_reason = Some(blocked_reason);
    attachment.status = AgentAttachmentStatus::Blocked;
    attachment
}

fn attachment_name(path: &Path, fallback: &str) -> String {
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .unwrap_or(fallback)
        .to_string()
}

fn infer_attachment_kind_and_mime(path: &Path) -> (AgentAttachmentKind, String) {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "png" => (AgentAttachmentKind::Image, "image/png".to_string()),
        "jpg" | "jpeg" => (AgentAttachmentKind::Image, "image/jpeg".to_string()),
        "gif" => (AgentAttachmentKind::Image, "image/gif".to_string()),
        "webp" => (AgentAttachmentKind::Image, "image/webp".to_string()),
        "bmp" => (AgentAttachmentKind::Image, "image/bmp".to_string()),
        "svg" => (AgentAttachmentKind::Image, "image/svg+xml".to_string()),
        "txt" => (AgentAttachmentKind::Text, "text/plain".to_string()),
        "md" | "markdown" => (AgentAttachmentKind::Text, "text/markdown".to_string()),
        "csv" => (AgentAttachmentKind::Text, "text/csv".to_string()),
        "json" => (AgentAttachmentKind::Text, "application/json".to_string()),
        "jsonl" => (
            AgentAttachmentKind::Text,
            "application/x-ndjson".to_string(),
        ),
        "xml" => (AgentAttachmentKind::Text, "application/xml".to_string()),
        "yaml" | "yml" => (AgentAttachmentKind::Text, "application/yaml".to_string()),
        "log" => (AgentAttachmentKind::Text, "text/plain".to_string()),
        "html" | "htm" => (AgentAttachmentKind::Text, "text/html".to_string()),
        "css" => (AgentAttachmentKind::Text, "text/css".to_string()),
        "js" | "jsx" | "ts" | "tsx" | "rs" | "py" | "sql" => {
            (AgentAttachmentKind::Text, "text/plain".to_string())
        }
        _ => (
            AgentAttachmentKind::File,
            "application/octet-stream".to_string(),
        ),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        stage_agent_attachment_paths, AgentAttachmentKind, AgentAttachmentStatus,
        AGENT_ATTACHMENT_COUNT_LIMIT, AGENT_ATTACHMENT_TOTAL_BYTES_LIMIT,
    };

    #[test]
    fn stages_text_and_image_attachments_with_context_gating() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let text_path = temp_dir.path().join("notes.md");
        let image_path = temp_dir.path().join("screen.png");
        std::fs::write(&text_path, "Revenue changed after the event.").expect("write text");
        std::fs::write(&image_path, [137, 80, 78, 71, 13, 10, 26, 10]).expect("write image");

        let attachments = stage_agent_attachment_paths(
            vec![
                text_path.to_string_lossy().to_string(),
                image_path.to_string_lossy().to_string(),
            ],
            0,
            0,
        );

        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].name, "notes.md");
        assert_eq!(attachments[0].kind, AgentAttachmentKind::Text);
        assert_eq!(attachments[0].status, AgentAttachmentStatus::Ready);
        assert!(attachments[0].content_included);
        assert!(attachments[0]
            .text_snippet
            .as_deref()
            .unwrap_or_default()
            .contains("Revenue changed"));
        assert_eq!(attachments[1].name, "screen.png");
        assert_eq!(attachments[1].kind, AgentAttachmentKind::Image);
        assert_eq!(attachments[1].status, AgentAttachmentStatus::Ready);
        assert!(!attachments[1].content_included);
        assert!(attachments[1]
            .blocked_reason
            .as_deref()
            .unwrap_or_default()
            .contains("text-only"));
    }

    #[test]
    fn blocks_attachments_that_exceed_count_or_total_size_limits() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let text_path = temp_dir.path().join("extra.txt");
        std::fs::write(&text_path, "extra").expect("write text");

        let count_blocked = stage_agent_attachment_paths(
            vec![text_path.to_string_lossy().to_string()],
            AGENT_ATTACHMENT_COUNT_LIMIT,
            0,
        );
        assert_eq!(count_blocked[0].status, AgentAttachmentStatus::Blocked);
        assert!(count_blocked[0]
            .blocked_reason
            .as_deref()
            .unwrap_or_default()
            .contains("Too many"));

        let size_blocked = stage_agent_attachment_paths(
            vec![text_path.to_string_lossy().to_string()],
            0,
            AGENT_ATTACHMENT_TOTAL_BYTES_LIMIT,
        );
        assert_eq!(size_blocked[0].status, AgentAttachmentStatus::Blocked);
        assert!(size_blocked[0]
            .blocked_reason
            .as_deref()
            .unwrap_or_default()
            .contains("total size"));
    }
}
