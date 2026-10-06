import {
  getLlama,
  GgufInsights,
  readGgufFileInfo,
  type GgufFileInfo,
  type Llama,
} from "node-llama-cpp";

import { modelDownloadUri } from "@lyonbrown/local-model/internal/internal/catalog";
import type {
  ModelCompatibilityEvaluation,
  ModelCompatibilityEvaluator,
  ModelPreset,
} from "@lyonbrown/local-model/internal/internal/contracts";

const MAX_CATALOG_SIZE = 8;
const METADATA_TIMEOUT_MS = 5_000;

interface HardwareSnapshot {
  readonly vram: { readonly total: number; readonly free: number; readonly unifiedSize: number };
  readonly ram: { readonly total: number; readonly free: number };
}

interface HardwareLlama {
  getVramState(): Promise<HardwareSnapshot["vram"]>;
  getRamState(): Promise<HardwareSnapshot["ram"]>;
  dispose?(): Promise<void>;
}

interface InsightsAdapter {
  readonly isSupportedByLlamaCpp: boolean;
  resolveAndScoreConfig(
    targetContextTokens: number,
    hardware: HardwareSnapshot,
  ): Promise<{ readonly compatibilityScore: number; readonly bonusScore: number }>;
}

interface NodeLlamaModelEvaluatorApi {
  getLlama(): Promise<HardwareLlama>;
  readGgufFileInfo(uri: string, signal: AbortSignal): Promise<unknown>;
  createInsights(info: unknown, llama: HardwareLlama): Promise<InsightsAdapter>;
}

export const createNodeLlamaModelEvaluator = (
  api: NodeLlamaModelEvaluatorApi = defaultApi,
): ModelCompatibilityEvaluator => ({
  async evaluate(
    presets: readonly ModelPreset[],
    signal: AbortSignal,
  ): Promise<readonly ModelCompatibilityEvaluation[]> {
    if (presets.length > MAX_CATALOG_SIZE) throw new Error("trusted_model_catalog_too_large");
    if (signal.aborted) throw new Error("model_selection_cancelled");
    let llama: HardwareLlama | undefined;
    try {
      const loadedLlama = await api.getLlama();
      llama = loadedLlama;
      const hardware = await readHardwareSnapshot(loadedLlama);
      const outcomes = await Promise.allSettled(
        presets.map((preset) => evaluatePreset(api, loadedLlama, hardware, preset, signal)),
      );
      return outcomes.flatMap((outcome) => (outcome.status === "fulfilled" ? [outcome.value] : []));
    } catch {
      return [];
    } finally {
      await disposeQuietly(llama);
    }
  },
});

const evaluatePreset = async (
  api: NodeLlamaModelEvaluatorApi,
  llama: HardwareLlama,
  hardware: HardwareSnapshot,
  preset: ModelPreset,
  signal: AbortSignal,
): Promise<ModelCompatibilityEvaluation> => {
  const operationSignal = AbortSignal.any([signal, AbortSignal.timeout(METADATA_TIMEOUT_MS)]);
  const info = await api.readGgufFileInfo(modelDownloadUri(preset), operationSignal);
  const insights = await api.createInsights(info, llama);
  if (!insights.isSupportedByLlamaCpp) throw new Error("model_architecture_unsupported");
  const score = await insights.resolveAndScoreConfig(preset.minimumContextTokens, hardware);
  return {
    modelId: preset.id,
    compatibilityScore: score.compatibilityScore,
    bonusScore: score.bonusScore,
  };
};

const readHardwareSnapshot = async (llama: HardwareLlama): Promise<HardwareSnapshot> => {
  const [vram, ram] = await Promise.all([llama.getVramState(), llama.getRamState()]);
  return { vram, ram };
};

const disposeQuietly = async (llama: HardwareLlama | undefined): Promise<void> => {
  try {
    await llama?.dispose?.();
  } catch {
    // Hardware probing failures are reduced to the conservative catalog fallback.
  }
};

const defaultApi: NodeLlamaModelEvaluatorApi = {
  async getLlama(): Promise<HardwareLlama> {
    return getLlama({
      build: "never",
      skipDownload: true,
      usePrebuiltBinaries: true,
      progressLogs: false,
    });
  },
  readGgufFileInfo(uri: string, signal: AbortSignal): Promise<GgufFileInfo> {
    return readGgufFileInfo(uri, {
      sourceType: "network",
      signal,
      logWarnings: false,
      spliceSplitFiles: false,
      fetchRetryOptions: { retries: 0 },
    });
  },
  async createInsights(info: unknown, llama: HardwareLlama): Promise<InsightsAdapter> {
    const insights = await GgufInsights.from(info as GgufFileInfo, llama as Llama);
    return {
      isSupportedByLlamaCpp: insights.isSupportedByLlamaCpp,
      resolveAndScoreConfig: async (targetContextTokens, hardware) => {
        const score = await insights.configurationResolver.resolveAndScoreConfig(
          { targetContextSize: targetContextTokens },
          {
            getVramState: () => Promise.resolve(hardware.vram),
            getRamState: () => Promise.resolve(hardware.ram),
          },
        );
        return {
          compatibilityScore: score.compatibilityScore,
          bonusScore: score.bonusScore,
        };
      },
    };
  },
};
