import type { AgentClient, AgentCommand, RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import {
  getChatSubmitAvailability,
  getDaemonMutationAvailability,
  reduceUiModel,
  type UiModelAction,
  type ThemeId,
} from "@lyonbrown/ui-model";
import type { Subscription } from "rxjs";

import { parseInput, type ParsedInput } from "@lyonbrown/agent-cli/internal/commands";
import {
  assertSelectableModel,
  cancelCommand,
  isActiveTurn,
  modelDownloadCommand,
  publicMessage,
} from "@lyonbrown/agent-cli/internal/controller-helpers";
import {
  createInitialFrame,
  type AgentTuiControllerOptions,
  type AgentTuiFrame,
} from "@lyonbrown/agent-cli/internal/controller-state";
import { projectRemoteSnapshot } from "@lyonbrown/agent-cli/internal/projection";
import { decisionModelCommand } from "@lyonbrown/agent-cli/internal/decision-model-commands";
import { observeRuntimeSnapshots } from "@lyonbrown/agent-cli/internal/runtime-snapshot-stream";

export { createInitialFrame } from "@lyonbrown/agent-cli/internal/controller-state";
export type {
  AgentTuiControllerOptions,
  AgentTuiFrame,
} from "@lyonbrown/agent-cli/internal/controller-state";

export class AgentTuiController {
  private frame: AgentTuiFrame;
  private client: AgentClient | null = null;
  private eventAbort: AbortController | null = null;
  private eventSubscription: Subscription | null = null;
  private generation = 0;
  private closed = false;

  public constructor(private readonly options: AgentTuiControllerOptions) {
    this.frame = createInitialFrame(options.initialPreferences);
  }

  public currentFrame(): AgentTuiFrame {
    return this.frame;
  }

  public open(): void {
    if (this.closed) return;
    this.dispatch({ type: "clientOpened" });
    this.dispatch({ type: "connectRequested" });
    void this.connect(++this.generation);
  }

  public toggleLanguage(): void {
    const locale = this.frame.ui.preferences.locale === "en-US" ? "zh-CN" : "en-US";
    this.dispatch({ type: "localeChanged", locale });
    void this.persistPreferences().catch((error: unknown) => {
      this.update({ notice: publicMessage(error, this.frame.ui.preferences.locale) });
    });
  }

  public previewTheme(themeId: ThemeId): void {
    this.dispatch({ type: "themeChanged", themeId });
  }

  public togglePalette(): void {
    const open = this.frame.overlay?.kind !== "commands";
    this.update({ overlay: open ? { kind: "commands" } : null });
  }

  public toggleActivity(): void {
    const open = this.frame.overlay?.kind !== "activity";
    this.update({ overlay: open ? { kind: "activity" } : null });
  }

  public closeOverlay(): void {
    this.update({ overlay: null });
  }

  public async submit(raw: string): Promise<boolean> {
    try {
      await this.handle(parseInput(raw));
      this.update({ notice: null });
      return true;
    } catch (error: unknown) {
      this.update({ notice: publicMessage(error, this.frame.ui.preferences.locale) });
      return false;
    }
  }

  public async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.generation += 1;
    this.eventAbort?.abort();
    this.eventAbort = null;
    this.eventSubscription?.unsubscribe();
    this.eventSubscription = null;
    const client = this.client;
    this.client = null;
    if (client !== null) await client.close();
  }

  private async connect(generation: number): Promise<void> {
    try {
      const connection = await this.options.launcher.connect();
      if (!this.isCurrent(generation)) {
        await connection.client.close();
        return;
      }
      this.client = connection.client;
      this.dispatch({ type: "handshakeSucceeded", serverInstanceId: connection.serverInstanceId });
      this.acceptSnapshot(connection.serverInstanceId, connection.snapshot, generation);
      const abort = new AbortController();
      this.eventAbort = abort;
      this.eventSubscription = observeRuntimeSnapshots(
        connection.client,
        connection.snapshot.revision,
        abort.signal,
      ).subscribe({
        next: (snapshot) => {
          if (!this.isCurrent(generation) || abort.signal.aborted) return;
          this.acceptSnapshot(connection.serverInstanceId, snapshot, generation);
        },
        complete: () => {
          this.handleEventStreamTermination(generation, abort, "event stream ended");
        },
        error: () => {
          this.handleEventStreamTermination(generation, abort, "event stream unavailable");
        },
      });
    } catch {
      if (this.isCurrent(generation)) {
        this.dispatch({ type: "connectionUnavailable", reason: "daemon unavailable" });
      }
    }
  }

  private handleEventStreamTermination(
    generation: number,
    abort: AbortController,
    reason: string,
  ): void {
    if (this.isCurrent(generation) && !abort.signal.aborted) {
      this.dispatch({ type: "connectionLost", reason });
    }
  }

  private acceptSnapshot(
    serverInstanceId: string,
    snapshot: RuntimeSnapshotDto,
    generation: number,
  ): void {
    if (!this.isCurrent(generation)) return;
    if (this.frame.snapshot !== null && snapshot.revision <= this.frame.snapshot.revision) return;
    const sessionChanged =
      this.frame.snapshot !== null && this.frame.snapshot.sessionId !== snapshot.sessionId;
    if (sessionChanged && this.frame.overlay?.kind === "activity") {
      this.frame = { ...this.frame, overlay: null };
    }
    this.frame = { ...this.frame, snapshot };
    this.dispatch({
      type: "snapshotReceived",
      snapshot: projectRemoteSnapshot(serverInstanceId, snapshot),
    });
    this.syncApproval(snapshot);
  }

  private syncApproval(snapshot: RuntimeSnapshotDto): void {
    const current = this.frame.ui.approval;
    const pending = snapshot.pendingApproval;
    if (pending !== null && current?.request.requestId !== pending.requestId) {
      if (this.frame.overlay !== null) this.update({ overlay: null });
      this.dispatch({
        type: "approvalRequested",
        request: { requestId: pending.requestId, summary: pending.toolName },
      });
    } else if (pending === null && current !== null) {
      this.dispatch({
        type: "approvalResolved",
        requestId: current.request.requestId,
        serverInstanceId: current.serverInstanceId,
      });
    }
  }

  private async handle(input: ParsedInput): Promise<void> {
    switch (input.kind) {
      case "language": {
        const locale =
          input.locale ?? (this.frame.ui.preferences.locale === "en-US" ? "zh-CN" : "en-US");
        this.dispatch({
          type: "localeChanged",
          locale,
        });
        await this.persistPreferences();
        return;
      }
      case "theme":
        this.dispatch({ type: "themeChanged", themeId: input.themeId });
        await this.persistPreferences();
        return;
      case "model":
        assertSelectableModel(this.frame.snapshot, input.selection);
        return this.enqueueFresh({ type: "select-session-model", selection: input.selection });
      case "control":
        this.update({ overlay: { kind: "control", tab: input.tab } });
        return;
      case "palette":
        this.togglePalette();
        return;
      case "activity":
        this.toggleActivity();
        return;
      case "help":
        this.update({ overlay: { kind: "help" } });
        return;
      case "status":
        this.update({ overlay: { kind: "status" } });
        return;
      case "sessions":
        this.update({ overlay: { kind: "sessions" } });
        return;
      case "unavailable":
        throw new Error(`/${input.command} is not available yet`);
      case "quit":
        this.options.requestExit?.();
        return;
      case "chat":
        if (!getChatSubmitAvailability(this.frame.ui).enabled) {
          throw new Error("chat is unavailable until the daemon and model are ready");
        }
        return this.enqueue(
          isActiveTurn(this.frame.snapshot?.phase)
            ? { type: "steer-current-turn", text: input.text }
            : { type: "start-turn", text: input.text },
        );
      case "steer":
        return this.enqueueFresh({ type: "steer-current-turn", text: input.text });
      case "follow-up":
        return this.enqueueFresh({ type: "queue-follow-up", text: input.text });
      case "cancel":
        return this.enqueueFresh(cancelCommand(this.frame.snapshot));
      case "force-stop":
        return this.enqueueFresh({ type: "force-stop" });
      case "download":
      case "retry":
        return this.enqueueFresh(modelDownloadCommand(this.frame.snapshot, input.kind));
      case "decision-action":
        return this.enqueueFresh(decisionModelCommand(this.frame.snapshot, input.action));
      case "approve":
        return this.respondApproval(input.scope === "once" ? "allow-once" : "allow-for-session");
      case "deny":
        return this.respondApproval(input.scope === "once" ? "deny-once" : "deny-remaining");
    }
  }

  private async respondApproval(
    decision: "allow-once" | "allow-for-session" | "deny-once" | "deny-remaining",
  ): Promise<void> {
    const approval = this.frame.snapshot?.pendingApproval;
    if (approval === null || approval === undefined) throw new Error("no approval is pending");
    return this.enqueueFresh({ type: "respond-approval", requestId: approval.requestId, decision });
  }

  private async enqueueFresh(command: AgentCommand): Promise<void> {
    if (!getDaemonMutationAvailability(this.frame.ui).enabled) {
      throw new Error("daemon state is stale");
    }
    return this.enqueue(command);
  }

  private async enqueue(command: AgentCommand): Promise<void> {
    if (this.client === null) throw new Error("daemon is not connected");
    await this.client.enqueue(command);
  }

  private async persistPreferences(): Promise<void> {
    await this.options.preferenceStore?.save(this.frame.ui.preferences);
  }

  private dispatch(action: UiModelAction): void {
    this.frame = { ...this.frame, ui: reduceUiModel(this.frame.ui, action) };
    this.options.render(this.frame);
  }

  private update(update: Partial<AgentTuiFrame>): void {
    this.frame = { ...this.frame, ...update };
    this.options.render(this.frame);
  }

  private isCurrent(generation: number): boolean {
    return !this.closed && generation === this.generation;
  }
}
