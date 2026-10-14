import type { AgentCommand, RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import type { DesktopAction } from "@lyonbrown/agent-desktop/internal/shared/contract";
import { projectSnapshot } from "@lyonbrown/agent-desktop/internal/main/projection";
export function commandFor(action: DesktopAction, snapshot: RuntimeSnapshotDto): AgentCommand {
  const view = projectSnapshot(snapshot, "reused");
  switch (action.type) {
    case "send":
      if (!view.canSend) throw new Error("CHAT_UNAVAILABLE");
      return { type: "start-turn", text: action.text };
    case "cancel":
      if (!view.canCancel) throw new Error("CANCEL_UNAVAILABLE");
      return { type: "cancel-turn" };
    case "approve":
    case "deny":
      if (snapshot.pendingApproval?.state !== "pending") throw new Error("APPROVAL_UNAVAILABLE");
      return {
        type: "respond-approval",
        requestId: snapshot.pendingApproval.requestId,
        decision: action.type === "approve" ? "allow-once" : "deny-once",
      };
    case "model": {
      const model = action.purpose === "chat" ? snapshot.modelSetup : snapshot.decisionModel;
      const id =
        action.purpose === "chat"
          ? snapshot.modelSetup?.managedModel?.id
          : snapshot.decisionModel?.modelId;
      if (!model?.allowedActions.includes(action.action))
        throw new Error("MODEL_ACTION_UNAVAILABLE");
      if (action.action === "cancel") {
        if (!model.operationId) throw new Error("MODEL_ACTION_UNAVAILABLE");
        return {
          type: "cancel-model-download",
          operationId: model.operationId,
          purpose: action.purpose,
        };
      }
      if (!id) throw new Error("MODEL_ACTION_UNAVAILABLE");
      return { type: "start-model-download", modelId: id, purpose: action.purpose };
    }
  }
}
