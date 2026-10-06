import { Agent, type AgentEvent } from "@earendil-works/pi-agent-core";
import type { AssistantMessage } from "@earendil-works/pi-ai";

import { BudgetTracker, validateBudget } from "@lyonbrown/kernel-pi/internal/budget";
import {
  kernelError,
  toPublicKernelError,
  type KernelRuntimeError,
} from "@lyonbrown/kernel-pi/internal/errors";
import { ListenerDispatcher } from "@lyonbrown/kernel-pi/internal/listener-dispatcher";
import { createPiStreamFunction } from "@lyonbrown/kernel-pi/internal/pi-bridge";
import {
  createPiModel,
  normalizeModelDescriptor,
} from "@lyonbrown/kernel-pi/internal/pi-conversion";
import { SecretRedactor } from "@lyonbrown/kernel-pi/internal/redaction";
import {
  limitsFromBudget,
  sanitizeBoundedText,
  sanitizeText,
} from "@lyonbrown/kernel-pi/internal/sanitization";
import { normalizeTools } from "@lyonbrown/kernel-pi/internal/tool-normalization";
import { ToolExecutor } from "@lyonbrown/kernel-pi/internal/tools";
import type {
  KernelEvent,
  KernelEventListener,
  KernelSession,
  KernelToolDefinition,
  PiKernelSessionOptions,
} from "@lyonbrown/kernel-pi/internal/types";

export class PiKernelSession implements KernelSession {
  readonly #agent: Agent;
  readonly #unsubscribeAgent: () => void;
  readonly #listeners = new ListenerDispatcher({ maxPendingEvents: 64, timeoutMs: 1_000 });
  readonly #activeTools = new Set<string>();
  #tracker: BudgetTracker;
  #fatalError: KernelRuntimeError | undefined;
  #terminalFatal: KernelRuntimeError | undefined;
  #sinkFatal: KernelRuntimeError | undefined;
  #running = false;
  #cancelled = false;
  #disposed = false;
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
    const modelDescriptor = normalizeModelDescriptor(options.model, options.budget.maxInputBytes);
    const registeredTools = normalizeTools(listRegisteredTools(options), options);
    const toolByName = new Map(registeredTools.map((tool) => [tool.name, tool]));
    this.#tracker = new BudgetTracker(options.budget);
    const redactor = new SecretRedactor({
      maxSecretCount: options.budget.maxSecretCount ?? 128,
      maxSecretBytes: options.budget.maxSecretBytes ?? 65_536,
    });
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
        model: createPiModel(modelDescriptor),
        tools,
      },
      streamFn: createPiStreamFunction({
        sessionId: options.sessionId,
        adapter: options.model,
        model: modelDescriptor,
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
    this.#unsubscribeAgent = this.#agent.subscribe((event) => this.handlePiEvent(event));
  }

  async startTurn(input: string, options?: { readonly signal?: AbortSignal }): Promise<void> {
    options?.signal?.throwIfAborted();
    this.assertUsable();
    const terminal = this.sessionTerminalError();
    if (terminal) throw terminal;
    if (this.#running) throw kernelError("SESSION_BUSY", "A turn is already active");
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
    this.assertControlAvailable();
    const safeInput = this.safeControlInput(input);
    this.#agent.steer({ role: "user", content: safeInput, timestamp: Date.now() });
  }

  followUp(input: string): void {
    this.assertControlAvailable();
    const safeInput = this.safeControlInput(input);
    this.#agent.followUp({ role: "user", content: safeInput, timestamp: Date.now() });
  }

  cancel(): void {
    this.#cancelled = true;
    this.#agent.abort();
  }

  subscribe(listener: KernelEventListener): () => void {
    return this.#listeners.subscribe(listener);
  }

  dispose(): void {
    if (this.#disposed) return;
    this.cancel();
    this.#disposed = true;
    this.#unsubscribeAgent();
    this.#listeners.dispose();
  }

  private safeControlInput(input: string): string {
    const safe = sanitizeBoundedText(input, this.options.budget.maxInputBytes);
    this.#tracker.input(safe);
    return safe;
  }

  private currentFatalError(): KernelRuntimeError | undefined {
    return this.sessionTerminalError() ?? this.#fatalError;
  }

  private markFatal(error: KernelRuntimeError): void {
    if (error.code === "TOOL_OUTCOME_UNKNOWN") this.#terminalFatal ??= error;
    else this.#fatalError ??= error;
    this.#agent.abort();
  }

  private sessionTerminalError(): KernelRuntimeError | undefined {
    return this.#sinkFatal ?? this.#terminalFatal;
  }

  private assertUsable(): void {
    if (this.#disposed) throw kernelError("SESSION_DISPOSED", "Kernel session is disposed");
  }

  private assertControlAvailable(): void {
    this.assertUsable();
    const terminal = this.sessionTerminalError();
    if (terminal) throw terminal;
  }

  private async emit(event: KernelEvent): Promise<void> {
    if (this.#sinkFatal) throw this.#sinkFatal;
    try {
      await this.options.eventSink.append(event);
    } catch {
      const outcomeUnknown = "callId" in event && this.#activeTools.has(event.callId);
      const fatal = kernelError(
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
    this.#listeners.dispatch(event);
  }

  private async handlePiEvent(event: AgentEvent): Promise<void> {
    if (event.type === "agent_start") {
      await this.emit({ type: "run_started", sessionId: this.options.sessionId });
      return;
    }
    if (event.type === "agent_end") {
      if (this.#sinkFatal) return;
      const fatal = this.#terminalFatal ?? this.#fatalError;
      if (fatal) {
        await this.emit({
          type: "run_failed",
          sessionId: this.options.sessionId,
          code: fatal.code,
          message: fatal.message,
        });
      } else if (this.#cancelled) {
        await this.emit({ type: "run_cancelled", sessionId: this.options.sessionId });
      } else {
        await this.emit({ type: "run_completed", sessionId: this.options.sessionId });
      }
      return;
    }
    if (event.type === "turn_start") {
      if (this.currentFatalError()) return;
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
      if (this.currentFatalError()) return;
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
      !this.currentFatalError()
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
  return toPublicKernelError(error);
}

function isSignalAborted(signal: AbortSignal | undefined): boolean {
  return signal?.aborted ?? false;
}

function listRegisteredTools(options: PiKernelSessionOptions): readonly KernelToolDefinition[] {
  try {
    return options.toolRegistry.list();
  } catch {
    throw kernelError("INTERNAL", "Kernel runtime failed");
  }
}
