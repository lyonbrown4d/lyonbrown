import { types as nodeTypes } from "node:util";

import {
  asRevision,
  createInitialAgentRuntimeState,
  parseAgentRuntimeEvent,
  reduceAgentRuntimeEvent,
  type AgentRuntimeState,
  type SessionId,
} from "@lyonbrown/agent-domain";
import type { StoredEvent } from "@lyonbrown/event-store";

import { runtimeError } from "./errors.js";
import { readRuntimeEventPage, type RuntimeEventStorePort } from "./event-store-port.js";

const RECOVERY_PAGE_SIZE = 256;
const MAX_RECOVERY_EVENTS = 100_000;

export async function recoverRuntimeState(
  sessionId: SessionId,
  store: RuntimeEventStorePort,
): Promise<AgentRuntimeState> {
  let state = createInitialAgentRuntimeState(sessionId);
  const targetRevision = await safeCurrentRevision(store, sessionId);
  if (targetRevision > BigInt(MAX_RECOVERY_EVENTS)) throw streamCorrupt();
  let recovered = 0;

  // EventStore intentionally exposes bounded pages only. This sequential I/O loop preserves
  // stream order, reuses the injected store, and has a hard recovery-event budget.
  while (state.revision < targetRevision) {
    const remaining = MAX_RECOVERY_EVENTS - recovered;
    if (remaining <= 0) throw streamCorrupt();
    const revisionsRemaining = Number(targetRevision - state.revision);
    const requestedPageSize = Math.min(RECOVERY_PAGE_SIZE, remaining, revisionsRemaining);
    const page = await safeLoad(store, sessionId, state.revision, requestedPageSize);
    if (page.length === 0) throw streamCorrupt();
    for (const stored of page) {
      if (recovered >= MAX_RECOVERY_EVENTS) throw streamCorrupt();
      state = applyStoredEvent(state, stored, sessionId);
      recovered += 1;
      if (state.revision > targetRevision) throw streamCorrupt();
    }
  }
  return state;
}

export function applyStoredEvent(
  state: AgentRuntimeState,
  stored: StoredEvent,
  sessionId: SessionId,
): AgentRuntimeState {
  try {
    const envelope = readStoredEvent(stored);
    if (envelope.sessionId !== sessionId || envelope.type !== readPayloadType(envelope.payload)) {
      throw streamCorrupt();
    }
    return reduceAgentRuntimeEvent(state, {
      revision: asRevision(envelope.revision),
      event: parseAgentRuntimeEvent(envelope.payload),
    });
  } catch {
    throw streamCorrupt();
  }
}

function readStoredEvent(value: unknown): StoredEvent {
  if (
    typeof value !== "object" ||
    value === null ||
    nodeTypes.isProxy(value) ||
    (Reflect.getPrototypeOf(value) !== Object.prototype && Reflect.getPrototypeOf(value) !== null)
  ) {
    throw streamCorrupt();
  }
  const expectedKeys = ["eventId", "sessionId", "revision", "type", "occurredAt", "payload"];
  const keys = Reflect.ownKeys(value);
  if (
    keys.length !== expectedKeys.length ||
    keys.some((key) => typeof key !== "string" || !expectedKeys.includes(key))
  ) {
    throw streamCorrupt();
  }
  const data = new Map<string, unknown>();
  for (const key of expectedKeys) {
    const descriptor = Reflect.getOwnPropertyDescriptor(value, key);
    if (descriptor === undefined || !("value" in descriptor) || !descriptor.enumerable) {
      throw streamCorrupt();
    }
    data.set(key, descriptor.value as unknown);
  }
  const eventId = data.get("eventId");
  const storedSessionId = data.get("sessionId");
  const revision = data.get("revision");
  const type = data.get("type");
  const occurredAt = data.get("occurredAt");
  if (
    typeof eventId !== "string" ||
    typeof storedSessionId !== "string" ||
    typeof revision !== "bigint" ||
    typeof type !== "string" ||
    typeof occurredAt !== "string"
  ) {
    throw streamCorrupt();
  }
  return {
    eventId,
    sessionId: storedSessionId,
    revision,
    type,
    occurredAt,
    payload: data.get("payload"),
  };
}

function readPayloadType(payload: unknown): string | undefined {
  if (typeof payload !== "object" || payload === null || nodeTypes.isProxy(payload)) {
    return undefined;
  }
  const descriptor = Reflect.getOwnPropertyDescriptor(payload, "type");
  return descriptor !== undefined && "value" in descriptor && typeof descriptor.value === "string"
    ? descriptor.value
    : undefined;
}

async function safeCurrentRevision(
  store: RuntimeEventStorePort,
  sessionId: string,
): Promise<bigint> {
  try {
    return asRevision(await store.currentRevision(sessionId));
  } catch {
    throw streamCorrupt();
  }
}

async function safeLoad(
  store: RuntimeEventStorePort,
  sessionId: string,
  afterRevision: bigint,
  limit: number,
): Promise<readonly StoredEvent[]> {
  try {
    return readRuntimeEventPage(await store.loadPage(sessionId, { afterRevision, limit }), limit);
  } catch {
    throw streamCorrupt();
  }
}

function streamCorrupt() {
  return runtimeError("EVENT_STREAM_CORRUPT", "Durable runtime event stream is invalid");
}
