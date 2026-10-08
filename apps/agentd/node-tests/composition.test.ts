import type { NativeControlPlaneProvider } from "@lyonbrown/local-control-plane";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { createRuntimeInternal, startAgentdInternal } = vi.hoisted(() => ({
  createRuntimeInternal: vi.fn(),
  startAgentdInternal: vi.fn(),
}));

vi.mock("../node-src/bootstrap.ts", () => ({
  startProductionAgentd: startAgentdInternal,
}));

vi.mock("../node-src/production-runtime.ts", () => ({
  createProductionAgentdRuntime: createRuntimeInternal,
}));

import * as composition from "@lyonbrown/agentd/internal/composition";

describe("public production composition", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("starts a supervised daemon with the production runtime factory", async () => {
    expect(Object.keys(composition)).toEqual([
      "createProductionDecisionService",
      "startProductionAgentd",
    ]);
    const nativeProvider = Object.freeze({}) as NativeControlPlaneProvider;
    const options = {
      workspace: "C:\\work\\lyonbrown",
      homeDirectory: "C:\\Users\\agent",
      effectiveConfigFingerprint: "smoke-v1",
      expectedScopeId: "a".repeat(64),
      nativeProvider,
    };
    const running = Object.freeze({
      address: "127.0.0.1:43123",
      port: 43123,
      scopeId: options.expectedScopeId,
      close: vi.fn(async () => undefined),
    });
    startAgentdInternal.mockResolvedValue(running);

    await expect(composition.startProductionAgentd(options)).resolves.toBe(running);
    expect(startAgentdInternal).toHaveBeenCalledWith({
      ...options,
      createRuntime: createRuntimeInternal,
    });
  });
});
