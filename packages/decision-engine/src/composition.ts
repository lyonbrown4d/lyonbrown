import { Piscina } from "piscina";

import {
  isValidLocalOpenJevConfiguration,
  type LocalOpenJevConfiguration,
} from "@lyonbrown/decision-engine/internal/open-jev-configuration";
import {
  createWorkerOpenJevDecisionEngine,
  type DecisionWorkerPool,
  type OpenJevWorkerTask,
} from "@lyonbrown/decision-engine/internal/worker-engine";
import type { DecisionEnginePort } from "@lyonbrown/decision-engine/internal/types";

export type { LocalOpenJevConfiguration } from "@lyonbrown/decision-engine/internal/open-jev-configuration";

export interface OpenJevWorkerPoolOptions {
  readonly filename: string;
  readonly minThreads: number;
  readonly maxThreads: number;
  readonly maxQueue: number;
  readonly concurrentTasksPerWorker: number;
  readonly idleTimeout: number;
  readonly closeTimeout: number;
  readonly resourceLimits: {
    readonly maxOldGenerationSizeMb: number;
    readonly maxYoungGenerationSizeMb: number;
    readonly stackSizeMb: number;
  };
}

export interface OpenJevCompositionDependencies {
  readonly createPool: (options: OpenJevWorkerPoolOptions) => DecisionWorkerPool;
}

export type CreateDecisionEngineResult =
  | { readonly kind: "configured"; readonly engine: DecisionEnginePort }
  | {
      readonly kind: "unavailable";
      readonly reason: "invalid_configuration" | "worker_pool_creation_failed";
    };

const WORKER_OPTIONS: OpenJevWorkerPoolOptions = Object.freeze({
  filename: new URL("./open-jev-worker.js", import.meta.url).href,
  minThreads: 0,
  maxThreads: 1,
  maxQueue: 1,
  concurrentTasksPerWorker: 1,
  idleTimeout: 60_000,
  closeTimeout: 5_000,
  resourceLimits: Object.freeze({
    maxOldGenerationSizeMb: 1_024,
    maxYoungGenerationSizeMb: 128,
    stackSizeMb: 8,
  }),
});

const productionDependencies: OpenJevCompositionDependencies = Object.freeze({
  createPool(options: OpenJevWorkerPoolOptions): DecisionWorkerPool {
    const pool = new Piscina<OpenJevWorkerTask, unknown>(options);
    return {
      run: (task, runOptions) => pool.run(task, runOptions),
      close: () => pool.close({ force: true }),
    };
  },
});

export function createLocalOpenJevDecisionEngine(
  configuration: LocalOpenJevConfiguration,
  dependencies: OpenJevCompositionDependencies = productionDependencies,
): CreateDecisionEngineResult {
  if (!isValidLocalOpenJevConfiguration(configuration)) {
    return { kind: "unavailable", reason: "invalid_configuration" };
  }
  try {
    const pool = dependencies.createPool(WORKER_OPTIONS);
    return {
      kind: "configured",
      engine: createWorkerOpenJevDecisionEngine({
        pool,
        configuration,
        closeTimeoutMs: WORKER_OPTIONS.closeTimeout,
      }),
    };
  } catch {
    return { kind: "unavailable", reason: "worker_pool_creation_failed" };
  }
}
