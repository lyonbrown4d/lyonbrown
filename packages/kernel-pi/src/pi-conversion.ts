import type { Api, Message, Model, Usage } from "@earendil-works/pi-ai";
import { types } from "node:util";

import { kernelError } from "@lyonbrown/kernel-pi/internal/errors";
import { inspectOwnDataProperties } from "@lyonbrown/kernel-pi/internal/inspection";
import {
  sanitizeJson,
  sanitizeText,
  type SanitizationLimits,
} from "@lyonbrown/kernel-pi/internal/sanitization";
import type {
  KernelModelAdapter,
  KernelModelDescriptor,
  KernelModelMessage,
} from "@lyonbrown/kernel-pi/internal/types";

export function normalizeModelDescriptor(
  adapter: KernelModelAdapter,
  maximumBytes: number,
): KernelModelDescriptor {
  try {
    const candidate: unknown = adapter;
    if (typeof candidate !== "object" || candidate === null || types.isProxy(candidate)) {
      throw new TypeError("Invalid model adapter");
    }
    const modelProperty = Object.getOwnPropertyDescriptor(candidate, "model");
    if (!modelProperty || !("value" in modelProperty)) {
      throw new TypeError("Invalid model descriptor");
    }
    const rawModel: unknown = modelProperty.value;
    if (typeof rawModel !== "object" || rawModel === null || Array.isArray(rawModel)) {
      throw new TypeError("Invalid model descriptor");
    }
    const properties = inspectOwnDataProperties(rawModel, 2, () =>
      kernelError("MODEL_ERROR", "Model adapter failed"),
    );
    const fields = new Map(properties.map((property) => [property.key, property.value]));
    if (fields.size !== 2 || !fields.has("provider") || !fields.has("id")) {
      throw new TypeError("Invalid model descriptor");
    }
    const provider = safeModelIdentity(fields.get("provider"), maximumBytes);
    const id = safeModelIdentity(fields.get("id"), maximumBytes);
    return Object.freeze({ provider, id });
  } catch {
    throw kernelError("MODEL_ERROR", "Model adapter failed");
  }
}

export function createPiModel(descriptor: KernelModelDescriptor): Model<Api> {
  return {
    id: descriptor.id,
    name: descriptor.id,
    api: "lyonbrown-kernel-adapter",
    provider: descriptor.provider,
    baseUrl: "kernel://model-adapter",
    reasoning: false,
    input: ["text"],
    cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
    contextWindow: 1_000_000,
    maxTokens: 1_000_000,
  };
}

function safeModelIdentity(value: unknown, maximumBytes: number): string {
  if (typeof value !== "string" || value.length === 0)
    throw new TypeError("Invalid model identity");
  const sanitized = sanitizeText(value);
  if (sanitized !== value || Buffer.byteLength(value, "utf8") > maximumBytes) {
    throw new TypeError("Invalid model identity");
  }
  return value;
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
    throw kernelError("MODEL_ERROR", "Model usage is invalid");
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
