import { asSessionId } from "@lyonbrown/agent-domain";
import { openEventStore } from "@lyonbrown/event-store";
import { describe, expect, test } from "vitest";

import { createAgentRuntime } from "../src/index.ts";
import { eventTypes, runtimeEventStore } from "./support.ts";

describe("runtime event writer local failures", () => {
  test.each(["event id", "clock"] as const)(
    "sanitizes a failing %s after validation",
    async (kind) => {
      const store = openEventStore({ testOnlyMemory: true });
      const sessionId = asSessionId(`session-failing-${kind.replace(" ", "-")}`);
      let idCalls = 0;
      let clockCalls = 0;
      const runtime = await createAgentRuntime({
        sessionId,
        eventStore: runtimeEventStore(store),
        createEventId: () => {
          idCalls += 1;
          if (kind === "event id" && idCalls > 1) throw new Error("raw id secret");
          return `event-${String(idCalls)}`;
        },
        now: () => {
          clockCalls += 1;
          if (kind === "clock" && clockCalls > 1) throw new Error("raw clock secret");
          return "2026-01-01T00:00:00.000Z";
        },
        createKernelSession: () => ({
          startTurn: () => Promise.resolve(),
          steer: () => undefined,
          followUp: () => undefined,
          cancel: () => undefined,
          subscribe: () => () => undefined,
          dispose: () => undefined,
        }),
      });

      await expect(runtime.startTurn("question")).rejects.toMatchObject({
        code: "EVENT_SINK_UNAVAILABLE",
        message: "Durable runtime event sink unavailable",
      });
      expect(await eventTypes(store, sessionId)).toEqual(["session_opened"]);
      await runtime.dispose();
      store.close();
    },
  );
});
