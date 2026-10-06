import type { ChatHistoryItem, ChatModelFunctions } from "node-llama-cpp";

import type {
  LocalModelInferenceEvent,
  LocalModelJsonValue,
  LocalModelMessage,
  LocalModelTool,
} from "@lyonbrown/local-model/internal/types";

interface NativeFunctionCall {
  readonly functionName: string;
  readonly params: unknown;
}

export const toNativeHistory = (messages: readonly LocalModelMessage[]): ChatHistoryItem[] => {
  const results = new Map(
    messages
      .filter((message) => message.role === "tool_result")
      .map((message) => [message.callId, { content: message.content, isError: message.isError }]),
  );
  return messages.flatMap((message): ChatHistoryItem[] => {
    switch (message.role) {
      case "system":
        return [{ type: "system", text: message.content }];
      case "user":
        return [{ type: "user", text: message.content }];
      case "assistant": {
        const response: ChatHistoryItem & { readonly type: "model" } = {
          type: "model",
          response: [
            ...(message.content === "" ? [] : [message.content]),
            ...(message.toolCalls ?? []).map((call) => ({
              type: "functionCall" as const,
              name: call.name,
              params: call.arguments,
              result: results.get(call.callId),
            })),
          ],
        };
        return [response];
      }
      case "tool_result":
        return [];
    }
  });
};

export const toNativeFunctions = (
  tools: readonly LocalModelTool[],
): ChatModelFunctions | undefined => {
  if (tools.length === 0) return undefined;
  return Object.fromEntries(
    tools.map((tool) => [tool.name, { description: tool.description, params: tool.inputSchema }]),
  ) as ChatModelFunctions;
};

const isPlainRecord = (value: unknown): value is Readonly<Record<string, unknown>> => {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const prototype = Reflect.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
};

const copyJson = (value: unknown, depth = 0): LocalModelJsonValue => {
  if (depth > 16) throw new Error("native_function_arguments_invalid");
  if (value === null || typeof value === "boolean" || typeof value === "string") return value;
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (Array.isArray(value)) {
    if (value.length > 1_024) throw new Error("native_function_arguments_invalid");
    return value.map((item) => copyJson(item, depth + 1));
  }
  if (!isPlainRecord(value)) throw new Error("native_function_arguments_invalid");
  const entries = Object.entries(value);
  if (entries.length > 1_024) throw new Error("native_function_arguments_invalid");
  return Object.fromEntries(entries.map(([key, item]) => [key, copyJson(item, depth + 1)]));
};

export const normalizeNativeFunctionCalls = (
  calls: readonly unknown[] | undefined,
  tools: readonly LocalModelTool[],
): readonly Extract<LocalModelInferenceEvent, { readonly type: "tool_call" }>[] => {
  if (calls === undefined) return [];
  const names = new Set(tools.map((tool) => tool.name));
  if (calls.length === 0 || calls.length > 64) throw new Error("native_function_calls_invalid");
  return calls.map((candidate) => {
    if (!isPlainRecord(candidate)) throw new Error("native_function_call_invalid");
    const call = candidate as unknown as NativeFunctionCall;
    if (typeof call.functionName !== "string") throw new Error("native_function_call_invalid");
    if (!names.has(call.functionName)) throw new Error("native_function_call_unknown");
    const arguments_ = copyJson(call.params);
    if (!isPlainRecord(arguments_)) throw new Error("native_function_arguments_invalid");
    return { type: "tool_call", name: call.functionName, arguments: arguments_ };
  });
};
