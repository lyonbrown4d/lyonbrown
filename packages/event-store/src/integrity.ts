import { decodeEventPayload } from "./codec.js";
import { CorruptEventStoreError } from "./errors.js";
import type { SessionMetadata, StoredEvent } from "./types.js";
import { isCanonicalIsoTimestamp, MAX_SQLITE_INTEGER } from "./validation.js";

const malformedUnicode = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u;
const MAX_IDENTIFIER_BYTES = 128;

export interface SessionIntegrityRow {
  readonly session_id: unknown;
  readonly current_revision: unknown;
  readonly created_at: unknown;
  readonly updated_at: unknown;
  readonly max_revision: unknown;
  readonly event_count: unknown;
}

export interface RuntimeEventRow {
  readonly session_id: unknown;
  readonly revision: unknown;
  readonly event_id: unknown;
  readonly event_type: unknown;
  readonly occurred_at: unknown;
  readonly payload_json: unknown;
}

function corrupt(message: string, cause?: unknown): never {
  throw new CorruptEventStoreError(message, cause === undefined ? undefined : { cause });
}

function storedIdentifier(value: unknown, role: string): string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    malformedUnicode.test(value) ||
    Buffer.byteLength(value, "utf8") > MAX_IDENTIFIER_BYTES
  ) {
    return corrupt(
      `${role} is not a well-formed 1-${String(MAX_IDENTIFIER_BYTES)} byte identifier`,
    );
  }
  return value;
}

function storedRevision(value: unknown, role: string, minimum: bigint): bigint {
  if (typeof value !== "bigint" || value < minimum || value > MAX_SQLITE_INTEGER) {
    return corrupt(`${role} is outside the signed 64-bit revision range`);
  }
  return value;
}

function storedTimestamp(value: unknown, role: string): string {
  if (!isCanonicalIsoTimestamp(value)) return corrupt(`${role} is not a canonical ISO timestamp`);
  return value;
}

export function validateSessionIntegrity(row: SessionIntegrityRow): SessionMetadata {
  const sessionId = storedIdentifier(row.session_id, "Stored sessionId");
  const currentRevision = storedRevision(row.current_revision, "Stored current revision", 0n);
  const maxRevision = storedRevision(row.max_revision, "Stored maximum revision", 0n);
  const eventCount = storedRevision(row.event_count, "Stored event count", 0n);
  if (currentRevision !== maxRevision || currentRevision !== eventCount) {
    corrupt(
      `Session ${JSON.stringify(sessionId)} revision metadata disagrees with its event stream`,
    );
  }
  return {
    sessionId,
    currentRevision,
    createdAt: storedTimestamp(row.created_at, "Stored session creation time"),
    updatedAt: storedTimestamp(row.updated_at, "Stored session update time"),
  };
}

export function validateEventRows(
  rows: readonly RuntimeEventRow[],
  sessionId: string,
  afterRevision: bigint,
): readonly StoredEvent[] {
  let expectedRevision = afterRevision + 1n;
  return rows.map((row) => {
    const rowSessionId = storedIdentifier(row.session_id, "Stored event sessionId");
    if (rowSessionId !== sessionId) corrupt("Stored event belongs to a different session");
    const revision = storedRevision(row.revision, "Stored event revision", 1n);
    if (revision !== expectedRevision) corrupt("Stored event revisions are not contiguous");
    expectedRevision += 1n;
    if (typeof row.payload_json !== "string") corrupt("Stored event payload is not text");
    try {
      return {
        eventId: storedIdentifier(row.event_id, "Stored eventId"),
        sessionId: rowSessionId,
        revision,
        type: storedIdentifier(row.event_type, "Stored event type"),
        occurredAt: storedTimestamp(row.occurred_at, "Stored event occurrence time"),
        payload: decodeEventPayload(row.payload_json),
      };
    } catch (error: unknown) {
      if (error instanceof CorruptEventStoreError) throw error;
      return corrupt("Stored event payload is invalid", error);
    }
  });
}
