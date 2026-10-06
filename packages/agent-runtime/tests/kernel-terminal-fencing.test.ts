import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, test } from "vitest";

import { createAgentRuntime } from "@lyonbrown/agent-runtime/internal/index";
import {
  eventTypes,
  runtimeEventStore,
  sequentialEventIds,
} from "@lyonbrown/agent-runtime/test/support";

describe("kernel terminal event fencing", () => {
  test("keeps a supervised steering failure terminal when the old kernel completes late", async () => {
    const store = openEventStore({ testOnlyMemory: true });
    const sessionId = asSessionId("session-steering-failure-terminal");
    let releaseKernel: (() => void) | undefined;
    let completionSettled: (() => void) | undefined;
    let completionRejected = false;
    const lateCompletionSettled = new Promise<void>((resolve) => {
      completionSettled = resolve;
    });
    const runtime = await createAgentRuntime({
      sessionId,
      eventStore: runtimeEventStore(store),
      createEventId: sequentialEventIds(),
      createKernelSession: ({ eventSink }) => ({
        async startTurn() {
          await eventSink.append({ type: "run_started", sessionId });
          await new Promise<void>((resolve) => {
            releaseKernel = resolve;
          });
          try {
            await eventSink.append({ type: "run_completed", sessionId });
          } catch {
            completionRejected = true;
          } finally {
            completionSettled?.();
          }
        },
        steer: () => {
          throw new Error("raw steering delivery secret");
        },
        followUp: () => undefined,
        cancel: () => undefined,
        subscribe: () => () => undefined,
        dispose: () => undefined,
      }),
    });

    await runtime.startTurn("question");
    await expect.poll(() => runtime.snapshot().phase).toBe("running");
    await runtime.steer("focus");
    await expect.poll(() => runtime.snapshot().phase).toBe("failed");
    const failedRevision = runtime.snapshot().revision;

    releaseKernel?.();
    await lateCompletionSettled;

    expect(completionRejected).toBe(false);
    expect(runtime.snapshot()).toMatchObject({
      revision: failedRevision,
      phase: "failed",
      turns: [{ status: "failed" }],
      lastError: { code: "KERNEL_FAILURE", message: "Kernel session failed" },
    });
    expect(await eventTypes(store, sessionId)).not.toContain("run_completed");
    await runtime.dispose();
    store.close();
  });
});
