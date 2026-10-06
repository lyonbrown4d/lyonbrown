import { resolve } from "node:path";

import { describe, expect, it, vi } from "vitest";

import { loadLocalOpenJevDecisionEngine } from "@lyonbrown/decision-engine/internal/composition";

const modelRevision = "0123456789abcdef0123456789abcdef01234567";

describe("OpenJev production composition", () => {
  it("loads an exact local model path with remote model access disabled", async () => {
    const runtime = { decide: vi.fn(), dispose: vi.fn().mockResolvedValue(undefined) };
    const load = vi.fn().mockResolvedValue(runtime);
    const disableRemoteModels = vi.fn();
    const modelPath = resolve("fixtures/decision-model");

    const result = await loadLocalOpenJevDecisionEngine(
      { modelPath, modelId: "kev-0.6b", modelRevision },
      { load, disableRemoteModels },
    );

    expect(result.kind).toBe("ready");
    expect(disableRemoteModels).toHaveBeenCalledOnce();
    expect(load).toHaveBeenCalledWith({
      model: modelPath,
      device: "cpu",
      dtype: "q4",
      truncation: "error",
    });
  });

  it.each(["kev-0.6b", "https://huggingface.co/model", "../model"])(
    "rejects non-absolute model location %s without loading the provider",
    async (modelPath) => {
      const load = vi.fn();
      const result = await loadLocalOpenJevDecisionEngine(
        { modelPath, modelId: "kev-0.6b", modelRevision },
        { load, disableRemoteModels: vi.fn() },
      );

      expect(result).toEqual({ kind: "unavailable", reason: "invalid_configuration" });
      expect(load).not.toHaveBeenCalled();
    },
  );

  it("sanitizes load failures", async () => {
    const result = await loadLocalOpenJevDecisionEngine(
      {
        modelPath: resolve("fixtures/decision-model"),
        modelId: "kev-0.6b",
        modelRevision,
      },
      {
        load: vi.fn().mockRejectedValue(new Error("C:\\private\\model.onnx")),
        disableRemoteModels: vi.fn(),
      },
    );

    expect(result).toEqual({ kind: "unavailable", reason: "provider_load_failed" });
  });
});
