import type { NativeModelStorageCapability } from "../native-storage.js";
import type { LocalModelService } from "../types.js";
import { DEFAULT_PRESET } from "./catalog.js";
import type { LlamaEngineAdapter, LlamaLease } from "./contracts.js";
import { verifyModelFile } from "./file-verification.js";
import { createLocalModelManager } from "./manager.js";
import { DEFAULT_MODEL_IDENTITY } from "./model-identity.js";
import { createNodeLlamaEngine } from "./node-llama.js";
import {
  NodeModelIo,
  type ModelDownloaderApi,
  type ModelFileVerifier,
  type ProcessLockAcquirer,
} from "./node-model-io.js";

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
    let callbackInvoked = false;
    const result = await capability.withInstalledFile(
      DEFAULT_MODEL_IDENTITY,
      signal,
      async (filePath) => {
        callbackInvoked = true;
        await verifier(filePath, DEFAULT_PRESET, signal);
        callbackLease = await engineFactory(filePath).loadDefault(signal);
        return callbackLease;
      },
    );
    if (
      result.kind !== "present" ||
      !callbackInvoked ||
      callbackLease === undefined ||
      result.value !== callbackLease
    ) {
      await disposeQuietly(callbackLease);
      throw new Error("secure_model_anchor_invalid");
    }
    return callbackLease;
  },
});

const disposeQuietly = async (lease: LlamaLease | undefined): Promise<void> => {
  try {
    await lease?.dispose();
  } catch {
    // Do not expose native disposal details across the composition boundary.
  }
};
