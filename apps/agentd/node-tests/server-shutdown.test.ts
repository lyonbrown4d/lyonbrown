import { describe, expect, it, vi } from "vitest";

import { shutdownServer, type GrpcShutdownHandle } from "@lyonbrown/agentd/internal/server";

describe("gRPC server shutdown", () => {
  it("forces shutdown when graceful shutdown does not settle within the bound", async () => {
    const tryShutdown = vi.fn(() => undefined);
    const forceShutdown = vi.fn();
    const server: GrpcShutdownHandle = {
      tryShutdown,
      forceShutdown,
    };

    await expect(shutdownServer(server, 5)).resolves.toBeUndefined();
    expect(tryShutdown).toHaveBeenCalledOnce();
    expect(forceShutdown).toHaveBeenCalledOnce();
  });

  it("does not force a server that completes graceful shutdown", async () => {
    const forceShutdown = vi.fn();
    const server: GrpcShutdownHandle = {
      tryShutdown: (callback) => callback(),
      forceShutdown,
    };

    await expect(shutdownServer(server, 5)).resolves.toBeUndefined();
    expect(forceShutdown).not.toHaveBeenCalled();
  });

  it("rejects invalid shutdown bounds", async () => {
    const tryShutdown = vi.fn();
    const server: GrpcShutdownHandle = {
      tryShutdown,
      forceShutdown: vi.fn(),
    };

    await expect(shutdownServer(server, 0)).rejects.toThrow(RangeError);
    expect(tryShutdown).not.toHaveBeenCalled();
  });
});
