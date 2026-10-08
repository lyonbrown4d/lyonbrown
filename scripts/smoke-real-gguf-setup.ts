import {
  hasProgress,
  isActiveDownload,
  isReady,
  isSetup,
  requireManagedModel,
  requireOperationId,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-assertions";
import type {
  RealGgufSmokeFlowOptions,
  RealGgufSmokePort,
  SmokeSnapshot,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-flow";
import {
  assertProgress,
  type ProgressState,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-lib";
import { waitSmokeSnapshot } from "@lyonbrown/workspace/internal/smoke-real-gguf-wait";

export interface PreparedRealGgufModel {
  readonly modelId: string;
  readonly expectedSha256: string;
  readonly ready: SmokeSnapshot;
}

export async function prepareRealGgufModel(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
): Promise<PreparedRealGgufModel> {
  const initial = await waitSmokeSnapshot(
    port,
    options,
    "setup-ready-or-required",
    (snapshot) => isReady(snapshot) || isSetup(snapshot, "required"),
  );
  const model = requireManagedModel(initial);
  const expectedSha256 = options.trustedModelDigests[model.id];
  if (expectedSha256 === undefined || model.sha256 !== expectedSha256) {
    throw new Error("REAL_GGUF_CATALOG_DIGEST_MISMATCH");
  }
  const ready = isReady(initial)
    ? await verifyCachedModel(port, initial, expectedSha256)
    : await downloadModel(port, options, initial, model);
  return { modelId: model.id, expectedSha256, ready };
}

async function verifyCachedModel(
  port: RealGgufSmokePort,
  snapshot: SmokeSnapshot,
  expectedSha256: string,
): Promise<SmokeSnapshot> {
  await port.verifyInstalledCache(expectedSha256);
  return snapshot;
}

async function downloadModel(
  port: RealGgufSmokePort,
  options: RealGgufSmokeFlowOptions,
  initial: SmokeSnapshot,
  model: NonNullable<NonNullable<SmokeSnapshot["modelSetup"]>["managedModel"]>,
): Promise<SmokeSnapshot> {
  const initialSetup = initial.modelSetup;
  if (
    initialSetup?.operationId !== null ||
    initialSetup.downloadedBytes !== 0n ||
    initialSetup.failure !== null ||
    !initialSetup.allowedActions.includes("download")
  ) {
    throw new Error("REAL_GGUF_UNEXPECTED_AUTO_DOWNLOAD");
  }

  await port.startDownload(model.id);
  const firstProgress = await waitSmokeSnapshot(
    port,
    options,
    "first-download-progress",
    (snapshot) => hasProgress(snapshot, options.cancelAfterBytes),
  );
  const firstOperation = requireOperationId(firstProgress);
  await port.cancelDownload(firstOperation);
  await waitSmokeSnapshot(port, options, "download-cancelled", (snapshot) => {
    const setup = snapshot.modelSetup;
    return (
      setup?.phase === "cancelled" &&
      setup.operationId === firstOperation &&
      setup.allowedActions.includes("retry")
    );
  });

  await port.startDownload(model.id);
  let progress: ProgressState | undefined;
  let secondOperation = "";
  const ready = await waitSmokeSnapshot(
    port,
    { ...options, phaseTimeoutMs: options.downloadTimeoutMs },
    "download-ready",
    isReady,
    (snapshot) => {
      const setup = snapshot.modelSetup;
      if (!setup?.operationId || !isActiveDownload(setup)) return;
      if (secondOperation.length === 0) {
        secondOperation = setup.operationId;
        if (secondOperation === firstOperation) throw new Error("REAL_GGUF_OPERATION_NOT_RENEWED");
        progress = {
          operationId: secondOperation,
          downloadedBytes: 0n,
          totalBytes: null,
          maxBytes: model.maxBytes,
        };
      }
      if (setup.operationId !== secondOperation || progress === undefined) return;
      if (setup.phase === "downloading") {
        progress = assertProgress(progress, setup.downloadedBytes, setup.totalBytes);
      }
    },
  );
  if (secondOperation.length === 0 || progress === undefined || progress.downloadedBytes <= 0n) {
    throw new Error("REAL_GGUF_PROGRESS_NOT_OBSERVED");
  }
  return ready;
}
