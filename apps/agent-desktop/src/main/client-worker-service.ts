import type { ActionResult, DesktopState } from "@lyonbrown/agent-desktop/internal/shared/contract";
import { workerRequestSchema } from "@lyonbrown/agent-desktop/internal/main/worker-wire";
export interface WorkerBackend {
  connect(): Promise<ActionResult>;
  act(value: unknown): Promise<ActionResult>;
  close(): Promise<void>;
}
export interface WorkerPort {
  connected(): boolean;
  send(value: unknown, done: (error: Error | null) => void): void;
  disconnect(): void;
}
export class ClientWorkerService {
  private readonly backend: WorkerBackend;
  private sending = false;
  private latest: DesktopState | undefined;
  private closed = false;
  private backendClose: Promise<void> | undefined;
  private disposeBackend(): Promise<void> {
    this.backendClose ??= this.backend.close();
    return this.backendClose;
  }
  public constructor(
    factory: (publish: (state: DesktopState) => void) => WorkerBackend,
    private readonly port: WorkerPort,
  ) {
    this.backend = factory((state) => {
      this.publish(state);
    });
  }
  public publish(state: DesktopState): void {
    this.latest = state;
    if (this.sending || this.closed || !this.port.connected()) return;
    this.sending = true;
    const next = this.latest;
    this.latest = undefined;
    this.port.send({ type: "state", state: next }, (error) => {
      this.sending = false;
      if (error) {
        void this.close();
        return;
      }
      if (this.latest) this.publish(this.latest);
    });
  }
  public async receive(value: unknown): Promise<void> {
    if (this.closed) return;
    const parsed = workerRequestSchema.safeParse(value);
    if (!parsed.success) {
      await this.close();
      return;
    }
    const request = parsed.data;
    const result =
      request.type === "connect"
        ? await this.backend.connect()
        : request.type === "action"
          ? await this.backend.act(request.action)
          : (await this.disposeBackend(), { ok: true, error: null });
    if (!this.port.connected()) {
      await this.close();
      return;
    }
    this.port.send({ type: "reply", id: request.id, result }, (error) => {
      if (error || request.type === "close") void this.close();
    });
  }
  public async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    await this.disposeBackend();
    if (this.port.connected()) this.port.disconnect();
  }
}
