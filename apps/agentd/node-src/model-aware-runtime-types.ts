import type { AgentRuntime } from "@lyonbrown/agent-runtime";
import type { EventStore } from "@lyonbrown/event-store";
import type { LocalModelLease, LocalModelService } from "@lyonbrown/local-model";

import type { RuntimeHost } from "./runtime-host.ts";

export interface CreateModelAwareRuntimeOptions {
  readonly sessionId: string;
  readonly modelService: LocalModelService;
  readonly eventStore: EventStore;
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
