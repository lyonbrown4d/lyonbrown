import { createInitialUiModelState, reduceUiModel } from "@lyonbrown/ui-model";
import { describe, expect, it, vi } from "vitest";

import type { AgentTuiFrame } from "@lyonbrown/agent-cli/internal/controller";
import { createPiScreen } from "@lyonbrown/agent-cli/internal/pi-screen";
import { MemoryTerminal } from "@lyonbrown/agent-cli/test/support/memory-terminal";
import { runtimeSnapshot } from "@lyonbrown/agent-cli/test/support/runtime";

function frame(): AgentTuiFrame {
  let ui = createInitialUiModelState();
  ui = reduceUiModel(ui, { type: "connectRequested" });
  ui = reduceUiModel(ui, { type: "handshakeSucceeded", serverInstanceId: "server" });
  ui = reduceUiModel(ui, {
    type: "snapshotReceived",
    snapshot: {
      serverInstanceId: "server",
      sessionId: "session",
      revision: 1n,
      modelSetup: { phase: "Required", operationId: null },
      activities: [],
    },
  });
  return {
    ui,
    notice: null,
    overlay: { kind: "control", tab: "decision" },
    snapshot: runtimeSnapshot({
      phase: "model-setup-required",
      modelSetup: {
        source: "managed-preset",
        phase: "required",
        managedModel: null,
        operationId: null,
        downloadedBytes: 0n,
        totalBytes: null,
        failure: null,
        allowedActions: ["download"],
      },
      decisionModel: {
        phase: "required",
        providerId: "open-jev",
        modelId: "kev-0.6b",
        modelRevision: "a".repeat(40),
        advisoryOnly: true,
        operationId: null,
        downloadedBytes: 0n,
        totalBytes: 100n,
        allowedActions: ["download"],
        failure: null,
      },
    }),
  };
}

describe("decision panel terminal input", () => {
  it("navigates between decision settings and theme preview without starting an install", () => {
    const terminal = new MemoryTerminal();
    const submit = vi.fn();
    const preview = vi.fn();
    const screen = createPiScreen({
      terminal,
      frame: frame(),
      onSubmit: submit,
      onExit: vi.fn(),
      onLanguageToggle: vi.fn(),
      onPaletteToggle: vi.fn(),
      onActivityToggle: vi.fn(),
      onOverlayClose: vi.fn(),
      onThemePreview: preview,
    });
    screen.start();
    screen.update(frame());
    try {
      terminal.input("\u001b[D");
      terminal.input("\u001b[D");
      terminal.input(" ");
      expect(preview).toHaveBeenCalledWith(frame().ui.preferences.themeId);
      expect(submit).not.toHaveBeenCalled();
      terminal.input("\r");
      expect(submit).toHaveBeenCalledWith(`/theme ${frame().ui.preferences.themeId}`);
      expect(submit).not.toHaveBeenCalledWith("/decision install");
    } finally {
      screen.stop();
    }
  });

  it("keeps decision installation keys separate from chat setup keys and stale state", async () => {
    const terminal = new MemoryTerminal();
    const submit = vi.fn();
    const close = vi.fn();
    const palette = vi.fn();
    const screen = createPiScreen({
      terminal,
      frame: frame(),
      onSubmit: submit,
      onExit: vi.fn(),
      onLanguageToggle: vi.fn(),
      onPaletteToggle: palette,
      onActivityToggle: vi.fn(),
      onOverlayClose: close,
    });
    screen.start();
    screen.update(frame());
    try {
      await vi.waitFor(() => expect(terminal.output).toContain("Decision"));
      terminal.input("d");
      expect(submit).toHaveBeenCalledWith("/decision install");
      expect(submit).not.toHaveBeenCalledWith("/download");
      const downloading = frame();
      const snapshot = downloading.snapshot;
      const decision = snapshot?.decisionModel;
      if (snapshot === null || decision === null || decision === undefined)
        throw new Error("fixture has no decision model");
      screen.update({
        ...downloading,
        snapshot: runtimeSnapshot({
          ...downloading.snapshot,
          decisionModel: {
            ...decision,
            phase: "downloading",
            operationId: "decision-op",
            allowedActions: ["cancel"],
          },
        }),
      });
      terminal.input("c");
      expect(submit).toHaveBeenCalledWith("/decision cancel");
      const stale = frame();
      screen.update({
        ...stale,
        ui: { ...stale.ui, remote: { ...stale.ui.remote, snapshotFresh: false } },
      });
      const count = submit.mock.calls.length;
      terminal.input("\r");
      expect(submit).toHaveBeenCalledTimes(count);
      terminal.input("\u001b");
      expect(close).toHaveBeenCalledOnce();
      screen.update({ ...frame(), overlay: null });
      terminal.input("\u0010");
      expect(palette).toHaveBeenCalledOnce();
    } finally {
      screen.stop();
    }
  });
});
