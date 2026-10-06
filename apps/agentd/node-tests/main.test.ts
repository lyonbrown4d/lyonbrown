import type { NativeControlPlaneProvider } from "@lyonbrown/local-control-plane";
import { describe, expect, it, vi } from "vitest";

import { main, type AgentdMainDependencies } from "@lyonbrown/agentd/internal/main";
import { createProductionAgentdRuntime } from "@lyonbrown/agentd/internal/production-runtime";

describe("agentd main composition", () => {
  it("passes validated launcher arguments and the native provider to bootstrap", async () => {
    const provider = Object.freeze({}) as NativeControlPlaneProvider;
    const running = {
      address: "127.0.0.1:43125",
      port: 43125,
      scopeId: "a".repeat(64),
      close: vi.fn(async () => undefined),
    };
    const startAgentd = vi.fn(async () => running);
    const waitForShutdown = vi.fn(async () => undefined);
    const dependencies: AgentdMainDependencies = {
      homeDirectory: () => "C:\\Users\\agent",
      createNativeProvider: () => provider,
      startAgentd,
      waitForShutdown,
    };

    await main(
      [
        "--scope-id",
        "a".repeat(64),
        "--workspace",
        "C:\\work\\lyonbrown",
        "--effective-config-fingerprint",
        "production-v1",
      ],
      dependencies,
    );

    expect(startAgentd).toHaveBeenCalledOnce();
    expect(startAgentd).toHaveBeenCalledWith({
      workspace: "C:\\work\\lyonbrown",
      homeDirectory: "C:\\Users\\agent",
      effectiveConfigFingerprint: "production-v1",
      expectedScopeId: "a".repeat(64),
      nativeProvider: provider,
      createRuntime: createProductionAgentdRuntime,
    });
    expect(waitForShutdown).toHaveBeenCalledWith(running);
  });
});
