import type { NativeModelStorageCapability } from "@lyonbrown/local-model/internal/native-storage";
import type { LocalModelService } from "@lyonbrown/local-model/internal/types";
import {
  DEFAULT_AUTO_DOWNLOAD_MAX_BYTES,
  DEFAULT_PRESET,
  MODEL_CATALOG,
} from "@lyonbrown/local-model/internal/internal/catalog";
import type {
  LlamaEngineAdapter,
  LlamaLease,
  ModelPreset,
  ModelSelector,
} from "@lyonbrown/local-model/internal/internal/contracts";
import { verifyModelFile } from "@lyonbrown/local-model/internal/internal/file-verification";
import { createLocalModelManager } from "@lyonbrown/local-model/internal/internal/manager";
import { createHardwareAwareModelSelector } from "@lyonbrown/local-model/internal/internal/model-selection";
import { modelIdentityFor } from "@lyonbrown/local-model/internal/internal/model-identity";
import { createNodeLlamaEngine } from "@lyonbrown/local-model/internal/internal/node-llama";
import { createNodeLlamaModelEvaluator } from "@lyonbrown/local-model/internal/internal/node-llama-model-evaluator";
import {
  NodeModelIo,
  type ModelFileDownloader,
  type ModelFileVerifier,
  type ProcessLockAcquirer,
} from "@lyonbrown/local-model/internal/internal/node-model-io";

interface CompositionDependencies {
  readonly modelFileDownloader?: ModelFileDownloader;
  readonly nodeLlamaEngine?: typeof createNodeLlamaEngine;
  readonly verifyFile?: ModelFileVerifier;
  readonly lockProcess?: ProcessLockAcquirer;
  readonly modelCatalog?: readonly ModelPreset[];
  readonly modelSelector?: ModelSelector;
}

export const createComposedLocalModelService = (
  capability: NativeModelStorageCapability,
  dependencies: CompositionDependencies = {},
): LocalModelService => {
  const verifier = dependencies.verifyFile ?? verifyModelFile;
  const io = new NodeModelIo(
    capability,
    dependencies.modelFileDownloader,
    verifier,
    dependencies.lockProcess,
  );
  const engine = createAnchoredLlamaEngine(
    capability,
    dependencies.nodeLlamaEngine ?? createNodeLlamaEngine,
    verifier,
  );
  const selection = {
    catalog: dependencies.modelCatalog ?? MODEL_CATALOG,
    maxAutoDownloadBytes: DEFAULT_AUTO_DOWNLOAD_MAX_BYTES,
    selector:
      dependencies.modelSelector ??
      createHardwareAwareModelSelector(createNodeLlamaModelEvaluator()),
  };
  return createLocalModelManager(io, engine, undefined, undefined, selection);
};

const createAnchoredLlamaEngine = (
  capability: NativeModelStorageCapability,
  engineFactory: typeof createNodeLlamaEngine,
  verifier: ModelFileVerifier,
): LlamaEngineAdapter => {
  const load = async (preset: ModelPreset, signal: AbortSignal): Promise<LlamaLease> => {
    let callbackLease: LlamaLease | undefined;
    let callbackTask: Promise<LlamaLease> | undefined;
    const callbackState = { invoked: false, violation: false, closed: false };
    try {
      const result = await capability.withInstalledFile(
        modelIdentityFor(preset),
        signal,
        (filePath) => {
          if (callbackState.closed || callbackState.invoked) {
            callbackState.violation = true;
            const rejected = Promise.reject<LlamaLease>(new Error("secure_model_anchor_invalid"));
            void rejected.catch(() => undefined);
            return rejected;
          }
          callbackState.invoked = true;
          callbackTask = (async () => {
            await verifier(filePath, preset, signal);
            callbackLease = await engineFactory(filePath).loadDefault(signal);
            return callbackLease;
          })();
          void callbackTask.catch(() => undefined);
          return callbackTask;
        },
      );
      callbackState.closed = true;
      if (
        result.kind !== "present" ||
        callbackState.violation ||
        callbackLease === undefined ||
        result.value !== callbackLease
      ) {
        throw new Error("secure_model_anchor_invalid");
      }
      return callbackLease;
    } catch {
      callbackState.closed = true;
      await settleQuietly(callbackTask);
      await disposeQuietly(callbackLease);
      throw new Error("secure_model_anchor_invalid");
    }
  };
  return {
    load,
    loadDefault: (signal) => load(DEFAULT_PRESET, signal),
  };
};

const settleQuietly = async (task: Promise<unknown> | undefined): Promise<void> => {
  try {
    await task;
  } catch {
    // The stable public error below replaces private verifier or loader details.
  }
};

const disposeQuietly = async (lease: LlamaLease | undefined): Promise<void> => {
  try {
    await lease?.dispose();
  } catch {
    // Do not expose native disposal details across the composition boundary.
  }
};
