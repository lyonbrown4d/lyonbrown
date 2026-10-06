import { describe, expect, it, vi } from "vitest";

import { AgentTuiController, type AgentTuiFrame } from "@lyonbrown/agent-cli/internal/controller";
import {
  connection,
  FakeAgentClient,
  FakeLauncher,
} from "@lyonbrown/agent-cli/test/support/fake-launcher";
import { runtimeSnapshot } from "@lyonbrown/agent-cli/test/support/runtime";

describe("AgentTuiController", () => {
  it("renders connecting before daemon discovery settles", () => {
    const frames: AgentTuiFrame[] = [];
    const never = new Promise<never>(() => undefined);
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(never),
      render: (frame) => frames.push(frame),
    });

    controller.open();

    expect(frames.at(-1)?.ui.remote.connection).toBe("connecting");
  });

  it("projects the authenticated connection and submits chat commands", async () => {
    const client = new FakeAgentClient();
    const frames: AgentTuiFrame[] = [];
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client))),
      render: (frame) => frames.push(frame),
    });

    controller.open();
    await vi.waitFor(() => expect(frames.at(-1)?.ui.remote.connection).toBe("ready"));
    await controller.submit("hello");
    await controller.submit("/steer use tests");
    await controller.submit("/follow-up explain it");
    await controller.submit("/cancel");

    expect(client.commands).toEqual([
      { type: "start-turn", text: "hello" },
      { type: "steer-current-turn", text: "use tests" },
      { type: "queue-follow-up", text: "explain it" },
      { type: "cancel-turn" },
    ]);
  });

  it("steers the active turn when ordinary composer text is submitted while running", async () => {
    const client = new FakeAgentClient();
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(
        Promise.resolve(connection(client, runtimeSnapshot({ phase: "streaming" }))),
      ),
      render: () => undefined,
    });
    controller.open();
    await vi.waitFor(() => expect(controller.currentFrame().ui.remote.connection).toBe("ready"));

    await controller.submit("focus on the failing test");

    expect(client.commands).toEqual([
      { type: "steer-current-turn", text: "focus on the failing test" },
    ]);
  });

  it("changes language with F2 semantics and exposes all six themes", async () => {
    const client = new FakeAgentClient();
    const frames: AgentTuiFrame[] = [];
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client))),
      render: (frame) => frames.push(frame),
    });
    controller.open();

    controller.toggleLanguage();
    await controller.submit("/theme graphite-coral");

    expect(frames.at(-1)?.ui.preferences).toEqual({
      locale: "zh-CN",
      themeId: "graphite-coral",
    });
  });

  it("previews a theme without persistence and saves only explicit theme or language choices", async () => {
    const client = new FakeAgentClient();
    const saved: { locale: string; themeId: string }[] = [];
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client))),
      render: () => undefined,
      preferenceStore: {
        load: async () => null,
        save: async (preferences) => {
          saved.push(preferences);
        },
      },
    });
    controller.open();

    controller.previewTheme("carbon-mint");
    expect(controller.currentFrame().ui.preferences.themeId).toBe("carbon-mint");
    expect(saved).toEqual([]);

    await controller.submit("/theme graphite-coral");
    await controller.submit("/language zh-CN");

    expect(saved).toEqual([
      { locale: "en-US", themeId: "graphite-coral" },
      { locale: "zh-CN", themeId: "graphite-coral" },
    ]);
  });

  it("reports a failed F2 preference save without an unhandled rejection", async () => {
    const client = new FakeAgentClient();
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client))),
      render: () => undefined,
      preferenceStore: {
        load: async () => null,
        save: async () => Promise.reject(new Error("preference store unavailable")),
      },
    });
    controller.open();

    controller.toggleLanguage();

    await vi.waitFor(() =>
      expect(controller.currentFrame().notice).toBe("preference store unavailable"),
    );
  });

  it("localizes client-side command failures without sending unknown slash input", async () => {
    const client = new FakeAgentClient();
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client))),
      render: () => undefined,
    });
    controller.open();
    controller.toggleLanguage();

    await controller.submit("/unknown secret");

    expect(controller.currentFrame().notice).toContain("未知命令");
    expect(client.commands).toEqual([]);
  });

  it("opens typed overlays, selects a session model and requests local exit", async () => {
    const client = new FakeAgentClient();
    const exits: string[] = [];
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client))),
      render: () => undefined,
      requestExit: () => exits.push("exit"),
    });
    controller.open();
    await vi.waitFor(() => expect(controller.currentFrame().ui.remote.connection).toBe("ready"));

    await controller.submit("/model");
    expect(controller.currentFrame().overlay).toEqual({ kind: "control", tab: "model" });
    await controller.submit("/model auto");
    await controller.submit("/status");
    expect(controller.currentFrame().overlay).toEqual({ kind: "status" });
    await controller.submit("/quit");

    expect(client.commands).toContainEqual({
      type: "select-session-model",
      selection: { kind: "auto" },
    });
    expect(exits).toEqual(["exit"]);
  });

  it("uses only fresh snapshot capabilities for retry and approval", async () => {
    const client = new FakeAgentClient();
    const snapshot = runtimeSnapshot({
      phase: "awaiting-approval",
      modelSetup: {
        source: "managed-preset",
        phase: "failed",
        managedModel: {
          id: "qwen3-1.7b-q4_k_m",
          displayName: null,
          recommendationReason: null,
          expectedDownloadBytes: 100n,
          maxBytes: 110n,
          minAvailableMemoryBytes: 1n,
          sha256: "a".repeat(64),
        },
        operationId: "download-1",
        downloadedBytes: 20n,
        totalBytes: 100n,
        failure: { code: "network", message: "offline", retryable: true },
        allowedActions: ["retry"],
      },
      pendingApproval: {
        requestId: "approval-1",
        toolName: "write",
        argumentsJson: "{}",
        stepId: "step-1",
        preview: null,
        deadlineRemainingMs: 5_000n,
        state: "pending",
        approvedBy: null,
      },
    });
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client, snapshot))),
      render: () => undefined,
    });
    controller.open();
    await vi.waitFor(() => expect(controller.currentFrame().ui.remote.connection).toBe("ready"));

    await controller.submit("/retry");
    await controller.submit("/approve once");

    expect(client.commands).toEqual([
      { type: "start-model-download", modelId: "qwen3-1.7b-q4_k_m" },
      { type: "respond-approval", requestId: "approval-1", decision: "allow-once" },
    ]);
  });

  it("keeps chat disabled while activation has no operation id", async () => {
    const client = new FakeAgentClient();
    const snapshot = runtimeSnapshot({
      phase: "model-setup-required",
      modelSetup: {
        source: "managed-preset",
        phase: "activating",
        managedModel: null,
        operationId: null,
        downloadedBytes: 0n,
        totalBytes: null,
        failure: null,
        allowedActions: [],
      },
    });
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client, snapshot))),
      render: () => undefined,
    });
    controller.open();
    await vi.waitFor(() => expect(controller.currentFrame().ui.remote.connection).toBe("ready"));

    await controller.submit("must not be sent");

    expect(client.commands).toEqual([]);
    expect(controller.currentFrame().notice).toBe(
      "chat is unavailable until the daemon and model are ready",
    );
  });

  it("retries a failed managed model without an operation id", async () => {
    const client = new FakeAgentClient();
    const snapshot = runtimeSnapshot({
      phase: "model-setup-required",
      modelSetup: {
        source: "managed-preset",
        phase: "failed",
        managedModel: {
          id: "qwen3-1.7b-q4_k_m",
          displayName: null,
          recommendationReason: null,
          expectedDownloadBytes: 100n,
          maxBytes: 110n,
          minAvailableMemoryBytes: 1n,
          sha256: "a".repeat(64),
        },
        operationId: null,
        downloadedBytes: 20n,
        totalBytes: 100n,
        failure: { code: "activation", message: "load failed", retryable: true },
        allowedActions: ["retry"],
      },
    });
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client, snapshot))),
      render: () => undefined,
    });
    controller.open();
    await vi.waitFor(() => expect(controller.currentFrame().ui.remote.connection).toBe("ready"));

    await controller.submit("/retry");

    expect(client.commands).toEqual([
      { type: "start-model-download", modelId: "qwen3-1.7b-q4_k_m" },
    ]);
  });
});
