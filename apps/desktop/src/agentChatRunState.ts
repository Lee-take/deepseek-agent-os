export type AgentChatComposerAction = "send" | "stop" | "send_guidance";

export type AgentChatGuidanceStatus = "idle" | "queued" | "guiding";

export type AgentChatLoopStepState = "done" | "current" | "waiting";

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
}): AgentChatComposerAction {
  if (!input.pending) {
    return "send";
  }

  return input.draft.trim() || (input.attachmentCount ?? 0) > 0 ? "send_guidance" : "stop";
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
