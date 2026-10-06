import { domainError } from "@lyonbrown/agent-domain/internal/errors";
import type { SessionId } from "@lyonbrown/agent-domain/internal/identifiers";
import { incrementRevision, type Revision } from "@lyonbrown/agent-domain/internal/revision";
import type { AgentRuntimeEvent } from "@lyonbrown/agent-domain/internal/runtime-events";
import { hasActiveTurn } from "@lyonbrown/agent-domain/internal/runtime-state-mutations";
import { applyRuntimeEvent } from "@lyonbrown/agent-domain/internal/runtime-state-reducer";
import type { AgentRuntimeState } from "@lyonbrown/agent-domain/internal/runtime-state-types";
import {
  ZERO_RUNTIME_USAGE,
  freezeState,
} from "@lyonbrown/agent-domain/internal/runtime-state-types";

export type {
  AgentRuntimePhase,
  AgentRuntimeState,
  RuntimeActivityState,
  RuntimeFailure,
  RuntimeToolState,
  RuntimeTurnState,
} from "@lyonbrown/agent-domain/internal/runtime-state-types";

export interface VersionedAgentRuntimeEvent {
  readonly revision: Revision;
  readonly event: AgentRuntimeEvent;
}

export function createInitialAgentRuntimeState(sessionId: SessionId): AgentRuntimeState {
  return freezeState({
    sessionId,
    revision: 0n as Revision,
    phase: "uninitialized",
    turns: [],
    tools: [],
    queuedFollowUps: [],
    usage: ZERO_RUNTIME_USAGE,
    assistantTextBytes: 0,
    outcomeUnknown: false,
  });
}

export function reduceAgentRuntimeEvent(
  state: AgentRuntimeState,
  input: VersionedAgentRuntimeEvent,
): AgentRuntimeState {
  if (input.event.sessionId !== state.sessionId) {
    throw domainError("failed_precondition", "Runtime event belongs to a different session", {
      reason: "session_mismatch",
      expected: state.sessionId,
      actual: input.event.sessionId,
    });
  }
  const expectedRevision = incrementRevision(state.revision);
  if (input.revision !== expectedRevision) {
    throw domainError("failed_precondition", "Runtime event revision is not contiguous", {
      reason: "revision_mismatch",
      expected: expectedRevision,
      actual: input.revision,
    });
  }
  if (state.phase === "uninitialized" && input.event.type !== "session_opened") {
    throw domainError("failed_precondition", "The first runtime event must be session_opened", {
      reason: "session_not_opened",
    });
  }
  if (state.phase !== "uninitialized" && input.event.type === "session_opened") {
    throw domainError("failed_precondition", "Runtime session is already opened", {
      reason: "session_already_opened",
    });
  }
  if (
    state.phase === "failed_terminal" &&
    !(state.outcomeUnknown && input.event.type === "run_failed")
  ) {
    throw domainError("failed_precondition", "Runtime session is terminal", {
      reason: state.outcomeUnknown ? "terminal_uncertain_outcome" : "terminal_runtime_error",
    });
  }
  if (
    input.event.type === "user_message_submitted" &&
    (hasActiveTurn(state) ||
      state.phase === "running" ||
      state.tools.some((tool) => tool.status === "running"))
  ) {
    throw domainError("failed_precondition", "Runtime session has unresolved work", {
      reason: "unresolved_runtime_work",
    });
  }
  return applyRuntimeEvent(state, input.revision, input.event);
}
