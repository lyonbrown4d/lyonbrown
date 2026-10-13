import type { RuntimeDecisionPort } from "@lyonbrown/agent-runtime";

import { createSystemOneDecisionPort } from "@lyonbrown/agentd/internal/decision-adapter";
import { createDecisionModelController } from "@lyonbrown/agentd/internal/decision-model-controller";
import {
  createProductionDecisionService,
  type ProductionDecisionService,
} from "@lyonbrown/agentd/internal/decision-service";

/** Stable runtime port; missing models never freeze a failed lazy engine into the session. */
export function createManagedDecisionService(
  environment: Readonly<NodeJS.ProcessEnv>,
  dependencies: {
    readonly createController: typeof createDecisionModelController;
    readonly createService: typeof createProductionDecisionService;
  } = {
    createController: createDecisionModelController,
    createService: createProductionDecisionService,
  },
): ProductionDecisionService {
  const setup = dependencies.createController(environment);
  let closed = false;
  let service: ProductionDecisionService | undefined;
  const port: RuntimeDecisionPort = {
    evaluate(request) {
      const status = setup.status();
      if (closed)
        return createSystemOneDecisionPort({ unavailableReason: "provider_unavailable" }).evaluate(
          request,
        );
      if (status.phase !== "configured") {
        return createSystemOneDecisionPort({
          unavailableReason:
            status.failure?.code === "decision_model_corrupted"
              ? "model_corrupted"
              : status.phase === "unavailable"
                ? "provider_unavailable"
                : "model_missing",
        }).evaluate(request);
      }
      service ??= dependencies.createService(environment);
      return service.port.evaluate(request);
    },
  };
  return {
    port,
    setup,
    async dispose() {
      closed = true;
      try {
        await setup.dispose();
      } finally {
        await service?.dispose();
      }
    },
  };
}
