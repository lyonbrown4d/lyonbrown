import { describe, expect, test, vi } from "vitest";

import { BudgetTracker, validateBudget } from "../src/budget.js";
import { KernelRuntimeError, asSafeError, safeErrorMessage } from "../src/errors.js";
import {
  createPiKernelSession,
  type KernelModelAdapter,
  type KernelToolDefinition,
  type PiKernelSessionOptions,
} from "../src/index.js";
import {
  SecretRedactor,
  assertByteLimit,
  isSecretReference,
  redactSecretReferences,
  sanitizeJson,
  sanitizeText,
  utf8Bytes,
} from "../src/sanitization.js";

const valueLimits = {
  maxDepth: 2,
  maxStringBytes: 8,
  maxCollectionEntries: 4,
  maxNodes: 8,
};

const limits = {
  maxInputBytes: 4,
  maxTurns: 1,
  maxSteps: 1,
  maxOutputBytes: 4,
  maxToolResultBytes: 4,
  maxValueDepth: 2,
  maxValueStringBytes: 8,
  maxCollectionEntries: 4,
  maxValueNodes: 8,
  toolTimeoutMs: 1,
};

describe("untrusted value hardening", () => {
  test("normalizes malformed Unicode and strips terminal/control/bidi characters", () => {
    const malformed = `a\ud800b\udc00c😀\u0001\u001b[31m\u202E`;
    expect(sanitizeText(malformed)).toBe("a�b�c😀");
    expect(utf8Bytes("😀")).toBe(4);
  });

  test("accepts bounded JSON primitives, arrays, and null-prototype objects", () => {
    const nullPrototype = Object.create(null) as Record<string, unknown>;
    nullPrototype.safe = [null, true, 2, "ok"];
    expect(sanitizeJson(nullPrototype, { ...valueLimits, maxDepth: 3 })).toEqual({
      safe: [null, true, 2, "ok"],
    });
  });

  test("rejects non-JSON, dangerous, non-finite, and deeply nested values", () => {
    expect(() => sanitizeJson(Number.NaN, valueLimits)).toThrow("JSON-compatible");
    expect(() => sanitizeJson(undefined, valueLimits)).toThrow("JSON-compatible");
    expect(() => sanitizeJson(new Date(), valueLimits)).toThrow("plain JSON");
    const dangerous = Object.create(null) as Record<string, unknown>;
    dangerous.__proto__ = "pollute";
    expect(() => sanitizeJson(dangerous, valueLimits)).toThrow("Dangerous object key");
    expect(() => sanitizeJson({ a: { b: { c: true } } }, { ...valueLimits, maxDepth: 1 })).toThrow(
      "nesting depth",
    );
  });

  test("redacts registered values and symbolic secret references recursively", () => {
    const redactor = new SecretRedactor();
    redactor.add("");
    redactor.add("token");
    expect(redactor.text("token and token")).toBe("[REDACTED] and [REDACTED]");
    expect(redactor.json({ nested: ["token", 1] }, { ...valueLimits, maxDepth: 3 })).toEqual({
      nested: ["[REDACTED]", 1],
    });
    const symbolic = { a: [{ kind: "secretRef", reference: "credential-id" }], b: false };
    const symbolicLimits = { ...valueLimits, maxDepth: 3, maxStringBytes: 32 };
    expect(redactSecretReferences(symbolic, symbolicLimits)).toEqual({
      a: ["[REDACTED]"],
      b: false,
    });
    expect(JSON.stringify(redactSecretReferences(symbolic, symbolicLimits))).not.toContain(
      "credential-id",
    );
  });

  test("rejects hostile secret-reference containers and recognizes only exact shape", () => {
    expect(isSecretReference(null)).toBe(false);
    expect(isSecretReference([])).toBe(false);
    expect(isSecretReference({ kind: "secretRef", reference: 42 })).toBe(false);
    expect(isSecretReference({ kind: "secretRef", reference: "id" })).toBe(true);
    expect(isSecretReference({ kind: "secretRef", reference: "id", extra: true })).toBe(false);
    const getter = vi.fn(() => "id");
    const accessorReference = Object.defineProperty({ kind: "secretRef" }, "reference", {
      enumerable: true,
      get: getter,
    });
    expect(isSecretReference(accessorReference)).toBe(false);
    expect(getter).not.toHaveBeenCalled();
    const dangerous = Object.create(null) as Record<string, unknown>;
    Object.defineProperty(dangerous, "constructor", { value: "pollute", enumerable: true });
    expect(() => redactSecretReferences(dangerous, valueLimits)).toThrow("Dangerous object key");
    expect(() =>
      redactSecretReferences({ a: { b: true } }, { ...valueLimits, maxDepth: 0 }),
    ).toThrow("nesting depth");
  });

  test("redacts object keys and rejects collisions after redaction", () => {
    const redactor = new SecretRedactor();
    redactor.add("token");
    const keyLimits = { ...valueLimits, maxStringBytes: 32 };
    expect(redactor.json({ "token-key": 1 }, keyLimits)).toEqual({ "[REDACTED]-key": 1 });
    expect(() => redactor.json({ "token-key": 1, "[REDACTED]-key": 2 }, keyLimits)).toThrow(
      "Redacted object keys collide",
    );
  });

  test("rejects proxies and accessors without invoking user code", () => {
    const getter = vi.fn(() => "secret");
    const accessor = Object.defineProperty({}, "value", { enumerable: true, get: getter });
    expect(() => sanitizeJson(accessor, valueLimits)).toThrow("accessor");
    expect(getter).not.toHaveBeenCalled();

    const trap = vi.fn(() => ({ enumerable: true, configurable: true, value: "secret" }));
    const proxy = new Proxy({}, { getOwnPropertyDescriptor: trap });
    expect(() => sanitizeJson(proxy, valueLimits)).toThrow("Proxy");
    expect(trap).not.toHaveBeenCalled();
  });

  test("checks string, collection, and node budgets before traversal", () => {
    expect(() => sanitizeJson("\u001b[31m".repeat(3), valueLimits)).toThrow("string byte");
    const getter = vi.fn(() => "must not run");
    const oversized = new Array<unknown>(5);
    Object.defineProperty(oversized, 0, { enumerable: true, get: getter });
    expect(() => sanitizeJson(oversized, valueLimits)).toThrow("collection width");
    expect(getter).not.toHaveBeenCalled();
    expect(() => sanitizeJson([1, 2, [3, 4]], { ...valueLimits, maxNodes: 4 })).toThrow(
      "node budget",
    );
  });
});

describe("resource accounting", () => {
  test("validates every configured budget", () => {
    validateBudget(limits);
    for (const key of Object.keys(limits) as (keyof typeof limits)[]) {
      expect(() => {
        validateBudget({ ...limits, [key]: 0 });
      }).toThrow(`${key} must be`);
      expect(() => {
        validateBudget({ ...limits, [key]: 1.5 });
      }).toThrow(`${key} must be`);
    }
  });

  test("counts turns, steps, input, and cumulative output", () => {
    const tracker = new BudgetTracker(limits);
    tracker.input("1234");
    expect(tracker.turn()).toBe(1);
    tracker.step();
    tracker.output("12");
    expect(tracker.remainingOutputBytes()).toBe(2);
    expect(() => {
      tracker.input("12345");
    }).toThrow("resource budget");
    expect(() => tracker.turn()).toThrow("Maximum model turns");
    expect(() => {
      tracker.step();
    }).toThrow("Maximum tool steps");
    expect(() => {
      tracker.output("345");
    }).toThrow("Maximum model output");
    expect(() => {
      assertByteLimit("12345", 4, "INPUT_BUDGET_EXCEEDED");
    }).toThrow("resource budget");
  });

  test("normalizes unknown thrown values without leaking them", () => {
    const source = new Error("known");
    expect(asSafeError(source)).toBe(source);
    expect(asSafeError({ secret: "unknown" }).message).toBe("Unknown runtime error");
    const getter = vi.fn(() => "raw-secret");
    const hostile = Object.defineProperty({}, "message", { get: getter });
    expect(safeErrorMessage(hostile)).toBe("Unknown runtime error");
    expect(getter).not.toHaveBeenCalled();
    expect(safeErrorMessage(new Error("x".repeat(5_000)))).toBe(
      "Runtime error message exceeded resource budget",
    );
    const runtimeError = new KernelRuntimeError("MODEL_ERROR", "safe");
    expect(runtimeError).toMatchObject({
      name: "KernelRuntimeError",
      code: "MODEL_ERROR",
      message: "safe",
    });
    expect(runtimeError).not.toHaveProperty("cause");
  });
});

describe("construction budgets", () => {
  const model: KernelModelAdapter = {
    model: { provider: "offline", id: "construction" },
    async *stream() {
      await Promise.resolve();
      yield { type: "done", reason: "stop" };
    },
  };

  function options(tool: KernelToolDefinition, systemPrompt = "safe"): PiKernelSessionOptions {
    return {
      sessionId: "construction",
      systemPrompt,
      model,
      toolRegistry: {
        list: () => [tool],
        get: () => tool,
        hasPermission: () => true,
      },
      approvalGate: { request: () => Promise.resolve({ decision: "approve" }) },
      eventSink: { append: () => Promise.resolve() },
      secretResolver: { resolve: () => Promise.resolve("secret") },
      budget: { ...limits, maxInputBytes: 32, maxValueStringBytes: 16 },
    };
  }

  const baseTool: KernelToolDefinition = {
    name: "safe_tool",
    label: "Safe tool",
    description: "safe",
    inputSchema: { type: "object", additionalProperties: false },
    permissions: [],
    risk: "low",
    execute: () => Promise.resolve({ content: "ok" }),
  };

  test("checks raw system prompt and tool description bytes before sanitizing", () => {
    expect(() => createPiKernelSession(options(baseTool, "\u001b".repeat(40)))).toThrow(
      "resource budget",
    );
    expect(() =>
      createPiKernelSession(options({ ...baseTool, description: "d".repeat(40) })),
    ).toThrow("resource budget");
  });

  test("rejects hostile schemas without invoking accessors", () => {
    const getter = vi.fn(() => "string");
    const schema = Object.defineProperty({}, "type", { enumerable: true, get: getter });
    expect(() =>
      createPiKernelSession(
        options({ ...baseTool, inputSchema: schema as Readonly<Record<string, unknown>> }),
      ),
    ).toThrow("accessor");
    expect(getter).not.toHaveBeenCalled();
  });

  test("bounds the tool registry before adapting entries", () => {
    const base = options(baseTool);
    expect(() =>
      createPiKernelSession({
        ...base,
        toolRegistry: {
          ...base.toolRegistry,
          list: () =>
            Array.from({ length: 5 }, (_, index) => ({
              ...baseTool,
              name: `tool_${String(index)}`,
            })),
        },
      }),
    ).toThrow("Tool registry exceeds resource budget");
  });

  test("bounds permissions in each tool definition", () => {
    expect(() =>
      createPiKernelSession(options({ ...baseTool, permissions: ["a", "b", "c", "d", "e"] })),
    ).toThrow("Tool permissions exceed resource budget");
  });
});
