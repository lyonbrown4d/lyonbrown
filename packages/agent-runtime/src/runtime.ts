import {
  parseAgentRuntimeEvent,
  validateText,
  type AgentRuntimeState,
} from "@lyonbrown/agent-domain";
import type { KernelEvent } from "@lyonbrown/kernel-pi";

import { RuntimeBackgroundTasks } from "./background-tasks.js";
import { runtimeError } from "./errors.js";
import type { RuntimeEventWriter } from "./event-writer.js";
import { mapKernelEvent } from "./kernel-events.js";
import * as runtimeHelpers from "./runtime-helpers.js";
import { LatestSnapshotFeed } from "./snapshot-feed.js";
import type { AgentRuntime, CreateAgentRuntimeOptions, RuntimeSnapshotFeed } from "./types.js";

export class DefaultAgentRuntime implements AgentRuntime {
  readonly #kernel: ReturnType<CreateAgentRuntimeOptions["createKernelSession"]>;
  readonly #background = new RuntimeBackgroundTasks();
  #running = false;
  #claimPending = false;
  #cancelRequested = false;
  #pendingFollowUps = 0;
  #disposed = false;
  #disposePromise: Promise<void> | undefined;

  constructor(
    private readonly options: CreateAgentRuntimeOptions,
    private readonly writer: RuntimeEventWriter,
    private readonly feeds: Set<LatestSnapshotFeed>,
  ) {
    this.#kernel = options.createKernelSession({
      sessionId: options.sessionId,
      eventSink: { append: (event) => this.#appendKernelEvent(event) },
    });
  }

  snapshot(): AgentRuntimeState {
    return this.writer.state;
  }

  snapshots(): RuntimeSnapshotFeed {
    this.#assertUsable();
    const feed = new LatestSnapshotFeed(this.writer.state, () => {
      this.feeds.delete(feed);
    });
    this.feeds.add(feed);
    return feed;
  }

  async startTurn(input: string, options?: { readonly signal?: AbortSignal }): Promise<void> {
    this.#assertAvailable();
    options?.signal?.throwIfAborted();
    if (this.#running || this.#pendingFollowUps > 0)
      throw runtimeError("SESSION_BUSY", "A runtime turn is already active");
    const text = validateText(input, "input");
    this.#running = true;
    this.#claimPending = true;
    this.#cancelRequested = false;
    try {
      await this.writer.append(
        parseAgentRuntimeEvent({
          type: "user_message_submitted",
          sessionId: this.options.sessionId,
          text,
        }),
      );
    } catch {
      this.#claimPending = false;
      this.#running = false;
      if (this.writer.fatal !== undefined) throw this.writer.fatal;
      throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
    }
    this.#claimPending = false;
    if (this.#disposed) {
      this.#running = false;
      throw runtimeError("RUNTIME_DISPOSED", "Agent runtime is disposed");
    }
    // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition -- cancel can race while append awaits.
    if (this.#cancelRequested) {
      await this.#finishPendingCancellation();
      return;
    }
    if (!this.#launchKernel(text, options)) {
      this.#running = false;
      throw runtimeError("RUNTIME_DISPOSED", "Agent runtime is disposed");
    }
  }

  async steer(input: string): Promise<void> {
    this.#assertAvailable();
    runtimeHelpers.assertActiveControl(this.writer.state, this.#running, this.#claimPending);
    const text = validateText(input, "input");
    try {
      await this.writer.append(
        parseAgentRuntimeEvent({
          type: "steering_submitted",
          sessionId: this.options.sessionId,
          text,
        }),
      );
      if (this.#disposed) return;
      try {
        this.#kernel.steer(text);
      } catch {
        this.#superviseKernelFailure();
      }
    } catch {
      if (this.writer.fatal !== undefined) throw this.writer.fatal;
      throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
    }
  }

  async followUp(input: string): Promise<void> {
    this.#assertAvailable();
    const text = validateText(input, "input");
    this.#pendingFollowUps += 1;
    try {
      await this.writer.append(
        parseAgentRuntimeEvent({
          type: "follow_up_queued",
          sessionId: this.options.sessionId,
          text,
        }),
      );
    } catch {
      if (this.writer.fatal !== undefined) throw this.writer.fatal;
      throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
    } finally {
      this.#pendingFollowUps -= 1;
    }
    this.#resumeQueuedFollowUps();
  }

  cancel(): void {
    this.#assertAvailable();
    runtimeHelpers.assertActiveControl(this.writer.state, this.#running, this.#claimPending);
    const wasRequested = this.#cancelRequested;
    this.#cancelRequested = true;
    try {
      this.#kernel.cancel();
    } catch {
      this.#cancelRequested = wasRequested;
      throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
    }
  }

  dispose(): Promise<void> {
    if (this.#disposePromise !== undefined) return this.#disposePromise;
    this.#disposed = true;
    this.#disposePromise = this.#disposeResources();
    return this.#disposePromise;
  }

  #appendKernelEvent(event: KernelEvent): Promise<void> {
    this.#assertUsable();
    if (this.writer.fatal !== undefined) throw this.writer.fatal;
    if (runtimeHelpers.ignoresLateKernelEvent(this.writer.state, event)) return Promise.resolve();
    return this.writer.append(mapKernelEvent(this.options.sessionId, event));
  }

  #launchKernel(input: string, options?: { readonly signal?: AbortSignal }): boolean {
    return this.#background.track((lifecycleSignal) =>
      this.#runKernel(input, runtimeHelpers.combineRuntimeSignal(options?.signal, lifecycleSignal)),
    );
  }

  async #runKernel(input: string, signal: AbortSignal): Promise<void> {
    if (this.#disposed) return;
    try {
      await this.#kernel.startTurn(input, { signal });
    } catch {
      await runtimeHelpers.recordKernelFailure(this.writer, this.options.sessionId, this.#disposed);
    } finally {
      this.#cancelRequested = false;
      this.#running = false;
      this.#resumeQueuedFollowUps();
    }
  }

  #superviseKernelFailure(): void {
    this.#background.track(async () =>
      runtimeHelpers.recordKernelFailure(this.writer, this.options.sessionId, this.#disposed),
    );
  }

  #resumeQueuedFollowUps(): void {
    if (
      this.#running ||
      this.#disposed ||
      this.writer.fatal !== undefined ||
      this.writer.state.outcomeUnknown ||
      this.writer.state.phase === "failed_terminal"
    ) {
      return;
    }
    const text = this.writer.state.queuedFollowUps[0];
    if (text === undefined) return;
    const status = this.writer.state.turns.at(-1)?.status;
    if (status === "submitted" || status === "running") return;
    this.#running = true;
    this.#claimPending = true;
    this.#cancelRequested = false;
    if (!this.#background.track(async () => this.#claimFollowUp(text))) {
      this.#claimPending = false;
      this.#running = false;
    }
  }

  async #claimFollowUp(text: string): Promise<void> {
    try {
      await this.writer.append(
        parseAgentRuntimeEvent({
          type: "follow_up_started",
          sessionId: this.options.sessionId,
          text,
        }),
      );
    } catch {
      this.#claimPending = false;
      this.#running = false;
      return;
    }
    this.#claimPending = false;
    if (this.#disposed) {
      this.#running = false;
      return;
    }
    if (this.#cancelRequested) {
      await this.#finishPendingCancellation();
      return;
    }
    if (!this.#launchKernel(text)) this.#running = false;
  }

  async #finishPendingCancellation(): Promise<void> {
    try {
      await this.writer.append(
        parseAgentRuntimeEvent({ type: "run_cancelled", sessionId: this.options.sessionId }),
      );
    } finally {
      this.#cancelRequested = false;
      this.#running = false;
      this.#resumeQueuedFollowUps();
    }
  }

  resumeQueuedFollowUps(): void {
    this.#resumeQueuedFollowUps();
  }

  #assertAvailable(): void {
    this.#assertUsable();
    if (this.writer.fatal !== undefined) throw this.writer.fatal;
    if (this.writer.state.outcomeUnknown) {
      throw runtimeError("TERMINAL_SESSION", "Runtime session has an uncertain tool outcome");
    }
    if (this.writer.state.phase === "failed_terminal") {
      throw runtimeError("TERMINAL_SESSION", "Runtime session is terminal");
    }
    if (
      !this.#running &&
      (runtimeHelpers.hasUnresolvedTurn(this.writer.state) ||
        this.writer.state.phase === "running" ||
        this.writer.state.tools.some((tool) => tool.status === "running"))
    ) {
      throw runtimeError("TERMINAL_SESSION", "Runtime session has unresolved work");
    }
  }

  async #disposeResources(): Promise<void> {
    let failed = false;
    try {
      failed = await this.#background.close({
        cancel: () => {
          this.#kernel.cancel();
        },
        dispose: () => {
          this.#kernel.dispose();
        },
        cancelActive: this.#running,
      });
    } finally {
      for (const feed of this.feeds) feed.close();
      this.feeds.clear();
    }
    if (failed) throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
  }

  #assertUsable(): void {
    if (this.#disposed) throw runtimeError("RUNTIME_DISPOSED", "Agent runtime is disposed");
  }
}
