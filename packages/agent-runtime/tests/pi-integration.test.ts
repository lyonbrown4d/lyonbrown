import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import {
  createPiKernelSession,
  type KernelModelAdapter,
  type KernelModelStreamEvent,
} from "@lyonbrown/kernel-pi";
import { expect, test } from "vitest";

import { createAgentRuntime } from "../src/index.js";
import { eventTypes, sequentialEventIds } from "./support.js";

const budget = {
  maxInputBytes: 4_096,
  maxTurns: 4,
  maxSteps: 4,
  maxOutputBytes: 4_096,
  maxToolResultBytes: 1_024,
  maxValueDepth: 6,
  maxValueStringBytes: 1_024,
  maxCollectionEntries: 64,
  maxValueNodes: 256,
  toolTimeoutMs: 100,
};

test("runs the real Pi kernel through the durable runtime adapter", async () => {
  const model: KernelModelAdapter = {
    model: { provider: "offline", id: "runtime-integration" },
    async *stream(): AsyncIterable<KernelModelStreamEvent> {
      await Promise.resolve();
      yield { type: "text_delta", delta: "answer" };
      yield { type: "usage", usage: { inputTokens: 2, outputTokens: 1, totalTokens: 3 } };
      yield { type: "done", reason: "stop" };
    },
  };
  const store = openEventStore({ testOnlyMemory: true });
  const runtime = await createAgentRuntime({
    sessionId: asSessionId("session-pi"),
    eventStore: store,
    createEventId: sequentialEventIds(),
    createKernelSession: ({ sessionId, eventSink }) =>
      createPiKernelSession({
        sessionId,
        eventSink,
        model,
        systemPrompt: "Answer briefly.",
        toolRegistry: { list: () => [], get: () => undefined, hasPermission: () => false },
        approvalGate: { request: () => Promise.resolve({ decision: "deny" }) },
        secretResolver: { resolve: () => Promise.reject(new Error("not configured")) },
        budget,
      }),
  });

  await runtime.startTurn("question");

  expect(runtime.snapshot()).toMatchObject({
    phase: "completed",
    turns: [{ userText: "question", assistantText: "answer", status: "completed" }],
    usage: { inputTokens: 2, outputTokens: 1, totalTokens: 3 },
  });
  expect(await eventTypes(store, "session-pi")).toEqual([
    "session_opened",
    "user_message_submitted",
    "run_started",
    "turn_started",
    "assistant_text_delta",
    "usage_recorded",
    "turn_completed",
    "run_completed",
  ]);
  runtime.dispose();
  store.close();
});
