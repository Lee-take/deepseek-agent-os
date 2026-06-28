import { invoke } from "@tauri-apps/api/core";
import { Brain, Database, FolderOpen, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import type { ChangeEvent } from "react";
import type { AccessMode, FoundationState, ModelRoute, ThinkingLevel } from "./types";

const fallbackState: FoundationState = {
  app_name: "DeepSeek Agent OS",
  model_route: "auto",
  thinking_level: "auto",
  access_mode: "ask_on_risk",
  workspace_scope: "workspace",
};

export function App() {
  const [state, setState] = useState<FoundationState>(fallbackState);

  useEffect(() => {
    void invoke<FoundationState>("get_foundation_state")
      .then(setState)
      .catch(() => setState(fallbackState));
  }, []);

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

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">D</div>
          <div>
            <strong>{state.app_name}</strong>
            <span>Local-first Agent OS</span>
          </div>
        </div>
        <nav className="nav-list" aria-label="Primary">
          <button className="nav-item active" type="button">
            <FolderOpen size={18} /> Workbench
          </button>
          <button className="nav-item" type="button">
            <Database size={18} /> Memory
          </button>
          <button className="nav-item" type="button">
            <ShieldCheck size={18} /> Approvals
          </button>
        </nav>
      </aside>

      <section className="workspace">
        <header className="toolbar">
          <select value={state.model_route} aria-label="Model route" onChange={updateModelRoute}>
            <option value="auto">DeepSeek Auto</option>
            <option value="flash">DeepSeek Flash</option>
            <option value="pro">DeepSeek Pro</option>
          </select>
          <select value={state.access_mode} aria-label="Access mode" onChange={updateAccessMode}>
            <option value="ask_every_step">Every step asks</option>
            <option value="ask_on_risk">Ask on risk</option>
            <option value="limited_auto">Limited auto</option>
            <option value="full_access">Full access</option>
          </select>
          <select value={state.thinking_level} aria-label="Thinking level" onChange={updateThinkingLevel}>
            <option value="auto">Thinking auto</option>
            <option value="fast">Fast</option>
            <option value="standard">Standard</option>
            <option value="deep">Deep</option>
          </select>
        </header>

        <section className="workbench">
          <div className="timeline">
            <p className="eyebrow">Foundation MVP</p>
            <h1>Operations Briefing Workbench</h1>
            <p className="summary">
              The first runnable slice proves the desktop shell, policy controls,
              DeepSeek routing defaults, and local kernel boundary.
            </p>
          </div>
          <aside className="inspector">
            <div className="inspector-header">
              <Brain size={18} />
              <strong>Runtime Controls</strong>
            </div>
            <dl>
              <div>
                <dt>Model</dt>
                <dd>{state.model_route}</dd>
              </div>
              <div>
                <dt>Access</dt>
                <dd>{state.access_mode}</dd>
              </div>
              <div>
                <dt>Thinking</dt>
                <dd>{state.thinking_level}</dd>
              </div>
              <div>
                <dt>Scope</dt>
                <dd>{state.workspace_scope}</dd>
              </div>
            </dl>
          </aside>
        </section>
      </section>
    </main>
  );
}
