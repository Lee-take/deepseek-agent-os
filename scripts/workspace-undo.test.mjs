#!/usr/bin/env node

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [panel, center, commands, kernel, store, lifecycle, main] = await Promise.all([
  readFile("apps/desktop/src/WorkspaceUndoPanel.tsx", "utf8"),
  readFile("apps/desktop/src/AutomationCenter.tsx", "utf8"),
  readFile("apps/desktop/src-tauri/src/workspace_undo_commands.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/kernel/workspace_undo.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/kernel/event_store/workspace_undo.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/kernel/task_lifecycle.rs", "utf8"),
  readFile("apps/desktop/src-tauri/src/main.rs", "utf8"),
]);
const productionStore = store.split("#[cfg(test)]")[0];

assert.match(panel, /invoke<WorkspaceUndoView\[\]>\("list_workspace_undo_items"\)/);
assert.match(panel, /invoke<WorkspaceUndoResult>\("undo_workspace_mutation", \{/);
assert.match(panel, /checkpointId: item\.id/);
assert.match(panel, /actionRevision: item\.action_revision/);
assert.match(panel, /结果未知时不会自动重试/);
assert.doesNotMatch(panel, /source_path|destination_path|credential|token|secret/i);
assert.match(center, /<WorkspaceUndoPanel language=\{language\} \/>/);
assert.match(commands, /apply_workspace_undo\(&store, checkpoint_id, action_revision\.trim\(\)\)/);
assert.doesNotMatch(commands, /source_path|destination_path/);
assert.match(kernel, /FILE_FLAG_OPEN_REPARSE_POINT/);
assert.match(kernel, /file_identity/);
assert.match(kernel, /MAX_WORKSPACE_UNDO_PREIMAGE_BYTES/);
assert.match(productionStore, /WorkspaceMutationCheckpointStatus::EffectStarted[\s\S]*RepairRequired/);
assert.doesNotMatch(productionStore, /client\.mutate|execute_checkpointed_mutation/);
assert.match(lifecycle, /TaskLifecycleSource::WorkspaceCheckpoint/);
assert.match(lifecycle, /TaskLifecycleActionKind::UndoLocalChange/);
assert.match(main, /list_workspace_undo_items,/);
assert.match(main, /undo_workspace_mutation,/);

console.log("workspace checkpoint and exact undo UI checks passed");
