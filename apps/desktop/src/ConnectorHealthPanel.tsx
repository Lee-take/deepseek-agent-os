import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type {
  ConnectorAccountSummary,
  ConnectorAuthorizationReview,
  Language,
} from "./types";

const healthCopy = {
  zh: {
    connected: "已连接",
    needs_repair: "需要修复",
    disconnect_pending: "正在完成断开",
    disconnected: "已断开",
    revocation_pending: "撤销结果待确认",
  },
  en: {
    connected: "Connected",
    needs_repair: "Needs repair",
    disconnect_pending: "Disconnect finishing",
    disconnected: "Disconnected",
    revocation_pending: "Revocation unconfirmed",
  },
} satisfies Record<Language, Record<ConnectorAccountSummary["health"], string>>;

function providerLabel(provider: ConnectorAccountSummary["provider_label"], language: Language) {
  switch (provider) {
    case "microsoft365":
      return "Microsoft 365";
    case "google_workspace":
      return "Google Workspace";
    default:
      return language === "zh" ? "工作区连接" : "Workspace connector";
  }
}

const syncCopy = {
  zh: {
    not_enabled: "未启用持续同步",
    never_synced: "尚未同步",
    healthy: "同步正常",
    delayed: "同步已延后",
    stopped: "同步已停止",
  },
  en: {
    not_enabled: "Continuous sync is off",
    never_synced: "Not synced yet",
    healthy: "Sync healthy",
    delayed: "Sync delayed",
    stopped: "Sync stopped",
  },
} satisfies Record<Language, Record<ConnectorAccountSummary["sync_state"], string>>;

const abilityCopy = {
  zh: {
    mail_read: "读取邮件",
    mail_attachments: "读取附件",
    mail_draft: "创建邮件草稿",
    mail_send: "审批后发送邮件",
    mail_sync: "持续同步邮件",
    calendar_read: "读取日历",
    calendar_change: "审批后修改日历",
    calendar_sync: "持续同步日历",
  },
  en: {
    mail_read: "Read mail",
    mail_attachments: "Read attachments",
    mail_draft: "Create mail drafts",
    mail_send: "Send mail after approval",
    mail_sync: "Continuously sync mail",
    calendar_read: "Read calendar",
    calendar_change: "Change calendar after approval",
    calendar_sync: "Continuously sync calendar",
  },
} satisfies Record<Language, Record<ConnectorAccountSummary["abilities"][number], string>>;

export function ConnectorHealthPanel({ language }: { language: Language }) {
  const [accounts, setAccounts] = useState<ConnectorAccountSummary[]>([]);
  const [reviews, setReviews] = useState<ConnectorAuthorizationReview[]>([]);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState(false);
  const zh = language === "zh";

  const refresh = useCallback(async () => {
    const [nextAccounts, nextReviews] = await Promise.all([
      invoke<ConnectorAccountSummary[]>("list_connector_account_summaries"),
      invoke<ConnectorAuthorizationReview[]>("list_connector_authorization_reviews"),
    ]);
    setAccounts(nextAccounts);
    setReviews(nextReviews);
    setError(false);
  }, []);

  useEffect(() => {
    void refresh().catch(() => setError(true));
  }, [refresh]);

  const disconnect = async (accountId: string) => {
    setPending(accountId);
    setError(false);
    try {
      await invoke("disconnect_connector_account", { accountId });
      await refresh();
    } catch {
      setError(true);
    } finally {
      setPending(null);
    }
  };

  const cancelReview = async (reviewId: string) => {
    setPending(reviewId);
    setError(false);
    try {
      await invoke("resolve_connector_authorization_review", {
        request: { review_id: reviewId, intent: "cancel" },
      });
      await refresh();
    } catch {
      setError(true);
    } finally {
      setPending(null);
    }
  };

  return (
    <section className="connector-health" aria-labelledby="connector-health-title">
      <div className="queue-heading">
        <strong id="connector-health-title">
          {zh ? "连接账户" : "Connected accounts"}
        </strong>
        <span>{accounts.length}</span>
      </div>
      <p className="section-description">
        {zh
          ? "这里只显示账户状态和已授权能力，不显示或传输登录凭据。"
          : "Only account health and granted abilities appear here. Sign-in credentials are never shown or transferred."}
      </p>
      {error ? (
        <p className="package-error" role="alert">
          {zh ? "无法安全更新账户状态，请稍后重试。" : "Account state could not be updated safely. Try again later."}
        </p>
      ) : null}
      {reviews
        .filter((review) => review.status !== "connected")
        .map((review) => (
          <article className="automation-row" key={review.review_id}>
            <div>
              <strong>{providerLabel(review.provider_label, language)}</strong>
              <span>
                {review.abilities.map((ability) => abilityCopy[language][ability]).join(" · ")}
              </span>
              <span>
                {review.status === "awaiting_confirmation"
                  ? zh
                    ? "正在等待连接确认；此版本尚未启用真实账户授权。"
                    : "Waiting for connection confirmation; live account authorization is not enabled in this build."
                  : review.status === "connecting"
                    ? zh
                      ? "正在安全完成连接。"
                      : "Finishing the connection safely."
                    : review.status === "cancelling"
                      ? zh
                        ? "正在安全清理连接信息。"
                        : "Removing connection information safely."
                      : review.status === "cancelled"
                        ? zh
                          ? "连接已取消。"
                          : "Connection cancelled."
                        : zh
                          ? "连接需要安全修复，系统不会自动重试授权。"
                          : "The connection needs safe repair; authorization will not retry automatically."}
              </span>
            </div>
            <span className={`access-status ${review.status}`}>
              {review.status === "awaiting_confirmation"
                ? zh
                  ? "等待确认"
                  : "Awaiting confirmation"
                : review.status === "connecting"
                  ? zh
                    ? "正在连接"
                    : "Connecting"
                  : review.status === "cancelling"
                    ? zh
                      ? "正在取消"
                      : "Cancelling"
                    : review.status === "cancelled"
                      ? zh
                        ? "已取消"
                        : "Cancelled"
                      : zh
                        ? "需要修复"
                        : "Needs repair"}
            </span>
            {review.status === "awaiting_confirmation" ? (
              <button
                type="button"
                disabled={pending !== null}
                onClick={() => void cancelReview(review.review_id)}
              >
                {pending === review.review_id
                  ? zh
                    ? "正在取消…"
                    : "Cancelling…"
                  : zh
                    ? "取消连接"
                    : "Cancel connection"}
              </button>
            ) : null}
          </article>
        ))}
      {accounts.length === 0 ? (
        <p className="empty-state">
          {zh ? "尚未连接工作区账户。" : "No workspace account is connected yet."}
        </p>
      ) : (
        accounts.map((account) => {
          const canDisconnect =
            account.health === "connected" || account.health === "needs_repair";
          return (
            <article className="automation-row" key={account.id}>
              <div>
                <strong>{account.display_name}</strong>
                <span>
                  {providerLabel(account.provider_label, language)} · {syncCopy[language][account.sync_state]}
                </span>
                <span>{account.abilities.map((ability) => abilityCopy[language][ability]).join(" · ")}</span>
                {account.last_successful_sync_at ? (
                  <span>
                    {zh ? "上次成功同步：" : "Last successful sync: "}
                    {new Date(account.last_successful_sync_at).toLocaleString()}
                  </span>
                ) : null}
                {account.health === "needs_repair" ? (
                  <span>
                    {zh
                      ? "当前尚无安全的重新授权入口；可断开本地连接，系统不会自动重新授权。"
                      : "A safe reauthorization path is not available yet. You can disconnect locally; the app will not reauthorize automatically."}
                  </span>
                ) : account.health === "disconnect_pending" ? (
                  <span>
                    {zh
                      ? "启动恢复会继续完成本地凭据移除。"
                      : "Startup recovery will continue local credential removal."}
                  </span>
                ) : account.health === "revocation_pending" ? (
                  <span>
                    {zh
                      ? "请在恢复中心核对下一步；系统不会自动重复撤销。"
                      : "Review the next step in Recovery Center; revocation will not be repeated automatically."}
                  </span>
                ) : null}
              </div>
              <span className={`access-status ${account.health}`}>
                {healthCopy[language][account.health]}
              </span>
              {canDisconnect ? (
                <button
                  type="button"
                  disabled={pending !== null}
                  onClick={() => void disconnect(account.id)}
                >
                  {pending === account.id
                    ? zh
                      ? "正在断开…"
                      : "Disconnecting…"
                    : zh
                      ? "断开连接"
                      : "Disconnect"}
                </button>
              ) : null}
            </article>
          );
        })
      )}
    </section>
  );
}
