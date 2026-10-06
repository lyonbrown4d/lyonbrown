import { describe, expect, it } from "vitest";

import {
  EventAlreadyExistsError,
  PayloadValidationError,
  encodeEventPayload,
  openEventStore,
} from "@lyonbrown/event-store/internal/index";

describe("event-store input budgets", () => {
  it.each([
    ["negative revision", -1n, [{ eventId: "event", type: "Type", payload: {} }]],
    ["empty batch", 0n, []],
    [
      "oversized batch",
      0n,
      Array.from({ length: 257 }, (_, index) => ({
        eventId: `event-${String(index)}`,
        type: "Type",
        payload: {},
      })),
    ],
  ] as const)("rejects %s", async (_label, revision, events) => {
    const store = openEventStore({ testOnlyMemory: true });
    await expect(store.append("session", revision, events)).rejects.toBeInstanceOf(RangeError);
    store.close();
  });

  it.each([
    ["", "event", "Type"],
    ["s".repeat(129), "event", "Type"],
    ["session", "", "Type"],
    ["session", "event", ""],
    ["session", "event-\ud800", "Type"],
  ])("rejects invalid identifiers", async (sessionId, eventId, type) => {
    const store = openEventStore({ testOnlyMemory: true });
    await expect(
      store.append(sessionId, 0n, [{ eventId, type, payload: {} }]),
    ).rejects.toBeInstanceOf(TypeError);
    store.close();
  });

  it("rejects duplicate ids before starting a transaction", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    await expect(
      store.append("session", 0n, [
        { eventId: "event", type: "A", payload: {} },
        { eventId: "event", type: "B", payload: {} },
      ]),
    ).rejects.toBeInstanceOf(EventAlreadyExistsError);
    store.close();
  });

  it("rejects invalid timestamps and a batch larger than 4 MiB", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    await expect(
      store.append("session", 0n, [
        { eventId: "event", type: "Type", occurredAt: "not-a-date", payload: {} },
      ]),
    ).rejects.toBeInstanceOf(TypeError);

    const payload = "x".repeat(900_000);
    await expect(
      store.append(
        "session",
        0n,
        Array.from({ length: 5 }, (_, index) => ({
          eventId: `event-${String(index)}`,
          type: "Type",
          payload,
        })),
      ),
    ).rejects.toThrow(/4 MiB/i);
    store.close();
  });

  it.each([0, 1001, 1.5])("rejects invalid read limit %s", async (limit) => {
    const store = openEventStore({ testOnlyMemory: true });
    await expect(store.load("session", { afterRevision: 0n, limit })).rejects.toBeInstanceOf(
      RangeError,
    );
    await expect(store.listSessions({ limit })).rejects.toBeInstanceOf(RangeError);
    store.close();
  });

  it("rejects invalid read revisions, offsets and busy timeouts", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    await expect(store.load("session", { afterRevision: -1n, limit: 1 })).rejects.toBeInstanceOf(
      RangeError,
    );
    await expect(store.listSessions({ limit: 1, offset: -1 })).rejects.toBeInstanceOf(RangeError);
    store.close();
    expect(() => openEventStore({ testOnlyMemory: true, busyTimeoutMs: -1 })).toThrow(RangeError);
  });
});

describe("additional safe JSON shapes", () => {
  it.each([undefined, Symbol("unsafe"), () => undefined, new Date()])(
    "rejects unsupported payload %s",
    (payload) => {
      expect(() => encodeEventPayload(payload)).toThrow(PayloadValidationError);
    },
  );

  it("rejects cyclic objects", () => {
    const payload: { cyclic?: unknown } = {};
    payload.cyclic = payload;
    expect(() => encodeEventPayload(payload)).toThrow(/cycles/i);
  });

  it("rejects symbols, custom array properties and sparse arrays without rejecting DAGs", () => {
    const withSymbol = { ok: true, [Symbol("unsafe")]: true };
    const custom = [1] as number[] & { extra?: boolean };
    custom.extra = true;
    const sparse = new Array(1);
    const shared = {};

    expect(() => encodeEventPayload(withSymbol)).toThrow(/symbol/i);
    expect(() => encodeEventPayload(custom)).toThrow(/custom/i);
    expect(() => encodeEventPayload(sparse)).toThrow(/sparse/i);
    expect(() => encodeEventPayload({ first: shared, second: shared })).not.toThrow();
  });
});
