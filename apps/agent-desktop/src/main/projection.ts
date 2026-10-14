import type { RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import type { DesktopModel, DesktopState } from "@lyonbrown/agent-desktop/internal/shared/contract";
const ACTIVE = new Set([
  "preparing",
  "streaming",
  "awaiting-approval",
  "running-tool",
  "cancelling",
]);
export function projectSnapshot(
  snapshot: RuntimeSnapshotDto,
  source: "reused" | "spawned",
  connectionToken = "fixture",
): DesktopState {
  const models: DesktopModel[] = [];
  const chat = snapshot.modelSetup;
  if (chat)
    models.push({
      purpose: "chat",
      title: chat.managedModel?.displayName ?? chat.managedModel?.id ?? "Chat model",
      phase: chat.phase,
      downloadedBytes: chat.downloadedBytes.toString(),
      totalBytes: chat.totalBytes?.toString() ?? null,
      actions: chat.allowedActions,
    });
  const decision = snapshot.decisionModel;
  if (decision)
    models.push({
      purpose: "decision",
      title: decision.modelId,
      phase: decision.phase,
      downloadedBytes: decision.downloadedBytes.toString(),
      totalBytes: decision.totalBytes.toString(),
      actions: decision.allowedActions,
    });
  const approval = snapshot.pendingApproval;
  const latestTurn = snapshot.turns.at(-1);
  const submitted = latestTurn?.status === "pending" || latestTurn?.status === "running";
  const active = submitted || ACTIVE.has(snapshot.phase);
  return {
    status: "connected",
    source,
    revision: snapshot.revision.toString(),
    actionToken: connectionToken + ":" + snapshot.revision.toString(),
    phase: submitted && snapshot.phase === "ready" ? "preparing" : snapshot.phase,
    messages: snapshot.turns.flatMap((turn) => [
      { id: turn.turnId + ":user", role: "user" as const, text: turn.userText },
      { id: turn.turnId + ":assistant", role: "assistant" as const, text: turn.assistantText },
    ]),
    canSend:
      ["ready", "completed", "failed-recoverable"].includes(snapshot.phase) &&
      approval === null &&
      !active,
    canCancel: active && snapshot.phase !== "cancelling",
    approval: approval
      ? {
          tool: approval.toolName,
          risk: approval.preview?.risk ?? "unknown",
          target: approval.preview?.target ?? "Not provided",
        }
      : null,
    models,
    error: snapshot.lastError === null ? null : "RUNTIME_ERROR",
  };
}
export function disconnectedState(): DesktopState {
  return {
    status: "disconnected",
    source: null,
    revision: null,
    actionToken: null,
    phase: "initializing",
    messages: [],
    canSend: false,
    canCancel: false,
    approval: null,
    models: [],
    error: null,
  };
}
