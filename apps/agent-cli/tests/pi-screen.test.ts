import type { Terminal } from "@earendil-works/pi-tui";
import { describe, expect, it, vi } from "vitest";

import { createPiScreen } from "../src/pi-screen.js";
import { createInitialFrame } from "../src/controller.js";

describe("Pi TUI screen", () => {
  it("uses TuiMainScreen focus propagation and preserves Chinese IME input", async () => {
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
    terminal.input("中文输入");
    expect(screen.editorText()).toBe("中文输入");
    terminal.input("\r");

    await vi.waitFor(() => {
      expect(submitted).toEqual(["中文输入"]);
      expect(terminal.output).toContain("Lyonbrown");
    });
    screen.stop();
  });
});

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
