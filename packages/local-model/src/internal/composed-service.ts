import type { NativeModelStorageCapability } from "../native-storage.ts";
import type { LocalModelService } from "../types.ts";
import { DEFAULT_PRESET } from "./catalog.ts";
import type { LlamaEngineAdapter, LlamaLease } from "./contracts.ts";
import { verifyModelFile } from "./file-verification.ts";
import { createLocalModelManager } from "./manager.ts";
import { DEFAULT_MODEL_IDENTITY } from "./model-identity.ts";
import { createNodeLlamaEngine } from "./node-llama.ts";
import {
  NodeModelIo,
  type ModelDownloaderApi,
  type ModelFileVerifier,
  type ProcessLockAcquirer,
} from "./node-model-io.ts";

interface CompositionDependencies {
  readonly downloaderApi?: ModelDownloaderApi;
  readonly nodeLlamaEngine?: typeof createNodeLlamaEngine;
  readonly verifyFile?: ModelFileVerifier;
  readonly lockProcess?: ProcessLockAcquirer;
}

export const createComposedLocalModelService = (
  capability: NativeModelStorageCapability,
  dependencies: CompositionDependencies = {},
): LocalModelService => {
  const verifier = dependencies.verifyFile ?? verifyModelFile;
  const io = new NodeModelIo(
    capability,
    dependencies.downloaderApi,
    verifier,
    dependencies.lockProcess,
  );
  const engine = createAnchoredLlamaEngine(
    capability,
    dependencies.nodeLlamaEngine ?? createNodeLlamaEngine,
    verifier,
  );
  return createLocalModelManager(io, engine);
};

const createAnchoredLlamaEngine = (
  capability: NativeModelStorageCapability,
  engineFactory: typeof createNodeLlamaEngine,
  verifier: ModelFileVerifier,
): LlamaEngineAdapter => ({
  async loadDefault(signal: AbortSignal): Promise<LlamaLease> {
    let callbackLease: LlamaLease | undefined;
    let callbackTask: Promise<LlamaLease> | undefined;
    const callbackState = { invoked: false, violation: false, closed: false };
    try {
      const result = await capability.withInstalledFile(
        DEFAULT_MODEL_IDENTITY,
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
            await verifier(filePath, DEFAULT_PRESET, signal);
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
  },
});

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
