import type { AgentCommand, ModelSelectionDto, RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import type { Locale } from "@lyonbrown/ui-model";

export function isActiveTurn(phase: RuntimeSnapshotDto["phase"] | undefined): boolean {
  return phase === "preparing" || phase === "streaming" || phase === "running-tool";
}

export function publicMessage(error: unknown, locale: Locale): string {
  const message = error instanceof Error ? error.message : "request failed";
  if (locale !== "zh-CN") return message;
  if (message.startsWith("unknown command:")) {
    return message.replace("unknown command:", "未知命令：");
  }
  if (message.includes("is not available yet")) return "该功能尚不可用";
  if (message.includes("requires text")) return "该命令需要消息内容";
  if (message.includes("daemon state is stale")) return "Daemon 状态已过期，请等待重连";
  if (message.includes("daemon is not connected")) return "Daemon 尚未连接";
  if (message.includes("chat is unavailable")) return "Daemon 和模型就绪前无法开始对话";
  return message;
}

export function assertSelectableModel(
  snapshot: RuntimeSnapshotDto | null,
  selection: ModelSelectionDto,
): void {
  if (selection.kind === "auto") return;
  const selectable = snapshot?.modelCatalog?.models.some(
    (model) =>
      model.selectableForWorker &&
      model.route.provider === selection.provider &&
      model.route.model === selection.model,
  );
  if (selectable !== true) {
    throw new Error(`model ${selection.provider}/${selection.model} is not selectable`);
  }
}

export function cancelCommand(snapshot: RuntimeSnapshotDto | null): AgentCommand {
  const setup = snapshot?.modelSetup;
  if (
    setup?.operationId !== null &&
    setup?.operationId !== undefined &&
    setup.allowedActions.includes("cancel")
  ) {
    return { type: "cancel-model-download", operationId: setup.operationId };
  }
  return { type: "cancel-turn" };
}

export function modelDownloadCommand(
  snapshot: RuntimeSnapshotDto | null,
  action: "download" | "retry",
): AgentCommand {
  const setup = snapshot?.modelSetup;
  const modelId = setup?.managedModel?.id;
  if (modelId === undefined || setup?.allowedActions.includes(action) !== true) {
    throw new Error(`model ${action} is not available`);
  }
  return { type: "start-model-download", modelId };
}
