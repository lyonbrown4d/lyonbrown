import { describe, expect, it, vi } from "vitest";

import { AgentTuiController } from "@lyonbrown/agent-cli/internal/controller";
import {
  connection,
  FakeAgentClient,
  FakeLauncher,
} from "@lyonbrown/agent-cli/test/support/fake-launcher";
import { runtimeSnapshot } from "@lyonbrown/agent-cli/test/support/runtime";

describe("TUI interaction boundaries", () => {
  it("accepts only selectable models advertised by the daemon snapshot", async () => {
    const client = new FakeAgentClient();
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(
        Promise.resolve(
          connection(
            client,
            runtimeSnapshot({
              modelCatalog: {
                models: [
                  {
                    route: { provider: "local", model: "qwen3-0.6b" },
                    tier: "worker",
                    locality: "local",
                    contextWindow: 8_192,
                    maxOutputTokens: 1_024,
                    supportsTools: true,
                    supportsStreaming: true,
                    supportsJson: true,
                    supportsThinking: true,
                    selectableForWorker: true,
                    disabledReason: null,
                  },
                ],
              },
            }),
          ),
        ),
      ),
      render: () => undefined,
    });
    controller.open();
    await vi.waitFor(() => expect(controller.currentFrame().ui.remote.connection).toBe("ready"));

    expect(await controller.submit("/model local/not-advertised")).toBe(false);
    expect(client.commands).toEqual([]);
    expect(controller.currentFrame().notice).toContain("not selectable");

    expect(await controller.submit("/model local/qwen3-0.6b")).toBe(true);
    expect(client.commands).toEqual([
      {
        type: "select-session-model",
        selection: { kind: "pinned", provider: "local", model: "qwen3-0.6b" },
      },
    ]);
  });

  it("closes an activity inspector when the accepted session changes", async () => {
    const client = new FakeAgentClient();
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(
        Promise.resolve(connection(client, runtimeSnapshot({ sessionId: "session-a" }))),
      ),
      render: () => undefined,
    });
    controller.open();
    await vi.waitFor(() => expect(controller.currentFrame().ui.remote.connection).toBe("ready"));
    controller.toggleActivity();
    expect(controller.currentFrame().overlay).toEqual({ kind: "activity" });

    client.events.push({
      protocolVersion: 1,
      revision: 2n,
      snapshot: runtimeSnapshot({ revision: 2n, sessionId: "session-b" }),
    });

    await vi.waitFor(() => expect(controller.currentFrame().snapshot?.sessionId).toBe("session-b"));
    expect(controller.currentFrame().overlay).toBeNull();
  });
});
