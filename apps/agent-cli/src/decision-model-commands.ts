import type { AgentCommand, RuntimeSnapshotDto } from "@lyonbrown/agent-client";

export function decisionModelCommand(
  snapshot: RuntimeSnapshotDto | null,
  action: "install" | "retry" | "cancel",
): AgentCommand {
  const status = snapshot?.decisionModel;
  if (status === null || status === undefined)
    throw new Error("decision model status is unavailable");
  if (action === "cancel") {
    if (status.operationId === null || !status.allowedActions.includes("cancel"))
      throw new Error("decision model installation cannot be cancelled");
    return { type: "cancel-model-download", purpose: "decision", operationId: status.operationId };
  }
  if (!status.allowedActions.includes(action === "install" ? "download" : "retry"))
    throw new Error("decision model installation is unavailable");
  return { type: "start-model-download", purpose: "decision", modelId: status.modelId };
}
