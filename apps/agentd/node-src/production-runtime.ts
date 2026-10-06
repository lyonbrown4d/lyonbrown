import { join } from "node:path";

import {
  createNativeEventStorePathVerifier,
  createNativeModelStorageCapability,
} from "@lyonbrown/control-plane-native";
import { openEventStore } from "@lyonbrown/event-store";
import type { EventStore, PathSecurityVerifier } from "@lyonbrown/event-store";
import type { LocalModelService } from "@lyonbrown/local-model";
import {
  createLocalModelService,
  type NativeModelStorageCapability,
} from "@lyonbrown/local-model/composition";

import type { ProductionAgentdRuntime, ProductionRuntimeContext } from "./bootstrap.ts";
import { collectCleanupFailure } from "./cleanup.ts";
import {
  createModelAwareRuntime,
  type CreateModelAwareRuntimeOptions,
  type ModelAwareRuntime,
} from "./model-aware-runtime.ts";

export interface ProductionRuntimeDependencies {
  readonly openStore: (options: {
    readonly dataDirectory: string;
    readonly pathVerifier: PathSecurityVerifier;
  }) => EventStore;
  readonly createPathVerifier: (securityRoot: string) => PathSecurityVerifier;
  readonly createModelStorage: (modelsDirectory: string) => NativeModelStorageCapability;
  readonly createModelService: (storage: NativeModelStorageCapability) => LocalModelService;
  readonly createRuntime: (options: CreateModelAwareRuntimeOptions) => ModelAwareRuntime;
}

const DEFAULT_DEPENDENCIES: ProductionRuntimeDependencies = Object.freeze({
  openStore: openEventStore,
  createPathVerifier: (securityRoot: string) =>
    createNativeEventStorePathVerifier({ securityRoot }),
  createModelStorage: createNativeModelStorageCapability,
  createModelService: createLocalModelService,
  createRuntime: createModelAwareRuntime,
});

export async function createProductionAgentdRuntime(
  context: ProductionRuntimeContext,
  dependencies: ProductionRuntimeDependencies = DEFAULT_DEPENDENCIES,
): Promise<ProductionAgentdRuntime> {
  const eventStore = dependencies.openStore({
    dataDirectory: join(context.dataDirectory, "events"),
    pathVerifier: dependencies.createPathVerifier(context.dataDirectory),
  });
  let modelService: LocalModelService | undefined;
  try {
    const recent = await eventStore.listSessions({ limit: 1 });
    const sessionId = recent[0]?.sessionId ?? context.scopeId;
    modelService = dependencies.createModelService(
      dependencies.createModelStorage(join(context.dataDirectory, "models")),
    );
    return dependencies.createRuntime({ sessionId, modelService, eventStore });
  } catch (error: unknown) {
    const failures: unknown[] = [error];
    if (modelService !== undefined) {
      const ownedModelService = modelService;
      await collectCleanupFailure(() => ownedModelService.dispose(), failures);
    }
    await collectCleanupFailure(() => {
      eventStore.close();
    }, failures);
    throw new AggregateError(failures, "production runtime composition failed", { cause: error });
  }
}
