import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Archive,
  ArchiveRestore,
  Brain,
  Check,
  CircleStop,
  Clipboard,
  ClipboardList,
  Cloud,
  Database,
  Download,
  FileText,
  FolderOpen,
  Globe2,
  Link2,
  Mail,
  MonitorCog,
  MousePointerClick,
  Network,
  PackageOpen,
  Paperclip,
  Pencil,
  Pin,
  Play,
  Plus,
  Power,
  RefreshCw,
  Search,
  Send,
  ShieldCheck,
  TerminalSquare,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { ChangeEvent, FormEvent, MouseEvent } from "react";
import {
  derivePersistedConversationTitle,
  summarizeConversationTitleFromText,
} from "./conversationTitle";
import {
  agentChatComposerAction,
  agentChatLoopSteps,
  buildAgentGuidancePrompt,
  createAgentChatRun,
  finishAgentRun,
  hasOpenAgentRunRecords,
  idleDeepSeekStepState,
  latestParentAgentRunStatus,
  queueAgentRunGuidance,
  requestAgentRunCancel,
  shouldRunDurableAgentWorker,
  shouldShowAgentStopControl,
  userFacingAgentReplyContent,
} from "./agentChatRunState";
import type { AgentChatGuidanceStatus, AgentChatRun } from "./agentChatRunState";
import {
  agentChatPendingStageDelaysMs,
  agentChatPendingStageIndex,
} from "./agentChatPending";
import { summarizeAgentContextReceipt } from "./agentContextReceipt";
import { AutomationCenter } from "./AutomationCenter";
import { ConnectedWorkReviewPanel } from "./ConnectedWorkReviewPanel";
import {
  conversationAttachmentMetadata,
  formatAgentPromptWithAttachments,
  isAgentAttachmentDropInsideComposer,
  prepareAgentAttachmentPaths,
  readyAgentAttachments,
  summarizeAttachmentsForDisplay,
} from "./agentAttachments";
import type { AgentAttachment } from "./agentAttachments";
import {
  settingsPanelItems,
  shouldExposePluginsSidebarEntry,
} from "./settingsPanel";
import { translations } from "./i18n";
import {
  blockedParentsWithTerminalChildren,
  expertBudgetLabel,
  expertTeamCanSynthesize,
  readyExpertAttempts,
} from "./expert-team";
import type {
  AccessMode,
  AgentChatActionProposal,
  AgentChatMissingPrerequisite,
  AgentChatResponse,
  AgentContextReceipt,
  AgentRunRecord,
  AgentRunWorkerResult,
  AgentSoulProfileUpdateReceipt,
  AgentToolContract,
  AgentSoulProfileState,
  AppUpdateDownloadResult,
  AppUpdateInstallResult,
  AppUpdateStatus,
  CapabilityAccessRecord,
  CapabilityDescriptor,
  CapabilityFamily,
  CapabilityInvocation,
  CapabilityKind,
  ComputerControlUnlockStatus,
  ComputerUseRunResult,
  ComputerUseSessionStartResult,
  ComputerUseSessionView,
  ComputerUseStepView,
  DeepSeekChatCacheState,
  DeepSeekChatTelemetry,
  DeepSeekPricingState,
  ComputerUseBackendStatus,
  FoundationState,
  GoalEnvelopeProposal,
  GoalEnvelopeUiProjection,
  LargeModelProvider,
  Language,
  OnboardingReadinessProjection,
  MemoryBackgroundMaintenanceSummary,
  MemoryCandidate,
  MemoryCandidateRecord,
  MemoryLifecycle,
  MemoryMaintenanceReviewItem,
  MemoryMaintenanceReviewKind,
  MemoryRecord,
  MemoryRecordDeletion,
  MemorySearchMatch,
  MemoryRecordUpdate,
  MemoryRelationKind,
  MemorySelectedFeedback,
  MemorySelectedFeedbackKind,
  MemoryScope,
  MemorySensitivity,
  MemoryType,
  ModelRoute,
  ModelDrivenToolStrategy,
  NetworkSearchSourceModel,
  NetworkSearchRouteStatus,
  OperationsBriefingRun,
  PermissionAuditEntry,
  SkillRecord,
  SkillUpdateSweepResult,
  TaskRecord,
  TaskGroupedAuthorizationIntent,
  TaskGroupedAuthorizationView,
  TerminalReadCommand,
  ThemeStyle,
  ThinkingLevel,
  ToolInvocationRecord,
  WorkPackage,
  WorkPackageImportPreview,
  WorkPackageImportSummary,
} from "./types";

function invokeAgentTool(
  toolId: string,
  input: Record<string, unknown>,
  accessMode: AccessMode,
) {
  return invoke<ToolInvocationRecord>("execute_agent_tool", {
    request: {
      tool_id: toolId,
      input,
      access_mode: accessMode,
      run_id: null,
    },
  });
}

function upsertToolInvocation(
  invocations: ToolInvocationRecord[],
  invocation: ToolInvocationRecord,
) {
  return [invocation, ...invocations.filter((current) => current.id !== invocation.id)];
}

const fallbackState: FoundationState = {
  app_name: "DS Agent",
  large_model_provider: "deepseek",
  model_route: "auto",
  thinking_level: "auto",
  access_mode: "full_access",
  workspace_scope: "workspace",
  network_search_source_model: null,
  tool_backends: {
    network_search: "source_backed_model",
    email: "architecture_only",
    drive: "local_folder_export_package",
    computer_screenshot: "local_windows_screen_capture",
    computer_control: "local_windows_input_control",
  },
};

const fallbackOnboardingReadiness: OnboardingReadinessProjection = {
  schema_version: 1,
  overall: "setup_required",
  next_step: "deepseek_key",
  deepseek: {
    source: "missing",
    configured: false,
    verification: "not_checked",
    code: "key_missing",
    chat_completion_ready: false,
    balance_available: null,
    flash_model: "deepseek-v4-flash",
    pro_model: "deepseek-v4-pro",
    flash_available: null,
    pro_available: null,
    retryable: false,
    last_verified_at: null,
    message_key: "onboarding.deepseek.key_missing",
  },
  workspace: {
    configured: false,
    workspace_name: null,
    workspace_root_display: null,
    root_exists: false,
    managed_directories_ready: false,
    writable: null,
    code: "workspace_missing",
    retryable: true,
    message_key: "onboarding.workspace.workspace_missing",
  },
  version: {
    current_version: "1.4.0",
    status: "current",
    blocking: false,
    message_key: "onboarding.version.current",
  },
  checked_at: null,
};

const fallbackDeepSeekChatCacheState: DeepSeekChatCacheState = {
  entries: 0,
};

const defaultMemorySearchMatch: MemorySearchMatch = {
  source: "direct",
  linked_memory_id: null,
  relation: null,
};

const fallbackNetworkSearchRouteStatus: NetworkSearchRouteStatus = {
  backend: "source_backed_model",
  execution_mode: "permission_audit_only",
  evidence_policy: "pending_user_confirmation",
  network_requests_enabled: false,
  deepseek_orchestration_ready: false,
  requires_user_confirmation: true,
  note:
    "The selected model route needs a separate source-linked web-search option before search can run.",
};

const fallbackComputerUseBackendStatus: ComputerUseBackendStatus = {
  screenshot_backend: "local_windows_screen_capture",
  screenshot_available: true,
  screenshot_note: "Screen inspection uses the local Windows screen route.",
  screenshot_permission_required: false,
  screenshot_permission_note:
    "Local Windows desktop capture usually runs without a separate OS permission prompt, but secure desktops and protected windows can block pixels.",
  control_backend: "local_windows_input_control",
  control_available: true,
  control_requires_approval: true,
  control_note: "Mouse and keyboard control uses the local Windows input route.",
  control_permission_required: false,
  control_permission_note:
    "Local Windows input control runs against the foreground desktop and can be blocked by secure desktop prompts or elevated target windows.",
  codex_bridge: {
    required: false,
    transport_env_var: "DEEPSEEK_AGENT_OS_BRIDGE_TRANSPORT",
    transport: null,
    transport_decision_required: false,
    transport_options: [
      {
        value: "http",
        label: "Local HTTP bridge service",
        note:
          "Use a user-started local HTTP service with health, screenshot, control, and web-search endpoints.",
      },
    ],
    endpoint_env_var: "DEEPSEEK_AGENT_OS_BRIDGE_URL",
    endpoint_configured: false,
    connected: false,
    note:
      "Selected local Computer Use route does not need the local bridge service.",
  },
};

const fallbackComputerControlUnlockStatus: ComputerControlUnlockStatus = {
  challenge: "",
  unlocked: false,
  unlocked_until: null,
};

const fallbackModelDrivenToolStrategy: ModelDrivenToolStrategy = {
  large_model_provider: "deepseek",
  large_model_supports_network_search: false,
  network_search_source_model_required: true,
  network_search_source_model: null,
  free_network_search_source_model_options: [
    {
      value: "free_web_source",
      label: "Free web source model",
      note: "Use a free source-linked web-search option for evidence and citations.",
    },
    {
      value: "free_local_browser",
      label: "Free local browser search (alpha)",
      note:
        "Alpha preset: currently uses the same local search implementation; reserved for local browser/search-page retrieval.",
    },
    {
      value: "free_source_aggregator",
      label: "Free source aggregator (alpha)",
      note:
        "Alpha preset: currently uses the same local search implementation; reserved for pluggable source aggregation.",
    },
  ],
  network_search_backend: "source_backed_model",
  computer_screenshot_backend: "local_windows_screen_capture",
  computer_control_backend: "local_windows_input_control",
  runtime_platform: "windows",
  macos_supported: true,
  note:
    "Selected model route needs a separate source-linked web-search option before search can run.",
};

const fallbackAgentSoulProfileState: AgentSoulProfileState = {
  exists: false,
  content: "",
  summary_lines: [],
  used_bytes: 0,
  max_bytes: 800,
};

const fallbackAppUpdateStatus: AppUpdateStatus = {
  current_version: "0.1.1",
  latest_version: null,
  update_available: false,
  asset_name: null,
  release_url: null,
  message: null,
};

const fallbackDeepSeekPricingState: DeepSeekPricingState = {
  pricing_configured: false,
  note: "DeepSeek cost estimates are disabled until a local pricing table is configured",
  settings: {
    enabled: false,
    flash_prompt_usd_per_million_tokens: "",
    flash_completion_usd_per_million_tokens: "",
    pro_prompt_usd_per_million_tokens: "",
    pro_completion_usd_per_million_tokens: "",
  },
};

const LANGUAGE_STORAGE_KEY = "deepseek-agent-os:ui-language:v1";
const THEME_STORAGE_KEY = "deepseek-agent-os:theme-style:v1";
const AGENT_CONVERSATIONS_STORAGE_KEY = "deepseek-agent-os:agent-conversations:v1";
const AGENT_CONTEXT_COMPRESSION_SOFT_LIMIT_TOKENS = 96_000;
const AGENT_CONTEXT_RECENT_MESSAGE_COUNT = 10;
const AGENT_SOUL_BOOTSTRAP_CONTEXT_MAX_CHARS = 16_384;

const memoryTypeValues: MemoryType[] = [
  "preference",
  "project_context",
  "workflow_rule",
  "artifact",
  "failure_pattern",
];

const memoryScopeValues: MemoryScope[] = ["workspace", "project", "organization", "user"];

const memorySensitivityValues: MemorySensitivity[] = ["normal", "sensitive"];

const memoryLifecycleValues: MemoryLifecycle[] = ["active", "archived", "expires"];

type MemoryFeedbackReviewFilter =
  | "all"
  | "needs_review"
  | MemorySelectedFeedbackKind;

type MemoryFeedbackReviewSort = "priority" | "latest" | "feedback_count";

type MemoryMaintenanceFilter =
  | "all"
  | "needs_review"
  | "snoozed"
  | MemoryMaintenanceReviewKind;

type MemoryMaintenanceSort = "priority" | "latest" | "feedback_count";

const memoryFeedbackReviewFilterValues: MemoryFeedbackReviewFilter[] = [
  "all",
  "needs_review",
  "useful",
  "irrelevant",
  "stale",
  "conflicting",
  "should_update",
];

const memoryReviewSortValues: MemoryFeedbackReviewSort[] = [
  "priority",
  "latest",
  "feedback_count",
];

const memoryMaintenanceFilterValues: MemoryMaintenanceFilter[] = [
  "all",
  "needs_review",
  "retrieval",
  "update_archive",
  "conflict",
  "snoozed",
];

const memoryMaintenanceSortValues: MemoryMaintenanceSort[] = [
  "priority",
  "latest",
  "feedback_count",
];

type MemoryEditDraft = {
  id: string;
  title: string;
  body: string;
  memory_type: MemoryType;
  scope: MemoryScope;
  sensitivity: MemorySensitivity;
  lifecycle: MemoryLifecycle;
  expires_at: string;
};

type WorkflowStepState = "done" | "current" | "waiting" | "needs_action" | "blocked";
type WorkflowStatusTone = "ready" | "running" | "needs_action" | "done" | "blocked";
type AgentChatSetupPrompt = "deepseek_key" | "workspace" | "doctor" | "network_search";
type QueuedAgentPrompt = {
  prompt: string;
  displayPrompt: string;
  attachments: AgentAttachment[];
  runId: string | null;
};

type AgentConversationMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  attachments?: AgentAttachment[];
  model?: string;
  protocol_version?: string;
  proposed_actions?: AgentChatActionProposal[];
  missing_prerequisites?: AgentChatMissingPrerequisite[];
  memory_candidates?: MemoryCandidate[];
  soul_profile_update?: AgentSoulProfileUpdateReceipt | null;
  goal_envelope?: GoalEnvelopeProposal | null;
  goal_projection?: GoalEnvelopeUiProjection | null;
  run_error?: string;
  created_at: string;
};

type AgentConversationSession = {
  id: string;
  title: string;
  messages: AgentConversationMessage[];
  soul_profile_bootstrap: string | null;
  updated_at: string;
  context_state: "normal" | "compressed";
  pinned: boolean;
  archived: boolean;
  manual_title: boolean;
};

type AgentConversationMenuState = {
  conversationId: string;
  x: number;
  y: number;
};

type WorkflowStep = {
  key: string;
  label: string;
  detail: string;
  state: WorkflowStepState;
};

function createClientId(prefix: string): string {
  return `${prefix}-${globalThis.crypto?.randomUUID?.() ?? Date.now().toString(36)}`;
}

function normalizeAgentSoulBootstrap(content: string | null | undefined): string {
  return (content ?? "").trim().slice(0, AGENT_SOUL_BOOTSTRAP_CONTEXT_MAX_CHARS);
}

function agentSoulProfileBootstrapFromState(profileState: AgentSoulProfileState): string {
  return profileState.exists ? normalizeAgentSoulBootstrap(profileState.content) : "";
}

function createEmptyAgentConversation(soulProfileBootstrap = ""): AgentConversationSession {
  const now = new Date().toISOString();
  const normalizedSoulProfileBootstrap = normalizeAgentSoulBootstrap(soulProfileBootstrap);
  return {
    id: createClientId("conversation"),
    title: "",
    messages: [],
    soul_profile_bootstrap: normalizedSoulProfileBootstrap || null,
    updated_at: now,
    context_state: "normal",
    pinned: false,
    archived: false,
    manual_title: false,
  };
}

function deriveConversationTitle(messages: AgentConversationMessage[], fallback: string): string {
  const firstUserMessage =
    messages.find((message) => message.role === "user")?.content.trim() || fallback.trim();
  return summarizeConversationTitleFromText(firstUserMessage);
}

function sortAgentConversations(
  conversations: AgentConversationSession[],
): AgentConversationSession[] {
  return [...conversations].sort((left, right) => {
    if (left.pinned !== right.pinned) {
      return left.pinned ? -1 : 1;
    }
    return new Date(right.updated_at).getTime() - new Date(left.updated_at).getTime();
  });
}

function estimateConversationTokens(messages: AgentConversationMessage[]): number {
  const charCount = messages.reduce((total, message) => total + message.content.length, 0);
  return Math.ceil(charCount / 3);
}

function buildAgentSoulBootstrapContextSection(soulProfileBootstrap: string | null): string {
  const normalizedSoulProfileBootstrap = normalizeAgentSoulBootstrap(soulProfileBootstrap);
  if (!normalizedSoulProfileBootstrap) {
    return "";
  }

  return [
    "DS Agent conversation Soul startup context from memory/soul.md.",
    "This hidden context was loaded when this conversation started. Treat it as long-term identity and collaboration memory, not as the current user request.",
    "Keep this Soul context outside older-turn compression so compression does not erase it.",
    normalizedSoulProfileBootstrap,
  ].join("\n");
}

function buildAgentConversationContextPrompt(
  prompt: string,
  messages: AgentConversationMessage[],
  soulProfileBootstrap: string | null = null,
): { prompt: string; compressed: boolean } {
  const soulBootstrapSection = buildAgentSoulBootstrapContextSection(soulProfileBootstrap);
  if (messages.length === 0) {
    if (soulBootstrapSection) {
      return {
        prompt: [soulBootstrapSection, "Current user message:", prompt].join("\n\n"),
        compressed: false,
      };
    }
    return { prompt, compressed: false };
  }

  const estimatedTokens = estimateConversationTokens(messages);
  const shouldCompress = estimatedTokens > AGENT_CONTEXT_COMPRESSION_SOFT_LIMIT_TOKENS;
  const recentCount = shouldCompress ? AGENT_CONTEXT_RECENT_MESSAGE_COUNT : 16;
  const recentMessages = messages.slice(-recentCount);
  const olderMessages = messages.slice(0, Math.max(0, messages.length - recentCount));
  const compactOlderContext = olderMessages.slice(-24).map((message, index) => {
    const excerpt = message.content.replace(/\s+/g, " ").trim().slice(0, 220);
    return `${index + 1}. ${message.role}: ${excerpt}`;
  });
  const recentContext = recentMessages.map((message, index) => {
    return `${index + 1}. ${message.role}: ${message.content.trim()}`;
  });
  const contextSections = [
    "DS Agent conversation context. Use this as prior context, but answer the current user message directly.",
    `Estimated prior context tokens: ${estimatedTokens}.`,
  ];

  if (soulBootstrapSection && shouldCompress) {
    contextSections.push(soulBootstrapSection);
  }

  if (compactOlderContext.length > 0) {
    contextSections.push(
      shouldCompress
        ? "Older turns were automatically compacted because the conversation is approaching the DeepSeek context budget:"
        : "Older turns:",
      compactOlderContext.join("\n"),
    );
  }

  contextSections.push("Recent turns:", recentContext.join("\n"));
  contextSections.push("Current user message:", prompt);

  return {
    prompt: contextSections.join("\n\n"),
    compressed: shouldCompress,
  };
}

function latestAssistantMessage(
  messages: AgentConversationMessage[],
): AgentConversationMessage | undefined {
  return [...messages].reverse().find((message) => message.role === "assistant");
}

function messageHasAgentEnvelope(message: AgentConversationMessage | undefined): boolean {
  return (
    ((message?.proposed_actions?.length ?? 0) > 0) ||
    ((message?.missing_prerequisites?.length ?? 0) > 0)
  );
}

function userFacingAgentRunError(
  message: AgentConversationMessage | undefined,
  fallback: {
    requestFailed: string;
    responseReadFailed: string;
  },
): string {
  const explicitRunError = message?.run_error?.trim();
  if (explicitRunError) {
    return explicitRunError;
  }

  const content = message?.content?.trim() ?? "";
  if (
    content.includes("deepseek chat response could not be read") ||
    content.includes("error decoding response body")
  ) {
    return fallback.responseReadFailed;
  }

  return "";
}

function userFacingAgentMessageContent(
  message: AgentConversationMessage,
  fallback: {
    responseReadFailed: string;
  },
): string {
  if (
    message.role === "assistant" &&
    (message.content.includes("deepseek chat response could not be read") ||
      message.content.includes("error decoding response body"))
  ) {
    return fallback.responseReadFailed;
  }

  return message.role === "assistant"
    ? userFacingAgentReplyContent(message.content)
    : message.content;
}

function goalEnvelopeStatusLabel(
  status: GoalEnvelopeUiProjection["status"],
  language: Language,
): string {
  const labels = {
    proposed: language === "zh" ? "待本地校验" : "Proposed",
    blocked: language === "zh" ? "校验受阻" : "Validation blocked",
    validated: language === "zh" ? "本地校验通过" : "Validated locally",
    frozen: language === "zh" ? "目标已冻结" : "Goal frozen",
    verification_blocked: language === "zh" ? "完成验证受阻" : "Verification blocked",
    complete: language === "zh" ? "证据验证完成" : "Verified complete",
  } satisfies Record<GoalEnvelopeUiProjection["status"], string>;
  return labels[status];
}

function goalEnvelopeStatusTone(status: GoalEnvelopeUiProjection["status"]): string {
  if (status === "complete") {
    return "succeeded";
  }
  if (status === "blocked" || status === "verification_blocked") {
    return "blocked";
  }
  return "proposed";
}

function userFacingAgentActionDetail(action: AgentChatActionProposal): string {
  const dispatchNote = action.dispatch_note?.trim();
  if (action.action_type === "browser_open" && action.execution_state === "succeeded" && dispatchNote) {
    return dispatchNote;
  }

  const blockedReason = action.blocked_reason?.trim();
  if (blockedReason) {
    return blockedReason;
  }

  const target = action.target?.trim();
  const destination = action.destination?.trim();
  if (target && destination) {
    return `${target} -> ${destination}`;
  }

  const reason = action.reason?.trim();
  if (reason) {
    return reason;
  }

  if (target) {
    return target;
  }

  if (action.execution_state !== "succeeded") {
    return dispatchNote || "";
  }

  return "";
}

function shouldShowAgentActionInChat(action: AgentChatActionProposal): boolean {
  return (
    action.execution_state === "needs_confirmation" ||
    action.execution_state === "blocked" ||
    action.execution_state === "failed"
  );
}

function shouldShowWorkflowStepDetail(step: WorkflowStep): boolean {
  return step.detail.trim().length > 0 && step.state === "blocked";
}

function readInitialAgentConversations(): AgentConversationSession[] {
  if (typeof window === "undefined") {
    return [createEmptyAgentConversation()];
  }

  try {
    const stored = window.localStorage.getItem(AGENT_CONVERSATIONS_STORAGE_KEY);
    const parsed = stored ? JSON.parse(stored) : null;
    if (!Array.isArray(parsed)) {
      return [createEmptyAgentConversation()];
    }
    const sessions = parsed
      .filter((session): session is AgentConversationSession => {
        return (
          typeof session?.id === "string" &&
          Array.isArray(session.messages) &&
          typeof session.updated_at === "string"
        );
      })
      .map((session) => {
        const manualTitle = session.manual_title === true;
        const storedTitle = typeof session.title === "string" ? session.title : "";
        const firstUserMessage =
          session.messages.find((message) => message.role === "user")?.content.trim() || "";

        return {
          id: session.id,
          title: derivePersistedConversationTitle({
            firstUserMessage,
            manualTitle,
            storedTitle,
          }),
          messages: session.messages,
          soul_profile_bootstrap:
            normalizeAgentSoulBootstrap(
              typeof session.soul_profile_bootstrap === "string"
                ? session.soul_profile_bootstrap
                : "",
            ) || null,
          updated_at: session.updated_at,
          context_state:
            session.context_state === "compressed" ? ("compressed" as const) : ("normal" as const),
          pinned: session.pinned === true,
          archived: session.archived === true,
          manual_title: manualTitle,
        };
      });
    if (!sessions.length) {
      return [createEmptyAgentConversation()];
    }
    if (sessions.every((session) => session.archived)) {
      return [createEmptyAgentConversation(), ...sortAgentConversations(sessions)];
    }
    return sortAgentConversations(sessions);
  } catch {
    return [createEmptyAgentConversation()];
  }
}

function hasDesktopRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function prerequisitesNeedNetworkSearchSetup(
  prerequisites: AgentChatMissingPrerequisite[],
): boolean {
  return prerequisites.some((prerequisite) => {
    const kind = prerequisite.kind.trim().toLowerCase();
    return kind === "network_search" || kind === "search" || kind === "web_search";
  });
}

function prerequisitesNeedWorkspaceSetup(
  prerequisites: AgentChatMissingPrerequisite[],
): boolean {
  return prerequisites.some((prerequisite) => {
    const kind = prerequisite.kind.trim().toLowerCase();
    return kind === "workspace" || kind === "work_root" || kind === "local_workspace";
  });
}

function isoToDateInputValue(value: string | null): string {
  return value ? value.slice(0, 10) : "";
}

function dateInputValueToIso(
  value: string,
  lifecycle: MemoryLifecycle,
): string | null {
  if (lifecycle !== "expires" || !value) {
    return null;
  }

  return `${value}T23:59:59.000Z`;
}

function readInitialLanguage(): Language {
  if (typeof window === "undefined") {
    return "zh";
  }

  const storedLanguage = window.localStorage.getItem(LANGUAGE_STORAGE_KEY);
  return storedLanguage === "en" ? "en" : "zh";
}

function readInitialThemeStyle(): ThemeStyle {
  if (typeof window === "undefined") {
    return "porcelain";
  }

  const storedTheme = window.localStorage.getItem(THEME_STORAGE_KEY);
  if (storedTheme === "ink" || storedTheme === "porcelain") {
    return storedTheme;
  }
  return "porcelain";
}

function formatTaskDate(value: string, language: Language) {
  return new Intl.DateTimeFormat(language === "zh" ? "zh-CN" : "en-US", {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function formatMicroUsd(value: number) {
  const dollars = value / 1_000_000;
  const formatted = dollars.toFixed(6).replace(/\.?0+$/, "");
  return `$${formatted || "0"}`;
}

function agentChatRunFromRecord(record: AgentRunRecord, displayPrompt?: string): AgentChatRun {
  return {
    id: record.id,
    conversation_id: record.conversation_id,
    prompt: displayPrompt ?? record.prompt,
    status: record.status,
    cancel_requested: record.cancel_requested,
    queued_guidance: record.queued_guidance
      .filter((guidance) => guidance.applied_at === null)
      .map((guidance) => ({
        id: guidance.id,
        content: guidance.guidance,
        attachment_count: 0,
        created_at: guidance.queued_at,
      })),
    created_at: record.started_at,
    updated_at: record.updated_at,
  };
}

function taskGroupedAuthorizationStatusLabel(
  status: TaskGroupedAuthorizationView["status"],
  language: Language,
) {
  const labels = language === "zh"
    ? {
        pending: "等待确认",
        approved: "已授权",
        rejected: "已拒绝",
        revoked: "已撤销",
        expired: "已过期",
        scope_changed: "范围已变化",
      }
    : {
        pending: "Pending",
        approved: "Approved",
        rejected: "Rejected",
        revoked: "Revoked",
        expired: "Expired",
        scope_changed: "Scope changed",
      };
  return labels[status];
}

function TaskGroupedAuthorizationCard({
  authorization,
  language,
  capabilityLabels,
  riskLabels,
  pending,
  onResolve,
  onRevoke,
}: {
  authorization: TaskGroupedAuthorizationView;
  language: Language;
  capabilityLabels: Record<CapabilityKind, string>;
  riskLabels: Record<TaskGroupedAuthorizationView["risk_level"], string>;
  pending: boolean;
  onResolve: (intent: TaskGroupedAuthorizationIntent, approved: boolean) => void;
  onRevoke: (intent: TaskGroupedAuthorizationIntent) => void;
}) {
  const field = (values: string[]) => values.join(language === "zh" ? "、" : ", ") || "—";
  const labels = language === "zh"
    ? {
        title: "本次任务授权",
        applications: "应用",
        paths: "路径",
        accounts: "账户",
        recipients: "收件人",
        timeWindows: "时间范围",
        externalTargets: "外部目标",
        expiry: "有效期",
        verifiers: "验证方式",
        audits: "逐能力审计",
        approve: "确认授权",
        reject: "拒绝",
        revoke: "撤销授权",
        updating: "正在核对…",
      }
    : {
        title: "Task authorization",
        applications: "Applications",
        paths: "Paths",
        accounts: "Accounts",
        recipients: "Recipients",
        timeWindows: "Time range",
        externalTargets: "External targets",
        expiry: "Expires",
        verifiers: "Verification",
        audits: "Capability audit",
        approve: "Approve task",
        reject: "Reject",
        revoke: "Revoke authorization",
        updating: "Checking…",
      };

  return (
    <article
      className="capability-card"
      data-task-grouped-authorization={authorization.status}
    >
      <div className="capability-card-header">
        <ShieldCheck size={16} aria-hidden="true" />
        <div>
          <strong>{labels.title}</strong>
          <span>{taskGroupedAuthorizationStatusLabel(authorization.status, language)}</span>
        </div>
      </div>
      <p>{authorization.goal}</p>
      <div className="capability-meta">
        <span className={`risk ${authorization.risk_level}`}>
          {riskLabels[authorization.risk_level]}
        </span>
        <span className={`access-status ${authorization.status}`}>
          {taskGroupedAuthorizationStatusLabel(authorization.status, language)}
        </span>
      </div>
      <div className="agent-action-list">
        <ul>
          <li><span>{labels.applications}</span><p>{field(authorization.applications)}</p></li>
          <li><span>{labels.paths}</span><p>{field(authorization.paths)}</p></li>
          <li><span>{labels.accounts}</span><p>{field(authorization.accounts)}</p></li>
          <li><span>{labels.recipients}</span><p>{field(authorization.recipients)}</p></li>
          <li><span>{labels.timeWindows}</span><p>{field(authorization.time_windows)}</p></li>
          <li><span>{labels.externalTargets}</span><p>{field(authorization.external_targets)}</p></li>
          <li><span>{labels.expiry}</span><p>{formatTaskDate(authorization.expires_at, language)}</p></li>
          <li><span>{labels.verifiers}</span><p>{field(authorization.verifiers)}</p></li>
        </ul>
      </div>
      <div className="tool-output">
        <div className="recent-audit-heading">{labels.audits}</div>
        <div className="tool-output-list">
          {authorization.capability_audits.map((audit, index) => (
            <article className="tool-output-row" key={`${audit.capability}-${index}`}>
              <div>
                <strong>{capabilityLabels[audit.capability]}</strong>
                <span className={`risk ${audit.risk_level}`}>{riskLabels[audit.risk_level]}</span>
              </div>
              <span className={`access-status ${audit.status}`}>
                {taskGroupedAuthorizationStatusLabel(audit.status, language)}
              </span>
            </article>
          ))}
        </div>
      </div>
      {authorization.status === "pending" ? (
        <div className="capability-card-approval">
          <button type="button" disabled={pending} onClick={() => onResolve(authorization.intent, true)}>
            <Check size={14} aria-hidden="true" />
            {pending ? labels.updating : labels.approve}
          </button>
          <button type="button" disabled={pending} onClick={() => onResolve(authorization.intent, false)}>
            <X size={14} aria-hidden="true" />
            {labels.reject}
          </button>
        </div>
      ) : authorization.status === "approved" ? (
        <div className="capability-card-approval">
          <button type="button" disabled={pending} onClick={() => onRevoke(authorization.intent)}>
            <X size={14} aria-hidden="true" />
            {pending ? labels.updating : labels.revoke}
          </button>
        </div>
      ) : null}
    </article>
  );
}

function capabilityFamilyIcon(family: CapabilityFamily) {
  switch (family) {
    case "file":
      return FileText;
    case "network":
      return Network;
    case "browser":
      return Globe2;
    case "email":
      return Mail;
    case "drive":
      return Cloud;
    case "terminal":
      return TerminalSquare;
    case "computer_use":
      return MonitorCog;
    case "app_update":
      return PackageOpen;
    case "skill":
      return PackageOpen;
  }
}

export function App() {
  const [state, setState] = useState<FoundationState>(fallbackState);
  const [onboardingReadiness, setOnboardingReadiness] =
    useState<OnboardingReadinessProjection>(fallbackOnboardingReadiness);
  const deepSeekCredentialStatus = onboardingReadiness.deepseek;
  const workspaceReadiness = onboardingReadiness.workspace;
  const localDirectoryNeedsSetup = workspaceReadiness.code !== "ready";
  const [deepSeekChatCacheState, setDeepSeekChatCacheState] =
    useState<DeepSeekChatCacheState>(fallbackDeepSeekChatCacheState);
  const [deepSeekTelemetry, setDeepSeekTelemetry] = useState<DeepSeekChatTelemetry[]>([]);
  const [networkSearchRouteStatus, setNetworkSearchRouteStatus] =
    useState<NetworkSearchRouteStatus>(fallbackNetworkSearchRouteStatus);
  const [computerUseBackendStatus, setComputerUseBackendStatus] =
    useState<ComputerUseBackendStatus>(fallbackComputerUseBackendStatus);
  const [computerControlUnlockStatus, setComputerControlUnlockStatus] =
    useState<ComputerControlUnlockStatus>(fallbackComputerControlUnlockStatus);
  const [computerUseSessions, setComputerUseSessions] = useState<ComputerUseSessionView[]>([]);
  const [computerUseSteps, setComputerUseSteps] = useState<ComputerUseStepView[]>([]);
  const [modelToolStrategy, setModelToolStrategy] =
    useState<ModelDrivenToolStrategy>(fallbackModelDrivenToolStrategy);
  const [soulProfileState, setSoulProfileState] =
    useState<AgentSoulProfileState>(fallbackAgentSoulProfileState);
  const [appUpdateStatus, setAppUpdateStatus] =
    useState<AppUpdateStatus>(fallbackAppUpdateStatus);
  const [deepSeekPricingState, setDeepSeekPricingState] =
    useState<DeepSeekPricingState>(fallbackDeepSeekPricingState);
  const [language, setLanguage] = useState<Language>(readInitialLanguage);
  const [themeStyle, setThemeStyle] = useState<ThemeStyle>(readInitialThemeStyle);
  const workbenchSectionRef = useRef<HTMLElement | null>(null);
  const memorySectionRef = useRef<HTMLElement | null>(null);
  const chatThreadRef = useRef<HTMLDivElement | null>(null);
  const chatComposerRef = useRef<HTMLFormElement | null>(null);
  const [taskRecords, setTaskRecords] = useState<TaskRecord[]>([]);
  const [memoryRecords, setMemoryRecords] = useState<MemoryRecord[]>([]);
  const [memoryCandidateRecords, setMemoryCandidateRecords] = useState<MemoryCandidateRecord[]>([]);
  const [selectedMemoryFeedbackRecords, setSelectedMemoryFeedbackRecords] = useState<
    MemorySelectedFeedback[]
  >([]);
  const [memoryMaintenanceReviews, setMemoryMaintenanceReviews] = useState<
    MemoryMaintenanceReviewItem[]
  >([]);
  const [permissionAudits, setPermissionAudits] = useState<PermissionAuditEntry[]>([]);
  const [capabilityCatalog, setCapabilityCatalog] = useState<CapabilityDescriptor[]>([]);
  const [capabilityRecords, setCapabilityRecords] = useState<CapabilityAccessRecord[]>([]);
  const [capabilityInvocations, setCapabilityInvocations] = useState<CapabilityInvocation[]>([]);
  const [taskGroupedAuthorizations, setTaskGroupedAuthorizations] = useState<
    TaskGroupedAuthorizationView[]
  >([]);
  const [taskGroupedAuthorizationPending, setTaskGroupedAuthorizationPending] = useState<
    string | null
  >(null);
  const [taskGroupedAuthorizationNotice, setTaskGroupedAuthorizationNotice] = useState("");
  const [taskGroupedAuthorizationError, setTaskGroupedAuthorizationError] = useState("");
  const [agentToolContracts, setAgentToolContracts] = useState<AgentToolContract[]>([]);
  const [toolInvocations, setToolInvocations] = useState<ToolInvocationRecord[]>([]);
  const [agentContextReceipts, setAgentContextReceipts] = useState<AgentContextReceipt[]>([]);
  const [skillRecords, setSkillRecords] = useState<SkillRecord[]>([]);
  const [skillUpdateSweep, setSkillUpdateSweep] = useState<SkillUpdateSweepResult | null>(null);
  const [skillUpdatePending, setSkillUpdatePending] = useState(false);
  const [skillActionPending, setSkillActionPending] = useState<string | null>(null);
  const [skillNotice, setSkillNotice] = useState("");
  const [skillError, setSkillError] = useState("");
  const [agentRunRecords, setAgentRunRecords] = useState<AgentRunRecord[]>([]);
  const [operationsBriefingRuns, setOperationsBriefingRuns] = useState<OperationsBriefingRun[]>([]);
  const [memoryQuery, setMemoryQuery] = useState("");
  const [memoryFeedbackFilter, setMemoryFeedbackFilter] =
    useState<MemoryFeedbackReviewFilter>("needs_review");
  const [memoryFeedbackSort, setMemoryFeedbackSort] =
    useState<MemoryFeedbackReviewSort>("priority");
  const [memoryMaintenanceFilter, setMemoryMaintenanceFilter] =
    useState<MemoryMaintenanceFilter>("needs_review");
  const [memoryMaintenanceSort, setMemoryMaintenanceSort] =
    useState<MemoryMaintenanceSort>("priority");
  const [candidateTitle, setCandidateTitle] = useState("");
  const [candidateBody, setCandidateBody] = useState("");
  const [candidateMemoryType, setCandidateMemoryType] = useState<MemoryType>("preference");
  const [candidateMemoryScope, setCandidateMemoryScope] = useState<MemoryScope>("workspace");
  const [candidateSensitivity, setCandidateSensitivity] =
    useState<MemorySensitivity>("normal");
  const [candidateLifecycle, setCandidateLifecycle] = useState<MemoryLifecycle>("active");
  const [candidateExpiresAt, setCandidateExpiresAt] = useState("");
  const [memoryLinkSourceId, setMemoryLinkSourceId] = useState("");
  const [memoryLinkTargetId, setMemoryLinkTargetId] = useState("");
  const [memoryExistingLinkRelation, setMemoryExistingLinkRelation] =
    useState<MemoryRelationKind>("related");
  const [memoryExistingLinkNote, setMemoryExistingLinkNote] = useState("");
  const [memoryExistingLinkPending, setMemoryExistingLinkPending] = useState(false);
  const [memoryEditDraft, setMemoryEditDraft] = useState<MemoryEditDraft | null>(null);
  const [browserUrl, setBrowserUrl] = useState("");
  const [browserSubmitUrl, setBrowserSubmitUrl] = useState("");
  const [browserSubmitSummary, setBrowserSubmitSummary] = useState("");
  const [networkSearchQuery, setNetworkSearchQuery] = useState("");
  const [networkSearchScope, setNetworkSearchScope] = useState("");
  const [filePath, setFilePath] = useState("");
  const [fileWritePath, setFileWritePath] = useState("");
  const [fileWriteSummary, setFileWriteSummary] = useState("");
  const [fileWriteContent, setFileWriteContent] = useState("");
  const [folderPath, setFolderPath] = useState("");
  const [briefingFolderPath, setBriefingFolderPath] = useState("");
  const [terminalCommand, setTerminalCommand] =
    useState<TerminalReadCommand>("git status --short");
  const [terminalWriteCommand, setTerminalWriteCommand] = useState("");
  const [emailMailbox, setEmailMailbox] = useState("");
  const [emailReadQuery, setEmailReadQuery] = useState("");
  const [driveLocation, setDriveLocation] = useState("");
  const [driveReadQuery, setDriveReadQuery] = useState("");
  const [driveWriteLocation, setDriveWriteLocation] = useState("");
  const [driveWriteSummary, setDriveWriteSummary] = useState("");
  const [draftEmailTo, setDraftEmailTo] = useState("");
  const [draftEmailSubject, setDraftEmailSubject] = useState("");
  const [draftEmailBody, setDraftEmailBody] = useState("");
  const [emailTo, setEmailTo] = useState("");
  const [emailSubject, setEmailSubject] = useState("");
  const [emailBody, setEmailBody] = useState("");
  const [computerControlTarget, setComputerControlTarget] = useState("");
  const [computerControlAction, setComputerControlAction] = useState("");
  const [computerControlUnlockToken, setComputerControlUnlockToken] = useState("");
  const [computerUseActionDraft, setComputerUseActionDraft] = useState("type:DS Agent verified");
  const [lastComputerScreenshotInvocationId, setLastComputerScreenshotInvocationId] =
    useState<string | null>(null);
  const [setupWorkspaceName, setSetupWorkspaceName] = useState("");
  const [setupWorkspaceDir, setSetupWorkspaceDir] = useState("");
  const [soulProfileDraft, setSoulProfileDraft] = useState("");
  const [deepSeekPricingEnabled, setDeepSeekPricingEnabled] = useState(false);
  const [deepSeekFlashPromptPrice, setDeepSeekFlashPromptPrice] = useState("");
  const [deepSeekFlashCompletionPrice, setDeepSeekFlashCompletionPrice] = useState("");
  const [deepSeekProPromptPrice, setDeepSeekProPromptPrice] = useState("");
  const [deepSeekProCompletionPrice, setDeepSeekProCompletionPrice] = useState("");
  const [taskTitle, setTaskTitle] = useState("");
  const [taskSummary, setTaskSummary] = useState("");
  const [agentPrompt, setAgentPrompt] = useState("");
  const [agentAttachments, setAgentAttachments] = useState<AgentAttachment[]>([]);
  const [agentAttachmentError, setAgentAttachmentError] = useState("");
  const [agentAttachmentDragActive, setAgentAttachmentDragActive] = useState(false);
  const [agentConversations, setAgentConversations] = useState<AgentConversationSession[]>(
    readInitialAgentConversations,
  );
  const [activeAgentConversationId, setActiveAgentConversationId] = useState(
    () =>
      agentConversations.find((conversation) => !conversation.archived)?.id ??
      agentConversations[0]?.id ??
      createEmptyAgentConversation().id,
  );
  const [agentMessages, setAgentMessages] = useState<AgentConversationMessage[]>(
    () =>
      agentConversations.find((conversation) => !conversation.archived)?.messages ??
      agentConversations[0]?.messages ??
      [],
  );
  const [conversationMenu, setConversationMenu] =
    useState<AgentConversationMenuState | null>(null);
  const [renamingConversationId, setRenamingConversationId] = useState<string | null>(null);
  const [renameConversationTitle, setRenameConversationTitle] = useState("");
  const [agentChatPending, setAgentChatPending] = useState(false);
  const [activeAgentRun, setActiveAgentRun] = useState<AgentChatRun | null>(null);
  const [agentChatPendingStage, setAgentChatPendingStage] = useState(0);
  const [agentGuidanceStatus, setAgentGuidanceStatus] =
    useState<AgentChatGuidanceStatus>("idle");
  const [queuedAgentGuidance, setQueuedAgentGuidance] = useState("");
  const [agentActionPending, setAgentActionPending] = useState<string | null>(null);
  const [agentChatError, setAgentChatError] = useState("");
  const [agentChatNotice, setAgentChatNotice] = useState("");
  const [agentSetupPrompt, setAgentSetupPrompt] = useState<AgentChatSetupPrompt | null>(null);
  const [pendingAgentPrompt, setPendingAgentPrompt] = useState("");
  const [deepSeekApiKeyDraft, setDeepSeekApiKeyDraft] = useState("");
  const [exportedPackageJson, setExportedPackageJson] = useState("");
  const [importPackageJson, setImportPackageJson] = useState("");
  const [importPreview, setImportPreview] = useState<WorkPackageImportPreview | null>(null);
  const [soulProfileModalOpen, setSoulProfileModalOpen] = useState(false);
  const [packageNotice, setPackageNotice] = useState("");
  const [packageError, setPackageError] = useState("");
  const [memoryNotice, setMemoryNotice] = useState("");
  const [memoryError, setMemoryError] = useState("");
  const [memoryCandidateNotice, setMemoryCandidateNotice] = useState("");
  const [memoryCandidateError, setMemoryCandidateError] = useState("");
  const [auditError, setAuditError] = useState("");
  const [capabilityError, setCapabilityError] = useState("");
  const [browserNotice, setBrowserNotice] = useState("");
  const [browserError, setBrowserError] = useState("");
  const [browserSubmitNotice, setBrowserSubmitNotice] = useState("");
  const [browserSubmitError, setBrowserSubmitError] = useState("");
  const [networkSearchNotice, setNetworkSearchNotice] = useState("");
  const [networkSearchError, setNetworkSearchError] = useState("");
  const [fileNotice, setFileNotice] = useState("");
  const [fileError, setFileError] = useState("");
  const [fileWriteNotice, setFileWriteNotice] = useState("");
  const [fileWriteError, setFileWriteError] = useState("");
  const [folderNotice, setFolderNotice] = useState("");
  const [folderError, setFolderError] = useState("");
  const [terminalNotice, setTerminalNotice] = useState("");
  const [terminalError, setTerminalError] = useState("");
  const [terminalWriteNotice, setTerminalWriteNotice] = useState("");
  const [terminalWriteError, setTerminalWriteError] = useState("");
  const [emailReadNotice, setEmailReadNotice] = useState("");
  const [emailReadError, setEmailReadError] = useState("");
  const [driveReadNotice, setDriveReadNotice] = useState("");
  const [driveReadError, setDriveReadError] = useState("");
  const [driveWriteNotice, setDriveWriteNotice] = useState("");
  const [driveWriteError, setDriveWriteError] = useState("");
  const [emailDraftNotice, setEmailDraftNotice] = useState("");
  const [emailDraftError, setEmailDraftError] = useState("");
  const [emailNotice, setEmailNotice] = useState("");
  const [emailError, setEmailError] = useState("");
  const [computerNotice, setComputerNotice] = useState("");
  const [computerError, setComputerError] = useState("");
  const [computerControlNotice, setComputerControlNotice] = useState("");
  const [computerControlError, setComputerControlError] = useState("");
  const [computerControlUnlockNotice, setComputerControlUnlockNotice] = useState("");
  const [computerControlUnlockError, setComputerControlUnlockError] = useState("");
  const [computerUseStepNotice, setComputerUseStepNotice] = useState("");
  const [computerUseStepError, setComputerUseStepError] = useState("");
  const [briefingNotice, setBriefingNotice] = useState("");
  const [briefingError, setBriefingError] = useState("");
  const [memoryFeedbackNotice, setMemoryFeedbackNotice] = useState("");
  const [memoryFeedbackError, setMemoryFeedbackError] = useState("");
  const [setupNotice, setSetupNotice] = useState("");
  const [setupError, setSetupError] = useState("");
  const [soulProfileNotice, setSoulProfileNotice] = useState("");
  const [soulProfileError, setSoulProfileError] = useState("");
  const [appUpdateNotice, setAppUpdateNotice] = useState("");
  const [appUpdateError, setAppUpdateError] = useState("");
  const [downloadedAppUpdate, setDownloadedAppUpdate] =
    useState<AppUpdateDownloadResult | null>(null);
  const appUpdateDownloadKeyRef = useRef<string | null>(null);
  const [deepSeekCacheNotice, setDeepSeekCacheNotice] = useState("");
  const [deepSeekCacheError, setDeepSeekCacheError] = useState("");
  const [deepSeekPricingNotice, setDeepSeekPricingNotice] = useState("");
  const [deepSeekPricingError, setDeepSeekPricingError] = useState("");
  const [deepSeekReadinessNotice, setDeepSeekReadinessNotice] = useState("");
  const [deepSeekReadinessError, setDeepSeekReadinessError] = useState("");
  const [packagePending, setPackagePending] = useState(false);
  const [memoryPending, setMemoryPending] = useState(false);
  const [memoryCandidatePending, setMemoryCandidatePending] = useState(false);
  const [memoryFeedbackPending, setMemoryFeedbackPending] = useState<string | null>(null);
  const [memoryUpdatePending, setMemoryUpdatePending] = useState<string | null>(null);
  const [memoryDeletionPending, setMemoryDeletionPending] = useState<string | null>(null);
  const [browserPending, setBrowserPending] = useState(false);
  const [browserSubmitPending, setBrowserSubmitPending] = useState(false);
  const [networkSearchPending, setNetworkSearchPending] = useState(false);
  const [filePending, setFilePending] = useState(false);
  const [fileWritePending, setFileWritePending] = useState(false);
  const [folderPending, setFolderPending] = useState(false);
  const [terminalPending, setTerminalPending] = useState(false);
  const [terminalWritePending, setTerminalWritePending] = useState(false);
  const [emailReadPending, setEmailReadPending] = useState(false);
  const [driveReadPending, setDriveReadPending] = useState(false);
  const [driveWritePending, setDriveWritePending] = useState(false);
  const [emailDraftPending, setEmailDraftPending] = useState(false);
  const [emailPending, setEmailPending] = useState(false);
  const [computerPending, setComputerPending] = useState(false);
  const [computerControlPending, setComputerControlPending] = useState(false);
  const [computerControlUnlockPending, setComputerControlUnlockPending] = useState(false);
  const [computerUseStepPending, setComputerUseStepPending] = useState(false);
  const [briefingPending, setBriefingPending] = useState(false);
  const [setupPending, setSetupPending] = useState(false);
  const [soulProfilePending, setSoulProfilePending] = useState(false);
  const [appUpdateDownloadPending, setAppUpdateDownloadPending] = useState(false);
  const [appUpdateInstallPending, setAppUpdateInstallPending] = useState(false);
  const [deepSeekCachePending, setDeepSeekCachePending] = useState(false);
  const [deepSeekPricingPending, setDeepSeekPricingPending] = useState(false);
  const [deepSeekReadinessPending, setDeepSeekReadinessPending] = useState(false);
  const [capabilityPending, setCapabilityPending] = useState<CapabilityKind | null>(null);
  const [resolutionPending, setResolutionPending] = useState<string | null>(null);
  const agentMessagesRef = useRef(agentMessages);
  const queuedAgentGuidanceRef = useRef("");
  const queuedAgentPromptRef = useRef<QueuedAgentPrompt[]>([]);
  const agentStopRequestedRef = useRef(false);
  const agentChatRunTokenRef = useRef(0);
  const agentChatPendingRef = useRef(agentChatPending);
  const activeAgentConversationIdRef = useRef(activeAgentConversationId);
  const backgroundAgentWorkerBusyRef = useRef(false);
  const skillUpdateSweepStartedRef = useRef(false);
  const systemSkillRecords = useMemo(
    () => skillRecords.filter((record) => record.package_kind === "system_skill"),
    [skillRecords],
  );
  const installedPluginRecords = useMemo(
    () => skillRecords.filter((record) => record.package_kind === "plugin"),
    [skillRecords],
  );
  const installedSkillRecords = useMemo(
    () => skillRecords.filter((record) => record.package_kind === "skill"),
    [skillRecords],
  );
  const agentActionPermissionRequestIds = useMemo(() => {
    const requestIds = new Set<string>();
    for (const conversation of agentConversations) {
      for (const message of conversation.messages) {
        for (const action of message.proposed_actions ?? []) {
          if (action.permission_request_id) {
            requestIds.add(action.permission_request_id);
          }
        }
      }
    }
    return requestIds;
  }, [agentConversations]);
  const feedbackReviewItems = useMemo(() => {
    const memoriesById = new Map(memoryRecords.map((memory) => [memory.id, memory]));
    const feedbackByMemory = selectedMemoryFeedbackRecords.reduce(
      (groups, feedback) => {
        const records = groups.get(feedback.memory_id) ?? [];
        records.push(feedback);
        groups.set(feedback.memory_id, records);
        return groups;
      },
      new Map<string, MemorySelectedFeedback[]>(),
    );

    return Array.from(feedbackByMemory.entries())
      .map(([memoryId, feedbackRecords]) => {
        const counts = feedbackRecords.reduce(
          (summary, feedback) => ({
            ...summary,
            [feedback.feedback]: (summary[feedback.feedback] ?? 0) + 1,
          }),
          {} as Partial<Record<MemorySelectedFeedbackKind, number>>,
        );
        const latestFeedback = [...feedbackRecords].sort((left, right) =>
          String(right.created_at).localeCompare(String(left.created_at)),
        )[0];
        const repeatedIrrelevantFeedback = (counts.irrelevant ?? 0) >= 2;
        const repeatedStaleFeedback = (counts.stale ?? 0) >= 2;
        const needsFeedbackReview =
          repeatedIrrelevantFeedback ||
          repeatedStaleFeedback ||
          feedbackRecords.some((feedback) =>
            ["stale", "conflicting", "should_update"].includes(feedback.feedback),
          );

        return {
          memoryId,
          memory: memoriesById.get(memoryId) ?? null,
          counts,
          latestFeedback,
          records: feedbackRecords,
          needsFeedbackReview,
        };
      })
      .sort((left, right) => {
        if (left.needsFeedbackReview !== right.needsFeedbackReview) {
          return left.needsFeedbackReview ? -1 : 1;
        }
        return String(right.latestFeedback?.created_at ?? "").localeCompare(
          String(left.latestFeedback?.created_at ?? ""),
        );
      });
  }, [memoryRecords, selectedMemoryFeedbackRecords]);
  const filteredFeedbackReviewItems = useMemo(() => {
    const matchesFilter = (item: (typeof feedbackReviewItems)[number]) => {
      if (memoryFeedbackFilter === "all") {
        return true;
      }
      if (memoryFeedbackFilter === "needs_review") {
        return item.needsFeedbackReview;
      }
      return (item.counts[memoryFeedbackFilter] ?? 0) > 0;
    };
    return feedbackReviewItems
      .filter(matchesFilter)
      .sort((left, right) => {
        if (memoryFeedbackSort === "priority") {
          if (left.needsFeedbackReview !== right.needsFeedbackReview) {
            return left.needsFeedbackReview ? -1 : 1;
          }
        }
        if (memoryFeedbackSort === "feedback_count") {
          if (left.records.length !== right.records.length) {
            return right.records.length - left.records.length;
          }
        }
        return String(right.latestFeedback?.created_at ?? "").localeCompare(
          String(left.latestFeedback?.created_at ?? ""),
        );
      });
  }, [feedbackReviewItems, memoryFeedbackFilter, memoryFeedbackSort]);
  const filteredMemoryMaintenanceReviews = useMemo(() => {
    const matchesFilter = (item: MemoryMaintenanceReviewItem) => {
      if (memoryMaintenanceFilter === "all") {
        return true;
      }
      if (memoryMaintenanceFilter === "needs_review") {
        return item.review_needed;
      }
      if (memoryMaintenanceFilter === "snoozed") {
        return Boolean(item.snoozed_until);
      }
      return item.review_kinds.includes(memoryMaintenanceFilter);
    };
    return memoryMaintenanceReviews
      .filter(matchesFilter)
      .sort((left, right) => {
        if (memoryMaintenanceSort === "priority") {
          if (left.review_needed !== right.review_needed) {
            return left.review_needed ? -1 : 1;
          }
          if (left.review_kinds.length !== right.review_kinds.length) {
            return right.review_kinds.length - left.review_kinds.length;
          }
        }
        if (memoryMaintenanceSort === "feedback_count") {
          if (left.feedback_count !== right.feedback_count) {
            return right.feedback_count - left.feedback_count;
          }
        }
        return String(right.latest_feedback?.created_at ?? "").localeCompare(
          String(left.latest_feedback?.created_at ?? ""),
        );
      });
  }, [memoryMaintenanceFilter, memoryMaintenanceReviews, memoryMaintenanceSort]);
  const copy = translations[language];
  const exposePluginsSidebarEntry = shouldExposePluginsSidebarEntry();
  const settingsPanelItemCount = settingsPanelItems.length;
  const agentChatPendingBaseStatus =
    copy.chatWorkbench.pendingStages[
      Math.min(agentChatPendingStage, copy.chatWorkbench.pendingStages.length - 1)
    ] ?? copy.chatWorkbench.sendingStatus;
  const agentChatPendingStatus =
    activeAgentRun?.status === "cancel_requested"
      ? copy.chatWorkbench.stopRequestedFeedback
      : agentGuidanceStatus === "guiding"
      ? copy.chatWorkbench.guidanceRunning
      : agentGuidanceStatus === "queued"
        ? copy.chatWorkbench.guidanceQueued
        : agentChatPendingBaseStatus;
  const readyAgentAttachmentCount = readyAgentAttachments(agentAttachments).length;
  const agentComposerAction = agentChatComposerAction({
    pending: agentChatPending,
    draft: agentPrompt,
    attachmentCount: readyAgentAttachmentCount,
  });
  const showAgentStopControl = shouldShowAgentStopControl({
    pending: agentChatPending,
    composerAction: agentComposerAction,
  });
  const hasOpenAgentRuns = hasOpenAgentRunRecords(agentRunRecords);
  const networkSearchSourceModelMissing =
    modelToolStrategy.network_search_source_model_required &&
    !state.network_search_source_model;
  const appUpdateVersionLabel =
    appUpdateStatus.latest_version ?? downloadedAppUpdate?.latest_version ?? copy.appUpdate.update;
  const downloadedAppUpdateReady =
    downloadedAppUpdate !== null &&
    appUpdateStatus.update_available &&
    (appUpdateStatus.latest_version === null ||
      downloadedAppUpdate.latest_version === appUpdateStatus.latest_version) &&
    (appUpdateStatus.asset_name === null ||
      downloadedAppUpdate.asset_name === appUpdateStatus.asset_name);
  const appUpdateBusy = appUpdateDownloadPending || appUpdateInstallPending;
  const appUpdateButtonLabel = appUpdateInstallPending
    ? copy.appUpdate.installing
    : copy.appUpdate.install;
  const latestDeepSeekTelemetry = deepSeekTelemetry[0] ?? null;
  const latestDeepSeekTelemetryCacheLabel = latestDeepSeekTelemetry
    ? latestDeepSeekTelemetry.cache_status === "hit"
      ? copy.backendLabels.cacheHit
      : latestDeepSeekTelemetry.cache_status === "miss"
        ? copy.backendLabels.cacheMiss
        : copy.backendLabels.cacheDisabled
    : "";
  const latestDeepSeekTelemetryText = latestDeepSeekTelemetry
    ? `${latestDeepSeekTelemetryCacheLabel} / ${latestDeepSeekTelemetry.elapsed_ms}ms / ${
        latestDeepSeekTelemetry.total_tokens ?? copy.backendLabels.notSelected
      } ${copy.backendLabels.tokens}${
        latestDeepSeekTelemetry.estimated_cost_micro_usd !== null
          ? ` / ${copy.backendLabels.cost} ${formatMicroUsd(
              latestDeepSeekTelemetry.estimated_cost_micro_usd,
            )}`
          : ""
      }`
    : copy.backendLabels.noTelemetry;
  const computerControlUnlockUntilText =
    computerControlUnlockStatus.unlocked && computerControlUnlockStatus.unlocked_until
      ? formatTaskDate(computerControlUnlockStatus.unlocked_until, language)
      : "";
  const activeComputerUseSession = computerUseSessions[0] ?? null;
  const activeComputerUseStep = activeComputerUseSession
    ? (computerUseSteps.find((step) => step.id === activeComputerUseSession.active_step_id) ??
      computerUseSteps[computerUseSteps.length - 1] ??
      null)
    : null;
  const computerUseStepCopy =
    language === "zh"
      ? {
          title: "耐久可验证的电脑操作",
          empty: "先完成一次已授权截图，再创建隔离记事本步骤。",
          start: "用本次截图创建步骤",
          starting: "正在创建…",
          action: "单次结构化动作",
          bind: "绑定动作",
          binding: "正在绑定…",
          confirmRun: "确认并运行",
          requesting: "正在请求…",
          takeover: "我来接管",
          cancel: "取消步骤",
          reobserve: "用新截图重新观察",
          observed: "已观察",
          awaiting_approval: "等待精确审批",
          ready: "已就绪",
          action_started: "动作已开始",
          awaiting_verification: "等待验证",
          verified: "已验证",
          needs_replan: "需要重新规划",
          user_taken_over: "用户已接管",
          effect_unknown: "效果未知",
          verification_failed: "验证失败",
          cancelled: "已取消",
          unlockHint: "运行前还需要上方的本地解锁。",
          screenshotHint: "重新观察前，请先在电脑操作工具中拍摄一张新截图。",
        }
      : {
          title: "Durable verified Computer Use",
          empty: "Capture one permission-checked screenshot, then create an isolated Notepad step.",
          start: "Create step from screenshot",
          starting: "Creating…",
          action: "One structured action",
          bind: "Bind action",
          binding: "Binding…",
          confirmRun: "Confirm and run",
          requesting: "Requesting…",
          takeover: "Take over",
          cancel: "Cancel step",
          reobserve: "Re-observe from new screenshot",
          observed: "Observed",
          awaiting_approval: "Awaiting exact approval",
          ready: "Ready",
          action_started: "Action started",
          awaiting_verification: "Awaiting verification",
          verified: "Verified",
          needs_replan: "Needs replan",
          user_taken_over: "User took over",
          effect_unknown: "Effect unknown",
          verification_failed: "Verification failed",
          cancelled: "Cancelled",
          unlockHint: "The local unlock above is also required before execution.",
          screenshotHint: "Capture a fresh screenshot in Computer Use tools before re-observing.",
        };
  const deepSeekReadinessMessage =
    copy.onboarding.deepseekMessages[deepSeekCredentialStatus.code];
  const workspaceReadinessMessage =
    copy.onboarding.workspaceMessages[workspaceReadiness.code];
  const primaryDeepSeekApiKeyPlaceholder = deepSeekCredentialStatus.configured
      ? copy.settingsPanel.apiKeyConfiguredPlaceholder
      : copy.settingsPanel.apiKeyPlaceholder;
  const primaryDeepSeekApiKeyReady = deepSeekCredentialStatus.chat_completion_ready;
  const canRetryDeepSeekReadiness =
    deepSeekCredentialStatus.retryable ||
    deepSeekCredentialStatus.code === "not_checked" ||
    deepSeekCredentialStatus.code === "ready";
  const modelAvailabilityMessage = (available: boolean | null) =>
    available === null
      ? copy.onboarding.notChecked
      : available
        ? copy.onboarding.available
        : copy.onboarding.unavailable;

  const hydrateWorkspaceInputs = (readiness: OnboardingReadinessProjection) => {
    if (readiness.workspace.workspace_name) {
      setSetupWorkspaceName(readiness.workspace.workspace_name);
    }
  };

  const hydrateDeepSeekPricingInputs = (pricingState: DeepSeekPricingState) => {
    setDeepSeekPricingEnabled(pricingState.settings.enabled);
    setDeepSeekFlashPromptPrice(
      pricingState.settings.flash_prompt_usd_per_million_tokens,
    );
    setDeepSeekFlashCompletionPrice(
      pricingState.settings.flash_completion_usd_per_million_tokens,
    );
    setDeepSeekProPromptPrice(pricingState.settings.pro_prompt_usd_per_million_tokens);
    setDeepSeekProCompletionPrice(
      pricingState.settings.pro_completion_usd_per_million_tokens,
    );
  };

  const applySoulProfileState = (profileState: AgentSoulProfileState) => {
    setSoulProfileState(profileState);
    setSoulProfileDraft(profileState.content);
    const soulProfileBootstrap = agentSoulProfileBootstrapFromState(profileState) || null;
    setAgentConversations((currentConversations) =>
      currentConversations.map((conversation) => ({
        ...conversation,
        soul_profile_bootstrap: soulProfileBootstrap,
      })),
    );
  };

  const loadSoulProfileStateForBootstrap = async (): Promise<AgentSoulProfileState> => {
    if (!hasDesktopRuntime()) {
      return soulProfileState;
    }
    try {
      const profileState = await invoke<AgentSoulProfileState>("get_agent_soul_profile");
      applySoulProfileState(profileState);
      return profileState;
    } catch {
      return soulProfileState;
    }
  };

  const refreshSoulProfileAfterAgentResponse = async (response: AgentChatResponse) => {
    if (
      response.soul_profile_update?.status !== "applied" &&
      response.soul_profile_update?.status !== "unchanged"
    ) {
      return;
    }
    await loadSoulProfileStateForBootstrap();
  };

  useEffect(() => {
    if (!hasDesktopRuntime()) {
      setState(fallbackState);
      setOnboardingReadiness(fallbackOnboardingReadiness);
      setDeepSeekChatCacheState(fallbackDeepSeekChatCacheState);
      setDeepSeekTelemetry([]);
      setComputerControlUnlockStatus(fallbackComputerControlUnlockStatus);
      applySoulProfileState(fallbackAgentSoulProfileState);
      setAppUpdateStatus(fallbackAppUpdateStatus);
      setDeepSeekPricingState(fallbackDeepSeekPricingState);
      return;
    }

    void invoke<FoundationState>("get_foundation_state")
      .then(async (foundationState) => {
        setState(foundationState);
        const status = await invoke<AppUpdateStatus>("check_app_update");
        setAppUpdateStatus(status);
        if (status.update_available) {
          void downloadAvailableAppUpdate(status);
        }
      })
      .catch(() => setState(fallbackState));
    void invoke<OnboardingReadinessProjection>("get_onboarding_readiness")
      .then((readiness) => {
        setOnboardingReadiness(readiness);
        hydrateWorkspaceInputs(readiness);
        if (readiness.overall !== "ready") {
          setAgentSetupPrompt(readiness.next_step === "ready" ? "doctor" : readiness.next_step);
        }
      })
      .catch(() => {
        setOnboardingReadiness(fallbackOnboardingReadiness);
        setDeepSeekReadinessError(copy.onboarding.loadFailed);
      });
    void invoke<DeepSeekChatCacheState>("get_deepseek_chat_cache_state")
      .then(setDeepSeekChatCacheState)
      .catch(() => setDeepSeekChatCacheState(fallbackDeepSeekChatCacheState));
    void invoke<DeepSeekChatTelemetry[]>("list_deepseek_chat_telemetry")
      .then(setDeepSeekTelemetry)
      .catch(() => setDeepSeekTelemetry([]));
    void invoke<ComputerControlUnlockStatus>("get_computer_control_unlock_status")
      .then(setComputerControlUnlockStatus)
      .catch(() => setComputerControlUnlockStatus(fallbackComputerControlUnlockStatus));
    void refreshDurableComputerUseState().catch(() => {
      setComputerUseSessions([]);
      setComputerUseSteps([]);
    });
    void invoke<AgentSoulProfileState>("get_agent_soul_profile")
      .then(applySoulProfileState)
      .catch(() => {
        applySoulProfileState(fallbackAgentSoulProfileState);
        setSoulProfileError(copy.settingsPanel.soulProfileLoadFailed);
      });
    void invoke<DeepSeekPricingState>("get_deepseek_pricing_state")
      .then((pricingState) => {
        setDeepSeekPricingState(pricingState);
        hydrateDeepSeekPricingInputs(pricingState);
      })
      .catch(() => {
        setDeepSeekPricingState(fallbackDeepSeekPricingState);
        setDeepSeekPricingError(copy.deepSeekPricing.loadFailed);
      });
  }, []);

  useEffect(() => {
    if (!hasDesktopRuntime()) {
      setModelToolStrategy(fallbackModelDrivenToolStrategy);
      setNetworkSearchRouteStatus(fallbackNetworkSearchRouteStatus);
      setComputerUseBackendStatus(fallbackComputerUseBackendStatus);
      return;
    }

    let cancelled = false;
    const strategyRequest = {
      largeModelProvider: state.large_model_provider,
      networkSearchSourceModel: state.network_search_source_model,
    };

    void Promise.all([
      invoke<ModelDrivenToolStrategy>("get_model_driven_tool_strategy", strategyRequest),
      invoke<NetworkSearchRouteStatus>(
        "get_network_search_route_status_for_model",
        strategyRequest,
      ),
      invoke<ComputerUseBackendStatus>(
        "get_computer_use_backend_status_for_model",
        strategyRequest,
      ),
    ])
      .then(([strategy, networkSearchStatus, computerUseStatus]) => {
        if (cancelled) {
          return;
        }

        setModelToolStrategy(strategy);
        setNetworkSearchRouteStatus(networkSearchStatus);
        setComputerUseBackendStatus(computerUseStatus);
        setState((currentState) => ({
          ...currentState,
          tool_backends: {
            ...currentState.tool_backends,
            network_search: strategy.network_search_backend,
            computer_screenshot: strategy.computer_screenshot_backend,
            computer_control: strategy.computer_control_backend,
          },
        }));
      })
      .catch(() => {
        if (cancelled) {
          return;
        }

        setModelToolStrategy(fallbackModelDrivenToolStrategy);
        setNetworkSearchRouteStatus(fallbackNetworkSearchRouteStatus);
        setComputerUseBackendStatus(fallbackComputerUseBackendStatus);
      });

    return () => {
      cancelled = true;
    };
  }, [state.large_model_provider, state.network_search_source_model]);

  useEffect(() => {
    agentMessagesRef.current = agentMessages;
  }, [agentMessages]);

  useEffect(() => {
    agentChatPendingRef.current = agentChatPending;
  }, [agentChatPending]);

  useEffect(() => {
    activeAgentConversationIdRef.current = activeAgentConversationId;
  }, [activeAgentConversationId]);

  useEffect(() => {
    const chatThread = chatThreadRef.current;
    if (!chatThread) {
      return;
    }
    chatThread.scrollTo({
      top: chatThread.scrollHeight,
      behavior: "smooth",
    });
  }, [agentMessages.length, agentChatPending]);

  useEffect(() => {
    if (!agentChatPending) {
      setAgentChatPendingStage(0);
      return;
    }

    setAgentChatPendingStage(0);
    const stageCount = copy.chatWorkbench.pendingStages.length;
    const timers = agentChatPendingStageDelaysMs(stageCount)
      .slice(1)
      .map((delayMs) =>
        window.setTimeout(() => {
          setAgentChatPendingStage(agentChatPendingStageIndex(delayMs, stageCount));
        }, delayMs),
      );

    return () => {
      timers.forEach((timer) => window.clearTimeout(timer));
    };
  }, [agentChatPending, copy.chatWorkbench.pendingStages.length]);

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }
    window.localStorage.setItem(
      AGENT_CONVERSATIONS_STORAGE_KEY,
      JSON.stringify(agentConversations.slice(0, 50)),
    );
  }, [agentConversations]);

  useEffect(() => {
    if (!conversationMenu || typeof window === "undefined") {
      return;
    }

    const closeMenu = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Element && target.closest(".conversation-context-menu")) {
        return;
      }
      setConversationMenu(null);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setConversationMenu(null);
      }
    };
    window.addEventListener("pointerdown", closeMenu);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeMenu);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [conversationMenu]);

  const updateActiveAgentMessages = (
    updater:
      | AgentConversationMessage[]
      | ((currentMessages: AgentConversationMessage[]) => AgentConversationMessage[]),
    fallbackTitle = "",
  ) => {
    setAgentMessages((currentMessages) => {
      const nextMessages =
        typeof updater === "function" ? updater(currentMessages) : updater;
      agentMessagesRef.current = nextMessages;
      setAgentConversations((currentConversations) =>
        currentConversations
          .map((conversation) => {
            if (conversation.id !== activeAgentConversationId) {
              return conversation;
            }
            const title =
              conversation.manual_title
                ? conversation.title
                : deriveConversationTitle(nextMessages, fallbackTitle) ||
              conversation.title;
            return {
              ...conversation,
              title,
              messages: nextMessages,
              updated_at: new Date().toISOString(),
              context_state:
                estimateConversationTokens(nextMessages) >
                AGENT_CONTEXT_COMPRESSION_SOFT_LIMIT_TOKENS
                  ? ("compressed" as const)
                  : ("normal" as const),
            };
          })
          .sort((left, right) => {
            if (left.pinned !== right.pinned) {
              return left.pinned ? -1 : 1;
            }
            return new Date(right.updated_at).getTime() - new Date(left.updated_at).getTime();
          }),
      );
      return nextMessages;
    });
  };

  const setQueuedAgentGuidanceValue = (value: string) => {
    queuedAgentGuidanceRef.current = value;
    setQueuedAgentGuidance(value);
  };

  const setAgentStopRequestedValue = (value: boolean) => {
    agentStopRequestedRef.current = value;
  };

  const upsertAgentRunRecord = (record: AgentRunRecord) => {
    setAgentRunRecords((currentRecords) =>
      [record, ...currentRecords.filter((currentRecord) => currentRecord.id !== record.id)].sort(
        (left, right) =>
          new Date(right.updated_at).getTime() - new Date(left.updated_at).getTime(),
      ),
    );
  };

  const queueAgentGuidance = (
    guidanceValue: string,
    attachments: AgentAttachment[] = [],
  ) => {
    const readyAttachments = readyAgentAttachments(attachments);
    const guidance = guidanceValue.trim() || (readyAttachments.length > 0 ? copy.chatWorkbench.attachmentsOnlyPrompt : "");
    if (!guidance && readyAttachments.length === 0) {
      setAgentChatError(copy.chatWorkbench.emptyPrompt);
      return;
    }
    const guidanceWithAttachments = formatAgentPromptWithAttachments(guidance, readyAttachments);

    setAgentPrompt("");
    setAgentAttachments([]);
    setAgentAttachmentError("");
    setAgentChatError("");
    setAgentChatNotice(copy.chatWorkbench.guidanceQueuedFeedback);
    setAgentGuidanceStatus("queued");
    const currentRun = activeAgentRun;
    if (currentRun && hasDesktopRuntime()) {
      void invoke<AgentRunRecord>("queue_agent_run_guidance_record", {
        runId: currentRun.id,
        guidance: guidanceWithAttachments,
      })
        .then(upsertAgentRunRecord)
        .catch(() => null);
    }
    setActiveAgentRun((currentRun) =>
      currentRun
        ? queueAgentRunGuidance(currentRun, {
            id: createClientId("guidance"),
            content: guidanceWithAttachments,
            attachmentCount: readyAttachments.length,
            createdAt: new Date().toISOString(),
          })
        : currentRun,
    );
    setQueuedAgentGuidanceValue(
      queuedAgentGuidanceRef.current
        ? `${queuedAgentGuidanceRef.current}\n\n${guidanceWithAttachments}`
        : guidanceWithAttachments,
    );
  };

  const requestAgentStop = () => {
    setAgentChatError("");
    setAgentChatNotice(copy.chatWorkbench.stopRequestedFeedback);
    setAgentGuidanceStatus("idle");
    setQueuedAgentGuidanceValue("");
    setAgentStopRequestedValue(true);
    if (activeAgentRun && hasDesktopRuntime()) {
      void invoke<AgentRunRecord>("request_agent_run_cancel_record", {
        runId: activeAgentRun.id,
        reason: "User requested stop from the chat composer.",
      })
        .then(upsertAgentRunRecord)
        .catch(() => null);
    }
    setActiveAgentRun((currentRun) =>
      currentRun ? requestAgentRunCancel(currentRun, new Date().toISOString()) : currentRun,
    );
    agentChatRunTokenRef.current += 1;
    setAgentChatPending(false);
  };

  const addAgentAttachmentPaths = useCallback(async (paths: string[]) => {
    setAgentAttachmentError("");
    if (!hasDesktopRuntime()) {
      setAgentAttachmentError(copy.chatWorkbench.attachmentDesktopOnly);
      return;
    }

    const preparedPaths = prepareAgentAttachmentPaths(paths, agentAttachments);
    if (preparedPaths.length === 0) {
      return;
    }

    try {
      const readyAttachments = readyAgentAttachments(agentAttachments);
      const stagedAttachments = await invoke<AgentAttachment[]>("stage_agent_attachments", {
        paths: preparedPaths,
        existingCount: readyAttachments.length,
        existingTotalBytes: readyAttachments.reduce(
          (total, attachment) => total + attachment.byte_size,
          0,
        ),
      });
      setAgentAttachments((currentAttachments) => [
        ...currentAttachments,
        ...stagedAttachments,
      ]);
      const blockedReasons = stagedAttachments
        .filter((attachment) => attachment.status === "blocked" && attachment.blocked_reason)
        .map((attachment) => `${attachment.name}: ${attachment.blocked_reason}`);
      setAgentAttachmentError(blockedReasons.join(" "));
    } catch (error) {
      setAgentAttachmentError(String(error) || copy.chatWorkbench.attachmentAddFailed);
    }
  }, [
    agentAttachments,
    copy.chatWorkbench.attachmentAddFailed,
    copy.chatWorkbench.attachmentDesktopOnly,
  ]);

  const selectAgentAttachments = async () => {
    try {
      const selected = await open({
        multiple: true,
        directory: false,
      });
      const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
      await addAgentAttachmentPaths(paths);
    } catch (error) {
      setAgentAttachmentError(String(error) || copy.chatWorkbench.attachmentAddFailed);
    }
  };

  const removeAgentAttachment = (attachmentId: string) => {
    setAgentAttachments((currentAttachments) =>
      currentAttachments.filter((attachment) => attachment.id !== attachmentId),
    );
    setAgentAttachmentError("");
  };

  useEffect(() => {
    if (!hasDesktopRuntime()) {
      return;
    }

    let disposed = false;
    let unlistenDragDrop: (() => void) | null = null;
    const isInsideComposer = (position: { x: number; y: number }) => {
      const composer = chatComposerRef.current;
      if (!composer) {
        return false;
      }

      return isAgentAttachmentDropInsideComposer(
        position,
        composer.getBoundingClientRect(),
        window.devicePixelRatio,
      );
    };

    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "leave") {
          setAgentAttachmentDragActive(false);
          return;
        }

        if (payload.type === "enter" || payload.type === "over") {
          setAgentAttachmentDragActive(isInsideComposer(payload.position));
          return;
        }

        if (payload.type === "drop") {
          const shouldAddDroppedFiles = isInsideComposer(payload.position);
          setAgentAttachmentDragActive(false);
          if (shouldAddDroppedFiles) {
            void addAgentAttachmentPaths(payload.paths);
          }
        }
      })
      .then((unlisten) => {
        if (disposed) {
          unlisten();
          return;
        }
        unlistenDragDrop = unlisten;
      })
      .catch(() => {
        setAgentAttachmentDragActive(false);
      });

    return () => {
      disposed = true;
      setAgentAttachmentDragActive(false);
      if (unlistenDragDrop) {
        unlistenDragDrop();
      }
    };
  }, [addAgentAttachmentPaths]);

  const startNewAgentConversation = () => {
    const conversation = createEmptyAgentConversation(
      agentSoulProfileBootstrapFromState(soulProfileState),
    );
    setAgentConversations((currentConversations) =>
      sortAgentConversations([conversation, ...currentConversations]),
    );
    setActiveAgentConversationId(conversation.id);
    setAgentMessages([]);
    agentMessagesRef.current = [];
    setAgentPrompt("");
    setAgentChatError("");
    setAgentChatNotice("");
    setPendingAgentPrompt("");
    setAgentSetupPrompt(null);
    setAgentGuidanceStatus("idle");
    setQueuedAgentGuidanceValue("");
    setAgentStopRequestedValue(false);
    setConversationMenu(null);
    setRenamingConversationId(null);
  };

  const openAgentConversation = (conversationId: string) => {
    const conversation = agentConversations.find(
      (candidate) => candidate.id === conversationId && !candidate.archived,
    );
    if (!conversation) {
      return;
    }
    setActiveAgentConversationId(conversation.id);
    setAgentMessages(conversation.messages);
    agentMessagesRef.current = conversation.messages;
    setAgentPrompt("");
    setAgentChatError("");
    setAgentChatNotice("");
    setAgentGuidanceStatus("idle");
    setQueuedAgentGuidanceValue("");
    setAgentStopRequestedValue(false);
    setConversationMenu(null);
  };

  const openAgentConversationMenu = (
    event: MouseEvent,
    conversation: AgentConversationSession,
  ) => {
    event.preventDefault();
    const menuWidth = 156;
    const menuHeight = 132;
    const x =
      typeof window === "undefined"
        ? event.clientX
        : Math.min(event.clientX, window.innerWidth - menuWidth - 8);
    const y =
      typeof window === "undefined"
        ? event.clientY
        : Math.min(event.clientY, window.innerHeight - menuHeight - 8);
    setConversationMenu({
      conversationId: conversation.id,
      x: Math.max(8, x),
      y: Math.max(8, y),
    });
  };

  const toggleAgentConversationPinned = (conversationId: string) => {
    setAgentConversations((currentConversations) =>
      sortAgentConversations(
        currentConversations.map((conversation) =>
          conversation.id === conversationId
            ? { ...conversation, pinned: !conversation.pinned }
            : conversation,
        ),
      ),
    );
    setConversationMenu(null);
  };

  const archiveAgentConversation = (conversationId: string) => {
    setAgentConversations((currentConversations) => {
      const archivedConversations = currentConversations.map((conversation) =>
        conversation.id === conversationId
          ? { ...conversation, archived: true, updated_at: new Date().toISOString() }
          : conversation,
      );
      const sortedConversations = sortAgentConversations(archivedConversations);
      if (conversationId === activeAgentConversationId) {
        const replacement = sortedConversations.find(
          (conversation) => !conversation.archived,
        );
        if (replacement) {
          setActiveAgentConversationId(replacement.id);
          setAgentMessages(replacement.messages);
        } else {
          const emptyConversation = createEmptyAgentConversation(
            agentSoulProfileBootstrapFromState(soulProfileState),
          );
          setActiveAgentConversationId(emptyConversation.id);
          setAgentMessages([]);
          return [emptyConversation, ...sortedConversations];
        }
        setAgentPrompt("");
        setAgentChatError("");
      }
      return sortedConversations;
    });
    setConversationMenu(null);
    if (renamingConversationId === conversationId) {
      setRenamingConversationId(null);
      setRenameConversationTitle("");
    }
  };

  const beginRenameAgentConversation = (conversation: AgentConversationSession) => {
    setRenamingConversationId(conversation.id);
    setRenameConversationTitle(
      conversation.title ||
        deriveConversationTitle(conversation.messages, "") ||
        copy.nav.untitledConversation,
    );
    setConversationMenu(null);
  };

  const cancelRenameAgentConversation = () => {
    setRenamingConversationId(null);
    setRenameConversationTitle("");
  };

  const saveRenameAgentConversation = () => {
    const title = renameConversationTitle.trim();
    if (!title || !renamingConversationId) {
      cancelRenameAgentConversation();
      return;
    }
    const now = new Date().toISOString();
    setAgentConversations((currentConversations) =>
      sortAgentConversations(
        currentConversations.map((conversation) =>
          conversation.id === renamingConversationId
            ? {
                ...conversation,
                title,
                manual_title: true,
                updated_at: now,
              }
            : conversation,
        ),
      ),
    );
    setRenamingConversationId(null);
    setRenameConversationTitle("");
  };

  const submitRenameAgentConversation = (event: FormEvent) => {
    event.preventDefault();
    saveRenameAgentConversation();
  };

  useEffect(() => {
    if (!hasDesktopRuntime()) {
      setTaskRecords([]);
      setMemoryRecords([]);
      setMemoryCandidateRecords([]);
      setSelectedMemoryFeedbackRecords([]);
      setMemoryMaintenanceReviews([]);
      setPermissionAudits([]);
      setCapabilityCatalog([]);
      setCapabilityRecords([]);
      setCapabilityInvocations([]);
      setTaskGroupedAuthorizations([]);
      setAgentToolContracts([]);
      setToolInvocations([]);
      setAgentContextReceipts([]);
      setSkillRecords([]);
      setSkillUpdateSweep(null);
      setAgentRunRecords([]);
      setOperationsBriefingRuns([]);
      return;
    }

    void Promise.all([
      invoke<TaskRecord[]>("list_task_records"),
      invoke<MemoryRecord[]>("list_memory_records"),
      invoke<MemoryCandidateRecord[]>("list_memory_candidate_records"),
      invoke<MemorySelectedFeedback[]>("list_selected_memory_feedback"),
      invoke<MemoryMaintenanceReviewItem[]>("list_memory_maintenance_reviews"),
      invoke<PermissionAuditEntry[]>("list_permission_audit_entries"),
      invoke<CapabilityDescriptor[]>("list_capability_catalog"),
      invoke<CapabilityAccessRecord[]>("list_capability_access_records"),
      invoke<TaskGroupedAuthorizationView[]>("list_task_grouped_authorizations"),
      invoke<CapabilityInvocation[]>("list_capability_invocations"),
      invoke<AgentToolContract[]>("list_agent_tool_contracts"),
      invoke<ToolInvocationRecord[]>("list_agent_tool_invocations"),
      invoke<AgentContextReceipt[]>("list_agent_context_receipts"),
      invoke<SkillRecord[]>("list_skill_records"),
      invoke<AgentRunRecord[]>("list_agent_run_records"),
      invoke<OperationsBriefingRun[]>("list_operations_briefing_runs"),
    ])
      .then(([
        records,
        memories,
        memoryCandidates,
        selectedMemoryFeedback,
        maintenanceReviews,
        audits,
        catalog,
        capabilityAccessRecords,
        groupedAuthorizations,
        invocations,
        toolContracts,
        recordedToolInvocations,
        contextReceipts,
        skills,
        agentRuns,
        briefingRuns,
      ]) => {
        setTaskRecords(records);
        setMemoryRecords(memories);
        setMemoryCandidateRecords(memoryCandidates);
        setSelectedMemoryFeedbackRecords(selectedMemoryFeedback);
        setMemoryMaintenanceReviews(maintenanceReviews);
        setPermissionAudits(audits);
        setCapabilityCatalog(catalog);
        setCapabilityRecords(capabilityAccessRecords);
        setTaskGroupedAuthorizations(groupedAuthorizations);
        setCapabilityInvocations(invocations);
        setAgentToolContracts(toolContracts);
        setToolInvocations(recordedToolInvocations);
        setAgentContextReceipts(contextReceipts);
        setSkillRecords(skills);
        setAgentRunRecords(agentRuns);
        setOperationsBriefingRuns(briefingRuns);
        void runMemoryBackgroundMaintenance().catch(() => {
          // Background memory maintenance is best-effort; explicit feedback still remains logged.
        });
        if (!skillUpdateSweepStartedRef.current) {
          skillUpdateSweepStartedRef.current = true;
          setSkillUpdatePending(true);
          void invoke<SkillUpdateSweepResult>("run_skill_update_sweep")
            .then((sweep) => {
              setSkillUpdateSweep(sweep);
              setSkillRecords(sweep.records);
            })
            .catch((error) => {
              setSkillError(`${copy.skills.updateFailed} ${String(error)}`);
            })
            .finally(() => setSkillUpdatePending(false));
        }
      })
      .catch(() => {
        setPackageError(copy.package.loadFailed);
        setMemoryError(copy.memory.loadFailed);
        setMemoryCandidateError(copy.memory.loadFailed);
        setAuditError(copy.audit.loadFailed);
        setCapabilityError(copy.capabilities.loadFailed);
        setTaskGroupedAuthorizationError(
          language === "zh" ? "无法读取任务授权状态。" : "Could not load task authorization state.",
        );
        setBriefingError(copy.operationsBriefing.loadFailed);
      });
  }, [
    copy.audit.loadFailed,
    copy.capabilities.loadFailed,
    copy.memory.loadFailed,
    copy.operationsBriefing.loadFailed,
    copy.package.loadFailed,
    copy.skills.updateFailed,
    language,
  ]);

  const setLocalSkillEnabled = async (record: SkillRecord, enabled: boolean) => {
    setSkillActionPending(record.id);
    setSkillNotice("");
    setSkillError("");
    try {
      const updated = await invoke<SkillRecord>("set_skill_enabled", {
        skillId: record.id,
        enabled,
        note: enabled
          ? "Enabled directly from the installed plugin manager."
          : "Disabled directly from the installed plugin manager.",
      });
      setSkillRecords((records) =>
        records.map((current) => (current.id === updated.id ? updated : current)),
      );
      setSkillNotice(enabled ? copy.skills.enabledNotice : copy.skills.disabledNotice);
    } catch (error) {
      setSkillError(`${copy.skills.statusFailed} ${String(error)}`);
    } finally {
      setSkillActionPending(null);
    }
  };

  const uninstallLocalSkill = async (record: SkillRecord) => {
    setSkillActionPending(record.id);
    setSkillNotice("");
    setSkillError("");
    try {
      const records = await invoke<SkillRecord[]>("uninstall_skill", {
        skillId: record.id,
        note: "Uninstalled directly from the installed plugin manager.",
      });
      setSkillRecords(records);
      setSkillNotice(copy.skills.uninstalled);
    } catch (error) {
      setSkillError(`${copy.skills.uninstallFailed} ${String(error)}`);
    } finally {
      setSkillActionPending(null);
    }
  };

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

  const updateLargeModelProvider = (event: ChangeEvent<HTMLSelectElement>) => {
    setState((currentState) => ({
      ...currentState,
      large_model_provider: event.target.value as LargeModelProvider,
    }));
  };

  const updateNetworkSearchSourceModel = (event: ChangeEvent<HTMLSelectElement>) => {
    const value = event.target.value;
    setState((currentState) => ({
      ...currentState,
      network_search_source_model: value
        ? (value as NetworkSearchSourceModel)
        : null,
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

  const clearDeepSeekChatCache = async () => {
    setDeepSeekCachePending(true);
    setDeepSeekCacheNotice("");
    setDeepSeekCacheError("");

    try {
      const removed = await invoke<number>("clear_deepseek_chat_cache");
      const cacheState = await invoke<DeepSeekChatCacheState>(
        "get_deepseek_chat_cache_state",
      );
      setDeepSeekChatCacheState(cacheState);
      setDeepSeekCacheNotice(copy.backendLabels.cacheCleared(removed));
    } catch (error) {
      setDeepSeekCacheError(String(error) || copy.backendLabels.cacheClearFailed);
    } finally {
      setDeepSeekCachePending(false);
    }
  };

  const persistLocalDirectorySetup = async (
    options: { workspaceDir?: string; workspaceName?: string } = {},
  ): Promise<OnboardingReadinessProjection | null> => {
    const workspaceDir = options.workspaceDir ?? setupWorkspaceDir;
    const workspaceName = options.workspaceName ?? setupWorkspaceName;
    setSetupPending(true);
    setSetupError("");
    setSetupNotice("");

    try {
      const readiness = await invoke<OnboardingReadinessProjection>(
        "save_local_directory_settings",
        {
          workspaceDir,
          workspaceName,
        },
      );
      setOnboardingReadiness(readiness);
      hydrateWorkspaceInputs(readiness);
      if (readiness.workspace.code === "ready") {
        setSetupNotice(copy.localSetup.saved);
      }
      return readiness;
    } catch {
      setSetupError(copy.localSetup.failed);
      return null;
    } finally {
      setSetupPending(false);
    }
  };

  const saveLocalDirectorySetup = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    await persistLocalDirectorySetup();
  };

  const saveSoulProfile = async () => {
    if (!soulProfileDraft.trim()) {
      setSoulProfileError(copy.settingsPanel.soulProfileEmpty);
      return;
    }
    setSoulProfilePending(true);
    setSoulProfileNotice("");
    setSoulProfileError("");

    try {
      const profileState = await invoke<AgentSoulProfileState>("save_agent_soul_profile", {
        content: soulProfileDraft,
      });
      applySoulProfileState(profileState);
      setSoulProfileNotice(copy.settingsPanel.soulProfileSaved);
    } catch (error) {
      setSoulProfileError(String(error) || copy.settingsPanel.soulProfileSaveFailed);
    } finally {
      setSoulProfilePending(false);
    }
  };

  const saveDeepSeekPricingSetup = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setDeepSeekPricingPending(true);
    setDeepSeekPricingNotice("");
    setDeepSeekPricingError("");

    try {
      const pricingState = await invoke<DeepSeekPricingState>(
        "save_deepseek_pricing_settings",
        {
          enabled: deepSeekPricingEnabled,
          flashPromptUsdPerMillionTokens: deepSeekFlashPromptPrice,
          flashCompletionUsdPerMillionTokens: deepSeekFlashCompletionPrice,
          proPromptUsdPerMillionTokens: deepSeekProPromptPrice,
          proCompletionUsdPerMillionTokens: deepSeekProCompletionPrice,
        },
      );
      setDeepSeekPricingState(pricingState);
      hydrateDeepSeekPricingInputs(pricingState);
      setDeepSeekPricingNotice(copy.deepSeekPricing.saved);
    } catch (error) {
      setDeepSeekPricingError(String(error) || copy.deepSeekPricing.failed);
    } finally {
      setDeepSeekPricingPending(false);
    }
  };

  const saveDeepSeekKey = async (apiKey: string): Promise<OnboardingReadinessProjection | null> => {
    setDeepSeekReadinessPending(true);
    setDeepSeekReadinessError("");
    setDeepSeekReadinessNotice("");
    try {
      const readiness = await invoke<OnboardingReadinessProjection>("save_deepseek_api_key", {
        apiKey,
      });
      setOnboardingReadiness(readiness);
      if (
        readiness.deepseek.source === "stored" &&
        readiness.deepseek.code !== "key_format_invalid" &&
        readiness.deepseek.code !== "credential_store_unavailable"
      ) {
        setDeepSeekReadinessNotice(copy.onboarding.keySaved);
      }
      return readiness;
    } catch {
      setDeepSeekReadinessError(copy.onboarding.commandFailed);
      return null;
    } finally {
      setDeepSeekApiKeyDraft("");
      setDeepSeekReadinessPending(false);
    }
  };

  const retryDeepSeekReadiness = async (): Promise<OnboardingReadinessProjection | null> => {
    setDeepSeekReadinessPending(true);
    setDeepSeekReadinessError("");
    setDeepSeekReadinessNotice("");
    try {
      const readiness = await invoke<OnboardingReadinessProjection>(
        "verify_deepseek_api_key",
      );
      setOnboardingReadiness(readiness);
      setDeepSeekReadinessNotice(copy.onboarding.checkFinished);
      return readiness;
    } catch {
      setDeepSeekReadinessError(copy.onboarding.commandFailed);
      return null;
    } finally {
      setDeepSeekApiKeyDraft("");
      setDeepSeekReadinessPending(false);
    }
  };

  const removeDeepSeekKey = async () => {
    setDeepSeekReadinessPending(true);
    setDeepSeekReadinessError("");
    setDeepSeekReadinessNotice("");
    try {
      const readiness = await invoke<OnboardingReadinessProjection>(
        "remove_deepseek_api_key",
      );
      setOnboardingReadiness(readiness);
      setDeepSeekReadinessNotice(copy.onboarding.keyRemoved);
    } catch {
      setDeepSeekReadinessError(copy.onboarding.commandFailed);
    } finally {
      setDeepSeekApiKeyDraft("");
      setDeepSeekReadinessPending(false);
    }
  };

  const chooseLocalDirectory = async (options: { autoSave?: boolean } = {}) => {
    setSetupError("");
    setSetupNotice("");
    const previousWorkspaceDir = setupWorkspaceDir;

    try {
      const selected = await open({
        title: copy.localSetup.workspaceDialogTitle,
        directory: true,
        multiple: false,
        defaultPath: setupWorkspaceDir || undefined,
      });

      if (!selected || Array.isArray(selected)) {
        return;
      }

      setSetupWorkspaceDir(selected);
      if (options.autoSave) {
        const saved = await persistLocalDirectorySetup({ workspaceDir: selected });
        if (!saved) {
          setSetupWorkspaceDir(previousWorkspaceDir);
        }
      }
    } catch {
      setSetupError(copy.localSetup.chooseFailed);
    }
  };

  async function downloadAvailableAppUpdate(status: AppUpdateStatus) {
    if (!status.update_available) {
      return;
    }
    const downloadKey = `${status.latest_version ?? "unknown"}:${status.asset_name ?? "installer"}`;
    if (appUpdateDownloadKeyRef.current === downloadKey) {
      return;
    }

    appUpdateDownloadKeyRef.current = downloadKey;
    setAppUpdateDownloadPending(true);
    setAppUpdateError("");
    setAppUpdateNotice("");

    try {
      const result = await invoke<AppUpdateDownloadResult>("download_app_update");
      if (
        (status.latest_version !== null && result.latest_version !== status.latest_version) ||
        (status.asset_name !== null && result.asset_name !== status.asset_name) ||
        !/^dsur1\.[0-9a-f]{32}\.[0-9]+$/u.test(result.download_receipt) ||
        !/^[0-9a-f]{64}$/u.test(result.sha256) ||
        !Number.isSafeInteger(result.byte_size) ||
        result.byte_size <= 0
      ) {
        throw new Error(copy.appUpdate.downloadFailed);
      }
      setDownloadedAppUpdate(result);
      setAppUpdateNotice(copy.appUpdate.downloadReady(result.latest_version));
    } catch (error) {
      appUpdateDownloadKeyRef.current = null;
      setAppUpdateError(String(error) || copy.appUpdate.downloadFailed);
    } finally {
      setAppUpdateDownloadPending(false);
    }
  }

  const installAvailableAppUpdate = async () => {
    if (!downloadedAppUpdateReady || downloadedAppUpdate === null) {
      return;
    }
    setAppUpdateInstallPending(true);
    setAppUpdateError("");
    setAppUpdateNotice("");

    try {
      const result = await invoke<AppUpdateInstallResult>("install_app_update", {
        downloadReceipt: downloadedAppUpdate.download_receipt,
      });
      if (!result.restart_scheduled) {
        throw new Error(copy.appUpdate.installFailed);
      }
      setAppUpdateNotice(copy.appUpdate.installStarted(downloadedAppUpdate.latest_version));
    } catch (error) {
      setAppUpdateError(String(error) || copy.appUpdate.installFailed);
    } finally {
      setAppUpdateInstallPending(false);
    }
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

  async function refreshCapabilityState() {
    const [records, groupedAuthorizations, audits, invocations, contextReceipts, tools] =
      await Promise.all([
      invoke<CapabilityAccessRecord[]>("list_capability_access_records"),
      invoke<TaskGroupedAuthorizationView[]>("list_task_grouped_authorizations"),
      invoke<PermissionAuditEntry[]>("list_permission_audit_entries"),
      invoke<CapabilityInvocation[]>("list_capability_invocations"),
      invoke<AgentContextReceipt[]>("list_agent_context_receipts"),
      invoke<ToolInvocationRecord[]>("list_agent_tool_invocations"),
      ]);
    setCapabilityRecords(records);
    setTaskGroupedAuthorizations(groupedAuthorizations);
    setPermissionAudits(audits);
    setCapabilityInvocations(invocations);
    setAgentContextReceipts(contextReceipts);
    setToolInvocations(tools);
  }

  async function refreshTaskGroupedAuthorizationState() {
    const groupedAuthorizations = await invoke<TaskGroupedAuthorizationView[]>(
      "list_task_grouped_authorizations",
    );
    setTaskGroupedAuthorizations(groupedAuthorizations);
  }

  async function refreshDurableComputerUseState() {
    const sessions = await invoke<ComputerUseSessionView[]>(
      "list_durable_computer_use_sessions",
    );
    setComputerUseSessions(sessions);
    const session = sessions[0];
    if (!session) {
      setComputerUseSteps([]);
      return;
    }
    const steps = await invoke<ComputerUseStepView[]>("list_durable_computer_use_steps", {
      sessionId: session.id,
    });
    setComputerUseSteps(steps);
  }

  const refreshComputerControlUnlockStatus = async () => {
    const unlockStatus = await invoke<ComputerControlUnlockStatus>(
      "get_computer_control_unlock_status",
    );
    setComputerControlUnlockStatus(unlockStatus);
    return unlockStatus;
  };

  const refreshOperationsBriefingRuns = async () => {
    const runs = await invoke<OperationsBriefingRun[]>("list_operations_briefing_runs");
    setOperationsBriefingRuns(runs);
  };

  const refreshSkillRecords = async () => {
    const records = await invoke<SkillRecord[]>("list_skill_records");
    setSkillRecords(records);
    return records;
  };

  const refreshAgentRunRecords = async () => {
    const runs = await invoke<AgentRunRecord[]>("list_agent_run_records");
    setAgentRunRecords(runs);
    return runs;
  };

  useEffect(() => {
    if (!hasDesktopRuntime() || localDirectoryNeedsSetup) {
      return;
    }
    let cancelled = false;
    const sweepAutomations = async () => {
      await invoke<number>("reconcile_automation_runs");
      const queued = await invoke<number>("run_due_automation_sweep");
      if (!cancelled && queued > 0) {
        await refreshAgentRunRecords();
      }
    };
    void sweepAutomations().catch(() => null);
    const timer = window.setInterval(() => {
      void sweepAutomations().catch(() => null);
    }, 30_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [localDirectoryNeedsSetup]);

  useEffect(() => {
    if (!hasDesktopRuntime() || (!agentChatPending && !hasOpenAgentRuns)) {
      return;
    }

    void refreshAgentRunRecords().catch(() => null);
    const timer = window.setInterval(() => {
      void refreshAgentRunRecords().catch(() => null);
    }, 1500);

    return () => {
      window.clearInterval(timer);
    };
  }, [agentChatPending, hasOpenAgentRuns]);

  useEffect(() => {
    if (!hasDesktopRuntime() || localDirectoryNeedsSetup) {
      return;
    }

    let cancelled = false;
    const runNextDurableAgentTask = async () => {
      if (cancelled || !shouldRunDurableAgentWorker({
        desktopRuntime: hasDesktopRuntime(),
        setupNeeded: localDirectoryNeedsSetup,
        workerBusy: backgroundAgentWorkerBusyRef.current,
        chatPending: agentChatPendingRef.current,
        queuedLocalCount: queuedAgentPromptRef.current.length,
        credentialReady: deepSeekCredentialStatus.chat_completion_ready,
      })) {
        return;
      }

      backgroundAgentWorkerBusyRef.current = true;
      try {
        const currentRuns = await refreshAgentRunRecords();
        const recoverableParents = blockedParentsWithTerminalChildren(currentRuns);
        if (recoverableParents.length > 0) {
          const queuedRetries = (
            await Promise.all(
              recoverableParents.map((parent) =>
                invoke<AgentRunRecord[]>("queue_expert_team_retries", {
                  parentRunId: parent.id,
                }),
              ),
            )
          ).flat();
          queuedRetries.forEach(upsertAgentRunRecord);
          if (queuedRetries.length > 0) {
            await refreshAgentRunRecords();
            return;
          }
          const queuedParents = await Promise.all(
            recoverableParents.map((parent) =>
              expertTeamCanSynthesize(currentRuns, parent.id)
                ? invoke<AgentRunRecord>("queue_parent_agent_synthesis", {
                    parentRunId: parent.id,
                  })
                : Promise.resolve(null),
            ),
          );
          queuedParents.forEach((record) => record && upsertAgentRunRecord(record));
          if (queuedParents.some(Boolean)) {
            await refreshAgentRunRecords();
            return;
          }
        }
        const queuedSubagents = readyExpertAttempts(currentRuns);
        if (queuedSubagents.length > 0) {
          const subagentResults = await Promise.all(
            queuedSubagents.map((record, index) =>
              invoke<AgentRunWorkerResult | null>("run_next_queued_agent_chat_worker", {
                runId: record.id,
                executionPrompt: record.execution_prompt,
                workerId: `desktop-subagent-worker-${index + 1}`,
                largeModelProvider: state.large_model_provider,
                modelRoute: state.model_route,
                thinkingLevel: state.thinking_level,
                accessMode: state.access_mode,
                networkSearchSourceModel: state.network_search_source_model || null,
              }),
            ),
          );
          subagentResults.forEach((result) => {
            if (result) {
              upsertAgentRunRecord(result.record);
            }
          });
          const finishedRuns = await refreshAgentRunRecords();
          const waitingParents = finishedRuns.filter(
            (record) => record.role === "parent" && record.status === "blocked",
          );
          await Promise.all(
            waitingParents.map(async (parent) => {
              const children = finishedRuns.filter(
                (record) => record.parent_run_id === parent.id,
              );
              if (
                children.length > 0 &&
                children.every((child) =>
                  ["completed", "failed", "cancelled"].includes(child.status),
                )
              ) {
                const queuedRetries = await invoke<AgentRunRecord[]>(
                  "queue_expert_team_retries",
                  { parentRunId: parent.id },
                );
                queuedRetries.forEach(upsertAgentRunRecord);
                if (
                  queuedRetries.length === 0 &&
                  expertTeamCanSynthesize(finishedRuns, parent.id)
                ) {
                  const queuedParent = await invoke<AgentRunRecord>(
                    "queue_parent_agent_synthesis",
                    { parentRunId: parent.id },
                  );
                  upsertAgentRunRecord(queuedParent);
                }
              }
            }),
          );
          await Promise.all([refreshAgentRunRecords(), refreshCapabilityState()]);
          return;
        }
        const workerResult = await invoke<AgentRunWorkerResult | null>(
          "run_next_queued_agent_chat_worker",
          {
            runId: null,
            executionPrompt: null,
            workerId: "desktop-durable-worker",
            largeModelProvider: state.large_model_provider,
            modelRoute: state.model_route,
            thinkingLevel: state.thinking_level,
            accessMode: state.access_mode,
            networkSearchSourceModel: state.network_search_source_model || null,
          },
        );
        if (!workerResult || cancelled) {
          return;
        }

        upsertAgentRunRecord(workerResult.record);
        if (
          workerResult.record.status === "blocked" &&
          workerResult.response.subagent_plan.length > 0
        ) {
          await refreshAgentRunRecords();
          return;
        }
        await refreshSoulProfileAfterAgentResponse(workerResult.response);
        const assistantMessage: AgentConversationMessage = {
          id: workerResult.response.id,
          role: "assistant",
          content: workerResult.response.content,
          model: workerResult.response.model,
          protocol_version: workerResult.response.protocol_version,
          goal_projection: workerResult.response.goal_projection,
          proposed_actions: workerResult.response.proposed_actions,
          missing_prerequisites: workerResult.response.missing_prerequisites,
          memory_candidates: workerResult.response.memory_candidates,
          soul_profile_update: workerResult.response.soul_profile_update,
          created_at: workerResult.response.created_at,
        };
        setAgentConversations((currentConversations) => {
          let matchedConversation = false;
          const nextConversations = currentConversations.map((conversation) => {
            if (conversation.id !== workerResult.record.conversation_id) {
              return conversation;
            }
            matchedConversation = true;
            const hasAssistantMessage = conversation.messages.some(
              (message) => message.id === assistantMessage.id,
            );
            const hasUserPrompt = conversation.messages.some(
              (message) =>
                message.role === "user" && message.content === workerResult.record.prompt,
            );
            const messages = hasAssistantMessage
              ? conversation.messages
              : [
                  ...(hasUserPrompt
                    ? conversation.messages
                    : [
                        ...conversation.messages,
                        {
                          id: `run-${workerResult.record.id}-user`,
                          role: "user" as const,
                          content: workerResult.record.prompt,
                          created_at: workerResult.record.started_at,
                        },
                      ]),
                  assistantMessage,
                ];
            if (activeAgentConversationIdRef.current === conversation.id) {
              agentMessagesRef.current = messages;
              setAgentMessages(messages);
              setActiveAgentRun(agentChatRunFromRecord(workerResult.record));
            }
            return {
              ...conversation,
              messages,
              title:
                conversation.manual_title
                  ? conversation.title
                  : deriveConversationTitle(messages, workerResult.record.prompt) ||
                    conversation.title,
              updated_at: workerResult.record.updated_at,
            };
          });
          if (matchedConversation) {
            return sortAgentConversations(nextConversations);
          }

          const recoveredConversation = createEmptyAgentConversation();
          recoveredConversation.id = workerResult.record.conversation_id;
          recoveredConversation.title = deriveConversationTitle(
            [
              {
                id: `run-${workerResult.record.id}-user`,
                role: "user",
                content: workerResult.record.prompt,
                created_at: workerResult.record.started_at,
              },
              assistantMessage,
            ],
            workerResult.record.prompt,
          );
          recoveredConversation.messages = [
            {
              id: `run-${workerResult.record.id}-user`,
              role: "user",
              content: workerResult.record.prompt,
              created_at: workerResult.record.started_at,
            },
            assistantMessage,
          ];
          recoveredConversation.updated_at = workerResult.record.updated_at;
          return sortAgentConversations([...nextConversations, recoveredConversation]);
        });
        await Promise.all([
          refreshAgentRunRecords(),
          refreshCapabilityState(),
          refreshSkillRecords(),
        ]);
      } catch {
        if (!cancelled) {
          void refreshAgentRunRecords().catch(() => null);
        }
      } finally {
        backgroundAgentWorkerBusyRef.current = false;
      }
    };

    void runNextDurableAgentTask();
    const timer = window.setInterval(() => {
      void runNextDurableAgentTask();
    }, 3000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [
    deepSeekCredentialStatus.chat_completion_ready,
    localDirectoryNeedsSetup,
    state.access_mode,
    state.large_model_provider,
    state.model_route,
    state.network_search_source_model,
    state.thinking_level,
  ]);

  const refreshMemoryCandidateRecords = async () => {
    const candidates = await invoke<MemoryCandidateRecord[]>("list_memory_candidate_records");
    setMemoryCandidateRecords(candidates);
    return candidates;
  };

  const refreshMemoryMaintenanceReviews = async () => {
    const reviews = await invoke<MemoryMaintenanceReviewItem[]>("list_memory_maintenance_reviews");
    setMemoryMaintenanceReviews(reviews);
    return reviews;
  };

  const runMemoryBackgroundMaintenance = async () => {
    const summary = await invoke<MemoryBackgroundMaintenanceSummary>(
      "run_memory_background_maintenance",
    );
    await Promise.all([refreshMemoryCandidateRecords(), refreshMemoryMaintenanceReviews()]);
    return summary;
  };

  const searchMemoryRecords = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setMemoryPending(true);
    setMemoryError("");
    setMemoryNotice("");

    try {
      const memories = await loadMemoryRecords(memoryQuery);
      setMemoryRecords(memories);
    } catch {
      setMemoryError(copy.memory.loadFailed);
    } finally {
      setMemoryPending(false);
    }
  };

  const proposeMemoryCandidate = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!candidateTitle.trim() || !candidateBody.trim()) {
      setMemoryCandidateError(copy.memory.emptyCandidate);
      return;
    }
    if (candidateLifecycle === "expires" && !candidateExpiresAt) {
      setMemoryCandidateError(copy.memory.emptyExpiration);
      return;
    }

    setMemoryCandidatePending(true);
    setMemoryCandidateError("");
    setMemoryCandidateNotice("");

    try {
      await invoke<MemoryCandidateRecord>("propose_memory_candidate", {
        title: candidateTitle,
        body: candidateBody,
        memoryType: candidateMemoryType,
        scope: candidateMemoryScope,
        sensitivity: candidateSensitivity,
        lifecycle: candidateLifecycle,
        expiresAt: dateInputValueToIso(candidateExpiresAt, candidateLifecycle),
      });
      await runMemoryBackgroundMaintenance();
      const memories = await loadMemoryRecords(memoryQuery);
      setMemoryRecords(memories);
      setCandidateTitle("");
      setCandidateBody("");
      setCandidateExpiresAt("");
      setMemoryCandidateNotice(
        `${copy.memory.proposed} ${copy.memory.maintenanceAutomatic}: ${copy.memory.maintenanceNoUserAction}`,
      );
    } catch (error) {
      setMemoryCandidateError(String(error) || copy.memory.proposeFailed);
    } finally {
      setMemoryCandidatePending(false);
    }
  };

  const linkExistingMemoryRecords = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!memoryLinkSourceId || !memoryLinkTargetId || memoryLinkSourceId === memoryLinkTargetId) {
      setMemoryError(copy.memory.emptyExistingLink);
      return;
    }

    setMemoryExistingLinkPending(true);
    setMemoryError("");
    setMemoryNotice("");

    try {
      await invoke<MemoryRecord[]>("link_memory_records", {
        sourceMemoryId: memoryLinkSourceId,
        targetMemoryId: memoryLinkTargetId,
        relation: memoryExistingLinkRelation,
        note: memoryExistingLinkNote,
      });
      const memories = await loadMemoryRecords(memoryQuery);
      setMemoryRecords(memories);
      setMemoryNotice(copy.memory.existingLinked);
      setMemoryExistingLinkNote("");
    } catch (error) {
      setMemoryError(String(error) || copy.memory.existingLinkFailed);
    } finally {
      setMemoryExistingLinkPending(false);
    }
  };

  const recordSelectedMemoryFeedback = async (
    receiptId: string,
    memoryId: string,
    feedback: MemorySelectedFeedbackKind,
  ) => {
    const pendingKey = `${receiptId}:${memoryId}:${feedback}`;
    setMemoryFeedbackPending(pendingKey);
    setMemoryFeedbackNotice("");
    setMemoryFeedbackError("");

    try {
      const recordedFeedback = await invoke<MemorySelectedFeedback>("record_selected_memory_feedback", {
        memoryId,
        contextReceiptId: receiptId,
        feedback,
        note: copy.memoryFeedback.options[feedback],
      });
      setSelectedMemoryFeedbackRecords((currentRecords) => [
        recordedFeedback,
        ...currentRecords,
      ]);
      const maintenanceSummary = await runMemoryBackgroundMaintenance();
      setMemoryFeedbackNotice(
        copy.memoryFeedback.recordedWithMaintenance(
          maintenanceSummary.update_candidates_created,
          maintenanceSummary.retrieval_reviews_marked,
          maintenanceSummary.auto_updates_applied,
          maintenanceSummary.auto_archives_applied,
          maintenanceSummary.auto_candidate_decisions_applied,
          maintenanceSummary.merge_candidates_created,
          maintenanceSummary.auto_merges_applied,
          maintenanceSummary.model_update_rewrites_used,
        ),
      );
    } catch (error) {
      setMemoryFeedbackError(String(error) || copy.memoryFeedback.recordFailed);
    } finally {
      setMemoryFeedbackPending(null);
    }
  };

  const beginMemoryRecordEdit = (memory: MemoryRecord) => {
    setMemoryEditDraft({
      id: memory.id,
      title: memory.title,
      body: memory.body,
      memory_type: memory.memory_type,
      scope: memory.scope,
      sensitivity: memory.sensitivity,
      lifecycle: memory.lifecycle,
      expires_at: isoToDateInputValue(memory.expires_at),
    });
    setMemoryError("");
    setMemoryNotice("");
  };

  const patchMemoryEditDraft = (patch: Partial<MemoryEditDraft>) => {
    setMemoryEditDraft((currentDraft) =>
      currentDraft
        ? {
            ...currentDraft,
            ...patch,
          }
        : currentDraft,
    );
  };

  const cancelMemoryRecordEdit = () => {
    setMemoryEditDraft(null);
    setMemoryError("");
  };

  const updateMemoryRecord = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!memoryEditDraft) {
      return;
    }
    if (!memoryEditDraft.title.trim() || !memoryEditDraft.body.trim()) {
      setMemoryError(copy.memory.emptyEdit);
      return;
    }
    if (memoryEditDraft.lifecycle === "expires" && !memoryEditDraft.expires_at) {
      setMemoryError(copy.memory.emptyExpiration);
      return;
    }

    setMemoryUpdatePending(memoryEditDraft.id);
    setMemoryError("");
    setMemoryNotice("");

    try {
      await invoke<MemoryRecordUpdate>("update_memory_record", {
        memoryId: memoryEditDraft.id,
        title: memoryEditDraft.title,
        body: memoryEditDraft.body,
        memoryType: memoryEditDraft.memory_type,
        scope: memoryEditDraft.scope,
        sensitivity: memoryEditDraft.sensitivity,
        lifecycle: memoryEditDraft.lifecycle,
        expiresAt: dateInputValueToIso(
          memoryEditDraft.expires_at,
          memoryEditDraft.lifecycle,
        ),
        note: copy.memory.edit,
      });
      const memories = await loadMemoryRecords(memoryQuery);
      setMemoryRecords(memories);
      setMemoryEditDraft(null);
      setMemoryNotice(copy.memory.updated);
    } catch (error) {
      setMemoryError(String(error) || copy.memory.updateFailed);
    } finally {
      setMemoryUpdatePending(null);
    }
  };

  const deleteMemoryRecord = async (memoryId: string) => {
    setMemoryDeletionPending(memoryId);
    setMemoryError("");
    setMemoryNotice("");

    try {
      await invoke<MemoryRecordDeletion>("delete_memory_record", {
        memoryId,
        note: copy.memory.delete,
      });
      const memories = await loadMemoryRecords(memoryQuery);
      setMemoryRecords(memories);
      if (memoryEditDraft?.id === memoryId) {
        setMemoryEditDraft(null);
      }
      setMemoryNotice(copy.memory.deleted);
    } catch (error) {
      setMemoryError(String(error) || copy.memory.deleteFailed);
    } finally {
      setMemoryDeletionPending(null);
    }
  };

  const requestCapabilityAccess = async (capability: CapabilityKind) => {
    setCapabilityPending(capability);
    setCapabilityError("");
    setAuditError("");

    try {
      await invoke<CapabilityAccessRecord>("request_capability_access", {
        accessMode: state.access_mode,
        capability,
      });
      await refreshCapabilityState();
    } catch (error) {
      setCapabilityError(String(error) || copy.capabilities.requestFailed);
    } finally {
      setCapabilityPending(null);
    }
  };

  const browseBrowserUrl = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedUrl = browserUrl.trim();
    if (!trimmedUrl) {
      setBrowserError(copy.browserTool.failed);
      return;
    }

    setBrowserPending(true);
    setBrowserError("");
    setBrowserNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("browse_url", {
        accessMode: state.access_mode,
        url: trimmedUrl,
      });
      await refreshCapabilityState();
      if (invocation.status === "pending_approval") {
        setBrowserNotice(copy.browserTool.pendingHint);
      }
    } catch (error) {
      setBrowserError(String(error) || copy.browserTool.failed);
    } finally {
      setBrowserPending(false);
    }
  };

  const submitBrowserBoundary = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedUrl = browserSubmitUrl.trim();
    const trimmedSummary = browserSubmitSummary.trim();
    if (!trimmedUrl || !trimmedSummary) {
      setBrowserSubmitError(copy.browserSubmitTool.failed);
      return;
    }

    setBrowserSubmitPending(true);
    setBrowserSubmitError("");
    setBrowserSubmitNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("submit_browser_boundary", {
        accessMode: state.access_mode,
        url: trimmedUrl,
        summary: trimmedSummary,
      });
      await refreshCapabilityState();
      setBrowserSubmitNotice(
        invocation.status === "pending_approval"
          ? copy.browserSubmitTool.pendingHint
          : copy.browserSubmitTool.blocked,
      );
    } catch (error) {
      setBrowserSubmitError(String(error) || copy.browserSubmitTool.failed);
    } finally {
      setBrowserSubmitPending(false);
    }
  };

  const searchNetworkBoundary = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedQuery = networkSearchQuery.trim();
    const trimmedScope = networkSearchScope.trim();
    if (!trimmedQuery) {
      setNetworkSearchError(copy.networkSearchTool.failed);
      return;
    }
    if (networkSearchSourceModelMissing) {
      setNetworkSearchError(copy.networkSearchTool.sourceModelMissing);
      return;
    }
    if (!networkSearchRouteStatus.network_requests_enabled) {
      setNetworkSearchError(copy.networkSearchTool.routeNotEnabled);
      return;
    }

    setNetworkSearchPending(true);
    setNetworkSearchError("");
    setNetworkSearchNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("search_network_boundary", {
        accessMode: state.access_mode,
        largeModelProvider: state.large_model_provider,
        query: trimmedQuery,
        scope: trimmedScope,
        networkSearchSourceModel: state.network_search_source_model,
      });
      await refreshCapabilityState();
      setNetworkSearchNotice(
        invocation.status === "pending_approval"
          ? copy.networkSearchTool.pendingHint
          : invocation.status === "succeeded"
            ? copy.networkSearchTool.completed
            : copy.networkSearchTool.blocked,
      );
    } catch (error) {
      setNetworkSearchError(String(error) || copy.networkSearchTool.failed);
    } finally {
      setNetworkSearchPending(false);
    }
  };

  const readLocalFilePath = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedPath = filePath.trim();
    if (!trimmedPath) {
      setFileError(copy.fileTool.failed);
      return;
    }

    setFilePending(true);
    setFileError("");
    setFileNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("read_local_file", {
        accessMode: state.access_mode,
        path: trimmedPath,
      });
      await refreshCapabilityState();
      if (invocation.status === "pending_approval") {
        setFileNotice(copy.fileTool.pendingHint);
      }
    } catch (error) {
      setFileError(String(error) || copy.fileTool.failed);
    } finally {
      setFilePending(false);
    }
  };

  const writeFileBoundary = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedPath = fileWritePath.trim();
    const trimmedSummary = fileWriteSummary.trim();
    const trimmedContent = fileWriteContent.trim();
    if (!trimmedPath || !trimmedSummary || !trimmedContent) {
      setFileWriteError(copy.fileWriteTool.failed);
      return;
    }

    setFileWritePending(true);
    setFileWriteError("");
    setFileWriteNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("write_file_boundary", {
        accessMode: state.access_mode,
        path: trimmedPath,
        summary: trimmedSummary,
        content: fileWriteContent,
      });
      await refreshCapabilityState();
      setFileWriteNotice(
        invocation.status === "pending_approval"
          ? copy.fileWriteTool.pendingHint
          : invocation.status === "succeeded"
            ? copy.fileWriteTool.completed
            : copy.fileWriteTool.failed,
      );
    } catch (error) {
      setFileWriteError(String(error) || copy.fileWriteTool.failed);
    } finally {
      setFileWritePending(false);
    }
  };

  const ingestEvidenceFolderPath = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedPath = folderPath.trim();
    if (!trimmedPath) {
      setFolderError(copy.folderTool.failed);
      return;
    }

    setFolderPending(true);
    setFolderError("");
    setFolderNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("ingest_evidence_folder", {
        accessMode: state.access_mode,
        folderPath: trimmedPath,
      });
      await refreshCapabilityState();
      if (invocation.status === "pending_approval") {
        setFolderNotice(copy.folderTool.pendingHint);
      }
    } catch (error) {
      setFolderError(String(error) || copy.folderTool.failed);
    } finally {
      setFolderPending(false);
    }
  };

  const runTerminalReadCommand = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setTerminalPending(true);
    setTerminalError("");
    setTerminalNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("run_terminal_read", {
        accessMode: state.access_mode,
        command: terminalCommand,
      });
      await refreshCapabilityState();
      if (invocation.status === "pending_approval") {
        setTerminalNotice(copy.terminalTool.pendingHint);
      }
    } catch (error) {
      setTerminalError(String(error) || copy.terminalTool.failed);
    } finally {
      setTerminalPending(false);
    }
  };

  const runTerminalWriteBoundary = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!terminalWriteCommand.trim()) {
      setTerminalWriteError(copy.terminalTool.writeFailed);
      return;
    }

    setTerminalWritePending(true);
    setTerminalWriteError("");
    setTerminalWriteNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("run_terminal_write", {
        accessMode: state.access_mode,
        command: terminalWriteCommand,
      });
      await refreshCapabilityState();
      setTerminalWriteNotice(
        invocation.status === "pending_approval"
          ? copy.terminalTool.writePendingHint
          : copy.terminalTool.writeBlocked,
      );
    } catch (error) {
      setTerminalWriteError(String(error) || copy.terminalTool.writeFailed);
    } finally {
      setTerminalWritePending(false);
    }
  };

  const sendEmailBoundary = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedTo = emailTo.trim();
    const trimmedSubject = emailSubject.trim();
    const trimmedBody = emailBody.trim();
    if (!trimmedTo || !trimmedSubject || !trimmedBody) {
      setEmailError(copy.emailTool.failed);
      return;
    }

    setEmailPending(true);
    setEmailError("");
    setEmailNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("send_email_boundary", {
        accessMode: state.access_mode,
        to: trimmedTo,
        subject: trimmedSubject,
        body: trimmedBody,
      });
      await refreshCapabilityState();
      setEmailNotice(
        invocation.status === "pending_approval"
          ? copy.emailTool.pendingHint
          : copy.emailTool.blocked,
      );
    } catch (error) {
      setEmailError(String(error) || copy.emailTool.failed);
    } finally {
      setEmailPending(false);
    }
  };

  const readEmailBoundary = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedMailbox = emailMailbox.trim();
    const trimmedQuery = emailReadQuery.trim();
    if (!trimmedMailbox || !trimmedQuery) {
      setEmailReadError(copy.emailReadTool.failed);
      return;
    }

    setEmailReadPending(true);
    setEmailReadError("");
    setEmailReadNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("read_email_boundary", {
        accessMode: state.access_mode,
        mailbox: trimmedMailbox,
        query: trimmedQuery,
      });
      await refreshCapabilityState();
      setEmailReadNotice(
        invocation.status === "pending_approval"
          ? copy.emailReadTool.pendingHint
          : copy.emailReadTool.blocked,
      );
    } catch (error) {
      setEmailReadError(String(error) || copy.emailReadTool.failed);
    } finally {
      setEmailReadPending(false);
    }
  };

  const createEmailDraftBoundary = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedTo = draftEmailTo.trim();
    const trimmedSubject = draftEmailSubject.trim();
    const trimmedBody = draftEmailBody.trim();
    if (!trimmedTo || !trimmedSubject || !trimmedBody) {
      setEmailDraftError(copy.emailDraftTool.failed);
      return;
    }

    setEmailDraftPending(true);
    setEmailDraftError("");
    setEmailDraftNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("create_email_draft_boundary", {
        accessMode: state.access_mode,
        to: trimmedTo,
        subject: trimmedSubject,
        body: trimmedBody,
      });
      await refreshCapabilityState();
      setEmailDraftNotice(
        invocation.status === "pending_approval"
          ? copy.emailDraftTool.pendingHint
          : copy.emailDraftTool.blocked,
      );
    } catch (error) {
      setEmailDraftError(String(error) || copy.emailDraftTool.failed);
    } finally {
      setEmailDraftPending(false);
    }
  };

  const readDriveBoundary = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedLocation = driveLocation.trim();
    const trimmedQuery = driveReadQuery.trim();
    if (!trimmedLocation || !trimmedQuery) {
      setDriveReadError(copy.driveReadTool.failed);
      return;
    }

    setDriveReadPending(true);
    setDriveReadError("");
    setDriveReadNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("read_drive_boundary", {
        accessMode: state.access_mode,
        location: trimmedLocation,
        query: trimmedQuery,
      });
      await refreshCapabilityState();
      setDriveReadNotice(
        invocation.status === "pending_approval"
          ? copy.driveReadTool.pendingHint
          : invocation.status === "succeeded"
            ? copy.driveReadTool.completed
            : copy.driveReadTool.blocked,
      );
    } catch (error) {
      setDriveReadError(String(error) || copy.driveReadTool.failed);
    } finally {
      setDriveReadPending(false);
    }
  };

  const writeDriveBoundary = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedLocation = driveWriteLocation.trim();
    const trimmedSummary = driveWriteSummary.trim();
    if (!trimmedLocation || !trimmedSummary) {
      setDriveWriteError(copy.driveWriteTool.failed);
      return;
    }

    setDriveWritePending(true);
    setDriveWriteError("");
    setDriveWriteNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("write_drive_boundary", {
        accessMode: state.access_mode,
        location: trimmedLocation,
        summary: trimmedSummary,
      });
      await refreshCapabilityState();
      setDriveWriteNotice(
        invocation.status === "pending_approval"
          ? copy.driveWriteTool.pendingHint
          : invocation.status === "succeeded"
            ? copy.driveWriteTool.completed
            : copy.driveWriteTool.blocked,
      );
    } catch (error) {
      setDriveWriteError(String(error) || copy.driveWriteTool.failed);
    } finally {
      setDriveWritePending(false);
    }
  };

  const captureComputerScreenshot = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setComputerPending(true);
    setComputerError("");
    setComputerNotice("");
    setLastComputerScreenshotInvocationId(null);

    try {
      const invocation = await invoke<CapabilityInvocation>("capture_computer_screenshot", {
        accessMode: state.access_mode,
        largeModelProvider: state.large_model_provider,
        networkSearchSourceModel: state.network_search_source_model,
      });
      await refreshCapabilityState();
      if (invocation.status === "pending_approval") {
        setComputerNotice(copy.computerTool.pendingHint);
      } else if (invocation.status === "succeeded") {
        setLastComputerScreenshotInvocationId(invocation.id);
        setComputerNotice(copy.computerTool.captured);
      } else {
        setComputerNotice(copy.computerTool.unavailable);
      }
    } catch (error) {
      setComputerError(String(error) || copy.computerTool.failed);
    } finally {
      setComputerPending(false);
    }
  };

  const startDurableComputerUseStep = async () => {
    if (!lastComputerScreenshotInvocationId) {
      setComputerUseStepError(computerUseStepCopy.empty);
      return;
    }
    setComputerUseStepPending(true);
    setComputerUseStepError("");
    setComputerUseStepNotice("");
    try {
      const result = await invoke<ComputerUseSessionStartResult>(
        "start_durable_computer_use_session",
        {
          screenshotInvocationId: lastComputerScreenshotInvocationId,
          runId: null,
          safeGoalSummary:
            language === "zh"
              ? "在隔离的记事本类应用中执行一个可验证动作。"
              : "Execute one verifiable action in an isolated Notepad-like app.",
          undoCapability: "none",
        },
      );
      setComputerUseSessions([result.session]);
      setComputerUseSteps([result.step]);
      setComputerUseStepNotice(result.step.status_reason ?? computerUseStepCopy.observed);
    } catch (error) {
      setComputerUseStepError(String(error) || computerUseStepCopy.empty);
    } finally {
      setComputerUseStepPending(false);
    }
  };

  const bindDurableComputerUseAction = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!activeComputerUseStep || !computerUseActionDraft.trim()) {
      return;
    }
    setComputerUseStepPending(true);
    setComputerUseStepError("");
    setComputerUseStepNotice("");
    try {
      const step = await invoke<ComputerUseStepView>("bind_durable_computer_use_action", {
        stepId: activeComputerUseStep.id,
        actionContract: computerUseActionDraft.trim(),
        safeSummary:
          language === "zh"
            ? "在隔离的前台编辑器中执行这一项已显示的结构化动作。"
            : "Execute this displayed structured action in the isolated foreground editor.",
        postcondition: { kind: "semantic_changed" },
      });
      await refreshDurableComputerUseState();
      setComputerUseStepNotice(step.status_reason ?? computerUseStepCopy.awaiting_approval);
    } catch (error) {
      setComputerUseStepError(String(error));
    } finally {
      setComputerUseStepPending(false);
    }
  };

  const runDurableComputerUseStep = async () => {
    if (!activeComputerUseStep) {
      return;
    }
    setComputerUseStepPending(true);
    setComputerUseStepError("");
    setComputerUseStepNotice("");
    try {
      const result = await invoke<ComputerUseRunResult>("run_durable_computer_use_step", {
        stepId: activeComputerUseStep.id,
        accessMode: state.access_mode,
      });
      await Promise.all([refreshDurableComputerUseState(), refreshCapabilityState()]);
      if (result.safe_error) {
        setComputerUseStepError(result.safe_error);
      } else if (result.capability_invocation?.status === "pending_approval") {
        setComputerUseStepNotice(
          language === "zh"
            ? "精确动作已进入审批队列；批准后再次点击“确认并运行”。"
            : "The exact action is in the approval queue. Approve it, then confirm and run again.",
        );
      } else {
        setComputerUseStepNotice(result.step.status_reason ?? result.execution_summary ?? "");
      }
    } catch (error) {
      setComputerUseStepError(String(error));
    } finally {
      setComputerUseStepPending(false);
    }
  };

  const takeOverDurableComputerUseStep = async () => {
    if (!activeComputerUseStep) {
      return;
    }
    setComputerUseStepPending(true);
    setComputerUseStepError("");
    try {
      const step = await invoke<ComputerUseStepView>("take_over_durable_computer_use_step", {
        stepId: activeComputerUseStep.id,
        reason:
          language === "zh" ? "用户从界面主动接管。" : "User took over from the interface.",
      });
      await refreshDurableComputerUseState();
      setComputerUseStepNotice(step.status_reason ?? computerUseStepCopy.user_taken_over);
    } catch (error) {
      setComputerUseStepError(String(error));
    } finally {
      setComputerUseStepPending(false);
    }
  };

  const cancelDurableComputerUseStep = async () => {
    if (!activeComputerUseStep) {
      return;
    }
    setComputerUseStepPending(true);
    setComputerUseStepError("");
    try {
      const step = await invoke<ComputerUseStepView>("cancel_durable_computer_use_step", {
        stepId: activeComputerUseStep.id,
        reason: language === "zh" ? "用户取消了动作前步骤。" : "User cancelled the pre-action step.",
      });
      await refreshDurableComputerUseState();
      setComputerUseStepNotice(step.status_reason ?? computerUseStepCopy.cancelled);
    } catch (error) {
      setComputerUseStepError(String(error));
    } finally {
      setComputerUseStepPending(false);
    }
  };

  const reobserveDurableComputerUseStep = async () => {
    if (!activeComputerUseSession || !lastComputerScreenshotInvocationId) {
      setComputerUseStepError(computerUseStepCopy.screenshotHint);
      return;
    }
    setComputerUseStepPending(true);
    setComputerUseStepError("");
    try {
      const step = await invoke<ComputerUseStepView>(
        "reobserve_durable_computer_use_session",
        {
          sessionId: activeComputerUseSession.id,
          screenshotInvocationId: lastComputerScreenshotInvocationId,
          undoCapability: "none",
        },
      );
      await refreshDurableComputerUseState();
      setComputerUseStepNotice(step.status_reason ?? computerUseStepCopy.observed);
    } catch (error) {
      setComputerUseStepError(String(error));
    } finally {
      setComputerUseStepPending(false);
    }
  };

  const unlockComputerControl = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedToken = computerControlUnlockToken.trim();
    if (!trimmedToken) {
      setComputerControlUnlockError(copy.computerControlTool.unlockFailed);
      return;
    }

    setComputerControlUnlockPending(true);
    setComputerControlUnlockError("");
    setComputerControlUnlockNotice("");

    try {
      const unlockStatus = await invoke<ComputerControlUnlockStatus>(
        "unlock_computer_control",
        {
          token: trimmedToken,
        },
      );
      setComputerControlUnlockStatus(unlockStatus);
      setComputerControlUnlockToken("");
      setComputerControlUnlockNotice(copy.computerControlTool.unlockReady);
    } catch (error) {
      setComputerControlUnlockError(String(error) || copy.computerControlTool.unlockFailed);
    } finally {
      setComputerControlUnlockPending(false);
    }
  };

  const controlComputerBoundary = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedTarget = computerControlTarget.trim();
    const trimmedAction = computerControlAction.trim();
    if (!trimmedTarget || !trimmedAction) {
      setComputerControlError(copy.computerControlTool.failed);
      return;
    }

    setComputerControlPending(true);
    setComputerControlError("");
    setComputerControlNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("control_computer_boundary", {
        accessMode: state.access_mode,
        largeModelProvider: state.large_model_provider,
        networkSearchSourceModel: state.network_search_source_model,
        target: trimmedTarget,
        action: trimmedAction,
      });
      await refreshCapabilityState();
      setComputerControlNotice(
        invocation.status === "pending_approval"
          ? copy.computerControlTool.pendingHint
          : invocation.status === "succeeded"
            ? copy.computerControlTool.executed
            : copy.computerControlTool.blocked,
      );
    } catch (error) {
      setComputerControlError(String(error) || copy.computerControlTool.failed);
    } finally {
      void refreshComputerControlUnlockStatus().catch(() =>
        setComputerControlUnlockStatus(fallbackComputerControlUnlockStatus),
      );
      setComputerControlPending(false);
    }
  };

  const runOperationsBriefingWorkflow = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedPath = briefingFolderPath.trim();
    if (!trimmedPath) {
      setBriefingError(copy.operationsBriefing.failed);
      return;
    }

    setBriefingPending(true);
    setBriefingError("");
    setBriefingNotice("");

    try {
      const run = await invoke<OperationsBriefingRun>("run_operations_briefing", {
        accessMode: state.access_mode,
        evidenceFolderPath: trimmedPath,
        largeModelProvider: state.large_model_provider,
        modelRoute: state.model_route,
        thinkingLevel: state.thinking_level,
      });
      const [, , telemetry, cacheState] = await Promise.all([
        refreshCapabilityState(),
        refreshOperationsBriefingRuns(),
        invoke<DeepSeekChatTelemetry[]>("list_deepseek_chat_telemetry"),
        invoke<DeepSeekChatCacheState>("get_deepseek_chat_cache_state"),
      ]);
      setDeepSeekTelemetry(telemetry);
      setDeepSeekChatCacheState(cacheState);
      if (run.status === "pending_approval") {
        setBriefingNotice(copy.operationsBriefing.pendingHint);
      }
    } catch (error) {
      setBriefingError(String(error) || copy.operationsBriefing.failed);
    } finally {
      setBriefingPending(false);
    }
  };

  const seedOperationsBriefingEvidenceTemplates = async () => {
    setBriefingPending(true);
    setBriefingError("");
    setBriefingNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>(
        "seed_operations_briefing_evidence_templates",
        {
          accessMode: state.access_mode,
        },
      );
      await refreshCapabilityState();
      if (invocation.evidence_ref) {
        setBriefingFolderPath(invocation.evidence_ref);
        setFolderPath((current) => current || invocation.evidence_ref || "");
      }
      setBriefingNotice(
        invocation.status === "pending_approval"
          ? copy.operationsBriefing.seedPendingHint
          : invocation.status === "succeeded"
            ? copy.operationsBriefing.seededTemplates
            : copy.operationsBriefing.seedFailed,
      );
    } catch (error) {
      setBriefingError(String(error) || copy.operationsBriefing.seedFailed);
    } finally {
      setBriefingPending(false);
    }
  };

  const resolveCapabilityAccess = async (requestId: string, approved: boolean) => {
    setResolutionPending(requestId);
    setCapabilityError("");

    try {
      await invoke("resolve_capability_access_request", {
        requestId,
        approved,
        note: approved ? copy.capabilities.approve : copy.capabilities.reject,
      });
      await refreshCapabilityState();
      return true;
    } catch (error) {
      setCapabilityError(String(error) || copy.capabilities.resolveFailed);
      return false;
    } finally {
      setResolutionPending(null);
    }
  };

  const resolveTaskGroupedAuthorization = async (
    intent: TaskGroupedAuthorizationIntent,
    approved: boolean,
  ) => {
    setTaskGroupedAuthorizationPending(intent.group_id);
    setTaskGroupedAuthorizationNotice("");
    setTaskGroupedAuthorizationError("");
    let rejectedByKernel = false;
    try {
      await invoke<TaskGroupedAuthorizationView>("resolve_task_grouped_authorization", {
        intent,
        approved,
      });
      setTaskGroupedAuthorizationNotice(
        language === "zh"
          ? approved
            ? "已按 Kernel 中的精确任务范围完成授权。"
            : "已拒绝本次任务授权。"
          : approved
            ? "The exact Kernel-owned task scope is approved."
            : "The task authorization was rejected.",
      );
    } catch {
      rejectedByKernel = true;
    } finally {
      try {
        await refreshTaskGroupedAuthorizationState();
        if (rejectedByKernel) {
          setTaskGroupedAuthorizationError(
            language === "zh"
              ? "授权意图已过期或与当前范围冲突；已刷新为 Kernel 权威状态。"
              : "The intent was stale or conflicted with the current scope; Kernel state was refreshed.",
          );
        }
      } catch {
        setTaskGroupedAuthorizations([]);
        setTaskGroupedAuthorizationNotice("");
        setTaskGroupedAuthorizationError(
          language === "zh"
            ? "无法重读 Kernel 权威状态；旧授权操作已隐藏，未授予任何新权限。"
            : "Kernel authority could not be re-read; stale actions were hidden and no new authority was granted.",
        );
      }
      setTaskGroupedAuthorizationPending(null);
    }
  };

  const revokeTaskGroupedAuthorization = async (intent: TaskGroupedAuthorizationIntent) => {
    setTaskGroupedAuthorizationPending(intent.group_id);
    setTaskGroupedAuthorizationNotice("");
    setTaskGroupedAuthorizationError("");
    let rejectedByKernel = false;
    try {
      await invoke<TaskGroupedAuthorizationView>("revoke_task_grouped_authorization", {
        intent,
      });
      setTaskGroupedAuthorizationNotice(
        language === "zh"
          ? "已撤销本次任务授权。"
          : "The task authorization was revoked.",
      );
    } catch {
      rejectedByKernel = true;
    } finally {
      try {
        await refreshTaskGroupedAuthorizationState();
        if (rejectedByKernel) {
          setTaskGroupedAuthorizationError(
            language === "zh"
              ? "撤销意图已过期或与当前范围冲突；已刷新为 Kernel 权威状态。"
              : "The revoke intent was stale or conflicted with the current scope; Kernel state was refreshed.",
          );
        }
      } catch {
        setTaskGroupedAuthorizations([]);
        setTaskGroupedAuthorizationNotice("");
        setTaskGroupedAuthorizationError(
          language === "zh"
            ? "无法重读 Kernel 权威状态；旧授权操作已隐藏，未授予任何新权限。"
            : "Kernel authority could not be re-read; stale actions were hidden and no new authority was granted.",
        );
      }
      setTaskGroupedAuthorizationPending(null);
    }
  };

  const clearPackageStatus = () => {
    setPackageNotice("");
    setPackageError("");
  };

  const exportOperationsBriefingPackage = async () => {
    clearPackageStatus();
    setBriefingPending(true);
    setPackagePending(true);
    setBriefingError("");
    setBriefingNotice("");

    try {
      const workPackage = await invoke<WorkPackage>("export_work_package");
      setExportedPackageJson(JSON.stringify(workPackage, null, 2));
      setBriefingNotice(copy.operationsBriefing.exported);
      setPackageNotice(copy.package.exported);
    } catch (error) {
      setBriefingError(String(error) || copy.operationsBriefing.failed);
    } finally {
      setBriefingPending(false);
      setPackagePending(false);
    }
  };

  const exportOperationsBriefingReport = async () => {
    if (!latestOperationsBriefingRun) {
      return;
    }

    setBriefingPending(true);
    setBriefingError("");
    setBriefingNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("export_operations_briefing_report", {
        accessMode: state.access_mode,
        runId: latestOperationsBriefingRun.id,
      });
      await refreshCapabilityState();
      setBriefingNotice(
        invocation.status === "pending_approval"
          ? copy.operationsBriefing.reportPendingHint
          : invocation.status === "succeeded"
            ? copy.operationsBriefing.reportExported
            : copy.operationsBriefing.reportExportFailed,
      );
    } catch (error) {
      setBriefingError(String(error) || copy.operationsBriefing.reportExportFailed);
    } finally {
      setBriefingPending(false);
    }
  };

  const exportOperationsBriefingHtmlReport = async () => {
    if (!latestOperationsBriefingRun) {
      return;
    }

    setBriefingPending(true);
    setBriefingError("");
    setBriefingNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("export_operations_briefing_html_report", {
        accessMode: state.access_mode,
        runId: latestOperationsBriefingRun.id,
      });
      await refreshCapabilityState();
      setBriefingNotice(
        invocation.status === "pending_approval"
          ? copy.operationsBriefing.htmlReportPendingHint
          : invocation.status === "succeeded"
            ? copy.operationsBriefing.htmlReportExported
            : copy.operationsBriefing.htmlReportExportFailed,
      );
    } catch (error) {
      setBriefingError(String(error) || copy.operationsBriefing.htmlReportExportFailed);
    } finally {
      setBriefingPending(false);
    }
  };

  const exportOperationsBriefingPdfReport = async () => {
    if (!latestOperationsBriefingRun) {
      return;
    }

    setBriefingPending(true);
    setBriefingError("");
    setBriefingNotice("");

    try {
      const invocation = await invoke<CapabilityInvocation>("export_operations_briefing_pdf_report", {
        accessMode: state.access_mode,
        runId: latestOperationsBriefingRun.id,
      });
      await refreshCapabilityState();
      setBriefingNotice(
        invocation.status === "pending_approval"
          ? copy.operationsBriefing.pdfReportPendingHint
          : invocation.status === "succeeded"
            ? copy.operationsBriefing.pdfReportExported
            : copy.operationsBriefing.pdfReportExportFailed,
      );
    } catch (error) {
      setBriefingError(String(error) || copy.operationsBriefing.pdfReportExportFailed);
    } finally {
      setBriefingPending(false);
    }
  };

  const createTaskRecord = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    clearPackageStatus();

    const trimmedSummary = taskSummary.trim();
    const derivedTitle = trimmedSummary.split(/\r?\n/)[0]?.trim().slice(0, 80) ?? "";
    const normalizedTitle = taskTitle.trim() || derivedTitle;

    if (!normalizedTitle) {
      setPackageError(copy.package.emptyTitle);
      return;
    }

    setPackagePending(true);
    try {
      const record = await invoke<TaskRecord>("create_task_record", {
        title: normalizedTitle,
        summary: trimmedSummary,
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

  const sendAgentPrompt = async (
    promptValue: string,
    options: {
      skipWorkspaceSetup?: boolean;
      skipNetworkSearchSetup?: boolean;
      isGuidanceContinuation?: boolean;
      displayPrompt?: string;
      attachments?: AgentAttachment[];
      runId?: string;
    } = {},
  ) => {
    const readyAttachments = readyAgentAttachments(options.attachments ?? []);
    const prompt = promptValue.trim() || (readyAttachments.length > 0 ? copy.chatWorkbench.attachmentsOnlyPrompt : "");
    if (!prompt && readyAttachments.length === 0) {
      setAgentChatError(copy.chatWorkbench.emptyPrompt);
      return;
    }
    const promptWithAttachments = formatAgentPromptWithAttachments(prompt, readyAttachments);

    setAgentChatError("");
    if (options.isGuidanceContinuation) {
      setAgentChatNotice(copy.chatWorkbench.guidanceRunningFeedback);
      setAgentGuidanceStatus("guiding");
    } else {
      setAgentChatNotice("");
      setAgentGuidanceStatus("idle");
      setQueuedAgentGuidanceValue("");
    }
    setAgentStopRequestedValue(false);

    if (!hasDesktopRuntime()) {
      const displayPrompt = options.displayPrompt?.trim() || prompt;
      updateActiveAgentMessages((currentMessages) => [
        ...currentMessages,
        {
          id: createClientId("user"),
          role: "user",
          content: displayPrompt,
          attachments: options.attachments
            ? conversationAttachmentMetadata(options.attachments)
            : undefined,
          created_at: new Date().toISOString(),
        },
        {
          id: createClientId("assistant"),
          role: "assistant",
          content: copy.chatWorkbench.desktopRuntimeMissing,
          created_at: new Date().toISOString(),
        },
      ], prompt);
      setAgentPrompt("");
      return;
    }

    const runToken = (agentChatRunTokenRef.current += 1);
    if (onboardingReadiness.overall !== "ready") {
      setPendingAgentPrompt(prompt);
      setDeepSeekApiKeyDraft("");
      setAgentSetupPrompt(
        onboardingReadiness.next_step === "ready"
          ? "doctor"
          : onboardingReadiness.next_step,
      );
      return;
    }

    const displayPrompt = options.displayPrompt?.trim() || prompt;
    if (agentChatPending && !options.isGuidanceContinuation && !options.runId) {
      let queuedRunId: string | null = null;
      try {
        const queuedConversation = agentConversations.find(
          (conversation) => conversation.id === activeAgentConversationId,
        );
        const queuedExecutionPrompt = buildAgentConversationContextPrompt(
          promptWithAttachments,
          agentMessagesRef.current,
          queuedConversation?.soul_profile_bootstrap ?? null,
        ).prompt;
        const queuedRecord = await invoke<AgentRunRecord>("enqueue_agent_run_record", {
          conversationId: activeAgentConversationId,
          prompt: displayPrompt,
          attachmentCount: readyAttachments.length,
          executionPrompt: queuedExecutionPrompt,
        });
        queuedRunId = queuedRecord.id;
        upsertAgentRunRecord(queuedRecord);
      } catch {
        // The local worker queue still preserves the user intent if the audit registry is unavailable.
      }

      queuedAgentPromptRef.current.push({
        prompt: promptValue,
        displayPrompt,
        attachments: options.attachments ?? [],
        runId: queuedRunId,
      });
      setAgentPrompt("");
      setAgentAttachments([]);
      setAgentAttachmentError("");
      setAgentChatError("");
      setAgentChatNotice(copy.chatWorkbench.taskQueuedFeedback);
      return;
    }

    const priorMessages = agentMessagesRef.current;
    const activeConversation = agentConversations.find(
      (conversation) => conversation.id === activeAgentConversationId,
    );
    let capturedSoulProfileBootstrap = activeConversation?.soul_profile_bootstrap || "";
    if (priorMessages.length === 0) {
      capturedSoulProfileBootstrap = agentSoulProfileBootstrapFromState(
        await loadSoulProfileStateForBootstrap(),
      );
    }
    if (priorMessages.length === 0) {
      setAgentConversations((currentConversations) =>
        currentConversations.map((conversation) =>
          conversation.id === activeAgentConversationId
            ? {
                ...conversation,
                soul_profile_bootstrap: capturedSoulProfileBootstrap || null,
              }
            : conversation,
        ),
      );
    }
    let agentRunId = options.runId ?? createClientId("run");
    if (!options.isGuidanceContinuation) {
      const existingRunRecord = options.runId
        ? agentRunRecords.find((record) => record.id === options.runId) ?? null
        : null;
      let nextAgentRun = createAgentChatRun({
        id: agentRunId,
        conversationId: activeAgentConversationId,
        prompt: displayPrompt,
        createdAt: new Date().toISOString(),
      });
      try {
        const queuedRecord = options.runId
          ? null
          : await invoke<AgentRunRecord>("enqueue_agent_run_record", {
              conversationId: activeAgentConversationId,
              prompt: displayPrompt,
              attachmentCount: readyAttachments.length,
              executionPrompt: null,
        });
        if (queuedRecord) {
          agentRunId = queuedRecord.id;
          nextAgentRun = agentChatRunFromRecord(queuedRecord, displayPrompt);
          upsertAgentRunRecord(queuedRecord);
        } else if (existingRunRecord) {
          nextAgentRun = agentChatRunFromRecord(existingRunRecord, displayPrompt);
        }
      } catch {
        // Local run state remains usable when the audit registry is unavailable.
      }
      setActiveAgentRun(nextAgentRun);
    }
    const userMessage: AgentConversationMessage = {
      id: createClientId("user"),
      role: "user",
      content: displayPrompt,
      attachments: options.attachments
        ? conversationAttachmentMetadata(options.attachments)
        : undefined,
      created_at: new Date().toISOString(),
    };

    setAgentPrompt("");
    setAgentAttachments([]);
    setAgentAttachmentError("");
    updateActiveAgentMessages((currentMessages) => [...currentMessages, userMessage], displayPrompt);
    setAgentChatPending(true);

    let runFinishedStatus: "completed" | "failed" = "completed";
    let runFinishError: string | null = null;
    let runFinishedByWorker = false;
    let finishedWorkerRecord: AgentRunRecord | null = null;
    try {
      const contextPacket = buildAgentConversationContextPrompt(
        promptWithAttachments,
        priorMessages,
        capturedSoulProfileBootstrap || null,
      );
      let response: AgentChatResponse;
      if (options.isGuidanceContinuation) {
        void invoke<AgentRunRecord>("record_agent_run_step_record", {
          runId: agentRunId,
          sequence: 1,
          status: "running",
          label: copy.runStatus.steps.deepseek,
          detail: agentChatPendingStatus,
        })
          .then(upsertAgentRunRecord)
          .catch(() => null);
        response = await invoke<AgentChatResponse>("run_agent_chat", {
          prompt: contextPacket.prompt,
          largeModelProvider: state.large_model_provider,
          modelRoute: state.model_route,
          thinkingLevel: state.thinking_level,
          accessMode: state.access_mode,
          networkSearchSourceModel: state.network_search_source_model || null,
        });
        void invoke<AgentRunRecord>("record_agent_run_step_record", {
          runId: agentRunId,
          sequence: 1,
          status: "completed",
          label: copy.runStatus.steps.deepseek,
          detail: response.model,
        })
          .then(upsertAgentRunRecord)
          .catch(() => null);
      } else {
        const workerResult = await invoke<AgentRunWorkerResult | null>(
          "run_next_queued_agent_chat_worker",
          {
            runId: agentRunId,
            executionPrompt: contextPacket.prompt,
            workerId: "desktop-chat-worker",
            largeModelProvider: state.large_model_provider,
            modelRoute: state.model_route,
            thinkingLevel: state.thinking_level,
            accessMode: state.access_mode,
            networkSearchSourceModel: state.network_search_source_model || null,
          },
        );
        if (!workerResult) {
          throw new Error("Queued agent run was not available for the background worker.");
        }
        runFinishedByWorker = true;
        finishedWorkerRecord = workerResult.record;
        agentRunId = workerResult.record.id;
        upsertAgentRunRecord(workerResult.record);
        response = workerResult.response;
      }
      if (runToken !== agentChatRunTokenRef.current || agentStopRequestedRef.current) {
        return;
      }
      await refreshSoulProfileAfterAgentResponse(response);
      if (contextPacket.compressed) {
        setAgentConversations((currentConversations) =>
          currentConversations.map((conversation) =>
            conversation.id === activeAgentConversationId
              ? { ...conversation, context_state: "compressed" }
              : conversation,
          ),
        );
      }
      if (response.subagent_plan.length === 0) {
        updateActiveAgentMessages((currentMessages) => [
          ...currentMessages,
          {
            id: response.id,
            role: "assistant",
            content: response.content,
            model: response.model,
            protocol_version: response.protocol_version,
            goal_envelope: response.goal_envelope,
            goal_projection: response.goal_projection,
            proposed_actions: response.proposed_actions,
            missing_prerequisites: response.missing_prerequisites,
            memory_candidates: response.memory_candidates,
            soul_profile_update: response.soul_profile_update,
            created_at: response.created_at,
          },
        ], displayPrompt);
      }
      const [telemetry, cacheState] = await Promise.all([
        invoke<DeepSeekChatTelemetry[]>("list_deepseek_chat_telemetry"),
        invoke<DeepSeekChatCacheState>("get_deepseek_chat_cache_state"),
      ]);
      setDeepSeekTelemetry(telemetry);
      setDeepSeekChatCacheState(cacheState);
      await Promise.all([
        refreshCapabilityState(),
        refreshSkillRecords(),
        runMemoryBackgroundMaintenance(),
      ]);
      const memories = await loadMemoryRecords(memoryQuery);
      setMemoryRecords(memories);
      const needsWorkspaceSetup = prerequisitesNeedWorkspaceSetup(response.missing_prerequisites);
      if (needsWorkspaceSetup && !options.skipWorkspaceSetup) {
        setPendingAgentPrompt(prompt);
        setAgentSetupPrompt("workspace");
        return;
      }
      const needsNetworkSearchSetup =
        prerequisitesNeedNetworkSearchSetup(response.missing_prerequisites) &&
        ((modelToolStrategy.network_search_source_model_required &&
          !state.network_search_source_model) ||
          !networkSearchRouteStatus.network_requests_enabled);
      if (needsNetworkSearchSetup && !options.skipNetworkSearchSetup) {
        setPendingAgentPrompt(prompt);
        setAgentSetupPrompt("network_search");
      }
    } catch (error) {
      if (runToken !== agentChatRunTokenRef.current || agentStopRequestedRef.current) {
        return;
      }
      const rawMessage = String(error);
      const message =
        rawMessage.includes("deepseek chat response could not be read") ||
        rawMessage.includes("error decoding response body")
          ? copy.chatWorkbench.deepSeekResponseReadFailed
          : copy.chatWorkbench.deepSeekRequestFailed;
      setAgentChatError("");
      if (options.isGuidanceContinuation) {
        void invoke<AgentRunRecord>("record_agent_run_step_record", {
          runId: agentRunId,
          sequence: 1,
          status: "failed",
          label: copy.runStatus.steps.deepseek,
          detail: message,
        })
          .then(upsertAgentRunRecord)
          .catch(() => null);
      } else {
        runFinishedByWorker = true;
        void refreshAgentRunRecords().catch(() => null);
      }
      updateActiveAgentMessages((currentMessages) => [
        ...currentMessages,
        {
          id: createClientId("assistant"),
          role: "assistant",
          content: message,
          run_error: message,
          created_at: new Date().toISOString(),
        },
      ], displayPrompt);
      runFinishedStatus = "failed";
      runFinishError = message;
    } finally {
      if (runToken !== agentChatRunTokenRef.current) {
        return;
      }

      const nextGuidance = queuedAgentGuidanceRef.current;
      if (nextGuidance && !agentStopRequestedRef.current) {
        setQueuedAgentGuidanceValue("");
        setAgentGuidanceStatus("guiding");
        setAgentChatNotice(copy.chatWorkbench.guidanceRunningFeedback);
        await sendAgentPrompt(buildAgentGuidancePrompt(nextGuidance), {
          ...options,
          isGuidanceContinuation: true,
          displayPrompt: nextGuidance,
          runId: agentRunId,
        });
        return;
      }

      if (!agentStopRequestedRef.current && !runFinishedByWorker) {
        const finishedRecord = await invoke<AgentRunRecord>("finish_agent_run_record", {
          runId: agentRunId,
          status: runFinishedStatus,
          summary: runFinishedStatus === "completed" ? "Agent chat run completed." : null,
          error: runFinishError,
        }).catch(() => null);
        if (finishedRecord) {
          upsertAgentRunRecord(finishedRecord);
        }
      }

      setActiveAgentRun((currentRun) =>
        currentRun?.id === agentRunId && !currentRun.cancel_requested
          ? finishedWorkerRecord
            ? agentChatRunFromRecord(finishedWorkerRecord, currentRun.prompt)
            : finishAgentRun(currentRun, {
                status: runFinishedStatus,
                finishedAt: new Date().toISOString(),
              })
          : currentRun,
      );
      setAgentChatPending(false);
      setAgentGuidanceStatus("idle");
      void refreshAgentRunRecords().catch(() => null);
      const nextQueuedPrompt = queuedAgentPromptRef.current.shift();
      if (nextQueuedPrompt && !agentStopRequestedRef.current) {
        window.setTimeout(() => {
          void sendAgentPrompt(nextQueuedPrompt.prompt, {
            displayPrompt: nextQueuedPrompt.displayPrompt,
            attachments: nextQueuedPrompt.attachments,
            runId: nextQueuedPrompt.runId ?? undefined,
          });
        }, 0);
      }
    }
  };

  const resolveAndResumeAgentActionGroup = async (
    messageId: string,
    actionEntries: Array<{ actionIndex: number; action: AgentChatActionProposal }>,
    approved: boolean,
  ) => {
    const actionKey = `${messageId}:approval-group`;
    setAgentActionPending(actionKey);
    setAgentChatError("");

    if (!hasDesktopRuntime()) {
      setAgentChatError(copy.chatWorkbench.desktopRuntimeMissing);
      setAgentActionPending(null);
      return;
    }

    try {
      for (const { action } of actionEntries) {
        if (!action.permission_request_id) {
          continue;
        }
        const approvalRecord = capabilityRecords.find(
          (record) => record.request.id === action.permission_request_id,
        );
        if (approvalRecord?.effective_status !== "pending_approval") {
          continue;
        }
        setResolutionPending(action.permission_request_id);
        await invoke("resolve_capability_access_request", {
          requestId: action.permission_request_id,
          approved,
          note: approved
            ? copy.chatWorkbench.confirmAndRun
            : copy.chatWorkbench.taskApprovalRejected,
        });
      }

      for (const { actionIndex, action } of actionEntries) {
        const approvalRecord = action.permission_request_id
          ? capabilityRecords.find(
              (record) => record.request.id === action.permission_request_id,
            )
          : null;
        const updatedAction: AgentChatActionProposal =
          !approved && approvalRecord?.effective_status !== "pending_approval"
            ? {
                ...action,
                execution_state: "blocked",
                blocked_reason: copy.chatWorkbench.taskApprovalRejected,
                dispatch_note: copy.chatWorkbench.taskApprovalRejected,
              }
            : await invoke<AgentChatActionProposal>("resume_agent_chat_action", {
                accessMode: state.access_mode,
                largeModelProvider: state.large_model_provider,
                networkSearchSourceModel: state.network_search_source_model || null,
                action,
              });
        updateActiveAgentMessages((currentMessages) =>
          currentMessages.map((message) => {
            if (message.id !== messageId || !message.proposed_actions) {
              return message;
            }
            return {
              ...message,
              proposed_actions: message.proposed_actions.map((currentAction, index) =>
                index === actionIndex ? updatedAction : currentAction,
              ),
            };
          }),
        );
      }
      await Promise.all([refreshCapabilityState(), refreshOperationsBriefingRuns()]);
    } catch (error) {
      await Promise.all([refreshCapabilityState(), refreshOperationsBriefingRuns()]).catch(
        () => null,
      );
      setAgentChatError(String(error) || copy.chatWorkbench.resumeActionFailed);
    } finally {
      setResolutionPending(null);
      setAgentActionPending(null);
    }
  };

  const sendAgentMessage = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (agentComposerAction === "stop") {
      requestAgentStop();
      return;
    }

    const attachmentsForSubmit = agentAttachments;
    const readyAttachmentsForSubmit = readyAgentAttachments(attachmentsForSubmit);
    const promptForSubmit =
      agentPrompt.trim() ||
      (readyAttachmentsForSubmit.length > 0 ? copy.chatWorkbench.attachmentsOnlyPrompt : agentPrompt);

    if (agentComposerAction === "send_guidance") {
      queueAgentGuidance(promptForSubmit, attachmentsForSubmit);
      return;
    }

    await sendAgentPrompt(promptForSubmit, { attachments: attachmentsForSubmit });
  };

  const continueAgentAfterDeepSeekKey = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    let submittedKey = deepSeekApiKeyDraft.trim();
    if (!submittedKey) {
      setAgentChatError(copy.chatWorkbench.deepSeekKeyRequired);
      return;
    }
    const readiness = await saveDeepSeekKey(submittedKey);
    submittedKey = "";
    if (!readiness) {
      return;
    }
    if (readiness.overall !== "ready") {
      setAgentSetupPrompt(readiness.next_step === "ready" ? "doctor" : readiness.next_step);
      return;
    }
    const prompt = pendingAgentPrompt;
    setAgentSetupPrompt(null);
    setPendingAgentPrompt("");
    await sendAgentPrompt(prompt, { attachments: agentAttachments });
  };

  const continueAgentAfterWorkspaceSetup = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const readiness = await persistLocalDirectorySetup();
    if (!readiness) {
      return;
    }
    if (readiness.overall !== "ready") {
      setAgentSetupPrompt(readiness.next_step === "ready" ? "doctor" : readiness.next_step);
      return;
    }
    const prompt = pendingAgentPrompt;
    setAgentSetupPrompt(null);
    setPendingAgentPrompt("");
    await sendAgentPrompt(prompt, {
      skipWorkspaceSetup: true,
      attachments: agentAttachments,
    });
  };

  const continueAgentAfterDoctor = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    let readiness: OnboardingReadinessProjection | null = null;
    if (deepSeekCredentialStatus.chat_completion_ready) {
      setDeepSeekReadinessPending(true);
      setDeepSeekReadinessError("");
      try {
        readiness = await invoke<OnboardingReadinessProjection>(
          "get_onboarding_readiness",
        );
        setOnboardingReadiness(readiness);
      } catch {
        setDeepSeekReadinessError(copy.onboarding.commandFailed);
      } finally {
        setDeepSeekReadinessPending(false);
      }
    } else {
      readiness = await retryDeepSeekReadiness();
    }
    if (!readiness) {
      return;
    }
    if (readiness.overall !== "ready") {
      setAgentSetupPrompt(readiness.next_step === "ready" ? "doctor" : readiness.next_step);
      return;
    }
    const prompt = pendingAgentPrompt;
    setAgentSetupPrompt(null);
    setPendingAgentPrompt("");
    await sendAgentPrompt(prompt, { attachments: agentAttachments });
  };

  const continueAgentAfterNetworkSearchSetup = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (modelToolStrategy.network_search_source_model_required && !state.network_search_source_model) {
      setAgentChatError(copy.networkSearchTool.sourceModelMissing);
      return;
    }
    const prompt = pendingAgentPrompt;
    setAgentSetupPrompt(null);
    setPendingAgentPrompt("");
    await sendAgentPrompt(prompt, {
      skipNetworkSearchSetup: true,
      attachments: agentAttachments,
    });
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

  const previewWorkPackageImport = async () => {
    clearPackageStatus();

    if (!importPackageJson.trim()) {
      setPackageError(copy.package.emptyImport);
      setImportPreview(null);
      return;
    }

    setPackagePending(true);
    try {
      const preview = await invoke<WorkPackageImportPreview>("preview_work_package_import", {
        packageJson: importPackageJson,
      });
      setImportPreview(preview);
      setPackageNotice(copy.package.previewReady);
    } catch (error) {
      setImportPreview(null);
      setPackageError(String(error) || copy.package.previewFailed);
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
      await runMemoryBackgroundMaintenance();
      const [records, memories, memoryCandidates, briefingRuns] = await Promise.all([
        invoke<TaskRecord[]>("list_task_records"),
        loadMemoryRecords(memoryQuery),
        invoke<MemoryCandidateRecord[]>("list_memory_candidate_records"),
        invoke<OperationsBriefingRun[]>("list_operations_briefing_runs"),
      ]);
      setTaskRecords(records);
      setMemoryRecords(memories);
      setMemoryCandidateRecords(memoryCandidates);
      setOperationsBriefingRuns(briefingRuns);
      setImportPackageJson("");
      setImportPreview(null);
      setPackageNotice(
        copy.package.imported(
          summary.imported,
          summary.skipped,
          summary.memory_candidates.imported,
          summary.memory_candidates.skipped,
          summary.operations_briefing_runs.imported,
          summary.operations_briefing_runs.skipped,
          summary.workflow_templates.imported,
          summary.workflow_templates.skipped,
        ),
      );
    } catch (error) {
      setPackageError(String(error));
    } finally {
      setPackagePending(false);
    }
  };

  const latestOperationsBriefingRun = operationsBriefingRuns[0];
  const latestConversationAgentRunStatus = latestParentAgentRunStatus(
    agentRunRecords,
    activeAgentConversationId,
  );
  const latestAgentRunCompleted = latestConversationAgentRunStatus === "completed";
  const latestRunFailed = latestOperationsBriefingRun?.status === "failed";
  const latestRunReady = latestOperationsBriefingRun?.status === "draft_ready";
  const latestOperationsRunNeedsApproval =
    latestOperationsBriefingRun?.status === "pending_approval";
  const latestAgentMessage = latestAssistantMessage(agentMessages);
  const latestAgentRunError = userFacingAgentRunError(latestAgentMessage, {
    requestFailed: copy.chatWorkbench.deepSeekRequestFailed,
    responseReadFailed: copy.chatWorkbench.deepSeekResponseReadFailed,
  });
  const latestAgentEnvelopeMessage = messageHasAgentEnvelope(latestAgentMessage)
    ? latestAgentMessage
    : undefined;
  const latestAgentActions = latestAgentEnvelopeMessage?.proposed_actions ?? [];
  const latestRunNeedsApproval =
    latestOperationsRunNeedsApproval ||
    latestAgentActions.some((action) => action.execution_state === "needs_confirmation");
  const latestAgentMissingPrerequisites =
    latestAgentEnvelopeMessage?.missing_prerequisites ?? [];
  const latestUserMessageWithAttachments = [...agentMessages]
    .reverse()
    .find((message) => message.role === "user" && (message.attachments?.length ?? 0) > 0);
  const visibleAgentAttachments = agentChatPending
    ? latestUserMessageWithAttachments?.attachments ?? []
    : agentAttachments;
  const visibleReadyAttachments = readyAgentAttachments(visibleAgentAttachments);
  const visibleMetadataOnlyAttachmentCount = visibleReadyAttachments.filter(
    (attachment) => !attachment.content_included,
  ).length;
  const visibleBlockedAttachmentCount = visibleAgentAttachments.filter(
    (attachment) => attachment.status === "blocked",
  ).length;
  const latestAgentHasExecutionPlan =
    latestAgentActions.length > 0 || latestAgentMissingPrerequisites.length > 0;
  const showAgentEnvelopeStatus =
    latestAgentHasExecutionPlan && !agentChatPending && !briefingPending;
  const showAgentErrorStatus =
    latestAgentRunError.length > 0 && !agentChatPending && !briefingPending;
  const latestAgentHasBlockedAction = latestAgentActions.some(
    (action) => action.execution_state === "blocked",
  );
  const latestAgentNeedsUserAction =
    latestAgentMissingPrerequisites.length > 0 ||
    latestAgentActions.some((action) => action.execution_state === "needs_confirmation");
  const latestAgentHasWaitingAction = latestAgentActions.some(
    (action) =>
      action.execution_state === "proposed" || action.execution_state === "waiting_prerequisite",
  );
  const recentAgentRunRecords = agentRunRecords
    .filter((record) => record.role === "parent")
    .sort((left, right) => new Date(right.updated_at).getTime() - new Date(left.updated_at).getTime())
    .slice(0, 3);
  const recentToolInvocations = [...toolInvocations]
    .sort(
      (left, right) =>
        new Date(right.finished_at ?? right.created_at).getTime() -
        new Date(left.finished_at ?? left.created_at).getTime(),
    )
    .slice(0, 4);
  const queuedAgentRunCount = agentRunRecords.filter((record) => record.status === "queued").length;
  const latestAgentAllActionsDone =
    latestAgentActions.length > 0 &&
    latestAgentActions.every((action) => action.execution_state === "succeeded");
  const runStatusTone: WorkflowStatusTone = agentChatPending || briefingPending
    ? "running"
    : showAgentErrorStatus ||
        (showAgentEnvelopeStatus && latestAgentHasBlockedAction) ||
        latestRunFailed
      ? "blocked"
      : (showAgentEnvelopeStatus && latestAgentNeedsUserAction) || latestRunNeedsApproval
        ? "needs_action"
        : (showAgentEnvelopeStatus && latestAgentAllActionsDone) ||
            latestAgentRunCompleted ||
            latestRunReady
          ? "done"
          : showAgentEnvelopeStatus && latestAgentHasWaitingAction
            ? "running"
            : "ready";
  const runStatusTitle = showAgentEnvelopeStatus
    ? copy.runStatus.agentActionTitle
    : runStatusTone === "running"
      ? copy.runStatus.runningTitle
      : runStatusTone === "blocked"
        ? copy.runStatus.failedTitle
        : runStatusTone === "needs_action"
          ? copy.runStatus.needsApprovalTitle
          : runStatusTone === "done"
            ? copy.runStatus.doneTitle
            : copy.runStatus.readyTitle;
  const runStatusBody = showAgentEnvelopeStatus
    ? copy.runStatus.agentActionBody
    : runStatusTone === "running"
      ? copy.runStatus.runningBody
      : runStatusTone === "blocked"
        ? copy.runStatus.failedBody
        : runStatusTone === "needs_action"
          ? copy.runStatus.needsApprovalBody
          : runStatusTone === "done"
            ? copy.runStatus.doneBody
            : copy.runStatus.readyBody;
  const latestRunContextReceipt = latestOperationsBriefingRun?.context_receipt;
  const renderLegacyCenterManagementPanels = false;
  const agentAttachmentSteps: WorkflowStep[] =
    visibleAgentAttachments.length > 0
      ? [
          {
            key: "agent-chat-attachments",
            label: copy.runStatus.steps.attachments,
            detail: copy.runStatus.stepDetails.attachments(
              visibleReadyAttachments.length,
              visibleMetadataOnlyAttachmentCount,
              visibleBlockedAttachmentCount,
            ),
            state:
              visibleBlockedAttachmentCount > 0
                ? "needs_action"
                : agentChatPending
                  ? "done"
                  : "current",
          },
        ]
      : [];
  const agentPendingSteps: WorkflowStep[] = [
    ...agentAttachmentSteps,
    ...agentChatLoopSteps({
    pending: agentChatPending,
    pendingStage: agentChatPendingStage,
    pendingStatus: agentChatPendingStatus,
    guidanceStatus: agentGuidanceStatus,
    queuedGuidance: queuedAgentGuidance,
    labels: {
      goal: copy.runStatus.steps.understand,
      execute: copy.runStatus.steps.deepseek,
      guidance: copy.runStatus.steps.guidance,
      verify: copy.runStatus.steps.validate,
    },
    details: {
      goal: copy.chatWorkbench.loopGoalDetail,
      verify: copy.chatWorkbench.loopVerifyDetail,
    },
  }),
  ];
  const agentStarterPrompts = [
    copy.chatWorkbench.quickAsk,
    copy.chatWorkbench.quickDraft,
    copy.chatWorkbench.quickAnalyze,
  ];
  const showAgentStarterPrompts =
    !agentChatPending &&
    agentMessages.length === 0 &&
    agentPrompt.trim().length === 0 &&
    agentAttachments.length === 0;
  const agentErrorSteps: WorkflowStep[] = showAgentErrorStatus
    ? [
        {
          key: "agent-chat-error",
          label: copy.runStatus.steps.deepseek,
          detail: latestAgentRunError,
          state: "blocked",
        },
      ]
    : [];
  const agentEnvelopeSteps: WorkflowStep[] = showAgentEnvelopeStatus
    ? [
        {
          key: "agent-model-envelope",
          label: copy.runStatus.steps.deepseek,
          detail: latestDeepSeekTelemetry
            ? latestDeepSeekTelemetryText
            : latestAgentEnvelopeMessage?.protocol_version ?? "ds-agent-envelope-v1",
          state: "done",
        },
        ...latestAgentMissingPrerequisites.map((prerequisite, index) => ({
          key: `agent-prerequisite-${index}`,
          label: copy.chatWorkbench.missingPrerequisitesLabel,
          detail: `${prerequisite.kind}: ${prerequisite.message}`,
          state: "needs_action" as WorkflowStepState,
        })),
        ...latestAgentActions.map((action, index) => ({
          key: `agent-action-${index}`,
          label: action.title || action.action_type,
          detail: userFacingAgentActionDetail(action) || action.action_type,
          state:
            action.execution_state === "blocked"
              ? ("blocked" as WorkflowStepState)
              : action.execution_state === "failed"
                ? ("blocked" as WorkflowStepState)
                : action.execution_state === "succeeded"
                  ? ("done" as WorkflowStepState)
                  : action.execution_state === "needs_confirmation"
                    ? ("needs_action" as WorkflowStepState)
                    : ("waiting" as WorkflowStepState),
        })),
      ]
    : [];
  const operationsRunStatusSteps: WorkflowStep[] = [
    {
      key: "understand",
      label: copy.runStatus.steps.understand,
      detail: latestRunContextReceipt?.user_intent ?? copy.runStatus.stepDetails.understand,
      state:
        latestAgentRunCompleted || latestOperationsBriefingRun || briefingPending
          ? "done"
          : "waiting",
    },
    {
      key: "evidence",
      label: copy.runStatus.steps.evidence,
      detail:
        latestRunContextReceipt?.selected_evidence[0] ??
        latestOperationsBriefingRun?.evidence_folder_path ??
        copy.runStatus.stepDetails.evidence,
      state: latestAgentRunCompleted
        ? "done"
        : latestOperationsRunNeedsApproval
        ? "needs_action"
        : latestOperationsBriefingRun
        ? "done"
        : briefingPending
          ? "current"
          : localDirectoryNeedsSetup
            ? "blocked"
            : "waiting",
    },
    {
      key: "memory",
      label: copy.runStatus.steps.memory,
      detail:
        latestRunContextReceipt?.selected_memories[0] ??
        copy.runStatus.stepDetails.memory,
      state: latestAgentRunCompleted
        ? "done"
        : latestOperationsBriefingRun
        ? "done"
        : briefingPending
          ? "current"
          : "waiting",
    },
    {
      key: "deepseek",
      label: copy.runStatus.steps.deepseek,
      detail: latestDeepSeekTelemetry
        ? latestDeepSeekTelemetryText
        : copy.runStatus.stepDetails.deepseek,
      state: latestAgentRunCompleted
        ? "done"
        : latestOperationsBriefingRun
        ? "done"
        : briefingPending
          ? "current"
          : idleDeepSeekStepState(deepSeekCredentialStatus.chat_completion_ready),
    },
    {
      key: "validate",
      label: copy.runStatus.steps.validate,
      detail:
        latestRunContextReceipt?.validation_results[0] ??
        copy.runStatus.stepDetails.validate,
      state: latestAgentRunCompleted
        ? "done"
        : latestRunFailed
        ? "blocked"
        : latestRunReady
          ? "done"
          : briefingPending
            ? "waiting"
            : "waiting",
    },
    {
      key: "report",
      label: copy.runStatus.steps.report,
      detail: latestRunFailed
        ? latestOperationsBriefingRun?.summary ?? copy.runStatus.stepDetails.report
        : copy.runStatus.stepDetails.report,
      state: latestAgentRunCompleted
        ? "done"
        : latestRunFailed
        ? "blocked"
        : latestRunReady
          ? "done"
          : latestOperationsRunNeedsApproval
            ? "needs_action"
            : "waiting",
    },
  ];
  const runStatusSteps = agentPendingSteps.length
    ? agentPendingSteps
    : agentErrorSteps.length
      ? agentErrorSteps
    : agentEnvelopeSteps.length
      ? agentEnvelopeSteps
      : operationsRunStatusSteps;
  const latestAgentHasOpenWork =
    latestAgentMissingPrerequisites.length > 0 ||
    latestAgentActions.some((action) => action.execution_state !== "succeeded");
  const shouldShowRunInspector =
    showAgentEnvelopeStatus ||
    showAgentErrorStatus ||
    visibleAgentAttachments.length > 0 ||
    agentChatPending ||
    agentRunRecords.length > 0 ||
    briefingPending ||
    latestRunNeedsApproval ||
    latestRunFailed ||
    latestAgentHasOpenWork;
  const visibleAgentConversations = sortAgentConversations(
    agentConversations.filter((conversation) => !conversation.archived),
  );
  const activeConversationMenuTarget = conversationMenu
    ? agentConversations.find(
        (conversation) => conversation.id === conversationMenu.conversationId,
      )
    : null;
  return (
    <main className="app-shell">
      <aside className="sidebar">
        <header className="sidebar-header">
          <div className="brand-row">
            <div className="brand" title={state.app_name}>
              <img className="brand-mark-image" src="/ds-agent-mark.png" alt={state.app_name} />
            </div>
            <div className="app-update-slot brand-update-slot">
              {appUpdateStatus.update_available ? (
                <div
                  className="app-update-stack"
                  title={
                    appUpdateError ||
                    appUpdateNotice ||
                    (appUpdateStatus.latest_version
                      ? `${state.app_name} ${appUpdateVersionLabel}`
                      : copy.appUpdate.update)
                  }
                >
                  {downloadedAppUpdateReady ? (
                    <button
                      className="app-update-button"
                      type="button"
                      disabled={appUpdateBusy}
                      onClick={() => void installAvailableAppUpdate()}
                    >
                      <Download size={14} aria-hidden="true" />
                      {appUpdateButtonLabel}
                    </button>
                  ) : null}
                  <span className={`app-update-version${appUpdateError ? " error" : ""}`}>
                    {appUpdateError || appUpdateVersionLabel}
                  </span>
                </div>
              ) : null}
            </div>
          </div>
          <div className="language-switch" role="group" aria-label={copy.controls.language}>
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
        <nav className="conversation-rail" aria-label={copy.nav.conversations}>
          <button className="nav-item new-chat-item" type="button" onClick={startNewAgentConversation}>
            <Pencil size={18} /> {copy.nav.newChat}
          </button>
          <div className="conversation-list">
            <div className="conversation-heading">
              <span>{copy.nav.conversations}</span>
              <span>{visibleAgentConversations.length}</span>
            </div>
            {visibleAgentConversations.map((conversation) => {
              const title = conversation.title || copy.nav.untitledConversation;
              const isRenaming = renamingConversationId === conversation.id;
              const isActive = conversation.id === activeAgentConversationId;
              return isRenaming ? (
                <form
                  className={
                    isActive
                      ? "conversation-item conversation-rename-form active"
                      : "conversation-item conversation-rename-form"
                  }
                  key={conversation.id}
                  onSubmit={submitRenameAgentConversation}
                >
                  <input
                    aria-label={copy.nav.renameConversation}
                    autoFocus
                    value={renameConversationTitle}
                    onChange={(event) => setRenameConversationTitle(event.target.value)}
                    onBlur={cancelRenameAgentConversation}
                    onKeyDown={(event) => {
                      if (event.key === "Escape") {
                        cancelRenameAgentConversation();
                        return;
                      }
                      if (event.key === "Enter") {
                        event.preventDefault();
                        saveRenameAgentConversation();
                      }
                    }}
                  />
                </form>
              ) : (
                <button
                  className={isActive ? "conversation-item active" : "conversation-item"}
                  type="button"
                  key={conversation.id}
                  onClick={() => openAgentConversation(conversation.id)}
                  onContextMenu={(event) => openAgentConversationMenu(event, conversation)}
                  title={title}
                >
                  <span className="conversation-title-line">
                    <span>{title}</span>
                    {conversation.pinned ? (
                      <Pin size={12} aria-label={copy.nav.pinned} />
                    ) : null}
                  </span>
                  <small>
                    {conversation.context_state === "compressed"
                      ? copy.nav.contextCompressed
                      : formatTaskDate(conversation.updated_at, language)}
                  </small>
                </button>
              );
            })}
          </div>
        </nav>

        {conversationMenu && activeConversationMenuTarget ? (
          <div
            className="conversation-context-menu"
            role="menu"
            style={{ left: conversationMenu.x, top: conversationMenu.y }}
          >
            <button
              type="button"
              role="menuitem"
              onClick={() => toggleAgentConversationPinned(activeConversationMenuTarget.id)}
            >
              <Pin size={14} aria-hidden="true" />
              {activeConversationMenuTarget.pinned ? copy.nav.unpin : copy.nav.pin}
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => archiveAgentConversation(activeConversationMenuTarget.id)}
            >
              <Archive size={14} aria-hidden="true" />
              {copy.nav.archive}
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => beginRenameAgentConversation(activeConversationMenuTarget)}
            >
              <Pencil size={14} aria-hidden="true" />
              {copy.nav.rename}
            </button>
          </div>
        ) : null}

        <section className="sidebar-tools" aria-label={copy.navLabel}>
          <details className="sidebar-tool sidebar-hub user-settings-hub">
            <summary>
              <MonitorCog size={16} aria-hidden="true" />
              <span>{copy.nav.settings}</span>
            </summary>
            <div className="sidebar-hub-body">
              <section
                className="sidebar-controls user-settings-controls"
                aria-label={copy.settingsPanel.title}
                data-settings-count={settingsPanelItemCount}
              >
                <form
                  className="deepseek-key-settings"
                  onSubmit={(event) => {
                    event.preventDefault();
                    if (deepSeekApiKeyDraft.trim()) {
                      void saveDeepSeekKey(deepSeekApiKeyDraft);
                    }
                  }}
                >
                  <label>
                    <span>{copy.settingsPanel.deepSeekApiKey}</span>
                  <div className="api-key-input-row">
                    <input
                      type="password"
                      value={deepSeekApiKeyDraft}
                      aria-label={copy.settingsPanel.deepSeekApiKey}
                      placeholder={primaryDeepSeekApiKeyPlaceholder}
                      autoComplete="off"
                      onChange={(event) => setDeepSeekApiKeyDraft(event.target.value)}
                    />
                    {primaryDeepSeekApiKeyReady ? (
                      <span
                        className="api-key-ready-indicator"
                        aria-label={copy.settingsPanel.apiKeyReady}
                        title={copy.settingsPanel.apiKeyReady}
                      >
                        √
                      </span>
                    ) : null}
                  </div>
                  </label>
                  <div className="deepseek-key-actions">
                    <button
                      type="submit"
                      disabled={deepSeekReadinessPending || !deepSeekApiKeyDraft.trim()}
                    >
                      {deepSeekReadinessPending
                        ? copy.onboarding.checking
                        : deepSeekCredentialStatus.configured
                          ? copy.onboarding.replaceKey
                          : copy.onboarding.saveAndCheck}
                    </button>
                    {deepSeekCredentialStatus.source === "stored" ? (
                      <button
                        type="button"
                        onClick={() => void removeDeepSeekKey()}
                        disabled={deepSeekReadinessPending}
                      >
                        {copy.onboarding.removeKey}
                      </button>
                    ) : null}
                  </div>
                </form>
                <label>
                  <span>{copy.controls.modelRoute}</span>
                  <select
                    value={state.model_route}
                    aria-label={copy.controls.modelRoute}
                    onChange={updateModelRoute}
                  >
                    <option value="auto">{copy.modelOptions.auto}</option>
                    <option value="flash">{copy.modelOptions.flash}</option>
                    <option value="pro">{copy.modelOptions.pro}</option>
                  </select>
                </label>
                <label>
                  <span>{copy.controls.thinkingLevel}</span>
                  <select
                    value={state.thinking_level}
                    aria-label={copy.controls.thinkingLevel}
                    onChange={updateThinkingLevel}
                  >
                    <option value="auto">{copy.thinkingOptions.auto}</option>
                    <option value="fast">{copy.thinkingOptions.fast}</option>
                    <option value="standard">{copy.thinkingOptions.standard}</option>
                    <option value="deep">{copy.thinkingOptions.deep}</option>
                  </select>
                </label>
                <label>
                  <span>{copy.controls.themeStyle}</span>
                  <select
                    value={themeStyle}
                    aria-label={copy.controls.themeStyle}
                    onChange={updateThemeStyle}
                  >
                    <option value="porcelain">{copy.themeOptions.porcelain}</option>
                    <option value="ink">{copy.themeOptions.ink}</option>
                  </select>
                </label>
                <div className="soul-profile-settings">
                  <div className="soul-profile-settings-actions">
                    <button
                      type="button"
                      onClick={() => setSoulProfileModalOpen(true)}
                      aria-label={copy.settingsPanel.soulProfileOpen}
                    >
                      <Brain size={14} aria-hidden="true" />
                      {copy.settingsPanel.soulProfile}
                    </button>
                  </div>
                </div>
                <div className="setup-form compact-settings-form">
                  <label>
                    <span>{copy.settingsPanel.workspaceDirectory}</span>
                    <div className="setup-field">
                      <input
                        value={setupWorkspaceDir}
                        aria-label={copy.settingsPanel.workspaceDirectory}
                        placeholder={
                          workspaceReadiness.workspace_root_display ||
                          copy.localSetup.workspacePlaceholder
                        }
                        readOnly
                      />
                      <button
                        type="button"
                        onClick={() => void chooseLocalDirectory({ autoSave: true })}
                        disabled={setupPending}
                      >
                        <FolderOpen size={14} aria-hidden="true" />
                        {setupPending ? copy.localSetup.saving : copy.settingsPanel.chooseWorkspace}
                      </button>
                    </div>
                  </label>
                  {setupNotice ? <p className="package-message">{setupNotice}</p> : null}
                  {setupError ? <p className="package-error">{setupError}</p> : null}
                </div>
                <div className="settings-balance-panel readiness-doctor-panel">
                  <div>
                    <span>{copy.onboarding.doctorTitle}</span>
                    <p className="setup-status">{deepSeekReadinessMessage}</p>
                    <p className="setup-status">{workspaceReadinessMessage}</p>
                    <p className="setup-status">
                      {copy.onboarding.credentialSource[deepSeekCredentialStatus.source]}
                    </p>
                  </div>
                  <button
                    type="button"
                    onClick={() => void retryDeepSeekReadiness()}
                    disabled={
                      deepSeekReadinessPending ||
                      !deepSeekCredentialStatus.configured ||
                      !canRetryDeepSeekReadiness
                    }
                  >
                    <Database size={14} aria-hidden="true" />
                    {deepSeekReadinessPending
                      ? copy.onboarding.checking
                      : copy.onboarding.retryCheck}
                  </button>
                  {deepSeekReadinessNotice ? (
                    <p className="package-message">{deepSeekReadinessNotice}</p>
                  ) : null}
                  {deepSeekReadinessError ? (
                    <p className="package-error">{deepSeekReadinessError}</p>
                  ) : null}
                </div>
                <p className="product-attribution">{copy.settingsPanel.attribution}</p>
              </section>
            </div>
          </details>
          <details className="sidebar-tool sidebar-hub plugins-hub" hidden={!exposePluginsSidebarEntry}>
            <summary>
              <PackageOpen size={16} aria-hidden="true" />
              <span>{copy.nav.plugins}</span>
            </summary>
            <div className="sidebar-hub-body">
          <details className="sidebar-tool operations-tool" open>
            <summary>
              <PackageOpen size={16} aria-hidden="true" />
              <span>{copy.skills.title}</span>
            </summary>
            <div className="skill-catalog">
              <p className="skill-catalog-note">{copy.skills.autoInvoke}</p>
              <p className="skill-update-status">
                <RefreshCw size={14} aria-hidden="true" />
                <span>
                  {skillUpdatePending
                    ? copy.skills.checkingUpdates
                    : skillUpdateSweep
                      ? copy.skills.updateSummary(
                          skillUpdateSweep.checked,
                          skillUpdateSweep.updated,
                          skillUpdateSweep.failed,
                        )
                      : copy.skills.automaticUpdates}
                </span>
              </p>
              {[
                { title: copy.skills.systemSkills, records: systemSkillRecords },
                { title: copy.skills.installedPlugins, records: installedPluginRecords },
                { title: copy.skills.installedSkills, records: installedSkillRecords },
              ].map((group) => (
                <section className="skill-catalog-group" key={group.title}>
                  <h4>{group.title}</h4>
                  <div className="skill-catalog-list">
                    {group.records.map((record) => (
                      <details className="skill-catalog-item" key={record.id}>
                        <summary>
                          <span>{record.manifest.name}</span>
                          <small>
                            {record.package_kind === "system_skill"
                              ? copy.skills.systemSkillType
                              : record.package_kind === "plugin"
                                ? copy.skills.pluginType
                                : copy.skills.skillType}
                          </small>
                        </summary>
                        <div className="skill-catalog-item-body">
                          <p>{record.manifest.description}</p>
                          <dl className="skill-catalog-meta">
                            <div>
                              <dt>{copy.skills.version}</dt>
                              <dd>{record.manifest.version}</dd>
                            </div>
                            <div>
                              <dt>{copy.skills.source}</dt>
                              <dd>
                                {record.source_identity?.repository_url ?? copy.skills.localSource}
                              </dd>
                            </div>
                          </dl>
                          <div className="skill-catalog-badges">
                            <span
                              className={
                                record.enablement_status === "enabled"
                                  ? "skill-status enabled"
                                  : "skill-status disabled"
                              }
                            >
                              {record.enablement_status === "enabled"
                                ? copy.skills.enabled
                                : copy.skills.disabled}
                            </span>
                            {record.system_protected ? (
                              <span className="skill-status protected">
                                <ShieldCheck size={12} aria-hidden="true" />
                                {copy.skills.protected}
                              </span>
                            ) : null}
                          </div>
                          {!record.system_protected ? (
                            <div className="skill-catalog-actions">
                              <button
                                type="button"
                                disabled={skillActionPending === record.id}
                                onClick={() =>
                                  void setLocalSkillEnabled(
                                    record,
                                    record.enablement_status !== "enabled",
                                  )
                                }
                              >
                                <Power size={13} aria-hidden="true" />
                                {record.enablement_status === "enabled"
                                  ? copy.skills.disable
                                  : copy.skills.enable}
                              </button>
                              <button
                                className="danger-button"
                                type="button"
                                disabled={skillActionPending === record.id}
                                onClick={() => void uninstallLocalSkill(record)}
                              >
                                <Trash2 size={13} aria-hidden="true" />
                                {copy.skills.uninstall}
                              </button>
                            </div>
                          ) : null}
                        </div>
                      </details>
                    ))}
                    {group.records.length === 0 && group.title !== copy.skills.systemSkills ? (
                      <p className="skill-catalog-empty">{copy.skills.empty}</p>
                    ) : null}
                  </div>
                </section>
              ))}
              <p className="skill-catalog-note">{copy.skills.installFromChat}</p>
              {skillNotice ? <p className="package-message">{skillNotice}</p> : null}
              {skillError ? <p className="package-error">{skillError}</p> : null}
            </div>
          </details>

          <details className="sidebar-tool operations-tool ordinary-user-hidden">
            <summary>
              <ClipboardList size={16} aria-hidden="true" />
              <span>{copy.skills.scenarioTemplates}</span>
            </summary>
            <div className="skill-catalog">
              <div className="skill-catalog-list">
                <details className="skill-catalog-item">
                  <summary>{copy.skills.operationsTitle}</summary>
                  <div className="skill-catalog-item-body">
                    <p>{copy.skills.operationsDescription}</p>
                  </div>
                </details>
              </div>
            </div>
          </details>

          <details className="sidebar-tool package-tool ordinary-user-hidden">
            <summary>
              <PackageOpen size={16} aria-hidden="true" />
              <span>{copy.package.title}</span>
            </summary>
            <div className="sidebar-action-stack">
              <button type="button" onClick={exportCurrentWorkPackage} disabled={packagePending}>
                <PackageOpen size={14} aria-hidden="true" />
                {copy.package.exportPackage}
              </button>
              <button type="button" onClick={copyCurrentWorkPackage} disabled={packagePending}>
                <Clipboard size={14} aria-hidden="true" />
                {copy.package.copyPackage}
              </button>
              <textarea
                value={importPackageJson}
                aria-label={copy.package.importJson}
                placeholder={copy.package.importJson}
                rows={4}
                onChange={(event) => {
                  setImportPackageJson(event.target.value);
                  setImportPreview(null);
                }}
              />
              <div className="sidebar-split-actions">
                <button type="button" onClick={previewWorkPackageImport} disabled={packagePending}>
                  <FileText size={14} aria-hidden="true" />
                  {copy.package.previewImport}
                </button>
                <button type="button" onClick={importWorkPackageJson} disabled={packagePending}>
                  <ArchiveRestore size={14} aria-hidden="true" />
                  {copy.package.importPackage}
                </button>
              </div>
              <div className="sidebar-compact-panel" aria-live="polite">
                <div className="queue-heading">
                  <strong>{copy.package.title}</strong>
                  <span>{taskRecords.length}</span>
                </div>
                {taskRecords.length === 0 ? (
                  <p className="empty-state">{copy.package.noRecords}</p>
                ) : (
                  <div className="sidebar-record-list">
                    {taskRecords.slice(0, 4).map((record) => (
                      <article className="sidebar-record-row single" key={record.id}>
                        <div>
                          <strong>{record.title}</strong>
                          {record.summary ? <p>{record.summary}</p> : null}
                        </div>
                        <time dateTime={record.created_at}>
                          {formatTaskDate(record.created_at, language)}
                        </time>
                      </article>
                    ))}
                  </div>
                )}
              </div>
              {exportedPackageJson ? (
                <textarea
                  className="package-json sidebar-package-json"
                  value={exportedPackageJson}
                  aria-label={copy.package.packageJson}
                  rows={4}
                  readOnly
                />
              ) : null}
              {importPreview ? (
                <section className="import-preview sidebar-import-preview" aria-labelledby="sidebar-import-preview-title">
                  <strong id="sidebar-import-preview-title">{copy.package.previewTitle}</strong>
                  <div>
                    <span>
                      {copy.package.previewTotalTasks}: {importPreview.task_records.total}
                    </span>
                    <span>
                      {copy.package.previewNewTasks}: {importPreview.task_records.new}
                    </span>
                    <span>
                      {copy.package.previewMemoryCandidates}:{" "}
                      {importPreview.memory_candidates.total}
                    </span>
                    <span>
                      {copy.package.previewArchivedRuns}:{" "}
                      {importPreview.operations_briefing_runs.total}
                    </span>
                    <span>
                      {copy.package.previewWorkflowTemplates}:{" "}
                      {importPreview.workflow_templates.total}
                    </span>
                  </div>
                  <p>{copy.package.previewMemoryCandidateHint}</p>
                  <p>{copy.package.previewArchiveHint}</p>
                </section>
              ) : null}
            </div>
            {packageNotice ? <p className="package-message">{packageNotice}</p> : null}
            {packageError ? <p className="package-error">{packageError}</p> : null}
          </details>

            </div>
          </details>

          <details
            className="sidebar-tool sidebar-hub settings-hub legacy-settings-hub"
          >
            <summary>
              <MonitorCog size={16} aria-hidden="true" />
              <span>{copy.nav.settings}</span>
            </summary>
            <div className="sidebar-hub-body">
          <details
            className="sidebar-tool memory-tool"
            ref={(node) => {
              memorySectionRef.current = node;
            }}
          >
            <summary>
              <Database size={16} aria-hidden="true" />
              <span>{copy.memory.title}</span>
            </summary>
            <div className="sidebar-action-stack">
              <form className="memory-search sidebar-memory-search" onSubmit={searchMemoryRecords}>
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
              {memoryRecords.length >= 2 ? (
                <form className="memory-link-form sidebar-memory-link-form" onSubmit={linkExistingMemoryRecords}>
                  <select
                    value={memoryLinkSourceId}
                    aria-label={copy.memory.linkSource}
                    onChange={(event) => setMemoryLinkSourceId(event.target.value)}
                    disabled={memoryExistingLinkPending}
                  >
                    <option value="">{copy.memory.linkSource}</option>
                    {memoryRecords.map((memory) => (
                      <option key={memory.id} value={memory.id}>
                        {memory.title}
                      </option>
                    ))}
                  </select>
                  <select
                    value={memoryLinkTargetId}
                    aria-label={copy.memory.linkTarget}
                    onChange={(event) => setMemoryLinkTargetId(event.target.value)}
                    disabled={memoryExistingLinkPending}
                  >
                    <option value="">{copy.memory.linkTarget}</option>
                    {memoryRecords.map((memory) => (
                      <option key={memory.id} value={memory.id}>
                        {memory.title}
                      </option>
                    ))}
                  </select>
                  <button type="submit" disabled={memoryExistingLinkPending}>
                    <Link2 size={15} aria-hidden="true" />
                    {memoryExistingLinkPending
                      ? copy.memory.linkingExisting
                      : copy.memory.linkExisting}
                  </button>
                </form>
              ) : null}
              {memoryNotice ? <p className="package-message">{memoryNotice}</p> : null}
              {memoryError ? <p className="package-error">{memoryError}</p> : null}
              <div className="sidebar-compact-panel">
                <div className="queue-heading">
                  <strong>{copy.memory.title}</strong>
                  <span>{memoryRecords.length}</span>
                </div>
                {memoryRecords.length === 0 ? (
                  <p className="empty-state">{copy.memory.noMemories}</p>
                ) : (
                  <div className="sidebar-record-list">
                    {memoryRecords.slice(0, 5).map((memory) => (
                      <article className="sidebar-record-row single" key={memory.id}>
                        <div>
                          <strong>{memory.title}</strong>
                          <p>{memory.body}</p>
                          <div className="memory-meta">
                            <span>{copy.memory.typeOptions[memory.memory_type]}</span>
                            <span>{copy.memory.scopeOptions[memory.scope]}</span>
                            <span>{copy.memory.lifecycleOptions[memory.lifecycle]}</span>
                          </div>
                        </div>
                      </article>
                    ))}
                  </div>
                )}
              </div>
              <div className="sidebar-compact-panel">
                <div className="queue-heading">
                  <strong>{copy.memory.candidates}</strong>
                  <span>{memoryCandidateRecords.length}</span>
                </div>
                {memoryCandidateNotice ? (
                  <p className="package-message">{memoryCandidateNotice}</p>
                ) : null}
                {memoryCandidateError ? (
                  <p className="package-error">{memoryCandidateError}</p>
                ) : null}
                {memoryCandidateRecords.length === 0 ? (
                  <p className="empty-state">{copy.memory.noCandidates}</p>
                ) : (
                  <div className="sidebar-record-list">
                    {memoryCandidateRecords.slice(0, 4).map((record) => (
                      <article className="sidebar-record-row candidate" key={record.candidate.id}>
                        <div>
                          <strong>{record.candidate.title}</strong>
                          <p>{record.candidate.body}</p>
                        </div>
                        <span className={`access-status ${record.effective_status}`}>
                          {copy.memory.candidateStatus[record.effective_status]}
                        </span>
                        {record.effective_status === "pending" ? (
                          <p className="memory-feedback-note">
                            {copy.memory.maintenanceAutomatic}:{" "}
                            {copy.memory.maintenanceNoUserAction}
                          </p>
                        ) : null}
                      </article>
                    ))}
                  </div>
                )}
              </div>
            </div>
          </details>

          <details className="sidebar-tool settings-tool" open={localDirectoryNeedsSetup}>
            <summary>
              <MonitorCog size={16} aria-hidden="true" />
              <span>{copy.inspector.title}</span>
            </summary>
            <section className="sidebar-controls" aria-label={copy.inspector.title}>
              <label>
                <span>{copy.controls.largeModelProvider}</span>
                <select
                  value={state.large_model_provider}
                  aria-label={copy.controls.largeModelProvider}
                  onChange={updateLargeModelProvider}
                >
                  <option value="deepseek">{copy.largeModelOptions.deepseek}</option>
                  <option value="chatgpt">{copy.largeModelOptions.chatgpt}</option>
                  <option value="codex">{copy.largeModelOptions.codex}</option>
                  <option value="custom">{copy.largeModelOptions.custom}</option>
                </select>
              </label>
              <label>
                <span>{copy.controls.modelRoute}</span>
                <select
                  value={state.model_route}
                  aria-label={copy.controls.modelRoute}
                  onChange={updateModelRoute}
                >
                  <option value="auto">{copy.modelOptions.auto}</option>
                  <option value="flash">{copy.modelOptions.flash}</option>
                  <option value="pro">{copy.modelOptions.pro}</option>
                </select>
              </label>
              <label>
                <span>{copy.controls.accessMode}</span>
                <select
                  value={state.access_mode}
                  aria-label={copy.controls.accessMode}
                  onChange={updateAccessMode}
                >
                  <option value="ask_every_step">{copy.accessOptions.ask_every_step}</option>
                  <option value="ask_on_risk">{copy.accessOptions.ask_on_risk}</option>
                  <option value="limited_auto">{copy.accessOptions.limited_auto}</option>
                  <option value="full_access">{copy.accessOptions.full_access}</option>
                </select>
              </label>
              <label>
                <span>{copy.controls.thinkingLevel}</span>
                <select
                  value={state.thinking_level}
                  aria-label={copy.controls.thinkingLevel}
                  onChange={updateThinkingLevel}
                >
                  <option value="auto">{copy.thinkingOptions.auto}</option>
                  <option value="fast">{copy.thinkingOptions.fast}</option>
                  <option value="standard">{copy.thinkingOptions.standard}</option>
                  <option value="deep">{copy.thinkingOptions.deep}</option>
                </select>
              </label>
              <label>
                <span>{copy.controls.themeStyle}</span>
                <select
                  value={themeStyle}
                  aria-label={copy.controls.themeStyle}
                  onChange={updateThemeStyle}
                >
                  <option value="porcelain">{copy.themeOptions.porcelain}</option>
                  <option value="ink">{copy.themeOptions.ink}</option>
                </select>
              </label>
            </section>
            <details
              className={
                localDirectoryNeedsSetup
                  ? "setup-disclosure setup-required"
                  : "setup-disclosure"
              }
              open={localDirectoryNeedsSetup}
            >
              <summary className="section-heading">
                <FolderOpen size={18} aria-hidden="true" />
                <span>{copy.localSetup.title}</span>
                <small>
                  {localDirectoryNeedsSetup
                    ? copy.localSetup.required
                    : copy.localSetup.ready}
                </small>
              </summary>
              <div className="setup-disclosure-body">
                <form className="setup-form" onSubmit={saveLocalDirectorySetup}>
                  <label>
                    <span>{copy.localSetup.workspaceName}</span>
                    <input
                      value={setupWorkspaceName}
                      aria-label={copy.localSetup.workspaceName}
                      placeholder={copy.localSetup.workspaceNamePlaceholder}
                      onChange={(event) => setSetupWorkspaceName(event.target.value)}
                    />
                  </label>
                  <label>
                    <span>{copy.localSetup.workspaceDir}</span>
                    <div className="setup-field">
                      <input
                        value={setupWorkspaceDir}
                        aria-label={copy.localSetup.workspaceDir}
                        placeholder={copy.localSetup.workspacePlaceholder}
                        onChange={(event) => setSetupWorkspaceDir(event.target.value)}
                      />
                      <button
                        type="button"
                        onClick={() => void chooseLocalDirectory()}
                      >
                        <FolderOpen size={14} aria-hidden="true" />
                        {copy.localSetup.choose}
                      </button>
                    </div>
                  </label>
                  <p className="setup-help">{copy.localSetup.managedStructure}</p>
                  <button type="submit" disabled={setupPending}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {setupPending ? copy.localSetup.saving : copy.localSetup.save}
                  </button>
                </form>
                {setupNotice ? <p className="package-message">{setupNotice}</p> : null}
                {setupError ? <p className="package-error">{setupError}</p> : null}
              </div>
            </details>

            <details className="setup-disclosure">
              <summary className="section-heading">
                <Database size={18} aria-hidden="true" />
                <span>{copy.deepSeekPricing.title}</span>
                <small>
                  {deepSeekPricingState.pricing_configured
                    ? copy.deepSeekPricing.statusConfigured
                    : copy.deepSeekPricing.statusNotConfigured}
                </small>
              </summary>
              <div className="setup-disclosure-body">
                <p className="setup-status" title={deepSeekPricingState.note}>
                  {deepSeekPricingState.note}
                </p>
                <form className="setup-form" onSubmit={saveDeepSeekPricingSetup}>
                  <label className="setup-checkbox">
                    <input
                      type="checkbox"
                      checked={deepSeekPricingEnabled}
                      onChange={(event) => setDeepSeekPricingEnabled(event.target.checked)}
                    />
                    <span>{copy.deepSeekPricing.enabled}</span>
                  </label>
                  <p className="setup-status">{copy.deepSeekPricing.help}</p>
                  <label>
                    <span>{copy.deepSeekPricing.flashPrompt}</span>
                    <input
                      type="number"
                      min="0"
                      step="0.000001"
                      value={deepSeekFlashPromptPrice}
                      aria-label={copy.deepSeekPricing.flashPrompt}
                      placeholder={copy.deepSeekPricing.pricePlaceholder}
                      onChange={(event) => setDeepSeekFlashPromptPrice(event.target.value)}
                    />
                  </label>
                  <label>
                    <span>{copy.deepSeekPricing.flashCompletion}</span>
                    <input
                      type="number"
                      min="0"
                      step="0.000001"
                      value={deepSeekFlashCompletionPrice}
                      aria-label={copy.deepSeekPricing.flashCompletion}
                      placeholder={copy.deepSeekPricing.pricePlaceholder}
                      onChange={(event) => setDeepSeekFlashCompletionPrice(event.target.value)}
                    />
                  </label>
                  <label>
                    <span>{copy.deepSeekPricing.proPrompt}</span>
                    <input
                      type="number"
                      min="0"
                      step="0.000001"
                      value={deepSeekProPromptPrice}
                      aria-label={copy.deepSeekPricing.proPrompt}
                      placeholder={copy.deepSeekPricing.pricePlaceholder}
                      onChange={(event) => setDeepSeekProPromptPrice(event.target.value)}
                    />
                  </label>
                  <label>
                    <span>{copy.deepSeekPricing.proCompletion}</span>
                    <input
                      type="number"
                      min="0"
                      step="0.000001"
                      value={deepSeekProCompletionPrice}
                      aria-label={copy.deepSeekPricing.proCompletion}
                      placeholder={copy.deepSeekPricing.pricePlaceholder}
                      onChange={(event) => setDeepSeekProCompletionPrice(event.target.value)}
                    />
                  </label>
                  <button type="submit" disabled={deepSeekPricingPending}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {deepSeekPricingPending
                      ? copy.deepSeekPricing.saving
                      : copy.deepSeekPricing.save}
                  </button>
                </form>
                {deepSeekPricingNotice ? (
                  <p className="package-message">{deepSeekPricingNotice}</p>
                ) : null}
                {deepSeekPricingError ? (
                  <p className="package-error">{deepSeekPricingError}</p>
                ) : null}
              </div>
            </details>
          </details>
            </div>
          </details>
        </section>
      </aside>

      <section className="workspace">

        <section
          className={`workbench ${shouldShowRunInspector ? "has-inspector" : "chat-only"}`}
          ref={workbenchSectionRef}
        >
          <div className="timeline">
            <section className="agent-chat-panel" aria-label={copy.chatWorkbench.title}>
              <div className="chat-thread" ref={chatThreadRef} aria-live="polite">
                {agentMessages.map((message) => {
                  const visibleProposedActions =
                    message.role === "assistant"
                      ? (message.proposed_actions ?? []).filter(shouldShowAgentActionInChat)
                      : [];
                  const messageApprovalActions = (message.proposed_actions ?? [])
                    .map((action, actionIndex) => ({ action, actionIndex }))
                    .filter(({ action }) => action.execution_state === "needs_confirmation");
                  const messageApprovalRecords = messageApprovalActions.map(({ action }) =>
                    action.permission_request_id
                      ? capabilityRecords.find(
                          (record) => record.request.id === action.permission_request_id,
                        )
                      : null,
                  );
                  const taskGroupedAuthorization = message.goal_projection
                    ? taskGroupedAuthorizations.find(
                        (authorization) =>
                          authorization.intent.task_id === message.goal_projection?.goal_id,
                      )
                    : undefined;
                  const taskApprovalReady =
                    !taskGroupedAuthorization &&
                    messageApprovalActions.length > 0 &&
                    messageApprovalActions.every(({ action }, index) => {
                      if (!action.permission_request_id) {
                        return true;
                      }
                      const record = messageApprovalRecords[index];
                      return (
                        record?.effective_status === "pending_approval" ||
                        (record?.effective_status === "approved" &&
                          (record.grant_state === "reusable" ||
                            record.grant_state === "one_shot_available"))
                      );
                    });
                  const taskApprovalHasPending = messageApprovalActions.some(
                    ({ action }, index) =>
                      !action.permission_request_id ||
                      messageApprovalRecords[index]?.effective_status === "pending_approval",
                  );
                  const taskApprovalKey = `${message.id}:approval-group`;

                  return (
                  <article className={`chat-message ${message.role}`} key={message.id}>
                    {message.role === "assistant" ? (
                      <div className="chat-avatar" aria-hidden="true">
                        <Brain size={16} />
                      </div>
                    ) : null}
                    <div className="chat-bubble">
                      {message.role === "assistant" ? (
                        <span>{message.model ?? copy.chatWorkbench.assistantLabel}</span>
                      ) : null}
                      <p>
                        {userFacingAgentMessageContent(message, {
                          responseReadFailed: copy.chatWorkbench.deepSeekResponseReadFailed,
                        })}
                      </p>
                      {message.attachments?.length ? (
                        <div className="attachment-strip sent">
                          {message.attachments.map((attachment) => (
                            <div
                              className={`attachment-chip ${attachment.kind} ${attachment.status}`}
                              key={`${message.id}-${attachment.id}`}
                              title={attachment.blocked_reason ?? attachment.name}
                            >
                              <span className="attachment-thumbnail" aria-hidden="true">
                                {attachment.kind === "image" && attachment.status === "ready" ? (
                                  <img src={convertFileSrc(attachment.local_path)} alt="" />
                                ) : (
                                  <FileText size={15} aria-hidden="true" />
                                )}
                              </span>
                              <span className="attachment-details">
                                <strong>{attachment.name}</strong>
                                <small>
                                  {attachment.kind} / {Math.ceil(attachment.byte_size / 1024)} KB
                                  {!attachment.content_included
                                    ? ` / ${copy.chatWorkbench.attachmentMetadataOnly}`
                                    : ""}
                                </small>
                              </span>
                            </div>
                          ))}
                        </div>
                      ) : null}
                      {message.role === "assistant" &&
                      (message.goal_projection || message.goal_envelope) ? (
                        <div className="agent-action-list">
                          <strong>{language === "zh" ? "目标契约" : "Goal contract"}</strong>
                          <ul>
                            <li>
                              <span
                                className={`agent-action-state ${goalEnvelopeStatusTone(
                                  message.goal_projection?.status ?? "proposed",
                                )}`}
                              >
                                {goalEnvelopeStatusLabel(
                                  message.goal_projection?.status ?? "proposed",
                                  language,
                                )}
                              </span>
                              <p>
                                {message.goal_projection?.user_goal_summary ??
                                  message.goal_envelope?.user_goal}
                              </p>
                              {message.goal_projection ? (
                                <small>
                                  {language === "zh" ? "完成条件" : "Done when"}: {message.goal_projection.done_when_count}
                                  {` / ${language === "zh" ? "产物" : "artifacts"}: ${message.goal_projection.required_artifact_count}`}
                                  {` / ${language === "zh" ? "验证器" : "verifiers"}: ${message.goal_projection.verifier_count}`}
                                </small>
                              ) : null}
                            </li>
                          </ul>
                          {message.goal_projection?.reason_codes.length ? (
                            <p>
                              {language === "zh" ? "原因码" : "Reason codes"}: {message.goal_projection.reason_codes.join(", ")}
                            </p>
                          ) : null}
                          {message.goal_projection?.revision ? (
                            <small>
                              revision: {message.goal_projection.revision}
                              {` / fingerprint: ${message.goal_projection.fingerprint}`}
                            </small>
                          ) : null}
                        </div>
                      ) : null}
                      {message.role === "assistant" && taskGroupedAuthorization ? (
                        <TaskGroupedAuthorizationCard
                          authorization={taskGroupedAuthorization}
                          language={language}
                          capabilityLabels={copy.capabilityOptions}
                          riskLabels={copy.riskOptions}
                          pending={
                            taskGroupedAuthorizationPending ===
                            taskGroupedAuthorization.intent.group_id
                          }
                          onResolve={(intent, approved) =>
                            void resolveTaskGroupedAuthorization(intent, approved)
                          }
                          onRevoke={(intent) => void revokeTaskGroupedAuthorization(intent)}
                        />
                      ) : null}
                      {message.role === "assistant" && message.missing_prerequisites?.length ? (
                        <div className="agent-action-list">
                          <strong>{copy.chatWorkbench.missingPrerequisitesLabel}</strong>
                          <ul>
                            {message.missing_prerequisites.map((prerequisite, index) => (
                              <li key={`${message.id}-prerequisite-${index}`}>
                                <span>{prerequisite.kind}</span>
                                <p>{prerequisite.message}</p>
                              </li>
                            ))}
                          </ul>
                        </div>
                      ) : null}
                      {visibleProposedActions.length ? (
                        <div className="agent-action-list">
                          <strong>{copy.chatWorkbench.actionPlanLabel}</strong>
                          <ul>
                            {visibleProposedActions.map((action) => {
                              const actionIndex = message.proposed_actions?.indexOf(action) ?? -1;
                              const actionDetail = userFacingAgentActionDetail(action);
                              return (
                                <li key={`${message.id}-action-${actionIndex}`}>
                                  <span className={`agent-action-state ${action.execution_state}`}>
                                    {copy.chatWorkbench.actionState[action.execution_state]}
                                  </span>
                                  <p>
                                    {action.title || action.action_type}
                                    {action.target ? ` · ${action.target}` : ""}
                                  </p>
                                  {actionDetail ? <small>{actionDetail}</small> : null}
                                </li>
                              );
                            })}
                          </ul>
                          {taskApprovalReady ? (
                            <div className="agent-action-approval">
                              <p>
                                {copy.chatWorkbench.approvalSummary(
                                  messageApprovalActions.length,
                                )}
                              </p>
                              <div className="agent-action-approval-buttons">
                                <button
                                  className="agent-action-resume"
                                  type="button"
                                  disabled={
                                    agentActionPending !== null || resolutionPending !== null
                                  }
                                  onClick={() =>
                                    void resolveAndResumeAgentActionGroup(
                                      message.id,
                                      messageApprovalActions,
                                      true,
                                    )
                                  }
                                >
                                  <Check size={13} aria-hidden="true" />
                                  {agentActionPending === taskApprovalKey
                                    ? copy.chatWorkbench.confirmingAction
                                    : taskApprovalHasPending
                                      ? copy.chatWorkbench.confirmAndRun
                                      : copy.chatWorkbench.resumeAction}
                                </button>
                                {taskApprovalHasPending ? (
                                  <button
                                    className="agent-action-reject"
                                    type="button"
                                    disabled={
                                      agentActionPending !== null || resolutionPending !== null
                                    }
                                    onClick={() =>
                                      void resolveAndResumeAgentActionGroup(
                                        message.id,
                                        messageApprovalActions,
                                        false,
                                      )
                                    }
                                  >
                                    <X size={13} aria-hidden="true" />
                                    {copy.capabilities.reject}
                                  </button>
                                ) : null}
                              </div>
                            </div>
                          ) : null}
                        </div>
                      ) : null}
                      {message.role === "assistant" && message.memory_candidates?.length ? (
                        <div className="agent-action-list">
                          <strong>{copy.chatWorkbench.memoryCandidatesLabel}</strong>
                          <ul>
                            {message.memory_candidates.map((candidate) => (
                              <li key={candidate.id}>
                                <span>{candidate.title}</span>
                                <p>{candidate.body}</p>
                                {candidate.rationale ? <small>{candidate.rationale}</small> : null}
                              </li>
                            ))}
                          </ul>
                        </div>
                      ) : null}
                    </div>
                  </article>
                  );
                })}
                {agentChatPending ? (
                  <article className="chat-message assistant pending">
                    <div className="chat-avatar" aria-hidden="true">
                      <Brain size={16} />
                    </div>
                    <div className="chat-bubble">
                      <span>{copy.chatWorkbench.assistantLabel}</span>
                      <p>{agentChatPendingStatus}</p>
                    </div>
                  </article>
                ) : null}
              </div>

              <div className="chat-input-dock">
                <form
                  className={`chat-composer${agentAttachmentDragActive ? " drag-active" : ""}`}
                  ref={chatComposerRef}
                  onSubmit={sendAgentMessage}
                >
                  {agentAttachments.length > 0 ? (
                    <div className="attachment-strip composer">
                      {agentAttachments.map((attachment) => (
                        <div
                          className={`attachment-chip ${attachment.kind} ${attachment.status}`}
                          key={attachment.id}
                          title={attachment.blocked_reason ?? attachment.name}
                        >
                          <span className="attachment-thumbnail" aria-hidden="true">
                            {attachment.kind === "image" && attachment.status === "ready" ? (
                              <img src={convertFileSrc(attachment.local_path)} alt="" />
                            ) : (
                              <FileText size={15} aria-hidden="true" />
                            )}
                          </span>
                          <span className="attachment-details">
                            <strong>{attachment.name}</strong>
                            <small>
                              {attachment.status === "blocked"
                                ? copy.chatWorkbench.attachmentBlocked
                                : `${attachment.kind} / ${Math.ceil(attachment.byte_size / 1024)} KB${
                                    !attachment.content_included
                                      ? ` / ${copy.chatWorkbench.attachmentMetadataOnly}`
                                      : ""
                                  }`}
                            </small>
                          </span>
                          <button
                            type="button"
                            className="attachment-remove"
                            aria-label={`${copy.chatWorkbench.removeAttachment}: ${attachment.name}`}
                            onClick={() => removeAgentAttachment(attachment.id)}
                          >
                            <X size={12} aria-hidden="true" />
                          </button>
                        </div>
                      ))}
                    </div>
                  ) : null}
                  {showAgentStarterPrompts ? (
                    <div
                      className="starter-prompts"
                      aria-label={copy.chatWorkbench.starterPromptsLabel}
                    >
                      {agentStarterPrompts.map((prompt) => (
                        <button
                          type="button"
                          className="starter-prompt"
                          key={prompt}
                          onClick={() => setAgentPrompt(prompt)}
                        >
                          {prompt}
                        </button>
                      ))}
                    </div>
                  ) : null}
                  <textarea
                    value={agentPrompt}
                    aria-label={copy.chatWorkbench.composerPlaceholder}
                    placeholder={copy.chatWorkbench.composerPlaceholder}
                    rows={4}
                    onChange={(event) => setAgentPrompt(event.target.value)}
                    onKeyDown={(event) => {
                      if (
                        event.key !== "Enter" ||
                        event.nativeEvent.isComposing
                      ) {
                        return;
                      }
                      if (event.ctrlKey) {
                        event.preventDefault();
                        const textarea = event.currentTarget;
                        const cursorStart = textarea.selectionStart;
                        const cursorEnd = textarea.selectionEnd;
                        const nextPrompt = `${agentPrompt.slice(0, cursorStart)}\n${agentPrompt.slice(
                          cursorEnd,
                        )}`;
                        const nextCursor = cursorStart + 1;
                        setAgentPrompt(nextPrompt);
                        window.requestAnimationFrame(() => {
                          textarea.setSelectionRange(nextCursor, nextCursor);
                        });
                        return;
                      }
                      if (event.metaKey || event.altKey || event.shiftKey) {
                        return;
                      }
                      event.preventDefault();
                      if (!agentChatPending || agentPrompt.trim() || readyAgentAttachmentCount > 0) {
                        event.currentTarget.form?.requestSubmit();
                      }
                    }}
                  />
                  <div className="composer-actions">
                    {agentChatPending || readyAgentAttachmentCount > 0 ? (
                      <span>
                        {agentChatPending
                          ? agentChatPendingStatus
                          : summarizeAttachmentsForDisplay(agentAttachments)}
                      </span>
                    ) : null}
                    <button
                      type="button"
                      className="secondary-action attachment-add"
                      onClick={() => void selectAgentAttachments()}
                    >
                      <Paperclip size={15} aria-hidden="true" />
                      {copy.chatWorkbench.addAttachment}
                    </button>
                    {agentChatPending && (agentPrompt.trim() || readyAgentAttachmentCount > 0) ? (
                      <button
                        type="button"
                        className="secondary-action"
                        onClick={() => queueAgentGuidance(agentPrompt, agentAttachments)}
                      >
                        <Plus size={15} aria-hidden="true" />
                        {copy.chatWorkbench.queueGuidance}
                      </button>
                    ) : null}
                    {showAgentStopControl ? (
                      <button
                        type="button"
                        className="secondary-action"
                        onClick={requestAgentStop}
                      >
                        <CircleStop size={15} aria-hidden="true" />
                        {copy.chatWorkbench.stopTask}
                      </button>
                    ) : null}
                    <button
                      className={`primary-action composer-submit ${agentComposerAction}`}
                      type="submit"
                    >
                      {agentComposerAction === "stop" ? (
                        <CircleStop size={16} aria-hidden="true" />
                      ) : (
                        <Send size={16} aria-hidden="true" />
                      )}
                      {agentComposerAction === "stop"
                        ? copy.chatWorkbench.stopTask
                        : agentComposerAction === "send_new_task"
                          ? copy.chatWorkbench.queueTask
                        : copy.chatWorkbench.saveTask}
                    </button>
                  </div>
                </form>
                {agentChatNotice ? (
                  <p className="package-message chat-feedback">{agentChatNotice}</p>
                ) : null}
                {agentAttachmentError ? (
                  <p className="package-error chat-feedback">{agentAttachmentError}</p>
                ) : null}
                {agentChatError ? <p className="package-error chat-feedback">{agentChatError}</p> : null}
              </div>
            </section>

            <ConnectedWorkReviewPanel language={language} />

            {renderLegacyCenterManagementPanels ? (
              <>
            <section className="workflow-panel" aria-labelledby="operations-briefing-title">
              <div className="section-heading">
                <ClipboardList size={18} aria-hidden="true" />
                <h2 id="operations-briefing-title">{copy.operationsBriefing.title}</h2>
              </div>

              <div className="workflow-run-list" aria-live="polite">
                {operationsBriefingRuns.length === 0 ? (
                  <p className="empty-state">{copy.operationsBriefing.noRuns}</p>
                ) : (
                  <>
                    <div className="queue-heading">
                      <strong>{copy.operationsBriefing.runs}</strong>
                      <span>{operationsBriefingRuns.length}</span>
                    </div>
                    {operationsBriefingRuns.map((operationsBriefingRun, runIndex) => (
                      <article className="workflow-run" key={operationsBriefingRun.id}>
                        <header className="workflow-run-header">
                          <div>
                            <span>
                              {runIndex === 0
                                ? copy.operationsBriefing.latestRun
                                : copy.operationsBriefing.runs}
                            </span>
                            <strong>{operationsBriefingRun.title}</strong>
                          </div>
                          <span className={`access-status ${operationsBriefingRun.status}`}>
                            {copy.operationsBriefing.status[operationsBriefingRun.status]}
                          </span>
                        </header>
                        <p>{operationsBriefingRun.summary}</p>
                        {operationsBriefingRun.warnings.length > 0 ? (
                          <p>{operationsBriefingRun.warnings.join(" ")}</p>
                        ) : null}
                        <footer>
                          <span>{formatTaskDate(operationsBriefingRun.created_at, language)}</span>
                          {operationsBriefingRun.archived_from_package ? (
                            <span className="archive-label">{copy.operationsBriefing.archived}</span>
                          ) : null}
                          {operationsBriefingRun.evidence_folder_path ? (
                            <span>
                              {copy.operationsBriefing.evidence}:{" "}
                              {operationsBriefingRun.evidence_folder_path}
                            </span>
                          ) : null}
                          {operationsBriefingRun.archived_from_package && !operationsBriefingRun.evidence_folder_path ? (
                            <span className="archive-evidence-note">
                              {copy.operationsBriefing.archiveEvidenceRedacted}
                            </span>
                          ) : null}
                        </footer>
                        <section
                          className="context-receipt"
                          aria-label={copy.operationsBriefing.contextReceipt}
                        >
                          <strong>{copy.operationsBriefing.contextReceipt}</strong>
                          <dl className="context-receipt-meta">
                            <div>
                              <dt>{copy.operationsBriefing.contextUserIntent}</dt>
                              <dd>{operationsBriefingRun.context_receipt.user_intent}</dd>
                            </div>
                            <div>
                              <dt>{copy.operationsBriefing.contextLoopMode}</dt>
                              <dd>{operationsBriefingRun.context_receipt.loop_mode}</dd>
                            </div>
                            <div>
                              <dt>{copy.operationsBriefing.contextWorkflowPolicy}</dt>
                              <dd>{operationsBriefingRun.context_receipt.workflow_policy}</dd>
                            </div>
                            <div>
                              <dt>{copy.operationsBriefing.contextModelRoute}</dt>
                              <dd>{operationsBriefingRun.context_receipt.model_route}</dd>
                            </div>
                            <div>
                              <dt>{copy.operationsBriefing.contextThinkingLevel}</dt>
                              <dd>{operationsBriefingRun.context_receipt.thinking_level}</dd>
                            </div>
                            <div>
                              <dt>{copy.operationsBriefing.contextTokenCache}</dt>
                              <dd>{operationsBriefingRun.context_receipt.token_cache_state}</dd>
                            </div>
                          </dl>
                          <div className="context-receipt-lists">
                            <div>
                              <span>{copy.operationsBriefing.contextSelectedEvidence}</span>
                              {operationsBriefingRun.context_receipt.selected_evidence.length > 0 ? (
                                <ul>
                                  {operationsBriefingRun.context_receipt.selected_evidence.map(
                                    (evidence, evidenceIndex) => (
                                      <li key={`${operationsBriefingRun.id}-evidence-${evidenceIndex}`}>
                                        {evidence}
                                      </li>
                                    ),
                                  )}
                                </ul>
                              ) : (
                                <p>{copy.operationsBriefing.contextNoItems}</p>
                              )}
                            </div>
                            <div>
                              <span>{copy.operationsBriefing.contextSelectedMemories}</span>
                              {operationsBriefingRun.context_receipt.selected_memories.length > 0 ? (
                                <ul>
                                  {operationsBriefingRun.context_receipt.selected_memories.map(
                                    (memory, memoryIndex) => (
                                      <li key={`${operationsBriefingRun.id}-memory-${memoryIndex}`}>
                                        {memory}
                                      </li>
                                    ),
                                  )}
                                </ul>
                              ) : (
                                <p>{copy.operationsBriefing.contextNoSelectedMemories}</p>
                              )}
                            </div>
                            <div>
                              <span>{copy.operationsBriefing.contextValidation}</span>
                              {operationsBriefingRun.context_receipt.validation_results.length > 0 ? (
                                <ul>
                                  {operationsBriefingRun.context_receipt.validation_results.map(
                                    (result, resultIndex) => (
                                      <li key={`${operationsBriefingRun.id}-validation-${resultIndex}`}>
                                        {result}
                                      </li>
                                    ),
                                  )}
                                </ul>
                              ) : (
                                <p>{copy.operationsBriefing.contextNoItems}</p>
                              )}
                            </div>
                            <div>
                              <span>{copy.operationsBriefing.contextIntentionalOmissions}</span>
                              {operationsBriefingRun.context_receipt.intentional_omissions.length > 0 ? (
                                <ul>
                                  {operationsBriefingRun.context_receipt.intentional_omissions.map(
                                    (omission, omissionIndex) => (
                                      <li key={`${operationsBriefingRun.id}-omission-${omissionIndex}`}>
                                        {omission}
                                      </li>
                                    ),
                                  )}
                                </ul>
                              ) : (
                                <p>{copy.operationsBriefing.contextNoItems}</p>
                              )}
                            </div>
                          </div>
                        </section>
                        <div className="workflow-run-sections">
                          <section>
                            <strong>{copy.operationsBriefing.anomalies}</strong>
                            {operationsBriefingRun.anomalies.length === 0 ? (
                              <p className="empty-state">{copy.operationsBriefing.noAnomalies}</p>
                            ) : (
                              <ul>
                                {operationsBriefingRun.anomalies.map((anomaly, anomalyIndex) => (
                                  <li
                                    key={`${operationsBriefingRun.id}-anomaly-${anomaly.area}-${anomalyIndex}`}
                                  >
                                    <span>{anomaly.area}</span>
                                    {anomaly.signal}
                                  </li>
                                ))}
                              </ul>
                            )}
                          </section>
                          <section>
                            <strong>{copy.operationsBriefing.actions}</strong>
                            {operationsBriefingRun.action_plan.length === 0 ? (
                              <p className="empty-state">{copy.operationsBriefing.noActions}</p>
                            ) : (
                              <ul>
                                {operationsBriefingRun.action_plan.map((action, actionIndex) => (
                                  <li
                                    key={`${operationsBriefingRun.id}-action-${action.owner}-${actionIndex}`}
                                  >
                                    <span>{action.owner}</span>
                                    {action.action}
                                  </li>
                                ))}
                              </ul>
                            )}
                          </section>
                        </div>
                        {runIndex === 0 ? (
                          <div className="workflow-run-actions">
                            <button
                              type="button"
                              onClick={exportOperationsBriefingPackage}
                              disabled={briefingPending || packagePending}
                            >
                              <PackageOpen size={14} aria-hidden="true" />
                              {copy.operationsBriefing.exportPackage}
                            </button>
                            <button
                              type="button"
                              onClick={() => void exportOperationsBriefingReport()}
                              disabled={briefingPending}
                            >
                              <FileText size={14} aria-hidden="true" />
                              {copy.operationsBriefing.exportReport}
                            </button>
                            <button
                              type="button"
                              onClick={() => void exportOperationsBriefingHtmlReport()}
                              disabled={briefingPending}
                            >
                              <FileText size={14} aria-hidden="true" />
                              {copy.operationsBriefing.exportHtmlReport}
                            </button>
                            <button
                              type="button"
                              onClick={() => void exportOperationsBriefingPdfReport()}
                              disabled={briefingPending}
                            >
                              <FileText size={14} aria-hidden="true" />
                              {copy.operationsBriefing.exportPdfReport}
                            </button>
                          </div>
                        ) : null}
                      </article>
                    ))}
                  </>
                )}
              </div>
            </section>

            <section className="package-panel" aria-labelledby="work-package-title">
              <div className="section-heading">
                <PackageOpen size={18} aria-hidden="true" />
                <h2 id="work-package-title">{copy.package.title}</h2>
              </div>

              <section
                className="memory-panel inline"
                aria-labelledby="memory-panel-title"
                ref={memorySectionRef}
              >
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
                {memoryRecords.length >= 2 ? (
                  <form className="memory-link-form" onSubmit={linkExistingMemoryRecords}>
                    <select
                      value={memoryLinkSourceId}
                      aria-label={copy.memory.linkSource}
                      onChange={(event) => setMemoryLinkSourceId(event.target.value)}
                      disabled={memoryExistingLinkPending}
                    >
                      <option value="">{copy.memory.linkSource}</option>
                      {memoryRecords.map((memory) => (
                        <option key={memory.id} value={memory.id}>
                          {memory.title}
                        </option>
                      ))}
                    </select>
                    <select
                      value={memoryLinkTargetId}
                      aria-label={copy.memory.linkTarget}
                      onChange={(event) => setMemoryLinkTargetId(event.target.value)}
                      disabled={memoryExistingLinkPending}
                    >
                      <option value="">{copy.memory.linkTarget}</option>
                      {memoryRecords.map((memory) => (
                        <option key={memory.id} value={memory.id}>
                          {memory.title}
                        </option>
                      ))}
                    </select>
                    <select
                      value={memoryExistingLinkRelation}
                      aria-label={copy.memory.linkRelation}
                      onChange={(event) =>
                        setMemoryExistingLinkRelation(event.target.value as MemoryRelationKind)
                      }
                      disabled={memoryExistingLinkPending}
                    >
                      {(Object.keys(copy.memory.relationOptions) as MemoryRelationKind[]).map(
                        (relation) => (
                          <option key={relation} value={relation}>
                            {copy.memory.relationOptions[relation]}
                          </option>
                        ),
                      )}
                    </select>
                    <input
                      value={memoryExistingLinkNote}
                      aria-label={copy.memory.linkExistingNote}
                      placeholder={copy.memory.linkExistingNote}
                      onChange={(event) => setMemoryExistingLinkNote(event.target.value)}
                      disabled={memoryExistingLinkPending}
                    />
                    <button type="submit" disabled={memoryExistingLinkPending}>
                      <Link2 size={15} aria-hidden="true" />
                      {memoryExistingLinkPending
                        ? copy.memory.linkingExisting
                        : copy.memory.linkExisting}
                    </button>
                  </form>
                ) : null}
                {memoryNotice ? <p className="package-message">{memoryNotice}</p> : null}
                {memoryError ? <p className="package-error">{memoryError}</p> : null}
                <section
                  className="memory-feedback-review"
                  aria-label={copy.memory.feedbackReview}
                >
                  <div className="memory-subsection-heading">
                    <strong>{copy.memory.feedbackReview}</strong>
                    <span>{copy.memory.feedbackReviewCount(selectedMemoryFeedbackRecords.length)}</span>
                  </div>
                  <div className="memory-review-controls">
                    <label>
                      <span>{copy.memory.feedbackFilter}</span>
                      <select
                        value={memoryFeedbackFilter}
                        onChange={(event) =>
                          setMemoryFeedbackFilter(event.target.value as MemoryFeedbackReviewFilter)
                        }
                      >
                        {memoryFeedbackReviewFilterValues.map((value) => (
                          <option key={value} value={value}>
                            {copy.memory.feedbackFilterOptions[value]}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label>
                      <span>{copy.memory.feedbackSort}</span>
                      <select
                        value={memoryFeedbackSort}
                        onChange={(event) =>
                          setMemoryFeedbackSort(event.target.value as MemoryFeedbackReviewSort)
                        }
                      >
                        {memoryReviewSortValues.map((value) => (
                          <option key={value} value={value}>
                            {copy.memory.feedbackSortOptions[value]}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>
                  {filteredFeedbackReviewItems.length === 0 ? (
                    <p className="empty-state">{copy.memory.feedbackReviewEmpty}</p>
                  ) : (
                    <div className="memory-list compact">
                      {filteredFeedbackReviewItems.slice(0, 8).map((item) => (
                        <article className="memory-row" key={item.memoryId}>
                          <div className="memory-row-title">
                            <strong>
                              {item.memory?.title ?? copy.memory.feedbackMemoryMissing}
                            </strong>
                            {item.needsFeedbackReview ? (
                              <span>{copy.memory.needsFeedbackReview}</span>
                            ) : null}
                          </div>
                          <div className="memory-meta">
                            {(Object.keys(copy.memoryFeedback.options) as MemorySelectedFeedbackKind[])
                              .filter((feedback) => (item.counts[feedback] ?? 0) > 0)
                              .map((feedback) => (
                                <span key={feedback}>
                                  {copy.memoryFeedback.options[feedback]}: {item.counts[feedback]}
                                </span>
                              ))}
                          </div>
                          {item.latestFeedback ? (
                            <p className="memory-feedback-note">
                              {copy.memory.latestFeedback}:{" "}
                              {copy.memoryFeedback.options[item.latestFeedback.feedback]}
                              {item.latestFeedback.note ? ` · ${item.latestFeedback.note}` : ""}
                            </p>
                          ) : null}
                        </article>
                      ))}
                    </div>
                  )}
                </section>
                <section
                  className="memory-feedback-review memory-maintenance-review"
                  aria-label={copy.memory.maintenanceReview}
                >
                  <div className="memory-subsection-heading">
                    <strong>{copy.memory.maintenanceReview}</strong>
                    <span>{copy.memory.maintenanceReviewCount(memoryMaintenanceReviews.length)}</span>
                  </div>
                  <div className="memory-review-controls">
                    <label>
                      <span>{copy.memory.maintenanceFilter}</span>
                      <select
                        value={memoryMaintenanceFilter}
                        onChange={(event) =>
                          setMemoryMaintenanceFilter(event.target.value as MemoryMaintenanceFilter)
                        }
                      >
                        {memoryMaintenanceFilterValues.map((value) => (
                          <option key={value} value={value}>
                            {copy.memory.maintenanceFilterOptions[value]}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label>
                      <span>{copy.memory.maintenanceSort}</span>
                      <select
                        value={memoryMaintenanceSort}
                        onChange={(event) =>
                          setMemoryMaintenanceSort(event.target.value as MemoryMaintenanceSort)
                        }
                      >
                        {memoryMaintenanceSortValues.map((value) => (
                          <option key={value} value={value}>
                            {copy.memory.maintenanceSortOptions[value]}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>
                  {filteredMemoryMaintenanceReviews.length === 0 ? (
                    <p className="empty-state">{copy.memory.maintenanceReviewEmpty}</p>
                  ) : (
                    <div className="memory-list compact">
                      {filteredMemoryMaintenanceReviews.slice(0, 8).map((item) => (
                        <article className="memory-row" key={item.memory.id}>
                          <div className="memory-row-title">
                            <strong>{item.memory.title}</strong>
                            {item.review_needed ? (
                              <span>{copy.memory.needsFeedbackReview}</span>
                            ) : item.snoozed_until ? (
                              <span>
                                {copy.memory.maintenanceSnoozeUntil}:{" "}
                                {formatTaskDate(item.snoozed_until, language)}
                              </span>
                            ) : null}
                          </div>
                          <p>{item.memory.body}</p>
                          <div className="memory-meta">
                            <span>
                              {copy.memory.maintenanceQuality}: {item.quality_score}
                            </span>
                            {item.review_kinds.map((kind) => (
                              <span key={kind}>{copy.memory.maintenanceReviewKindOptions[kind]}</span>
                            ))}
                            {(Object.keys(copy.memoryFeedback.options) as MemorySelectedFeedbackKind[])
                              .filter((feedback) => (item.feedback_counts[feedback] ?? 0) > 0)
                              .map((feedback) => (
                                <span key={feedback}>
                                  {copy.memoryFeedback.options[feedback]}:{" "}
                                  {item.feedback_counts[feedback]}
                                </span>
                              ))}
                          </div>
                          {item.quality_signals.length > 0 ? (
                            <p className="memory-feedback-note">
                              {copy.memory.maintenanceQualitySignals}:{" "}
                              {item.quality_signals
                                .map((signal) => signal.replace(/_/g, " "))
                                .join(", ")}
                            </p>
                          ) : null}
                          {item.recommended_actions.length > 0 ? (
                            <p className="memory-feedback-note">
                              {copy.memory.maintenanceRecommendedActions}:{" "}
                              {item.recommended_actions
                                .map((action) => copy.memory.maintenanceActionOptions[action])
                                .join(", ")}
                            </p>
                          ) : null}
                          {item.latest_feedback ? (
                            <p className="memory-feedback-note">
                              {copy.memory.latestFeedback}:{" "}
                              {copy.memoryFeedback.options[item.latest_feedback.feedback]}
                              {item.latest_feedback.note ? ` · ${item.latest_feedback.note}` : ""}
                            </p>
                          ) : null}
                          {item.last_action ? (
                            <p className="memory-feedback-note">
                              {copy.memory.maintenanceLastAction}:{" "}
                              {copy.memory.maintenanceActionOptions[item.last_action.action]}
                              {item.last_action.note ? ` · ${item.last_action.note}` : ""}
                            </p>
                          ) : null}
                          <p className="memory-feedback-note">
                            {copy.memory.maintenanceAutomatic}:{" "}
                            {item.review_needed
                              ? copy.memory.needsFeedbackReview
                              : copy.memory.maintenanceNoUserAction}
                          </p>
                        </article>
                      ))}
                    </div>
                  )}
                </section>
                {memoryRecords.length === 0 ? (
                  <p className="empty-state">{copy.memory.noMemories}</p>
                ) : (
                  <div className="memory-list">
                    {memoryRecords.map((memory) => {
                      const isEditing = memoryEditDraft?.id === memory.id;
                      const searchMatch = memory.search_match ?? defaultMemorySearchMatch;
                      const searchLinkedMemory = searchMatch.linked_memory_id
                        ? memory.linked_memories.find(
                            (linkedMemory) =>
                              linkedMemory.id === searchMatch.linked_memory_id,
                          )
                        : null;
                      const showSearchMatch = searchMatch.source !== "direct";

                      return (
                        <article className="memory-row" key={memory.id}>
                          {isEditing && memoryEditDraft ? (
                            <form className="memory-edit-form" onSubmit={updateMemoryRecord}>
                              <input
                                value={memoryEditDraft.title}
                                aria-label={copy.memory.editTitle}
                                placeholder={copy.memory.editTitle}
                                onChange={(event) =>
                                  patchMemoryEditDraft({ title: event.target.value })
                                }
                              />
                              <textarea
                                value={memoryEditDraft.body}
                                aria-label={copy.memory.editBody}
                                placeholder={copy.memory.editBody}
                                onChange={(event) =>
                                  patchMemoryEditDraft({ body: event.target.value })
                                }
                              />
                              <div className="memory-candidate-metadata">
                                <label>
                                  <span>{copy.memory.candidateType}</span>
                                  <select
                                    value={memoryEditDraft.memory_type}
                                    onChange={(event) =>
                                      patchMemoryEditDraft({
                                        memory_type: event.target.value as MemoryType,
                                      })
                                    }
                                  >
                                    {memoryTypeValues.map((value) => (
                                      <option value={value} key={value}>
                                        {copy.memory.typeOptions[value]}
                                      </option>
                                    ))}
                                  </select>
                                </label>
                                <label>
                                  <span>{copy.memory.candidateScope}</span>
                                  <select
                                    value={memoryEditDraft.scope}
                                    onChange={(event) =>
                                      patchMemoryEditDraft({
                                        scope: event.target.value as MemoryScope,
                                      })
                                    }
                                  >
                                    {memoryScopeValues.map((value) => (
                                      <option value={value} key={value}>
                                        {copy.memory.scopeOptions[value]}
                                      </option>
                                    ))}
                                  </select>
                                </label>
                                <label>
                                  <span>{copy.memory.candidateSensitivity}</span>
                                  <select
                                    value={memoryEditDraft.sensitivity}
                                    onChange={(event) =>
                                      patchMemoryEditDraft({
                                        sensitivity: event.target.value as MemorySensitivity,
                                      })
                                    }
                                  >
                                    {memorySensitivityValues.map((value) => (
                                      <option value={value} key={value}>
                                        {copy.memory.sensitivityOptions[value]}
                                      </option>
                                    ))}
                                  </select>
                                </label>
                                <label>
                                  <span>{copy.memory.candidateLifecycle}</span>
                                  <select
                                    value={memoryEditDraft.lifecycle}
                                    onChange={(event) =>
                                      patchMemoryEditDraft({
                                        lifecycle: event.target.value as MemoryLifecycle,
                                      })
                                    }
                                  >
                                    {memoryLifecycleValues.map((value) => (
                                      <option value={value} key={value}>
                                        {copy.memory.lifecycleOptions[value]}
                                      </option>
                                    ))}
                                  </select>
                                </label>
                                {memoryEditDraft.lifecycle === "expires" ? (
                                  <label>
                                    <span>{copy.memory.expiresAt}</span>
                                    <input
                                      type="date"
                                      value={memoryEditDraft.expires_at}
                                      onChange={(event) =>
                                        patchMemoryEditDraft({
                                          expires_at: event.target.value,
                                        })
                                      }
                                    />
                                  </label>
                                ) : null}
                              </div>
                              <div className="candidate-actions">
                                <button type="submit" disabled={memoryUpdatePending !== null}>
                                  <Check size={14} aria-hidden="true" />
                                  {memoryUpdatePending === memory.id
                                    ? copy.memory.saving
                                    : copy.memory.save}
                                </button>
                                <button
                                  type="button"
                                  onClick={cancelMemoryRecordEdit}
                                  disabled={memoryUpdatePending !== null}
                                >
                                  <X size={14} aria-hidden="true" />
                                  {copy.memory.cancel}
                                </button>
                              </div>
                            </form>
                          ) : (
                            <>
                              <strong>{memory.title}</strong>
                              <p>{memory.body}</p>
                              <div className="memory-meta" aria-label={copy.memory.metadata}>
                                <span>{copy.memory.typeOptions[memory.memory_type]}</span>
                                <span>{copy.memory.scopeOptions[memory.scope]}</span>
                                <span>{copy.memory.sensitivityOptions[memory.sensitivity]}</span>
                                <span>{copy.memory.lifecycleOptions[memory.lifecycle]}</span>
                                {memory.expires_at ? (
                                  <span>
                                    {copy.memory.expiresAt}:{" "}
                                    {formatTaskDate(memory.expires_at, language)}
                                  </span>
                                ) : null}
                              </div>
                              {memory.linked_memories.length > 0 ? (
                                <div className="memory-linked-list">
                                  <span>{copy.memory.linkedMemories(memory.linked_memories.length)}</span>
                                  {memory.linked_memories.map((linkedMemory) => (
                                    <span className="memory-link-pill" key={linkedMemory.id}>
                                      <Link2 size={12} aria-hidden="true" />
                                      <strong>{copy.memory.relationOptions[linkedMemory.relation]}</strong>
                                      {linkedMemory.title}
                                      {linkedMemory.note ? (
                                        <span className="memory-link-note">
                                          {copy.memory.linkNote}: {linkedMemory.note}
                                        </span>
                                      ) : null}
                                    </span>
                                  ))}
                                </div>
                              ) : null}
                              {showSearchMatch ? (
                                <div className="memory-search-match">
                                  <span>{copy.memory.searchMatchedBy}</span>
                                  <span className="memory-link-pill">
                                    <Search size={12} aria-hidden="true" />
                                    <strong>
                                      {copy.memory.searchMatchOptions[searchMatch.source]}
                                    </strong>
                                    {searchMatch.relation ? (
                                      <span>
                                        {copy.memory.relationOptions[searchMatch.relation]}
                                      </span>
                                    ) : null}
                                    <span>
                                      {searchLinkedMemory?.title ?? copy.memory.searchMatchUnknown}
                                    </span>
                                  </span>
                                </div>
                              ) : null}
                              <div className="candidate-actions">
                                <button
                                  type="button"
                                  onClick={() => beginMemoryRecordEdit(memory)}
                                  disabled={
                                    memoryUpdatePending !== null || memoryDeletionPending !== null
                                  }
                                >
                                  <Pencil size={14} aria-hidden="true" />
                                  {copy.memory.edit}
                                </button>
                                <button
                                  type="button"
                                  onClick={() => void deleteMemoryRecord(memory.id)}
                                  disabled={
                                    memoryDeletionPending !== null || memoryUpdatePending !== null
                                  }
                                >
                                  <X size={14} aria-hidden="true" />
                                  {memoryDeletionPending === memory.id
                                    ? copy.memory.deleting
                                    : copy.memory.delete}
                                </button>
                              </div>
                            </>
                          )}
                          <span>
                            {copy.memory.autoCapture} · {formatTaskDate(memory.created_at, language)}
                          </span>
                        </article>
                      );
                    })}
                  </div>
                )}
                <section className="memory-candidate-panel" aria-labelledby="memory-candidates-title">
                  <div className="queue-heading">
                    <strong id="memory-candidates-title">{copy.memory.candidates}</strong>
                    <span>{memoryCandidateRecords.length}</span>
                  </div>
                  <form className="memory-candidate-form" onSubmit={proposeMemoryCandidate}>
                    <input
                      value={candidateTitle}
                      aria-label={copy.memory.candidateTitle}
                      placeholder={copy.memory.candidateTitle}
                      onChange={(event) => setCandidateTitle(event.target.value)}
                    />
                    <textarea
                      value={candidateBody}
                      aria-label={copy.memory.candidateBody}
                      placeholder={copy.memory.candidateBody}
                      rows={3}
                      onChange={(event) => setCandidateBody(event.target.value)}
                    />
                    <div className="memory-candidate-metadata">
                      <label>
                        <span>{copy.memory.candidateType}</span>
                        <select
                          value={candidateMemoryType}
                          onChange={(event) =>
                            setCandidateMemoryType(event.target.value as MemoryType)
                          }
                        >
                          {memoryTypeValues.map((value) => (
                            <option key={value} value={value}>
                              {copy.memory.typeOptions[value]}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label>
                        <span>{copy.memory.candidateScope}</span>
                        <select
                          value={candidateMemoryScope}
                          onChange={(event) =>
                            setCandidateMemoryScope(event.target.value as MemoryScope)
                          }
                        >
                          {memoryScopeValues.map((value) => (
                            <option key={value} value={value}>
                              {copy.memory.scopeOptions[value]}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label>
                        <span>{copy.memory.candidateSensitivity}</span>
                        <select
                          value={candidateSensitivity}
                          onChange={(event) =>
                            setCandidateSensitivity(event.target.value as MemorySensitivity)
                          }
                        >
                          {memorySensitivityValues.map((value) => (
                            <option key={value} value={value}>
                              {copy.memory.sensitivityOptions[value]}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label>
                        <span>{copy.memory.candidateLifecycle}</span>
                        <select
                          value={candidateLifecycle}
                          onChange={(event) =>
                            setCandidateLifecycle(event.target.value as MemoryLifecycle)
                          }
                        >
                          {memoryLifecycleValues.map((value) => (
                            <option key={value} value={value}>
                              {copy.memory.lifecycleOptions[value]}
                            </option>
                          ))}
                        </select>
                      </label>
                      {candidateLifecycle === "expires" ? (
                        <label>
                          <span>{copy.memory.expiresAt}</span>
                          <input
                            type="date"
                            value={candidateExpiresAt}
                            onChange={(event) => setCandidateExpiresAt(event.target.value)}
                          />
                        </label>
                      ) : null}
                    </div>
                    <button type="submit" disabled={memoryCandidatePending}>
                      <Plus size={15} aria-hidden="true" />
                      {memoryCandidatePending ? copy.memory.proposing : copy.memory.propose}
                    </button>
                  </form>
                  {memoryCandidateNotice ? (
                    <p className="package-message">{memoryCandidateNotice}</p>
                  ) : null}
                  {memoryCandidateError ? (
                    <p className="package-error">{memoryCandidateError}</p>
                  ) : null}
                  {memoryCandidateRecords.length === 0 ? (
                    <p className="empty-state">{copy.memory.noCandidates}</p>
                  ) : (
                    <div className="memory-list">
                      {memoryCandidateRecords.map((record) => (
                        <article className="memory-row candidate-row" key={record.candidate.id}>
                          <div className="candidate-row-header">
                            <strong>{record.candidate.title}</strong>
                            <span className={`access-status ${record.effective_status}`}>
                              {copy.memory.candidateStatus[record.effective_status]}
                            </span>
                          </div>
                          <p>{record.candidate.body}</p>
                          <div className="memory-meta" aria-label={copy.memory.metadata}>
                            <span>{copy.memory.typeOptions[record.candidate.memory_type]}</span>
                            <span>{copy.memory.scopeOptions[record.candidate.scope]}</span>
                            <span>
                              {copy.memory.sensitivityOptions[record.candidate.sensitivity]}
                            </span>
                            <span>{copy.memory.lifecycleOptions[record.candidate.lifecycle]}</span>
                            {record.candidate.expires_at ? (
                              <span>
                                {copy.memory.expiresAt}:{" "}
                                {formatTaskDate(record.candidate.expires_at, language)}
                              </span>
                            ) : null}
                          </div>
                          {record.candidate.privacy_review ||
                          record.candidate.evidence_excerpt ? (
                            <div className="memory-candidate-gate">
                              <strong>{copy.memory.candidateGate}</strong>
                              <div className="memory-meta">
                                {record.candidate.privacy_review ? (
                                  <span>
                                    {copy.memory.candidatePrivacyReview}:{" "}
                                    {record.candidate.privacy_review}
                                  </span>
                                ) : null}
                                <span>
                                  {copy.memory.candidateSuggestedAction}:{" "}
                                  {
                                    copy.memory.candidateSuggestedActionOptions[
                                      record.candidate.suggested_action
                                    ]
                                  }
                                </span>
                              </div>
                              {record.candidate.evidence_excerpt ? (
                                <p>
                                  {copy.memory.candidateEvidenceExcerpt}:{" "}
                                  {record.candidate.evidence_excerpt}
                                </p>
                              ) : null}
                            </div>
                          ) : null}
                          {record.conflicting_memory_ids.length > 0 ? (
                            <p className="memory-conflict">
                              {copy.memory.conflictWarning(
                                record.conflicting_memory_ids.length,
                              )}
                            </p>
                          ) : null}
                          {record.conflicting_memories.length > 0 ? (
                            <div className="memory-conflict-details">
                              <strong>{copy.memory.conflictDetails}</strong>
                              {record.conflicting_memories.map((memory) => (
                                <article className="memory-conflict-item" key={memory.id}>
                                  <div className="candidate-row-header">
                                    <span>{memory.title}</span>
                                    <span>
                                      {copy.memory.updatedAt}:{" "}
                                      {formatTaskDate(memory.updated_at, language)}
                                    </span>
                                  </div>
                                  <p>{memory.body}</p>
                                  <div className="memory-meta">
                                    <span>{copy.memory.typeOptions[memory.memory_type]}</span>
                                    <span>{copy.memory.scopeOptions[memory.scope]}</span>
                                    <span>
                                      {copy.memory.sensitivityOptions[memory.sensitivity]}
                                    </span>
                                    <span>{copy.memory.lifecycleOptions[memory.lifecycle]}</span>
                                    {memory.expires_at ? (
                                      <span>
                                        {copy.memory.expiresAt}:{" "}
                                        {formatTaskDate(memory.expires_at, language)}
                                      </span>
                                    ) : null}
                                  </div>
                                </article>
                              ))}
                            </div>
                          ) : null}
                          {record.effective_status === "pending" ? (
                            <p className="memory-feedback-note">
                              {copy.memory.maintenanceAutomatic}:{" "}
                              {copy.memory.maintenanceNoUserAction}
                            </p>
                          ) : null}
                        </article>
                      ))}
                    </div>
                  )}
                </section>
              </section>

              <AutomationCenter
                language={language}
                onRunQueued={async () => {
                  await refreshAgentRunRecords();
                }}
              />

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

              {exportedPackageJson ? (
                <textarea
                  className="package-json"
                  value={exportedPackageJson}
                  aria-label={copy.package.packageJson}
                  rows={5}
                  readOnly
                />
              ) : null}

              {importPreview ? (
                <section className="import-preview" aria-labelledby="import-preview-title">
                  <strong id="import-preview-title">{copy.package.previewTitle}</strong>
                  <div>
                    <span>
                      {copy.package.previewTotalTasks}: {importPreview.task_records.total}
                    </span>
                    <span>
                      {copy.package.previewNewTasks}: {importPreview.task_records.new}
                    </span>
                    <span>
                      {copy.package.previewSkippedTasks}: {importPreview.task_records.skipped}
                    </span>
                    <span>
                      {copy.package.previewMemoryCandidates}:{" "}
                      {importPreview.memory_candidates.total}
                    </span>
                    <span>
                      {copy.package.previewNewMemoryCandidates}:{" "}
                      {importPreview.memory_candidates.new}
                    </span>
                    <span>
                      {copy.package.previewSkippedMemoryCandidates}:{" "}
                      {importPreview.memory_candidates.skipped}
                    </span>
                    <span>
                      {copy.package.previewArchivedRuns}:{" "}
                      {importPreview.operations_briefing_runs.total}
                    </span>
                    <span>
                      {copy.package.previewNewArchivedRuns}:{" "}
                      {importPreview.operations_briefing_runs.new}
                    </span>
                    <span>
                      {copy.package.previewSkippedArchivedRuns}:{" "}
                      {importPreview.operations_briefing_runs.skipped}
                    </span>
                    <span>
                      {copy.package.previewWorkflowTemplates}:{" "}
                      {importPreview.workflow_templates.total}
                    </span>
                    <span>
                      {copy.package.previewNewWorkflowTemplates}:{" "}
                      {importPreview.workflow_templates.new}
                    </span>
                    <span>
                      {copy.package.previewSkippedWorkflowTemplates}:{" "}
                      {importPreview.workflow_templates.skipped}
                    </span>
                  </div>
                  <p>{copy.package.previewMemoryCandidateHint}</p>
                  <p>
                    {importPreview.memory_candidates.review_supported
                      ? copy.package.previewMemoryCandidateReviewSupported
                      : copy.package.previewMemoryCandidateReviewUnsupported}
                  </p>
                  <p>{copy.package.previewArchiveHint}</p>
                  <p>
                    {importPreview.operations_briefing_runs.replay_supported
                      ? copy.package.previewArchiveReplaySupported
                      : copy.package.previewArchiveReplayUnsupported}
                  </p>
                  <p>{copy.package.previewWorkflowTemplateHint}</p>
                  <p>
                    {importPreview.workflow_templates.import_supported
                      ? copy.package.previewWorkflowTemplateImportSupported
                      : copy.package.previewWorkflowTemplateImportUnsupported}
                  </p>
                </section>
              ) : null}

              {packageNotice ? <p className="package-message">{packageNotice}</p> : null}
              {packageError ? <p className="package-error">{packageError}</p> : null}
            </section>
              </>
            ) : null}
          </div>
          {shouldShowRunInspector || activeComputerUseStep ? (
          <aside className="inspector run-inspector">
            <div className="inspector-header">
              <ClipboardList size={18} aria-hidden="true" />
              <strong>{copy.runStatus.title}</strong>
            </div>
            {activeComputerUseStep ? (
              <section className="computer-use-step-panel" aria-labelledby="computer-use-step-title">
                <header>
                  <div>
                    <span>v0.8</span>
                    <strong id="computer-use-step-title">{computerUseStepCopy.title}</strong>
                  </div>
                  <span
                    className={`computer-use-step-status ${activeComputerUseStep.status}`}
                    data-status={activeComputerUseStep.status}
                  >
                    {computerUseStepCopy[activeComputerUseStep.status]}
                  </span>
                </header>
                <ol className="computer-use-step-timeline" aria-label={computerUseStepCopy.title}>
                  <li className="complete">
                    <span>1</span>
                    <small>{computerUseStepCopy.observed}</small>
                  </li>
                  <li
                    className={
                      activeComputerUseStep.status === "awaiting_approval" ? "current" : "complete"
                    }
                  >
                    <span>2</span>
                    <small>{computerUseStepCopy.awaiting_approval}</small>
                  </li>
                  <li
                    className={
                      ["observed", "awaiting_approval", "ready"].includes(
                        activeComputerUseStep.status,
                      )
                        ? activeComputerUseStep.status === "ready"
                          ? "current"
                          : ""
                        : "complete"
                    }
                  >
                    <span>3</span>
                    <small>{computerUseStepCopy.action_started}</small>
                  </li>
                  <li
                    className={
                      activeComputerUseStep.status === "verified"
                        ? "complete"
                        : activeComputerUseStep.status === "awaiting_verification"
                          ? "current"
                          : ""
                    }
                  >
                    <span>4</span>
                    <small>{computerUseStepCopy.verified}</small>
                  </li>
                </ol>
                <p>{activeComputerUseStep.status_reason ?? activeComputerUseStep.pre_safe_summary}</p>
                <dl>
                  <div>
                    <dt>Window</dt>
                    <dd>{activeComputerUseStep.window_fingerprint.slice(0, 12)}</dd>
                  </div>
                  <div>
                    <dt>Target</dt>
                    <dd>{activeComputerUseStep.target_fingerprint?.slice(0, 12) ?? "—"}</dd>
                  </div>
                  <div>
                    <dt>Undo</dt>
                    <dd>{activeComputerUseStep.undo_capability}</dd>
                  </div>
                </dl>
                {activeComputerUseStep.action_safe_summary ? (
                  <div className="computer-use-action-receipt">
                    <strong>{activeComputerUseStep.action_display}</strong>
                    <span>{activeComputerUseStep.action_safe_summary}</span>
                  </div>
                ) : null}
                {activeComputerUseStep.status === "observed" ? (
                  <form onSubmit={bindDurableComputerUseAction}>
                    <label htmlFor="computer-use-action-draft">{computerUseStepCopy.action}</label>
                    <input
                      id="computer-use-action-draft"
                      value={computerUseActionDraft}
                      onChange={(event) => setComputerUseActionDraft(event.target.value)}
                    />
                    <button type="submit" disabled={computerUseStepPending}>
                      {computerUseStepPending
                        ? computerUseStepCopy.binding
                        : computerUseStepCopy.bind}
                    </button>
                  </form>
                ) : null}
                {["awaiting_approval", "ready"].includes(activeComputerUseStep.status) ? (
                  <div className="computer-use-step-actions">
                    <button
                      type="button"
                      disabled={computerUseStepPending || !computerControlUnlockStatus.unlocked}
                      onClick={() => void runDurableComputerUseStep()}
                    >
                      <Play size={14} aria-hidden="true" />
                      {computerUseStepPending
                        ? computerUseStepCopy.requesting
                        : computerUseStepCopy.confirmRun}
                    </button>
                    {!computerControlUnlockStatus.unlocked ? (
                      <small>{computerUseStepCopy.unlockHint}</small>
                    ) : null}
                  </div>
                ) : null}
                {["awaiting_approval", "ready", "action_started", "awaiting_verification"].includes(
                  activeComputerUseStep.status,
                ) ? (
                  <button
                    className="computer-use-takeover"
                    type="button"
                    disabled={computerUseStepPending}
                    onClick={() => void takeOverDurableComputerUseStep()}
                  >
                    <CircleStop size={14} aria-hidden="true" />
                    {computerUseStepCopy.takeover}
                  </button>
                ) : null}
                {["needs_replan", "user_taken_over", "verification_failed", "cancelled", "verified"].includes(
                  activeComputerUseStep.status,
                ) ? (
                  <div className="computer-use-step-actions">
                    <button
                      type="button"
                      disabled={computerUseStepPending || !lastComputerScreenshotInvocationId}
                      onClick={() => void reobserveDurableComputerUseStep()}
                    >
                      <RefreshCw size={14} aria-hidden="true" />
                      {computerUseStepCopy.reobserve}
                    </button>
                    {!lastComputerScreenshotInvocationId ? (
                      <small>{computerUseStepCopy.screenshotHint}</small>
                    ) : null}
                  </div>
                ) : null}
                {["observed", "awaiting_approval", "ready", "needs_replan", "user_taken_over", "verification_failed"].includes(
                  activeComputerUseStep.status,
                ) ? (
                  <button
                    className="computer-use-cancel"
                    type="button"
                    disabled={computerUseStepPending}
                    onClick={() => void cancelDurableComputerUseStep()}
                  >
                    {computerUseStepCopy.cancel}
                  </button>
                ) : null}
                {computerUseStepNotice ? (
                  <p className="package-message">{computerUseStepNotice}</p>
                ) : null}
                {computerUseStepError ? (
                  <p className="package-error">{computerUseStepError}</p>
                ) : null}
              </section>
            ) : null}
            <section className="run-status-panel" aria-labelledby="run-status-title">
              <div className={`run-status-callout ${runStatusTone}`}>
                <span>{copy.runStatus.current}</span>
                <strong id="run-status-title">{runStatusTitle}</strong>
                <p>{runStatusBody}</p>
                {latestOperationsBriefingRun ? (
                  <footer>
                    <span>{latestOperationsBriefingRun.title}</span>
                    <span>
                      {formatTaskDate(latestOperationsBriefingRun.created_at, language)}
                    </span>
                  </footer>
                ) : null}
              </div>
              <div className="queue-heading">
                <strong>{copy.runStatus.workflowSteps}</strong>
                <span>{runStatusSteps.length}</span>
              </div>
              <ol className="workflow-step-list" aria-label={copy.runStatus.workflowSteps}>
                {runStatusSteps.map((step, stepIndex) => {
                  const hasStepDetail = shouldShowWorkflowStepDetail(step);

                  return (
                    <li
                      className={`workflow-step ${step.state}${hasStepDetail ? " has-detail" : ""}`}
                      key={step.key}
                    >
                      <span className="workflow-step-marker">{stepIndex + 1}</span>
                      <div>
                        <strong>{step.label}</strong>
                        {hasStepDetail ? <p>{step.detail}</p> : null}
                      </div>
                      <span className={`workflow-step-state ${step.state}`}>
                        {copy.runStatus.stepState[step.state]}
                      </span>
                    </li>
                  );
                })}
              </ol>
              {recentAgentRunRecords.length > 0 ? (
                <>
                  <div className="queue-heading">
                    <strong>{copy.runStatus.recentRuns}</strong>
                    <span>
                      {queuedAgentRunCount > 0
                        ? `${copy.runStatus.queuedRuns}: ${queuedAgentRunCount}`
                        : recentAgentRunRecords.length}
                    </span>
                  </div>
                  <div className="sidebar-record-list">
                    {recentAgentRunRecords.map((record) => {
                      const expertAttempts = agentRunRecords
                        .filter((child) => child.parent_run_id === record.id)
                        .sort(
                          (left, right) =>
                            (left.expert_contract?.attempt ?? 0) -
                            (right.expert_contract?.attempt ?? 0),
                        );
                      return (
                        <section className="expert-team-run" key={record.id}>
                          <article className="sidebar-record-row parent-run-row">
                            <div>
                              <span>{formatTaskDate(record.updated_at, language)}</span>
                              <strong>{record.prompt}</strong>
                              <small>
                                {copy.runStatus.runStepsLabel(record.steps.length)}
                                {" / "}
                                {copy.runStatus.runArtifactsLabel(record.artifacts.length)}
                                {record.recovery_count > 0
                                  ? ` / ${copy.runStatus.recoveryLabel(record.recovery_count)}`
                                  : ""}
                                {record.expert_merge_receipt
                                  ? language === "zh"
                                    ? " / 已通过审查合并"
                                    : " / reviewed merge ready"
                                  : ""}
                              </small>
                              {record.status_reason ? <small>{record.status_reason}</small> : null}
                            </div>
                            <span className={`access-status ${record.status}`}>
                              {copy.runStatus.agentRunStatus[record.status]}
                            </span>
                          </article>
                          {expertAttempts.length > 0 ? (
                            <div className="expert-attempt-list">
                              {expertAttempts.map((attempt) => {
                                const budget = expertBudgetLabel(attempt);
                                const role = attempt.expert_contract?.role;
                                const roleLabel = role
                                  ? language === "zh"
                                    ? {
                                        research: "调研",
                                        analysis: "分析",
                                        production: "制作",
                                        review: "审查",
                                      }[role]
                                    : role
                                  : language === "zh"
                                    ? "子任务"
                                    : "subtask";
                                return (
                                  <article className="sidebar-record-row expert-attempt-row" key={attempt.id}>
                                    <div>
                                      <span>
                                        {roleLabel}
                                        {attempt.expert_contract
                                          ? ` · #${attempt.expert_contract.attempt}`
                                          : ""}
                                        {attempt.subtask_key ? ` · ${attempt.subtask_key}` : ""}
                                      </span>
                                      <strong>{attempt.prompt}</strong>
                                      {budget ? (
                                        <small>
                                          {language === "zh" ? "令牌" : "tokens"} {budget.tokens}
                                          {` / ${language === "zh" ? "工具" : "tools"} ${budget.tools}`}
                                          {` / ${language === "zh" ? "证据" : "evidence"} ${budget.evidence}`}
                                          {` / ${language === "zh" ? "冲突" : "conflicts"} ${budget.conflicts}`}
                                          {` / ${language === "zh" ? "闸门" : "gates"} ${budget.gatesPassed}/${budget.gatesTotal}`}
                                        </small>
                                      ) : null}
                                      {attempt.expert_result?.review ? (
                                        <small>
                                          {language === "zh" ? "审查" : "review"}: {attempt.expert_result.review.decision}
                                          {` · ${attempt.expert_result.review.target_revision.slice(0, 10)}`}
                                        </small>
                                      ) : null}
                                      {attempt.finish_error ? <small>{attempt.finish_error}</small> : null}
                                    </div>
                                    <span className={`access-status ${attempt.status}`}>
                                      {copy.runStatus.agentRunStatus[attempt.status]}
                                    </span>
                                  </article>
                                );
                              })}
                            </div>
                          ) : null}
                        </section>
                      );
                    })}
                  </div>
                </>
              ) : null}
              {recentToolInvocations.length > 0 ? (
                <>
                  <div className="queue-heading">
                    <strong>{copy.runStatus.recentTools}</strong>
                    <span>{recentToolInvocations.length}</span>
                  </div>
                  <div className="sidebar-record-list">
                    {recentToolInvocations.map((invocation) => {
                      const contract = agentToolContracts.find(
                        (candidate) => candidate.id === invocation.tool_id,
                      );
                      return (
                        <article className="sidebar-record-row" key={invocation.id}>
                          <div>
                            <span>{formatTaskDate(invocation.created_at, language)}</span>
                            <strong>{contract?.title ?? invocation.tool_id}</strong>
                            <small>
                              {invocation.verification.summary}
                              {invocation.evidence[0]
                                ? ` / ${invocation.evidence[0].reference}`
                                : ""}
                            </small>
                          </div>
                          <span className={`access-status ${invocation.status}`}>
                            {copy.runStatus.toolStatus[invocation.status]}
                          </span>
                        </article>
                      );
                    })}
                  </div>
                </>
              ) : null}
            </section>
            <details className="inspector-details">
              <summary>
                <ShieldCheck size={16} aria-hidden="true" />
                <span>{copy.runStatus.permissionsAndTools}</span>
              </summary>
            <section className="audit-panel" aria-labelledby="audit-panel-title">
              <div className="inspector-header compact">
                <ShieldCheck size={18} aria-hidden="true" />
                <strong id="audit-panel-title">{copy.capabilities.title}</strong>
              </div>
              {capabilityError ? <p className="package-error">{capabilityError}</p> : null}
              {taskGroupedAuthorizationNotice ? (
                <p className="package-message">{taskGroupedAuthorizationNotice}</p>
              ) : null}
              {taskGroupedAuthorizationError ? (
                <p className="package-error">{taskGroupedAuthorizationError}</p>
              ) : null}
              {taskGroupedAuthorizations.length > 0 ? (
                <div className="capability-grid">
                  {taskGroupedAuthorizations.map((authorization) => (
                    <TaskGroupedAuthorizationCard
                      key={authorization.intent.group_id}
                      authorization={authorization}
                      language={language}
                      capabilityLabels={copy.capabilityOptions}
                      riskLabels={copy.riskOptions}
                      pending={
                        taskGroupedAuthorizationPending === authorization.intent.group_id
                      }
                      onResolve={(intent, approved) =>
                        void resolveTaskGroupedAuthorization(intent, approved)
                      }
                      onRevoke={(intent) => void revokeTaskGroupedAuthorization(intent)}
                    />
                  ))}
                </div>
              ) : null}
              <form className="browser-tool" onSubmit={browseBrowserUrl}>
                <div className="tool-heading">
                  <Globe2 size={16} aria-hidden="true" />
                  <strong>{copy.browserTool.title}</strong>
                </div>
                <div className="browser-tool-row">
                  <input
                    value={browserUrl}
                    aria-label={copy.browserTool.urlPlaceholder}
                    placeholder={copy.browserTool.urlPlaceholder}
                    onChange={(event) => setBrowserUrl(event.target.value)}
                  />
                  <button type="submit" disabled={browserPending || capabilityPending !== null}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {browserPending ? copy.browserTool.browsing : copy.browserTool.browse}
                  </button>
                </div>
                {browserNotice ? <p className="package-message">{browserNotice}</p> : null}
                {browserError ? <p className="package-error">{browserError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={submitBrowserBoundary}>
                <div className="tool-heading">
                  <MousePointerClick size={16} aria-hidden="true" />
                  <strong>{copy.browserSubmitTool.title}</strong>
                </div>
                <div className="email-tool-fields">
                  <input
                    value={browserSubmitUrl}
                    aria-label={copy.browserSubmitTool.urlPlaceholder}
                    placeholder={copy.browserSubmitTool.urlPlaceholder}
                    onChange={(event) => setBrowserSubmitUrl(event.target.value)}
                  />
                  <input
                    value={browserSubmitSummary}
                    aria-label={copy.browserSubmitTool.summaryPlaceholder}
                    placeholder={copy.browserSubmitTool.summaryPlaceholder}
                    onChange={(event) => setBrowserSubmitSummary(event.target.value)}
                  />
                  <button
                    type="submit"
                    disabled={browserSubmitPending || capabilityPending !== null}
                  >
                    <MousePointerClick size={14} aria-hidden="true" />
                    {browserSubmitPending
                      ? copy.browserSubmitTool.requestingSubmit
                      : copy.browserSubmitTool.requestSubmit}
                  </button>
                </div>
                {browserSubmitNotice ? (
                  <p className="package-message">{browserSubmitNotice}</p>
                ) : null}
                {browserSubmitError ? (
                  <p className="package-error">{browserSubmitError}</p>
                ) : null}
              </form>
              <form className="browser-tool" onSubmit={searchNetworkBoundary}>
                <div className="tool-heading">
                  <Globe2 size={16} aria-hidden="true" />
                  <strong>{copy.networkSearchTool.title}</strong>
                </div>
                {modelToolStrategy.network_search_source_model_required ? (
                  <div
                    className="source-model-prompt"
                    role={networkSearchSourceModelMissing ? "alert" : undefined}
                  >
                    <strong>{copy.networkSearchTool.sourceModelRequiredTitle}</strong>
                    <p>{copy.networkSearchTool.sourceModelRequiredBody}</p>
                    <select
                      value={state.network_search_source_model ?? ""}
                      aria-label={copy.controls.networkSearchSourceModel}
                      onChange={updateNetworkSearchSourceModel}
                    >
                      <option value="">
                        {copy.networkSearchTool.sourceModelPlaceholder}
                      </option>
                      {modelToolStrategy.free_network_search_source_model_options.map(
                        (option) => (
                          <option
                            key={option.value}
                            value={option.value}
                            title={option.note}
                          >
                            {copy.networkSearchSourceOptions[option.value]}
                          </option>
                        ),
                      )}
                    </select>
                  </div>
                ) : null}
                <div className="email-tool-fields">
                  <input
                    value={networkSearchQuery}
                    aria-label={copy.networkSearchTool.queryPlaceholder}
                    placeholder={copy.networkSearchTool.queryPlaceholder}
                    onChange={(event) => setNetworkSearchQuery(event.target.value)}
                  />
                  <input
                    value={networkSearchScope}
                    aria-label={copy.networkSearchTool.scopePlaceholder}
                    placeholder={copy.networkSearchTool.scopePlaceholder}
                    onChange={(event) => setNetworkSearchScope(event.target.value)}
                  />
                  <button
                    type="submit"
                    disabled={
                      networkSearchPending ||
                      capabilityPending !== null ||
                      networkSearchSourceModelMissing ||
                      !networkSearchRouteStatus.network_requests_enabled
                    }
                  >
                    <MousePointerClick size={14} aria-hidden="true" />
                    {networkSearchPending
                      ? copy.networkSearchTool.requestingSearch
                      : copy.networkSearchTool.requestSearch}
                  </button>
                </div>
                {networkSearchNotice ? (
                  <p className="package-message">{networkSearchNotice}</p>
                ) : null}
                {networkSearchError ? (
                  <p className="package-error">{networkSearchError}</p>
                ) : null}
              </form>
              <form className="browser-tool" onSubmit={readLocalFilePath}>
                <div className="tool-heading">
                  <FileText size={16} aria-hidden="true" />
                  <strong>{copy.fileTool.title}</strong>
                </div>
                <div className="browser-tool-row">
                  <input
                    value={filePath}
                    aria-label={copy.fileTool.pathPlaceholder}
                    placeholder={copy.fileTool.pathPlaceholder}
                    onChange={(event) => setFilePath(event.target.value)}
                  />
                  <button type="submit" disabled={filePending || capabilityPending !== null}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {filePending ? copy.fileTool.reading : copy.fileTool.read}
                  </button>
                </div>
                {fileNotice ? <p className="package-message">{fileNotice}</p> : null}
                {fileError ? <p className="package-error">{fileError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={writeFileBoundary}>
                <div className="tool-heading">
                  <FileText size={16} aria-hidden="true" />
                  <strong>{copy.fileWriteTool.title}</strong>
                </div>
                <div className="email-tool-fields">
                  <input
                    value={fileWritePath}
                    aria-label={copy.fileWriteTool.pathPlaceholder}
                    placeholder={copy.fileWriteTool.pathPlaceholder}
                    onChange={(event) => setFileWritePath(event.target.value)}
                  />
                  <input
                    value={fileWriteSummary}
                    aria-label={copy.fileWriteTool.summaryPlaceholder}
                    placeholder={copy.fileWriteTool.summaryPlaceholder}
                    onChange={(event) => setFileWriteSummary(event.target.value)}
                  />
                  <textarea
                    value={fileWriteContent}
                    aria-label={copy.fileWriteTool.contentPlaceholder}
                    placeholder={copy.fileWriteTool.contentPlaceholder}
                    onChange={(event) => setFileWriteContent(event.target.value)}
                  />
                  <button type="submit" disabled={fileWritePending || capabilityPending !== null}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {fileWritePending
                      ? copy.fileWriteTool.requestingWrite
                      : copy.fileWriteTool.requestWrite}
                  </button>
                </div>
                {fileWriteNotice ? <p className="package-message">{fileWriteNotice}</p> : null}
                {fileWriteError ? <p className="package-error">{fileWriteError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={ingestEvidenceFolderPath}>
                <div className="tool-heading">
                  <FolderOpen size={16} aria-hidden="true" />
                  <strong>{copy.folderTool.title}</strong>
                </div>
                <div className="browser-tool-row">
                  <input
                    value={folderPath}
                    aria-label={copy.folderTool.pathPlaceholder}
                    placeholder={copy.folderTool.pathPlaceholder}
                    onChange={(event) => setFolderPath(event.target.value)}
                  />
                  <button type="submit" disabled={folderPending || capabilityPending !== null}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {folderPending ? copy.folderTool.ingesting : copy.folderTool.ingest}
                  </button>
                </div>
                {folderNotice ? <p className="package-message">{folderNotice}</p> : null}
                {folderError ? <p className="package-error">{folderError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={runTerminalReadCommand}>
                <div className="tool-heading">
                  <TerminalSquare size={16} aria-hidden="true" />
                  <strong>{copy.terminalTool.title}</strong>
                </div>
                <div className="browser-tool-row">
                  <select
                    value={terminalCommand}
                    aria-label={copy.terminalTool.commandLabel}
                    onChange={(event) =>
                      setTerminalCommand(event.target.value as TerminalReadCommand)
                    }
                  >
                    <option value="git status --short">
                      {copy.terminalTool.options["git status --short"]}
                    </option>
                    <option value="git diff --stat">
                      {copy.terminalTool.options["git diff --stat"]}
                    </option>
                    <option value="git branch --show-current">
                      {copy.terminalTool.options["git branch --show-current"]}
                    </option>
                    <option value="pwd">{copy.terminalTool.options.pwd}</option>
                  </select>
                  <button type="submit" disabled={terminalPending || capabilityPending !== null}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {terminalPending ? copy.terminalTool.running : copy.terminalTool.run}
                  </button>
                </div>
                {terminalNotice ? <p className="package-message">{terminalNotice}</p> : null}
                {terminalError ? <p className="package-error">{terminalError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={runTerminalWriteBoundary}>
                <div className="tool-heading">
                  <TerminalSquare size={16} aria-hidden="true" />
                  <strong>{copy.terminalTool.writeTitle}</strong>
                </div>
                <div className="browser-tool-row">
                  <input
                    value={terminalWriteCommand}
                    aria-label={copy.terminalTool.writeCommandLabel}
                    placeholder={copy.terminalTool.writePlaceholder}
                    onChange={(event) => setTerminalWriteCommand(event.target.value)}
                  />
                  <button
                    type="submit"
                    disabled={terminalWritePending || capabilityPending !== null}
                  >
                    <MousePointerClick size={14} aria-hidden="true" />
                    {terminalWritePending
                      ? copy.terminalTool.requestingWrite
                      : copy.terminalTool.requestWrite}
                  </button>
                </div>
                {terminalWriteNotice ? (
                  <p className="package-message">{terminalWriteNotice}</p>
                ) : null}
                {terminalWriteError ? (
                  <p className="package-error">{terminalWriteError}</p>
                ) : null}
              </form>
              <form className="browser-tool" onSubmit={readEmailBoundary}>
                <div className="tool-heading">
                  <Mail size={16} aria-hidden="true" />
                  <strong>{copy.emailReadTool.title}</strong>
                </div>
                <div className="email-tool-fields">
                  <input
                    value={emailMailbox}
                    aria-label={copy.emailReadTool.mailboxPlaceholder}
                    placeholder={copy.emailReadTool.mailboxPlaceholder}
                    onChange={(event) => setEmailMailbox(event.target.value)}
                  />
                  <input
                    value={emailReadQuery}
                    aria-label={copy.emailReadTool.queryPlaceholder}
                    placeholder={copy.emailReadTool.queryPlaceholder}
                    onChange={(event) => setEmailReadQuery(event.target.value)}
                  />
                  <button type="submit" disabled={emailReadPending || capabilityPending !== null}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {emailReadPending
                      ? copy.emailReadTool.requestingRead
                      : copy.emailReadTool.requestRead}
                  </button>
                </div>
                {emailReadNotice ? <p className="package-message">{emailReadNotice}</p> : null}
                {emailReadError ? <p className="package-error">{emailReadError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={createEmailDraftBoundary}>
                <div className="tool-heading">
                  <Mail size={16} aria-hidden="true" />
                  <strong>{copy.emailDraftTool.title}</strong>
                </div>
                <div className="email-tool-fields">
                  <input
                    value={draftEmailTo}
                    aria-label={copy.emailDraftTool.toPlaceholder}
                    placeholder={copy.emailDraftTool.toPlaceholder}
                    onChange={(event) => setDraftEmailTo(event.target.value)}
                  />
                  <input
                    value={draftEmailSubject}
                    aria-label={copy.emailDraftTool.subjectPlaceholder}
                    placeholder={copy.emailDraftTool.subjectPlaceholder}
                    onChange={(event) => setDraftEmailSubject(event.target.value)}
                  />
                  <textarea
                    value={draftEmailBody}
                    aria-label={copy.emailDraftTool.bodyPlaceholder}
                    placeholder={copy.emailDraftTool.bodyPlaceholder}
                    rows={3}
                    onChange={(event) => setDraftEmailBody(event.target.value)}
                  />
                  <button
                    type="submit"
                    disabled={emailDraftPending || capabilityPending !== null}
                  >
                    <MousePointerClick size={14} aria-hidden="true" />
                    {emailDraftPending
                      ? copy.emailDraftTool.requestingDraft
                      : copy.emailDraftTool.requestDraft}
                  </button>
                </div>
                {emailDraftNotice ? (
                  <p className="package-message">{emailDraftNotice}</p>
                ) : null}
                {emailDraftError ? <p className="package-error">{emailDraftError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={sendEmailBoundary}>
                <div className="tool-heading">
                  <Mail size={16} aria-hidden="true" />
                  <strong>{copy.emailTool.title}</strong>
                </div>
                <div className="email-tool-fields">
                  <input
                    value={emailTo}
                    aria-label={copy.emailTool.toPlaceholder}
                    placeholder={copy.emailTool.toPlaceholder}
                    onChange={(event) => setEmailTo(event.target.value)}
                  />
                  <input
                    value={emailSubject}
                    aria-label={copy.emailTool.subjectPlaceholder}
                    placeholder={copy.emailTool.subjectPlaceholder}
                    onChange={(event) => setEmailSubject(event.target.value)}
                  />
                  <textarea
                    value={emailBody}
                    aria-label={copy.emailTool.bodyPlaceholder}
                    placeholder={copy.emailTool.bodyPlaceholder}
                    rows={3}
                    onChange={(event) => setEmailBody(event.target.value)}
                  />
                  <button type="submit" disabled={emailPending || capabilityPending !== null}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {emailPending ? copy.emailTool.requestingSend : copy.emailTool.requestSend}
                  </button>
                </div>
                {emailNotice ? <p className="package-message">{emailNotice}</p> : null}
                {emailError ? <p className="package-error">{emailError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={readDriveBoundary}>
                <div className="tool-heading">
                  <Cloud size={16} aria-hidden="true" />
                  <strong>{copy.driveReadTool.title}</strong>
                </div>
                <div className="email-tool-fields">
                  <input
                    value={driveLocation}
                    aria-label={copy.driveReadTool.locationPlaceholder}
                    placeholder={copy.driveReadTool.locationPlaceholder}
                    onChange={(event) => setDriveLocation(event.target.value)}
                  />
                  <input
                    value={driveReadQuery}
                    aria-label={copy.driveReadTool.queryPlaceholder}
                    placeholder={copy.driveReadTool.queryPlaceholder}
                    onChange={(event) => setDriveReadQuery(event.target.value)}
                  />
                  <button type="submit" disabled={driveReadPending || capabilityPending !== null}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {driveReadPending
                      ? copy.driveReadTool.requestingRead
                      : copy.driveReadTool.requestRead}
                  </button>
                </div>
                {driveReadNotice ? <p className="package-message">{driveReadNotice}</p> : null}
                {driveReadError ? <p className="package-error">{driveReadError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={writeDriveBoundary}>
                <div className="tool-heading">
                  <Cloud size={16} aria-hidden="true" />
                  <strong>{copy.driveWriteTool.title}</strong>
                </div>
                <div className="email-tool-fields">
                  <input
                    value={driveWriteLocation}
                    aria-label={copy.driveWriteTool.locationPlaceholder}
                    placeholder={copy.driveWriteTool.locationPlaceholder}
                    onChange={(event) => setDriveWriteLocation(event.target.value)}
                  />
                  <input
                    value={driveWriteSummary}
                    aria-label={copy.driveWriteTool.summaryPlaceholder}
                    placeholder={copy.driveWriteTool.summaryPlaceholder}
                    onChange={(event) => setDriveWriteSummary(event.target.value)}
                  />
                  <button type="submit" disabled={driveWritePending || capabilityPending !== null}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {driveWritePending
                      ? copy.driveWriteTool.requestingWrite
                      : copy.driveWriteTool.requestWrite}
                  </button>
                </div>
                {driveWriteNotice ? <p className="package-message">{driveWriteNotice}</p> : null}
                {driveWriteError ? <p className="package-error">{driveWriteError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={captureComputerScreenshot}>
                <div className="tool-heading">
                  <MonitorCog size={16} aria-hidden="true" />
                  <strong>{copy.computerTool.title}</strong>
                </div>
                <div className="browser-tool-row">
                  <button type="submit" disabled={computerPending || capabilityPending !== null}>
                    <MousePointerClick size={14} aria-hidden="true" />
                    {computerPending ? copy.computerTool.capturing : copy.computerTool.capture}
                  </button>
                  {lastComputerScreenshotInvocationId && !activeComputerUseSession ? (
                    <button
                      type="button"
                      disabled={computerUseStepPending}
                      onClick={() => void startDurableComputerUseStep()}
                    >
                      <Play size={14} aria-hidden="true" />
                      {computerUseStepPending
                        ? computerUseStepCopy.starting
                        : computerUseStepCopy.start}
                    </button>
                  ) : null}
                </div>
                {computerNotice ? <p className="package-message">{computerNotice}</p> : null}
                {computerError ? <p className="package-error">{computerError}</p> : null}
              </form>
              <form className="browser-tool" onSubmit={unlockComputerControl}>
                <div className="tool-heading">
                  <ShieldCheck size={16} aria-hidden="true" />
                  <strong>{copy.computerControlTool.unlockTitle}</strong>
                </div>
                <div
                  className="computer-control-unlock-status"
                  data-unlocked={computerControlUnlockStatus.unlocked}
                >
                  <span>{copy.computerControlTool.unlockChallengeLabel}</span>
                  <code>{computerControlUnlockStatus.challenge || "------"}</code>
                  <span>
                    {computerControlUnlockStatus.unlocked && computerControlUnlockUntilText
                      ? `${copy.computerControlTool.unlockReady} ${copy.computerControlTool.unlockExpires} ${computerControlUnlockUntilText}`
                      : copy.computerControlTool.unlockRequired}
                  </span>
                </div>
                <div className="browser-tool-row">
                  <input
                    value={computerControlUnlockToken}
                    aria-label={copy.computerControlTool.unlockTokenPlaceholder}
                    placeholder={copy.computerControlTool.unlockTokenPlaceholder}
                    onChange={(event) => setComputerControlUnlockToken(event.target.value)}
                  />
                  <button
                    type="submit"
                    disabled={computerControlUnlockPending || capabilityPending !== null}
                  >
                    <ShieldCheck size={14} aria-hidden="true" />
                    {computerControlUnlockPending
                      ? copy.computerControlTool.unlockingControl
                      : copy.computerControlTool.unlockControl}
                  </button>
                </div>
                {computerControlUnlockNotice ? (
                  <p className="package-message">{computerControlUnlockNotice}</p>
                ) : null}
                {computerControlUnlockError ? (
                  <p className="package-error">{computerControlUnlockError}</p>
                ) : null}
              </form>
              <form className="browser-tool" onSubmit={controlComputerBoundary}>
                <div className="tool-heading">
                  <MonitorCog size={16} aria-hidden="true" />
                  <strong>{copy.computerControlTool.title}</strong>
                </div>
                <div className="email-tool-fields">
                  <input
                    value={computerControlTarget}
                    aria-label={copy.computerControlTool.targetPlaceholder}
                    placeholder={copy.computerControlTool.targetPlaceholder}
                    onChange={(event) => setComputerControlTarget(event.target.value)}
                  />
                  <input
                    value={computerControlAction}
                    aria-label={copy.computerControlTool.actionPlaceholder}
                    placeholder={copy.computerControlTool.actionPlaceholder}
                    onChange={(event) => setComputerControlAction(event.target.value)}
                  />
                  <button
                    type="submit"
                    disabled={computerControlPending || capabilityPending !== null}
                  >
                    <MousePointerClick size={14} aria-hidden="true" />
                    {computerControlPending
                      ? copy.computerControlTool.requestingControl
                      : copy.computerControlTool.requestControl}
                  </button>
                </div>
                {computerControlNotice ? (
                  <p className="package-message">{computerControlNotice}</p>
                ) : null}
                {computerControlError ? (
                  <p className="package-error">{computerControlError}</p>
                ) : null}
              </form>
              <div className="capability-grid">
                {capabilityCatalog.map((capability) => {
                  const Icon = capabilityFamilyIcon(capability.family);
                  const latestPendingRecord = capabilityRecords.find(
                    (record) =>
                      record.request.capability === capability.capability &&
                      record.effective_status === "pending_approval" &&
                      record.request.exact_tool === null &&
                      !agentActionPermissionRequestIds.has(record.request.id),
                  );
                  const latestRecord =
                    latestPendingRecord ??
                    capabilityRecords.find(
                      (record) => record.request.capability === capability.capability,
                    );

                  return (
                    <article className="capability-card" key={capability.capability}>
                      <div className="capability-card-header">
                        <Icon size={16} aria-hidden="true" />
                        <div>
                          <strong>{copy.capabilityOptions[capability.capability]}</strong>
                          <span>{copy.capabilityFamilyOptions[capability.family]}</span>
                        </div>
                        {capability.experimental ? (
                          <span className="experimental-label">{copy.capabilities.experimental}</span>
                        ) : null}
                      </div>
                      <p>{copy.capabilitySummaries[capability.capability]}</p>
                      <div className="capability-meta">
                        <span className={`risk ${capability.risk_level}`}>
                          {copy.riskOptions[capability.risk_level]}
                        </span>
                        {latestRecord ? (
                          <span className={`access-status ${latestRecord.effective_status}`}>
                            {copy.accessStatusOptions[latestRecord.effective_status]}
                          </span>
                        ) : null}
                        {latestRecord && latestRecord.grant_state !== "not_granted" ? (
                          <span className={`access-status ${latestRecord.grant_state}`}>
                            {copy.accessGrantOptions[latestRecord.grant_state]}
                          </span>
                        ) : null}
                      </div>
                      {latestPendingRecord ? (
                        <div className="capability-card-approval">
                          <button
                            type="button"
                            onClick={() =>
                              void resolveCapabilityAccess(latestPendingRecord.request.id, true)
                            }
                            disabled={capabilityPending !== null || resolutionPending !== null}
                          >
                            <Check size={14} aria-hidden="true" />
                            {resolutionPending === latestPendingRecord.request.id
                              ? copy.capabilities.resolving
                              : copy.capabilities.approve}
                          </button>
                          <button
                            type="button"
                            onClick={() =>
                              void resolveCapabilityAccess(latestPendingRecord.request.id, false)
                            }
                            disabled={capabilityPending !== null || resolutionPending !== null}
                          >
                            <X size={14} aria-hidden="true" />
                            {copy.capabilities.reject}
                          </button>
                        </div>
                      ) : (
                        <button
                          type="button"
                          onClick={() => void requestCapabilityAccess(capability.capability)}
                          disabled={capabilityPending !== null || resolutionPending !== null}
                        >
                          <MousePointerClick size={14} aria-hidden="true" />
                          {capabilityPending === capability.capability
                            ? copy.capabilities.requesting
                            : copy.capabilities.request}
                        </button>
                      )}
                    </article>
                  );
                })}
              </div>

              {auditError ? <p className="package-error">{auditError}</p> : null}
              <div className="tool-output">
                <div className="recent-audit-heading">{copy.browserTool.outputTitle}</div>
                {capabilityInvocations.length === 0 ? (
                  <p className="empty-state">{copy.browserTool.noOutput}</p>
                ) : (
                  <div className="tool-output-list">
                    {capabilityInvocations.slice(0, 4).map((invocation) => (
                      <article className="tool-output-row" key={invocation.id}>
                        <div>
                          <strong>
                            {invocation.title || copy.capabilityOptions[invocation.capability]}
                          </strong>
                          <span className={`access-status ${invocation.status}`}>
                            {copy.invocationStatusOptions[invocation.status]}
                          </span>
                        </div>
                        {invocation.excerpt ? <p>{invocation.excerpt}</p> : null}
                        {invocation.warnings.length > 0 ? (
                          <p>{invocation.warnings.join(" ")}</p>
                        ) : null}
                        <footer>
                          <span>{formatTaskDate(invocation.created_at, language)}</span>
                          {invocation.approval_request_id ? (
                            <span>
                              {copy.browserTool.approvalRequest}:{" "}
                              {invocation.approval_request_id}
                            </span>
                          ) : null}
                          {invocation.evidence_url || invocation.evidence_ref ? (
                            <span>{invocation.evidence_url || invocation.evidence_ref}</span>
                          ) : null}
                        </footer>
                      </article>
                    ))}
                  </div>
                )}
              </div>
              <div className="tool-output context-receipt-output">
                <div className="recent-audit-heading">
                  {copy.operationsBriefing.contextReceipt}
                </div>
                {memoryFeedbackNotice ? (
                  <p className="package-message">{memoryFeedbackNotice}</p>
                ) : null}
                {memoryFeedbackError ? (
                  <p className="package-error">{memoryFeedbackError}</p>
                ) : null}
                {agentContextReceipts.length === 0 ? (
                  <p className="empty-state">{copy.operationsBriefing.contextNoItems}</p>
                ) : (
                  <div className="tool-output-list">
                    {agentContextReceipts.slice(0, 3).map((receipt) => {
                      const summary = summarizeAgentContextReceipt(receipt);
                      const memoryFeedbackActions: Array<{
                        feedback: MemorySelectedFeedbackKind;
                        label: string;
                      }> = [
                        { feedback: "useful", label: copy.memoryFeedback.useful },
                        { feedback: "irrelevant", label: copy.memoryFeedback.irrelevant },
                        { feedback: "stale", label: copy.memoryFeedback.stale },
                        { feedback: "conflicting", label: copy.memoryFeedback.conflicting },
                        { feedback: "should_update", label: copy.memoryFeedback.shouldUpdate },
                      ];
                      return (
                        <article className="tool-output-row context-receipt-row" key={receipt.id}>
                          <div>
                            <strong>{summary.title}</strong>
                            <span className={`access-status ${summary.status}`}>
                              {summary.status}
                            </span>
                          </div>
                          {summary.evidence.length > 0 ? (
                            <p>
                              {copy.operationsBriefing.contextSelectedEvidence}:{" "}
                              {summary.evidence.join(" · ")}
                            </p>
                          ) : null}
                          {summary.memories.length > 0 ? (
                            <p>
                              {copy.operationsBriefing.contextSelectedMemories}:{" "}
                              {summary.memories.join(" · ")}
                            </p>
                          ) : null}
                          {summary.memoryRetrieval.length > 0 ? (
                            <p>
                              {copy.operationsBriefing.contextMemoryRetrieval}:{" "}
                              {summary.memoryRetrieval.join(" · ")}
                            </p>
                          ) : null}
                          {summary.memoryScores.length > 0 ? (
                            <p>
                              {copy.operationsBriefing.contextMemoryScores}:{" "}
                              {summary.memoryScores.join(" · ")}
                            </p>
                          ) : null}
                          {summary.memoryConflictHints.length > 0 ? (
                            <p>
                              {copy.operationsBriefing.contextMemoryConflictHints}:{" "}
                              {summary.memoryConflictHints.join(" · ")}
                            </p>
                          ) : null}
                          {summary.memoryCandidateGate.length > 0 ? (
                            <p>
                              {copy.operationsBriefing.contextMemoryCandidateGate}:{" "}
                              {summary.memoryCandidateGate.join(" · ")}
                            </p>
                          ) : null}
                          {summary.memoryFeedbackTargets.length > 0 ? (
                            <div className="memory-feedback-panel">
                              <strong>{copy.memoryFeedback.title}</strong>
                              {summary.memoryFeedbackTargets.map((target) => (
                                <div className="memory-feedback-row" key={target.memoryId}>
                                  <span>{target.title}</span>
                                  <div className="sidebar-row-actions">
                                    {memoryFeedbackActions.map((action) => {
                                      const pendingKey = `${receipt.id}:${target.memoryId}:${action.feedback}`;
                                      return (
                                        <button
                                          type="button"
                                          key={action.feedback}
                                          onClick={() =>
                                            void recordSelectedMemoryFeedback(
                                              receipt.id,
                                              target.memoryId,
                                              action.feedback,
                                            )
                                          }
                                          disabled={memoryFeedbackPending !== null}
                                        >
                                          {memoryFeedbackPending === pendingKey
                                            ? copy.memory.resolving
                                            : action.label}
                                        </button>
                                      );
                                    })}
                                  </div>
                                </div>
                              ))}
                            </div>
                          ) : null}
                          {summary.validation.length > 0 ? (
                            <p>
                              {copy.operationsBriefing.contextValidation}:{" "}
                              {summary.validation.join(" · ")}
                            </p>
                          ) : null}
                          {summary.policy.length > 0 ? (
                            <p>
                              {copy.runStatus.permissionsAndTools}:{" "}
                              {summary.policy.join(" · ")}
                            </p>
                          ) : null}
                          {summary.omissions.length > 0 ? (
                            <p>
                              {copy.operationsBriefing.contextIntentionalOmissions}:{" "}
                              {summary.omissions.join(" · ")}
                            </p>
                          ) : null}
                          <footer>
                            <span>{formatTaskDate(receipt.created_at, language)}</span>
                            {summary.meta.map((item) => (
                              <span key={item}>{item}</span>
                            ))}
                          </footer>
                        </article>
                      );
                    })}
                  </div>
                )}
              </div>
              <div className="recent-audit-heading">{copy.capabilities.auditTitle}</div>
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
            </details>
            <details className="inspector-details">
              <summary>
                <Brain size={16} aria-hidden="true" />
                <span>{copy.runStatus.routeDetails}</span>
              </summary>
            <dl>
              <div>
                <dt>{copy.inspector.largeModel}</dt>
                <dd>{copy.largeModelOptions[state.large_model_provider]}</dd>
              </div>
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
            <section className="tool-backend-list" aria-labelledby="tool-backend-title">
              <strong id="tool-backend-title">{copy.backendLabels.title}</strong>
              <dl>
                <div>
                  <dt>{copy.backendLabels.networkSearch}</dt>
                  <dd title={modelToolStrategy.note}>
                    {
                      copy.backendOptions.network_search[
                        modelToolStrategy.network_search_backend
                      ]
                    }
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.largeModelProvider}</dt>
                  <dd title={modelToolStrategy.note}>
                    {copy.largeModelOptions[modelToolStrategy.large_model_provider]}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.networkSearchSupport}</dt>
                  <dd title={modelToolStrategy.note}>
                    {modelToolStrategy.large_model_supports_network_search
                      ? copy.backendLabels.nativeSupported
                      : copy.backendLabels.sourceModelRequired}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.networkSearchSourceModel}</dt>
                  <dd title={modelToolStrategy.note}>
                    {modelToolStrategy.network_search_source_model
                      ? copy.networkSearchSourceOptions[
                          modelToolStrategy.network_search_source_model
                        ]
                      : copy.backendLabels.notSelected}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.networkSearchRoute}</dt>
                  <dd title={networkSearchRouteStatus.note}>
                    {copy.backendOptions.network_search[networkSearchRouteStatus.backend]}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.networkSearchExecution}</dt>
                  <dd title={networkSearchRouteStatus.note}>
                    {
                      copy.backendOptions.network_search_execution[
                        networkSearchRouteStatus.execution_mode
                      ]
                    }
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.networkSearchEvidence}</dt>
                  <dd title={networkSearchRouteStatus.note}>
                    {
                      copy.backendOptions.network_search_evidence[
                        networkSearchRouteStatus.evidence_policy
                      ]
                    }
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.networkRequests}</dt>
                  <dd title={networkSearchRouteStatus.note}>
                    {networkSearchRouteStatus.network_requests_enabled
                      ? copy.backendLabels.enabled
                      : copy.backendLabels.disabled}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.deepSeekOrchestration}</dt>
                  <dd title={networkSearchRouteStatus.note}>
                    {networkSearchRouteStatus.deepseek_orchestration_ready
                      ? copy.backendLabels.chatReady
                      : copy.backendLabels.chatNotReady}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.confirmationGate}</dt>
                  <dd title={networkSearchRouteStatus.note}>
                    {networkSearchRouteStatus.requires_user_confirmation
                      ? copy.backendLabels.confirmationRequired
                      : copy.backendLabels.confirmationNotRequired}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.deepSeekApi}</dt>
                  <dd>
                    {deepSeekCredentialStatus.configured
                      ? copy.backendLabels.apiKeyConfigured
                      : copy.backendLabels.apiKeyMissing}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.deepSeekChatApi}</dt>
                  <dd title={deepSeekReadinessMessage}>
                    {deepSeekCredentialStatus.chat_completion_ready
                      ? copy.backendLabels.chatReady
                      : copy.backendLabels.chatNotReady}
                  </dd>
                </div>
                <div>
                  <dt>{copy.onboarding.sourceLabel}</dt>
                  <dd>{copy.onboarding.credentialSource[deepSeekCredentialStatus.source]}</dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.deepSeekModels}</dt>
                  <dd>
                    {deepSeekCredentialStatus.flash_model} /{" "}
                    {deepSeekCredentialStatus.pro_model}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.deepSeekTelemetry}</dt>
                  <dd
                    title={
                      latestDeepSeekTelemetry
                        ? `${latestDeepSeekTelemetry.model} / ${latestDeepSeekTelemetry.request_hash}`
                        : ""
                    }
                  >
                    {latestDeepSeekTelemetryText}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.cacheEntries}</dt>
                  <dd className="backend-cache-control">
                    <span>{deepSeekChatCacheState.entries}</span>
                    <button
                      type="button"
                      onClick={() => void clearDeepSeekChatCache()}
                      disabled={deepSeekCachePending}
                    >
                      {deepSeekCachePending
                        ? copy.backendLabels.clearingCache
                        : copy.backendLabels.clearCache}
                    </button>
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.email}</dt>
                  <dd>{copy.backendOptions.email[state.tool_backends.email]}</dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.drive}</dt>
                  <dd>{copy.backendOptions.drive[state.tool_backends.drive]}</dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.computerScreenshot}</dt>
                  <dd>
                    {
                      copy.backendOptions.computer_screenshot[
                        modelToolStrategy.computer_screenshot_backend
                      ]
                    }
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.screenshotBackendStatus}</dt>
                  <dd title={computerUseBackendStatus.screenshot_note}>
                    {copy.backendOptions.computer_screenshot[
                      computerUseBackendStatus.screenshot_backend
                    ]}{" "}
                    /{" "}
                    {computerUseBackendStatus.screenshot_available
                      ? copy.backendLabels.backendAvailable
                      : copy.backendLabels.backendUnavailable}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.screenshotPermission}</dt>
                  <dd className="backend-permission-note">
                    <span>
                      {computerUseBackendStatus.screenshot_permission_required
                        ? copy.backendLabels.osPermissionRequired
                        : copy.backendLabels.osPermissionNotRequired}
                    </span>
                    <small>{computerUseBackendStatus.screenshot_permission_note}</small>
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.computerControl}</dt>
                  <dd>
                    {
                      copy.backendOptions.computer_control[
                        modelToolStrategy.computer_control_backend
                      ]
                    }
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.controlBackendStatus}</dt>
                  <dd title={computerUseBackendStatus.control_note}>
                    {copy.backendOptions.computer_control[computerUseBackendStatus.control_backend]}{" "}
                    /{" "}
                    {computerUseBackendStatus.control_available
                      ? copy.backendLabels.backendAvailable
                      : copy.backendLabels.backendUnavailable}
                    {computerUseBackendStatus.control_requires_approval
                      ? ` / ${copy.backendLabels.approvalRequired}`
                      : ""}
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.controlPermission}</dt>
                  <dd className="backend-permission-note">
                    <span>
                      {computerUseBackendStatus.control_permission_required
                        ? copy.backendLabels.osPermissionRequired
                        : copy.backendLabels.osPermissionNotRequired}
                    </span>
                    <small>{computerUseBackendStatus.control_permission_note}</small>
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.codexBridgeRuntime}</dt>
                  <dd className="backend-permission-note">
                    <span>
                      {computerUseBackendStatus.codex_bridge.required
                        ? copy.backendLabels.bridgeRequired
                        : copy.backendLabels.bridgeNotRequired}
                      {" / "}
                      {computerUseBackendStatus.codex_bridge.transport
                        ? copy.codexBridgeTransportOptions[
                            computerUseBackendStatus.codex_bridge.transport
                          ]
                        : copy.backendLabels.bridgeTransportMissing}
                      {" / "}
                      {computerUseBackendStatus.codex_bridge.endpoint_configured
                        ? copy.backendLabels.bridgeEndpointConfigured
                        : copy.backendLabels.bridgeEndpointMissing}
                      {" / "}
                      {computerUseBackendStatus.codex_bridge.connected
                        ? copy.backendLabels.bridgeConnected
                        : copy.backendLabels.bridgeNotConnected}
                    </span>
                    <small>
                      {computerUseBackendStatus.codex_bridge.transport_env_var} /{" "}
                      {computerUseBackendStatus.codex_bridge.endpoint_env_var}:{" "}
                      {computerUseBackendStatus.codex_bridge.note}
                    </small>
                  </dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.runtimePlatform}</dt>
                  <dd>{copy.runtimePlatformOptions[modelToolStrategy.runtime_platform]}</dd>
                </div>
                <div>
                  <dt>{copy.backendLabels.macosPath}</dt>
                  <dd>
                    {modelToolStrategy.macos_supported
                      ? copy.backendLabels.enabled
                      : copy.backendLabels.disabled}
                  </dd>
                </div>
              </dl>
              {deepSeekCacheNotice ? (
                <p className="package-message">{deepSeekCacheNotice}</p>
              ) : null}
              {deepSeekCacheError ? (
                <p className="package-error">{deepSeekCacheError}</p>
              ) : null}
            </section>
            </details>
          </aside>
          ) : (
            <aside className="run-rail-placeholder" aria-hidden="true" />
          )}
        </section>
      </section>
      {soulProfileModalOpen ? (
        <div className="setup-modal-backdrop" role="presentation">
          <section
            className="setup-modal soul-profile-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="soul-profile-modal-title"
          >
            <form
              className="setup-modal-form soul-profile-modal-form"
              onSubmit={(event) => {
                event.preventDefault();
                void saveSoulProfile();
              }}
            >
              <header>
                <Brain size={18} aria-hidden="true" />
                <div>
                  <h2 id="soul-profile-modal-title">
                    {copy.settingsPanel.soulProfileModalTitle}
                  </h2>
                  <p>{copy.settingsPanel.soulProfileModalDescription}</p>
                </div>
                <button
                  type="button"
                  className="modal-icon-button"
                  onClick={() => setSoulProfileModalOpen(false)}
                  aria-label={copy.settingsPanel.soulProfileClose}
                >
                  <X size={16} aria-hidden="true" />
                </button>
              </header>
              <div className="soul-profile-guide">
                {copy.settingsPanel.soulProfileGuides.map((guide) => (
                  <section key={guide.title}>
                    <h3>{guide.title}</h3>
                    <ul>
                      {guide.lines.map((line) => (
                        <li key={line}>{line}</li>
                      ))}
                    </ul>
                  </section>
                ))}
              </div>
              <label>
                <span>{copy.settingsPanel.soulProfile}</span>
                <textarea
                  value={soulProfileDraft}
                  aria-label={copy.settingsPanel.soulProfile}
                  placeholder={copy.settingsPanel.soulProfilePlaceholder}
                  onChange={(event) => setSoulProfileDraft(event.target.value)}
                />
              </label>
              {soulProfileNotice ? (
                <p className="package-message">{soulProfileNotice}</p>
              ) : null}
              {soulProfileError ? <p className="package-error">{soulProfileError}</p> : null}
              <div className="setup-modal-actions">
                <button type="submit" disabled={soulProfilePending}>
                  <Brain size={14} aria-hidden="true" />
                  {soulProfilePending
                    ? copy.settingsPanel.soulProfileSaving
                    : copy.settingsPanel.soulProfileSave}
                </button>
                <button
                  type="button"
                  onClick={() => setSoulProfileModalOpen(false)}
                  disabled={soulProfilePending}
                >
                  <X size={14} aria-hidden="true" />
                  {copy.settingsPanel.soulProfileClose}
                </button>
              </div>
            </form>
          </section>
        </div>
      ) : null}
      {agentSetupPrompt ? (
        <div className="setup-modal-backdrop" role="presentation">
          <section
            className="setup-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="agent-setup-modal-title"
          >
            {agentSetupPrompt === "deepseek_key" ? (
              <form className="setup-modal-form" onSubmit={continueAgentAfterDeepSeekKey}>
                <header>
                  <Brain size={18} aria-hidden="true" />
                  <div>
                    <h2 id="agent-setup-modal-title">{copy.chatWorkbench.deepSeekKeyTitle}</h2>
                    <p>{copy.chatWorkbench.deepSeekKeyBody}</p>
                  </div>
                </header>
                <input
                  type="password"
                  value={deepSeekApiKeyDraft}
                  aria-label={copy.chatWorkbench.deepSeekKeyPlaceholder}
                  placeholder={copy.chatWorkbench.deepSeekKeyPlaceholder}
                  autoFocus
                  autoComplete="off"
                  onChange={(event) => setDeepSeekApiKeyDraft(event.target.value)}
                />
                <p className="setup-status">{deepSeekReadinessMessage}</p>
                <p className="setup-help">{copy.onboarding.contactsDeepSeek}</p>
                {deepSeekReadinessError ? (
                  <p className="package-error">{deepSeekReadinessError}</p>
                ) : null}
                <div className="setup-modal-actions">
                  <button
                    type="submit"
                    disabled={deepSeekReadinessPending || !deepSeekApiKeyDraft.trim()}
                  >
                    <Send size={14} aria-hidden="true" />
                    {deepSeekReadinessPending
                      ? copy.onboarding.checking
                      : copy.onboarding.saveAndCheck}
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      setAgentSetupPrompt(null);
                      setPendingAgentPrompt("");
                    }}
                  >
                    <X size={14} aria-hidden="true" />
                    {copy.chatWorkbench.cancel}
                  </button>
                </div>
              </form>
            ) : null}

            {agentSetupPrompt === "workspace" ? (
              <form className="setup-modal-form" onSubmit={continueAgentAfterWorkspaceSetup}>
                <header>
                  <FolderOpen size={18} aria-hidden="true" />
                  <div>
                    <h2 id="agent-setup-modal-title">{copy.chatWorkbench.workspaceTitle}</h2>
                    <p>{copy.chatWorkbench.workspaceBody}</p>
                  </div>
                </header>
                <label>
                  <span>{copy.localSetup.workspaceName}</span>
                  <input
                    value={setupWorkspaceName}
                    aria-label={copy.localSetup.workspaceName}
                    placeholder={copy.localSetup.workspaceNamePlaceholder}
                    autoFocus
                    onChange={(event) => setSetupWorkspaceName(event.target.value)}
                  />
                </label>
                <label>
                  <span>{copy.localSetup.workspaceDir}</span>
                  <div className="setup-field">
                    <input
                      value={setupWorkspaceDir}
                      aria-label={copy.localSetup.workspaceDir}
                      placeholder={copy.localSetup.workspacePlaceholder}
                      onChange={(event) => setSetupWorkspaceDir(event.target.value)}
                    />
                    <button type="button" onClick={() => void chooseLocalDirectory()}>
                      <FolderOpen size={14} aria-hidden="true" />
                      {copy.localSetup.choose}
                    </button>
                  </div>
                </label>
                <p className="setup-status">{workspaceReadinessMessage}</p>
                <p className="setup-help">{copy.localSetup.managedStructure}</p>
                {setupError ? <p className="package-error">{setupError}</p> : null}
                <div className="setup-modal-actions">
                  <button type="submit" disabled={setupPending}>
                    <FolderOpen size={14} aria-hidden="true" />
                    {setupPending ? copy.localSetup.saving : copy.chatWorkbench.continue}
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      setAgentSetupPrompt(null);
                      setPendingAgentPrompt("");
                    }}
                    disabled={setupPending}
                  >
                    <X size={14} aria-hidden="true" />
                    {copy.chatWorkbench.cancel}
                  </button>
                </div>
              </form>
            ) : null}

            {agentSetupPrompt === "doctor" ? (
              <form className="setup-modal-form" onSubmit={continueAgentAfterDoctor}>
                <header>
                  <ShieldCheck size={18} aria-hidden="true" />
                  <div>
                    <h2 id="agent-setup-modal-title">{copy.onboarding.doctorTitle}</h2>
                    <p>{copy.onboarding.doctorBody}</p>
                  </div>
                </header>
                <div className="readiness-doctor-list">
                  <p>{deepSeekReadinessMessage}</p>
                  <p>{workspaceReadinessMessage}</p>
                  <p>
                    {deepSeekCredentialStatus.flash_model}:{" "}
                    {modelAvailabilityMessage(deepSeekCredentialStatus.flash_available)}
                  </p>
                  <p>
                    {deepSeekCredentialStatus.pro_model}:{" "}
                    {modelAvailabilityMessage(deepSeekCredentialStatus.pro_available)}
                  </p>
                </div>
                {deepSeekReadinessError ? (
                  <p className="package-error">{deepSeekReadinessError}</p>
                ) : null}
                <div className="setup-modal-actions">
                  <button
                    type="submit"
                    disabled={
                      deepSeekReadinessPending ||
                      (!deepSeekCredentialStatus.chat_completion_ready &&
                        !canRetryDeepSeekReadiness)
                    }
                  >
                    <RefreshCw size={14} aria-hidden="true" />
                    {deepSeekReadinessPending
                      ? copy.onboarding.checking
                      : copy.onboarding.retryCheck}
                  </button>
                  {workspaceReadiness.code !== "ready" ? (
                    <button
                      type="button"
                      onClick={() => setAgentSetupPrompt("workspace")}
                      disabled={deepSeekReadinessPending}
                    >
                      <FolderOpen size={14} aria-hidden="true" />
                      {copy.onboarding.chooseWorkspace}
                    </button>
                  ) : null}
                  <button
                    type="button"
                    onClick={() => {
                      setAgentSetupPrompt(null);
                      setPendingAgentPrompt("");
                    }}
                    disabled={deepSeekReadinessPending}
                  >
                    <X size={14} aria-hidden="true" />
                    {copy.chatWorkbench.cancel}
                  </button>
                </div>
              </form>
            ) : null}

            {agentSetupPrompt === "network_search" ? (
              <form className="setup-modal-form" onSubmit={continueAgentAfterNetworkSearchSetup}>
                <header>
                  <Globe2 size={18} aria-hidden="true" />
                  <div>
                    <h2 id="agent-setup-modal-title">{copy.chatWorkbench.networkSearchTitle}</h2>
                    <p>{copy.chatWorkbench.networkSearchBody}</p>
                  </div>
                </header>
                <label>
                  <span>{copy.controls.networkSearchSourceModel}</span>
                  <select
                    value={state.network_search_source_model ?? ""}
                    aria-label={copy.controls.networkSearchSourceModel}
                    onChange={updateNetworkSearchSourceModel}
                  >
                    <option value="">{copy.networkSearchTool.sourceModelPlaceholder}</option>
                    {modelToolStrategy.free_network_search_source_model_options.map((option) => (
                      <option key={option.value} value={option.value} title={option.note}>
                        {copy.networkSearchSourceOptions[option.value]}
                      </option>
                    ))}
                  </select>
                </label>
                {agentChatError ? <p className="package-error">{agentChatError}</p> : null}
                <div className="setup-modal-actions">
                  <button type="submit">
                    <Globe2 size={14} aria-hidden="true" />
                    {copy.chatWorkbench.continue}
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      setAgentSetupPrompt(null);
                      setPendingAgentPrompt("");
                    }}
                  >
                    <X size={14} aria-hidden="true" />
                    {copy.chatWorkbench.cancel}
                  </button>
                </div>
              </form>
            ) : null}
          </section>
        </div>
      ) : null}
    </main>
  );
}
