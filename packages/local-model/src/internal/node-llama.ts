import { getLlama } from "node-llama-cpp";

import type {
  LocalModelInferenceEvent,
  LocalModelInferenceRequest,
} from "@lyonbrown/local-model/internal/types";
import type {
  LlamaEngineAdapter,
  LlamaLease,
} from "@lyonbrown/local-model/internal/internal/contracts";
import {
  createDefaultNativeChat,
  createNodeLlamaInferenceStream,
  DISPOSE_ABORT,
  type ActiveInferenceRun,
  type NativeChat,
  type NativeModel,
  type NativeSequence,
} from "@lyonbrown/local-model/internal/internal/node-llama-inference";

const SHUTDOWN_TIMEOUT_MS = 5_000;

interface LoadedLlama {
  loadModel(options: { modelPath: string; loadSignal: AbortSignal }): Promise<NativeModel>;
  dispose?(): Promise<void>;
}

interface NodeLlamaApi {
  getLlama(options: {
    build: "never";
    skipDownload: true;
    usePrebuiltBinaries: true;
    progressLogs: false;
  }): Promise<LoadedLlama>;
  readonly createChat?: (sequence: NativeSequence) => NativeChat;
}

const awaitFinalizationOrTimeout = async (finalization: Promise<boolean>): Promise<void> => {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<{ readonly kind: "timeout" }>((resolve) => {
    timer = setTimeout(() => {
      resolve({ kind: "timeout" });
    }, SHUTDOWN_TIMEOUT_MS);
    timer.unref();
  });
  const outcome = await Promise.race([
    finalization.then((failed) => ({ kind: "finalized" as const, failed })),
    timeout,
  ]);
  if (timer !== undefined) clearTimeout(timer);
  if (outcome.kind === "timeout") throw new Error("native_runtime_shutdown_timeout");
  if (outcome.failed) throw new Error("native_runtime_dispose_failed");
};

export const createNodeLlamaEngine = (
  modelPath: string,
  api: NodeLlamaApi = { getLlama },
): LlamaEngineAdapter => ({
  async loadDefault(signal: AbortSignal): Promise<LlamaLease> {
    let llama: LoadedLlama;
    try {
      llama = await api.getLlama({
        build: "never",
        skipDownload: true,
        usePrebuiltBinaries: true,
        progressLogs: false,
      });
    } catch {
      throw new Error("native_runtime_load_failed");
    }
    try {
      const model = await llama.loadModel({ modelPath, loadSignal: signal });
      const activeRuns = new Set<ActiveInferenceRun>();
      const createChat = api.createChat ?? createDefaultNativeChat;
      let disposed = false;
      let disposePromise: Promise<void> | undefined;
      return {
        stream(request: LocalModelInferenceRequest): AsyncIterable<LocalModelInferenceEvent> {
          return createNodeLlamaInferenceStream(
            model,
            request,
            activeRuns,
            () => disposed,
            createChat,
          );
        },
        dispose(): Promise<void> {
          disposePromise ??= (async () => {
            disposed = true;
            const runs = [...activeRuns];
            for (const run of runs) run.controller.abort(DISPOSE_ABORT);
            const finalization = (async (): Promise<boolean> => {
              await Promise.allSettled(runs.map((run) => run.completion));
              let failed = false;
              try {
                await model.dispose();
              } catch {
                failed = true;
              }
              try {
                await llama.dispose?.();
              } catch {
                failed = true;
              }
              return failed;
            })();
            await awaitFinalizationOrTimeout(finalization);
          })();
          return disposePromise;
        },
      };
    } catch {
      try {
        await llama.dispose?.();
      } catch {
        // The fixed public error below intentionally hides native disposal details.
      }
      throw new Error("native_runtime_load_failed");
    }
  },
});
