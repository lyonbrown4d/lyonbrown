import { types as nodeTypes } from "node:util";

import { domainError } from "./errors.js";
import { asSessionId, assertIdentifier, type SessionId } from "./identifiers.js";
import { copyRuntimeJson, type RuntimeJsonValue } from "./runtime-json.js";
import { validateText } from "./text.js";

interface RuntimeEventBase {
  readonly sessionId: SessionId;
}

export interface RuntimeUsage {
  readonly inputTokens: number;
  readonly outputTokens: number;
  readonly totalTokens: number;
}

export type AgentRuntimeEvent =
  | ({ readonly type: "session_opened" } & RuntimeEventBase)
  | ({ readonly type: "user_message_submitted"; readonly text: string } & RuntimeEventBase)
  | ({ readonly type: "run_started" } & RuntimeEventBase)
  | ({ readonly type: "run_completed" } & RuntimeEventBase)
  | ({ readonly type: "run_cancelled" } & RuntimeEventBase)
  | ({ readonly type: "turn_started"; readonly turn: number } & RuntimeEventBase)
  | ({ readonly type: "turn_completed"; readonly turn: number } & RuntimeEventBase)
  | ({ readonly type: "assistant_text_delta"; readonly delta: string } & RuntimeEventBase)
  | ({ readonly type: "usage_recorded" } & RuntimeEventBase & RuntimeUsage)
  | ({
      readonly type: "tool_started";
      readonly callId: string;
      readonly toolName: string;
      readonly argumentsPreview: RuntimeJsonValue;
    } & RuntimeEventBase)
  | ({
      readonly type: "tool_succeeded";
      readonly callId: string;
      readonly toolName: string;
      readonly content: string;
      readonly details?: RuntimeJsonValue;
    } & RuntimeEventBase)
  | ({
      readonly type: "tool_failed" | "tool_outcome_unknown";
      readonly callId: string;
      readonly toolName: string;
      readonly code: string;
      readonly message: string;
    } & RuntimeEventBase)
  | ({
      readonly type: "run_failed" | "runtime_error";
      readonly code: string;
      readonly message: string;
    } & RuntimeEventBase);

const FIELDS_BY_TYPE: Readonly<Record<AgentRuntimeEvent["type"], ReadonlySet<string>>> = {
  session_opened: new Set(["type", "sessionId"]),
  user_message_submitted: new Set(["type", "sessionId", "text"]),
  run_started: new Set(["type", "sessionId"]),
  run_completed: new Set(["type", "sessionId"]),
  run_cancelled: new Set(["type", "sessionId"]),
  turn_started: new Set(["type", "sessionId", "turn"]),
  turn_completed: new Set(["type", "sessionId", "turn"]),
  assistant_text_delta: new Set(["type", "sessionId", "delta"]),
  usage_recorded: new Set(["type", "sessionId", "inputTokens", "outputTokens", "totalTokens"]),
  tool_started: new Set(["type", "sessionId", "callId", "toolName", "argumentsPreview"]),
  tool_succeeded: new Set(["type", "sessionId", "callId", "toolName", "content", "details"]),
  tool_failed: new Set(["type", "sessionId", "callId", "toolName", "code", "message"]),
  tool_outcome_unknown: new Set(["type", "sessionId", "callId", "toolName", "code", "message"]),
  run_failed: new Set(["type", "sessionId", "code", "message"]),
  runtime_error: new Set(["type", "sessionId", "code", "message"]),
};

type DataRecord = ReadonlyMap<string, unknown>;

function invalid(message: string, reason: string, field = "event"): never {
  throw domainError("invalid_argument", message, { field, reason });
}

function readRecord(value: unknown): DataRecord {
  if (
    typeof value !== "object" ||
    value === null ||
    nodeTypes.isProxy(value) ||
    (Reflect.getPrototypeOf(value) !== Object.prototype && Reflect.getPrototypeOf(value) !== null)
  ) {
    return invalid("Runtime event must be a plain data object", "not_plain_data");
  }
  const result = new Map<string, unknown>();
  for (const key of Reflect.ownKeys(value)) {
    if (typeof key !== "string") return invalid("Runtime event has a symbol key", "symbol_key");
    const descriptor = Reflect.getOwnPropertyDescriptor(value, key);
    if (descriptor === undefined || !("value" in descriptor) || !descriptor.enumerable) {
      return invalid("Runtime event must contain plain data properties", "not_plain_data", key);
    }
    result.set(key, descriptor.value as unknown);
  }
  return result;
}

function required(record: DataRecord, field: string): unknown {
  if (!record.has(field))
    return invalid(`Runtime event is missing ${field}`, "missing_field", field);
  return record.get(field);
}

function text(record: DataRecord, field: string): string {
  return validateText(required(record, field), field);
}

function identifier(record: DataRecord, field: string): string {
  return assertIdentifier(required(record, field), field);
}

function count(record: DataRecord, field: string): number {
  const value = required(record, field);
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    return invalid(`${field} must be a non-negative safe integer`, "invalid_count", field);
  }
  return value;
}

function eventType(record: DataRecord): AgentRuntimeEvent["type"] {
  const value = required(record, "type");
  if (typeof value !== "string" || !Object.hasOwn(FIELDS_BY_TYPE, value)) {
    return invalid("Unsupported runtime event type", "unsupported_event_type", "type");
  }
  return value as AgentRuntimeEvent["type"];
}

export function parseAgentRuntimeEvent(value: unknown): AgentRuntimeEvent {
  const record = readRecord(value);
  const type = eventType(record);
  const allowed = FIELDS_BY_TYPE[type];
  for (const field of record.keys()) {
    if (!allowed.has(field))
      return invalid(`Runtime event has unexpected field ${field}`, "unexpected_field", field);
  }
  const sessionId = asSessionId(required(record, "sessionId"));
  switch (type) {
    case "session_opened":
    case "run_started":
    case "run_completed":
    case "run_cancelled":
      return Object.freeze({ type, sessionId });
    case "user_message_submitted":
      return Object.freeze({ type, sessionId, text: text(record, "text") });
    case "turn_started":
    case "turn_completed":
      return Object.freeze({ type, sessionId, turn: count(record, "turn") });
    case "assistant_text_delta":
      return Object.freeze({ type, sessionId, delta: text(record, "delta") });
    case "usage_recorded":
      return Object.freeze({
        type,
        sessionId,
        inputTokens: count(record, "inputTokens"),
        outputTokens: count(record, "outputTokens"),
        totalTokens: count(record, "totalTokens"),
      });
    case "tool_started":
      return Object.freeze({
        type,
        sessionId,
        callId: identifier(record, "callId"),
        toolName: identifier(record, "toolName"),
        argumentsPreview: copyRuntimeJson(required(record, "argumentsPreview")),
      });
    case "tool_succeeded": {
      const base = {
        type,
        sessionId,
        callId: identifier(record, "callId"),
        toolName: identifier(record, "toolName"),
        content: text(record, "content"),
      } as const;
      return Object.freeze(
        record.has("details") ? { ...base, details: copyRuntimeJson(record.get("details")) } : base,
      );
    }
    case "tool_failed":
    case "tool_outcome_unknown":
      return Object.freeze({
        type,
        sessionId,
        callId: identifier(record, "callId"),
        toolName: identifier(record, "toolName"),
        code: identifier(record, "code"),
        message: text(record, "message"),
      });
    case "run_failed":
    case "runtime_error":
      return Object.freeze({
        type,
        sessionId,
        code: identifier(record, "code"),
        message: text(record, "message"),
      });
  }
}
