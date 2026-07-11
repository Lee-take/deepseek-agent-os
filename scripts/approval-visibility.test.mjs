#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const app = await readFile("apps/desktop/src/App.tsx", "utf8");

const inspectorStart = app.indexOf(
  '<details className="inspector-details" open={pendingCapabilityRecords.length > 0}>',
);
const inspectorEnd = app.indexOf("</details>", inspectorStart);
const inspector = app.slice(inspectorStart, inspectorEnd);
const approvalQueueIndex = inspector.indexOf(
  '<div className="approval-queue" ref={approvalsSectionRef}>',
);
const capabilityCatalogIndex = inspector.indexOf('<div className="capability-grid">');

assert.ok(inspectorStart >= 0, "permissions inspector must open for pending approvals");
assert.ok(approvalQueueIndex >= 0, "pending approval queue must be rendered");
assert.ok(capabilityCatalogIndex >= 0, "capability catalog must remain available");
assert.ok(
  approvalQueueIndex < capabilityCatalogIndex,
  "pending approvals must appear before the long capability catalog",
);
assert.match(inspector, /resolveVisibleToolApproval\(record\.request\.id, true\)/);
assert.match(inspector, /resolveVisibleToolApproval\(record\.request\.id, false\)/);
assert.match(app, /approvalsSectionRef\.current\?\.scrollIntoView/);
assert.match(app, /invocation\.status === "waiting_for_confirmation"/);
assert.match(app, /resolveVisibleToolApproval\(approvalRequestId, true\)/);
assert.match(app, /resolveVisibleToolApproval\(approvalRequestId, false\)/);
assert.match(app, /approveAndResumeAgentAction\(/);

console.log("approval visibility tests passed");
