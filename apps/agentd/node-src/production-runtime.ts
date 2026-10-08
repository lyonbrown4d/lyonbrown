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

import type {
  ProductionAgentdRuntime,
  ProductionRuntimeContext,
} from "@lyonbrown/agentd/internal/bootstrap";
import { collectCleanupFailure } from "@lyonbrown/agentd/internal/cleanup";
import {
  createProductionDecisionService,
  type ProductionDecisionService,
} from "@lyonbrown/agentd/internal/decision-service";
import {
  createModelAwareRuntime,
  type CreateModelAwareRuntimeOptions,
  type ModelAwareRuntime,
} from "@lyonbrown/agentd/internal/model-aware-runtime";
import { resolveLocalModelStorageDirectory } from "@lyonbrown/agentd/internal/model-storage-configuration";
import { agentdProxyEnvironment } from "@lyonbrown/agentd/internal/proxy-environment";
import { createProductionRuntime } from "@lyonbrown/agentd/internal/production-kernel";

export interface ProductionRuntimeDependencies {
  readonly openStore: (options: {
    readonly dataDirectory: string;
    readonly pathVerifier: PathSecurityVerifier;
  }) => EventStore;
  readonly createPathVerifier: (securityRoot: string) => PathSecurityVerifier;
  readonly createModelStorage: (modelsDirectory: string) => NativeModelStorageCapability;
  readonly createModelService: (storage: NativeModelStorageCapability) => LocalModelService;
  readonly createRuntime: (options: CreateModelAwareRuntimeOptions) => ModelAwareRuntime;
  readonly createDecisionService: (
    environment: Readonly<NodeJS.ProcessEnv>,
  ) => ProductionDecisionService;
  readonly environment: Readonly<NodeJS.ProcessEnv>;
  readonly acquireProxyEnvironment: () => () => void;
}

const DEFAULT_DEPENDENCIES: ProductionRuntimeDependencies = Object.freeze({
  openStore: openEventStore,
  createPathVerifier: (securityRoot: string) =>
    createNativeEventStorePathVerifier({ securityRoot }),
  createModelStorage: createNativeModelStorageCapability,
  createModelService: createLocalModelService,
  createRuntime: createModelAwareRuntime,
  createDecisionService: createProductionDecisionService,
  environment: process.env,
  acquireProxyEnvironment: () => agentdProxyEnvironment.acquire(),
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
  let decisionService: ProductionDecisionService | undefined;
  let releaseProxyEnvironment: (() => void) | undefined;
  try {
    releaseProxyEnvironment = dependencies.acquireProxyEnvironment();
    const recent = await eventStore.listSessions({ limit: 1 });
    const sessionId = recent[0]?.sessionId ?? context.scopeId;
    decisionService = dependencies.createDecisionService(dependencies.environment);
    modelService = dependencies.createModelService(
      dependencies.createModelStorage(
        resolveLocalModelStorageDirectory(
          dependencies.environment,
          join(context.dataDirectory, "models"),
        ),
      ),
    );
    const runtime = dependencies.createRuntime({
      sessionId,
      modelService,
      eventStore,
      createRuntime: (lease, modelId, store, activeSessionId) =>
        createProductionRuntime(
          lease,
          modelId,
          store,
          activeSessionId,
          context.canonicalWorkspace,
          decisionService?.port,
        ),
    });
    return withOwnedProductionResources(runtime, decisionService, releaseProxyEnvironment);
  } catch (error: unknown) {
    const failures: unknown[] = [error];
    if (modelService !== undefined) {
      const ownedModelService = modelService;
      await collectCleanupFailure(() => ownedModelService.dispose(), failures);
    }
    if (decisionService !== undefined) {
      const ownedDecisionService = decisionService;
      await collectCleanupFailure(() => ownedDecisionService.dispose(), failures);
    }
    if (releaseProxyEnvironment !== undefined) {
      await collectCleanupFailure(releaseProxyEnvironment, failures);
    }
    await collectCleanupFailure(() => {
      eventStore.close();
    }, failures);
    throw new AggregateError(failures, "production runtime composition failed", { cause: error });
  }
}

function withOwnedProductionResources(
  runtime: ModelAwareRuntime,
  decisionService: ProductionDecisionService,
  releaseProxyEnvironment: () => void,
): ProductionAgentdRuntime {
  let closeTask: Promise<void> | undefined;
  return Object.freeze({
    host: runtime.host,
    close(): Promise<void> {
      closeTask ??= closeRuntimeAndDecision(runtime, decisionService, releaseProxyEnvironment);
      return closeTask;
    },
  });
}

async function closeRuntimeAndDecision(
  runtime: ModelAwareRuntime,
  decisionService: ProductionDecisionService,
  releaseProxyEnvironment: () => void,
): Promise<void> {
  const failures: unknown[] = [];
  await collectCleanupFailure(() => runtime.close(), failures);
  await collectCleanupFailure(() => decisionService.dispose(), failures);
  await collectCleanupFailure(releaseProxyEnvironment, failures);
  if (failures.length > 0) {
    throw new AggregateError(failures, "production runtime cleanup failed");
  }
}
