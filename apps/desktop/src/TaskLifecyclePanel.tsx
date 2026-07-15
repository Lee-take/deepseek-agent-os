import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type {
  Language,
  TaskEffectState,
  TaskLifecyclePhase,
  TaskLifecycleSnapshot,
} from "./types";

const phaseCopy: Record<Language, Record<TaskLifecyclePhase, string>> = {
  zh: {
    queued: "已排队",
    running: "进行中",
    waiting_prerequisite: "等待准备条件",
    waiting_review: "等待你审阅",
    waiting_approval: "等待你确认",
    needs_recovery: "需要恢复",
    effect_unknown: "结果尚待核对",
    repair_required: "需要安全修复",
    blocked: "已暂停",
    completed: "已完成",
    failed: "未完成",
    cancelled: "已取消",
  },
  en: {
    queued: "Queued",
    running: "In progress",
    waiting_prerequisite: "Waiting for setup",
    waiting_review: "Waiting for your review",
    waiting_approval: "Waiting for confirmation",
    needs_recovery: "Recovery needed",
    effect_unknown: "Result needs checking",
    repair_required: "Safe repair needed",
    blocked: "Paused",
    completed: "Completed",
    failed: "Not completed",
    cancelled: "Cancelled",
  },
};

const effectCopy: Record<Language, Record<TaskEffectState, string>> = {
  zh: {
    no_effect: "尚未产生变更",
    read_only: "只读完成",
    local_reversible: "本地变更可恢复",
    local_applied: "本地变更已完成",
    local_uncertain: "本地结果尚待核对，不会自动重试",
    remote_known_not_applied: "未写入外部系统",
    remote_known_applied: "外部结果已确认",
    remote_uncertain: "不自动重试，请先核对",
    compensation_required: "需要补偿或人工处理",
  },
  en: {
    no_effect: "No change has been made",
    read_only: "Read-only work completed",
    local_reversible: "Local change can be recovered",
    local_applied: "Local change completed",
    local_uncertain: "Check the local result; it will not retry automatically",
    remote_known_not_applied: "No external write occurred",
    remote_known_applied: "External result confirmed",
    remote_uncertain: "Check first; this will not retry automatically",
    compensation_required: "Compensation or manual action is required",
  },
};

export function TaskLifecyclePanel({ language }: { language: Language }) {
  const [snapshot, setSnapshot] = useState<TaskLifecycleSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setSnapshot(await invoke<TaskLifecycleSnapshot>("list_task_lifecycle"));
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const zh = language === "zh";
  const items = snapshot?.items.slice(0, 12) ?? [];

  return (
    <section className="automation-card" aria-labelledby="task-lifecycle-title">
      <div className="queue-heading">
        <strong id="task-lifecycle-title">{zh ? "任务进度" : "Task activity"}</strong>
        <button type="button" disabled={loading} onClick={() => void refresh()}>
          {loading ? (zh ? "刷新中" : "Refreshing") : zh ? "刷新" : "Refresh"}
        </button>
      </div>
      <p className="automation-muted">
        {zh
          ? "这里统一显示任务、工具、连接器、文档和电脑操作的真实状态。结果未知时不会自动重复执行。"
          : "One view of tasks, tools, connections, documents, and computer actions. Unknown effects are never replayed automatically."}
      </p>
      {error ? <p className="error-text">{error}</p> : null}
      {!loading && items.length === 0 ? (
        <p className="automation-muted">{zh ? "暂无任务记录。" : "No task activity yet."}</p>
      ) : null}
      <div className="automation-list">
        {items.map((item) => (
          <article className="automation-row" key={`${item.source}:${item.id}`}>
            <strong>{item.title}</strong>
            <span className={`access-status ${item.phase}`}>
              {phaseCopy[language][item.phase]}
            </span>
            <small>{effectCopy[language][item.effect_state]}</small>
          </article>
        ))}
      </div>
    </section>
  );
}
