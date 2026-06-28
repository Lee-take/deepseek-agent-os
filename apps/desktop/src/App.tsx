import { invoke } from "@tauri-apps/api/core";
import {
  ArchiveRestore,
  Brain,
  Clipboard,
  Database,
  FolderOpen,
  Globe2,
  Languages,
  Mail,
  MonitorCog,
  PackageOpen,
  Plus,
  Search,
  ShieldCheck,
} from "lucide-react";
import { useEffect, useState } from "react";
import type { ChangeEvent, FormEvent } from "react";
import { translations } from "./i18n";
import type {
  AccessMode,
  CapabilityKind,
  FoundationState,
  Language,
  MemoryRecord,
  ModelRoute,
  PermissionAuditEntry,
  TaskRecord,
  ThemeStyle,
  ThinkingLevel,
  WorkPackage,
  WorkPackageImportSummary,
} from "./types";

const fallbackState: FoundationState = {
  app_name: "DeepSeek Agent OS",
  model_route: "auto",
  thinking_level: "auto",
  access_mode: "ask_on_risk",
  workspace_scope: "workspace",
};

const LANGUAGE_STORAGE_KEY = "deepseek-agent-os:ui-language:v1";
const THEME_STORAGE_KEY = "deepseek-agent-os:theme-style:v1";

function readInitialLanguage(): Language {
  if (typeof window === "undefined") {
    return "zh";
  }

  const storedLanguage = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
  return storedLanguage === "en" ? "en" : "zh";
}

function readInitialThemeStyle(): ThemeStyle {
  if (typeof window === "undefined") {
    return "deep";
  }

  const storedTheme = window.localStorage.getItem(THEME_STORAGE_KEY);
  if (storedTheme === "ink" || storedTheme === "porcelain") {
    return storedTheme;
  }
  return "deep";
}

function formatTaskDate(value: string, language: Language) {
  return new Intl.DateTimeFormat(language === "zh" ? "zh-CN" : "en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

export function App() {
  const [state, setState] = useState<FoundationState>(fallbackState);
  const [language, setLanguage] = useState<Language>(readInitialLanguage);
  const [themeStyle, setThemeStyle] = useState<ThemeStyle>(readInitialThemeStyle);
  const [taskRecords, setTaskRecords] = useState<TaskRecord[]>([]);
  const [memoryRecords, setMemoryRecords] = useState<MemoryRecord[]>([]);
  const [permissionAudits, setPermissionAudits] = useState<PermissionAuditEntry[]>([]);
  const [memoryQuery, setMemoryQuery] = useState("");
  const [taskTitle, setTaskTitle] = useState("");
  const [taskSummary, setTaskSummary] = useState("");
  const [exportedPackageJson, setExportedPackageJson] = useState("");
  const [importPackageJson, setImportPackageJson] = useState("");
  const [packageNotice, setPackageNotice] = useState("");
  const [packageError, setPackageError] = useState("");
  const [memoryError, setMemoryError] = useState("");
  const [auditError, setAuditError] = useState("");
  const [packagePending, setPackagePending] = useState(false);
  const [memoryPending, setMemoryPending] = useState(false);
  const [auditPending, setAuditPending] = useState<CapabilityKind | null>(null);
  const copy = translations[language];

  useEffect(() => {
    void invoke<FoundationState>("get_foundation_state")
      .then(setState)
      .catch(() => setState(fallbackState));
  }, []);

  useEffect(() => {
    void Promise.all([
      invoke<TaskRecord[]>("list_task_records"),
      invoke<MemoryRecord[]>("list_memory_records"),
      invoke<PermissionAuditEntry[]>("list_permission_audit_entries"),
    ])
      .then(([records, memories, audits]) => {
        setTaskRecords(records);
        setMemoryRecords(memories);
        setPermissionAudits(audits);
      })
      .catch(() => {
        setPackageError(copy.package.loadFailed);
        setMemoryError(copy.memory.loadFailed);
        setAuditError(copy.audit.loadFailed);
      });
  }, [copy.audit.loadFailed, copy.memory.loadFailed, copy.package.loadFailed]);

  useEffect(() => {
    document.documentElement.lang = language === "zh" ? "zh-CN" : "en";
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, language);
  }, [language]);

  useEffect(() => {
    document.documentElement.dataset.theme = themeStyle;
    window.localStorage.setItem(THEME_STORAGE_KEY, themeStyle);
  }, [themeStyle]);

  const updateModelRoute = (event: ChangeEvent<HTMLSelectElement>) => {
    setState((currentState) => ({
      ...currentState,
      model_route: event.target.value as ModelRoute,
    }));
  };

  const updateAccessMode = (event: ChangeEvent<HTMLSelectElement>) => {
    setState((currentState) => ({
      ...currentState,
      access_mode: event.target.value as AccessMode,
    }));
  };

  const updateThinkingLevel = (event: ChangeEvent<HTMLSelectElement>) => {
    setState((currentState) => ({
      ...currentState,
      thinking_level: event.target.value as ThinkingLevel,
    }));
  };

  const updateThemeStyle = (event: ChangeEvent<HTMLSelectElement>) => {
    setThemeStyle(event.target.value as ThemeStyle);
  };

  const switchLanguage = (nextLanguage: Language) => {
    setLanguage(nextLanguage);
  };

  const loadMemoryRecords = async (query: string) => {
    const trimmedQuery = query.trim();
    if (!trimmedQuery) {
      return invoke<MemoryRecord[]>("list_memory_records");
    }

    return invoke<MemoryRecord[]>("search_memory_records", {
      query: trimmedQuery,
    });
  };

  const searchMemoryRecords = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setMemoryPending(true);
    setMemoryError("");

    try {
      const memories = await loadMemoryRecords(memoryQuery);
      setMemoryRecords(memories);
    } catch {
      setMemoryError(copy.memory.loadFailed);
    } finally {
      setMemoryPending(false);
    }
  };

  const recordPermissionAudit = async (capability: CapabilityKind) => {
    setAuditPending(capability);
    setAuditError("");

    try {
      const entry = await invoke<PermissionAuditEntry>("record_permission_audit", {
        accessMode: state.access_mode,
        capability,
      });
      setPermissionAudits((currentAudits) => [entry, ...currentAudits].slice(0, 100));
    } catch (error) {
      setAuditError(String(error) || copy.audit.loadFailed);
    } finally {
      setAuditPending(null);
    }
  };

  const clearPackageStatus = () => {
    setPackageNotice("");
    setPackageError("");
  };

  const createTaskRecord = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    clearPackageStatus();

    if (!taskTitle.trim()) {
      setPackageError(copy.package.emptyTitle);
      return;
    }

    setPackagePending(true);
    try {
      const record = await invoke<TaskRecord>("create_task_record", {
        title: taskTitle,
        summary: taskSummary,
      });
      const memories = await loadMemoryRecords(memoryQuery);
      setTaskRecords((currentRecords) => [record, ...currentRecords]);
      setMemoryRecords(memories);
      setTaskTitle("");
      setTaskSummary("");
      setPackageNotice(copy.package.created);
    } catch (error) {
      setPackageError(String(error));
    } finally {
      setPackagePending(false);
    }
  };

  const exportCurrentWorkPackage = async () => {
    clearPackageStatus();
    setPackagePending(true);
    try {
      const workPackage = await invoke<WorkPackage>("export_work_package");
      setExportedPackageJson(JSON.stringify(workPackage, null, 2));
      setPackageNotice(copy.package.exported);
    } catch (error) {
      setPackageError(String(error));
    } finally {
      setPackagePending(false);
    }
  };

  const copyCurrentWorkPackage = async () => {
    clearPackageStatus();
    setPackagePending(true);
    try {
      let packageJson = exportedPackageJson;
      if (!packageJson) {
        const workPackage = await invoke<WorkPackage>("export_work_package");
        packageJson = JSON.stringify(workPackage, null, 2);
        setExportedPackageJson(packageJson);
      }
      await navigator.clipboard.writeText(packageJson);
      setPackageNotice(copy.package.copied);
    } catch {
      setPackageError(copy.package.copyFailed);
    } finally {
      setPackagePending(false);
    }
  };

  const importWorkPackageJson = async () => {
    clearPackageStatus();

    if (!importPackageJson.trim()) {
      setPackageError(copy.package.emptyImport);
      return;
    }

    setPackagePending(true);
    try {
      const summary = await invoke<WorkPackageImportSummary>("import_work_package", {
        packageJson: importPackageJson,
      });
      const [records, memories] = await Promise.all([
        invoke<TaskRecord[]>("list_task_records"),
        loadMemoryRecords(memoryQuery),
      ]);
      setTaskRecords(records);
      setMemoryRecords(memories);
      setImportPackageJson("");
      setPackageNotice(copy.package.imported(summary.imported, summary.skipped));
    } catch (error) {
      setPackageError(String(error));
    } finally {
      setPackagePending(false);
    }
  };

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">D</div>
          <div>
            <strong>{state.app_name}</strong>
            <span>{copy.brandTagline}</span>
          </div>
        </div>
        <div className="sidebar-preferences">
          <div className="language-switch" role="group" aria-label={copy.controls.language}>
            <Languages size={16} aria-hidden="true" />
            <button
              className={language === "zh" ? "language-option active" : "language-option"}
              type="button"
              aria-pressed={language === "zh"}
              onClick={() => switchLanguage("zh")}
            >
              中
            </button>
            <button
              className={language === "en" ? "language-option active" : "language-option"}
              type="button"
              aria-pressed={language === "en"}
              onClick={() => switchLanguage("en")}
            >
              EN
            </button>
          </div>
        </div>
        <nav className="nav-list" aria-label={copy.navLabel}>
          <button className="nav-item active" type="button">
            <FolderOpen size={18} /> {copy.nav.workbench}
          </button>
          <button className="nav-item" type="button">
            <Database size={18} /> {copy.nav.memory}
          </button>
          <button className="nav-item" type="button">
            <ShieldCheck size={18} /> {copy.nav.approvals}
          </button>
        </nav>
      </aside>

      <section className="workspace">
        <header className="toolbar">
          <select value={state.model_route} aria-label={copy.controls.modelRoute} onChange={updateModelRoute}>
            <option value="auto">{copy.modelOptions.auto}</option>
            <option value="flash">{copy.modelOptions.flash}</option>
            <option value="pro">{copy.modelOptions.pro}</option>
          </select>
          <select value={state.access_mode} aria-label={copy.controls.accessMode} onChange={updateAccessMode}>
            <option value="ask_every_step">{copy.accessOptions.ask_every_step}</option>
            <option value="ask_on_risk">{copy.accessOptions.ask_on_risk}</option>
            <option value="limited_auto">{copy.accessOptions.limited_auto}</option>
            <option value="full_access">{copy.accessOptions.full_access}</option>
          </select>
          <select value={state.thinking_level} aria-label={copy.controls.thinkingLevel} onChange={updateThinkingLevel}>
            <option value="auto">{copy.thinkingOptions.auto}</option>
            <option value="fast">{copy.thinkingOptions.fast}</option>
            <option value="standard">{copy.thinkingOptions.standard}</option>
            <option value="deep">{copy.thinkingOptions.deep}</option>
          </select>
          <select value={themeStyle} aria-label={copy.controls.themeStyle} onChange={updateThemeStyle}>
            <option value="deep">{copy.themeOptions.deep}</option>
            <option value="ink">{copy.themeOptions.ink}</option>
            <option value="porcelain">{copy.themeOptions.porcelain}</option>
          </select>
        </header>

        <section className="workbench">
          <div className="timeline">
            <p className="eyebrow">{copy.workbench.stage}</p>
            <h1>{copy.workbench.title}</h1>
            <p className="summary">{copy.workbench.summary}</p>

            <section className="package-panel" aria-labelledby="work-package-title">
              <div className="section-heading">
                <PackageOpen size={18} aria-hidden="true" />
                <h2 id="work-package-title">{copy.package.title}</h2>
              </div>

              <form className="task-form" onSubmit={createTaskRecord}>
                <input
                  value={taskTitle}
                  aria-label={copy.package.taskTitle}
                  placeholder={copy.package.taskTitle}
                  onChange={(event) => setTaskTitle(event.target.value)}
                />
                <textarea
                  value={taskSummary}
                  aria-label={copy.package.taskSummary}
                  placeholder={copy.package.taskSummary}
                  rows={3}
                  onChange={(event) => setTaskSummary(event.target.value)}
                />
                <button className="primary-action" type="submit" disabled={packagePending}>
                  <Plus size={16} aria-hidden="true" />
                  {copy.package.addRecord}
                </button>
              </form>

              <section className="memory-panel inline" aria-labelledby="memory-panel-title">
                <div className="inspector-header compact">
                  <Database size={18} aria-hidden="true" />
                  <strong id="memory-panel-title">{copy.memory.title}</strong>
                </div>
                <form className="memory-search" onSubmit={searchMemoryRecords}>
                  <input
                    value={memoryQuery}
                    aria-label={copy.memory.searchPlaceholder}
                    placeholder={copy.memory.searchPlaceholder}
                    onChange={(event) => setMemoryQuery(event.target.value)}
                  />
                  <button type="submit" disabled={memoryPending}>
                    <Search size={15} aria-hidden="true" />
                    {copy.memory.search}
                  </button>
                </form>
                {memoryError ? <p className="package-error">{memoryError}</p> : null}
                {memoryRecords.length === 0 ? (
                  <p className="empty-state">{copy.memory.noMemories}</p>
                ) : (
                  <div className="memory-list">
                    {memoryRecords.slice(0, 3).map((memory) => (
                      <article className="memory-row" key={memory.id}>
                        <strong>{memory.title}</strong>
                        <p>{memory.body}</p>
                        <span>
                          {copy.memory.autoCapture} · {formatTaskDate(memory.created_at, language)}
                        </span>
                      </article>
                    ))}
                  </div>
                )}
              </section>

              <div className="task-list" aria-live="polite">
                {taskRecords.length === 0 ? (
                  <p className="empty-state">{copy.package.noRecords}</p>
                ) : (
                  taskRecords.map((record) => (
                    <article className="task-row" key={record.id}>
                      <div>
                        <strong>{record.title}</strong>
                        {record.summary ? <p>{record.summary}</p> : null}
                      </div>
                      <time dateTime={record.created_at}>{formatTaskDate(record.created_at, language)}</time>
                    </article>
                  ))
                )}
              </div>

              <div className="package-actions">
                <button type="button" onClick={exportCurrentWorkPackage} disabled={packagePending}>
                  <PackageOpen size={16} aria-hidden="true" />
                  {copy.package.exportPackage}
                </button>
                <button type="button" onClick={copyCurrentWorkPackage} disabled={packagePending}>
                  <Clipboard size={16} aria-hidden="true" />
                  {copy.package.copyPackage}
                </button>
              </div>

              <textarea
                className="package-json"
                value={exportedPackageJson}
                aria-label={copy.package.packageJson}
                placeholder={copy.package.packageJson}
                rows={5}
                readOnly
              />

              <div className="import-row">
                <textarea
                  value={importPackageJson}
                  aria-label={copy.package.importJson}
                  placeholder={copy.package.importJson}
                  rows={4}
                  onChange={(event) => setImportPackageJson(event.target.value)}
                />
                <button type="button" onClick={importWorkPackageJson} disabled={packagePending}>
                  <ArchiveRestore size={16} aria-hidden="true" />
                  {copy.package.importPackage}
                </button>
              </div>

              {packageNotice ? <p className="package-message">{packageNotice}</p> : null}
              {packageError ? <p className="package-error">{packageError}</p> : null}
            </section>
          </div>
          <aside className="inspector">
            <div className="inspector-header">
              <Brain size={18} />
              <strong>{copy.inspector.title}</strong>
            </div>
            <section className="audit-panel" aria-labelledby="audit-panel-title">
              <div className="inspector-header compact">
                <ShieldCheck size={18} aria-hidden="true" />
                <strong id="audit-panel-title">{copy.audit.title}</strong>
              </div>
              <div className="audit-actions">
                <button
                  type="button"
                  onClick={() => void recordPermissionAudit("browser_browse")}
                  disabled={auditPending !== null}
                >
                  <Globe2 size={15} aria-hidden="true" />
                  {auditPending === "browser_browse" ? copy.audit.pending : copy.audit.browser}
                </button>
                <button
                  type="button"
                  onClick={() => void recordPermissionAudit("email_send")}
                  disabled={auditPending !== null}
                >
                  <Mail size={15} aria-hidden="true" />
                  {auditPending === "email_send" ? copy.audit.pending : copy.audit.emailSend}
                </button>
                <button
                  type="button"
                  onClick={() => void recordPermissionAudit("computer_control")}
                  disabled={auditPending !== null}
                >
                  <MonitorCog size={15} aria-hidden="true" />
                  {auditPending === "computer_control" ? copy.audit.pending : copy.audit.computerControl}
                </button>
              </div>
              {auditError ? <p className="package-error">{auditError}</p> : null}
              {permissionAudits.length === 0 ? (
                <p className="empty-state">{copy.audit.empty}</p>
              ) : (
                <div className="audit-list">
                  {permissionAudits.slice(0, 4).map((entry) => (
                    <article className="audit-row" key={entry.id}>
                      <strong>{copy.capabilityOptions[entry.capability]}</strong>
                      <span className={`decision ${entry.decision}`}>{copy.decisionOptions[entry.decision]}</span>
                      <p>
                        {copy.riskOptions[entry.risk_level]} · {copy.accessOptions[entry.access_mode]} ·{" "}
                        {formatTaskDate(entry.created_at, language)}
                      </p>
                    </article>
                  ))}
                </div>
              )}
            </section>
            <dl>
              <div>
                <dt>{copy.inspector.model}</dt>
                <dd>{copy.modelOptions[state.model_route]}</dd>
              </div>
              <div>
                <dt>{copy.inspector.access}</dt>
                <dd>{copy.accessOptions[state.access_mode]}</dd>
              </div>
              <div>
                <dt>{copy.inspector.thinking}</dt>
                <dd>{copy.thinkingOptions[state.thinking_level]}</dd>
              </div>
              <div>
                <dt>{copy.inspector.scope}</dt>
                <dd>{copy.scopeOptions[state.workspace_scope]}</dd>
              </div>
              <div>
                <dt>{copy.inspector.theme}</dt>
                <dd>{copy.themeOptions[themeStyle]}</dd>
              </div>
            </dl>
          </aside>
        </section>
      </section>
    </main>
  );
}
