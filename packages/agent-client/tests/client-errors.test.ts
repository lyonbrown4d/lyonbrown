import { Metadata, status } from "@grpc/grpc-js";
import { CommandErrorCode, CommandErrorDetail } from "@lyonbrown/protocol";
import { afterAll, describe, expect, it } from "vitest";

import { AgentClientError, createAgentClient, type AgentCommand } from "../src/index.ts";
import { bindingFor, clientFor, closeDaemons, spawnDaemon } from "./support/fake-daemon.ts";

afterAll(async () => {
  await closeDaemons();
});

describe("boundary and typed error mapping", () => {
  it("rejects deadlines and retry counts that can cause unbounded resource use", async () => {
    const daemon = await spawnDaemon();

    expect(() =>
      createAgentClient({
        binding: bindingFor(daemon),
        deadlines: { unaryMs: 300_001, commandAttempts: 11, retryBackoffMs: 60_001 },
      }),
    ).toThrow(/deadlines\.unaryMs/u);
  });

  it("rejects a structurally forged non-loopback binding", async () => {
    const daemon = await spawnDaemon();
    const binding = bindingFor(daemon);

    expect(() =>
      createAgentClient({ binding: { ...binding, endpoint: "example.com:443" } }),
    ).toThrow(/loopback/u);
  });

  it.each<AgentCommand>([
    { type: "start-turn", text: "a".repeat(1024 * 1024 + 1) },
    { type: "respond-approval", requestId: "", decision: "allow-once" },
  ])("rejects invalid UTF-8-budgeted command input before transport", async (command) => {
    const daemon = await spawnDaemon();
    const client = clientFor(daemon);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });

    await expect(client.enqueue(command)).rejects.toMatchObject({
      code: "INVALID_ARGUMENT",
      kind: "validation",
    });
    expect(daemon.calls).toHaveLength(0);
    await client.close();
  });

  it("maps structured command errors without leaking grpc errors", async () => {
    const daemon = await spawnDaemon({
      startTurn: (_call, callback) => {
        const details = CommandErrorDetail.encode(
          CommandErrorDetail.fromPartial({
            code: CommandErrorCode.COMMAND_ERROR_CODE_TURN_ALREADY_ACTIVE,
          }),
        ).finish();
        const metadata = new Metadata();
        metadata.set("grpc-status-details-bin", Buffer.from(details));
        callback({
          code: status.FAILED_PRECONDITION,
          message: "internal detail must not become the public message",
          details: "internal detail must not become the public message",
          metadata,
        } as never);
      },
    });
    const client = clientFor(daemon);
    await client.handshake({ clientName: "tui", clientVersion: "0.1.0" });

    await expect(client.enqueue({ type: "start-turn", text: "x" })).rejects.toEqual(
      new AgentClientError("command", "TURN_ALREADY_ACTIVE", "turn is already active"),
    );
    await client.close();
  });
});
