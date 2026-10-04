import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import type { KernelEventSink, KernelSession } from "@lyonbrown/kernel-pi";
import {
  createAgentRuntime,
  createRuntimeEventStorePort,
  type KernelSessionFactoryInput,
} from "@lyonbrown/agent-runtime";
import { RuntimePhase } from "@lyonbrown/protocol";
import { expect, test } from "vitest";

import { createRuntimeHostAdapter } from "../node-src/runtime-adapter.js";

test("StartTurn persists domain events before the adapter streams the projected snapshot", async () => {
  const store = openEventStore({ testOnlyMemory: true });
  let sink: KernelEventSink | undefined;
  const createKernelSession = ({ eventSink }: KernelSessionFactoryInput): KernelSession => {
    sink = eventSink;
    return {
      async startTurn() {
        await eventSink.append({ type: "run_started", sessionId: "adapter-integration" });
        await eventSink.append({ type: "turn_started", turn: 1 });
        await eventSink.append({ type: "text_delta", delta: "answer" });
        await eventSink.append({
          type: "usage",
          usage: { inputTokens: 2, outputTokens: 1, totalTokens: 3 },
        });
        await eventSink.append({ type: "turn_completed", turn: 1 });
        await eventSink.append({ type: "run_completed", sessionId: "adapter-integration" });
      },
      steer: () => undefined,
      followUp: () => undefined,
      cancel: () => undefined,
      subscribe: () => () => undefined,
      dispose: () => undefined,
    };
  };
  const runtime = await createAgentRuntime({
    sessionId: asSessionId("adapter-integration"),
    eventStore: createRuntimeEventStorePort(store),
    createKernelSession,
    createEventId: (() => {
      let id = 0;
      return () => `adapter-event-${String(++id)}`;
    })(),
    now: () => "2026-10-04T00:00:00.000Z",
  });
  expect(sink).toBeDefined();
  const host = createRuntimeHostAdapter(runtime);
  const controller = new AbortController();
  const iterator = host.subscribe(1n, controller.signal)[Symbol.asyncIterator]();
  const firstEvent = iterator.next();

  await expect(host.execute({ kind: "start-turn", text: "question" })).resolves.toEqual({
    ok: true,
  });
  await expect(firstEvent).resolves.toMatchObject({ value: { revision: 2n } });
  const finalEvent = await iterator.next();
  expect(finalEvent).toMatchObject({
    done: false,
    value: {
      revision: 8n,
      snapshot: {
        revision: 8n,
        phase: RuntimePhase.RUNTIME_PHASE_COMPLETED,
        turns: [
          {
            userText: "question",
            assistantText: "answer",
            activities: [
              {
                usage: { input: 2, output: 1, cacheRead: 0 },
              },
            ],
          },
        ],
      },
    },
  });
  const persisted = await store.load("adapter-integration", { afterRevision: 0n, limit: 20 });
  expect(persisted.map((event) => event.type)).toEqual([
    "session_opened",
    "user_message_submitted",
    "run_started",
    "turn_started",
    "assistant_text_delta",
    "usage_recorded",
    "turn_completed",
    "run_completed",
  ]);

  controller.abort();
  await iterator.return?.();
  runtime.dispose();
  store.close();
});

test("a recovered unresolved tool rejects a new start without invoking the replacement kernel", async () => {
  const store = openEventStore({ testOnlyMemory: true });
  const eventStore = createRuntimeEventStorePort(store);
  const first = await createAgentRuntime({
    sessionId: asSessionId("adapter-recovered-terminal"),
    eventStore,
    createKernelSession: ({ eventSink }) => ({
      async startTurn() {
        await eventSink.append({
          type: "run_started",
          sessionId: "adapter-recovered-terminal",
        });
        await eventSink.append({ type: "turn_started", turn: 1 });
        await eventSink.append({
          type: "tool_started",
          callId: "call-1",
          toolName: "shell",
          argumentsPreview: { command: "build" },
        });
      },
      steer: () => undefined,
      followUp: () => undefined,
      cancel: () => undefined,
      subscribe: () => () => undefined,
      dispose: () => undefined,
    }),
  });
  await first.startTurn("build");
  first.dispose();

  let replacementStarts = 0;
  const recovered = await createAgentRuntime({
    sessionId: asSessionId("adapter-recovered-terminal"),
    eventStore,
    createKernelSession: () => ({
      startTurn: () => {
        replacementStarts += 1;
        return Promise.resolve();
      },
      steer: () => undefined,
      followUp: () => undefined,
      cancel: () => undefined,
      subscribe: () => () => undefined,
      dispose: () => undefined,
    }),
  });
  const host = createRuntimeHostAdapter(recovered);

  await expect(host.execute({ kind: "start-turn", text: "retry" })).resolves.toEqual({
    ok: false,
    error: { code: "RUNTIME_STOPPED" },
  });
  expect(replacementStarts).toBe(0);

  recovered.dispose();
  store.close();
});
