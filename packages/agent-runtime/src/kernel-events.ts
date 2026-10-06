import {
  parseAgentRuntimeEvent,
  type AgentRuntimeEvent,
  type SessionId,
} from "@lyonbrown/agent-domain";
import type { KernelEvent } from "@lyonbrown/kernel-pi";

export function mapKernelEvent(sessionId: SessionId, event: KernelEvent): AgentRuntimeEvent {
  switch (event.type) {
    case "run_started":
    case "run_completed":
    case "run_cancelled":
      return parseAgentRuntimeEvent({ type: event.type, sessionId: event.sessionId });
    case "run_failed":
      return parseAgentRuntimeEvent({
        type: event.type,
        sessionId: event.sessionId,
        code: event.code,
        message: event.message,
      });
    case "turn_started":
    case "turn_completed":
      return parseAgentRuntimeEvent({ type: event.type, sessionId, turn: event.turn });
    case "text_delta":
      return parseAgentRuntimeEvent({
        type: "assistant_text_delta",
        sessionId,
        delta: event.delta,
      });
    case "usage":
      return parseAgentRuntimeEvent({ type: "usage_recorded", sessionId, ...event.usage });
    case "approval_requested":
      return parseAgentRuntimeEvent({
        type: event.type,
        sessionId,
        requestId: event.requestId,
        toolName: event.toolName,
        risk: event.risk,
        permissions: event.permissions,
        argumentsPreview: event.argumentsPreview,
      });
    case "approval_resolved":
      return parseAgentRuntimeEvent({
        type: event.type,
        sessionId,
        requestId: event.requestId,
        decision: event.decision,
      });
    case "tool_started":
      return parseAgentRuntimeEvent({
        type: event.type,
        sessionId,
        callId: event.callId,
        toolName: event.toolName,
        argumentsPreview: event.argumentsPreview,
      });
    case "tool_succeeded":
      return parseAgentRuntimeEvent({
        type: event.type,
        sessionId,
        callId: event.callId,
        toolName: event.toolName,
        content: event.content,
        ...(event.details === undefined ? {} : { details: event.details }),
      });
    case "tool_failed":
    case "tool_outcome_unknown":
      return parseAgentRuntimeEvent({
        type: event.type,
        sessionId,
        callId: event.callId,
        toolName: event.toolName,
        code: event.code,
        message: event.message,
      });
    case "runtime_error":
      return parseAgentRuntimeEvent({
        type: event.type,
        sessionId,
        code: event.code,
        message: event.message,
      });
  }
}
