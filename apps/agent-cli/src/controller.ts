import type { AgentClient, AgentCommand, RuntimeSnapshotDto } from "@lyonbrown/agent-client";
import type { AgentLauncher } from "@lyonbrown/agent-launch";
import {
  createInitialUiModelState,
  getChatSubmitAvailability,
  getDaemonMutationAvailability,
  reduceUiModel,
  type UiModelAction,
  type UiModelState,
} from "@lyonbrown/ui-model";

import { parseInput, type ParsedInput } from "./commands.js";
import { projectRemoteSnapshot } from "./projection.js";

export interface AgentTuiFrame {
  readonly ui: UiModelState;
  readonly snapshot: RuntimeSnapshotDto | null;
  readonly commandPaletteOpen: boolean;
  readonly activityOpen: boolean;
  readonly notice: string | null;
}

export interface AgentTuiControllerOptions {
  readonly launcher: AgentLauncher;
  readonly render: (frame: AgentTuiFrame) => void;
}

export function createInitialFrame(): AgentTuiFrame {
  return {
    ui: createInitialUiModelState(),
    snapshot: null,
    commandPaletteOpen: false,
    activityOpen: false,
    notice: null,
  };
}

export class AgentTuiController {
  private frame = createInitialFrame();
  private client: AgentClient | null = null;
  private subscription: AbortController | null = null;
  private generation = 0;
  private closed = false;

  public constructor(private readonly options: AgentTuiControllerOptions) {}

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
  }

  public togglePalette(): void {
    this.update({ commandPaletteOpen: !this.frame.commandPaletteOpen });
  }

  public toggleActivity(): void {
    this.update({ activityOpen: !this.frame.activityOpen });
  }

  public async submit(raw: string): Promise<void> {
    try {
      await this.handle(parseInput(raw));
      this.update({ notice: null, commandPaletteOpen: false });
    } catch (error: unknown) {
      this.update({ notice: publicMessage(error) });
    }
  }

  public async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.generation += 1;
    this.subscription?.abort();
    this.subscription = null;
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
      this.subscription = new AbortController();
      void this.consumeEvents(
        connection.client,
        connection.serverInstanceId,
        connection.snapshot.revision,
        generation,
        this.subscription,
      );
    } catch {
      if (this.isCurrent(generation)) {
        this.dispatch({ type: "connectionUnavailable", reason: "daemon unavailable" });
      }
    }
  }

  private async consumeEvents(
    client: AgentClient,
    serverInstanceId: string,
    afterRevision: bigint,
    generation: number,
    abort: AbortController,
  ): Promise<void> {
    try {
      for await (const event of client.subscribeEvents({
        afterRevision,
        signal: abort.signal,
      })) {
        if (!this.isCurrent(generation) || abort.signal.aborted) return;
        this.acceptSnapshot(serverInstanceId, event.snapshot, generation);
      }
      if (this.isCurrent(generation) && !abort.signal.aborted) {
        this.dispatch({ type: "connectionLost", reason: "event stream ended" });
      }
    } catch {
      if (this.isCurrent(generation) && !abort.signal.aborted) {
        this.dispatch({ type: "connectionLost", reason: "event stream unavailable" });
      }
    }
  }

  private acceptSnapshot(
    serverInstanceId: string,
    snapshot: RuntimeSnapshotDto,
    generation: number,
  ): void {
    if (!this.isCurrent(generation)) return;
    if (this.frame.snapshot !== null && snapshot.revision <= this.frame.snapshot.revision) return;
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
      case "language":
        this.dispatch({
          type: "localeChanged",
          locale:
            input.locale ?? (this.frame.ui.preferences.locale === "en-US" ? "zh-CN" : "en-US"),
        });
        return;
      case "theme":
        this.dispatch({ type: "themeChanged", themeId: input.themeId });
        return;
      case "palette":
        this.togglePalette();
        return;
      case "activity":
        this.toggleActivity();
        return;
      case "help":
        this.update({ commandPaletteOpen: true });
        return;
      case "chat":
        if (!getChatSubmitAvailability(this.frame.ui).enabled) {
          throw new Error("chat is unavailable until the daemon and model are ready");
        }
        return this.enqueue({ type: "start-turn", text: input.text });
      case "steer":
        return this.enqueueFresh({ type: "steer-current-turn", text: input.text });
      case "follow-up":
        return this.enqueueFresh({ type: "queue-follow-up", text: input.text });
      case "cancel":
        return this.cancelCurrentOperation();
      case "download":
      case "retry":
        return this.startDownload(input.kind);
      case "approve":
        return this.respondApproval(input.scope === "once" ? "allow-once" : "allow-for-session");
      case "deny":
        return this.respondApproval(input.scope === "once" ? "deny-once" : "deny-remaining");
    }
  }

  private async cancelCurrentOperation(): Promise<void> {
    const setup = this.frame.snapshot?.modelSetup;
    if (setup?.operationId !== null && setup?.operationId !== undefined) {
      if (setup.allowedActions.includes("cancel")) {
        return this.enqueueFresh({ type: "cancel-model-download", operationId: setup.operationId });
      }
    }
    return this.enqueueFresh({ type: "cancel-turn" });
  }

  private async startDownload(action: "download" | "retry"): Promise<void> {
    const setup = this.frame.snapshot?.modelSetup;
    const modelId = setup?.managedModel?.id;
    if (modelId === undefined || setup?.allowedActions.includes(action) !== true) {
      throw new Error(`model ${action} is not available`);
    }
    return this.enqueueFresh({ type: "start-model-download", modelId });
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

function publicMessage(error: unknown): string {
  return error instanceof Error ? error.message : "request failed";
}
