import { expect, test } from "vitest";
import {
  workerMessageSchema,
  workerRequestSchema,
} from "@lyonbrown/agent-desktop/internal/main/worker-wire";
import { disconnectedState } from "@lyonbrown/agent-desktop/internal/main/projection";
test("allows only bounded structured worker messages without transport secrets", () => {
  expect(workerMessageSchema.parse({ type: "state", state: disconnectedState() }).type).toBe(
    "state",
  );
  expect(
    workerMessageSchema.safeParse({
      type: "state",
      state: { ...disconnectedState(), token: "secret" },
    }).success,
  ).toBe(false);
  expect(
    workerMessageSchema.safeParse({ type: "reply", id: "1", result: { ok: true, error: null } })
      .success,
  ).toBe(true);
  expect(workerRequestSchema.safeParse({ type: "connect", id: "1" }).success).toBe(true);
  expect(workerRequestSchema.safeParse({ type: "eval", id: "1" }).success).toBe(false);
});
