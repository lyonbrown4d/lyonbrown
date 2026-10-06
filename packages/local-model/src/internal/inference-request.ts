import type {
  LocalModelInferenceErrorCode,
  LocalModelInferenceRequest,
  LocalModelMessage,
  LocalModelTool,
} from "@lyonbrown/local-model/internal/types";
import { LOCAL_MODEL_MAX_MESSAGE_BYTES } from "@lyonbrown/local-model/internal/types";

const MAX_MESSAGE_COUNT = 128;
const MAX_INPUT_BYTES = 1024 * 1024;
const MAX_OUTPUT_BYTES = 1024 * 1024;
const MAX_OUTPUT_TOKENS = 8_192;
const REQUEST_FIELDS = ["messages", "signal", "maxOutputTokens", "maxOutputBytes"] as const;
const REQUEST_WITH_TOOLS_FIELDS = [...REQUEST_FIELDS, "tools"] as const;
const MESSAGE_FIELDS = ["role", "content"] as const;
const ASSISTANT_MESSAGE_FIELDS = ["role", "content", "toolCalls"] as const;
const TOOL_RESULT_MESSAGE_FIELDS = ["role", "callId", "name", "content", "isError"] as const;
const TOOL_FIELDS = ["name", "description", "inputSchema"] as const;
const TOOL_CALL_FIELDS = ["callId", "name", "arguments"] as const;
const UNSUPPORTED_AUTHORITY_FIELDS = [
  "function_call",
  "functionCall",
  "functionCallId",
  "functions",
  "tool_call_id",
  "tool_calls",
  "toolCallId",
  "tool_choice",
  "toolChoice",
] as const;

interface ValidationFailure {
  readonly code: LocalModelInferenceErrorCode;
  readonly message: string;
}

export type InferenceRequestValidation =
  | {
      readonly valid: true;
      readonly messages: readonly LocalModelMessage[];
      readonly tools: readonly LocalModelTool[];
    }
  | { readonly valid: false; readonly failure: ValidationFailure };

const invalidRequest = (): InferenceRequestValidation => ({
  valid: false,
  failure: { code: "invalid_request", message: "Local model inference request is invalid." },
});

const unsupportedToolUse = (): InferenceRequestValidation => ({
  valid: false,
  failure: {
    code: "unsupported_tool_use",
    message: "Tool messages are not supported by the local model.",
  },
});

const isPositiveIntegerWithin = (value: number, maximum: number): boolean =>
  Number.isSafeInteger(value) && value > 0 && value <= maximum;

const isRecord = (value: unknown): value is object => typeof value === "object" && value !== null;

const hasExactFields = (value: object, fields: readonly string[]): boolean => {
  const keys = Reflect.ownKeys(value);
  return (
    keys.length === fields.length &&
    keys.every((key) => typeof key === "string" && fields.includes(key))
  );
};

const hasUnsupportedAuthority = (value: object): boolean =>
  UNSUPPORTED_AUTHORITY_FIELDS.some((field) => field in value);

export const validateInferenceRequest = (
  request: LocalModelInferenceRequest,
): InferenceRequestValidation => {
  const candidate: unknown = request;
  if (!isRecord(candidate)) return invalidRequest();
  if (hasUnsupportedAuthority(candidate)) return unsupportedToolUse();
  if (
    !hasExactFields(candidate, REQUEST_FIELDS) &&
    !hasExactFields(candidate, REQUEST_WITH_TOOLS_FIELDS)
  ) {
    return invalidRequest();
  }

  const checkedRequest = candidate as LocalModelInferenceRequest;
  const signal = checkedRequest.signal as unknown;
  if (
    typeof signal !== "object" ||
    signal === null ||
    typeof (signal as { readonly aborted?: unknown }).aborted !== "boolean" ||
    typeof (signal as { readonly addEventListener?: unknown }).addEventListener !== "function" ||
    typeof (signal as { readonly removeEventListener?: unknown }).removeEventListener !==
      "function" ||
    !isPositiveIntegerWithin(checkedRequest.maxOutputTokens, MAX_OUTPUT_TOKENS) ||
    !isPositiveIntegerWithin(checkedRequest.maxOutputBytes, MAX_OUTPUT_BYTES) ||
    !Array.isArray(checkedRequest.messages) ||
    checkedRequest.messages.length === 0 ||
    checkedRequest.messages.length > MAX_MESSAGE_COUNT
  ) {
    return invalidRequest();
  }

  let inputBytes = 0;
  for (const messageCandidate of checkedRequest.messages as readonly unknown[]) {
    if (!isRecord(messageCandidate)) return invalidRequest();
    if (hasUnsupportedAuthority(messageCandidate)) return unsupportedToolUse();
    const message = messageCandidate as { readonly role?: unknown; readonly content?: unknown };
    if (message.role === "tool") return unsupportedToolUse();
    const expectedFields =
      message.role === "assistant" && "toolCalls" in messageCandidate
        ? ASSISTANT_MESSAGE_FIELDS
        : message.role === "tool_result"
          ? TOOL_RESULT_MESSAGE_FIELDS
          : MESSAGE_FIELDS;
    if (!hasExactFields(messageCandidate, expectedFields)) return invalidRequest();
    if (
      (message.role !== "system" &&
        message.role !== "user" &&
        message.role !== "assistant" &&
        message.role !== "tool_result") ||
      typeof message.content !== "string"
    ) {
      return invalidRequest();
    }
    if (message.role === "assistant" && "toolCalls" in messageCandidate) {
      if (!validToolCalls((messageCandidate as { readonly toolCalls?: unknown }).toolCalls)) {
        return invalidRequest();
      }
    }
    if (message.role === "tool_result") {
      const result = messageCandidate as {
        readonly callId?: unknown;
        readonly name?: unknown;
        readonly isError?: unknown;
      };
      if (
        !validIdentifier(result.callId) ||
        !validIdentifier(result.name) ||
        typeof result.isError !== "boolean"
      ) {
        return invalidRequest();
      }
    }
    const messageBytes = Buffer.byteLength(message.content, "utf8");
    if (messageBytes > LOCAL_MODEL_MAX_MESSAGE_BYTES) return invalidRequest();
    inputBytes += messageBytes;
    if (inputBytes > MAX_INPUT_BYTES) return invalidRequest();
  }

  const tools = checkedRequest.tools ?? [];
  if (!Array.isArray(tools) || tools.length > 64 || !tools.every(validTool))
    return invalidRequest();
  return { valid: true, messages: checkedRequest.messages, tools };
};

const validIdentifier = (value: unknown): value is string =>
  typeof value === "string" && /^[A-Za-z0-9_.-]{1,128}$/u.test(value);

const validJson = (value: unknown, depth = 0): boolean => {
  if (depth > 16) return false;
  if (value === null || typeof value === "boolean" || typeof value === "string") return true;
  if (typeof value === "number") return Number.isFinite(value);
  if (Array.isArray(value))
    return value.length <= 1_024 && value.every((item) => validJson(item, depth + 1));
  if (!isRecord(value)) return false;
  const prototype = Reflect.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null) return false;
  const keys = Reflect.ownKeys(value);
  return (
    keys.length <= 1_024 &&
    keys.every((key) => typeof key === "string" && validJson(Reflect.get(value, key), depth + 1))
  );
};

const validToolCalls = (value: unknown): boolean =>
  Array.isArray(value) &&
  value.length <= 64 &&
  value.every((candidate: unknown) => {
    if (!isRecord(candidate) || !hasExactFields(candidate, TOOL_CALL_FIELDS)) return false;
    const call = candidate as {
      readonly callId?: unknown;
      readonly name?: unknown;
      readonly arguments?: unknown;
    };
    return validIdentifier(call.callId) && validIdentifier(call.name) && validJson(call.arguments);
  });

const validTool = (candidate: unknown): candidate is LocalModelTool => {
  if (!isRecord(candidate) || !hasExactFields(candidate, TOOL_FIELDS)) return false;
  const tool = candidate as {
    readonly name?: unknown;
    readonly description?: unknown;
    readonly inputSchema?: unknown;
  };
  return (
    validIdentifier(tool.name) &&
    typeof tool.description === "string" &&
    Buffer.byteLength(tool.description, "utf8") <= 4_096 &&
    validJson(tool.inputSchema)
  );
};

export const utf8Prefix = (text: string, maximumBytes: number): string => {
  let prefix = "";
  let usedBytes = 0;
  for (const character of text) {
    const characterBytes = Buffer.byteLength(character, "utf8");
    if (usedBytes + characterBytes > maximumBytes) break;
    prefix += character;
    usedBytes += characterBytes;
  }
  return prefix;
};
