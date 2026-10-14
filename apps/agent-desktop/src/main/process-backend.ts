import type { ChildProcess } from "node:child_process";
import type { ActionResult, DesktopState } from "@lyonbrown/agent-desktop/internal/shared/contract";
import { disconnectedState } from "@lyonbrown/agent-desktop/internal/main/projection";
import { workerMessageSchema } from "@lyonbrown/agent-desktop/internal/main/worker-wire";
import { spawnClientWorker } from "@lyonbrown/agent-desktop/internal/main/worker-process";
import { parseAction } from "@lyonbrown/agent-desktop/internal/main/security";
export class DesktopProcessBackend {
  private view: DesktopState = disconnectedState();
  private child: ChildProcess | undefined;
  private sequence = 0;
  private closed = false;
  private readonly retired = new WeakSet<ChildProcess>();
  private retire(child: ChildProcess): void {
    this.retired.add(child);
    this.unavailable();
    child.kill();
  }
  private readonly pending = new Map<string, (result: ActionResult) => void>();
  public constructor(
    private readonly publish: (state: DesktopState) => void,
    private readonly spawnWorker: () => ChildProcess = spawnClientWorker,
  ) {
    this.startWorker();
  }
  private startWorker(): void {
    try {
      const child = this.spawnWorker();
      this.child = child;
      child.on("message", (value: unknown) => {
        if (this.child !== child || this.closed || this.retired.has(child)) return;
        const parsed = workerMessageSchema.safeParse(value);
        if (!parsed.success) {
          this.retire(child);
          return;
        }
        const message = parsed.data;
        if (message.type === "state") {
          this.view = message.state;
          this.publish(this.view);
        } else {
          this.pending.get(message.id)?.(message.result);
          this.pending.delete(message.id);
        }
      });
      child.once("error", () => {
        if (this.child === child) {
          this.child = undefined;
          this.unavailable();
        }
      });
      child.once("exit", () => {
        if (this.child === child) {
          this.child = undefined;
          this.unavailable();
        }
      });
    } catch {
      this.child = undefined;
      this.unavailable();
    }
  }
  public state(): DesktopState {
    return this.view;
  }
  private unavailable(): void {
    this.view = {
      ...this.view,
      status: "disconnected",
      canSend: false,
      canCancel: false,
      actionToken: null,
      error: "BACKEND_UNAVAILABLE",
    };
    this.publish(this.view);
    for (const finish of this.pending.values()) finish({ ok: false, error: "BACKEND_UNAVAILABLE" });
    this.pending.clear();
  }
  private request(type: "connect" | "action" | "close", action?: unknown): Promise<ActionResult> {
    const child = this.child;
    if (
      !child?.connected ||
      this.retired.has(child) ||
      this.pending.size >= 16 ||
      (this.closed && type !== "close")
    )
      return Promise.resolve({ ok: false, error: "BACKEND_UNAVAILABLE" });
    const id = String(++this.sequence);
    return new Promise((done) => {
      const deadline = setTimeout(
        () => {
          this.pending.delete(id);
          this.retire(child);
          done({ ok: false, error: "BACKEND_UNAVAILABLE" });
        },
        type === "close" ? 2_000 : 35_000,
      );
      this.pending.set(id, (result) => {
        clearTimeout(deadline);
        done(result);
      });
      child.send({ type, id, ...(type === "action" ? { action } : {}) }, (error: Error | null) => {
        if (error) {
          this.pending.get(id)?.({ ok: false, error: "BACKEND_UNAVAILABLE" });
          this.pending.delete(id);
        }
      });
    });
  }
  public connect(): Promise<ActionResult> {
    if (this.closed) return Promise.resolve({ ok: false, error: "CLOSED" });
    if (!this.child) this.startWorker();
    return this.request("connect");
  }
  public act(action: unknown): Promise<ActionResult> {
    try {
      return this.request("action", parseAction(action));
    } catch {
      return Promise.resolve({ ok: false, error: "INVALID_ACTION" });
    }
  }
  public async close(): Promise<void> {
    if (this.closed) return;
    const child = this.child;
    await this.request("close");
    this.closed = true;
    if (child === undefined) return;
    if (child.exitCode !== null || child.signalCode !== null) return;
    await new Promise<void>((done) => {
      const deadline = setTimeout(() => {
        child.kill();
        done();
      }, 2_000);
      child.once("exit", () => {
        clearTimeout(deadline);
        done();
      });
    });
  }
}
