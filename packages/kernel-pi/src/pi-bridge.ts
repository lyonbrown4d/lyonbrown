import type { StreamFn } from "@earendil-works/pi-agent-core";
import { types } from "node:util";
import {
  createAssistantMessageEventStream,
  type AssistantMessage,
  type AssistantMessageEventStream,
  type Message,
  type ToolCall,
  type Usage,
} from "@earendil-works/pi-ai";

import type { BudgetTracker } from "./budget.js";
import { KernelRuntimeError } from "./errors.js";
import { assertUsage, toKernelMessage, toPiUsage } from "./pi-conversion.js";
import {
  assertByteLimit,
  sanitizeJson,
  sanitizeText,
  type SanitizationLimits,
} from "./sanitization.js";
import type {
  KernelJsonValue,
  KernelModelAdapter,
  KernelModelStreamEvent,
  KernelModelTool,
} from "./types.js";

const ZERO_USAGE: Usage = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
};

interface PiBridgeOptions {
  readonly sessionId: string;
  readonly adapter: KernelModelAdapter;
  readonly tracker: () => BudgetTracker;
  readonly limits: SanitizationLimits;
  readonly outputLimits: SanitizationLimits;
  readonly tools: readonly KernelModelTool[];
  readonly onUnknownTool: (callId: string, name: string) => Promise<void>;
  readonly hasTool: (name: string) => boolean;
  readonly onFatal: (error: KernelRuntimeError) => void;
}

export function createPiStreamFunction(options: PiBridgeOptions): StreamFn {
  return (_model, context, streamOptions) => {
    const stream = createAssistantMessageEventStream();
    void produceStream(stream, context.messages, streamOptions?.signal, options);
    return stream;
  };
}

async function produceStream(
  stream: AssistantMessageEventStream,
  messages: Message[],
  signal: AbortSignal | undefined,
  options: PiBridgeOptions,
): Promise<void> {
  const effectiveSignal = signal ?? new AbortController().signal;
  let partial = createMessage(options, "pending");
  let usage = ZERO_USAGE;
  let done = false;
  try {
    stream.push({ type: "start", partial: snapshot(partial) });
    const request = {
      sessionId: options.sessionId,
      model: options.adapter.model,
      messages: messages.map((message) => toKernelMessage(message, options.limits)),
      tools: options.tools,
      signal: effectiveSignal,
      maxOutputBytes: options.tracker().remainingOutputBytes(),
    };
    effectiveSignal.throwIfAborted();
    for await (const event of options.adapter.stream(request)) {
      if (effectiveSignal.aborted) {
        finishError(stream, partial, "aborted", "Model request aborted");
        return;
      }
      const result = await consumeModelEvent(stream, partial, event, options);
      partial = result.partial;
      usage = result.usage ?? usage;
      if (result.done) {
        done = true;
        break;
      }
    }
    if (!done)
      throw new KernelRuntimeError("MODEL_ERROR", "Model stream ended without a terminal event");
    partial = { ...partial, usage };
  } catch (error: unknown) {
    if (effectiveSignal.aborted) {
      finishError(stream, partial, "aborted", "Model request aborted");
      return;
    }
    const runtimeError =
      error instanceof KernelRuntimeError
        ? error
        : new KernelRuntimeError("MODEL_ERROR", "Model adapter failed");
    options.onFatal(runtimeError);
    finishError(stream, partial, "error", runtimeError.message);
  }
}

async function consumeModelEvent(
  stream: AssistantMessageEventStream,
  partial: AssistantMessage,
  rawEvent: KernelModelStreamEvent,
  options: PiBridgeOptions,
): Promise<{ partial: AssistantMessage; usage?: Usage; done: boolean }> {
  const rawEventType = inspectRawEvent(rawEvent, options.tracker());
  let sanitizedEvent: KernelJsonValue;
  try {
    sanitizedEvent = sanitizeJson(
      rawEvent,
      rawEventType === "text_delta" ? options.outputLimits : options.limits,
    );
  } catch (error: unknown) {
    if (
      error instanceof KernelRuntimeError &&
      (error.code === "INPUT_BUDGET_EXCEEDED" || error.code === "OUTPUT_BUDGET_EXCEEDED")
    ) {
      throw error;
    }
    throw new KernelRuntimeError("MODEL_ERROR", "Invalid model event denied");
  }
  if (!isJsonObject(sanitizedEvent)) {
    throw new KernelRuntimeError("MODEL_ERROR", "Model event must be an object");
  }
  const eventType = sanitizedEvent.type;
  if (
    eventType !== "usage" &&
    eventType !== "text_delta" &&
    eventType !== "tool_call" &&
    eventType !== "error" &&
    eventType !== "done"
  ) {
    throw new KernelRuntimeError("MODEL_ERROR", "Unknown model event denied");
  }
  if (
    eventType === "done" &&
    sanitizedEvent.reason !== "stop" &&
    sanitizedEvent.reason !== "length" &&
    sanitizedEvent.reason !== "tool_use"
  ) {
    throw new KernelRuntimeError("MODEL_ERROR", "Invalid model stop reason");
  }
  const event = sanitizedEvent as unknown as KernelModelStreamEvent;
  if (event.type === "usage") {
    assertUsage(event.usage);
    return { partial, usage: toPiUsage(event.usage), done: false };
  }
  if (event.type === "text_delta") {
    const delta = sanitizeText(event.delta);
    let index = partial.content.findIndex((content) => content.type === "text");
    if (index < 0) {
      index = partial.content.length;
      partial = { ...partial, content: [...partial.content, { type: "text", text: "" }] };
      stream.push({ type: "text_start", contentIndex: index, partial: snapshot(partial) });
    }
    const block = partial.content[index];
    if (block?.type !== "text")
      throw new KernelRuntimeError("MODEL_ERROR", "Invalid text stream state");
    const updated = { ...block, text: block.text + delta };
    partial = {
      ...partial,
      content: partial.content.map((content, contentIndex) =>
        contentIndex === index ? updated : content,
      ),
    };
    stream.push({ type: "text_delta", contentIndex: index, delta, partial: snapshot(partial) });
    return { partial, done: false };
  }
  if (event.type === "tool_call") {
    const callId = sanitizeText(event.callId);
    const name = sanitizeText(event.name);
    const arguments_ = sanitizeJson(event.arguments, options.limits);
    if (!isJsonObject(arguments_)) {
      throw new KernelRuntimeError("MODEL_ERROR", "Tool arguments must be an object");
    }
    assertByteLimit(
      JSON.stringify(arguments_),
      options.tracker().limits.maxInputBytes,
      "INPUT_BUDGET_EXCEEDED",
    );
    if (!options.hasTool(name)) await options.onUnknownTool(callId, name);
    const index = partial.content.length;
    partial = {
      ...partial,
      content: [...partial.content, { type: "toolCall", id: callId, name, arguments: {} }],
    };
    stream.push({ type: "toolcall_start", contentIndex: index, partial: snapshot(partial) });
    const toolCall: ToolCall = { type: "toolCall", id: callId, name, arguments: arguments_ };
    partial = {
      ...partial,
      content: partial.content.map((content, contentIndex) =>
        contentIndex === index ? toolCall : content,
      ),
    };
    stream.push({
      type: "toolcall_end",
      contentIndex: index,
      toolCall,
      partial: snapshot(partial),
    });
    return { partial, done: false };
  }
  if (event.type === "error") {
    const reason = event.code === "aborted" ? "aborted" : "error";
    const message = event.code === "aborted" ? "Model request aborted" : "Model request failed";
    options.onFatal(new KernelRuntimeError("MODEL_ERROR", message));
    finishError(stream, partial, reason, message);
    return { partial, done: true };
  }
  const reason = event.reason === "tool_use" ? "toolUse" : event.reason;
  const message: AssistantMessage = { ...partial, stopReason: reason };
  closeOpenText(stream, message);
  stream.push({ type: "done", reason, message });
  stream.end(message);
  return { partial: message, done: true };
}

function closeOpenText(stream: AssistantMessageEventStream, message: AssistantMessage): void {
  message.content.forEach((content, index) => {
    if (content.type === "text") {
      stream.push({
        type: "text_end",
        contentIndex: index,
        content: content.text,
        partial: snapshot(message),
      });
    }
  });
}

function finishError(
  stream: AssistantMessageEventStream,
  partial: AssistantMessage,
  reason: "aborted" | "error",
  message: string,
): void {
  const error: AssistantMessage = {
    ...partial,
    stopReason: reason,
    errorMessage: sanitizeText(message),
    timestamp: Date.now(),
  };
  stream.push({ type: "error", reason, error });
  stream.end(error);
}

function createMessage(
  options: PiBridgeOptions,
  stopReason: AssistantMessage["stopReason"],
): AssistantMessage {
  return {
    role: "assistant",
    content: [],
    api: "lyonbrown-kernel-adapter",
    provider: options.adapter.model.provider,
    model: options.adapter.model.id,
    usage: ZERO_USAGE,
    stopReason,
    timestamp: Date.now(),
  };
}

function snapshot(message: AssistantMessage): AssistantMessage {
  return { ...message, content: message.content.map((content) => ({ ...content })) };
}

function isJsonObject(value: KernelJsonValue): value is Readonly<Record<string, KernelJsonValue>> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function inspectRawEvent(event: unknown, tracker: BudgetTracker): unknown {
  if (typeof event !== "object" || event === null || types.isProxy(event)) return undefined;
  const descriptors = Object.getOwnPropertyDescriptors(event);
  const type = descriptors.type;
  const delta = descriptors.delta;
  if (
    type &&
    delta &&
    "value" in type &&
    "value" in delta &&
    type.value === "text_delta" &&
    typeof delta.value === "string"
  ) {
    tracker.output(delta.value);
  }
  return type && "value" in type ? type.value : undefined;
}
