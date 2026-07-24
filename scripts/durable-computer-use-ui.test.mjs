#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [app, types, styles] = await Promise.all([
  readFile("apps/desktop/src/App.tsx", "utf8"),
  readFile("apps/desktop/src/types.ts", "utf8"),
  readFile("apps/desktop/src/styles.css", "utf8"),
]);

assert.match(app, /className="computer-use-step-panel"/);
assert.match(app, /data-status=\{activeComputerUseStep\.status\}/);
assert.match(app, /"start_durable_computer_use_session"/);
assert.match(app, /"bind_durable_computer_use_action"/);
assert.match(app, /"run_durable_computer_use_step"/);
assert.match(app, /"take_over_durable_computer_use_step"/);
assert.match(app, /"reobserve_durable_computer_use_session"/);
assert.match(app, /function durableComputerUsePostcondition/);
assert.match(app, /"set_value:DS Agent verified"/);
assert.match(app, /expectedText: "selection:selected"/);
assert.match(app, /postcondition: durableComputerUsePostcondition\(computerUseActionDraft\)/);
assert.match(app, /isolated foreground Windows target/);
assert.doesNotMatch(
  app,
  /Execute one verifiable action in an isolated Notepad-like app/,
  "the reachable C5B surface must not remain falsely limited to Notepad",
);
assert.match(app, /computerUseStepPending \|\| !computerControlUnlockStatus\.unlocked/);
assert.match(
  app,
  /\["awaiting_approval", "ready", "action_started", "awaiting_verification"\]\.includes/,
  "takeover must remain visible across all nonterminal control states",
);
assert.match(
  app,
  /\["needs_replan", "user_taken_over", "verification_failed", "cancelled", "verified"\]\.includes/,
  "re-observation must be offered only after a stopped or terminal step",
);
assert.match(types, /export type ComputerUseStepStatus =/);
assert.match(types, /\| "effect_unknown"/);
assert.match(types, /action_display: string \| null/);
assert.doesNotMatch(types, /action_text:/, "public DTO must not expose typed text");
assert.match(styles, /\.computer-use-step-status\.effect_unknown/);
assert.match(styles, /\.computer-use-step-timeline/);

console.log("durable Computer Use UI tests passed");
