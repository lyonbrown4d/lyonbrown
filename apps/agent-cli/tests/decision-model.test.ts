import { createInitialUiModelState, reduceUiModel } from "@lyonbrown/ui-model";
import type { DecisionModelDto } from "@lyonbrown/agent-client";
import { describe, expect, it, vi } from "vitest";

import { parseInput } from "@lyonbrown/agent-cli/internal/commands";
import { AgentTuiController, type AgentTuiFrame } from "@lyonbrown/agent-cli/internal/controller";
import { decisionModelCommand } from "@lyonbrown/agent-cli/internal/decision-model-commands";
import { decisionPanelAction } from "@lyonbrown/agent-cli/internal/decision-model-surface";
import { renderOverlay } from "@lyonbrown/agent-cli/internal/overlay-surface";
import {
  connection,
  FakeAgentClient,
  FakeLauncher,
} from "@lyonbrown/agent-cli/test/support/fake-launcher";
import { runtimeSnapshot } from "@lyonbrown/agent-cli/test/support/runtime";

const MODEL: DecisionModelDto = {
  providerId: "open-jev",
  modelId: "kev-0.6b",
  modelRevision: "a".repeat(40),
  phase: "required",
  advisoryOnly: true,
  operationId: null,
  downloadedBytes: 0n,
  totalBytes: 380_000_000n,
  allowedActions: ["download"],
  failure: null,
};

function frame(model: DecisionModelDto = MODEL): AgentTuiFrame {
  let ui = createInitialUiModelState();
  ui = reduceUiModel(ui, { type: "connectRequested" });
  ui = reduceUiModel(ui, { type: "handshakeSucceeded", serverInstanceId: "server" });
  ui = reduceUiModel(ui, {
    type: "snapshotReceived",
    snapshot: {
      serverInstanceId: "server",
      sessionId: "session-1",
      revision: 1n,
      modelSetup: null,
      activities: [],
    },
  });
  return {
    ui,
    snapshot: runtimeSnapshot({ decisionModel: model }),
    overlay: { kind: "control", tab: "decision" },
    notice: null,
  };
}

describe("decision model TUI", () => {
  it("explains configuration and failure recovery and keeps absent status read-only", () => {
    const unconfigured = frame({ ...MODEL, phase: "unavailable", allowedActions: [] });
    expect(renderOverlay(unconfigured, 80).join("\n")).toContain(
      "Configure the model storage directory",
    );
    const failed = frame({
      ...MODEL,
      phase: "failed",
      failure: { code: "install_failed", message: "Setup failed", retryable: true },
      allowedActions: ["retry"],
    });
    expect(renderOverlay(failed, 80).join("\n")).toContain("Retry installation");
    expect(renderOverlay(failed, 80).join("\n")).toContain("install_failed");
    expect(decisionPanelAction(failed)).toBe("/decision retry");
    expect(decisionModelCommand(failed.snapshot, "retry")).toEqual({
      type: "start-model-download",
      purpose: "decision",
      modelId: "kev-0.6b",
    });
    const missing = { ...frame(), snapshot: runtimeSnapshot() };
    expect(renderOverlay(missing, 80).join("\n")).toContain("Status unavailable");
    expect(decisionPanelAction(missing)).toBeNull();
  });

  it.each(["en-US", "zh-CN"] as const)(
    "keeps help and unavailable session capabilities truthful in %s",
    (locale) => {
      const initial = frame();
      const f = {
        ...initial,
        ui: { ...initial.ui, preferences: { ...initial.ui.preferences, locale } },
      };
      const help = renderOverlay({ ...f, overlay: { kind: "help" } }, 80).join("\n");
      expect(help).toContain("Alt-Enter");
      expect(help).toContain(locale === "zh-CN" ? "排队到下一步" : "queue for next step");
      const sessions = renderOverlay({ ...f, overlay: { kind: "sessions" } }, 80).join("\n");
      expect(sessions).toContain(
        locale === "zh-CN" ? "尚未提供会话目录接口" : "does not expose a session catalog yet",
      );
    },
  );
  it("parses panel and explicit actions without treating ordinary chat as an install", () => {
    expect(parseInput("/decision")).toEqual({ kind: "control", tab: "decision" });
    expect(parseInput("/decision install")).toEqual({ kind: "decision-action", action: "install" });
    expect(() => parseInput("/decision unsafe")).toThrow();
  });

  it.each(["en-US", "zh-CN"] as const)(
    "renders optional setup and download confirmation in %s",
    (locale) => {
      const f = frame();
      const rendered = renderOverlay(
        { ...f, ui: { ...f.ui, preferences: { ...f.ui.preferences, locale } } },
        80,
      ).join("\n");
      expect(rendered).toContain(locale === "zh-CN" ? "尚未安装" : "Not installed");
      expect(rendered).toContain(locale === "zh-CN" ? "仅在确认后" : "only after confirmation");
      expect(decisionPanelAction(f)).toBe("/decision install");
    },
  );

  it("distinguishes installed weights from a loaded engine and disables stale commands", () => {
    const f = frame({ ...MODEL, phase: "configured", allowedActions: [] });
    expect(renderOverlay(f, 80).join("\n")).toContain("loads on first use");
    expect(decisionPanelAction(f)).toBeNull();
    const stale = { ...frame(), ui: { ...f.ui, remote: { ...f.ui.remote, snapshotFresh: false } } };
    expect(decisionPanelAction(stale)).toBeNull();
    expect(renderOverlay(stale, 80).join("\n")).toContain("actions disabled");
  });

  it("targets decision installation and cancellation without touching chat downloads", () => {
    expect(decisionModelCommand(runtimeSnapshot({ decisionModel: MODEL }), "install")).toEqual({
      type: "start-model-download",
      purpose: "decision",
      modelId: "kev-0.6b",
    });
    expect(() => decisionModelCommand(null, "install")).toThrow();
    expect(() =>
      decisionModelCommand(runtimeSnapshot({ decisionModel: MODEL }), "cancel"),
    ).toThrow();
    const installing = {
      ...MODEL,
      phase: "downloading" as const,
      operationId: "decision-op",
      allowedActions: ["cancel"] as const,
    };
    expect(decisionModelCommand(runtimeSnapshot({ decisionModel: installing }), "cancel")).toEqual({
      type: "cancel-model-download",
      purpose: "decision",
      operationId: "decision-op",
    });
  });

  it("allows chat with decision weights missing and sends install only on an explicit action", async () => {
    const client = new FakeAgentClient();
    const controller = new AgentTuiController({
      launcher: new FakeLauncher(
        Promise.resolve(connection(client, runtimeSnapshot({ decisionModel: MODEL }))),
      ),
      render: () => undefined,
    });
    controller.open();
    await vi.waitFor(() => expect(controller.currentFrame().ui.remote.connection).toBe("ready"));
    await controller.submit("hello");
    await controller.submit("/decision");
    expect(client.commands).toEqual([{ type: "start-turn", text: "hello" }]);
    await controller.submit("/decision install");
    expect(client.commands.at(-1)).toEqual({
      type: "start-model-download",
      purpose: "decision",
      modelId: "kev-0.6b",
    });
    await controller.close();
  });
});
