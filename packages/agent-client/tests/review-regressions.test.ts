import { Metadata, status, type ServerWritableStream, type ServiceError } from "@grpc/grpc-js";
import {
  ModelSelection,
  PROTOCOL_VERSION,
  RuntimeEvent,
  RuntimePhase,
  RuntimeSnapshot,
  type SubscribeEventsRequest,
} from "@lyonbrown/protocol";
import { afterAll, describe, expect, it } from "vitest";

import { AgentClientError } from "@lyonbrown/agent-client/internal/index";
import {
  bindingFor,
  clientFor,
  closeDaemons,
  spawnDaemon,
} from "@lyonbrown/agent-client/test/support/fake-daemon";

const OTHER_SCOPE = "b".repeat(64);

afterAll(async () => {
  await closeDaemons();
});

function serviceFailure(code: status): ServiceError {
  return Object.assign(new Error("private grpc failure"), {
    code,
    details: "private grpc failure",
    metadata: new Metadata(),
  }) as ServiceError;
}

function snapshot(revision = 1n): RuntimeSnapshot {
  return RuntimeSnapshot.fromPartial({
    protocolVersion: PROTOCOL_VERSION,
    revision,
    phase: RuntimePhase.RUNTIME_PHASE_READY,
    modelSelection: ModelSelection.fromPartial({ selection: { $case: "auto", auto: true } }),
  });
}

async function expectSanitizedTransportFailure(
  operation: () => Promise<unknown>,
  expected: { readonly kind: string; readonly code: string },
): Promise<void> {
  try {
    await operation();
    expect.fail("operation must fail");
  } catch (error: unknown) {
    expect(error).toBeInstanceOf(AgentClientError);
    expect(error).toMatchObject(expected);
    expect(String(error)).not.toContain("private grpc failure");
    expect(error).not.toHaveProperty("serviceError");
    expect(error).not.toHaveProperty("cause");
    expect(error).not.toHaveProperty("details.grpcCode");
  }
}

describe("review regression: unary errors", () => {
  it("sanitizes health transport failures", async () => {
    const daemon = await spawnDaemon({
      health: (_call, callback) => {
        callback(serviceFailure(status.UNAVAILABLE));
      },
    });
    const client = clientFor(daemon);
    await expectSanitizedTransportFailure(async () => client.health(), {
      kind: "unavailable",
      code: "DISCONNECTED",
    });
    await client.close();
  });

  it("sanitizes handshake transport failures", async () => {
    const daemon = await spawnDaemon({
      handshake: (_call, callback) => {
        callback(serviceFailure(status.INTERNAL));
      },
    });
    const client = clientFor(daemon);
    await expectSanitizedTransportFailure(
      async () => client.handshake({ clientName: "tui", clientVersion: "0.1.0" }),
      { kind: "protocol", code: "UNSUPPORTED" },
    );
    await client.close();
  });

  it("sanitizes getSnapshot transport failures", async () => {
    const daemon = await spawnDaemon({
      getSnapshot: (_call, callback) => {
        callback(serviceFailure(status.DEADLINE_EXCEEDED));
      },
    });
    const client = clientFor(daemon);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });
    await expectSanitizedTransportFailure(async () => client.getSnapshot(), {
      kind: "unavailable",
      code: "TIMEOUT",
    });
    await client.close();
  });
});

describe("review regression: binding scope and generation", () => {
  it("rejects rebinding the client to a different scope", async () => {
    const first = await spawnDaemon();
    const second = await spawnDaemon({ scopeId: OTHER_SCOPE });
    const client = clientFor(first);

    expect(() => {
      client.replaceBinding(bindingFor(second, OTHER_SCOPE));
    }).toThrow(expect.objectContaining({ kind: "protocol", code: "SCOPE_MISMATCH" }));
    await client.close();
  });

  it("rejects a late unary result after same-epoch binding replacement", async () => {
    let release: (() => void) | undefined;
    let markStarted: (() => void) | undefined;
    const started = new Promise<void>((resolve) => {
      markStarted = resolve;
    });
    const serverInstanceId = crypto.randomUUID();
    const first = await spawnDaemon({
      serverInstanceId,
      getSnapshot: (_call, callback) => {
        markStarted?.();
        release = () => {
          callback(null, snapshot());
        };
      },
    });
    const second = await spawnDaemon({ serverInstanceId });
    const client = clientFor(first);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });

    const pending = client.getSnapshot();
    await started;
    client.replaceBinding(bindingFor(second));
    release?.();

    await expect(pending).rejects.toMatchObject({ code: "STALE_SERVER_EPOCH" });
    await client.close();
  });

  it("rejects a late command receipt after same-epoch binding replacement", async () => {
    let release: (() => void) | undefined;
    let markStarted: (() => void) | undefined;
    const started = new Promise<void>((resolve) => {
      markStarted = resolve;
    });
    const serverInstanceId = crypto.randomUUID();
    const first = await spawnDaemon({
      serverInstanceId,
      startTurn: (call, callback) => {
        markStarted?.();
        release = () => {
          callback(null, {
            $type: "lyonbrown.agent.v1.CommandResponse",
            protocolVersion: PROTOCOL_VERSION,
            commandId: call.request.metadata?.commandId ?? "",
            accepted: true,
          });
        };
      },
    });
    const second = await spawnDaemon({ serverInstanceId });
    const client = clientFor(first);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });

    const pending = client.enqueue({ type: "start-turn", text: "late" });
    await started;
    client.replaceBinding(bindingFor(second));
    release?.();

    await expect(pending).rejects.toMatchObject({ code: "STALE_SERVER_EPOCH" });
    await client.close();
  });

  it("rejects a late stream event after same-epoch binding replacement", async () => {
    let streamCall: ServerWritableStream<SubscribeEventsRequest, RuntimeEvent> | undefined;
    let markSubscribed: (() => void) | undefined;
    const subscribed = new Promise<void>((resolve) => {
      markSubscribed = resolve;
    });
    const serverInstanceId = crypto.randomUUID();
    const first = await spawnDaemon({
      serverInstanceId,
      subscribe: (call) => {
        streamCall = call;
        markSubscribed?.();
      },
    });
    const second = await spawnDaemon({ serverInstanceId });
    const client = clientFor(first);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });
    const iterator = client.subscribeEvents({ afterRevision: 1n })[Symbol.asyncIterator]();
    const pending = iterator.next();
    await subscribed;

    client.replaceBinding(bindingFor(second));
    streamCall?.write(
      RuntimeEvent.fromPartial({
        protocolVersion: PROTOCOL_VERSION,
        revision: 2n,
        snapshot: snapshot(2n),
      }),
    );

    await expect(pending).rejects.toMatchObject({ code: "STALE_SERVER_EPOCH" });
    await client.close();
  });
});

describe("review regression: eager stream cancellation", () => {
  it("cancels a real grpc stream when aborted before first next", async () => {
    let observeCancellation: (() => void) | undefined;
    const cancellationObserved = new Promise<void>((resolve) => {
      observeCancellation = resolve;
    });
    let markSubscribed: (() => void) | undefined;
    const subscribed = new Promise<void>((resolve) => {
      markSubscribed = resolve;
    });
    const daemon = await spawnDaemon({
      subscribe: (call) => {
        markSubscribed?.();
        call.on("cancelled", () => observeCancellation?.());
      },
    });
    const client = clientFor(daemon);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });
    const controller = new AbortController();

    client.subscribeEvents({ afterRevision: 1n, signal: controller.signal });
    await subscribed;
    controller.abort();

    await expect(
      Promise.race([
        cancellationObserved.then(() => "cancelled"),
        new Promise<string>((resolve) =>
          setTimeout(() => {
            resolve("timeout");
          }, 250),
        ),
      ]),
    ).resolves.toBe("cancelled");
    await client.close();
  });
});
