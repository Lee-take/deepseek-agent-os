#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const app = await readFile("apps/desktop/src/App.tsx", "utf8");

const chatThreadStart = app.indexOf(
  '<div className="chat-thread" ref={chatThreadRef} aria-live="polite">',
);
const chatThreadEnd = app.indexOf('<div className="chat-input-dock">', chatThreadStart);
const chatThread = app.slice(chatThreadStart, chatThreadEnd);

assert.ok(chatThreadStart >= 0, "chat thread must remain the central scrolling surface");
assert.doesNotMatch(
  chatThread,
  /chat-approval-queue|pendingCapabilityRecords/,
  "historical global approvals must never be rendered as part of the active conversation",
);
assert.doesNotMatch(app, /resolveVisibleToolApproval|pendingCapabilityRecords/);
assert.doesNotMatch(app, /sidebar-approval-actions/);

const inspectorStart = app.indexOf('<details className="inspector-details">');
const inspectorApprovalEnd = app.indexOf('<form className="browser-tool"', inspectorStart);
const inspectorApprovalArea = app.slice(inspectorStart, inspectorApprovalEnd);
assert.ok(inspectorStart >= 0, "permissions inspector must remain available");
assert.doesNotMatch(
  inspectorApprovalArea,
  /agent-action-approval/,
  "the inspector must not duplicate task-bound approval controls",
);
const capabilityGridStart = app.indexOf('<div className="capability-grid">');
const capabilityGridEnd = app.indexOf('{auditError ?', capabilityGridStart);
const capabilityGrid = app.slice(capabilityGridStart, capabilityGridEnd);
assert.ok(capabilityGridStart >= 0, "manual capability cards must remain available");
assert.match(capabilityGrid, /className="capability-card-approval"/);
assert.match(
  capabilityGrid,
  /record\.request\.exact_tool === null[\s\S]*?!agentActionPermissionRequestIds\.has\(record\.request\.id\)/,
);
assert.match(
  capabilityGrid,
  /resolveCapabilityAccess\(latestPendingRecord\.request\.id, true\)/,
);
assert.match(
  capabilityGrid,
  /resolveCapabilityAccess\(latestPendingRecord\.request\.id, false\)/,
);
assert.match(app, /const resolveAndResumeAgentActionGroup = async/);
assert.match(chatThread, /className="agent-action-approval"/);
assert.match(chatThread, /copy\.chatWorkbench\.approvalSummary\(\s*messageApprovalActions\.length/);
assert.match(
  chatThread,
  /resolveAndResumeAgentActionGroup\(\s*message\.id,\s*messageApprovalActions,\s*true/,
);
assert.match(
  chatThread,
  /resolveAndResumeAgentActionGroup\(\s*message\.id,\s*messageApprovalActions,\s*false/,
);

const groupResolverStart = app.indexOf("const resolveAndResumeAgentActionGroup = async");
const groupResolverEnd = app.indexOf("const sendAgentMessage = async", groupResolverStart);
const groupResolver = app.slice(groupResolverStart, groupResolverEnd);
const permissionResolutionIndex = groupResolver.indexOf(
  'invoke("resolve_capability_access_request"',
);
const actionResumeIndex = groupResolver.indexOf(
  'invoke<AgentChatActionProposal>("resume_agent_chat_action"',
);
assert.ok(permissionResolutionIndex >= 0, "task approval must resolve each audit record");
assert.ok(actionResumeIndex > permissionResolutionIndex, "all task permissions resolve before actions resume");

console.log("approval visibility tests passed");
