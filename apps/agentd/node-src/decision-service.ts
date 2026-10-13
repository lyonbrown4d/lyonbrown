import type { RuntimeDecisionPort } from "@lyonbrown/agent-runtime";
import {
  createNativeModelBundleStorageCapability,
  type NativeModelBundleStorageCapability,
} from "@lyonbrown/control-plane-native";
import {
  createAnchoredOpenJevDecisionEngine,
  readDecisionModelStorageConfiguration,
  type AnchoredOpenJevDecisionEngineResult,
  type DecisionModelBundleStoragePort,
} from "@lyonbrown/decision-engine/composition";

import {
  createSystemOneDecisionPort,
  type CreateSystemOneDecisionPortOptions,
} from "@lyonbrown/agentd/internal/decision-adapter";
import type { DecisionModelControlPort } from "@lyonbrown/agentd/internal/decision-model-state";

export interface ProductionDecisionService {
  readonly port: RuntimeDecisionPort;
  readonly setup?: DecisionModelControlPort;
  dispose(): Promise<void>;
}

export interface ProductionDecisionServiceDependencies {
  readonly createStorage: (rootDirectory: string) => NativeModelBundleStorageCapability;
  readonly createAnchoredEngine: (
    storage: DecisionModelBundleStoragePort,
    options: { readonly signal: AbortSignal },
  ) => Promise<AnchoredOpenJevDecisionEngineResult>;
  readonly createPort: (options: CreateSystemOneDecisionPortOptions) => RuntimeDecisionPort;
}

const DEFAULT_DEPENDENCIES: ProductionDecisionServiceDependencies = Object.freeze({
  createStorage: createNativeModelBundleStorageCapability,
  createAnchoredEngine: (
    storage: DecisionModelBundleStoragePort,
    options: { readonly signal: AbortSignal },
  ) => createAnchoredOpenJevDecisionEngine(storage, options),
  createPort: createSystemOneDecisionPort,
});

const INITIALIZATION_DEADLINE_MS = 120_000;
const DISPOSAL_DRAIN_DEADLINE_MS = 5_000;

export function createProductionDecisionService(
  environment: Readonly<NodeJS.ProcessEnv>,
  dependencies: ProductionDecisionServiceDependencies = DEFAULT_DEPENDENCIES,
): ProductionDecisionService {
  const configuration = readDecisionModelStorageConfiguration(environment);
  if (configuration.kind !== "configured") {
    return unavailableService(
      dependencies.createPort({
        unavailableReason:
          configuration.reason === "missing_model_root" ? "model_missing" : "provider_unavailable",
      }),
    );
  }

  const lifecycle = new AbortController();
  let disposed = false;
  let engineTask: Promise<AnchoredOpenJevDecisionEngineResult> | undefined;
  let engineResult: AnchoredOpenJevDecisionEngineResult | undefined;
  let configuredPort: RuntimeDecisionPort | undefined;
  let engineDisposeTask: Promise<void> | undefined;
  let disposeTask: Promise<void> | undefined;
  const startEngine = (): Promise<AnchoredOpenJevDecisionEngineResult> => {
    if (engineTask !== undefined) return engineTask;
    try {
      const signal = AbortSignal.any([
        lifecycle.signal,
        AbortSignal.timeout(INITIALIZATION_DEADLINE_MS),
      ]);
      engineTask = dependencies
        .createAnchoredEngine(dependencies.createStorage(configuration.rootDirectory), { signal })
        .catch(() => unavailableEngine())
        .then((result) => {
          engineResult = result;
          return result;
        });
    } catch {
      engineResult = unavailableEngine();
      engineTask = Promise.resolve(engineResult);
    }
    return engineTask;
  };
  const disposeEngine = (result: AnchoredOpenJevDecisionEngineResult): Promise<void> => {
    if (result.kind !== "configured") return Promise.resolve();
    engineDisposeTask ??= result.engine.dispose();
    return engineDisposeTask;
  };
  return Object.freeze({
    port: {
      async evaluate(request: Parameters<RuntimeDecisionPort["evaluate"]>[0]) {
        if (disposed) return unavailableDecision(dependencies, "provider_unavailable", request);
        const waitSignal = AbortSignal.any([
          request.signal,
          AbortSignal.timeout(INITIALIZATION_DEADLINE_MS),
        ]);
        const result = await settleBeforeAbort(startEngine(), waitSignal);
        if (result === undefined) {
          return unavailableDecision(
            dependencies,
            request.signal.aborted ? "aborted" : "provider_unavailable",
            request,
          );
        }
        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- disposal can race while initialization awaits.
        if (disposed) {
          await disposeEngine(result);
          return unavailableDecision(dependencies, "provider_unavailable", request);
        }
        if (result.kind !== "configured") {
          return unavailableDecision(dependencies, mapUnavailableReason(result.reason), request);
        }
        configuredPort ??= dependencies.createPort({ engine: result.engine });
        return configuredPort.evaluate(request);
      },
    },
    dispose(): Promise<void> {
      if (disposeTask !== undefined) return disposeTask;
      disposed = true;
      if (engineResult === undefined) lifecycle.abort();
      disposeTask = drainEngine(engineTask, disposeEngine);
      return disposeTask;
    },
  });
}

function unavailableService(port: RuntimeDecisionPort): ProductionDecisionService {
  return Object.freeze({ port, dispose: () => Promise.resolve() });
}

function unavailableEngine(): AnchoredOpenJevDecisionEngineResult {
  return { kind: "unavailable", reason: "storage_unavailable" };
}

function unavailableDecision(
  dependencies: ProductionDecisionServiceDependencies,
  reason: "aborted" | "model_corrupted" | "model_missing" | "provider_unavailable",
  request: Parameters<RuntimeDecisionPort["evaluate"]>[0],
) {
  return dependencies.createPort({ unavailableReason: reason }).evaluate(request);
}

async function drainEngine(
  engineTask: Promise<AnchoredOpenJevDecisionEngineResult> | undefined,
  disposeEngine: (result: AnchoredOpenJevDecisionEngineResult) => Promise<void>,
): Promise<void> {
  if (engineTask === undefined) return;
  const result = await settleBeforeAbort(
    engineTask,
    AbortSignal.timeout(DISPOSAL_DRAIN_DEADLINE_MS),
  );
  if (result !== undefined) {
    await disposeEngine(result);
    return;
  }
  void engineTask.then(disposeEngine).catch(() => undefined);
}

function settleBeforeAbort<T>(task: Promise<T>, signal: AbortSignal): Promise<T | undefined> {
  if (signal.aborted) return Promise.resolve(undefined);
  return new Promise<T | undefined>((resolve, reject) => {
    const onAbort = (): void => {
      signal.removeEventListener("abort", onAbort);
      resolve(undefined);
    };
    signal.addEventListener("abort", onAbort, { once: true });
    void task.then(
      (value) => {
        signal.removeEventListener("abort", onAbort);
        resolve(value);
      },
      (error: unknown) => {
        signal.removeEventListener("abort", onAbort);
        reject(
          error instanceof Error
            ? error
            : new Error("Decision initialization failed", { cause: error }),
        );
      },
    );
  });
}

function mapUnavailableReason(
  reason: Exclude<AnchoredOpenJevDecisionEngineResult, { readonly kind: "configured" }>["reason"],
): "model_corrupted" | "model_missing" | "provider_unavailable" {
  if (reason === "model_corrupted" || reason === "model_missing") return reason;
  return "provider_unavailable";
}
