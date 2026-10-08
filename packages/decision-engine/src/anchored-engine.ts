import { BUNDLED_DECISION_MODEL } from "@lyonbrown/decision-engine/internal/bundled-model-catalog";
import {
  createLocalOpenJevDecisionEngine,
  type CreateDecisionEngineResult,
  type LocalOpenJevConfiguration,
} from "@lyonbrown/decision-engine/internal/composition";
import { verifyDecisionModelBundle } from "@lyonbrown/decision-engine/internal/bundled-model-verification";
import type {
  DecisionModelBundleIdentity,
  DecisionModelBundleStoragePort,
} from "@lyonbrown/decision-engine/internal/model-bundle-storage";
import type {
  DecisionEnginePort,
  DecisionOutcome,
  DecisionRequest,
} from "@lyonbrown/decision-engine/internal/types";

export interface AnchoredEngineDependencies {
  readonly createEngine: (configuration: LocalOpenJevConfiguration) => CreateDecisionEngineResult;
}

export interface CreateAnchoredOpenJevDecisionEngineOptions {
  readonly signal?: AbortSignal;
}

export interface CreateAnchoredDecisionEngineOptions {
  readonly dependencies?: AnchoredEngineDependencies;
  readonly signal?: AbortSignal;
}

export type AnchoredOpenJevDecisionEngineResult =
  | { readonly kind: "configured"; readonly engine: DecisionEnginePort }
  | {
      readonly kind: "unavailable";
      readonly reason:
        | "model_missing"
        | "model_corrupted"
        | "storage_unavailable"
        | "worker_configuration_failed";
    };

const DEFAULT_DEPENDENCIES: AnchoredEngineDependencies = Object.freeze({
  createEngine: createLocalOpenJevDecisionEngine,
});

export function createAnchoredOpenJevDecisionEngine(
  storage: DecisionModelBundleStoragePort,
  options: CreateAnchoredOpenJevDecisionEngineOptions = {},
): Promise<AnchoredOpenJevDecisionEngineResult> {
  return createAnchoredDecisionEngine(storage, BUNDLED_DECISION_MODEL, options);
}

export function createAnchoredDecisionEngine(
  storage: DecisionModelBundleStoragePort,
  model: DecisionModelBundleIdentity,
  options: CreateAnchoredDecisionEngineOptions = {},
): Promise<AnchoredOpenJevDecisionEngineResult> {
  const signal = options.signal ?? new AbortController().signal;
  const dependencies = options.dependencies ?? DEFAULT_DEPENDENCIES;
  let settleReady: (result: AnchoredOpenJevDecisionEngineResult) => void = () => undefined;
  const ready = new Promise<AnchoredOpenJevDecisionEngineResult>((resolve) => {
    settleReady = resolve;
  });
  let readySettled = false;
  const settleOnce = (result: AnchoredOpenJevDecisionEngineResult): void => {
    if (readySettled) return;
    readySettled = true;
    settleReady(result);
  };
  let releaseLifetime: () => void = () => undefined;
  const released = new Promise<void>((resolve) => {
    releaseLifetime = resolve;
  });
  let engineConfigured = false;
  const lifetime = storage
    .withInstalledBundle(model, signal, async (modelPath) => {
      try {
        await verifyDecisionModelBundle(modelPath, model, signal);
      } catch {
        settleOnce({ kind: "unavailable", reason: "model_corrupted" });
        return;
      }
      const created = dependencies.createEngine({
        modelPath: providerModelPath(modelPath),
        modelId: model.modelId,
        modelRevision: model.revision,
      });
      if (created.kind !== "configured") {
        settleOnce({ kind: "unavailable", reason: "worker_configuration_failed" });
        return;
      }
      engineConfigured = true;
      settleOnce({
        kind: "configured",
        engine: anchoredEngine(created.engine, releaseLifetime, () => lifetime),
      });
      await released;
      await created.engine.dispose();
    })
    .then((result) => {
      if (result.kind === "missing") {
        settleOnce({ kind: "unavailable", reason: "model_missing" });
      }
    })
    .catch((error: unknown) => {
      if (engineConfigured) throw error;
      settleOnce({ kind: "unavailable", reason: "storage_unavailable" });
    });

  return ready;
}

function providerModelPath(nativePath: string): string {
  if (process.platform !== "win32" || !nativePath.startsWith("\\\\?\\")) return nativePath;
  const drivePath = nativePath.slice(4);
  return /^[a-z]:\\/iu.test(drivePath) ? drivePath : nativePath;
}

function anchoredEngine(
  engine: DecisionEnginePort,
  releaseLifetime: () => void,
  lifetime: () => Promise<void>,
): DecisionEnginePort {
  let disposed = false;
  let disposePromise: Promise<void> | undefined;
  return {
    decide(request: DecisionRequest): Promise<DecisionOutcome> {
      return disposed
        ? Promise.resolve({ kind: "undetermined", reason: "disposed" })
        : engine.decide(request);
    },
    dispose(): Promise<void> {
      if (disposePromise !== undefined) return disposePromise;
      disposed = true;
      releaseLifetime();
      disposePromise = lifetime();
      return disposePromise;
    },
  };
}
