import { randomUUID } from "node:crypto";

import type { AgentRuntime } from "@lyonbrown/agent-runtime";
import type { EventStore } from "@lyonbrown/event-store";
import type { LocalModelLease } from "@lyonbrown/local-model";

import type { CreateModelAwareRuntimeOptions } from "@lyonbrown/agentd/internal/model-aware-runtime-types";
import { createProductionRuntime } from "@lyonbrown/agentd/internal/production-kernel";

export async function rotateRuntimeSession(options: {
  readonly current: AgentRuntime;
  readonly lease: LocalModelLease;
  readonly modelId: string;
  readonly eventStore: EventStore;
  readonly createRuntime: CreateModelAwareRuntimeOptions["createRuntime"];
}): Promise<AgentRuntime> {
  await options.current.dispose();
  const create = options.createRuntime ?? createProductionRuntime;
  return create(options.lease, options.modelId, options.eventStore, randomUUID());
}
