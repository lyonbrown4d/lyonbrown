import { expect, test } from "vitest";

import { createRuntimeHostAdapter } from "@lyonbrown/agentd/internal/runtime-adapter";
import {
  FakeAgentRuntime,
  openedState,
  stateFrom,
} from "@lyonbrown/agentd/test/runtime-adapter-support";

test("streams only revisions after the cursor with matching outer and inner revisions", async () => {
  const initial = openedState();
  const runtime = new FakeAgentRuntime(initial);
  const host = createRuntimeHostAdapter(runtime);
  const controller = new AbortController();
  const iterator = host.subscribe(initial.revision, controller.signal)[Symbol.asyncIterator]();
  const pending = iterator.next();
  const next = stateFrom([
    { type: "session_opened" },
    { type: "user_message_submitted", text: "question" },
  ]);
  runtime.feed.publish(next);

  const event = await pending;
  expect(event).toMatchObject({
    done: false,
    value: { revision: 2n, snapshot: { revision: 2n } },
  });
  controller.abort();
  await iterator.return?.();
  expect(runtime.feed.closeCalls).toBe(1);
});

test("preserves latest-value capacity one while the consumer applies backpressure", async () => {
  const runtime = new FakeAgentRuntime(openedState());
  const host = createRuntimeHostAdapter(runtime);
  const controller = new AbortController();
  const iterator = host.subscribe(0n, controller.signal)[Symbol.asyncIterator]();

  await expect(iterator.next()).resolves.toMatchObject({ value: { revision: 1n } });
  runtime.feed.publish(
    stateFrom([{ type: "session_opened" }, { type: "user_message_submitted", text: "one" }]),
  );
  runtime.feed.publish(
    stateFrom([
      { type: "session_opened" },
      { type: "user_message_submitted", text: "one" },
      { type: "run_started" },
    ]),
  );

  await expect(iterator.next()).resolves.toMatchObject({ value: { revision: 3n } });
  controller.abort();
  await iterator.return?.();
});

test("does not republish a duplicate revision from the runtime feed", async () => {
  const initial = openedState();
  const runtime = new FakeAgentRuntime(initial);
  const host = createRuntimeHostAdapter(runtime);
  const controller = new AbortController();
  const iterator = host.subscribe(0n, controller.signal)[Symbol.asyncIterator]();

  await expect(iterator.next()).resolves.toMatchObject({ value: { revision: 1n } });
  runtime.feed.publish(initial);
  const pending = iterator.next();
  await Promise.resolve();
  runtime.feed.publish(
    stateFrom([{ type: "session_opened" }, { type: "user_message_submitted", text: "question" }]),
  );

  await expect(pending).resolves.toMatchObject({ value: { revision: 2n } });
  controller.abort();
  await iterator.return?.();
});

test("a pre-aborted subscription closes its runtime feed without reading", async () => {
  const runtime = new FakeAgentRuntime(openedState());
  const host = createRuntimeHostAdapter(runtime);
  const controller = new AbortController();
  controller.abort();
  const iterator = host.subscribe(0n, controller.signal)[Symbol.asyncIterator]();

  await expect(iterator.next()).resolves.toEqual({ value: undefined, done: true });
  expect(runtime.feed.nextCalls).toBe(0);
  expect(runtime.feed.closeCalls).toBe(1);
});

test("rejects an invalid revision cursor before opening a runtime feed", async () => {
  const runtime = new FakeAgentRuntime(openedState());
  const host = createRuntimeHostAdapter(runtime);
  const iterator = host.subscribe(-1n, new AbortController().signal)[Symbol.asyncIterator]();

  await expect(iterator.next()).rejects.toThrow("runtime snapshot cannot be projected");
  expect(runtime.feed.nextCalls).toBe(0);
});
