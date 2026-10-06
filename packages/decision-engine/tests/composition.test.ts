import { describe, expect, it, vi } from "vitest";

import {
  createLocalOpenJevDecisionEngine,
  type OpenJevWorkerPoolOptions,
} from "@lyonbrown/decision-engine/internal/composition";
import type { DecisionWorkerPool } from "@lyonbrown/decision-engine/internal/worker-engine";
import { injectedOpenJevConfiguration } from "@lyonbrown/decision-engine/test/model-environment";

function workerPool(): DecisionWorkerPool {
  return {
    run: vi.fn().mockResolvedValue([]),
    close: vi.fn().mockResolvedValue(undefined),
  };
}

describe("OpenJev production composition", () => {
  it("creates one bounded Piscina worker for an exact local model path", async () => {
    const pool = workerPool();
    const createPool = vi.fn((_options: OpenJevWorkerPoolOptions) => pool);
    const configuration = injectedOpenJevConfiguration();

    const result = createLocalOpenJevDecisionEngine(configuration, { createPool });

    expect(result.kind).toBe("configured");
    expect(createPool).toHaveBeenCalledOnce();
    const poolOptions = createPool.mock.calls[0]?.[0];
    expect(poolOptions).toBeDefined();
    if (poolOptions === undefined) throw new Error("WORKER_POOL_OPTIONS_MISSING");
    expect(poolOptions.filename).toMatch(/open-jev-worker\.js$/u);
    expect(poolOptions).toEqual({
      filename: poolOptions.filename,
      minThreads: 0,
      maxThreads: 1,
      maxQueue: 1,
      concurrentTasksPerWorker: 1,
      idleTimeout: 60_000,
      closeTimeout: 5_000,
      resourceLimits: {
        maxOldGenerationSizeMb: 1_024,
        maxYoungGenerationSizeMb: 128,
        stackSizeMb: 8,
      },
    });
  });

  it.each(["kev-0.6b", "https://huggingface.co/model", "../model"])(
    "rejects non-absolute model location %s without creating a worker",
    async (modelPath) => {
      const createPool = vi.fn();
      const result = createLocalOpenJevDecisionEngine(
        {
          ...injectedOpenJevConfiguration(),
          modelPath,
        },
        { createPool },
      );

      expect(result).toEqual({ kind: "unavailable", reason: "invalid_configuration" });
      expect(createPool).not.toHaveBeenCalled();
    },
  );

  it("sanitizes worker-pool creation failures", async () => {
    const result = createLocalOpenJevDecisionEngine(injectedOpenJevConfiguration(), {
      createPool: () => {
        throw new Error("private worker failure");
      },
    });

    expect(result).toEqual({ kind: "unavailable", reason: "worker_pool_creation_failed" });
  });
});
