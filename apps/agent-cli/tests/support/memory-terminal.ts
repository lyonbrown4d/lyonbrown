import type { Terminal } from "@earendil-works/pi-tui";

export class MemoryTerminal implements Terminal {
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
