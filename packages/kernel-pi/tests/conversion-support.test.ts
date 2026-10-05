import { describe, expect, test } from "vitest";

import { kernelError } from "../src/errors.ts";
import {
  assertUsage,
  createPiModel,
  normalizeModelDescriptor,
  toKernelMessage,
  toPiUsage,
} from "../src/pi-conversion.ts";
import { SecretRedactor } from "../src/redaction.ts";
import {
  abortCode,
  createDeadlineSignal,
  isJsonObject,
  isKnownRisk,
  raceAbort,
  replaceSecretRefsForValidation,
  resolveSecrets,
  toSafePreExecutionError,
  toSafeToolError,
} from "../src/tool-support.ts";
import type { KernelModelAdapter, KernelModelStreamEvent } from "../src/types.ts";

const limits = {
  maxDepth: 4,
  maxStringBytes: 64,
  maxCollectionEntries: 16,
  maxNodes: 64,
  maxTotalBytes: 256,
  totalBytesErrorCode: "INPUT_BUDGET_EXCEEDED" as const,
};

const adapter = (model: unknown): KernelModelAdapter =>
  ({
    model,
    async *stream(): AsyncIterable<KernelModelStreamEvent> {
      await Promise.resolve();
      yield { type: "done", reason: "stop" };
    },
  }) as KernelModelAdapter;

describe("Pi conversion coverage", () => {
  test("normalizes a descriptor and creates the private Pi model", () => {
    const descriptor = normalizeModelDescriptor(adapter({ provider: "offline", id: "model" }), 64);
    expect(Object.isFrozen(descriptor)).toBe(true);
    expect(createPiModel(descriptor)).toMatchObject({
      id: "model",
      name: "model",
      provider: "offline",
      api: "lyonbrown-kernel-adapter",
    });
  });

  test.each([
    null,
    new Proxy(adapter({ provider: "offline", id: "model" }), {}),
    adapter(null),
    adapter([]),
    adapter({ provider: "offline" }),
    adapter({ provider: "offline", id: "model", extra: true }),
    adapter({ provider: "", id: "model" }),
    adapter({ provider: "off\u001b[31mline", id: "model" }),
    adapter({ provider: "offline", id: "too-long" }),
  ])("fails closed for malformed descriptor %#", (candidate) => {
    const unsafeCandidate: unknown = candidate;
    expect(() => normalizeModelDescriptor(unsafeCandidate as KernelModelAdapter, 4)).toThrow(
      expect.objectContaining({ code: "MODEL_ERROR", message: "Model adapter failed" }),
    );
  });

  test("converts usage and every normalized message role", () => {
    expect(() => {
      assertUsage({ inputTokens: 1, outputTokens: 2, totalTokens: 3 });
    }).not.toThrow();
    expect(() => {
      assertUsage({ inputTokens: -1, outputTokens: 0, totalTokens: 0 });
    }).toThrow("Model usage is invalid");
    expect(toPiUsage({ inputTokens: 1, outputTokens: 2, totalTokens: 3 })).toMatchObject({
      input: 1,
      output: 2,
      totalTokens: 3,
    });
    expect(toKernelMessage({ role: "user", content: "hi\u001b[31m" } as never, limits)).toEqual({
      role: "user",
      content: "hi",
    });
    expect(
      toKernelMessage(
        {
          role: "assistant",
          content: [
            { type: "text", text: "ok" },
            { type: "toolCall", id: "id", name: "tool", arguments: { value: true } },
          ],
        } as never,
        limits,
      ),
    ).toMatchObject({ role: "assistant", content: "ok", toolCalls: [{ name: "tool" }] });
    expect(
      toKernelMessage(
        {
          role: "toolResult",
          toolCallId: "id",
          toolName: "tool",
          content: [
            { type: "text", text: "done" },
            { type: "image", data: "ignored" },
          ],
          isError: false,
        } as never,
        limits,
      ),
    ).toMatchObject({ role: "tool_result", content: "done", isError: false });
  });
});

describe("tool support coverage", () => {
  test("normalizes symbolic references and primitive branches", () => {
    expect(
      replaceSecretRefsForValidation(
        { values: [{ kind: "secretRef", reference: "id" }, null, true, 3, "text"] },
        16,
      ),
    ).toEqual({ values: ["[SECRET]", null, true, 3, "text"] });
    expect(isKnownRisk("low")).toBe(true);
    expect(isKnownRisk("unexpected")).toBe(false);
    expect(isJsonObject({})).toBe(true);
    expect(isJsonObject([])).toBe(false);
  });

  test("composes deadline and abort races", async () => {
    const parent = new AbortController();
    const combined = createDeadlineSignal(parent.signal, 1_000);
    parent.abort();
    expect(combined.aborted).toBe(true);
    expect(createDeadlineSignal(undefined, 1_000).aborted).toBe(false);
    await expect(raceAbort(Promise.resolve("ok"), new AbortController().signal)).resolves.toBe(
      "ok",
    );
    const aborted = new AbortController();
    aborted.abort();
    await expect(raceAbort(Promise.resolve("late"), aborted.signal)).rejects.toHaveProperty(
      "name",
      "AbortError",
    );
    expect(abortCode(aborted.signal)).toBe("TOOL_CANCELLED");
    const timeout = AbortSignal.timeout(0);
    await new Promise((resolve) => setTimeout(resolve, 2));
    expect(abortCode(timeout)).toBe("TOOL_TIMEOUT");
  });

  test("resolves nested secrets and rejects invalid roots, failures, and overflow", async () => {
    const redactor = new SecretRedactor();
    const signal = new AbortController().signal;
    await expect(
      resolveSecrets(
        { nested: [{ kind: "secretRef", reference: "id" }, null, true, 3, "text"] },
        signal,
        () => Promise.resolve("secret"),
        redactor,
        limits,
      ),
    ).resolves.toEqual({ nested: ["secret", null, true, 3, "text"] });
    await expect(
      resolveSecrets("not-an-object", signal, () => Promise.resolve("secret"), redactor, limits),
    ).rejects.toMatchObject({ code: "TOOL_ARGUMENTS_INVALID" });
    await expect(
      resolveSecrets(
        { value: { kind: "secretRef", reference: "id" } },
        signal,
        () => Promise.reject(new Error("external")),
        redactor,
        limits,
      ),
    ).rejects.toMatchObject({ code: "TOOL_ARGUMENTS_INVALID" });
    await expect(
      resolveSecrets(
        { a: "12345678", b: "12345678" },
        signal,
        () => Promise.resolve("unused"),
        redactor,
        { ...limits, maxTotalBytes: 12 },
      ),
    ).rejects.toMatchObject({ code: "INPUT_BUDGET_EXCEEDED" });
  });

  test("maps internal and external authorization errors", () => {
    const redactor = new SecretRedactor();
    redactor.add("secret");
    expect(
      toSafePreExecutionError(kernelError("TOOL_DENIED", "secret denied"), redactor),
    ).toMatchObject({ code: "TOOL_DENIED", message: "[REDACTED] denied" });
    expect(toSafePreExecutionError(new Error("external"), redactor)).toMatchObject({
      code: "TOOL_DENIED",
      message: "Tool authorization unavailable",
    });
    expect(toSafeToolError(kernelError("MODEL_ERROR", "secret failed"), redactor)).toMatchObject({
      code: "MODEL_ERROR",
      message: "[REDACTED] failed",
    });
    expect(toSafeToolError(new Error("external"), redactor)).toMatchObject({
      code: "MODEL_ERROR",
      message: "Tool execution failed",
    });
  });
});
