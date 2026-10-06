import { env } from "@huggingface/transformers";
import { OpenJev } from "open-jev";

import { isValidLocalOpenJevConfiguration } from "@lyonbrown/decision-engine/internal/open-jev-configuration";
import type { OpenJevRuntime } from "@lyonbrown/decision-engine/internal/open-jev-runtime";
import type { OpenJevWorkerTask } from "@lyonbrown/decision-engine/internal/worker-engine";

interface OpenJevLoadOptions {
  readonly model: string;
  readonly device: "cpu";
  readonly dtype: "q4";
  readonly truncation: "error";
}

export interface OpenJevWorkerDependencies {
  readonly load: (options: OpenJevLoadOptions) => Promise<OpenJevRuntime>;
  readonly disableRemoteModels: () => void;
}

const productionDependencies: OpenJevWorkerDependencies = {
  load: async (options) => OpenJev.load(options),
  disableRemoteModels(): void {
    env.allowRemoteModels = false;
    env.allowLocalModels = true;
    env.useFSCache = false;
    env.cacheDir = null;
  },
};

const configurationKey = (task: OpenJevWorkerTask): string =>
  `${task.configuration.modelPath}\0${task.configuration.modelId}\0${task.configuration.modelRevision}`;

export function createOpenJevWorker(
  dependencies: OpenJevWorkerDependencies,
): (task: OpenJevWorkerTask) => Promise<unknown> {
  let runtimeTask: Promise<OpenJevRuntime> | undefined;
  let activeConfigurationKey: string | undefined;

  return async (task: OpenJevWorkerTask): Promise<unknown> => {
    if (!isValidLocalOpenJevConfiguration(task.configuration)) {
      throw new Error("OPEN_JEV_WORKER_CONFIGURATION_INVALID");
    }
    const nextConfigurationKey = configurationKey(task);
    if (activeConfigurationKey !== undefined && activeConfigurationKey !== nextConfigurationKey) {
      throw new Error("OPEN_JEV_WORKER_CONFIGURATION_CHANGED");
    }
    if (runtimeTask === undefined) {
      activeConfigurationKey = nextConfigurationKey;
      dependencies.disableRemoteModels();
      runtimeTask = dependencies.load({
        model: task.configuration.modelPath,
        device: "cpu",
        dtype: "q4",
        truncation: "error",
      });
    }
    const runtime = await runtimeTask;
    return runtime.decide(task.state, task.questions, { truncation: "error" });
  };
}

export default createOpenJevWorker(productionDependencies);
