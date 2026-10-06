import type { JsonObject, Message, ToolCall, ToolResultMessage } from "@earendil-works/pi-ai";

import { kernelError } from "@lyonbrown/kernel-pi/internal/errors";
import {
  limitsFromBudget,
  sanitizeBoundedText,
  sanitizeJson,
  type SanitizationLimits,
} from "@lyonbrown/kernel-pi/internal/sanitization";
import type {
  KernelConversationMessage,
  KernelJsonValue,
  KernelResourceBudget,
} from "@lyonbrown/kernel-pi/internal/types";

const MAX_HISTORY_MESSAGES = 128;
const IDENTIFIER = /^[A-Za-z0-9_.-]{1,128}$/u;
const ZERO_USAGE = Object.freeze({
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: Object.freeze({ input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 }),
});

export function isValidKernelConversationHistory(
  messages: readonly KernelConversationMessage[],
  budget: KernelResourceBudget,
): boolean {
  try {
    toPiHistory(messages, budget, limitsFromBudget(budget));
    return true;
  } catch {
    return false;
  }
}

export function toPiHistory(
  messages: readonly KernelConversationMessage[] | undefined,
  budget: KernelResourceBudget,
  limits: SanitizationLimits,
): Message[] {
  if (messages === undefined) return [];
  try {
    const normalized = sanitizeJson(messages, limits);
    if (!Array.isArray(normalized) || normalized.length > MAX_HISTORY_MESSAGES) {
      return invalidHistory();
    }
    const normalizedMessages = normalized as readonly KernelJsonValue[];
    return normalizedMessages.map((message) => {
      const record = dataRecord(message);
      if (record.role === "user") {
        exactFields(record, ["role", "content"]);
        return {
          role: "user",
          content: sanitizeBoundedText(text(record.content), budget.maxInputBytes),
          timestamp: 0,
        };
      }
      if (record.role === "assistant") {
        exactFields(record, ["role", "content", "toolCalls"]);
        const content: ({ readonly type: "text"; readonly text: string } | ToolCall)[] = [];
        const assistantText = sanitizeBoundedText(text(record.content), budget.maxInputBytes);
        if (assistantText.length > 0) content.push({ type: "text", text: assistantText });
        const calls = toolCalls(record.toolCalls);
        for (const call of calls) {
          content.push({
            type: "toolCall",
            id: identifier(call.callId),
            name: identifier(call.name),
            arguments: call.arguments as JsonObject,
          });
        }
        return {
          role: "assistant",
          content,
          api: "lyonbrown-kernel-adapter",
          provider: "lyonbrown-history",
          model: "recovered",
          usage: ZERO_USAGE,
          stopReason: calls.length > 0 ? "toolUse" : "stop",
          timestamp: 0,
        };
      }
      if (record.role !== "tool_result") return invalidHistory();
      exactFields(record, ["role", "callId", "name", "content", "isError"]);
      if (typeof record.isError !== "boolean") return invalidHistory();
      const result: ToolResultMessage = {
        role: "toolResult",
        toolCallId: identifier(text(record.callId)),
        toolName: identifier(text(record.name)),
        content: [
          {
            type: "text",
            text: sanitizeBoundedText(text(record.content), budget.maxToolResultBytes),
          },
        ],
        isError: record.isError,
        timestamp: 0,
      };
      return result;
    });
  } catch {
    return invalidHistory();
  }
}

type DataRecord = Readonly<Record<string, KernelJsonValue>>;

function dataRecord(value: KernelJsonValue | undefined): DataRecord {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return invalidHistory();
  }
  return value as DataRecord;
}

function exactFields(record: DataRecord, expected: readonly string[]): void {
  const keys = Object.keys(record);
  if (keys.length !== expected.length || keys.some((key) => !expected.includes(key))) {
    return invalidHistory();
  }
}

function text(value: KernelJsonValue | undefined): string {
  if (typeof value !== "string") return invalidHistory();
  return value;
}

function toolCalls(value: KernelJsonValue | undefined): readonly {
  readonly callId: string;
  readonly name: string;
  readonly arguments: DataRecord;
}[] {
  if (!Array.isArray(value) || value.length > 64) return invalidHistory();
  const candidates = value as readonly KernelJsonValue[];
  return candidates.map((candidate) => {
    const record = dataRecord(candidate);
    exactFields(record, ["callId", "name", "arguments"]);
    return {
      callId: text(record.callId),
      name: text(record.name),
      arguments: dataRecord(record.arguments),
    };
  });
}

function identifier(value: string): string {
  if (!IDENTIFIER.test(value)) return invalidHistory();
  return value;
}

function invalidHistory(): never {
  throw kernelError("INPUT_BUDGET_EXCEEDED", "Recovered conversation history is invalid");
}
