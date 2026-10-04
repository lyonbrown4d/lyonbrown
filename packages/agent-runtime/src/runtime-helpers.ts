import type { AgentRuntimeState } from "@lyonbrown/agent-domain";
import type { KernelEvent } from "@lyonbrown/kernel-pi";

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

export function ignoresLateKernelEvent(state: AgentRuntimeState, event: KernelEvent): boolean {
  return (
    state.turns.at(-1)?.status === "failed" &&
    (!state.outcomeUnknown || event.type !== "run_failed")
  );
}
