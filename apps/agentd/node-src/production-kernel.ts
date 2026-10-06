import { randomUUID } from "node:crypto";

import { asSessionId } from "@lyonbrown/agent-domain";
import type { AgentRuntimeState } from "@lyonbrown/agent-domain";
import {
  createAgentRuntime,
  createRuntimeEventStorePort,
  type AgentRuntime,
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
import { KERNEL_MAX_INPUT_BYTES } from "@lyonbrown/agentd/internal/runtime-limits";
import { createWorkspaceToolRegistry } from "@lyonbrown/agentd/internal/workspace-tools";

export async function createProductionRuntime(
  lease: LocalModelLease,
  modelId: string,
  eventStore: EventStore,
  sessionId: string,
  workspace = process.cwd(),
): Promise<AgentRuntime> {
  const model = createLocalKernelModelAdapter({ id: modelId, lease });
  const toolRegistry = await createWorkspaceToolRegistry(workspace);
  const recovered = await createRuntime(eventStore, sessionId, model, toolRegistry);
  if (!unsafeToResume(recovered.snapshot())) return recovered;
  await recovered.dispose();
  return createRuntime(eventStore, randomUUID(), model, toolRegistry);
}

function createRuntime(
  eventStore: EventStore,
  sessionId: string,
  model: ReturnType<typeof createLocalKernelModelAdapter>,
  toolRegistry: KernelToolRegistry,
): Promise<AgentRuntime> {
  const safeSessionId = asSessionId(sessionId);
  return createAgentRuntime({
    sessionId: safeSessionId,
    eventStore: createRuntimeEventStorePort(eventStore),
    createKernelSession: ({ eventSink }) =>
      createKernel(eventSink, safeSessionId, model, toolRegistry),
  });
}

function unsafeToResume(state: AgentRuntimeState): boolean {
  if (
    state.turns.length > 0 ||
    state.outcomeUnknown ||
    state.phase === "failed_terminal" ||
    state.phase === "running"
  ) {
    return true;
  }
  if (state.turns.some((turn) => turn.status === "submitted" || turn.status === "running")) {
    return true;
  }
  return state.tools.some((tool) => tool.status === "running" || tool.status === "outcome_unknown");
}

function createKernel(
  eventSink: KernelEventSink,
  sessionId: string,
  model: ReturnType<typeof createLocalKernelModelAdapter>,
  toolRegistry: KernelToolRegistry,
) {
  const approvalGate = new SessionApprovalGate();
  return createPiKernelSession({
    sessionId,
    systemPrompt: "You are Lyonbrown, a concise local coding assistant.",
    model,
    eventSink,
    toolRegistry,
    approvalGate,
    secretResolver: { resolve: async () => Promise.reject(new Error("secret access denied")) },
    budget: {
      maxInputBytes: KERNEL_MAX_INPUT_BYTES,
      maxTurns: 8,
      maxSteps: 16,
      maxOutputBytes: 262_144,
      maxToolResultBytes: 65_536,
      maxValueDepth: 16,
      maxValueStringBytes: 65_536,
      maxCollectionEntries: 1_024,
      maxValueNodes: 8_192,
      toolTimeoutMs: 30_000,
      maxSecretCount: 1,
      maxSecretBytes: 1,
    },
  });
}
