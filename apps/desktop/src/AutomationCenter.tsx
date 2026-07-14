import { FormEvent, useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type {
  AutomationDefinition,
  AutomationRun,
  Language,
  ReviewQueueItem,
} from "./types";
import { ConnectorHealthPanel } from "./ConnectorHealthPanel";
import { ArtifactDeliveryPanel } from "./ArtifactDeliveryPanel";
import { RecoveryCenter } from "./RecoveryCenter";

type Props = {
  language: Language;
  onRunQueued: () => Promise<void>;
};

export function AutomationCenter({ language, onRunQueued }: Props) {
  const [definitions, setDefinitions] = useState<AutomationDefinition[]>([]);
  const [runs, setRuns] = useState<AutomationRun[]>([]);
  const [reviewItems, setReviewItems] = useState<ReviewQueueItem[]>([]);
  const [goal, setGoal] = useState("");
  const [runAt, setRunAt] = useState("");
  const [frequency, setFrequency] = useState<"once" | "daily" | "weekly" | "monthly">("once");
  const [timeOfDay, setTimeOfDay] = useState("09:00");
  const [weekday, setWeekday] = useState(0);
  const [monthDay, setMonthDay] = useState(1);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingGoal, setEditingGoal] = useState("");

  const zh = language === "zh";
  const scheduleLabel = (definition: AutomationDefinition) => {
    const schedule = definition.schedule;
    if (schedule.kind === "once") return new Date(schedule.run_at).toLocaleString();
    const time = `${String(schedule.hour).padStart(2, "0")}:${String(schedule.minute).padStart(2, "0")}`;
    if (schedule.kind === "daily") return `${zh ? "每天" : "Daily"} ${time}`;
    if (schedule.kind === "weekly") return `${zh ? "每周" : "Weekly"} ${schedule.weekday + 1} · ${time}`;
    if (schedule.kind === "monthly") return `${zh ? "每月" : "Monthly"} ${schedule.day} · ${time}`;
    return `${zh ? "指定星期" : "Selected weekdays"} · ${time}`;
  };
  const refresh = useCallback(async () => {
    const [nextDefinitions, nextRuns, nextReviewItems] = await Promise.all([
      invoke<AutomationDefinition[]>("list_automation_definitions"),
      invoke<AutomationRun[]>("list_automation_runs"),
      invoke<ReviewQueueItem[]>("list_automation_review_items"),
    ]);
    setDefinitions(nextDefinitions);
    setRuns(nextRuns);
    setReviewItems(nextReviewItems);
  }, []);

  useEffect(() => {
    void refresh().catch((reason) => setError(String(reason)));
    const timer = window.setInterval(() => {
      void refresh().catch((reason) => setError(String(reason)));
    }, 30_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!goal.trim() || (frequency === "once" && !runAt)) return;
    setPending(true);
    setError("");
    try {
      const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
      if (frequency === "once") {
        await invoke("create_once_automation", {
          goal: goal.trim(),
          timezone,
          runAt: new Date(runAt).toISOString(),
        });
      } else {
        const [hour, minute] = timeOfDay.split(":").map(Number);
        await invoke("create_recurring_automation", {
          goal: goal.trim(),
          timezone,
          frequency,
          hour,
          minute,
          weekday: frequency === "weekly" ? weekday : null,
          day: frequency === "monthly" ? monthDay : null,
          weekdays: null,
        });
      }
      setGoal("");
      setRunAt("");
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setPending(false);
    }
  };

  const setEnabled = async (definition: AutomationDefinition, enabled: boolean) => {
    setPending(true);
    setError("");
    try {
      await invoke("set_automation_enabled", {
        definitionId: definition.id,
        enabled,
      });
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setPending(false);
    }
  };

  const runNow = async (definitionId: string) => {
    setPending(true);
    setError("");
    try {
      await invoke("run_automation_now", {
        definitionId,
        manualInvocationId: crypto.randomUUID(),
      });
      await onRunQueued();
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setPending(false);
    }
  };

  const saveGoal = async (definitionId: string) => {
    if (!editingGoal.trim()) return;
    setPending(true);
    try {
      await invoke("update_automation_goal", { definitionId, goal: editingGoal.trim() });
      setEditingId(null);
      setEditingGoal("");
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setPending(false);
    }
  };

  const deleteAutomation = async (definitionId: string) => {
    setPending(true);
    try {
      await invoke("delete_automation", { definitionId });
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setPending(false);
    }
  };

  const resolveReview = async (itemId: string, accepted: boolean) => {
    setPending(true);
    try {
      await invoke("resolve_automation_review_item", { itemId, accepted });
      await refresh();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setPending(false);
    }
  };

  return (
    <section className="automation-center" aria-labelledby="automation-center-title">
      <div className="section-heading">
        <div>
          <span>{zh ? "自动任务" : "Automations"}</span>
          <h2 id="automation-center-title">
            {zh ? "让 DS Agent 按时继续工作" : "Let DS Agent continue work on schedule"}
          </h2>
        </div>
        <span>{definitions.length}</span>
      </div>
      <p className="section-description">
        {zh
          ? "任务会保留运行记录；需要审阅或对外修改时仍会等待你确认。"
          : "Runs stay recorded. Reviews and external changes still wait for your confirmation."}
      </p>
      <form className="automation-form" onSubmit={submit}>
        <input
          value={goal}
          onChange={(event) => setGoal(event.target.value)}
          placeholder={zh ? "例如：明天下午整理本周项目进展" : "For example: summarize this week's progress tomorrow"}
          aria-label={zh ? "任务目标" : "Automation goal"}
        />
        <select value={frequency} onChange={(event) => setFrequency(event.target.value as typeof frequency)} aria-label={zh ? "重复频率" : "Frequency"}>
          <option value="once">{zh ? "一次" : "Once"}</option>
          <option value="daily">{zh ? "每天" : "Daily"}</option>
          <option value="weekly">{zh ? "每周" : "Weekly"}</option>
          <option value="monthly">{zh ? "每月" : "Monthly"}</option>
        </select>
        {frequency === "once" ? (
          <input type="datetime-local" value={runAt} onChange={(event) => setRunAt(event.target.value)} aria-label={zh ? "运行时间" : "Run time"} />
        ) : (
          <input type="time" value={timeOfDay} onChange={(event) => setTimeOfDay(event.target.value)} aria-label={zh ? "运行时间" : "Run time"} />
        )}
        {frequency === "weekly" ? (
          <select value={weekday} onChange={(event) => setWeekday(Number(event.target.value))} aria-label={zh ? "星期" : "Weekday"}>
            {[0, 1, 2, 3, 4, 5, 6].map((value) => <option value={value} key={value}>{zh ? `星期${["一", "二", "三", "四", "五", "六", "日"][value]}` : ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][value]}</option>)}
          </select>
        ) : null}
        {frequency === "monthly" ? (
          <input type="number" min={1} max={31} value={monthDay} onChange={(event) => setMonthDay(Number(event.target.value))} aria-label={zh ? "每月日期" : "Day of month"} />
        ) : null}
        <button type="submit" disabled={pending || !goal.trim() || (frequency === "once" && !runAt)}>
          {pending ? (zh ? "正在保存…" : "Saving…") : zh ? "创建任务" : "Create automation"}
        </button>
      </form>
      {error ? <p className="package-error">{error}</p> : null}
      <div className="automation-grid">
        <div>
          <div className="queue-heading">
            <strong>{zh ? "任务计划" : "Schedules"}</strong>
            <span>{definitions.length}</span>
          </div>
          {definitions.length === 0 ? (
            <p className="empty-state">{zh ? "还没有自动任务。" : "No automations yet."}</p>
          ) : (
            definitions.filter((definition) => definition.status !== "deleted").map((definition) => (
              <article className="automation-row" key={definition.id}>
                <div>
                  {editingId === definition.id ? (
                    <input value={editingGoal} onChange={(event) => setEditingGoal(event.target.value)} aria-label={zh ? "编辑任务目标" : "Edit automation goal"} />
                  ) : <strong>{definition.goal}</strong>}
                  <span>{scheduleLabel(definition)}</span>
                </div>
                {editingId === definition.id ? (
                  <button type="button" disabled={pending} onClick={() => void saveGoal(definition.id)}>{zh ? "保存" : "Save"}</button>
                ) : (
                  <button type="button" disabled={pending} onClick={() => { setEditingId(definition.id); setEditingGoal(definition.goal); }}>{zh ? "编辑" : "Edit"}</button>
                )}
                <button
                  type="button"
                  disabled={pending}
                  onClick={() => void setEnabled(definition, definition.status === "paused")}
                >
                  {definition.status === "paused"
                    ? zh
                      ? "恢复"
                      : "Resume"
                    : zh
                      ? "暂停"
                      : "Pause"}
                </button>
                <button type="button" disabled={pending} onClick={() => void runNow(definition.id)}>{zh ? "立即运行" : "Run now"}</button>
                <button type="button" disabled={pending} onClick={() => void deleteAutomation(definition.id)}>{zh ? "删除" : "Delete"}</button>
              </article>
            ))
          )}
        </div>
        <div>
          <div className="queue-heading">
            <strong>{zh ? "最近运行" : "Recent runs"}</strong>
            <span>{runs.length}</span>
          </div>
          {runs.slice(-5).reverse().map((run) => (
            <article className="automation-row" key={run.id}>
              <div>
                <strong>{new Date(run.scheduled_for).toLocaleString()}</strong>
                <span>{run.last_error ?? run.status}</span>
              </div>
              <span className={`access-status ${run.status}`}>{run.status}</span>
            </article>
          ))}
          <div className="queue-heading">
            <strong>{zh ? "待审阅" : "Review queue"}</strong>
            <span>{reviewItems.length}</span>
          </div>
          {reviewItems.map((item) => (
            <article className="automation-row" key={item.id}>
              <strong>{item.title}</strong>
              <span className={`access-status ${item.status}`}>{item.status}</span>
              {item.status === "pending_review" ? (
                <>
                  <button type="button" disabled={pending} onClick={() => void resolveReview(item.id, true)}>{zh ? "接受" : "Accept"}</button>
                  <button type="button" disabled={pending} onClick={() => void resolveReview(item.id, false)}>{zh ? "拒绝" : "Reject"}</button>
                </>
              ) : null}
            </article>
          ))}
        </div>
      </div>
      <ConnectorHealthPanel language={language} />
      <RecoveryCenter language={language} />
      <ArtifactDeliveryPanel language={language} />
    </section>
  );
}
