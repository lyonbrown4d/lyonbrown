import { EventAlreadyExistsError } from "./errors.js";
import type { EventToAppend, ListSessionsOptions, LoadOptions } from "./types.js";

export const MAX_BATCH_EVENTS = 256;
export const MAX_BATCH_PAYLOAD_BYTES = 4 * 1024 * 1024;
export const MAX_SQLITE_INTEGER = 9_223_372_036_854_775_807n;
const MAX_IDENTIFIER_BYTES = 128;
const MAX_READ_LIMIT = 1000;
const malformedUnicode = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u;

function assertIdentifier(value: string, role: string): void {
  if (
    value.length === 0 ||
    malformedUnicode.test(value) ||
    Buffer.byteLength(value, "utf8") > MAX_IDENTIFIER_BYTES
  ) {
    throw new TypeError(`${role} must be 1-${String(MAX_IDENTIFIER_BYTES)} UTF-8 bytes`);
  }
}

export function validateAppend(
  sessionId: string,
  expectedRevision: bigint,
  events: readonly EventToAppend[],
): void {
  assertIdentifier(sessionId, "sessionId");
  if (expectedRevision < 0n || expectedRevision > MAX_SQLITE_INTEGER) {
    throw new RangeError("expectedRevision is outside SQLite's signed 64-bit integer range");
  }
  if (events.length === 0 || events.length > MAX_BATCH_EVENTS) {
    throw new RangeError(`append requires 1-${String(MAX_BATCH_EVENTS)} events`);
  }
  if (expectedRevision + BigInt(events.length) > MAX_SQLITE_INTEGER) {
    throw new RangeError("revision would overflow SQLite's signed 64-bit integer range");
  }
  const ids = new Set<string>();
  for (const event of events) {
    assertIdentifier(event.eventId, "eventId");
    assertIdentifier(event.type, "event type");
    if (ids.has(event.eventId)) {
      throw new EventAlreadyExistsError(event.eventId);
    }
    ids.add(event.eventId);
    if (event.occurredAt !== undefined && !Number.isFinite(Date.parse(event.occurredAt))) {
      throw new TypeError("occurredAt must be an ISO-compatible timestamp");
    }
  }
}

export function validateLoad(sessionId: string, options: LoadOptions): void {
  assertIdentifier(sessionId, "sessionId");
  if (options.afterRevision < 0n || options.afterRevision > MAX_SQLITE_INTEGER) {
    throw new RangeError("afterRevision is outside SQLite's signed 64-bit integer range");
  }
  assertReadLimit(options.limit);
}

export function validateList(options: ListSessionsOptions): void {
  assertReadLimit(options.limit);
  if (
    options.offset !== undefined &&
    (!Number.isSafeInteger(options.offset) || options.offset < 0)
  ) {
    throw new RangeError("offset must be a non-negative safe integer");
  }
}

function assertReadLimit(limit: number): void {
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > MAX_READ_LIMIT) {
    throw new RangeError(`limit must be a safe integer from 1 to ${String(MAX_READ_LIMIT)}`);
  }
}
