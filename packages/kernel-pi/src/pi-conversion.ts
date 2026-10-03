import type { Api, Message, Model, Usage } from "@earendil-works/pi-ai";

import { KernelRuntimeError } from "./errors.js";
import { sanitizeJson, sanitizeText, type SanitizationLimits } from "./sanitization.js";
import type { KernelModelAdapter, KernelModelMessage } from "./types.js";

export function createPiModel(adapter: KernelModelAdapter): Model<Api> {
  return {
    id: adapter.model.id,
    name: adapter.model.id,
    api: "lyonbrown-kernel-adapter",
    provider: adapter.model.provider,
    baseUrl: "kernel://model-adapter",
    reasoning: false,
    input: ["text"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 1_000_000,
    maxTokens: 1_000_000,
  };
}

export function assertUsage(usage: {
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
}): void {
  if (
    !Number.isSafeInteger(usage.inputTokens) ||
    usage.inputTokens < 0 ||
    !Number.isSafeInteger(usage.outputTokens) ||
    usage.outputTokens < 0 ||
    !Number.isSafeInteger(usage.totalTokens) ||
    usage.totalTokens < usage.inputTokens + usage.outputTokens
  ) {
    throw new KernelRuntimeError("MODEL_ERROR", "Model usage is invalid");
  }
}

export function toPiUsage(usage: {
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
}): Usage {
  return {
    input: usage.inputTokens,
    output: usage.outputTokens,
    cacheRead: 0,
    cacheWrite: 0,
    totalTokens: usage.totalTokens,
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
  };
}

export function toKernelMessage(message: Message, limits: SanitizationLimits): KernelModelMessage {
  if (message.role === "system" || message.role === "user") {
    return { role: message.role, content: contentText(message.content) };
  }
  if (message.role === "assistant") {
    return {
      role: "assistant",
      content: message.content
        .filter((content) => content.type === "text")
        .map((content) => content.text)
        .join(""),
      toolCalls: message.content
        .filter((content) => content.type === "toolCall")
        .map((call) => ({
          callId: sanitizeText(call.id),
          name: sanitizeText(call.name),
          arguments: sanitizeJson(call.arguments, limits) as Readonly<Record<string, unknown>>,
        })),
    };
  }
  return {
    role: "tool_result",
    callId: sanitizeText(message.toolCallId),
    name: sanitizeText(message.toolName),
    content: contentText(message.content),
    isError: message.isError,
  };
}

function contentText(content: string | readonly { type: string; text?: string }[]): string {
  return typeof content === "string"
    ? sanitizeText(content)
    : content
        .map((block) => (block.type === "text" ? sanitizeText(block.text ?? "") : ""))
        .join("");
}
