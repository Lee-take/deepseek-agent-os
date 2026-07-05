#!/usr/bin/env node

import assert from "node:assert/strict";
import { test } from "node:test";

const attachmentsModuleUrl = new URL("../apps/desktop/src/agentAttachments.ts", import.meta.url);

const {
  AGENT_ATTACHMENT_COUNT_LIMIT,
  AGENT_ATTACHMENT_TOTAL_BYTES_LIMIT,
  buildAttachmentContextPrompt,
  canAddAgentAttachment,
  conversationAttachmentMetadata,
  isAgentAttachmentDropInsideComposer,
  prepareAgentAttachmentPaths,
} = await import(attachmentsModuleUrl);

test("allows attachments until count and total-byte limits are reached", () => {
  const currentAttachments = [
    { id: "a1", name: "source.md", byte_size: 8, status: "ready" },
    { id: "a2", name: "chart.png", byte_size: 12, status: "ready" },
  ];

  assert.deepEqual(
    canAddAgentAttachment(currentAttachments, { byte_size: 5 }),
    { ok: true, reason: "" },
  );

  assert.deepEqual(
    canAddAgentAttachment(
      Array.from({ length: AGENT_ATTACHMENT_COUNT_LIMIT }, (_, index) => ({
        id: `a${index}`,
        name: `${index}.txt`,
        byte_size: 1,
        status: "ready",
      })),
      { byte_size: 1 },
    ),
    { ok: false, reason: `最多添加 ${AGENT_ATTACHMENT_COUNT_LIMIT} 个附件。` },
  );

  assert.deepEqual(
    canAddAgentAttachment([{ id: "huge", name: "huge.txt", byte_size: AGENT_ATTACHMENT_TOTAL_BYTES_LIMIT, status: "ready" }], {
      byte_size: 1,
    }),
    { ok: false, reason: "附件总大小超过当前任务限制。" },
  );
});

test("builds bounded context with text snippets and image model gating", () => {
  const context = buildAttachmentContextPrompt([
    {
      id: "att-text",
      name: "notes.md",
      kind: "text",
      mime_type: "text/markdown",
      byte_size: 24,
      local_path: "C:\\Users\\prosb\\Desktop\\notes.md",
      content_included: true,
      text_snippet: "Revenue changed after the event.",
      status: "ready",
    },
    {
      id: "att-image",
      name: "screen.png",
      kind: "image",
      mime_type: "image/png",
      byte_size: 64,
      local_path: "C:\\Users\\prosb\\Desktop\\screen.png",
      content_included: false,
      text_snippet: null,
      blocked_reason: "DeepSeek V4 is text-only; image pixels were not sent.",
      status: "ready",
    },
  ]);

  assert.match(context, /Task-scoped local attachments/);
  assert.match(context, /notes\.md/);
  assert.match(context, /Revenue changed after the event/);
  assert.match(context, /screen\.png/);
  assert.match(context, /image pixels were not sent/);
  assert.doesNotMatch(context, /C:\\Users\\prosb/);
});

test("drops text snippets from persisted conversation attachment metadata", () => {
  const metadata = conversationAttachmentMetadata([
    {
      id: "att-text",
      name: "notes.md",
      kind: "text",
      mime_type: "text/markdown",
      byte_size: 24,
      local_path: "C:\\Users\\prosb\\Desktop\\notes.md",
      content_included: true,
      text_snippet: "Private snippet already sent to model context.",
      status: "ready",
    },
  ]);

  assert.equal(metadata[0].content_included, true);
  assert.equal(metadata[0].text_snippet, null);
});

test("prepares dragged attachment paths in order without duplicates", () => {
  const preparedPaths = prepareAgentAttachmentPaths(
    [
      "C:\\Users\\prosb\\Desktop\\brief.docx",
      "",
      "C:\\Users\\prosb\\Desktop\\screen.png",
      "c:\\users\\prosb\\desktop\\BRIEF.docx",
      "C:\\Users\\prosb\\Desktop\\notes.md",
    ],
    [
      {
        local_path: "C:\\Users\\prosb\\Desktop\\screen.png",
      },
    ],
  );

  assert.deepEqual(preparedPaths, [
    "C:\\Users\\prosb\\Desktop\\brief.docx",
    "C:\\Users\\prosb\\Desktop\\notes.md",
  ]);
});

test("accepts file drops only inside the composer bounds", () => {
  const composerBounds = {
    left: 100,
    top: 200,
    right: 500,
    bottom: 340,
  };

  assert.equal(
    isAgentAttachmentDropInsideComposer(
      { x: 300, y: 260 },
      composerBounds,
      1,
    ),
    true,
  );
  assert.equal(
    isAgentAttachmentDropInsideComposer(
      { x: 610, y: 260 },
      composerBounds,
      1,
    ),
    false,
  );
  assert.equal(
    isAgentAttachmentDropInsideComposer(
      { x: 600, y: 520 },
      composerBounds,
      2,
    ),
    true,
  );
});
