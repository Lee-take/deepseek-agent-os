import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { CalendarCheck2, MailCheck, ShieldCheck } from "lucide-react";

import type {
  ConnectedWorkCalendarEvent,
  ConnectedWorkExecution,
  ConnectedWorkMailAddress,
  ConnectedWorkReview,
  Language,
} from "./types";

function addressLabel(address: ConnectedWorkMailAddress) {
  return address.display_name
    ? `${address.display_name} <${address.address}>`
    : address.address;
}
function addressList(addresses: ConnectedWorkMailAddress[]) {
  return addresses.map(addressLabel).join(", ");
}

function calendarEvent(review: ConnectedWorkReview): ConnectedWorkCalendarEvent | null {
  return review.kind === "calendar" && "event" in review.intent
    ? review.intent.event
    : null;
}

function calendarActionLabel(review: ConnectedWorkReview, zh: boolean) {
  if (review.kind !== "calendar") return "";
  switch (review.intent.kind) {
    case "calendar_create_event":
      return zh ? "新建日程" : "Create event";
    case "calendar_update_event":
      return zh ? "更新日程" : "Update event";
    case "calendar_cancel_event":
      return zh ? "取消日程" : "Cancel event";
  }
}

export function ConnectedWorkReviewPanel({ language }: { language: Language }) {
  const [reviews, setReviews] = useState<ConnectedWorkReview[]>([]);
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const zh = language === "zh";

  const refresh = useCallback(async () => {
    const next = await invoke<ConnectedWorkReview[]>("list_connected_work_reviews");
    setReviews(next);
  }, []);

  useEffect(() => {
    void refresh().catch((reason) => setError(String(reason)));
    const timer = window.setInterval(() => {
      void refresh().catch((reason) => setError(String(reason)));
    }, 30_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const requestApproval = async (review: ConnectedWorkReview) => {
    setPendingId(review.review.id);
    setError("");
    setNotice("");
    try {
      const updated = await invoke<ConnectedWorkReview>(
        "request_connected_work_approval",
        {
          reviewId: review.review.id,
          actionRevision: review.review.action_revision,
        },
      );
      setReviews((current) =>
        current.map((item) => (item.review.id === updated.review.id ? updated : item)),
      );
      setNotice(
        zh
          ? "内容已冻结。最终批准只对当前这份内容有效。"
          : "Content is frozen. Final approval applies only to this exact version.",
      );
    } catch (reason) {
      setError(String(reason));
      await refresh().catch(() => undefined);
    } finally {
      setPendingId(null);
    }
  };

  const reject = async (review: ConnectedWorkReview) => {
    setPendingId(review.review.id);
    setError("");
    setNotice("");
    try {
      await invoke("reject_connected_work_review", {
        reviewId: review.review.id,
        actionRevision: review.review.action_revision,
      });
      await refresh();
      setNotice(zh ? "已拒绝，未进行任何外部修改。" : "Rejected. No external change was made.");
    } catch (reason) {
      setError(String(reason));
      await refresh().catch(() => undefined);
    } finally {
      setPendingId(null);
    }
  };

  const approveAndRun = async (review: ConnectedWorkReview) => {
    setPendingId(review.review.id);
    setError("");
    setNotice("");
    try {
      const result = await invoke<ConnectedWorkExecution>(
        "approve_and_run_connected_work_review",
        {
          reviewId: review.review.id,
          actionRevision: review.review.action_revision,
        },
      );
      await refresh();
      setNotice(
        result.effect_state === "known_applied"
          ? zh
            ? "连接账户已确认完成，并已保存可核对证据。"
            : "The connected account confirmed completion and evidence was saved."
          : zh
            ? "外部结果尚不能确认。系统不会重复执行，请前往恢复中心核对。"
            : "The external result is uncertain. DS Agent will not repeat it; review Recovery Center.",
      );
    } catch (reason) {
      setError(String(reason));
      await refresh().catch(() => undefined);
    } finally {
      setPendingId(null);
    }
  };

  if (reviews.length === 0 && !error && !notice) return null;

  return (
    <section className="connected-work-review" aria-labelledby="connected-work-review-title">
      <div className="queue-heading">
        <strong id="connected-work-review-title">
          {zh ? "发送与日历审阅" : "Mail and calendar review"}
        </strong>
        <span>{reviews.length}</span>
      </div>
      <p className="section-description">
        {zh
          ? "先核对完整内容，再对这一份精确版本批准。批准不会自动沿用到修改后的内容。"
          : "Review the full content, then approve this exact version. Approval never carries over to edited content."}
      </p>
      <div className="connected-work-list">
        {reviews.map((review) => {
          const waitingForExactApproval = review.review.status === "pending_approval";
          const busy = pendingId === review.review.id;
          const event = calendarEvent(review);
          return (
            <article className="connected-work-card" key={review.review.id}>
              <div className="connected-work-card-heading">
                <span className="connected-work-icon" aria-hidden="true">
                  {review.kind === "mail" ? <MailCheck size={18} /> : <CalendarCheck2 size={18} />}
                </span>
                <div>
                  <strong>
                    {review.kind === "mail"
                      ? zh
                        ? "准备发送的邮件"
                        : "Email ready to send"
                      : calendarActionLabel(review, zh)}
                  </strong>
                  <span>{review.account_display_name}</span>
                </div>
                <span className={`access-status ${review.review.status}`}>
                  {waitingForExactApproval
                    ? zh
                      ? "等待最终批准"
                      : "Awaiting final approval"
                    : zh
                      ? "等待内容审阅"
                      : "Awaiting content review"}
                </span>
              </div>

              {review.kind === "mail" ? (
                <div className="connected-work-content">
                  <dl className="connected-work-facts">
                    <div>
                      <dt>{zh ? "收件人" : "To"}</dt>
                      <dd>{addressList(review.content.to)}</dd>
                    </div>
                    {review.content.cc.length > 0 ? (
                      <div>
                        <dt>{zh ? "抄送" : "Cc"}</dt>
                        <dd>{addressList(review.content.cc)}</dd>
                      </div>
                    ) : null}
                    {review.content.bcc.length > 0 ? (
                      <div>
                        <dt>{zh ? "密送" : "Bcc"}</dt>
                        <dd>{addressList(review.content.bcc)}</dd>
                      </div>
                    ) : null}
                    <div>
                      <dt>{zh ? "主题" : "Subject"}</dt>
                      <dd>{review.content.subject || (zh ? "（无主题）" : "(No subject)")}</dd>
                    </div>
                  </dl>
                  <div className="connected-work-body">
                    <span>{zh ? "正文" : "Message"}</span>
                    <p>{review.content.body_text || (zh ? "（无正文）" : "(No message body)")}</p>
                  </div>
                </div>
              ) : (
                <div className="connected-work-content">
                  {event ? (
                    <>
                      <dl className="connected-work-facts">
                        <div>
                          <dt>{zh ? "日程" : "Event"}</dt>
                          <dd>{event.title}</dd>
                        </div>
                        <div>
                          <dt>{zh ? "时间" : "Time"}</dt>
                          <dd>
                            {new Date(event.starts_at).toLocaleString()} – {new Date(event.ends_at).toLocaleString()}
                          </dd>
                        </div>
                        <div>
                          <dt>{zh ? "时区" : "Timezone"}</dt>
                          <dd>{event.timezone}</dd>
                        </div>
                        {event.location ? (
                          <div>
                            <dt>{zh ? "地点" : "Location"}</dt>
                            <dd>{event.location}</dd>
                          </div>
                        ) : null}
                        {event.attendees.length > 0 ? (
                          <div>
                            <dt>{zh ? "参与人" : "Attendees"}</dt>
                            <dd>{addressList(event.attendees)}</dd>
                          </div>
                        ) : null}
                      </dl>
                      {event.description ? (
                        <div className="connected-work-body">
                          <span>{zh ? "说明" : "Description"}</span>
                          <p>{event.description}</p>
                        </div>
                      ) : null}
                      {event.notify_attendees ? (
                        <p className="connected-work-warning">
                          {zh
                            ? "执行后将由日历服务通知参与人。"
                            : "The calendar service will notify attendees after execution."}
                        </p>
                      ) : null}
                    </>
                  ) : (
                    <p className="connected-work-warning">
                      {zh
                        ? "执行后将取消这条日程。"
                        : "Execution will cancel this calendar event."}
                    </p>
                  )}
                </div>
              )}

              <div className="connected-work-actions">
                <span>
                  <ShieldCheck size={15} aria-hidden="true" />
                  {waitingForExactApproval
                    ? zh
                      ? "只批准当前显示的收件人、正文或日程内容"
                      : "Approves only the recipients, message, or event shown above"
                    : zh
                      ? "这一步只冻结内容，不会修改外部账户"
                      : "This step only freezes content; it does not change the external account"}
                </span>
                <div>
                  {waitingForExactApproval ? (
                    <button
                      className="primary-action"
                      type="button"
                      disabled={pendingId !== null}
                      onClick={() => void approveAndRun(review)}
                    >
                      {busy
                        ? zh
                          ? "正在安全执行…"
                          : "Running safely…"
                        : zh
                          ? "确认并执行"
                          : "Confirm and run"}
                    </button>
                  ) : (
                    <button
                      className="primary-action"
                      type="button"
                      disabled={pendingId !== null}
                      onClick={() => void requestApproval(review)}
                    >
                      {busy
                        ? zh
                          ? "正在冻结…"
                          : "Freezing…"
                        : zh
                          ? "内容无误，进入最终批准"
                          : "Content is correct; continue"}
                    </button>
                  )}
                  <button
                    type="button"
                    disabled={pendingId !== null}
                    onClick={() => void reject(review)}
                  >
                    {zh ? "拒绝" : "Reject"}
                  </button>
                </div>
              </div>
            </article>
          );
        })}
      </div>
      <div className="connected-work-feedback" aria-live="polite">
        {error ? <p className="package-error" role="alert">{error}</p> : null}
        {notice ? <p>{notice}</p> : null}
      </div>
    </section>
  );
}
