import { Agent, type AgentEvent } from "@earendil-works/pi-agent-core";
import type { AssistantMessage } from "@earendil-works/pi-ai";

import { BudgetTracker, validateBudget } from "./budget.js";
import { KernelRuntimeError } from "./errors.js";
import { createPiStreamFunction } from "./pi-bridge.js";
import { createPiModel } from "./pi-conversion.js";
import { SecretRedactor } from "./redaction.js";
import { limitsFromBudget, sanitizeBoundedText, sanitizeText } from "./sanitization.js";
import { normalizeTools } from "./tool-normalization.js";
import { ToolExecutor } from "./tools.js";
import type {
  KernelEvent,
  KernelEventListener,
  KernelSession,
  PiKernelSessionOptions,
} from "./types.js";

export class PiKernelSession implements KernelSession {
  readonly #listeners = new Set<KernelEventListener>();
  readonly #agent: Agent;
  readonly #activeTools = new Set<string>();
  #tracker: BudgetTracker;
  #fatalError: KernelRuntimeError | undefined;
  #sinkFatal: KernelRuntimeError | undefined;
  #running = false;
  #cancelled = false;
  #turn = 0;

  constructor(private readonly options: PiKernelSessionOptions) {
    validateBudget(options.budget);
    const limits = limitsFromBudget(options.budget);
    const outputLimits = limitsFromBudget(
      options.budget,
      options.budget.maxOutputBytes,
      "OUTPUT_BUDGET_EXCEEDED",
    );
    const systemPrompt = sanitizeBoundedText(options.systemPrompt, options.budget.maxInputBytes);
    const registeredTools = normalizeTools(options.toolRegistry.list(), options);
    const toolByName = new Map(registeredTools.map((tool) => [tool.name, tool]));
    this.#tracker = new BudgetTracker(options.budget);
    const redactor = new SecretRedactor();
    const toolExecutor = new ToolExecutor(
      options,
      () => this.#tracker,
      redactor,
      this.emit.bind(this),
      this.markFatal.bind(this),
    );
    const tools = registeredTools.map((tool) => toolExecutor.createAgentTool(tool));
    this.#agent = new Agent({
      initialState: {
        systemPrompt,
        model: createPiModel(options.model),
        tools,
      },
      streamFn: createPiStreamFunction({
        sessionId: options.sessionId,
        adapter: options.model,
        tracker: () => this.#tracker,
        limits,
        outputLimits,
        tools: registeredTools.map((tool) => ({
          name: tool.name,
          description: tool.description,
          inputSchema: tool.inputSchema,
        })),
        hasTool: (name) => toolByName.has(name),
        onUnknownTool: toolExecutor.reportUnknown.bind(toolExecutor),
        onFatal: this.markFatal.bind(this),
      }),
      toolExecution: "sequential",
      steeringMode: "one-at-a-time",
      followUpMode: "one-at-a-time",
      sessionId: options.sessionId,
    });
    this.#agent.subscribe((event) => this.handlePiEvent(event));
  }

  async startTurn(input: string, options?: { readonly signal?: AbortSignal }): Promise<void> {
    options?.signal?.throwIfAborted();
    if (this.#sinkFatal) throw this.#sinkFatal;
    if (this.#running) throw new KernelRuntimeError("SESSION_BUSY", "A turn is already active");
    this.#tracker = new BudgetTracker(this.options.budget);
    this.#fatalError = undefined;
    this.#activeTools.clear();
    this.#cancelled = false;
    this.#turn = 0;
    const safeInput = sanitizeBoundedText(input, this.options.budget.maxInputBytes);
    this.#tracker.input(safeInput);
    this.#running = true;
    const externalAbort = (): void => {
      this.cancel();
    };
    options?.signal?.addEventListener("abort", externalAbort, { once: true });
    try {
      const run = this.#agent.prompt(safeInput);
      if (isSignalAborted(options?.signal)) this.cancel();
      await run;
      const fatalError = this.currentFatalError();
      if (fatalError) throw fatalError;
    } catch (error: unknown) {
      throw this.currentFatalError() ?? toPublicError(error);
    } finally {
      options?.signal?.removeEventListener("abort", externalAbort);
      this.#running = false;
    }
  }

  steer(input: string): void {
    const safeInput = this.safeControlInput(input);
    this.#agent.steer({ role: "user", content: safeInput, timestamp: Date.now() });
  }

  followUp(input: string): void {
    const safeInput = this.safeControlInput(input);
    this.#agent.followUp({ role: "user", content: safeInput, timestamp: Date.now() });
  }

  cancel(): void {
    this.#cancelled = true;
    this.#agent.abort();
  }

  subscribe(listener: KernelEventListener): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  private safeControlInput(input: string): string {
    const safe = sanitizeBoundedText(input, this.options.budget.maxInputBytes);
    this.#tracker.input(safe);
    return safe;
  }

  private currentFatalError(): KernelRuntimeError | undefined {
    return this.#sinkFatal ?? this.#fatalError;
  }

  private markFatal(error: KernelRuntimeError): void {
    this.#fatalError ??= error;
    this.#agent.abort();
  }

  private async emit(event: KernelEvent): Promise<void> {
    if (this.#sinkFatal) throw this.#sinkFatal;
    try {
      await this.options.eventSink.append(event);
    } catch {
      const outcomeUnknown = "callId" in event && this.#activeTools.has(event.callId);
      const fatal = new KernelRuntimeError(
        outcomeUnknown ? "TOOL_OUTCOME_UNKNOWN" : "EVENT_SINK_UNAVAILABLE",
        outcomeUnknown
          ? "Tool outcome durability cannot be confirmed"
          : "Durable event sink unavailable",
      );
      this.#sinkFatal = fatal;
      this.markFatal(fatal);
      throw fatal;
    }

    if (event.type === "tool_started") this.#activeTools.add(event.callId);
    if (
      event.type === "tool_succeeded" ||
      event.type === "tool_failed" ||
      event.type === "tool_outcome_unknown"
    ) {
      this.#activeTools.delete(event.callId);
    }
    for (const listener of [...this.#listeners]) {
      try {
        await listener(event);
      } catch {
        this.#listeners.delete(listener);
      }
    }
  }

  private async handlePiEvent(event: AgentEvent): Promise<void> {
    if (event.type === "agent_start") {
      await this.emit({ type: "run_started", sessionId: this.options.sessionId });
      return;
    }
    if (event.type === "agent_end") {
      if (this.#sinkFatal) return;
      if (this.#fatalError) {
        await this.emit({
          type: "run_failed",
          sessionId: this.options.sessionId,
          code: this.#fatalError.code,
          message: this.#fatalError.message,
        });
      } else if (this.#cancelled) {
        await this.emit({ type: "run_cancelled", sessionId: this.options.sessionId });
      } else {
        await this.emit({ type: "run_completed", sessionId: this.options.sessionId });
      }
      return;
    }
    if (event.type === "turn_start") {
      try {
        const turn = this.#tracker.turn();
        await this.emit({ type: "turn_started", turn });
        this.#turn = turn;
      } catch (error: unknown) {
        this.markFatal(toPublicError(error));
      }
      return;
    }
    if (event.type === "turn_end") {
      if (this.#fatalError || this.#sinkFatal) return;
      const message = event.message as AssistantMessage;
      await this.emit({
        type: "usage",
        usage: {
          inputTokens: message.usage.input,
          outputTokens: message.usage.output,
          totalTokens: message.usage.totalTokens,
        },
      });
      await this.emit({ type: "turn_completed", turn: this.#turn });
      return;
    }
    if (
      event.type === "message_update" &&
      event.assistantMessageEvent.type === "text_delta" &&
      !this.#fatalError &&
      !this.#sinkFatal
    ) {
      await this.emit({
        type: "text_delta",
        delta: sanitizeText(event.assistantMessageEvent.delta),
      });
    }
  }
}

export function createPiKernelSession(options: PiKernelSessionOptions): KernelSession {
  return new PiKernelSession(options);
}

function toPublicError(error: unknown): KernelRuntimeError {
  return error instanceof KernelRuntimeError
    ? new KernelRuntimeError(error.code, error.message)
    : new KernelRuntimeError("MODEL_ERROR", "Kernel runtime failed");
}

function isSignalAborted(signal: AbortSignal | undefined): boolean {
  return signal?.aborted ?? false;
}
