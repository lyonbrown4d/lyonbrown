import { describe, expect, it } from "vitest";

import type { ModelCompatibilityEvaluator, ModelPreset } from "../src/internal/contracts.ts";
import { createHardwareAwareModelSelector } from "../src/internal/model-selection.ts";

const GIB = 1024 ** 3;

const smallPreset = preset({
  id: "fixture-small",
  expectedDownloadBytes: 700_000_000,
  minimumMemoryBytes: 2 * GIB,
  qualityRank: 1,
});
const capablePreset = preset({
  id: "fixture-capable",
  expectedDownloadBytes: 1_500_000_000,
  minimumMemoryBytes: 4 * GIB,
  qualityRank: 2,
});

describe("hardware-aware model selection", () => {
  it("selects the highest scoring compatible model without downloading it", async () => {
    const evaluator: ModelCompatibilityEvaluator = {
      evaluate: () =>
        Promise.resolve([
          { modelId: smallPreset.id, compatibilityScore: 1, bonusScore: 0 },
          { modelId: capablePreset.id, compatibilityScore: 0.9, bonusScore: 0.4 },
        ]),
    };
    const selector = createHardwareAwareModelSelector(evaluator);

    await expect(
      selector.select([smallPreset, capablePreset], new AbortController().signal),
    ).resolves.toEqual({ preset: capablePreset, source: "hardware" });
  });

  it("falls back to the smallest trusted preset when hardware evaluation fails", async () => {
    const evaluator: ModelCompatibilityEvaluator = {
      evaluate: () => Promise.reject(new Error("private hardware details")),
    };
    const selector = createHardwareAwareModelSelector(evaluator);

    await expect(
      selector.select([capablePreset, smallPreset], new AbortController().signal),
    ).resolves.toEqual({ preset: smallPreset, source: "fallback" });
  });

  it("rejects an empty catalog instead of accepting an untrusted runtime URI", async () => {
    const evaluator: ModelCompatibilityEvaluator = { evaluate: () => Promise.resolve([]) };
    const selector = createHardwareAwareModelSelector(evaluator);

    await expect(selector.select([], new AbortController().signal)).rejects.toThrow(
      "trusted_model_catalog_empty",
    );
  });

  it("ignores evaluator results for model ids outside the trusted catalog", async () => {
    const evaluator: ModelCompatibilityEvaluator = {
      evaluate: () =>
        Promise.resolve([
          { modelId: "attacker-controlled", compatibilityScore: 1, bonusScore: 10 },
        ]),
    };
    const selector = createHardwareAwareModelSelector(evaluator);

    await expect(selector.select([smallPreset], new AbortController().signal)).resolves.toEqual({
      preset: smallPreset,
      source: "fallback",
    });
  });

  it("never recommends an incompatible zero-score model even with a large bonus", async () => {
    const evaluator: ModelCompatibilityEvaluator = {
      evaluate: () =>
        Promise.resolve([
          { modelId: smallPreset.id, compatibilityScore: 0.6, bonusScore: 0 },
          { modelId: capablePreset.id, compatibilityScore: 0, bonusScore: 100 },
        ]),
    };
    const selector = createHardwareAwareModelSelector(evaluator);

    await expect(
      selector.select([smallPreset, capablePreset], new AbortController().signal),
    ).resolves.toEqual({ preset: smallPreset, source: "hardware" });
  });
});

function preset(
  overrides: Pick<
    ModelPreset,
    "id" | "expectedDownloadBytes" | "minimumMemoryBytes" | "qualityRank"
  >,
): ModelPreset {
  return {
    ...overrides,
    displayName: overrides.id,
    owner: "Fixture",
    repository: "Fixture-GGUF",
    revision: "1111111111111111111111111111111111111111",
    filename: `${overrides.id}.gguf`,
    sha256: "a".repeat(64),
    maxBytes: 2 * GIB,
    minimumContextTokens: 4_096,
    license: "apache-2.0",
  };
}
