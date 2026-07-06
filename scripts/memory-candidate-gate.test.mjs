#!/usr/bin/env node

import { readFileSync } from "node:fs";
import test from "node:test";
import assert from "node:assert/strict";

const appSource = readFileSync(new URL("../apps/desktop/src/App.tsx", import.meta.url), "utf8");
const typesSource = readFileSync(
  new URL("../apps/desktop/src/types.ts", import.meta.url),
  "utf8",
);
const i18nSource = readFileSync(new URL("../apps/desktop/src/i18n.ts", import.meta.url), "utf8");
const desktopTestSource = readFileSync(new URL("./desktop-test.mjs", import.meta.url), "utf8");

test("memory candidate review cards display gate metadata", () => {
  assert.match(typesSource, /export type MemoryCandidateSuggestedAction/);
  assert.match(typesSource, /evidence_excerpt:\s*string;/);
  assert.match(typesSource, /privacy_review:\s*string;/);
  assert.match(typesSource, /suggested_action:\s*MemoryCandidateSuggestedAction;/);
  assert.match(appSource, /record\.candidate\.privacy_review/);
  assert.match(appSource, /record\.candidate\.suggested_action/);
  assert.match(appSource, /record\.candidate\.evidence_excerpt/);
  assert.match(i18nSource, /candidateGate:\s*"Candidate gate"/);
  assert.match(i18nSource, /candidateSuggestedAction:/);
});

test("desktop test suite includes the memory candidate gate UI regression", () => {
  assert.match(desktopTestSource, /scripts\/memory-candidate-gate\.test\.mjs/);
});
