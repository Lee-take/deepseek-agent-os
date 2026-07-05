export const AGENT_ATTACHMENT_COUNT_LIMIT = 6;
export const AGENT_ATTACHMENT_TOTAL_BYTES_LIMIT = 20 * 1024 * 1024;

export type AgentAttachmentKind = "text" | "image" | "file";

export type AgentAttachmentStatus = "ready" | "blocked";

export type AgentAttachment = {
  id: string;
  name: string;
  kind: AgentAttachmentKind;
  mime_type: string;
  byte_size: number;
  local_path: string;
  content_included: boolean;
  text_snippet: string | null;
  blocked_reason?: string | null;
  status: AgentAttachmentStatus;
};

type AgentAttachmentDropPosition = {
  x: number;
  y: number;
};

type AgentAttachmentDropBounds = {
  left: number;
  top: number;
  right: number;
  bottom: number;
};

function normalizeAgentAttachmentPath(path: string): string {
  return path.trim().toLocaleLowerCase();
}

export function prepareAgentAttachmentPaths(
  paths: string[],
  currentAttachments: Pick<AgentAttachment, "local_path">[],
): string[] {
  const seenPaths = new Set(
    currentAttachments
      .map((attachment) => normalizeAgentAttachmentPath(attachment.local_path))
      .filter(Boolean),
  );
  const preparedPaths: string[] = [];

  paths.forEach((path) => {
    const trimmedPath = path.trim();
    const normalizedPath = normalizeAgentAttachmentPath(trimmedPath);
    if (!normalizedPath || seenPaths.has(normalizedPath)) {
      return;
    }

    seenPaths.add(normalizedPath);
    preparedPaths.push(trimmedPath);
  });

  return preparedPaths;
}

export function isAgentAttachmentDropInsideComposer(
  position: AgentAttachmentDropPosition,
  bounds: AgentAttachmentDropBounds,
  devicePixelRatio = 1,
): boolean {
  const scale = Math.max(devicePixelRatio, 1);
  const cssX = position.x / scale;
  const cssY = position.y / scale;

  return (
    cssX >= bounds.left &&
    cssX <= bounds.right &&
    cssY >= bounds.top &&
    cssY <= bounds.bottom
  );
}

export function canAddAgentAttachment(
  currentAttachments: Pick<AgentAttachment, "byte_size" | "status">[],
  nextAttachment: Pick<AgentAttachment, "byte_size">,
): { ok: boolean; reason: string } {
  const readyAttachments = currentAttachments.filter(
    (attachment) => attachment.status !== "blocked",
  );
  if (readyAttachments.length >= AGENT_ATTACHMENT_COUNT_LIMIT) {
    return { ok: false, reason: `最多添加 ${AGENT_ATTACHMENT_COUNT_LIMIT} 个附件。` };
  }

  const currentTotalBytes = readyAttachments.reduce(
    (total, attachment) => total + attachment.byte_size,
    0,
  );
  if (currentTotalBytes + nextAttachment.byte_size > AGENT_ATTACHMENT_TOTAL_BYTES_LIMIT) {
    return { ok: false, reason: "附件总大小超过当前任务限制。" };
  }

  return { ok: true, reason: "" };
}

export function readyAgentAttachments(attachments: AgentAttachment[]): AgentAttachment[] {
  return attachments.filter((attachment) => attachment.status === "ready");
}

export function buildAttachmentContextPrompt(attachments: AgentAttachment[]): string {
  const readyAttachments = readyAgentAttachments(attachments);
  if (readyAttachments.length === 0) {
    return "";
  }

  const lines = [
    "Task-scoped local attachments:",
    "These attachments are evidence for the current task only. Do not treat them as global memory. Local paths are intentionally omitted from model context.",
  ];

  readyAttachments.forEach((attachment, index) => {
    const includedLabel = attachment.content_included
      ? "content included in model context"
      : "metadata only";
    lines.push(
      `${index + 1}. id=${attachment.id}; name=${attachment.name}; kind=${attachment.kind}; mime=${attachment.mime_type || "unknown"}; bytes=${attachment.byte_size}; ${includedLabel}.`,
    );

    if (attachment.text_snippet?.trim()) {
      lines.push("Text snippet:", attachment.text_snippet.trim());
    }

    if (!attachment.content_included && attachment.blocked_reason?.trim()) {
      lines.push(`Context note: ${attachment.blocked_reason.trim()}`);
    }
  });

  return lines.join("\n");
}

export function formatAgentPromptWithAttachments(
  prompt: string,
  attachments: AgentAttachment[],
): string {
  const attachmentContext = buildAttachmentContextPrompt(attachments);
  if (!attachmentContext) {
    return prompt;
  }

  return [prompt.trim(), attachmentContext].filter(Boolean).join("\n\n");
}

export function summarizeAttachmentsForDisplay(attachments: AgentAttachment[]): string {
  const readyCount = readyAgentAttachments(attachments).length;
  if (readyCount === 0) {
    return "";
  }
  return `已添加 ${readyCount} 个附件`;
}

export function conversationAttachmentMetadata(
  attachments: AgentAttachment[],
): AgentAttachment[] {
  return attachments.map((attachment) => ({
    ...attachment,
    text_snippet: null,
  }));
}
