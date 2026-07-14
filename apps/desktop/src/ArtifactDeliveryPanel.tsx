import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type { ArtifactDelivery, Language } from "./types";

const copy = {
  zh: {
    title: "办公文件交付", empty: "还没有正在处理的办公文件。",
    unavailable: "暂时无法读取文件交付状态。", structure: "结构检查", visual: "实际渲染",
    passed: "已通过", rendered: "已成功渲染，可查看", waiting: "等待中", revised: "已自动修订",
    preview: "查看实际渲染预览", pages: "已渲染页数", previous: "上一页", next: "下一页",
    previewUnavailable: "预览暂时无法显示；交付状态仍可查看。",
    status: {
      generated_check_pending: "文件已生成，等待结构检查",
      structure_passed_visual_pending: "结构检查已通过，等待实际渲染",
      checks_passed_delivery_pending: "实际渲染已完成，可查看后交付",
      revision_in_progress: "检查发现问题，正在进行有限修订",
      completed: "文件已完成实际渲染，可查看后交付",
      needs_attention: "文件需要处理后才能交付",
    },
    format: { word: "Word", excel: "Excel", power_point: "PowerPoint", pdf: "PDF" },
  },
  en: {
    title: "Office deliveries", empty: "No office files are being processed yet.",
    unavailable: "File delivery status is temporarily unavailable.", structure: "Structure check", visual: "Actual render",
    passed: "Passed", rendered: "Rendered successfully; available to review", waiting: "Waiting", revised: "Automatic revisions",
    preview: "Review actual render", pages: "Rendered pages", previous: "Previous", next: "Next",
    previewUnavailable: "The preview is temporarily unavailable; delivery status remains available.",
    status: {
      generated_check_pending: "Generated and waiting for structure checks",
      structure_passed_visual_pending: "Structure passed; waiting for actual rendering",
      checks_passed_delivery_pending: "Actual render completed and available for review",
      revision_in_progress: "A bounded automatic revision is in progress",
      completed: "Actual render completed and available for review",
      needs_attention: "Needs attention before delivery",
    },
    format: { word: "Word", excel: "Excel", power_point: "PowerPoint", pdf: "PDF" },
  },
} satisfies Record<Language, {
  title: string; empty: string; unavailable: string; structure: string; visual: string;
  passed: string; rendered: string; waiting: string; revised: string; preview: string; pages: string;
  previous: string; next: string; previewUnavailable: string;
  status: Record<ArtifactDelivery["status_code"], string>;
  format: Record<ArtifactDelivery["format"], string>;
}>;

type Preview = { data: string; page: number; updatedAt: string };

export function ArtifactDeliveryPanel({ language }: { language: Language }) {
  const text = copy[language];
  const [items, setItems] = useState<ArtifactDelivery[]>([]);
  const [error, setError] = useState(false);
  const [previews, setPreviews] = useState<Record<string, Preview>>({});
  const [previewErrors, setPreviewErrors] = useState<Record<string, boolean>>({});

  const loadPreview = useCallback(async (item: ArtifactDelivery, page: number) => {
    try {
      const data = await invoke<string>("get_artifact_visual_preview", { artifactId: item.id, pageIndex: page });
      setPreviews((current) => ({ ...current, [item.id]: { data, page, updatedAt: item.updated_at } }));
      setPreviewErrors((current) => ({ ...current, [item.id]: false }));
    } catch {
      setPreviewErrors((current) => ({ ...current, [item.id]: true }));
    }
  }, []);

  const refresh = useCallback(async () => {
    try {
      const nextItems = await invoke<ArtifactDelivery[]>("list_artifact_deliveries");
      setItems(nextItems);
      setPreviews((current) => Object.fromEntries(Object.entries(current).filter(([id, preview]) =>
        nextItems.some((item) => item.id === id && item.updated_at === preview.updatedAt))));
      setError(false);
    } catch {
      setError(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 5000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  return (
    <section className="artifact-delivery-panel" aria-labelledby="artifact-delivery-title">
      <h3 id="artifact-delivery-title">{text.title}</h3>
      {error ? <p className="recovery-error">{text.unavailable}</p> : null}
      {!error && items.length === 0 ? <p className="empty-state">{text.empty}</p> : null}
      {items.map((item) => {
        const preview = previews[item.id];
        return (
          <article className="artifact-delivery-card" key={item.id}>
            <div className="artifact-delivery-heading"><strong>{text.format[item.format]}</strong><span>{text.status[item.status_code]}</span></div>
            <div className="artifact-delivery-checks">
              <span>{text.structure}: {item.structure_checked ? text.passed : text.waiting}</span>
              <span>{text.visual}: {item.visual_checked ? text.rendered : text.waiting}</span>
              {item.revision_attempts > 0 ? <span>{text.revised}: {item.revision_attempts}</span> : null}
              {item.preview_available ? <span>{text.pages}: {item.rendered_page_count}</span> : null}
            </div>
            {item.preview_available && !preview ? <button type="button" className="secondary-button" onClick={() => void loadPreview(item, 0)}>{text.preview}</button> : null}
            {preview ? <>
              <img className="artifact-render-preview" src={preview.data} alt={`${text.format[item.format]} ${text.visual}`} />
              <div className="artifact-preview-pagination">
                <button type="button" disabled={preview.page === 0} onClick={() => void loadPreview(item, preview.page - 1)}>{text.previous}</button>
                <span>{preview.page + 1} / {item.rendered_page_count}</span>
                <button type="button" disabled={preview.page + 1 >= item.rendered_page_count} onClick={() => void loadPreview(item, preview.page + 1)}>{text.next}</button>
              </div>
            </> : null}
            {previewErrors[item.id] ? <p className="recovery-error">{text.previewUnavailable}</p> : null}
          </article>
        );
      })}
    </section>
  );
}
