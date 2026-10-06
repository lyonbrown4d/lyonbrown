import { isAbsolute } from "node:path";

import { env } from "@huggingface/transformers";
import { OpenJev } from "open-jev";

import { createOpenJevDecisionEngine } from "@lyonbrown/decision-engine/internal/open-jev-adapter";
import type { OpenJevRuntime } from "@lyonbrown/decision-engine/internal/open-jev-runtime";
import type { DecisionEnginePort } from "@lyonbrown/decision-engine/internal/types";

export interface LocalOpenJevConfiguration {
  /** Absolute path to an already verified local model directory. */
  readonly modelPath: string;
  readonly modelId: string;
  /** Pinned upstream model commit. */
  readonly modelRevision: string;
}

interface OpenJevLoadOptions {
  readonly model: string;
  readonly device: "cpu";
  readonly dtype: "q4";
  readonly truncation: "error";
}

export interface OpenJevCompositionDependencies {
  readonly load: (options: OpenJevLoadOptions) => Promise<OpenJevRuntime>;
  readonly disableRemoteModels: () => void;
}

export type LoadDecisionEngineResult =
  | { readonly kind: "ready"; readonly engine: DecisionEnginePort }
  | {
      readonly kind: "unavailable";
      readonly reason: "invalid_configuration" | "provider_load_failed";
    };

const MODEL_ID = /^[a-z0-9][a-z0-9._-]{0,127}$/u;
const MODEL_REVISION = /^[a-f0-9]{40}$/u;

const productionDependencies: OpenJevCompositionDependencies = {
  load: async (options) => OpenJev.load(options),
  disableRemoteModels(): void {
    env.allowRemoteModels = false;
    env.allowLocalModels = true;
    env.useFSCache = false;
    env.cacheDir = null;
  },
};

function validConfiguration(configuration: LocalOpenJevConfiguration): boolean {
  return (
    isAbsolute(configuration.modelPath) &&
    !configuration.modelPath.includes("\0") &&
    MODEL_ID.test(configuration.modelId) &&
    MODEL_REVISION.test(configuration.modelRevision)
  );
}

export async function loadLocalOpenJevDecisionEngine(
  configuration: LocalOpenJevConfiguration,
  dependencies: OpenJevCompositionDependencies = productionDependencies,
): Promise<LoadDecisionEngineResult> {
  if (!validConfiguration(configuration)) {
    return { kind: "unavailable", reason: "invalid_configuration" };
  }
  try {
    dependencies.disableRemoteModels();
    const runtime = await dependencies.load({
      model: configuration.modelPath,
      device: "cpu",
      dtype: "q4",
      truncation: "error",
    });
    return {
      kind: "ready",
      engine: createOpenJevDecisionEngine({
        runtime,
        modelId: configuration.modelId,
        modelRevision: configuration.modelRevision,
      }),
    };
  } catch {
    return { kind: "unavailable", reason: "provider_load_failed" };
  }
}
