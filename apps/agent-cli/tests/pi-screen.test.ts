import { describe, expect, it, vi } from "vitest";

import { createPiScreen } from "@lyonbrown/agent-cli/internal/pi-screen";
import { createInitialFrame } from "@lyonbrown/agent-cli/internal/controller";
import { MemoryTerminal } from "@lyonbrown/agent-cli/test/support/memory-terminal";
import { runtimeSnapshot } from "@lyonbrown/agent-cli/test/support/runtime";

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
    terminal.input("\u000f");
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

  it("queues editor text with Alt-Enter without submitting a new turn", async () => {
    const terminal = new MemoryTerminal();
    const submitted: string[] = [];
    const screen = createPiScreen({
      terminal,
      frame: createInitialFrame(),
      onSubmit: (text) => submitted.push(text),
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onExit: () => undefined,
    });
    screen.start();
    terminal.input("review the tests");
    terminal.input("\u001b\r");

    await vi.waitFor(() => expect(submitted).toEqual(["/queue review the tests"]));
    expect(screen.editorText()).toBe("");
    screen.stop();
  });

  it("routes approval keys to the safe inline decision instead of the composer", async () => {
    const terminal = new MemoryTerminal();
    const submitted: string[] = [];
    const snapshot = runtimeSnapshot({
      phase: "awaiting-approval",
      pendingApproval: {
        requestId: "approval-1",
        toolName: "shell",
        argumentsJson: "{}",
        stepId: "step-1",
        preview: null,
        deadlineRemainingMs: 5_000n,
        state: "pending",
        approvedBy: null,
      },
    });
    const frame = { ...createInitialFrame(), snapshot };
    const screen = createPiScreen({
      terminal,
      frame,
      onSubmit: (text) => submitted.push(text),
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onExit: () => undefined,
    });
    screen.start();
    terminal.input("d");

    await vi.waitFor(() => expect(submitted).toEqual(["/deny once"]));
    expect(screen.editorText()).toBe("");
    screen.stop();
  });

  it("moves the approval focus and activates the selected safe decision", async () => {
    const terminal = new MemoryTerminal();
    const submitted: string[] = [];
    const snapshot = runtimeSnapshot({
      phase: "awaiting-approval",
      pendingApproval: {
        requestId: "approval-1",
        toolName: "shell",
        argumentsJson: "{}",
        stepId: "step-1",
        preview: null,
        deadlineRemainingMs: 5_000n,
        state: "pending",
        approvedBy: null,
      },
    });
    const screen = createPiScreen({
      terminal,
      frame: { ...createInitialFrame(), snapshot },
      onSubmit: (text) => submitted.push(text),
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onExit: () => undefined,
    });
    screen.start();
    terminal.input("\u001b[B");
    terminal.input("\r");

    await vi.waitFor(() => expect(submitted).toEqual(["/approve once"]));
    screen.stop();
  });

  it("denies approval and cancels the turn on Ctrl-C without exiting", async () => {
    const terminal = new MemoryTerminal();
    const submitted: string[] = [];
    const exited = vi.fn();
    const snapshot = runtimeSnapshot({
      phase: "awaiting-approval",
      pendingApproval: {
        requestId: "approval-1",
        toolName: "shell",
        argumentsJson: "{}",
        stepId: "step-1",
        preview: null,
        deadlineRemainingMs: 5_000n,
        state: "pending",
        approvedBy: null,
      },
    });
    const screen = createPiScreen({
      terminal,
      frame: { ...createInitialFrame(), snapshot },
      onSubmit: (text) => submitted.push(text),
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onExit: exited,
    });
    screen.start();
    terminal.input("\u0003");

    await vi.waitFor(() => expect(submitted).toEqual(["/deny once", "/cancel"]));
    expect(exited).not.toHaveBeenCalled();
    screen.stop();
  });

  it("cancels an active turn, then force-stops from cancelling", async () => {
    const terminal = new MemoryTerminal();
    const submitted: string[] = [];
    const exited = vi.fn();
    const initial = { ...createInitialFrame(), snapshot: runtimeSnapshot({ phase: "streaming" }) };
    const screen = createPiScreen({
      terminal,
      frame: initial,
      onSubmit: (text) => submitted.push(text),
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onExit: exited,
    });
    screen.start();
    terminal.input("\u0003");
    screen.update({ ...initial, snapshot: runtimeSnapshot({ phase: "cancelling" }) });
    terminal.input("\u0003");

    await vi.waitFor(() => expect(submitted).toEqual(["/cancel", "/force-stop"]));
    await vi.waitFor(() => expect(exited).toHaveBeenCalledOnce());
    screen.stop();
  });

  it("opens help with question mark only when the composer is empty", async () => {
    const terminal = new MemoryTerminal();
    const submitted: string[] = [];
    const screen = createPiScreen({
      terminal,
      frame: createInitialFrame(),
      onSubmit: (text) => submitted.push(text),
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onExit: () => undefined,
    });
    screen.start();
    terminal.input("?");

    await vi.waitFor(() => expect(submitted).toEqual(["/help"]));
    expect(screen.editorText()).toBe("");
    screen.stop();
  });

  it("follows the latest activity and pins an earlier Inspector selection", async () => {
    const terminal = new MemoryTerminal();
    const activities = ["first", "second"].map((reason, index) => ({
      stepId: `step-${String(index + 1)}`,
      phase: "completed" as const,
      route: {
        selected: { provider: "local", model: `model-${String(index + 1)}` },
        reason: `${reason} route`,
        fallbacks: [],
      },
      context: null,
      assistantText: "",
      usage: null,
      tools: [],
      outcome: null,
    }));
    const frame = {
      ...createInitialFrame(),
      snapshot: runtimeSnapshot({
        turns: [
          {
            turnId: "turn-1",
            userText: "inspect",
            assistantText: "",
            status: "running" as const,
            steering: [],
            activities,
          },
        ],
      }),
      overlay: { kind: "activity" as const },
    };
    const screen = createPiScreen({
      terminal,
      frame: createInitialFrame(),
      onSubmit: () => undefined,
      onLanguageToggle: () => undefined,
      onPaletteToggle: () => undefined,
      onActivityToggle: () => undefined,
      onOverlayClose: () => undefined,
      onExit: () => undefined,
    });
    screen.start();
    screen.update(frame);
    await vi.waitFor(() => expect(terminal.output).toContain("second route"));

    terminal.output = "";
    terminal.input("\u001b[A");

    await vi.waitFor(() => expect(terminal.output).toContain("first route"));
    screen.stop();
  });
});
