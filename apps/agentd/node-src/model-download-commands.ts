import type { LocalModelService, LocalModelStatus } from "@lyonbrown/local-model";

import {
  failedRuntimeResult,
  mapCancelDownloadResult,
  mapStartDownloadResult,
} from "./model-download-results.ts";
import type { RuntimeCommand, RuntimeResult } from "./runtime-host.ts";

type ModelCommand = Extract<
  RuntimeCommand,
  { readonly kind: "start-model-download" | "cancel-model-download" }
>;

export function executeModelDownloadCommand(options: {
  readonly command: ModelCommand;
  readonly status: LocalModelStatus;
  readonly service: LocalModelService;
  readonly activationFailed: boolean;
  readonly retryActivation: () => void;
}): RuntimeResult {
  if (options.command.kind === "cancel-model-download") {
    return mapCancelDownloadResult(
      options.service.cancelDownload(options.command.operationId),
      options.command.operationId,
    );
  }
  if (options.command.modelId !== options.status.model.id) {
    return failedRuntimeResult({
      code: "INVALID_MODEL_ID",
      requestedModelId: options.command.modelId,
      configuredModelId: options.status.model.id,
    });
  }
  if (options.activationFailed && options.status.phase === "ready") {
    options.retryActivation();
    return { ok: true };
  }
  return mapStartDownloadResult(options.service.startDownload());
}
