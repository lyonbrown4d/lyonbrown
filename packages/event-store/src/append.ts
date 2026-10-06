import type Database from "better-sqlite3";

import { decodeEventPayload, encodeEventPayload } from "@lyonbrown/event-store/internal/codec";
import {
  EventAlreadyExistsError,
  RevisionConflictError,
  RevisionExhaustedError,
  StorageBusyError,
} from "@lyonbrown/event-store/internal/errors";
import type { EventToAppend, StoredEvent } from "@lyonbrown/event-store/internal/types";
import {
  MAX_BATCH_PAYLOAD_BYTES,
  MAX_SQLITE_INTEGER,
  validateAppend,
} from "@lyonbrown/event-store/internal/validation";

interface RevisionRow {
  readonly current_revision: bigint;
}

interface EncodedEvent {
  readonly event: EventToAppend;
  readonly occurredAt: string;
  readonly payloadJson: string;
}

function sqliteCode(error: unknown): string | undefined {
  return typeof error === "object" &&
    error !== null &&
    "code" in error &&
    typeof error.code === "string"
    ? error.code
    : undefined;
}

function encodeBatch(events: readonly EventToAppend[], now: string): readonly EncodedEvent[] {
  const encoded: EncodedEvent[] = [];
  let batchBytes = 0;
  for (const event of events) {
    const payloadJson = encodeEventPayload(event.payload);
    batchBytes += Buffer.byteLength(payloadJson, "utf8");
    if (batchBytes > MAX_BATCH_PAYLOAD_BYTES) {
      throw new RangeError("Encoded event batch exceeds the 4 MiB payload budget");
    }
    encoded.push({ event, occurredAt: event.occurredAt ?? now, payloadJson });
  }
  return encoded;
}

export function appendEvents(
  database: Database.Database,
  busyTimeoutMs: number,
  sessionId: string,
  expectedRevision: bigint,
  events: readonly EventToAppend[],
): readonly StoredEvent[] {
  validateAppend(sessionId, expectedRevision, events);
  const now = new Date().toISOString();
  const encoded = encodeBatch(events, now);
  const findRevision = database.prepare<[string], RevisionRow>(
    "SELECT current_revision FROM sessions WHERE session_id = ?",
  );
  const findEvent = database.prepare<[string], { readonly event_id: string }>(
    "SELECT event_id FROM events WHERE event_id = ?",
  );
  const insertSession = database.prepare(
    "INSERT INTO sessions(session_id, current_revision, created_at, updated_at) VALUES (?, ?, ?, ?)",
  );
  const updateSession = database.prepare(
    "UPDATE sessions SET current_revision = ?, updated_at = ? WHERE session_id = ?",
  );
  const insertEvent = database.prepare(
    "INSERT INTO events(session_id, revision, event_id, event_type, occurred_at, payload_json) VALUES (?, ?, ?, ?, ?, ?)",
  );

  const transaction = database.transaction((): readonly StoredEvent[] => {
    for (const item of encoded) {
      if (findEvent.get(item.event.eventId) !== undefined) {
        throw new EventAlreadyExistsError(item.event.eventId);
      }
    }
    const revisionRow = findRevision.get(sessionId);
    const actualRevision = revisionRow?.current_revision ?? 0n;
    if (actualRevision !== expectedRevision) {
      throw new RevisionConflictError(sessionId, expectedRevision, actualRevision);
    }
    if (actualRevision + BigInt(encoded.length) > MAX_SQLITE_INTEGER) {
      throw new RevisionExhaustedError(sessionId, actualRevision);
    }
    if (revisionRow === undefined) insertSession.run(sessionId, 0n, now, now);

    const stored: StoredEvent[] = [];
    let revision = actualRevision;
    for (const item of encoded) {
      revision += 1n;
      insertEvent.run(
        sessionId,
        revision,
        item.event.eventId,
        item.event.type,
        item.occurredAt,
        item.payloadJson,
      );
      stored.push({
        eventId: item.event.eventId,
        sessionId,
        revision,
        type: item.event.type,
        occurredAt: item.occurredAt,
        payload: decodeEventPayload(item.payloadJson),
      });
    }
    updateSession.run(revision, now, sessionId);
    return stored;
  });

  try {
    return transaction.immediate();
  } catch (error: unknown) {
    if (
      error instanceof RevisionConflictError ||
      error instanceof RevisionExhaustedError ||
      error instanceof EventAlreadyExistsError
    ) {
      throw error;
    }
    const code = sqliteCode(error);
    if (code?.startsWith("SQLITE_BUSY") === true) {
      throw new StorageBusyError(busyTimeoutMs, { cause: error });
    }
    if (code?.startsWith("SQLITE_CONSTRAINT_UNIQUE") === true) {
      const duplicate = encoded.find((item) => findEvent.get(item.event.eventId) !== undefined);
      throw new EventAlreadyExistsError(duplicate?.event.eventId ?? "unknown", { cause: error });
    }
    throw error;
  }
}
