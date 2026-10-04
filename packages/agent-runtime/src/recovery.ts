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
import type { RuntimeEventStorePort } from "./types.js";

const RECOVERY_PAGE_SIZE = 256;
const DEFAULT_MAX_RECOVERY_EVENTS = 100_000;

export async function recoverRuntimeState(
  sessionId: SessionId,
  store: RuntimeEventStorePort,
  requestedLimit = DEFAULT_MAX_RECOVERY_EVENTS,
): Promise<AgentRuntimeState> {
  if (!Number.isSafeInteger(requestedLimit) || requestedLimit < 1 || requestedLimit > 1_000_000) {
    throw new RangeError("maxRecoveryEvents must be a safe integer from 1 to 1000000");
  }
  let state = createInitialAgentRuntimeState(sessionId);
  const targetRevision = await safeCurrentRevision(store, sessionId);
  let recovered = 0;

  // EventStore intentionally exposes bounded pages only. This sequential I/O loop preserves
  // stream order, reuses the injected store, and has a hard recovery-event budget.
  while (state.revision < targetRevision) {
    const remaining = requestedLimit - recovered;
    if (remaining <= 0) throw streamCorrupt();
    const page = await safeLoad(
      store,
      sessionId,
      state.revision,
      Math.min(RECOVERY_PAGE_SIZE, remaining),
    );
    if (page.length === 0) throw streamCorrupt();
    for (const stored of page) {
      state = applyStoredEvent(state, stored, sessionId);
      recovered += 1;
      if (state.revision === targetRevision) break;
      if (state.revision > targetRevision || recovered > requestedLimit) throw streamCorrupt();
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
    if (stored.sessionId !== sessionId || stored.type !== readPayloadType(stored.payload)) {
      throw streamCorrupt();
    }
    return reduceAgentRuntimeEvent(state, {
      revision: asRevision(stored.revision),
      event: parseAgentRuntimeEvent(stored.payload),
    });
  } catch {
    throw streamCorrupt();
  }
}

function readPayloadType(payload: unknown): string | undefined {
  if (typeof payload !== "object" || payload === null) return undefined;
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
    const page: unknown = await store.load(sessionId, { afterRevision, limit });
    if (!isStoredEventArray(page)) throw streamCorrupt();
    return page;
  } catch {
    throw streamCorrupt();
  }
}

function isStoredEventArray(value: unknown): value is readonly StoredEvent[] {
  return Array.isArray(value);
}

function streamCorrupt() {
  return runtimeError("EVENT_STREAM_CORRUPT", "Durable runtime event stream is invalid");
}
