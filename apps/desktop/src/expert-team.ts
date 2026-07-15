import type { AgentRunRecord } from "./types";

const terminalStatuses = new Set(["completed", "failed", "cancelled"]);

export function latestExpertAttempts(records: AgentRunRecord[], parentRunId: string) {
  const latest = new Map<string, AgentRunRecord>();
  records
    .filter((record) => record.parent_run_id === parentRunId && record.expert_contract)
    .forEach((record) => {
      const contract = record.expert_contract!;
      const current = latest.get(contract.key.toLowerCase());
      if (!current || (current.expert_contract?.attempt ?? 0) < contract.attempt) {
        latest.set(contract.key.toLowerCase(), record);
      }
    });
  return [...latest.values()];
}

function resourcesConflict(left: AgentRunRecord, right: AgentRunRecord) {
  const leftResources = left.expert_contract?.resources ?? [];
  const rightResources = right.expert_contract?.resources ?? [];
  return leftResources.some((leftResource) =>
    rightResources.some(
      (rightResource) =>
        leftResource.key.toLowerCase() === rightResource.key.toLowerCase() &&
        (leftResource.access === "write" || rightResource.access === "write"),
    ),
  );
}

export function isExpertAttemptReady(record: AgentRunRecord, records: AgentRunRecord[]) {
  const contract = record.expert_contract;
  if (!contract) {
    return record.role === "subagent" && record.status === "queued";
  }
  if (record.status !== "queued") {
    return false;
  }
  const teamRecords = records.filter(
    (candidate) => candidate.expert_contract?.team_id === contract.team_id,
  );
  if (teamRecords.filter((candidate) => candidate.status === "running").length >= 3) {
    return false;
  }
  const latest = latestExpertAttempts(records, contract.parent_run_id);
  const dependenciesPassed = contract.depends_on.every((dependencyKey) => {
    const dependency = latest.find(
      (candidate) => candidate.expert_contract?.key.toLowerCase() === dependencyKey.toLowerCase(),
    );
    return (
      dependency?.status === "completed" &&
      dependency.expert_result?.quality_gates.length !== 0 &&
      dependency.expert_result?.quality_gates.every((gate) => gate.passed) === true
    );
  });
  if (!dependenciesPassed) {
    return false;
  }
  return !teamRecords.some(
    (candidate) =>
      candidate.id !== record.id &&
      candidate.status === "running" &&
      resourcesConflict(record, candidate),
  );
}

export function readyExpertAttempts(records: AgentRunRecord[], limit = 3) {
  return records
    .filter((record) => record.role === "subagent" && record.status === "queued")
    .filter((record) => isExpertAttemptReady(record, records))
    .sort((left, right) => left.started_at.localeCompare(right.started_at))
    .slice(0, Math.max(0, Math.min(3, limit)));
}

export function blockedParentsWithTerminalChildren(records: AgentRunRecord[]) {
  return records.filter((parent) => {
    if (parent.role !== "parent" || parent.status !== "blocked") {
      return false;
    }
    const children = records.filter((child) => child.parent_run_id === parent.id);
    return children.length > 0 && children.every((child) => terminalStatuses.has(child.status));
  });
}

export function expertTeamCanSynthesize(records: AgentRunRecord[], parentRunId: string) {
  const children = records.filter((record) => record.parent_run_id === parentRunId);
  if (children.every((record) => !record.expert_contract)) {
    return children.length > 0 && children.every((record) => terminalStatuses.has(record.status));
  }
  const latest = latestExpertAttempts(records, parentRunId);
  return (
    latest.length > 0 &&
    latest.every(
      (record) =>
        record.status === "completed" &&
        record.expert_result?.quality_gates.length !== 0 &&
        record.expert_result?.quality_gates.every((gate) => gate.passed) === true,
    )
  );
}

export function expertBudgetLabel(record: AgentRunRecord) {
  const contract = record.expert_contract;
  if (!contract) {
    return null;
  }
  const usage = record.expert_result?.usage;
  return {
    tokens: `${usage?.tokens ?? 0}/${contract.budget.max_tokens}`,
    tools: `${usage?.tool_calls ?? 0}/${contract.budget.max_tool_calls}`,
    evidence: record.expert_result?.evidence.filter((item) => item.verified).length ?? 0,
    conflicts: record.expert_result?.unresolved_conflicts.length ?? 0,
    gatesPassed:
      record.expert_result?.quality_gates.filter((gate) => gate.passed).length ?? 0,
    gatesTotal: record.expert_result?.quality_gates.length ?? 0,
  };
}
