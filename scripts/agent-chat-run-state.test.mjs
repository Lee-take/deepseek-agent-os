#!/usr/bin/env node

import assert from "node:assert/strict";
import { test } from "node:test";

const runStateModuleUrl = new URL("../apps/desktop/src/agentChatRunState.ts", import.meta.url);

const {
  agentChatComposerAction,
  agentChatGuidanceStepState,
  agentChatLoopSteps,
  createAgentChatRun,
  hasOpenAgentRunRecords,
  idleDeepSeekStepState,
  latestParentAgentRunStatus,
  queueAgentRunGuidance,
  requestAgentRunCancel,
  shouldRunDurableAgentWorker,
  shouldShowAgentStopControl,
  userFacingAgentReplyContent,
  buildAgentGuidancePrompt,
} = await import(runStateModuleUrl);

test("uses the latest parent run from the active conversation for terminal workflow state", () => {
  const status = latestParentAgentRunStatus(
    [
      {
        conversation_id: "conversation-other",
        role: "parent",
        status: "waiting_for_confirmation",
        updated_at: "2026-07-16T12:53:00.000Z",
      },
      {
        conversation_id: "conversation-active",
        role: "subagent",
        status: "failed",
        updated_at: "2026-07-16T12:52:30.000Z",
      },
      {
        conversation_id: "conversation-active",
        role: "parent",
        status: "completed",
        updated_at: "2026-07-16T12:52:00.000Z",
      },
    ],
    "conversation-active",
  );

  assert.equal(status, "completed");
});

test("does not represent DeepSeek credential readiness as user confirmation", () => {
  assert.equal(idleDeepSeekStepState(true), "waiting");
  assert.equal(idleDeepSeekStepState(false), "blocked");
});

test("renders the user reply instead of persisted protocol JSON", () => {
  const persistedContent = `\`\`\`json
{
  "protocol_version": "1.0",
  "reply_to_user": "这是用户应该看到的正文。",
  "agent_actions": [],
  "subagent_plan": null
}
\`\`\``;

  assert.equal(userFacingAgentReplyContent(persistedContent), "这是用户应该看到的正文。");
});

test("keeps ordinary assistant content unchanged", () => {
  const content = "这是普通回复，不含协议外壳。";
  assert.equal(userFacingAgentReplyContent(content), content);
});

test("starts the durable worker only when desktop recovery is idle and configured", () => {
  assert.equal(
    shouldRunDurableAgentWorker({
      desktopRuntime: true,
      setupNeeded: false,
      workerBusy: false,
      chatPending: false,
      queuedLocalCount: 0,
      credentialReady: true,
    }),
    true,
  );
});

test("does not race the durable worker with foreground or local queued work", () => {
  const ready = {
    desktopRuntime: true,
    setupNeeded: false,
    workerBusy: false,
    chatPending: false,
    queuedLocalCount: 0,
    credentialReady: true,
  };
  assert.equal(shouldRunDurableAgentWorker({ ...ready, desktopRuntime: false }), false);
  assert.equal(shouldRunDurableAgentWorker({ ...ready, setupNeeded: true }), false);
  assert.equal(shouldRunDurableAgentWorker({ ...ready, workerBusy: true }), false);
  assert.equal(shouldRunDurableAgentWorker({ ...ready, chatPending: true }), false);
  assert.equal(shouldRunDurableAgentWorker({ ...ready, queuedLocalCount: 1 }), false);
  assert.equal(shouldRunDurableAgentWorker({ ...ready, credentialReady: false }), false);
});

test("uses a stop action while an agent task is running without draft guidance", () => {
  assert.equal(agentChatComposerAction({ pending: true, draft: "" }), "stop");
  assert.equal(agentChatComposerAction({ pending: true, draft: "   " }), "stop");
});

test("keeps the main composer available for a new queued task while a run is active", () => {
  assert.equal(
    agentChatComposerAction({ pending: true, draft: "继续检查下一份文件" }),
    "send_new_task",
  );
});

test("uses a send-guidance action only when the user explicitly targets the active run", () => {
  assert.equal(
    agentChatComposerAction({
      pending: true,
      draft: "补充：先打开网页",
      guidanceMode: true,
    }),
    "send_guidance",
  );
});

test("returns to stop action after submitted guidance clears the draft", () => {
  assert.equal(
    agentChatComposerAction({
      pending: true,
      draft: "补充：优先处理当前文件",
      guidanceMode: true,
    }),
    "send_guidance",
  );
  assert.equal(agentChatComposerAction({ pending: true, draft: "" }), "stop");
});

test("keeps attachments in the main composer as a new queued task while a run is active", () => {
  assert.equal(
    agentChatComposerAction({ pending: true, draft: "", attachmentCount: 1 }),
    "send_new_task",
  );
});

test("uses send-guidance action for attachments only in explicit guidance mode", () => {
  assert.equal(
    agentChatComposerAction({
      pending: true,
      draft: "",
      attachmentCount: 1,
      guidanceMode: true,
    }),
    "send_guidance",
  );
});

test("keeps a visible stop control when the active composer action queues a new task", () => {
  assert.equal(
    shouldShowAgentStopControl({
      pending: true,
      composerAction: "send_new_task",
    }),
    true,
  );
  assert.equal(
    shouldShowAgentStopControl({
      pending: true,
      composerAction: "send_guidance",
    }),
    true,
  );
  assert.equal(
    shouldShowAgentStopControl({
      pending: true,
      composerAction: "stop",
    }),
    false,
  );
  assert.equal(
    shouldShowAgentStopControl({
      pending: false,
      composerAction: "send",
    }),
    false,
  );
});

test("keeps normal send action when no agent task is running", () => {
  assert.equal(agentChatComposerAction({ pending: false, draft: "新任务" }), "send");
});

test("keeps the run inspector live while durable records are still open", () => {
  assert.equal(
    hasOpenAgentRunRecords([
      { status: "completed" },
      { status: "queued" },
    ]),
    true,
  );
  assert.equal(hasOpenAgentRunRecords([{ status: "running" }]), true);
  assert.equal(hasOpenAgentRunRecords([{ status: "waiting_for_prerequisite" }]), true);
  assert.equal(hasOpenAgentRunRecords([{ status: "waiting_for_confirmation" }]), true);
  assert.equal(hasOpenAgentRunRecords([{ status: "blocked" }]), true);
  assert.equal(hasOpenAgentRunRecords([{ status: "cancel_requested" }]), true);
  assert.equal(
    hasOpenAgentRunRecords([
      { status: "completed" },
      { status: "failed" },
      { status: "cancelled" },
    ]),
    false,
  );
});

test("marks queued and active guidance distinctly for the run inspector", () => {
  assert.equal(agentChatGuidanceStepState("queued"), "waiting");
  assert.equal(agentChatGuidanceStepState("guiding"), "current");
  assert.equal(agentChatGuidanceStepState("idle"), "waiting");
});

test("builds a guidance prompt that preserves the user supplement as one task", () => {
  const prompt = buildAgentGuidancePrompt("把结果限制在桌面已有文件内");

  assert.match(prompt, /补充说明/);
  assert.match(prompt, /同一任务/);
  assert.match(prompt, /把结果限制在桌面已有文件内/);
});

test("builds bounded loop steps for a running agent task", () => {
  const steps = agentChatLoopSteps({
    pending: true,
    pendingStage: 0,
    pendingStatus: "正在预处理指令并检查本地状态",
    guidanceStatus: "idle",
    queuedGuidance: "",
    labels: {
      goal: "理解任务",
      execute: "调用 DeepSeek",
      guidance: "补充指令",
      verify: "校验结果",
    },
    details: {
      goal: "建立目标和完成标准",
      verify: "按目标验证真实结果",
    },
  });

  assert.deepEqual(
    steps.map((step) => [step.key, step.state]),
    [
      ["agent-chat-goal", "current"],
      ["agent-chat-execute", "waiting"],
      ["agent-chat-verify", "waiting"],
    ],
  );
});

test("moves loop steps toward verification as the run progresses", () => {
  const steps = agentChatLoopSteps({
    pending: true,
    pendingStage: 3,
    pendingStatus: "请求仍在进行中，本地程序没有宕机",
    guidanceStatus: "queued",
    queuedGuidance: "补充：优先检查本地文件",
    labels: {
      goal: "理解任务",
      execute: "调用 DeepSeek",
      guidance: "补充指令",
      verify: "校验结果",
    },
    details: {
      goal: "建立目标和完成标准",
      verify: "按目标验证真实结果",
    },
  });

  assert.deepEqual(
    steps.map((step) => [step.key, step.state]),
    [
      ["agent-chat-goal", "done"],
      ["agent-chat-execute", "done"],
      ["agent-chat-guidance", "waiting"],
      ["agent-chat-verify", "current"],
    ],
  );
  assert.equal(steps[2].detail, "补充：优先检查本地文件");
});

test("creates an active background run with a stable id and audit timestamps", () => {
  const run = createAgentChatRun({
    id: "run-1",
    conversationId: "conversation-1",
    prompt: "整理这份材料",
    createdAt: "2026-07-09T10:00:00.000Z",
  });

  assert.equal(run.id, "run-1");
  assert.equal(run.conversation_id, "conversation-1");
  assert.equal(run.status, "running");
  assert.equal(run.prompt, "整理这份材料");
  assert.equal(run.created_at, "2026-07-09T10:00:00.000Z");
  assert.equal(run.updated_at, "2026-07-09T10:00:00.000Z");
  assert.deepEqual(run.queued_guidance, []);
});

test("queues follow-up guidance on the active run without replacing the composer draft model", () => {
  const run = createAgentChatRun({
    id: "run-1",
    conversationId: "conversation-1",
    prompt: "整理这份材料",
    createdAt: "2026-07-09T10:00:00.000Z",
  });

  const updated = queueAgentRunGuidance(run, {
    id: "guidance-1",
    content: "补充：先检查本地证据",
    attachmentCount: 1,
    createdAt: "2026-07-09T10:00:05.000Z",
  });

  assert.equal(updated.status, "running");
  assert.equal(updated.updated_at, "2026-07-09T10:00:05.000Z");
  assert.deepEqual(updated.queued_guidance, [
    {
      id: "guidance-1",
      content: "补充：先检查本地证据",
      attachment_count: 1,
      created_at: "2026-07-09T10:00:05.000Z",
    },
  ]);
});

test("marks a running background run as cancel-requested while preserving queued guidance", () => {
  const run = queueAgentRunGuidance(
    createAgentChatRun({
      id: "run-1",
      conversationId: "conversation-1",
      prompt: "整理这份材料",
      createdAt: "2026-07-09T10:00:00.000Z",
    }),
    {
      id: "guidance-1",
      content: "补充：先检查本地证据",
      createdAt: "2026-07-09T10:00:05.000Z",
    },
  );

  const updated = requestAgentRunCancel(run, "2026-07-09T10:00:07.000Z");

  assert.equal(updated.status, "cancel_requested");
  assert.equal(updated.cancel_requested, true);
  assert.equal(updated.updated_at, "2026-07-09T10:00:07.000Z");
  assert.equal(updated.queued_guidance.length, 1);
});
