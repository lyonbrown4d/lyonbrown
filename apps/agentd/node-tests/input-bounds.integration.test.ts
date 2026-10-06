import { status } from "@grpc/grpc-js";
import {
  QueueFollowUpRequest,
  StartTurnRequest,
  SteerCurrentTurnRequest,
} from "@lyonbrown/protocol";
import { afterAll, describe, expect, it } from "vitest";

import { MAX_MESSAGE_INPUT_BYTES } from "../node-src/runtime-limits.ts";
import {
  authorization,
  invokeUnary,
  requestMetadata,
  startTestServer,
  type TestServerState,
} from "./server-test-support.ts";

describe("runtime command input boundaries", () => {
  const states: TestServerState[] = [];
  afterAll(async () => Promise.all(states.map(async (state) => state.close())));

  it("accepts the local-model message boundary and rejects one byte beyond it", async () => {
    const state = await startTestServer();
    states.push(state);
    const metadata = authorization(state.token);
    const boundary = "x".repeat(MAX_MESSAGE_INPUT_BYTES);
    const oversized = `${boundary}x`;
    const commands = [
      {
        invoke: (text: string, id: string) =>
          invokeUnary((callback) =>
            state.client.startTurn(
              StartTurnRequest.create({ metadata: requestMetadata(id), text }),
              metadata,
              callback,
            ),
          ),
      },
      {
        invoke: (text: string, id: string) =>
          invokeUnary((callback) =>
            state.client.steerCurrentTurn(
              SteerCurrentTurnRequest.create({ metadata: requestMetadata(id), text }),
              metadata,
              callback,
            ),
          ),
      },
      {
        invoke: (text: string, id: string) =>
          invokeUnary((callback) =>
            state.client.queueFollowUp(
              QueueFollowUpRequest.create({ metadata: requestMetadata(id), text }),
              metadata,
              callback,
            ),
          ),
      },
    ];

    for (const [index, command] of commands.entries()) {
      await expect(command.invoke(boundary, `boundary-${String(index)}`)).resolves.toMatchObject({
        accepted: true,
      });
      await expect(command.invoke(oversized, `oversized-${String(index)}`)).rejects.toMatchObject({
        code: status.INVALID_ARGUMENT,
        details: `text exceeds ${String(MAX_MESSAGE_INPUT_BYTES)} bytes`,
      });
    }
    expect(state.host.execute).toHaveBeenCalledTimes(3);
  });

  it("returns a stable RESOURCE_EXHAUSTED status for bounded host queues", async () => {
    const state = await startTestServer();
    states.push(state);
    state.host.execute.mockResolvedValueOnce({
      ok: false,
      error: { code: "RESOURCE_EXHAUSTED" },
    });

    await expect(
      invokeUnary((callback) =>
        state.client.queueFollowUp(
          QueueFollowUpRequest.create({
            metadata: requestMetadata("queue-capacity"),
            text: "queued",
          }),
          authorization(state.token),
          callback,
        ),
      ),
    ).rejects.toMatchObject({
      code: status.RESOURCE_EXHAUSTED,
      details: "runtime queue capacity exceeded",
    });
  });
});
