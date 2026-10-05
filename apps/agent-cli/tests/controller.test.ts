import { describe, expect, it, vi } from "vitest";

import { AgentTuiController, type AgentTuiFrame } from "../src/controller.ts";
import { connection, FakeAgentClient, FakeLauncher } from "./support/fake-launcher.ts";
import { runtimeSnapshot } from "./support/runtime.ts";

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

  it("streams monotonic snapshots and marks a lost stream stale", async () => {
    const client = new FakeAgentClient();
    const frames: AgentTuiFrame[] = [];
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client))),
      render: (frame) => frames.push(frame),
    });
    controller.open();
    await vi.waitFor(() => expect(frames.at(-1)?.ui.remote.connection).toBe("ready"));

    const next = runtimeSnapshot({ revision: 2n, phase: "streaming" });
    client.events.push({ protocolVersion: 1, revision: 2n, snapshot: next });
    await vi.waitFor(() => expect(frames.at(-1)?.snapshot?.revision).toBe(2n));
    client.events.fail(new Error("stream gone"));

    await vi.waitFor(() => {
      expect(frames.at(-1)?.ui.remote).toMatchObject({
        connection: "stale",
        unavailableReason: "event stream unavailable",
      });
    });
  });

  it("marks a naturally completed event stream stale", async () => {
    const client = new FakeAgentClient();
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client))),
      render: () => undefined,
    });
    controller.open();
    await vi.waitFor(() => expect(controller.currentFrame().ui.remote.connection).toBe("ready"));

    client.events.complete();

    await vi.waitFor(() => {
      expect(controller.currentFrame().ui.remote).toMatchObject({
        connection: "stale",
        unavailableReason: "event stream ended",
      });
    });
  });

  it("atomically ignores a conflicting snapshot at the current revision", async () => {
    const client = new FakeAgentClient();
    const frames: AgentTuiFrame[] = [];
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client))),
      render: (frame) => frames.push(frame),
    });
    controller.open();
    await vi.waitFor(() => expect(frames.at(-1)?.ui.remote.connection).toBe("ready"));

    const conflicting = runtimeSnapshot({
      revision: 1n,
      lastError: "conflicting payload",
      pendingApproval: {
        requestId: "forged-at-same-revision",
        toolName: "execute",
        argumentsJson: "{}",
        stepId: "forged-step",
        preview: null,
        deadlineRemainingMs: 5_000n,
        state: "pending",
        approvedBy: null,
      },
    });
    client.events.push({ protocolVersion: 1, revision: 1n, snapshot: conflicting });
    await Promise.resolve();

    expect(controller.currentFrame().snapshot?.lastError).toBeNull();
    expect(controller.currentFrame().ui.approval).toBeNull();
  });

  it("fences late stream values and closes the authenticated client exactly once", async () => {
    const client = new FakeAgentClient();
    const frames: AgentTuiFrame[] = [];
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(Promise.resolve(connection(client))),
      render: (frame) => frames.push(frame),
    });
    controller.open();
    await vi.waitFor(() => expect(frames.at(-1)?.ui.remote.connection).toBe("ready"));

    await controller.close();
    const count = frames.length;
    client.events.push({
      protocolVersion: 1,
      revision: 2n,
      snapshot: runtimeSnapshot({ revision: 2n }),
    });
    await Promise.resolve();

    expect(client.closeCount).toBe(1);
    expect(client.subscriptions).toHaveLength(1);
    expect(client.subscriptions[0]?.afterRevision).toBe(1n);
    expect(client.subscriptions[0]?.signal).toBeInstanceOf(AbortSignal);
    expect(client.subscriptions[0]?.signal?.aborted).toBe(true);
    expect(frames).toHaveLength(count);
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
});
