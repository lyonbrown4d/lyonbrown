import { types as nodeTypes } from "node:util";

import { asSessionId } from "@lyonbrown/agent-domain";
import type { StoredEvent } from "@lyonbrown/event-store";
import { describe, expect, test } from "vitest";

import {
  createRuntimeEventPage,
  type RuntimeEventStorePort,
} from "@lyonbrown/agent-runtime/internal/event-store-port";
import { recoverRuntimeState } from "@lyonbrown/agent-runtime/internal/recovery";

const sessionId = asSessionId("session-recovery-hardening");

describe("recovery page hardening", () => {
  test("uses only frozen plain page capabilities across the async port", () => {
    const page = createRuntimeEventPage([opened(1n)], 1);

    expect(Object.isFrozen(page)).toBe(true);
    expect(Reflect.getPrototypeOf(page)).toBe(Object.prototype);
  });

  test("enforces the fixed 100000-event ceiling before loading", async () => {
    let loads = 0;
    const store: RuntimeEventStorePort = {
      currentRevision: () => Promise.resolve(100_001n),
      loadPage: () => {
        loads += 1;
        return Promise.resolve(createRuntimeEventPage([], 1));
      },
      append: () => Promise.resolve([]),
    };

    await expect(recoverRuntimeState(sessionId, store)).rejects.toMatchObject({
      code: "EVENT_STREAM_CORRUPT",
    });
    expect(loads).toBe(0);
  });

  test("rejects an oversized page before applying its first event", async () => {
    const events = [opened(1n), opened(2n)];
    const store: RuntimeEventStorePort = {
      currentRevision: () => Promise.resolve(1n),
      loadPage: () => Promise.resolve(createRuntimeEventPage(events, events.length)),
      append: () => Promise.resolve([]),
    };
    await expect(recoverRuntimeState(sessionId, store)).rejects.toMatchObject({
      code: "EVENT_STREAM_CORRUPT",
    });
  });

  test("rejects proxy pages without executing any proxy trap", async () => {
    let traps = 0;
    const hostile = new Proxy([] as StoredEvent[], {
      get: () => {
        traps += 1;
        throw new Error("trap executed");
      },
      getOwnPropertyDescriptor: () => {
        traps += 1;
        throw new Error("trap executed");
      },
      ownKeys: () => {
        traps += 1;
        throw new Error("trap executed");
      },
      getPrototypeOf: () => {
        traps += 1;
        throw new Error("trap executed");
      },
    });
    expect(nodeTypes.isProxy(hostile)).toBe(true);

    await expect(recoverRuntimeState(sessionId, onePageStore(hostile))).rejects.toMatchObject({
      code: "EVENT_STREAM_CORRUPT",
    });
    expect(traps).toBe(0);
  });

  test("rejects accessor and sparse pages without reading an accessor", async () => {
    let accessorCalls = 0;
    const accessorPage: StoredEvent[] = [];
    Object.defineProperty(accessorPage, "0", {
      enumerable: true,
      get: () => {
        accessorCalls += 1;
        return opened(1n);
      },
    });
    Object.defineProperty(accessorPage, "length", { value: 1, writable: true });
    await expect(recoverRuntimeState(sessionId, onePageStore(accessorPage))).rejects.toMatchObject({
      code: "EVENT_STREAM_CORRUPT",
    });
    expect(accessorCalls).toBe(0);

    const sparse = new Array<StoredEvent>(1);
    await expect(recoverRuntimeState(sessionId, onePageStore(sparse))).rejects.toMatchObject({
      code: "EVENT_STREAM_CORRUPT",
    });
  });
});

function opened(revision: bigint): StoredEvent {
  return {
    eventId: `event-${revision.toString()}`,
    sessionId,
    revision,
    type: "session_opened",
    occurredAt: "2026-01-01T00:00:00.000Z",
    payload: { type: "session_opened", sessionId },
  };
}

function onePageStore(page: readonly StoredEvent[]): RuntimeEventStorePort {
  return {
    currentRevision: () => Promise.resolve(1n),
    loadPage: (_sessionId, options) => Promise.resolve(createRuntimeEventPage(page, options.limit)),
    append: () => Promise.resolve([]),
  };
}
