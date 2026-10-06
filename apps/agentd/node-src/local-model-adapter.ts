import { randomUUID } from "node:crypto";

import type {
  LocalModelJsonValue,
  LocalModelLease,
  LocalModelMessage,
  LocalModelTool,
} from "@lyonbrown/local-model";
import type {
  KernelModelAdapter,
  KernelModelMessage,
  KernelModelRequest,
  KernelModelStreamEvent,
} from "@lyonbrown/kernel-pi";

const MAX_OUTPUT_TOKENS = 2_048;
const FAILED = "local model inference failed";

export interface CreateLocalKernelModelAdapterOptions {
  readonly id: string;
  readonly lease: LocalModelLease;
}

export function createLocalKernelModelAdapter(
  options: CreateLocalKernelModelAdapterOptions,
): KernelModelAdapter {
  return Object.freeze({
    model: Object.freeze({ provider: "local", id: options.id }),
    async *stream(request: KernelModelRequest): AsyncIterable<KernelModelStreamEvent> {
      try {
        const messages = normalizeMessages(request.messages);
        for await (const event of options.lease.stream({
          messages,
          tools: normalizeTools(request.tools),
          signal: request.signal,
          maxOutputTokens: MAX_OUTPUT_TOKENS,
          maxOutputBytes: request.maxOutputBytes,
        })) {
          switch (event.type) {
            case "text_delta":
              yield { type: "text_delta", delta: event.text };
              break;
            case "usage":
              yield {
                type: "usage",
                usage: {
                  inputTokens: event.inputTokens,
                  outputTokens: event.outputTokens,
                  totalTokens: event.inputTokens + event.outputTokens,
                },
              };
              break;
            case "tool_call":
              yield {
                type: "tool_call",
                callId: randomUUID(),
                name: event.name,
                arguments: event.arguments,
              };
              break;
            case "done":
              if (event.reason === "cancelled") {
                yield {
                  type: "error",
                  code: "aborted",
                  message: "local model inference cancelled",
                };
              } else {
                yield { type: "done", reason: event.reason };
              }
              return;
            case "error":
              yield {
                type: "error",
                code: request.signal.aborted ? "aborted" : "model_error",
                message: request.signal.aborted ? "local model inference cancelled" : FAILED,
              };
              return;
          }
        }
      } catch {
        yield {
          type: "error",
          code: request.signal.aborted ? "aborted" : "model_error",
          message: request.signal.aborted ? "local model inference cancelled" : FAILED,
        };
      }
    },
  });
}

function normalizeMessages(messages: readonly KernelModelMessage[]): readonly LocalModelMessage[] {
  return messages.map((message): LocalModelMessage => {
    if (message.role === "assistant") {
      return {
        role: message.role,
        content: message.content,
        ...(message.toolCalls.length === 0
          ? {}
          : {
              toolCalls: message.toolCalls.map((call) => ({
                callId: call.callId,
                name: call.name,
                arguments: toJsonRecord(call.arguments),
              })),
            }),
      };
    }
    if (message.role === "tool_result") return { ...message };
    return { role: message.role, content: message.content };
  });
}

function normalizeTools(tools: KernelModelRequest["tools"]): readonly LocalModelTool[] {
  return tools.map((tool) => ({
    name: tool.name,
    description: tool.description,
    inputSchema: toJsonRecord(tool.inputSchema),
  }));
}

function toJsonRecord(
  value: Readonly<Record<string, unknown>>,
): Readonly<Record<string, LocalModelJsonValue>> {
  const normalized = toJson(value);
  if (typeof normalized !== "object" || normalized === null || Array.isArray(normalized)) {
    throw new Error("invalid JSON object");
  }
  return normalized as Readonly<Record<string, LocalModelJsonValue>>;
}

function toJson(value: unknown, depth = 0): LocalModelJsonValue {
  if (depth > 16) throw new Error("invalid JSON depth");
  if (value === null || typeof value === "string" || typeof value === "boolean") return value;
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (Array.isArray(value)) return value.map((item) => toJson(item, depth + 1));
  if (typeof value !== "object") {
    throw new Error("invalid JSON value");
  }
  const prototype = Reflect.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) throw new Error("invalid JSON value");
  return Object.fromEntries(
    Object.entries(value).map(([key, item]) => [key, toJson(item, depth + 1)]),
  );
}
