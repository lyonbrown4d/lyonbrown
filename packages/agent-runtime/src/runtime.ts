import { randomUUID } from "node:crypto";

import {
  parseAgentRuntimeEvent,
  validateText,
  type AgentRuntimeState,
} from "@lyonbrown/agent-domain";
import type { KernelEvent } from "@lyonbrown/kernel-pi";

import { runtimeError } from "./errors.js";
import { RuntimeEventWriter } from "./event-writer.js";
import { mapKernelEvent } from "./kernel-events.js";
import { recoverRuntimeState } from "./recovery.js";
import { LatestSnapshotFeed } from "./snapshot-feed.js";
import type { AgentRuntime, CreateAgentRuntimeOptions, RuntimeSnapshotFeed } from "./types.js";

class DefaultAgentRuntime implements AgentRuntime {
  readonly #kernel: ReturnType<CreateAgentRuntimeOptions["createKernelSession"]>;
  #running = false;
  #disposed = false;

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
    if (this.#running) throw runtimeError("SESSION_BUSY", "A runtime turn is already active");
    const text = validateText(input, "input");
    this.#running = true;
    try {
      await this.writer.append(
        parseAgentRuntimeEvent({
          type: "user_message_submitted",
          sessionId: this.options.sessionId,
          text,
        }),
      );
      this.#launchKernel(text, options);
    } catch {
      this.#running = false;
      if (this.writer.fatal !== undefined) throw this.writer.fatal;
      throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
    }
  }

  async steer(input: string): Promise<void> {
    this.#assertAvailable();
    if (!this.#running)
      throw runtimeError("TERMINAL_SESSION", "Runtime session has no active turn");
    const text = validateText(input, "input");
    try {
      await this.writer.append(
        parseAgentRuntimeEvent({
          type: "steering_submitted",
          sessionId: this.options.sessionId,
          text,
        }),
      );
      this.#kernel.steer(text);
    } catch {
      if (this.writer.fatal !== undefined) throw this.writer.fatal;
      throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
    }
  }

  async followUp(input: string): Promise<void> {
    this.#assertAvailable();
    const text = validateText(input, "input");
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
    }
    this.#resumeQueuedFollowUps();
  }

  cancel(): void {
    this.#assertAvailable();
    try {
      this.#kernel.cancel();
    } catch {
      throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
    }
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    let disposeFailed = false;
    try {
      this.#kernel.dispose();
    } catch {
      disposeFailed = true;
    } finally {
      for (const feed of this.feeds) feed.close();
      this.feeds.clear();
    }
    if (disposeFailed) throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
  }

  #appendKernelEvent(event: KernelEvent): Promise<void> {
    this.#assertUsable();
    if (this.writer.fatal !== undefined) throw this.writer.fatal;
    return this.writer.append(mapKernelEvent(this.options.sessionId, event));
  }

  #launchKernel(input: string, options?: { readonly signal?: AbortSignal }): void {
    void this.#runKernel(input, options);
  }

  async #runKernel(input: string, options?: { readonly signal?: AbortSignal }): Promise<void> {
    try {
      await this.#kernel.startTurn(input, options);
    } catch {
      await this.#recordKernelFailure();
    } finally {
      this.#running = false;
      this.#resumeQueuedFollowUps();
    }
  }

  async #recordKernelFailure(): Promise<void> {
    if (this.#disposed || this.writer.fatal !== undefined || this.writer.state.outcomeUnknown)
      return;
    const status = this.writer.state.turns.at(-1)?.status;
    if (status !== "submitted" && status !== "running") return;
    try {
      await this.writer.append(
        parseAgentRuntimeEvent({
          type: "run_failed",
          sessionId: this.options.sessionId,
          code: "KERNEL_FAILURE",
          message: "Kernel session failed",
        }),
      );
    } catch {
      // The writer retains a sanitized sticky failure for the next command boundary.
    }
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
    void this.#claimFollowUp(text);
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
      this.#running = false;
      return;
    }
    this.#launchKernel(text);
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
    if (
      !this.#running &&
      (this.writer.state.phase === "running" ||
        this.writer.state.tools.some((tool) => tool.status === "running"))
    ) {
      throw runtimeError("TERMINAL_SESSION", "Runtime session has unresolved work");
    }
  }

  #assertUsable(): void {
    if (this.#disposed) throw runtimeError("RUNTIME_DISPOSED", "Agent runtime is disposed");
  }
}

export async function createAgentRuntime(
  options: CreateAgentRuntimeOptions,
): Promise<AgentRuntime> {
  const state = await recoverRuntimeState(options.sessionId, options.eventStore);
  const feeds = new Set<LatestSnapshotFeed>();
  const writer = new RuntimeEventWriter({
    sessionId: options.sessionId,
    eventStore: options.eventStore,
    initialState: state,
    createEventId: options.createEventId ?? randomUUID,
    now: options.now ?? (() => new Date().toISOString()),
    publish: (snapshot) => {
      for (const feed of feeds) feed.publish(snapshot);
    },
  });
  if (state.phase === "uninitialized") {
    await writer.append(
      parseAgentRuntimeEvent({ type: "session_opened", sessionId: options.sessionId }),
    );
  }
  try {
    const runtime = new DefaultAgentRuntime(options, writer, feeds);
    runtime.resumeQueuedFollowUps();
    return runtime;
  } catch {
    throw runtimeError("KERNEL_FAILURE", "Kernel session failed");
  }
}
