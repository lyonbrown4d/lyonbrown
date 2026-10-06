import { describe, expect, it } from "vitest";

import type {
  LocalModelInferenceEvent,
  LocalModelInferenceRequest,
} from "@lyonbrown/local-model/internal/index";
import { InferenceEventChannel } from "@lyonbrown/local-model/internal/internal/inference-channel";
import { validateInferenceRequest } from "@lyonbrown/local-model/internal/internal/inference-request";

const request = (
  overrides: Partial<LocalModelInferenceRequest> = {},
): LocalModelInferenceRequest => ({
  messages: [{ role: "user", content: "hello" }],
  signal: new AbortController().signal,
  maxOutputTokens: 64,
  maxOutputBytes: 1_024,
  ...overrides,
});

describe("inference request boundaries", () => {
  it.each([
    { messages: [] },
    { messages: Array.from({ length: 129 }, () => ({ role: "user", content: "x" }) as const) },
    { messages: [{ role: "user", content: "x".repeat(65 * 1_024) }] as const },
    { maxOutputTokens: 0 },
    { maxOutputTokens: 8_193 },
    { maxOutputBytes: 0 },
    { maxOutputBytes: 1_048_577 },
  ])("rejects an out-of-bounds request %#", (overrides) => {
    expect(validateInferenceRequest(request(overrides))).toEqual({
      valid: false,
      failure: {
        code: "invalid_request",
        message: "Local model inference request is invalid.",
      },
    });
  });

  it("accepts a bounded tools declaration", () => {
    expect(validateInferenceRequest(request({ tools: [] }))).toMatchObject({
      valid: true,
      tools: [],
    });
  });

  it.each([
    {
      tools: Array.from({ length: 65 }, () => ({
        name: "read",
        description: "read",
        inputSchema: {},
      })),
    },
    { tools: [{ name: "bad name", description: "read", inputSchema: {} }] },
    { tools: [{ name: "read", description: "x".repeat(4_097), inputSchema: {} }] },
    { tools: [{ name: "read", description: "read", inputSchema: new Date() }] },
    {
      messages: [
        {
          role: "assistant",
          content: "",
          toolCalls: [{ callId: "bad id", name: "read", arguments: {} }],
        },
      ],
    },
    {
      messages: [
        { role: "tool_result", callId: "call-1", name: "read", content: "x", isError: "no" },
      ],
    },
  ] as const)("rejects malformed tool contracts %#", (overrides) => {
    expect(
      validateInferenceRequest(
        request(overrides as unknown as Partial<LocalModelInferenceRequest>),
      ),
    ).toMatchObject({ valid: false });
  });

  it.each(["functions"])("rejects request-level %s authority", (field) => {
    const unsafe = { ...request(), [field]: [] } as unknown as LocalModelInferenceRequest;

    expect(validateInferenceRequest(unsafe)).toEqual({
      valid: false,
      failure: {
        code: "unsupported_tool_use",
        message: "Tool messages are not supported by the local model.",
      },
    });
  });

  it("rejects unknown request and message fields", () => {
    const unknownRequestField = {
      ...request(),
      metadata: "untrusted",
    } as unknown as LocalModelInferenceRequest;
    const unknownMessageField = request({
      messages: [{ role: "user", content: "hello", metadata: "untrusted" }],
    } as unknown as Partial<LocalModelInferenceRequest>);

    expect(validateInferenceRequest(unknownRequestField)).toMatchObject({ valid: false });
    expect(validateInferenceRequest(unknownMessageField)).toMatchObject({ valid: false });
  });

  it.each(["function_call", "tool_calls"])("rejects message-level %s authority", (field) => {
    const unsafe = request({
      messages: [{ role: "assistant", content: "", [field]: {} }],
    } as unknown as Partial<LocalModelInferenceRequest>);

    expect(validateInferenceRequest(unsafe)).toEqual({
      valid: false,
      failure: {
        code: "unsupported_tool_use",
        message: "Tool messages are not supported by the local model.",
      },
    });
  });
});

describe("bounded inference event channel", () => {
  it("coalesces queued text before adding a terminal event", async () => {
    const channel = new InferenceEventChannel();
    for (let index = 0; index < 32; index += 1) {
      channel.push({ type: "text_delta", text: String(index % 10) });
    }
    channel.push({ type: "done", reason: "stop" });
    channel.close();

    await expect(channel.next()).resolves.toEqual({
      value: { type: "text_delta", text: "01234567890123456789012345678901" },
      done: false,
    });
    await expect(channel.next()).resolves.toEqual({
      value: { type: "done", reason: "stop" },
      done: false,
    });
    await expect(channel.next()).resolves.toEqual({ value: undefined, done: true });
  });

  it("delivers directly to a waiting consumer and wakes it on close", async () => {
    const channel = new InferenceEventChannel();
    const pendingEvent = channel.next();
    const event: LocalModelInferenceEvent = { type: "text_delta", text: "ready" };
    channel.push(event);
    await expect(pendingEvent).resolves.toEqual({ value: event, done: false });

    const pendingClose = channel.next();
    channel.close();
    await expect(pendingClose).resolves.toEqual({ value: undefined, done: true });
  });
});
