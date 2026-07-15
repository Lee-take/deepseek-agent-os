#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const app = await readFile("apps/desktop/src/App.tsx", "utf8");

const chatThreadStart = app.indexOf(
  '<div className="chat-thread" ref={chatThreadRef} aria-live="polite">',
);
const chatThreadEnd = app.indexOf('<div className="chat-input-dock">', chatThreadStart);
const chatThread = app.slice(chatThreadStart, chatThreadEnd);
const approvalQueueIndex = chatThread.indexOf('className="chat-approval-queue"');
const pendingMessageIndex = chatThread.indexOf('className="chat-message assistant pending"');

assert.ok(chatThreadStart >= 0, "chat thread must remain the central scrolling surface");
assert.ok(approvalQueueIndex >= 0, "pending approval queue must be rendered in chat");
assert.ok(
  approvalQueueIndex > pendingMessageIndex,
  "pending approvals must follow the latest DS Agent status at the bottom of chat",
);
assert.match(chatThread, /resolveVisibleToolApproval\(record\.request\.id, true\)/);
assert.match(chatThread, /resolveVisibleToolApproval\(record\.request\.id, false\)/);
assert.match(
  app,
  /if \(pendingCapabilityRecords\.length === 0[\s\S]*?chatThreadRef\.current[\s\S]*?chatThread\?\.scrollTo\([\s\S]*?chatThread\.scrollHeight/,
);
assert.doesNotMatch(app, /sidebar-approval-actions/);

const inspectorStart = app.indexOf('<details className="inspector-details">');
const inspectorApprovalEnd = app.indexOf('<form className="browser-tool"', inspectorStart);
const inspectorApprovalArea = app.slice(inspectorStart, inspectorApprovalEnd);
assert.ok(inspectorStart >= 0, "permissions inspector must remain available");
assert.doesNotMatch(
  inspectorApprovalArea,
  /pendingCapabilityRecords\.map/,
  "right inspector must not duplicate the interactive approval queue",
);
assert.match(app, /approveAndResumeAgentAction\(/);

console.log("approval visibility tests passed");
