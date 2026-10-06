import type * as GrpcModule from "@grpc/grpc-js";
import { PROTOCOL_VERSION, RuntimeSnapshot, type RuntimeEvent } from "@lyonbrown/protocol";
import { describe, expect, it, vi } from "vitest";

import type { RuntimeHost } from "@lyonbrown/agentd/internal/runtime-host";

const grpcState = vi.hoisted(() => ({ constructorCalls: 0, bindCalls: 0 }));

vi.mock("@grpc/grpc-js", async (importOriginal) => {
  const actual = await importOriginal<typeof GrpcModule>();
  class FakeServer {
    public constructor(_options?: GrpcModule.ServerOptions) {
      grpcState.constructorCalls += 1;
    }

    public addService(): void {
      return undefined;
    }

    public bindAsync(
      _address: string,
      _credentials: unknown,
      callback: (error: Error | null, port: number) => void,
    ): void {
      grpcState.bindCalls += 1;
      callback(null, 43126);
    }

    public tryShutdown(callback: (error?: Error) => void): void {
      callback();
    }

    public forceShutdown(): void {
      return undefined;
    }
  }
  return { ...actual, Server: FakeServer };
});

const { startAgentdServer } = await import("@lyonbrown/agentd/internal/server");

const host: RuntimeHost = {
  execute: async () => ({ ok: true }),
  snapshot: () => RuntimeSnapshot.create({ protocolVersion: PROTOCOL_VERSION, revision: 0n }),
  subscribe: async function* () {
    yield* [] as RuntimeEvent[];
  },
  capabilities: () => [],
};

describe("agentd server startup validation", () => {
  it.each([0, -1, 60_001, Number.NaN, Number.POSITIVE_INFINITY, 1.5])(
    "rejects shutdown timeout %s before constructing or binding a server",
    async (shutdownTimeoutMs) => {
      grpcState.constructorCalls = 0;
      grpcState.bindCalls = 0;

      await expect(
        startAgentdServer({
          host,
          token: { exposeForTransport: () => "unused" } as never,
          serverInstanceId: "018f47d6-7c12-7b19-a0b6-ff87748f42bc",
          scopeFingerprint: "a".repeat(64),
          shutdownTimeoutMs,
        }),
      ).rejects.toThrow(RangeError);
      expect(grpcState.constructorCalls).toBe(0);
      expect(grpcState.bindCalls).toBe(0);
    },
  );
});
