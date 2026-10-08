import { randomUUID } from "node:crypto";

import { asSessionId } from "@lyonbrown/agent-domain";
import type { AgentRuntimeState } from "@lyonbrown/agent-domain";
import {
  createAgentRuntime,
  createRuntimeEventStorePort,
  type AgentRuntime,
  type RuntimeDecisionPort,
} from "@lyonbrown/agent-runtime";
import type { EventStore } from "@lyonbrown/event-store";
import {
  createPiKernelSession,
  type KernelEventSink,
  type KernelToolRegistry,
} from "@lyonbrown/kernel-pi";
import type { LocalModelLease } from "@lyonbrown/local-model";

import { createLocalKernelModelAdapter } from "@lyonbrown/agentd/internal/local-model-adapter";
import { SessionApprovalGate } from "@lyonbrown/agentd/internal/approval-gate";
import { PRODUCTION_KERNEL_BUDGET } from "@lyonbrown/agentd/internal/runtime-limits";
import { runtimeHistory } from "@lyonbrown/agentd/internal/runtime-history";
import { createWorkspaceToolRegistry } from "@lyonbrown/agentd/internal/workspace-tools";

export async function createProductionRuntime(
  lease: LocalModelLease,
  modelId: string,
  eventStore: EventStore,
  sessionId: string,
  workspace = process.cwd(),
  decision?: RuntimeDecisionPort,
): Promise<AgentRuntime> {
  const model = createLocalKernelModelAdapter({ id: modelId, lease });
  const toolRegistry = await createWorkspaceToolRegistry(workspace);
  const recovered = await createRuntime(eventStore, sessionId, model, toolRegistry, decision);
  if (!unsafeToResume(recovered.snapshot())) return recovered;
  await recovered.dispose();
  return createRuntime(eventStore, randomUUID(), model, toolRegistry, decision);
}

function createRuntime(
  eventStore: EventStore,
  sessionId: string,
  model: ReturnType<typeof createLocalKernelModelAdapter>,
  toolRegistry: KernelToolRegistry,
  decision: RuntimeDecisionPort | undefined,
): Promise<AgentRuntime> {
  const safeSessionId = asSessionId(sessionId);
  return createAgentRuntime({
    sessionId: safeSessionId,
    eventStore: createRuntimeEventStorePort(eventStore),
    ...(decision === undefined ? {} : { decision }),
    createKernelSession: ({ eventSink, state }) =>
      createKernel(eventSink, safeSessionId, model, toolRegistry, state),
  });
}

function unsafeToResume(state: AgentRuntimeState): boolean {
  return runtimeHistory(state) === undefined;
}

function createKernel(
  eventSink: KernelEventSink,
  sessionId: string,
  model: ReturnType<typeof createLocalKernelModelAdapter>,
  toolRegistry: KernelToolRegistry,
  state: AgentRuntimeState,
) {
  const approvalGate = new SessionApprovalGate(state.approvalPolicy);
  return createPiKernelSession({
    sessionId,
    systemPrompt: "You are Lyonbrown, a concise local coding assistant.",
    model,
    initialMessages: runtimeHistory(state) ?? [],
    eventSink,
    toolRegistry,
    approvalGate,
    secretResolver: { resolve: async () => Promise.reject(new Error("secret access denied")) },
    budget: PRODUCTION_KERNEL_BUDGET,
  });
}
