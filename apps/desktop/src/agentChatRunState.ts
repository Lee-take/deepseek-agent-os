export type AgentChatComposerAction = "send" | "send_new_task" | "stop" | "send_guidance";

export type AgentChatGuidanceStatus = "idle" | "queued" | "guiding";

export type AgentChatLoopStepState = "done" | "current" | "waiting";

export type AgentChatRunStatus =
  | "queued"
  | "running"
  | "waiting_for_prerequisite"
  | "waiting_for_confirmation"
  | "blocked"
  | "cancel_requested"
  | "completed"
  | "failed"
  | "cancelled";

export type AgentChatQueuedGuidance = {
  id: string;
  content: string;
  attachment_count: number;
  created_at: string;
};

export type AgentChatRun = {
  id: string;
  conversation_id: string;
  prompt: string;
  status: AgentChatRunStatus;
  cancel_requested: boolean;
  queued_guidance: AgentChatQueuedGuidance[];
  created_at: string;
  updated_at: string;
};

export type AgentChatLoopStep = {
  key: string;
  label: string;
  detail: string;
  state: AgentChatLoopStepState;
};

export function agentChatComposerAction(input: {
  pending: boolean;
  draft: string;
  attachmentCount?: number;
  guidanceMode?: boolean;
}): AgentChatComposerAction {
  if (!input.pending) {
    return "send";
  }

  const hasPayload = Boolean(input.draft.trim()) || (input.attachmentCount ?? 0) > 0;
  if (!hasPayload) {
    return "stop";
  }

  if (input.guidanceMode) {
    return "send_guidance";
  }

  return input.pending ? "send_new_task" : "send";
}

export function shouldShowAgentStopControl(input: {
  pending: boolean;
  composerAction: AgentChatComposerAction;
}): boolean {
  return input.pending && input.composerAction !== "stop";
}

export function hasOpenAgentRunRecords(
  records: Array<{ status: AgentChatRunStatus }>,
): boolean {
  return records.some(
    (record) =>
      record.status === "queued" ||
      record.status === "running" ||
      record.status === "waiting_for_prerequisite" ||
      record.status === "waiting_for_confirmation" ||
      record.status === "blocked" ||
      record.status === "cancel_requested",
  );
}

export function latestParentAgentRunStatus(
  records: Array<{
    conversation_id: string;
    role: "parent" | "subagent";
    status: AgentChatRunStatus;
    updated_at: string;
  }>,
  conversationId: string,
): AgentChatRunStatus | undefined {
  return records
    .filter(
      (record) => record.role === "parent" && record.conversation_id === conversationId,
    )
    .sort(
      (left, right) =>
        new Date(right.updated_at).getTime() - new Date(left.updated_at).getTime(),
    )[0]?.status;
}

export function idleDeepSeekStepState(
  chatCompletionReady: boolean,
): "waiting" | "blocked" {
  return chatCompletionReady ? "waiting" : "blocked";
}

export function userFacingAgentReplyContent(content: string): string {
  const trimmed = content.trim();
  let jsonText = trimmed;
  if (!(trimmed.startsWith("{") && trimmed.endsWith("}"))) {
    const fenceStart = trimmed.indexOf("```");
    if (fenceStart < 0) {
      return content;
    }
    const afterFence = trimmed.slice(fenceStart + 3);
    const firstNewline = afterFence.indexOf("\n");
    if (firstNewline < 0) {
      return content;
    }
    const label = afterFence.slice(0, firstNewline).trim();
    if (label && label.toLowerCase() !== "json") {
      return content;
    }
    const fencedBody = afterFence.slice(firstNewline + 1);
    const fenceEnd = fencedBody.lastIndexOf("```");
    if (fenceEnd < 0) {
      return content;
    }
    jsonText = fencedBody.slice(0, fenceEnd).trim();
  }

  try {
    const parsed = JSON.parse(jsonText) as Record<string, unknown>;
    const reply = parsed.reply_to_user ?? parsed.reply ?? parsed.user_reply;
    if (typeof reply === "string" && reply.trim()) {
      return reply.trim();
    }
    if (reply && typeof reply === "object" && !Array.isArray(reply)) {
      const replyObject = reply as Record<string, unknown>;
      for (const key of ["content", "text", "message", "body"]) {
        const value = replyObject[key];
        if (typeof value === "string" && value.trim()) {
          return value.trim();
        }
      }
    }
  } catch {
    return content;
  }

  return content;
}

export function shouldRunDurableAgentWorker(input: {
  desktopRuntime: boolean;
  setupNeeded: boolean;
  workerBusy: boolean;
  chatPending: boolean;
  queuedLocalCount: number;
  credentialReady: boolean;
}): boolean {
  return (
    input.desktopRuntime &&
    !input.setupNeeded &&
    !input.workerBusy &&
    !input.chatPending &&
    input.queuedLocalCount === 0 &&
    input.credentialReady
  );
}

export function createAgentChatRun(input: {
  id: string;
  conversationId: string;
  prompt: string;
  createdAt: string;
}): AgentChatRun {
  return {
    id: input.id,
    conversation_id: input.conversationId,
    prompt: input.prompt,
    status: "running",
    cancel_requested: false,
    queued_guidance: [],
    created_at: input.createdAt,
    updated_at: input.createdAt,
  };
}

export function queueAgentRunGuidance(
  run: AgentChatRun,
  input: {
    id: string;
    content: string;
    createdAt: string;
    attachmentCount?: number;
  },
): AgentChatRun {
  const content = input.content.trim();
  if (!content && (input.attachmentCount ?? 0) === 0) {
    return run;
  }

  return {
    ...run,
    queued_guidance: [
      ...run.queued_guidance,
      {
        id: input.id,
        content,
        attachment_count: input.attachmentCount ?? 0,
        created_at: input.createdAt,
      },
    ],
    updated_at: input.createdAt,
  };
}

export function requestAgentRunCancel(run: AgentChatRun, requestedAt: string): AgentChatRun {
  return {
    ...run,
    status: "cancel_requested",
    cancel_requested: true,
    updated_at: requestedAt,
  };
}

export function finishAgentRun(
  run: AgentChatRun,
  input: { status: Extract<AgentChatRunStatus, "completed" | "failed">; finishedAt: string },
): AgentChatRun {
  return {
    ...run,
    status: input.status,
    updated_at: input.finishedAt,
  };
}

export function agentChatGuidanceStepState(status: AgentChatGuidanceStatus): "waiting" | "current" {
  return status === "guiding" ? "current" : "waiting";
}

export function agentChatLoopSteps(input: {
  pending: boolean;
  pendingStage: number;
  pendingStatus: string;
  guidanceStatus: AgentChatGuidanceStatus;
  queuedGuidance: string;
  labels: {
    goal: string;
    execute: string;
    guidance: string;
    verify: string;
  };
  details: {
    goal: string;
    verify: string;
  };
}): AgentChatLoopStep[] {
  if (!input.pending) {
    return [];
  }

  const executeState: AgentChatLoopStepState =
    input.pendingStage >= 3 ? "done" : input.pendingStage >= 1 ? "current" : "waiting";
  const verifyState: AgentChatLoopStepState = input.pendingStage >= 3 ? "current" : "waiting";

  return [
    {
      key: "agent-chat-goal",
      label: input.labels.goal,
      detail: input.details.goal,
      state: input.pendingStage >= 1 ? "done" : "current",
    },
    {
      key: "agent-chat-execute",
      label: input.labels.execute,
      detail: input.pendingStatus,
      state: executeState,
    },
    ...(input.guidanceStatus !== "idle"
      ? [
          {
            key: "agent-chat-guidance",
            label: input.labels.guidance,
            detail:
              input.guidanceStatus === "guiding" ? input.pendingStatus : input.queuedGuidance,
            state: agentChatGuidanceStepState(input.guidanceStatus),
          },
        ]
      : []),
    {
      key: "agent-chat-verify",
      label: input.labels.verify,
      detail: input.details.verify,
      state: verifyState,
    },
  ];
}

export function buildAgentGuidancePrompt(guidance: string): string {
  return [
    "补充说明：",
    guidance.trim(),
    "",
    "请把这条补充说明并入刚才正在执行的同一任务中，当前小节点完成后继续按同一任务统一考虑。",
  ].join("\n");
}
