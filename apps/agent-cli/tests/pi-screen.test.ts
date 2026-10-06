import type { Terminal } from "@earendil-works/pi-tui";
import { describe, expect, it, vi } from "vitest";
import { createInitialUiModelState, reduceUiModel, type UiModelState } from "@lyonbrown/ui-model";

import { createPiScreen } from "../src/pi-screen.ts";
import { createInitialFrame, type AgentTuiFrame } from "../src/controller.ts";
import { runtimeSnapshot } from "./support/runtime.ts";

describe("Pi TUI screen", () => {
  it("uses TuiMainScreen focus propagation and preserves Chinese IME input", async () => {
    const terminal = new MemoryTerminal();
    const submitted: string[] = [];
    const screen = createPiScreen({
      terminal,
      frame: createInitialFrame(),
      onSubmit: (text) => {
        submitted.push(text);
      },
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onExit: () => undefined,
    });

    screen.start();
    terminal.input("中文输入");
    expect(screen.editorText()).toBe("中文输入");
    terminal.input("\r");

    await vi.waitFor(() => {
      expect(submitted).toEqual(["中文输入"]);
      expect(terminal.output).toContain("Lyonbrown");
    });
    screen.stop();
  });

  it("keeps global language, palette, activity and exit shortcuts available", () => {
    const terminal = new MemoryTerminal();
    const language = vi.fn();
    const palette = vi.fn();
    const activity = vi.fn();
    const exit = vi.fn();
    const screen = createPiScreen({
      terminal,
      frame: createInitialFrame(),
      onSubmit: () => undefined,
      onLanguageToggle: language,
      onPaletteToggle: palette,
      onActivityToggle: activity,
      onExit: exit,
    });

    screen.start();
    terminal.input("\u001bOQ");
    terminal.input("\u0010");
    terminal.input("\u001b[97;6u");
    terminal.input("\u0003");
    screen.update({
      ...createInitialFrame(),
      ui: {
        ...createInitialFrame().ui,
        preferences: { locale: "zh-CN", themeId: "graphite-coral" },
      },
    });

    expect(language).toHaveBeenCalledOnce();
    expect(palette).toHaveBeenCalledOnce();
    expect(activity).toHaveBeenCalledOnce();
    expect(exit).toHaveBeenCalledOnce();
    screen.stop();
  });

  it("uses Enter or D to explicitly confirm only a fresh allowed model download", async () => {
    const terminal = new MemoryTerminal();
    const submitted: string[] = [];
    const exited = vi.fn();
    const screen = createPiScreen({
      terminal,
      frame: modelSetupFrame("required", ["download"]),
      onSubmit: (text) => {
        submitted.push(text);
      },
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onExit: exited,
    });

    screen.start();
    terminal.input("d");
    terminal.input("x");

    await vi.waitFor(() => expect(submitted).toEqual(["/download"]));
    expect(screen.editorText()).toBe("");
    expect(exited).not.toHaveBeenCalled();
    screen.stop();
  });

  it("maps setup cancellation and exit keys without inventing actions", async () => {
    const terminal = new MemoryTerminal();
    const submitted: string[] = [];
    const exited = vi.fn();
    const screen = createPiScreen({
      terminal,
      frame: modelSetupFrame("downloading", ["cancel"]),
      onSubmit: (text) => {
        submitted.push(text);
      },
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onExit: exited,
    });

    screen.start();
    terminal.input("d");
    terminal.input("c");
    terminal.input("q");

    await vi.waitFor(() => {
      expect(submitted).toEqual(["/cancel"]);
      expect(exited).toHaveBeenCalledOnce();
    });
    screen.stop();
  });

  it("keeps stale setup state visible but blocks daemon mutations", async () => {
    const terminal = new MemoryTerminal();
    const submitted: string[] = [];
    const fresh = modelSetupFrame("required", ["download"]);
    const screen = createPiScreen({
      terminal,
      frame: {
        ...fresh,
        ui: { ...fresh.ui, remote: { ...fresh.ui.remote, connection: "stale" } },
      },
      onSubmit: (text) => {
        submitted.push(text);
      },
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onExit: () => undefined,
    });

    screen.start();
    terminal.input("\r");
    await Promise.resolve();

    expect(submitted).toEqual([]);
    screen.stop();
  });

  it.each([
    [true, true],
    [false, false],
  ])(
    "waits for a pending setup receipt and exits only when its result is %s",
    async (accepted, shouldExit) => {
      const terminal = new MemoryTerminal();
      const exited = vi.fn();
      let settle: ((value: boolean) => void) | undefined;
      const receipt = new Promise<boolean>((resolve) => {
        settle = resolve;
      });
      const screen = createPiScreen({
        terminal,
        frame: modelSetupFrame("required", ["download"]),
        onSubmit: () => receipt,
        onLanguageToggle: () => undefined,
        onPaletteToggle: () => undefined,
        onActivityToggle: () => undefined,
        onExit: exited,
      });

      screen.start();
      terminal.input("d");
      terminal.input("q");
      expect(exited).not.toHaveBeenCalled();

      settle?.(accepted);
      await receipt;
      await Promise.resolve();
      await Promise.resolve();
      expect(exited).toHaveBeenCalledTimes(shouldExit ? 1 : 0);
      screen.stop();
    },
  );
});

function modelSetupFrame(
  phase: "required" | "downloading",
  allowedActions: readonly ("download" | "cancel")[],
): AgentTuiFrame {
  const snapshot = runtimeSnapshot({
    phase: "model-setup-required",
    modelSetup: {
      source: "managed-preset",
      phase,
      managedModel: {
        id: "qwen3-1.7b-q4_k_m",
        displayName: null,
        recommendationReason: null,
        expectedDownloadBytes: 100n,
        maxBytes: 110n,
        minAvailableMemoryBytes: 1n,
        sha256: "a".repeat(64),
      },
      operationId: phase === "downloading" ? "download-1" : null,
      downloadedBytes: 0n,
      totalBytes: 100n,
      failure: null,
      allowedActions,
    },
  });
  let ui: UiModelState = createInitialUiModelState();
  ui = reduceUiModel(ui, { type: "clientOpened" });
  ui = reduceUiModel(ui, { type: "connectRequested" });
  ui = reduceUiModel(ui, {
    type: "handshakeSucceeded",
    serverInstanceId: "11111111-1111-4111-8111-111111111111",
  });
  ui = reduceUiModel(ui, {
    type: "snapshotReceived",
    snapshot: {
      serverInstanceId: "11111111-1111-4111-8111-111111111111",
      revision: snapshot.revision,
      sessionId: snapshot.sessionId ?? "pending-session",
      modelSetup:
        phase === "required"
          ? { phase: "Required", operationId: null }
          : { phase: "Downloading", operationId: "download-1" },
      activities: [],
    },
  });
  return {
    ui,
    snapshot,
    commandPaletteOpen: false,
    activityOpen: false,
    notice: null,
  };
}

class MemoryTerminal implements Terminal {
  public columns = 100;
  public rows = 30;
  public kittyProtocolActive = false;
  public output = "";
  private onInput: ((data: string) => void) | null = null;

  public start(onInput: (data: string) => void): void {
    this.onInput = onInput;
  }

  public stop(): void {
    this.onInput = null;
  }

  public input(data: string): void {
    this.onInput?.(data);
  }

  public async drainInput(): Promise<void> {}
  public write(data: string): void {
    this.output += data;
  }
  public moveBy(): void {}
  public hideCursor(): void {}
  public showCursor(): void {}
  public clearLine(): void {}
  public clearFromCursor(): void {}
  public clearScreen(): void {}
  public setTitle(): void {}
  public setProgress(): void {}
}
