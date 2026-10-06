import type { LocalModelLease, LocalModelMessage } from "@lyonbrown/local-model";
import type {
  KernelModelAdapter,
  KernelModelMessage,
  KernelModelRequest,
  KernelModelStreamEvent,
} from "@lyonbrown/kernel-pi";

const MAX_OUTPUT_TOKENS = 2_048;
const REJECTED = "local model request rejected";
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
      const messages = normalizeMessages(request.messages);
      if (request.tools.length > 0 || messages === undefined) {
        yield { type: "error", code: "model_error", message: REJECTED };
        return;
      }
      try {
        for await (const event of options.lease.stream({
          messages,
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

function normalizeMessages(
  messages: readonly KernelModelMessage[],
): readonly LocalModelMessage[] | undefined {
  const normalized: LocalModelMessage[] = [];
  for (const message of messages) {
    if (message.role === "tool_result") return undefined;
    if (message.role === "assistant" && message.toolCalls.length > 0) return undefined;
    normalized.push({ role: message.role, content: message.content });
  }
  return normalized;
}
