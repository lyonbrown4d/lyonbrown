import { Agent, type AgentEvent } from "@earendil-works/pi-agent-core";
import type { AssistantMessage } from "@earendil-works/pi-ai";

import { BudgetTracker, validateBudget } from "./budget.js";
import { KernelRuntimeError } from "./errors.js";
import { createPiStreamFunction } from "./pi-bridge.js";
import { createPiModel } from "./pi-conversion.js";
import { SecretRedactor, sanitizeText } from "./sanitization.js";
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
  #tracker: BudgetTracker;
  #fatalError: KernelRuntimeError | undefined;
  #running = false;
  #cancelled = false;
  #turn = 0;

  constructor(private readonly options: PiKernelSessionOptions) {
    validateBudget(options.budget);
    this.#tracker = new BudgetTracker(options.budget);
    const redactor = new SecretRedactor();
    const toolExecutor = new ToolExecutor(
      options,
      () => this.#tracker,
      redactor,
      this.emit.bind(this),
    );
    const tools = options.toolRegistry.list().map((tool) => toolExecutor.createAgentTool(tool));
    this.#agent = new Agent({
      initialState: {
        systemPrompt: sanitizeText(options.systemPrompt),
        model: createPiModel(options.model),
        tools,
      },
      streamFn: createPiStreamFunction({
        sessionId: options.sessionId,
        adapter: options.model,
        tracker: () => this.#tracker,
        maximumDepth: options.budget.maxValueDepth,
        tools: options.toolRegistry.list().map((tool) => ({
          name: sanitizeText(tool.name),
          description: sanitizeText(tool.description),
          inputSchema: tool.inputSchema,
        })),
        hasTool: (name) => options.toolRegistry.get(name) !== undefined,
        onUnknownTool: toolExecutor.reportUnknown.bind(toolExecutor),
        onFatal: (error) => {
          this.#fatalError = error;
        },
      }),
      toolExecution: "sequential",
      steeringMode: "one-at-a-time",
      followUpMode: "one-at-a-time",
      sessionId: options.sessionId,
    });
    this.#agent.subscribe((event) => this.handlePiEvent(event));
  }

  async startTurn(input: string, options?: { readonly signal?: AbortSignal }): Promise<void> {
    if (this.#running) throw new KernelRuntimeError("SESSION_BUSY", "A turn is already active");
    this.#tracker = new BudgetTracker(this.options.budget);
    this.#fatalError = undefined;
    this.#cancelled = false;
    this.#turn = 0;
    const safeInput = sanitizeText(input);
    this.#tracker.input(safeInput);
    this.#running = true;
    const externalAbort = (): void => {
      this.cancel();
    };
    options?.signal?.addEventListener("abort", externalAbort, { once: true });
    if (options?.signal?.aborted) this.cancel();
    try {
      await this.#agent.prompt(safeInput);
      const fatalError = this.currentFatalError();
      if (fatalError) {
        await this.emit({
          type: "runtime_error",
          code: fatalError.code,
          message: fatalError.message,
        });
        throw fatalError;
      }
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
    const safe = sanitizeText(input);
    this.#tracker.input(safe);
    return safe;
  }

  private currentFatalError(): KernelRuntimeError | undefined {
    return this.#fatalError;
  }

  private async emit(event: KernelEvent): Promise<void> {
    await this.options.eventSink.append(event);
    for (const listener of this.#listeners) await listener(event);
  }

  private async handlePiEvent(event: AgentEvent): Promise<void> {
    if (event.type === "agent_start") {
      await this.emit({ type: "run_started", sessionId: this.options.sessionId });
      return;
    }
    if (event.type === "agent_end") {
      const fatalError = this.currentFatalError();
      if (fatalError) {
        await this.emit({
          type: "run_failed",
          sessionId: this.options.sessionId,
          code: fatalError.code,
          message: fatalError.message,
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
        this.#turn = this.#tracker.turn();
        await this.emit({ type: "turn_started", turn: this.#turn });
      } catch (error: unknown) {
        this.#fatalError = error as KernelRuntimeError;
        this.#agent.abort();
      }
      return;
    }
    if (event.type === "turn_end") {
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
    if (event.type === "message_update" && event.assistantMessageEvent.type === "text_delta") {
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
