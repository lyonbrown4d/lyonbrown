import { homedir } from "node:os";
import { isAbsolute } from "node:path";
import type { AgentLauncher } from "@lyonbrown/agent-launch";
import type {
  CreateProductionAgentLauncherOptions,
  NativeControlPlaneProvider,
} from "@lyonbrown/agent-launch/composition";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  const launcher = {} as AgentLauncher;
  return {
    launcher,
    nativeProvider: Object.freeze({}) as NativeControlPlaneProvider,
    createProductionAgentLauncher: vi.fn<
      (options: CreateProductionAgentLauncherOptions) => AgentLauncher
    >(() => launcher),
  };
});

vi.mock("@lyonbrown/agent-launch/composition", () => ({
  createProductionAgentLauncher: mocks.createProductionAgentLauncher,
}));
vi.mock("@lyonbrown/control-plane-native", () => ({
  createControlPlaneNativeProvider: () => mocks.nativeProvider,
}));

import { createProductionMainDependencies } from "../src/production.ts";

describe("production TUI composition", () => {
  it("creates only the authenticated client launcher capability", () => {
    const dependencies = createProductionMainDependencies();

    expect(dependencies).toEqual({ launcher: mocks.launcher });
    expect(mocks.createProductionAgentLauncher).toHaveBeenCalledOnce();
    const options = mocks.createProductionAgentLauncher.mock.calls[0]?.[0];
    if (options === undefined) throw new Error("production launcher options were not captured");
    expect(options).toMatchObject({
      nativeProvider: mocks.nativeProvider,
      homeDirectory: homedir(),
      workspace: process.cwd(),
      effectiveConfigFingerprint: "production-default",
      agentd: {
        command: process.execPath,
        workingDirectory: process.cwd(),
      },
      client: { name: "agent-cli", version: "0.0.0" },
    });
    expect(options).not.toHaveProperty("runtime");
    expect(options).not.toHaveProperty("eventStore");
    expect(options).not.toHaveProperty("model");
    expect(options.agentd.arguments).toHaveLength(1);
    expect(isAbsolute(options.agentd.arguments?.[0] ?? "")).toBe(true);
    expect(options.agentd.arguments?.[0]).toMatch(/[\\/]apps[\\/]agentd[\\/]dist[\\/]main\.js$/u);
  });
});
