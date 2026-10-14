import { randomUUID } from "node:crypto";
import type { AgentClient, RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import type { AgentLauncher } from "@lyonbrown/agent-launch";
import type { ActionResult, DesktopState } from "@lyonbrown/agent-desktop/internal/shared/contract";
import { commandFor } from "@lyonbrown/agent-desktop/internal/main/actions";
import {
  disconnectedState,
  projectSnapshot,
} from "@lyonbrown/agent-desktop/internal/main/projection";
import { parseAction } from "@lyonbrown/agent-desktop/internal/main/security";

export class DesktopBackend {
  private view: DesktopState = disconnectedState();
  private snapshot: RuntimeSnapshotDto | null = null;
  private client: AgentClient | null = null;
  private abort: AbortController | null = null;
  private closed = false;
  private connecting = false;
  private busy = false;
  private generation = 0;
  private connectionToken = randomUUID();
  public constructor(
    private readonly launcher: AgentLauncher,
    private readonly publish: (state: DesktopState) => void,
  ) {}
  public state(): DesktopState {
    return this.view;
  }
  private isCurrent(generation: number): boolean {
    return !this.closed && generation === this.generation;
  }
  private update(view: DesktopState): void {
    this.view = view;
    this.publish(view);
  }
  public async connect(): Promise<ActionResult> {
    if (this.closed || this.connecting || this.busy) return { ok: false, error: "BUSY" };
    this.connecting = true;
    const generation = ++this.generation;
    this.connectionToken = randomUUID();
    this.abort?.abort();
    const old = this.client;
    this.client = null;
    const abort = new AbortController();
    this.abort = abort;
    this.update({
      ...this.view,
      status: "connecting",
      canSend: false,
      canCancel: false,
      error: null,
    });
    try {
      await old?.close();
      const connection = await this.launcher.connect({ signal: abort.signal });
      if (!this.isCurrent(generation)) {
        await connection.client.close();
        return { ok: false, error: "CLOSED" };
      }
      this.client = connection.client;
      this.snapshot = connection.snapshot;
      this.update(projectSnapshot(connection.snapshot, connection.source, this.connectionToken));
      void this.consume(connection.client, connection.source, generation, abort);
      return { ok: true, error: null };
    } catch {
      if (this.isCurrent(generation))
        this.update({ ...this.view, status: "disconnected", error: "BACKEND_UNAVAILABLE" });
      return { ok: false, error: "BACKEND_UNAVAILABLE" };
    } finally {
      this.connecting = false;
    }
  }
  private async consume(
    client: AgentClient,
    source: "reused" | "spawned",
    generation: number,
    abort: AbortController,
  ): Promise<void> {
    try {
      for await (const event of client.subscribeEvents({
        afterRevision: this.snapshot?.revision ?? 0n,
        signal: abort.signal,
      })) {
        if (abort.signal.aborted || this.closed || generation !== this.generation) return;
        if (event.snapshot.revision <= (this.snapshot?.revision ?? -1n)) continue;
        this.snapshot = event.snapshot;
        this.update(projectSnapshot(event.snapshot, source, this.connectionToken));
      }
    } catch {
      /* Public state below intentionally omits transport details. */
    }
    if (!abort.signal.aborted && !this.closed && generation === this.generation)
      this.update({
        ...this.view,
        status: "disconnected",
        canSend: false,
        canCancel: false,
        error: "CONNECTION_LOST",
      });
  }
  public async act(value: unknown): Promise<ActionResult> {
    if (this.busy) return { ok: false, error: "BUSY" };
    let action;
    try {
      action = parseAction(value);
    } catch {
      return { ok: false, error: "INVALID_ACTION" };
    }
    if (this.closed || this.view.status !== "connected" || !this.client || !this.snapshot)
      return { ok: false, error: "BACKEND_UNAVAILABLE" };
    if (action.token !== this.view.actionToken) return { ok: false, error: "STALE_STATE" };
    this.busy = true;
    try {
      await this.client.enqueue(commandFor(action, this.snapshot));
      return { ok: true, error: null };
    } catch {
      return { ok: false, error: "ACTION_REJECTED" };
    } finally {
      this.busy = false;
    }
  }
  public async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    ++this.generation;
    this.abort?.abort();
    const client = this.client;
    this.client = null;
    this.update({ ...this.view, status: "disconnected", canSend: false, canCancel: false });
    await client?.close();
  }
}
