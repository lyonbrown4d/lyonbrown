import { getLlama } from "node-llama-cpp";

import type { LlamaEngineAdapter, LlamaLease } from "./contracts.js";

interface LoadedModel {
  dispose(): Promise<void>;
}

interface LoadedLlama {
  loadModel(options: { modelPath: string; loadSignal: AbortSignal }): Promise<LoadedModel>;
  dispose?(): Promise<void>;
}

interface NodeLlamaApi {
  getLlama(options: {
    build: "never";
    skipDownload: true;
    usePrebuiltBinaries: true;
    progressLogs: false;
  }): Promise<LoadedLlama>;
}

export const createNodeLlamaEngine = (
  modelPath: string,
  api: NodeLlamaApi = { getLlama },
): LlamaEngineAdapter => ({
  async loadDefault(signal: AbortSignal): Promise<LlamaLease> {
    const llama = await api.getLlama({
      build: "never",
      skipDownload: true,
      usePrebuiltBinaries: true,
      progressLogs: false,
    });
    try {
      const model = await llama.loadModel({ modelPath, loadSignal: signal });
      let disposed = false;
      return {
        async dispose(): Promise<void> {
          if (disposed) return;
          disposed = true;
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
          if (failed) throw new Error("native_runtime_dispose_failed");
        },
      };
    } catch (error) {
      await llama.dispose?.();
      throw error;
    }
  },
});
