import type { RecoveredToolCall, RecoveredToolCallStatus, StoredEvent } from "./types.ts";

interface MutableRecoveredToolCall {
  toolCallId: string;
  toolName?: string;
  status: RecoveredToolCallStatus;
  startedEventId: string;
  terminalEventId?: string;
}

function payloadField(payload: unknown, field: string): string | undefined {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) return undefined;
  const descriptor = Object.getOwnPropertyDescriptor(payload, field);
  return descriptor !== undefined && "value" in descriptor && typeof descriptor.value === "string"
    ? descriptor.value
    : undefined;
}

function terminalStatus(type: string): RecoveredToolCallStatus | undefined {
  if (type === "ToolSucceeded") return "completed";
  if (type === "ToolFailed") return "failed";
  if (type === "ToolCancelled") return "cancelled";
  return undefined;
}

export function recoverToolCalls(events: readonly StoredEvent[]): readonly RecoveredToolCall[] {
  const calls = new Map<string, MutableRecoveredToolCall>();
  const ordered = [...events].sort((left, right) => (left.revision < right.revision ? -1 : 1));

  for (const event of ordered) {
    const toolCallId = payloadField(event.payload, "toolCallId");
    if (toolCallId === undefined || toolCallId.length === 0) continue;
    if (event.type === "ToolStarted") {
      const toolName = payloadField(event.payload, "toolName");
      calls.set(toolCallId, {
        toolCallId,
        ...(toolName === undefined ? {} : { toolName }),
        status: "outcome-unknown",
        startedEventId: event.eventId,
      });
      continue;
    }
    const status = terminalStatus(event.type);
    const call = calls.get(toolCallId);
    if (status !== undefined && call !== undefined) {
      call.status = status;
      call.terminalEventId = event.eventId;
    }
  }
  return [...calls.values()];
}
