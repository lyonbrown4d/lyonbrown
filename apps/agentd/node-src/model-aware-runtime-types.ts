import type { AgentRuntime } from "@lyonbrown/agent-runtime";
import type { EventStore } from "@lyonbrown/event-store";
import type { LocalModelLease, LocalModelService } from "@lyonbrown/local-model";

import type { RuntimeHost } from "@lyonbrown/agentd/internal/runtime-host";
import type { DecisionModelControlPort } from "@lyonbrown/agentd/internal/decision-model-state";

export interface CreateModelAwareRuntimeOptions {
  readonly sessionId: string;
  readonly modelService: LocalModelService;
  readonly eventStore: EventStore;
  readonly decisionModel?: DecisionModelControlPort;
  readonly createRuntime?: (
    lease: LocalModelLease,
    modelId: string,
    eventStore: EventStore,
    sessionId: string,
  ) => Promise<AgentRuntime>;
}

export interface ModelAwareRuntime {
  readonly host: RuntimeHost;
  close(): Promise<void>;
}
