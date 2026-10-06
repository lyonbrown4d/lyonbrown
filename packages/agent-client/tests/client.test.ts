import { status } from "@grpc/grpc-js";
import {
  ModelSelection,
  PROTOCOL_VERSION,
  RuntimeEvent,
  RuntimePhase,
  RuntimeSnapshot,
} from "@lyonbrown/protocol";
import { afterAll, describe, expect, it } from "vitest";

import type { AgentCommand } from "@lyonbrown/agent-client/internal/index";
import {
  bindingFor,
  clientFor,
  closeDaemons,
  spawnDaemon,
} from "@lyonbrown/agent-client/test/support/fake-daemon";

const SCOPE = "a".repeat(64);

afterAll(async () => {
  await closeDaemons();
});

describe("agent client authenticated transport", () => {
  it("handshakes, validates scope, and sends bearer metadata on every call", async () => {
    const daemon = await spawnDaemon();
    const client = clientFor(daemon);

    const session = await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });
    await client.health();
    await client.getSnapshot();
    for await (const event of client.subscribeEvents({ afterRevision: 1n })) {
      expect(event).toBeUndefined();
    }
    const receipt = await client.enqueue({ type: "start-turn", text: "hello" });

    expect(session).toEqual({
      protocolVersion: PROTOCOL_VERSION,
      serverVersion: "test",
      serverInstanceId: daemon.serverInstanceId,
      capabilities: ["events"],
      scopeId: SCOPE,
    });
    expect(receipt.accepted).toBe(true);
    expect(daemon.calls).toHaveLength(1);
    expect(daemon.calls[0]?.authorization).toBe(`Bearer ${daemon.token}`);
    expect(daemon.calls[0]?.expectedServerInstanceId).toBe(daemon.serverInstanceId);
    expect(daemon.authorizationHeaders).toEqual(Array(4).fill(`Bearer ${daemon.token}`));
    await client.close();
  });

  it("rejects a handshake whose scope does not match the discovery binding", async () => {
    const daemon = await spawnDaemon({ scopeId: "b".repeat(64) });
    const client = clientFor(daemon);

    await expect(
      client.handshake({ clientName: "tui", clientVersion: "0.1.0" }),
    ).rejects.toMatchObject({ code: "SCOPE_MISMATCH", kind: "protocol" });
    await client.close();
  });
});

describe("command correlation and epoch fencing", () => {
  it("maps every Lyonbrown command without exposing wire enums", async () => {
    const daemon = await spawnDaemon();
    const client = clientFor(daemon);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });
    const commands: readonly AgentCommand[] = [
      { type: "start-turn", text: "start" },
      { type: "steer-current-turn", text: "steer" },
      { type: "queue-follow-up", text: "follow" },
      { type: "cancel-turn" },
      { type: "force-stop" },
      { type: "respond-approval", requestId: "approval-1", decision: "allow-once" },
      { type: "respond-approval", requestId: "approval-2", decision: "allow-for-session" },
      { type: "respond-approval", requestId: "approval-3", decision: "deny-once" },
      { type: "respond-approval", requestId: "approval-4", decision: "deny-remaining" },
      { type: "select-session-model", selection: { kind: "auto" } },
      {
        type: "select-session-model",
        selection: { kind: "pinned", provider: "local", model: "qwen" },
      },
      { type: "start-model-download", modelId: "qwen" },
      { type: "cancel-model-download", operationId: "download-1" },
    ];

    for (const command of commands) await client.enqueue(command);

    expect(daemon.commands).toEqual([
      "start-turn",
      "steer-current-turn",
      "queue-follow-up",
      "cancel-turn",
      "force-stop",
      "respond-approval",
      "respond-approval",
      "respond-approval",
      "respond-approval",
      "select-session-model",
      "select-session-model",
      "start-model-download",
      "cancel-model-download",
    ]);
    expect(daemon.calls.every((call) => call.authorization === `Bearer ${daemon.token}`)).toBe(
      true,
    );
    await client.close();
  });

  it("reuses commandId and epoch for a retryable transport failure", async () => {
    let attempts = 0;
    const daemon = await spawnDaemon({
      startTurn: (call, callback) => {
        attempts += 1;
        if (attempts === 1) {
          callback({ code: status.UNAVAILABLE, message: "retry" } as never);
          return;
        }
        callback(null, {
          $type: "lyonbrown.agent.v1.CommandResponse",
          protocolVersion: PROTOCOL_VERSION,
          commandId: call.request.metadata?.commandId ?? "",
          accepted: true,
        });
      },
    });
    const client = clientFor(daemon);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });

    await client.enqueue({ type: "start-turn", text: "retry" });

    expect(daemon.calls).toHaveLength(2);
    expect(daemon.calls[0]?.commandId).toBe(daemon.calls[1]?.commandId);
    expect(daemon.calls[0]?.expectedServerInstanceId).toBe(
      daemon.calls[1]?.expectedServerInstanceId,
    );
    await client.close();
  });

  it("fails closed when the binding changes before retrying a non-idempotent command", async () => {
    let releaseFirst: (() => void) | undefined;
    const firstFailed = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const epochA = await spawnDaemon({
      startTurn: (_call, callback) => {
        callback({ code: status.UNAVAILABLE, message: "restart" } as never);
        releaseFirst?.();
      },
    });
    const epochB = await spawnDaemon();
    const client = clientFor(epochA);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });

    const pending = client.enqueue({ type: "start-turn", text: "do not replay" });
    await firstFailed;
    client.replaceBinding(bindingFor(epochB));

    await expect(pending).rejects.toMatchObject({
      code: "STALE_SERVER_EPOCH",
      kind: "unavailable",
    });
    expect(epochA.calls).toHaveLength(1);
    expect(epochB.calls).toHaveLength(0);
    await client.close();
  });

  it.each([
    [
      "wrong protocol",
      { protocolVersion: PROTOCOL_VERSION + 1, accepted: true },
      "INVALID_RECEIPT",
    ],
    ["wrong command", { commandId: "wrong", accepted: true }, "INVALID_RECEIPT"],
    ["rejected", { accepted: false }, "INVALID_RECEIPT"],
  ])("rejects a %s receipt", async (_label, override, expectedCode) => {
    const daemon = await spawnDaemon({
      startTurn: (call, callback) => {
        callback(null, {
          $type: "lyonbrown.agent.v1.CommandResponse",
          protocolVersion: PROTOCOL_VERSION,
          commandId: call.request.metadata?.commandId ?? "",
          ...override,
        });
      },
    });
    const client = clientFor(daemon);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });
    await expect(client.enqueue({ type: "start-turn", text: "x" })).rejects.toMatchObject({
      code: expectedCode,
      kind: "protocol",
    });
    await client.close();
  });
});

describe("validated snapshots and event streams", () => {
  it("maps a valid snapshot without exposing generated protobuf fields", async () => {
    const daemon = await spawnDaemon();
    const client = clientFor(daemon);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });

    const snapshot = await client.getSnapshot();

    expect(snapshot).toMatchObject({ revision: 1n, phase: "ready", sessionId: null });
    expect(snapshot).not.toHaveProperty("$type");
    await client.close();
  });

  it("rejects mismatched outer and inner event revisions", async () => {
    const daemon = await spawnDaemon({
      subscribe: (call) => {
        const snapshot = RuntimeSnapshot.fromPartial({
          protocolVersion: PROTOCOL_VERSION,
          revision: 3n,
          phase: RuntimePhase.RUNTIME_PHASE_READY,
          modelSelection: ModelSelection.fromPartial({
            selection: { $case: "auto", auto: true },
          }),
        });
        call.write(
          RuntimeEvent.fromPartial({
            protocolVersion: PROTOCOL_VERSION,
            revision: 2n,
            snapshot,
          }),
        );
        call.end();
      },
    });
    const client = clientFor(daemon);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });

    const events = client.subscribeEvents({ afterRevision: 1n });
    const iterator = events[Symbol.asyncIterator]();
    await expect(iterator.next()).rejects.toMatchObject({
      code: "REVISION_MISMATCH",
      kind: "protocol",
    });
    await client.close();
  });

  it("cancels the grpc stream when iteration is stopped", async () => {
    let cancelled = false;
    let observeCancellation: (() => void) | undefined;
    const cancellationObserved = new Promise<void>((resolve) => {
      observeCancellation = resolve;
    });
    const daemon = await spawnDaemon({
      subscribe: (call) => {
        call.on("cancelled", () => {
          cancelled = true;
          observeCancellation?.();
        });
        call.write(
          RuntimeEvent.fromPartial({
            protocolVersion: PROTOCOL_VERSION,
            revision: 2n,
            snapshot: RuntimeSnapshot.fromPartial({
              protocolVersion: PROTOCOL_VERSION,
              revision: 2n,
              phase: RuntimePhase.RUNTIME_PHASE_READY,
              modelSelection: ModelSelection.fromPartial({
                selection: { $case: "auto", auto: true },
              }),
            }),
          }),
        );
      },
    });
    const client = clientFor(daemon);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });

    for await (const event of client.subscribeEvents({ afterRevision: 1n })) {
      expect(event.revision).toBe(2n);
      break;
    }
    await cancellationObserved;

    expect(cancelled).toBe(true);
    await client.close();
  });
});
