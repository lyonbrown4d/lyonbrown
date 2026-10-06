import type {
  LocalModelInferenceErrorCode,
  LocalModelInferenceRequest,
  LocalModelMessage,
} from "@lyonbrown/local-model/internal/types";
import { LOCAL_MODEL_MAX_MESSAGE_BYTES } from "@lyonbrown/local-model/internal/types";

const MAX_MESSAGE_COUNT = 128;
const MAX_INPUT_BYTES = 1024 * 1024;
const MAX_OUTPUT_BYTES = 1024 * 1024;
const MAX_OUTPUT_TOKENS = 8_192;
const REQUEST_FIELDS = ["messages", "signal", "maxOutputTokens", "maxOutputBytes"] as const;
const MESSAGE_FIELDS = ["role", "content"] as const;
const TOOL_AUTHORITY_FIELDS = [
  "function_call",
  "functionCall",
  "functionCallId",
  "functions",
  "tool_call_id",
  "tool_calls",
  "toolCallId",
  "toolCalls",
  "tool_choice",
  "toolChoice",
  "tools",
] as const;

interface ValidationFailure {
  readonly code: LocalModelInferenceErrorCode;
  readonly message: string;
}

export type InferenceRequestValidation =
  | { readonly valid: true; readonly messages: readonly LocalModelMessage[] }
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

const hasToolAuthority = (value: object): boolean =>
  TOOL_AUTHORITY_FIELDS.some((field) => field in value);

const hasExactFields = (value: object, fields: readonly string[]): boolean => {
  const keys = Reflect.ownKeys(value);
  return (
    keys.length === fields.length &&
    keys.every((key) => typeof key === "string" && fields.includes(key))
  );
};

export const validateInferenceRequest = (
  request: LocalModelInferenceRequest,
): InferenceRequestValidation => {
  const candidate: unknown = request;
  if (!isRecord(candidate)) return invalidRequest();
  if (hasToolAuthority(candidate)) return unsupportedToolUse();
  if (!hasExactFields(candidate, REQUEST_FIELDS)) return invalidRequest();

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
    const message = messageCandidate as { readonly role?: unknown; readonly content?: unknown };
    if (message.role === "tool" || hasToolAuthority(messageCandidate)) return unsupportedToolUse();
    if (!hasExactFields(messageCandidate, MESSAGE_FIELDS)) return invalidRequest();
    if (
      (message.role !== "system" && message.role !== "user" && message.role !== "assistant") ||
      typeof message.content !== "string"
    ) {
      return invalidRequest();
    }
    const messageBytes = Buffer.byteLength(message.content, "utf8");
    if (messageBytes > LOCAL_MODEL_MAX_MESSAGE_BYTES) return invalidRequest();
    inputBytes += messageBytes;
    if (inputBytes > MAX_INPUT_BYTES) return invalidRequest();
  }

  return { valid: true, messages: checkedRequest.messages };
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
