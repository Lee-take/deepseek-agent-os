import { invoke } from "@tauri-apps/api/core";
import { Brain, Database, FolderOpen, Languages, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import type { ChangeEvent } from "react";
import { translations } from "./i18n";
import type { AccessMode, FoundationState, Language, ModelRoute, ThinkingLevel } from "./types";

const fallbackState: FoundationState = {
  app_name: "DeepSeek Agent OS",
  model_route: "auto",
  thinking_level: "auto",
  access_mode: "ask_on_risk",
  workspace_scope: "workspace",
};

const LANGUAGE_STORAGE_KEY = "deepseek-agent-os:ui-language:v1";

function readInitialLanguage(): Language {
  if (typeof window === "undefined") {
    return "zh";
  }

  const storedLanguage = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
  return storedLanguage === "en" ? "en" : "zh";
}

export function App() {
  const [state, setState] = useState<FoundationState>(fallbackState);
  const [language, setLanguage] = useState<Language>(readInitialLanguage);
  const copy = translations[language];

  useEffect(() => {
    void invoke<FoundationState>("get_foundation_state")
      .then(setState)
      .catch(() => setState(fallbackState));
  }, []);

  useEffect(() => {
    document.documentElement.lang = language === "zh" ? "zh-CN" : "en";
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, language);
  }, [language]);

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

  const switchLanguage = (nextLanguage: Language) => {
    setLanguage(nextLanguage);
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
        </header>

        <section className="workbench">
          <div className="timeline">
            <p className="eyebrow">{copy.workbench.stage}</p>
            <h1>{copy.workbench.title}</h1>
            <p className="summary">{copy.workbench.summary}</p>
          </div>
          <aside className="inspector">
            <div className="inspector-header">
              <Brain size={18} />
              <strong>{copy.inspector.title}</strong>
            </div>
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
            </dl>
          </aside>
        </section>
      </section>
    </main>
  );
}
