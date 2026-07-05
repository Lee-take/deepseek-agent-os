#!/usr/bin/env node

import assert from "node:assert/strict";
import { test } from "node:test";

const runStateModuleUrl = new URL("../apps/desktop/src/agentChatRunState.ts", import.meta.url);

const {
  agentChatComposerAction,
  agentChatGuidanceStepState,
  agentChatLoopSteps,
  buildAgentGuidancePrompt,
} = await import(runStateModuleUrl);

test("uses a stop action while an agent task is running without draft guidance", () => {
  assert.equal(agentChatComposerAction({ pending: true, draft: "" }), "stop");
  assert.equal(agentChatComposerAction({ pending: true, draft: "   " }), "stop");
});

test("uses a send-guidance action when the user types during a running task", () => {
  assert.equal(agentChatComposerAction({ pending: true, draft: "补充：先打开网页" }), "send_guidance");
});

test("returns to stop action after submitted guidance clears the draft", () => {
  assert.equal(agentChatComposerAction({ pending: true, draft: "补充：优先处理当前文件" }), "send_guidance");
  assert.equal(agentChatComposerAction({ pending: true, draft: "" }), "stop");
});

test("uses send-guidance action when attachments are added during a running task", () => {
  assert.equal(agentChatComposerAction({ pending: true, draft: "", attachmentCount: 1 }), "send_guidance");
});

test("keeps normal send action when no agent task is running", () => {
  assert.equal(agentChatComposerAction({ pending: false, draft: "新任务" }), "send");
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
