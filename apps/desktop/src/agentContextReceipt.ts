import type { AgentContextReceipt } from "./types";

export type AgentContextReceiptSummary = {
  title: string;
  status: string;
  meta: string[];
  evidence: string[];
  memories: string[];
  memoryRetrieval: string[];
  memoryScores: string[];
  memoryConflictHints: string[];
  memoryCandidateGate: string[];
  validation: string[];
  policy: string[];
  omissions: string[];
};

const MAX_LIST_ITEMS = 2;

export function summarizeAgentContextReceipt(
  receipt: AgentContextReceipt,
): AgentContextReceiptSummary {
  const retrieval = memoryRetrievalSummary(receipt.selected_memories);
  const selectedMemoryLines = receipt.selected_memories.filter((memory) =>
    memory.startsWith("memory_id="),
  );
  return {
    title: `${receipt.loop_mode} / ${receipt.action_type}`,
    status: receipt.execution_state,
    meta: compactList([receipt.model_route, receipt.thinking_level, receipt.token_cache_state]),
    evidence: receipt.selected_evidence.slice(0, MAX_LIST_ITEMS),
    memories: memoryTitleSummary(selectedMemoryLines),
    memoryRetrieval: retrieval.overview,
    memoryScores: memoryScoreSummary(selectedMemoryLines),
    memoryConflictHints: retrieval.conflictHints,
    memoryCandidateGate: receipt.memory_candidate_gate.slice(0, MAX_LIST_ITEMS),
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

function memoryRetrievalSummary(selectedMemories: string[]): {
  overview: string[];
  conflictHints: string[];
} {
  const retrievalLine = selectedMemories.find((memory) =>
    memory.startsWith("memory_retrieval=memory_runtime/v1"),
  );
  if (!retrievalLine) {
    return { overview: [], conflictHints: [] };
  }

  const fields = parseReceiptFields(retrievalLine);
  const selectedCount = fieldValue(fields, "selected_count");
  const candidateCount = fieldValue(fields, "candidate_count");
  const consideredRecords = fieldValue(fields, "considered_records");
  const maxRecords = fieldValue(fields, "max_records");
  const usedBytes = fieldValue(fields, "used_bytes");
  const maxBytes = fieldValue(fields, "max_bytes");
  const queryTerms = fieldValue(fields, "query_terms_count");
  const overview = compactList([
    selectedCount && candidateCount && consideredRecords
      ? `retrieval v1: selected ${selectedCount}/${candidateCount} candidates from ${consideredRecords} reviewed memories`
      : "",
    selectedCount && maxRecords && usedBytes && maxBytes && queryTerms
      ? `budget: records ${selectedCount}/${maxRecords}, bytes ${usedBytes}/${maxBytes}, query terms ${queryTerms}`
      : "",
  ]);

  const conflictHints = compactList([
    positiveField(fields, "filtered_sensitive")
      ? `${fields.filtered_sensitive} sensitive memory omitted from prompt context`
      : "",
    positiveField(fields, "filtered_archived")
      ? `${fields.filtered_archived} archived or stale memory omitted from prompt context`
      : "",
    positiveField(fields, "omitted_by_budget")
      ? `${fields.omitted_by_budget} lower-ranked memory omitted by retrieval budget`
      : "",
  ]);

  return { overview, conflictHints };
}

function memoryTitleSummary(memoryLines: string[]): string[] {
  return memoryLines
    .map((memory) => fieldValue(parseReceiptFields(memory), "title"))
    .filter(Boolean)
    .slice(0, MAX_LIST_ITEMS);
}

function memoryScoreSummary(memoryLines: string[]): string[] {
  return memoryLines
    .map((memory) => {
      const fields = parseReceiptFields(memory);
      const title = fieldValue(fields, "title");
      if (!title) {
        return "";
      }
      const scoreBreakdown = fieldValue(fields, "score_breakdown");
      const scoringDetails = compactList([
        fieldValue(fields, "match_reason") || "match_reason=unknown",
        fieldValue(fields, "inclusion_mode") || "compact",
        scoreBreakdown,
      ]).join("; ");
      return `rank ${fieldValue(fields, "rank") || "?"} score ${
        fieldValue(fields, "score") || "?"
      }: ${title} (${fieldValue(fields, "type") || "unknown"}/${
        fieldValue(fields, "scope") || "unknown"
      }; ${scoringDetails})`;
    })
    .filter(Boolean)
    .slice(0, MAX_LIST_ITEMS);
}

function parseReceiptFields(line: string): Record<string, string> {
  return line
    .split(";")
    .map((part) => part.trim())
    .filter(Boolean)
    .reduce<Record<string, string>>((fields, part) => {
      const separator = part.indexOf("=");
      if (separator <= 0) {
        return fields;
      }
      fields[part.slice(0, separator).trim()] = part.slice(separator + 1).trim();
      return fields;
    }, {});
}

function fieldValue(fields: Record<string, string>, key: string): string {
  return fields[key]?.trim() ?? "";
}

function positiveField(fields: Record<string, string>, key: string): boolean {
  return Number.parseInt(fieldValue(fields, key), 10) > 0;
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
