import { pathToFileURL } from "node:url";

import { createNativeModelBundleStorageCapability } from "@lyonbrown/control-plane-native";
import {
  installBundledDecisionModel,
  readDecisionModelStorageConfiguration,
  type DecisionModelBundleStoragePort,
  type InstallBundledDecisionModelOptions,
  type BundledDecisionModelInstallResult,
} from "@lyonbrown/decision-engine/composition";

import type { DecisionInstallProgress } from "@lyonbrown/agentd/internal/decision-install-process";
import { agentdProxyEnvironment } from "@lyonbrown/agentd/internal/proxy-environment";

export interface DecisionInstallWorkerDependencies {
  readonly createStorage: (root: string) => DecisionModelBundleStoragePort;
  readonly install: (
    storage: DecisionModelBundleStoragePort,
    options: InstallBundledDecisionModelOptions,
  ) => Promise<BundledDecisionModelInstallResult>;
  readonly acquireProxy: () => () => void;
}

const DEFAULT_DEPENDENCIES: DecisionInstallWorkerDependencies = {
  createStorage: createNativeModelBundleStorageCapability,
  install: installBundledDecisionModel,
  acquireProxy: () => agentdProxyEnvironment.acquire(),
};

export async function executeDecisionInstall(
  root: string | undefined,
  operationId: string | undefined,
  signal: AbortSignal,
  report: (value: DecisionInstallProgress) => void,
  complete: () => Promise<void>,
  dependencies: DecisionInstallWorkerDependencies = DEFAULT_DEPENDENCIES,
): Promise<void> {
  const configuration = readDecisionModelStorageConfiguration({
    LYONBROWN_DECISION_MODEL_ROOT: root,
  });
  if (
    configuration.kind !== "configured" ||
    operationId === undefined ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u.test(operationId)
  )
    throw new Error("invalid_install_configuration");
  const releaseProxy = dependencies.acquireProxy();
  let lastPercent = -1;
  try {
    await dependencies.install(dependencies.createStorage(configuration.rootDirectory), {
      operationId,
      signal,
      onPhase: (phase) => {
        report({ phase });
      },
      onProgress: ({ downloadedBytes, totalBytes }) => {
        const percent = Math.floor((downloadedBytes * 100) / totalBytes);
        if (percent === lastPercent) return;
        lastPercent = percent;
        report({ phase: "downloading", downloadedBytes, totalBytes });
      },
    });
    await complete();
  } finally {
    releaseProxy();
  }
}

async function main(): Promise<void> {
  if (process.send === undefined) throw new Error("install_worker_requires_ipc");
  const [root, operationId] = process.argv.slice(2);
  const lifecycle = new AbortController();
  process.on("message", (value: unknown) => {
    if (typeof value === "object" && value !== null && "type" in value && value.type === "cancel")
      lifecycle.abort();
  });
  process.once("disconnect", () => {
    lifecycle.abort();
  });
  await executeDecisionInstall(
    root,
    operationId,
    AbortSignal.any([lifecycle.signal, AbortSignal.timeout(60 * 60_000)]),
    (progress) => {
      if (process.connected)
        process.send?.({ type: "progress", ...progress }, undefined, undefined, () => undefined);
    },
    () =>
      new Promise<void>((resolve, reject) => {
        process.send?.({ type: "completed" }, undefined, undefined, (error: Error | null) => {
          if (error) reject(error);
          else resolve();
        });
      }),
  );
}

const entry = process.argv[1] === undefined ? undefined : pathToFileURL(process.argv[1]).href;
if (entry === import.meta.url) {
  void main()
    .catch(() => {
      process.exitCode = 1;
    })
    .finally(() => {
      if (process.connected) process.disconnect();
    });
}
