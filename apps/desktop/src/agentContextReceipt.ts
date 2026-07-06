import type { AgentContextReceipt } from "./types";

export type AgentContextReceiptSummary = {
  title: string;
  status: string;
  meta: string[];
  evidence: string[];
  memories: string[];
  validation: string[];
  policy: string[];
  omissions: string[];
};

const MAX_LIST_ITEMS = 2;

export function summarizeAgentContextReceipt(
  receipt: AgentContextReceipt,
): AgentContextReceiptSummary {
  return {
    title: `${receipt.loop_mode} / ${receipt.action_type}`,
    status: receipt.execution_state,
    meta: compactList([receipt.model_route, receipt.thinking_level, receipt.token_cache_state]),
    evidence: receipt.selected_evidence.slice(0, MAX_LIST_ITEMS),
    memories: receipt.selected_memories.slice(0, MAX_LIST_ITEMS),
    validation: receipt.validation_results.slice(0, MAX_LIST_ITEMS),
    policy: compactList([
      labelList("constraints", receipt.policy_constraints ?? []),
      labelList("tools", receipt.allowed_tools),
      labelList("validators", receipt.validators),
      labelList("stop", receipt.stop_conditions),
      labelList("matched", receipt.matched_stop_conditions ?? []),
      receipt.confirmation_rule ? `confirm: ${receipt.confirmation_rule}` : "",
    ]),
    omissions: receipt.intentional_omissions.slice(0, MAX_LIST_ITEMS),
  };
}

function labelList(label: string, values: string[]): string {
  const visibleValues = values.slice(0, MAX_LIST_ITEMS);
  if (visibleValues.length === 0) {
    return "";
  }
  return `${label}: ${visibleValues.join(", ")}`;
}

function compactList(values: Array<string | null | undefined>): string[] {
  return values.map((value) => value?.trim() ?? "").filter(Boolean);
}
