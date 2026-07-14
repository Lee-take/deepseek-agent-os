import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type {
  ConnectorRecoveryCommandResult,
  ConnectorRecoveryExternalEffectState,
  ConnectorRecoveryItem,
  ConnectorRecoveryNextStepCode,
  ConnectorRecoveryReasonCode,
  ConnectorRecoveryStatus,
  ConnectorReadActivity,
  Language,
} from "./types";

const reasonCopy = {
  zh: {
    attachment_legacy_workspace_unbound: "旧版落盘记录缺少可验证的工作区身份，自动处理已停止。",
    attachment_legacy_receipt_incomplete: "旧版已完成文件缺少完整身份回执，系统已保留文件。",
    attachment_retention_identity_changed: "保留文件的身份已变化，自动到期清理已停止。",
    attachment_stored_identity_changed: "已保存的文件身份不再匹配，自动清理已停止。",
    attachment_execution_record_incomplete: "持久执行记录不完整，系统已保留文件并隔离自动处理。",
    attachment_recovery_required: "附件落盘进入安全恢复状态，自动清理已停止。",
    account_needs_repair: "账户连接需要检查后才能再次使用。",
    account_disconnect_pending: "本地断开流程尚未完成，恢复流程将继续处理。",
    account_revocation_pending: "服务端撤销结果尚未确认，系统不会自动重放。",
    sync_retry_exhausted: "只读同步已用尽有限重试次数并停止。",
    reconciliation_required: "外部操作结果尚不确定，系统已冻结自动重放。",
  },
  en: {
    attachment_legacy_workspace_unbound:
      "This legacy landing has no verified workspace identity, so automatic handling stopped.",
    attachment_legacy_receipt_incomplete:
      "This legacy completed file has no complete identity receipt, so the file was preserved.",
    attachment_retention_identity_changed:
      "The retained file identity changed, so automatic expiry stopped.",
    attachment_stored_identity_changed:
      "The stored file identity no longer matches, so automatic cleanup stopped.",
    attachment_execution_record_incomplete:
      "The durable execution record is incomplete, so the file was preserved and automatic handling was isolated.",
    attachment_recovery_required:
      "The attachment landing entered safe recovery and automatic cleanup stopped.",
    account_needs_repair: "The account connection needs review before it can be used again.",
    account_disconnect_pending:
      "The local disconnect flow is incomplete and recovery will continue it.",
    account_revocation_pending:
      "Provider revocation is not confirmed, so the action will not be replayed automatically.",
    sync_retry_exhausted: "Read-only sync exhausted its bounded retries and stopped.",
    reconciliation_required:
      "The external action result is uncertain and automatic replay is frozen.",
  },
} satisfies Record<Language, Record<ConnectorRecoveryReasonCode, string>>;

const externalEffectCopy = {
  zh: {
    local_file_preserved: "未更改外部服务状态；本地文件仍被保留。",
    no_external_write: "没有执行外部写入。",
    local_credential_removal_pending: "仅本地凭据移除尚未完成；不会调用外部服务。",
    external_result_uncertain: "外部写入可能已发生，当前结果不确定。",
  },
  en: {
    local_file_preserved: "No provider state was changed; the local file remains preserved.",
    no_external_write: "No external write was performed.",
    local_credential_removal_pending:
      "Only local credential removal is pending; no provider call will be made.",
    external_result_uncertain: "An external write may have happened; the result is uncertain.",
  },
} satisfies Record<Language, Record<ConnectorRecoveryExternalEffectState, string>>;

const nextStepCopy = {
  zh: {
    retry_local_cleanup: "确认文件不再需要后，可使用下方按钮重新安排本地清理。",
    inspect_file_manually: "请人工检查保留的文件；系统不会自动删除。",
    review_account_connection: "请检查账户连接；需要时断开后重新连接，再恢复同步。",
    wait_for_local_disconnect_recovery: "请保持应用可运行；启动恢复会继续完成本地断开。",
    repair_account_connection: "请先核对服务端账户状态，再重新连接；不要重复撤销。",
    verify_provider_state: "请人工核对服务端结果；在确认前不要重复执行该操作。",
  },
  en: {
    retry_local_cleanup:
      "After confirming the file is no longer needed, use the button below to reschedule local cleanup.",
    inspect_file_manually: "Inspect the preserved file manually; it will not be deleted automatically.",
    review_account_connection:
      "Review the account connection and reconnect if needed before resuming sync.",
    wait_for_local_disconnect_recovery:
      "Keep the app available; startup recovery will continue the local disconnect.",
    repair_account_connection:
      "Verify the provider account state, then reconnect; do not repeat revocation.",
    verify_provider_state:
      "Verify the provider result manually and do not repeat the action until it is confirmed.",
  },
} satisfies Record<Language, Record<ConnectorRecoveryNextStepCode, string>>;

const statusCopy = {
  zh: {
    repair_required: "需要安全检查",
    needs_repair: "连接需要修复",
    disconnect_pending: "正在完成断开",
    revocation_pending: "撤销结果待确认",
    sync_exhausted: "同步已停止",
    reconciliation_required: "结果需要核对",
  },
  en: {
    repair_required: "Safe review needed",
    needs_repair: "Connection needs repair",
    disconnect_pending: "Disconnect finishing",
    revocation_pending: "Revocation unconfirmed",
    sync_exhausted: "Sync stopped",
    reconciliation_required: "Result needs verification",
  },
} satisfies Record<Language, Record<ConnectorRecoveryStatus, string>>;

export function RecoveryCenter({ language }: { language: Language }) {
  const [items, setItems] = useState<ConnectorRecoveryItem[]>([]);
  const [readActivity, setReadActivity] = useState<ConnectorReadActivity[]>([]);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<"" | "load" | "retry" | "sync" | "inspect">("");
  const [activityError, setActivityError] = useState(false);
  const [notice, setNotice] = useState<"" | "cleanup" | "sync" | "inspect" | "already">("");
  const [loading, setLoading] = useState(true);
  const zh = language === "zh";

  const refresh = useCallback(async () => {
    const [recoveryResult, activityResult] = await Promise.allSettled([
      invoke<ConnectorRecoveryItem[]>("list_connector_recovery_items"),
      invoke<ConnectorReadActivity[]>("list_connector_read_activity"),
    ]);
    if (recoveryResult.status === "fulfilled") {
      setItems(recoveryResult.value);
      setError("");
    } else {
      setError("load");
    }
    if (activityResult.status === "fulfilled") {
      setReadActivity(activityResult.value);
      setActivityError(false);
    } else {
      setActivityError(true);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void refresh().catch(() => {
      setError("load");
      setLoading(false);
    });
    const timer = window.setInterval(() => {
      void refresh().catch(() => setError("load"));
    }, 30_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const retry = async (item: ConnectorRecoveryItem) => {
    if (item.action?.kind !== "retry_attachment_cleanup") return;
    const itemId = item.id;
    setPending(itemId);
    setError("");
    setNotice("");
    try {
      const result = await invoke<ConnectorRecoveryCommandResult>(
        "retry_connector_attachment_cleanup",
        {
          request: {
            item_id: itemId,
            action_revision: item.action.action_revision,
          },
        },
      );
      setItems(result.items);
      setNotice(result.acceptance === "already_accepted" ? "already" : "cleanup");
    } catch {
      setError("retry");
    } finally {
      setPending(null);
    }
  };

  const resumeSync = async (item: ConnectorRecoveryItem) => {
    if (item.action?.kind !== "resume_sync") return;
    setPending(item.id);
    setError("");
    setNotice("");
    try {
      const result = await invoke<ConnectorRecoveryCommandResult>(
        "resume_connector_read_sync",
        {
          request: {
            item_id: item.id,
            action_revision: item.action.action_revision,
          },
        },
      );
      setItems(result.items);
      setNotice(result.acceptance === "already_accepted" ? "already" : "sync");
    } catch {
      setError("sync");
    } finally {
      setPending(null);
    }
  };

  const inspectExternalResult = async (item: ConnectorRecoveryItem) => {
    if (item.action?.kind !== "inspect_external_result") return;
    setPending(item.id);
    setError("");
    setNotice("");
    try {
      const result = await invoke<ConnectorRecoveryCommandResult>(
        "inspect_connector_external_result",
        {
          request: {
            item_id: item.id,
            action_revision: item.action.action_revision,
          },
        },
      );
      setItems(result.items);
      setNotice(result.acceptance === "already_accepted" ? "already" : "inspect");
    } catch {
      setError("inspect");
    } finally {
      setPending(null);
    }
  };

  const errorCopy =
    error === "retry"
      ? zh
        ? "无法安全安排本地清理。请刷新恢复状态后再试。"
        : "Local cleanup could not be queued safely. Refresh recovery state and try again."
      : error === "sync"
        ? zh
          ? "无法安全恢复只读同步。请刷新恢复状态后再试。"
          : "Read-only sync could not be resumed safely. Refresh recovery state and try again."
        : error === "inspect"
          ? zh
            ? "无法安全安排外部结果核对。请刷新恢复状态后再试。"
            : "The external result check could not be scheduled safely. Refresh recovery state and try again."
          : zh
            ? "无法安全加载恢复状态。请稍后重试。"
            : "Recovery state could not be loaded safely. Try again later.";

  return (
    <section className="connector-health recovery-center" aria-labelledby="recovery-center-title">
      <div className="queue-heading">
        <strong id="recovery-center-title">{zh ? "恢复中心" : "Recovery Center"}</strong>
        <span>{items.length}</span>
      </div>
      <p className="section-description">
        {zh
          ? "这里只显示脱敏的恢复状态。可用操作仅执行明确标注的本地恢复或只读核对与同步，不会重新授权连接或重放外部写操作。"
          : "Only redacted recovery state appears here. Available actions perform only clearly labeled local recovery or read-only checks and sync; they never reauthorize connections or replay external writes."}
      </p>
      {activityError ? (
        <p className="package-error" role="status">
          {zh
            ? "只读活动暂时无法显示；恢复项目仍可正常查看和处理。"
            : "Read activity is temporarily unavailable; recovery items remain available."}
        </p>
      ) : null}
      {error ? (
        <p className="package-error" role="alert">
          {errorCopy}
        </p>
      ) : null}
      <p aria-live="polite" className={notice ? "package-success" : "sr-only"}>
        {notice === "already"
          ? zh
            ? "该恢复操作已经安排，无需重复提交；这不表示后台工作已经完成。"
            : "This recovery action was already queued; no duplicate submission was needed, and background work is not yet claimed complete."
          : notice === "inspect"
          ? zh
            ? "已安排只读核对；在确认前不会重复执行原操作。"
            : "A read-only check was scheduled; the original action will not be repeated before confirmation."
          : notice === "sync"
          ? zh
            ? "已恢复只读同步调度；尚未声称同步完成。"
            : "Read-only sync scheduling resumed; sync is not yet claimed complete."
          : notice === "cleanup"
          ? zh
            ? "已安全安排本地清理。"
            : "Local cleanup was queued safely."
          : ""}
      </p>
      {loading ? (
        <p className="empty-state">{zh ? "正在加载恢复状态…" : "Loading recovery state…"}</p>
      ) : items.length === 0 ? (
        <p className="empty-state">
          {zh ? "目前没有需要处理的恢复项目。" : "No recovery items need attention."}
        </p>
      ) : (
        items.map((item) => {
          const title =
            item.kind === "reconciliation"
              ? zh
                ? "外部操作"
                : "External action"
              : item.title;
          const capability = item.sync_capability
            ? item.sync_capability === "mail"
              ? zh
                ? "邮件同步"
                : "Mail sync"
              : zh
                ? "日历同步"
                : "Calendar sync"
            : null;
          return (
            <article className="automation-row" key={`${item.kind}:${item.id}`}>
              <div>
                <strong>{capability ? `${title} · ${capability}` : title}</strong>
                <span>
                  <b>{zh ? "发生了什么：" : "What happened: "}</b>
                  {reasonCopy[language][item.reason_code]}
                </span>
                <span>
                  <b>{zh ? "外部操作：" : "External effect: "}</b>
                  {externalEffectCopy[language][item.external_effect_state]}
                </span>
                <span>
                  <b>{zh ? "下一步：" : "Next step: "}</b>
                  {nextStepCopy[language][item.next_step_code]}
                </span>
                <time dateTime={item.updated_at}>
                  {new Date(item.updated_at).toLocaleString(zh ? "zh-CN" : "en-US")}
                </time>
              </div>
              <span className={`access-status ${item.status}`}>
                {statusCopy[language][item.status]}
              </span>
              {item.action?.kind === "retry_attachment_cleanup" ? (
                <button type="button" disabled={pending !== null} onClick={() => void retry(item)}>
                  {pending === item.id
                    ? zh
                      ? "正在安全重试…"
                      : "Retrying safely…"
                    : zh
                      ? "安全重试"
                      : "Retry safely"}
                </button>
              ) : item.action?.kind === "resume_sync" ? (
                <button
                  type="button"
                  disabled={pending !== null}
                  onClick={() => void resumeSync(item)}
                >
                  {pending === item.id
                    ? zh
                      ? "正在恢复…"
                      : "Resuming…"
                    : zh
                      ? "恢复只读同步"
                      : "Resume read-only sync"}
                </button>
              ) : item.action?.kind === "inspect_external_result" ? (
                <button
                  type="button"
                  disabled={pending !== null}
                  onClick={() => void inspectExternalResult(item)}
                >
                  {pending === item.id
                    ? zh
                      ? "正在安排安全核对…"
                      : "Scheduling safe check…"
                    : zh
                      ? "核对外部结果"
                      : "Check external result"}
                </button>
              ) : null}
            </article>
          );
        })
      )}
      <div className="queue-heading recovery-activity-heading">
        <strong>{zh ? "只读活动" : "Read activity"}</strong>
        <span>{readActivity.length}</span>
      </div>
      <p className="section-description">
        {zh
          ? "显示最近 50 项邮件或日历只读执行。查询内容、账户、提供者、凭据和内部领取信息不会出现在这里。"
          : "Shows the latest 50 mail or calendar read executions. Queries, accounts, providers, credentials, and internal claim data never appear here."}
      </p>
      {readActivity.length === 0 ? (
        <p className="empty-state">{zh ? "目前没有只读活动。" : "No read activity yet."}</p>
      ) : (
        readActivity.map((activity) => (
          <article className="automation-row" key={activity.id}>
            <div>
              <strong>
                {activity.kind === "mail"
                  ? zh
                    ? "邮件读取"
                    : "Mail read"
                  : zh
                    ? "日历读取"
                    : "Calendar read"}
              </strong>
              {activity.item_count !== undefined ? (
                <span>
                  {zh ? `已安全记录 ${activity.item_count} 项结果。` : `${activity.item_count} results recorded safely.`}
                </span>
              ) : null}
              {activity.error_code ? (
                <span>
                  {activity.error_code === "external_result_uncertain"
                    ? zh
                      ? "结果不确定，系统不会自动重放读取。"
                      : "The result is uncertain and the read will not replay automatically."
                    : zh
                      ? "执行需要检查；敏感详情未显示。"
                      : "The execution needs review; sensitive details are hidden."}
                </span>
              ) : null}
              <time dateTime={activity.updated_at}>
                {new Date(activity.updated_at).toLocaleString(zh ? "zh-CN" : "en-US")}
              </time>
            </div>
            <span className={`access-status ${activity.phase}`}>
              {activity.phase === "queued"
                ? zh
                  ? "等待执行"
                  : "Queued"
                : activity.phase === "running"
                  ? zh
                    ? "正在执行"
                    : "Running"
                  : activity.phase === "completed"
                    ? zh
                      ? "已完成"
                      : "Completed"
                    : activity.phase === "cancelled"
                      ? zh
                        ? "已取消"
                        : "Cancelled"
                      : zh
                        ? "需要检查"
                        : "Needs attention"}
            </span>
          </article>
        ))
      )}
    </section>
  );
}
