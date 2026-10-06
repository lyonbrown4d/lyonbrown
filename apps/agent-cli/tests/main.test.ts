import type { AgentLauncher } from "@lyonbrown/agent-launch";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  productionLauncher: {} as AgentLauncher,
  runAgentTui: vi.fn<() => Promise<void>>(() => Promise.resolve()),
}));

vi.mock("../src/app.ts", () => ({ runAgentTui: mocks.runAgentTui }));
vi.mock("../src/production.ts", () => ({
  createProductionMainDependencies: () => ({ launcher: mocks.productionLauncher }),
}));

import { main } from "@lyonbrown/agent-cli/internal/main";

describe("agent production entry", () => {
  beforeEach(() => {
    mocks.runAgentTui.mockClear();
  });

  it("enters chat with a production launcher when dependencies are not injected", async () => {
    await expect(main([])).resolves.toBeUndefined();

    expect(mocks.runAgentTui).toHaveBeenCalledOnce();
    expect(mocks.runAgentTui).toHaveBeenCalledWith({ launcher: mocks.productionLauncher });
  });

  it("keeps explicit launcher injection for isolated callers", async () => {
    const launcher = {} as AgentLauncher;

    await main([], { launcher });

    expect(mocks.runAgentTui).toHaveBeenCalledWith({ launcher });
  });
});
