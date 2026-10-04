import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, test } from "vitest";

import { createAgentRuntime } from "../src/index.js";
import { runtimeEventStore, sequentialEventIds } from "./support.js";

const RAW_CANCEL_SECRET = "raw cancel secret";

const cancelFailureCases = [
  {
    name: "Error with cause",
    create: () => ({
      failure: new Error(RAW_CANCEL_SECRET, { cause: new Error("nested secret") }),
      accesses: () => 0,
    }),
  },
  {
    name: "hostile proxy",
    create: () => {
      let accesses = 0;
      const failure = new Proxy(
        {},
        {
          get: () => {
            accesses += 1;
            throw new Error(RAW_CANCEL_SECRET);
          },
          getPrototypeOf: () => {
            accesses += 1;
            throw new Error(RAW_CANCEL_SECRET);
          },
          has: () => {
            accesses += 1;
            throw new Error(RAW_CANCEL_SECRET);
          },
          ownKeys: () => {
            accesses += 1;
            throw new Error(RAW_CANCEL_SECRET);
          },
        },
      );
      return { failure, accesses: () => accesses };
    },
  },
] as const;

describe("agent runtime cancellation boundary", () => {
  test.each(cancelFailureCases)("sanitizes a kernel cancel $name", async ({ create }) => {
    const store = openEventStore({ testOnlyMemory: true });
    const { failure, accesses } = create();
    const runtime = await createAgentRuntime({
      sessionId: asSessionId("session-cancel-failure"),
      eventStore: runtimeEventStore(store),
      createEventId: sequentialEventIds(),
      createKernelSession: () => ({
        startTurn: () => Promise.resolve(),
        steer: () => undefined,
        followUp: () => undefined,
        cancel: () => {
          // eslint-disable-next-line @typescript-eslint/only-throw-error -- hostile adapters can throw unknown values.
          throw failure;
        },
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });
    let caught: unknown;
    try {
      runtime.cancel();
    } catch (error) {
      caught = error;
    }
    await runtime.dispose();
    store.close();

    expect(caught === failure).toBe(false);
    expect(accesses()).toBe(0);
    expect(caught).toMatchObject({
      name: "AgentRuntimeError",
      code: "KERNEL_FAILURE",
      message: "Kernel session failed",
    });
    expect(caught).not.toHaveProperty("cause");
    expect(String(caught)).not.toContain(RAW_CANCEL_SECRET);
  });
});
