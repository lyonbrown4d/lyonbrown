import type {
  ModelCompatibilityEvaluation,
  ModelCompatibilityEvaluator,
  ModelPreset,
  ModelSelection,
  ModelSelector,
} from "./contracts.ts";

export const createHardwareAwareModelSelector = (
  evaluator: ModelCompatibilityEvaluator,
): ModelSelector => ({
  async select(presets: readonly ModelPreset[], signal: AbortSignal): Promise<ModelSelection> {
    if (presets.length === 0) throw new Error("trusted_model_catalog_empty");
    const fallback = [...presets].sort(compareFallback)[0];
    if (fallback === undefined) throw new Error("trusted_model_catalog_empty");
    try {
      const evaluations = await evaluator.evaluate(presets, signal);
      if (signal.aborted) throw signal.reason;
      const selected = selectEvaluation(presets, evaluations);
      if (selected !== undefined) return { preset: selected, source: "hardware" };
    } catch {
      if (signal.aborted) throw signal.reason;
    }
    return { preset: fallback, source: "fallback" };
  },
});

const selectEvaluation = (
  presets: readonly ModelPreset[],
  evaluations: readonly ModelCompatibilityEvaluation[],
): ModelPreset | undefined => {
  const trustedById = new Map(presets.map((preset) => [preset.id, preset]));
  const valid = evaluations
    .filter(isValidEvaluation)
    .map((evaluation) => ({ evaluation, preset: trustedById.get(evaluation.modelId) }))
    .filter(
      (entry): entry is { evaluation: ModelCompatibilityEvaluation; preset: ModelPreset } =>
        entry.preset !== undefined,
    )
    .sort(compareEvaluations);
  return valid[0]?.preset;
};

const isValidEvaluation = (evaluation: ModelCompatibilityEvaluation): boolean =>
  Number.isFinite(evaluation.compatibilityScore) &&
  evaluation.compatibilityScore > 0 &&
  evaluation.compatibilityScore <= 1 &&
  Number.isFinite(evaluation.bonusScore) &&
  evaluation.bonusScore >= 0;

const compareEvaluations = (
  left: { readonly evaluation: ModelCompatibilityEvaluation; readonly preset: ModelPreset },
  right: { readonly evaluation: ModelCompatibilityEvaluation; readonly preset: ModelPreset },
): number => {
  const leftScore = left.evaluation.compatibilityScore + left.evaluation.bonusScore;
  const rightScore = right.evaluation.compatibilityScore + right.evaluation.bonusScore;
  return (
    rightScore - leftScore ||
    right.preset.qualityRank - left.preset.qualityRank ||
    left.preset.expectedDownloadBytes - right.preset.expectedDownloadBytes ||
    left.preset.id.localeCompare(right.preset.id)
  );
};

const compareFallback = (left: ModelPreset, right: ModelPreset): number =>
  left.minimumMemoryBytes - right.minimumMemoryBytes ||
  left.expectedDownloadBytes - right.expectedDownloadBytes ||
  left.id.localeCompare(right.id);
