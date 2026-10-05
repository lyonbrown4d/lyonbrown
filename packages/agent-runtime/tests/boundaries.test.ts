import {
  asRevision,
  asSessionId,
  createInitialAgentRuntimeState,
  reduceAgentRuntimeEvent,
} from "@lyonbrown/agent-domain";
import type { StoredEvent } from "@lyonbrown/event-store";
import type { KernelEvent } from "@lyonbrown/kernel-pi";
import { describe, expect, test } from "vitest";

import { mapKernelEvent } from "../src/kernel-events.ts";
import { createRuntimeEventPage, type RuntimeEventStorePort } from "../src/event-store-port.ts";
import { recoverRuntimeState } from "../src/recovery.ts";
import { LatestSnapshotFeed } from "../src/snapshot-feed.ts";

const sessionId = asSessionId("session-boundary");

describe("kernel event mapping", () => {
  test.each<[KernelEvent, string]>([
    [{ type: "run_started", sessionId }, "run_started"],
    [{ type: "run_completed", sessionId }, "run_completed"],
    [{ type: "run_cancelled", sessionId }, "run_cancelled"],
    [{ type: "run_failed", sessionId, code: "FAILED", message: "failed" }, "run_failed"],
    [{ type: "turn_started", turn: 1 }, "turn_started"],
    [{ type: "turn_completed", turn: 1 }, "turn_completed"],
    [{ type: "text_delta", delta: "text" }, "assistant_text_delta"],
    [
      { type: "usage", usage: { inputTokens: 1, outputTokens: 2, totalTokens: 3 } },
      "usage_recorded",
    ],
    [
      {
        type: "tool_started",
        callId: "call-1",
        toolName: "read_file",
        argumentsPreview: { path: "README.md" },
      },
      "tool_started",
    ],
    [
      {
        type: "tool_succeeded",
        callId: "call-1",
        toolName: "read_file",
        content: "ok",
      },
      "tool_succeeded",
    ],
    [
      {
        type: "tool_succeeded",
        callId: "call-1",
        toolName: "read_file",
        content: "ok",
        details: { lines: 1 },
      },
      "tool_succeeded",
    ],
    [
      {
        type: "tool_failed",
        callId: "call-1",
        toolName: "read_file",
        code: "FAILED",
        message: "failed",
      },
      "tool_failed",
    ],
    [
      {
        type: "tool_outcome_unknown",
        callId: "call-1",
        toolName: "write_file",
        code: "UNKNOWN",
        message: "unknown",
      },
      "tool_outcome_unknown",
    ],
    [{ type: "runtime_error", code: "FAILED", message: "failed" }, "runtime_error"],
  ])("maps $expected without leaking transport types", (event, expected) => {
    expect(mapKernelEvent(sessionId, event)).toMatchObject({ type: expected, sessionId });
  });
});

describe("latest snapshot feed", () => {
  test("delivers a waiting reader and rejects a second concurrent read", async () => {
    const initial = createInitialAgentRuntimeState(sessionId);
    const feed = new LatestSnapshotFeed(initial);
    expect(feed[Symbol.asyncIterator]()).toBe(feed);
    await expect(feed.next()).resolves.toEqual({ value: initial, done: false });
    const waiting = feed.next();
    await expect(feed.next()).rejects.toMatchObject({ code: "SESSION_BUSY" });
    const opened = reduceAgentRuntimeEvent(initial, {
      revision: asRevision(1n),
      event: { type: "session_opened", sessionId },
    });
    feed.publish(opened);
    await expect(waiting).resolves.toEqual({ value: opened, done: false });
  });

  test("close is idempotent, releases a waiter, and ignores later publishes", async () => {
    let closes = 0;
    const initial = createInitialAgentRuntimeState(sessionId);
    const feed = new LatestSnapshotFeed(initial, () => {
      closes += 1;
    });
    await feed.next();
    const waiting = feed.next();
    feed.close();
    feed.close();
    feed.publish(initial);

    await expect(waiting).resolves.toEqual({ value: undefined, done: true });
    await expect(feed.next()).resolves.toEqual({ value: undefined, done: true });
    expect(closes).toBe(1);
  });

  test("return and throw both close the iterator exactly once", async () => {
    let closes = 0;
    const feed = new LatestSnapshotFeed(createInitialAgentRuntimeState(sessionId), () => {
      closes += 1;
    });
    await expect(feed.return()).resolves.toEqual({ value: undefined, done: true });
    await expect(feed.throw(new Error("consumer stopped"))).rejects.toThrow("consumer stopped");
    await expect(feed.next()).resolves.toEqual({ value: undefined, done: true });
    expect(closes).toBe(1);
  });
});

describe("recovery boundary", () => {
  test("fails closed when revision lookup or page loading fails", async () => {
    await expect(recoverRuntimeState(sessionId, throwingStore())).rejects.toMatchObject({
      code: "EVENT_STREAM_CORRUPT",
    });
    await expect(
      recoverRuntimeState(sessionId, {
        currentRevision: () => Promise.resolve(1n),
        loadPage: (_sessionId, options) =>
          Promise.resolve(createRuntimeEventPage([], options.limit)),
        append: () => Promise.resolve([]),
      }),
    ).rejects.toMatchObject({ code: "EVENT_STREAM_CORRUPT" });
  });

  test("rejects mismatched durable envelopes", async () => {
    const event: StoredEvent = {
      eventId: "event-1",
      sessionId,
      revision: 1n,
      type: "wrong_type",
      occurredAt: "2026-01-01T00:00:00.000Z",
      payload: { type: "session_opened", sessionId },
    };
    await expect(
      recoverRuntimeState(sessionId, {
        currentRevision: () => Promise.resolve(1n),
        loadPage: (_sessionId, options) =>
          Promise.resolve(createRuntimeEventPage([event], options.limit)),
        append: () => Promise.resolve([]),
      }),
    ).rejects.toMatchObject({ code: "EVENT_STREAM_CORRUPT" });
  });

  test("contains hostile page access behind the corrupt-stream error", async () => {
    const hostile = new Proxy([] as StoredEvent[], {
      get: () => {
        throw new Error("raw hostile page detail");
      },
    });
    await expect(
      recoverRuntimeState(sessionId, {
        currentRevision: () => Promise.resolve(1n),
        loadPage: (_sessionId, options) =>
          Promise.resolve(createRuntimeEventPage(hostile, options.limit)),
        append: () => Promise.resolve([]),
      }),
    ).rejects.toMatchObject({
      code: "EVENT_STREAM_CORRUPT",
      message: "Durable runtime event stream is invalid",
    });
  });
});

function throwingStore(): RuntimeEventStorePort {
  return {
    currentRevision: () => Promise.reject(new Error("raw storage detail")),
    loadPage: () => Promise.reject(new Error("raw storage detail")),
    append: () => Promise.reject(new Error("raw storage detail")),
  };
}
