import {
  parseAgentRuntimeEvent,
  type AgentRuntimeState,
  type SessionId,
} from "@lyonbrown/agent-domain";
import type { KernelEvent } from "@lyonbrown/kernel-pi";

import { runtimeError } from "./errors.ts";
import type { RuntimeEventWriter } from "./event-writer.ts";

export function combineRuntimeSignal(
  userSignal: AbortSignal | undefined,
  lifecycleSignal: AbortSignal,
): AbortSignal {
  return userSignal === undefined
    ? lifecycleSignal
    : AbortSignal.any([userSignal, lifecycleSignal]);
}

export function hasUnresolvedTurn(state: AgentRuntimeState): boolean {
  const status = state.turns.at(-1)?.status;
  return status === "submitted" || status === "running";
}

export function assertActiveControl(
  state: AgentRuntimeState,
  running: boolean,
  claimPending: boolean,
): void {
  if (claimPending) return;
  const active = hasUnresolvedTurn(state);
  if (running && active) return;
  if (running || active) {
    throw runtimeError("TERMINAL_SESSION", "Runtime active state is no longer controllable");
  }
  throw runtimeError("NO_ACTIVE_TURN", "Runtime session has no active turn");
}

export async function recordKernelFailure(
  writer: RuntimeEventWriter,
  sessionId: SessionId,
  disposed: boolean,
): Promise<void> {
  if (disposed || writer.fatal !== undefined || writer.state.outcomeUnknown) return;
  if (!hasUnresolvedTurn(writer.state)) return;
  try {
    await writer.append(
      parseAgentRuntimeEvent({
        type: "run_failed",
        sessionId,
        code: "KERNEL_FAILURE",
        message: "Kernel session failed",
      }),
    );
  } catch {
    // The writer retains a sanitized sticky failure for the next command boundary.
  }
}

export function ignoresLateKernelEvent(state: AgentRuntimeState, event: KernelEvent): boolean {
  return (
    state.turns.at(-1)?.status === "failed" &&
    (!state.outcomeUnknown || event.type !== "run_failed")
  );
}
