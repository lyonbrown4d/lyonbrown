import { status } from "@grpc/grpc-js";
import {
  CancelModelDownloadRequest,
  CommandErrorCode,
  ForceStopRequest,
  RespondApprovalRequest,
  SelectSessionModelRequest,
  StartModelDownloadRequest,
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
} from "./server-test-support.ts";

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

  it("returns the matching typed Unsupported variant from every deferred RPC", async () => {
    const state = await start();
    const metadata = authorization(state.token);
    const client = state.client;
    const calls = [
      invokeUnary((callback) =>
        client.respondApproval(
          RespondApprovalRequest.create({ metadata: requestMetadata("approval") }),
          metadata,
          callback,
        ),
      ),
      invokeUnary((callback) =>
        client.selectSessionModel(
          SelectSessionModelRequest.create({ metadata: requestMetadata("model") }),
          metadata,
          callback,
        ),
      ),
      invokeUnary((callback) =>
        client.startModelDownload(
          StartModelDownloadRequest.create({ metadata: requestMetadata("download") }),
          metadata,
          callback,
        ),
      ),
      invokeUnary((callback) =>
        client.cancelModelDownload(
          CancelModelDownloadRequest.create({ metadata: requestMetadata("cancel-download") }),
          metadata,
          callback,
        ),
      ),
    ];
    const errors = await Promise.all(
      calls.map(async (call) => call.catch((error: unknown) => error)),
    );
    expect(errors.map((error) => detailFrom(error).context)).toEqual([
      {
        $case: "unsupportedCommand",
        unsupportedCommand: UnsupportedCommand.UNSUPPORTED_COMMAND_RESPOND_APPROVAL,
      },
      {
        $case: "unsupportedCommand",
        unsupportedCommand: UnsupportedCommand.UNSUPPORTED_COMMAND_SELECT_SESSION_MODEL,
      },
      {
        $case: "unsupportedCommand",
        unsupportedCommand: UnsupportedCommand.UNSUPPORTED_COMMAND_START_MODEL_DOWNLOAD,
      },
      {
        $case: "unsupportedCommand",
        unsupportedCommand: UnsupportedCommand.UNSUPPORTED_COMMAND_CANCEL_MODEL_DOWNLOAD,
      },
    ]);
  });
});
