import {
  Metadata,
  credentials,
  status,
  type ClientReadableStream,
  type ServiceError,
} from "@grpc/grpc-js";
import {
  AgentServiceClient,
  AgentServiceService,
  CancelTurnRequest,
  CommandErrorCode,
  GetSnapshotRequest,
  GRPC_CHANNEL_OPTIONS,
  HandshakeRequest,
  HealthRequest,
  MAX_GRPC_MESSAGE_BYTES,
  PROTOCOL_VERSION,
  QueueFollowUpRequest,
  RequestMetadata,
  RuntimeEvent,
  StartTurnRequest,
  SteerCurrentTurnRequest,
  type HandshakeResponse,
  type RuntimeSnapshot,
} from "@lyonbrown/protocol";
import { afterAll, describe, expect, it, vi } from "vitest";
import { createAgentService } from "@lyonbrown/agentd/internal/service";
import type { RuntimeResult } from "@lyonbrown/agentd/internal/runtime-host";
import {
  FakeHost,
  SCOPE_FINGERPRINT,
  SERVER_INSTANCE_ID,
  type TestServerState,
  authorization,
  detailFrom,
  health,
  invokeUnary,
  requestMetadata,
  startTestServer,
  startTurn,
} from "@lyonbrown/agentd/test/server-test-support";

describe("agentd gRPC server", () => {
  const states: TestServerState[] = [];

  afterAll(async () => {
    await Promise.all(states.map(async (state) => state.close()));
  });

  async function start(host = new FakeHost()) {
    const state = await startTestServer(host);
    states.push(state);
    return state;
  }

  it("registers every proto RPC explicitly", () => {
    const implementation = createAgentService({
      host: new FakeHost(),
      serverInstanceId: SERVER_INSTANCE_ID,
      scopeFingerprint: SCOPE_FINGERPRINT,
    });
    expect(Object.keys(implementation).sort()).toEqual(Object.keys(AgentServiceService).sort());
  });

  it("rejects unauthenticated calls before request validation", async () => {
    const state = await start();
    const invalid = StartTurnRequest.create({ text: "" });
    await expect(startTurn(state.client, invalid, new Metadata())).rejects.toMatchObject({
      code: status.UNAUTHENTICATED,
      details: "authentication required",
    });
    expect(state.host.execute).not.toHaveBeenCalled();
  });

  it("serves authenticated health calls", async () => {
    const state = await start();
    const response = await health(
      state.client,
      HealthRequest.create({ metadata: requestMetadata("health", "") }),
      authorization(state.token),
    );
    expect(response.protocolVersion).toBe(PROTOCOL_VERSION);
  });

  it("enforces the 16 MiB receive limit at the transport boundary", async () => {
    const state = await start();
    const largeClient = new AgentServiceClient(state.address, credentials.createInsecure(), {
      ...GRPC_CHANNEL_OPTIONS,
      "grpc.max_send_message_length": MAX_GRPC_MESSAGE_BYTES + 1_024,
    });
    try {
      await expect(
        startTurn(
          largeClient,
          StartTurnRequest.create({
            metadata: requestMetadata("oversize"),
            text: "x".repeat(MAX_GRPC_MESSAGE_BYTES),
          }),
          authorization(state.token),
        ),
      ).rejects.toMatchObject({ code: status.RESOURCE_EXHAUSTED });
      expect(state.host.execute).not.toHaveBeenCalled();
    } finally {
      largeClient.close();
    }
  });

  it("routes every implemented unary RPC through the narrow runtime port", async () => {
    const state = await start();
    const metadata = authorization(state.token);
    const handshake = await invokeUnary<HandshakeResponse>((callback) =>
      state.client.handshake(
        HandshakeRequest.create({
          metadata: requestMetadata("handshake", ""),
          clientName: "test",
          clientVersion: "1.0.0",
        }),
        metadata,
        callback,
      ),
    );
    expect(handshake).toMatchObject({
      serverInstanceId: SERVER_INSTANCE_ID,
      scopeFingerprint: SCOPE_FINGERPRINT,
      capabilities: ["streaming", "runtime.test.v1"],
    });

    await invokeUnary((callback) =>
      state.client.steerCurrentTurn(
        SteerCurrentTurnRequest.create({ metadata: requestMetadata("steer"), text: "steer" }),
        metadata,
        callback,
      ),
    );
    await invokeUnary((callback) =>
      state.client.queueFollowUp(
        QueueFollowUpRequest.create({ metadata: requestMetadata("follow"), text: "follow" }),
        metadata,
        callback,
      ),
    );
    await invokeUnary((callback) =>
      state.client.cancelTurn(
        CancelTurnRequest.create({ metadata: requestMetadata("cancel") }),
        metadata,
        callback,
      ),
    );
    const snapshot = await invokeUnary<RuntimeSnapshot>((callback) =>
      state.client.getSnapshot(
        GetSnapshotRequest.create({ metadata: requestMetadata("snapshot") }),
        metadata,
        callback,
      ),
    );
    expect(snapshot.revision).toBe(1n);
    expect(state.host.execute.mock.calls.map(([command]) => command.kind)).toEqual([
      "steer-current-turn",
      "queue-follow-up",
      "cancel-turn",
    ]);
  });

  it("fences stale epochs with raw typed protobuf details", async () => {
    const state = await start();
    const call = startTurn(
      state.client,
      StartTurnRequest.create({ metadata: requestMetadata("stale", "stale-epoch"), text: "hi" }),
      authorization(state.token),
    );
    const error = await call.catch((caught: unknown) => caught);
    expect(error).toMatchObject({ code: status.FAILED_PRECONDITION });
    expect(detailFrom(error)).toMatchObject({
      code: CommandErrorCode.COMMAND_ERROR_CODE_EPOCH_MISMATCH,
      context: {
        $case: "epochMismatch",
        epochMismatch: {
          expectedServerInstanceId: "stale-epoch",
          actualServerInstanceId: SERVER_INSTANCE_ID,
        },
      },
    });
    expect(state.host.execute).not.toHaveBeenCalled();
  });

  it("coalesces identical in-flight commands and rejects conflicting reuse", async () => {
    const host = new FakeHost();
    let release = (): void => undefined;
    host.execute.mockImplementation(
      () =>
        new Promise<RuntimeResult>((resolve) => {
          release = () => {
            resolve({ ok: true });
          };
        }),
    );
    const state = await start(host);
    const metadata = authorization(state.token);
    const same = StartTurnRequest.create({ metadata: requestMetadata("replay"), text: "same" });
    const first = startTurn(state.client, same, metadata);
    const second = startTurn(state.client, same, metadata);
    await vi.waitFor(() => {
      expect(host.execute).toHaveBeenCalledOnce();
    });
    const conflict = startTurn(
      state.client,
      StartTurnRequest.create({ metadata: requestMetadata("replay"), text: "different" }),
      metadata,
    );
    await expect(conflict).rejects.toMatchObject({ code: status.ALREADY_EXISTS });
    release();
    await expect(Promise.all([first, second])).resolves.toHaveLength(2);
    expect(host.execute).toHaveBeenCalledOnce();
  });

  it("keeps NUL-containing client and command ids in distinct replay domains", async () => {
    const state = await start();
    const metadata = authorization(state.token);
    const firstMetadata = RequestMetadata.create({
      protocolVersion: PROTOCOL_VERSION,
      clientInstanceId: "a",
      commandId: "b\0c",
      expectedServerInstanceId: SERVER_INSTANCE_ID,
    });
    const secondMetadata = RequestMetadata.create({
      protocolVersion: PROTOCOL_VERSION,
      clientInstanceId: "a\0b",
      commandId: "c",
      expectedServerInstanceId: SERVER_INSTANCE_ID,
    });

    const first = await startTurn(
      state.client,
      StartTurnRequest.create({ metadata: firstMetadata, text: "first" }),
      metadata,
    );
    const second = await startTurn(
      state.client,
      StartTurnRequest.create({ metadata: secondMetadata, text: "second" }),
      metadata,
    );

    expect([first.commandId, second.commandId]).toEqual(["b\0c", "c"]);
    expect(state.host.execute).toHaveBeenCalledTimes(2);
  });

  it("cancels the host subscription when the client cancels", async () => {
    let aborted = false;
    const host = new FakeHost();
    host.subscribe = async function* (_afterRevision, signal): AsyncIterable<RuntimeEvent> {
      try {
        yield RuntimeEvent.create({ revision: 2n, snapshot: this.snapshot() });
        await new Promise<void>((resolve) => {
          signal.addEventListener("abort", () => {
            resolve();
          });
        });
      } finally {
        aborted = signal.aborted;
      }
    };
    const state = await start(host);
    const stream: ClientReadableStream<RuntimeEvent> = state.client.subscribeEvents(
      {
        $type: "lyonbrown.agent.v1.SubscribeEventsRequest",
        metadata: requestMetadata("stream"),
        afterRevision: 1n,
      },
      authorization(state.token),
    );
    await new Promise<void>((resolve, reject) => {
      stream.once("data", () => {
        stream.cancel();
        resolve();
      });
      stream.once("error", (error: ServiceError) => {
        if (error.code !== status.CANCELLED) reject(error);
      });
    });
    await vi.waitFor(() => {
      expect(aborted).toBe(true);
    });
  });
});
