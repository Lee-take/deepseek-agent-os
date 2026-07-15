import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type {
  Language,
  WorkspaceUndoResult,
  WorkspaceUndoView,
} from "./types";

const titleCopy: Record<Language, Record<string, string>> = {
  zh: {
    created_local_file: "已创建本地文件",
    updated_local_file: "已更新本地文件",
    deleted_local_file: "已删除本地文件",
    renamed_local_file: "已重命名本地文件",
    created_local_directory: "已创建本地文件夹",
    renamed_local_directory: "已重命名本地文件夹",
    deleted_local_directory: "已删除本地文件夹",
  },
  en: {
    created_local_file: "Local file created",
    updated_local_file: "Local file updated",
    deleted_local_file: "Local file deleted",
    renamed_local_file: "Local file renamed",
    created_local_directory: "Local folder created",
    renamed_local_directory: "Local folder renamed",
    deleted_local_directory: "Local folder deleted",
  },
};

const statusCopy: Record<Language, Record<string, string>> = {
  zh: {
    ready: "可以精确撤销",
    not_undoable: "此变更不能自动撤销",
    undone: "已撤销",
    repair_required: "结果需要人工核对",
    failed: "未产生变更",
    effect_started: "正在核对结果",
    undo_started: "正在核对撤销结果",
    intent: "正在准备",
    prepared: "正在准备",
  },
  en: {
    ready: "Exact undo available",
    not_undoable: "This change cannot be undone automatically",
    undone: "Undone",
    repair_required: "Manual result check required",
    failed: "No change was made",
    effect_started: "Checking the result",
    undo_started: "Checking the undo result",
    intent: "Preparing",
    prepared: "Preparing",
  },
};

export function WorkspaceUndoPanel({ language }: { language: Language }) {
  const [items, setItems] = useState<WorkspaceUndoView[]>([]);
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const zh = language === "zh";

  const refresh = useCallback(async () => {
    setError(null);
    try {
      setItems(await invoke<WorkspaceUndoView[]>("list_workspace_undo_items"));
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const undo = useCallback(
    async (item: WorkspaceUndoView) => {
      if (!item.action_revision) return;
      setPendingId(item.id);
      setError(null);
      try {
        await invoke<WorkspaceUndoResult>("undo_workspace_mutation", {
          checkpointId: item.id,
          actionRevision: item.action_revision,
        });
        await refresh();
      } catch (reason) {
        setError(String(reason));
        await refresh();
      } finally {
        setPendingId(null);
      }
    },
    [refresh],
  );

  const visible = items
    .filter((item) =>
      ["ready", "not_undoable", "undone", "repair_required"].includes(item.status),
    )
    .slice(0, 8);

  return (
    <section className="automation-card" aria-labelledby="workspace-undo-title">
      <div className="queue-heading">
        <strong id="workspace-undo-title">{zh ? "本地变更与撤销" : "Local changes and undo"}</strong>
        <button type="button" onClick={() => void refresh()}>
          {zh ? "刷新" : "Refresh"}
        </button>
      </div>
      <p className="automation-muted">
        {zh
          ? "只有目标仍是原操作产生的同一个对象时才会撤销。结果未知时不会自动重试，也不会显示文件路径。"
          : "Undo runs only when the target is still the exact object created by the original action. Unknown results are not retried, and file paths are never shown here."}
      </p>
      {error ? <p className="error-text">{error}</p> : null}
      {visible.length === 0 ? (
        <p className="automation-muted">{zh ? "暂无本地变更记录。" : "No local changes yet."}</p>
      ) : null}
      <div className="automation-list">
        {visible.map((item) => (
          <article className="automation-row" key={item.id}>
            <strong>{titleCopy[language][item.title_code] ?? (zh ? "本地变更" : "Local change")}</strong>
            <span className={`access-status ${item.status}`}>
              {statusCopy[language][item.status] ?? item.status}
            </span>
            {item.undo_available && item.action_revision ? (
              <button
                type="button"
                disabled={pendingId === item.id}
                onClick={() => void undo(item)}
              >
                {pendingId === item.id
                  ? zh
                    ? "正在撤销"
                    : "Undoing"
                  : zh
                    ? "撤销这次变更"
                    : "Undo this change"}
              </button>
            ) : null}
          </article>
        ))}
      </div>
    </section>
  );
}
