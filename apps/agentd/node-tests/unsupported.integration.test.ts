import { status } from "@grpc/grpc-js";
import {
  ApprovalDecision,
  CommandErrorCode,
  ForceStopRequest,
  ModelSelection,
  RespondApprovalRequest,
  SelectSessionModelRequest,
  UnsupportedCommand,
} from "@lyonbrown/protocol";
import { afterAll, describe, expect, it } from "vitest";
import {
  type TestServerState,
  authorization,
  detailFrom,
  forceStop,
  invokeUnary,
  requestMetadata,
  startTestServer,
} from "@lyonbrown/agentd/test/server-test-support";

describe("deferred command RPCs", () => {
  const states: TestServerState[] = [];

  afterAll(async () => {
    await Promise.all(states.map(async (state) => state.close()));
  });

  async function start(): Promise<TestServerState> {
    const state = await startTestServer();
    states.push(state);
    return state;
  }

  it("returns typed Unsupported for ForceStop without reaching RuntimeHost", async () => {
    const state = await start();
    const error = await forceStop(
      state.client,
      ForceStopRequest.create({ metadata: requestMetadata("unsupported") }),
      authorization(state.token),
    ).catch((caught: unknown) => caught);
    expect(error).toMatchObject({ code: status.FAILED_PRECONDITION });
    expect(detailFrom(error)).toMatchObject({
      code: CommandErrorCode.COMMAND_ERROR_CODE_UNSUPPORTED,
      context: {
        $case: "unsupportedCommand",
        unsupportedCommand: UnsupportedCommand.UNSUPPORTED_COMMAND_FORCE_STOP,
      },
    });
    expect(state.host.execute).not.toHaveBeenCalled();
  });

  it("forwards approval responses and model selection to RuntimeHost", async () => {
    const state = await start();
    const metadata = authorization(state.token);
    const client = state.client;
    await expect(
      invokeUnary((callback) =>
        client.respondApproval(
          RespondApprovalRequest.create({
            metadata: requestMetadata("approval"),
            requestId: "call-1",
            decision: ApprovalDecision.APPROVAL_DECISION_ALLOW_ONCE,
          }),
          metadata,
          callback,
        ),
      ),
    ).resolves.toMatchObject({ accepted: true });
    expect(state.host.execute).toHaveBeenCalledWith({
      kind: "respond-approval",
      requestId: "call-1",
      decision: "allow-once",
    });

    await expect(
      invokeUnary((callback) =>
        client.selectSessionModel(
          SelectSessionModelRequest.create({
            metadata: requestMetadata("model"),
            selection: ModelSelection.create({ selection: { $case: "auto", auto: true } }),
          }),
          metadata,
          callback,
        ),
      ),
    ).resolves.toMatchObject({ accepted: true });
    expect(state.host.execute).toHaveBeenCalledWith({
      kind: "select-session-model",
      selection: { kind: "auto" },
    });
  });
});
