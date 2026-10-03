import { describe, expect, test } from "vitest";

import { BudgetTracker, validateBudget } from "../src/budget.js";
import { KernelRuntimeError, asSafeError } from "../src/errors.js";
import {
  SecretRedactor,
  assertByteLimit,
  isSecretReference,
  redactSecretReferences,
  sanitizeJson,
  sanitizeText,
  utf8Bytes,
} from "../src/sanitization.js";

const limits = {
  maxInputBytes: 4,
  maxTurns: 1,
  maxSteps: 1,
  maxOutputBytes: 4,
  maxToolResultBytes: 4,
  maxValueDepth: 2,
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
    expect(sanitizeJson(nullPrototype, 3)).toEqual({ safe: [null, true, 2, "ok"] });
  });

  test("rejects non-JSON, dangerous, non-finite, and deeply nested values", () => {
    expect(() => sanitizeJson(Number.NaN, 2)).toThrow("JSON-compatible");
    expect(() => sanitizeJson(undefined, 2)).toThrow("JSON-compatible");
    expect(() => sanitizeJson(new Date(), 2)).toThrow("plain JSON");
    const dangerous = Object.create(null) as Record<string, unknown>;
    dangerous.__proto__ = "pollute";
    expect(() => sanitizeJson(dangerous, 2)).toThrow("Dangerous object key");
    expect(() => sanitizeJson({ a: { b: { c: true } } }, 1)).toThrow("nesting depth");
  });

  test("redacts registered values and symbolic secret references recursively", () => {
    const redactor = new SecretRedactor();
    redactor.add("");
    redactor.add("token");
    expect(redactor.text("token and token")).toBe("[REDACTED] and [REDACTED]");
    expect(redactor.json({ nested: ["token", 1] }, 3)).toEqual({ nested: ["[REDACTED]", 1] });
    const symbolic = { a: [{ kind: "secretRef", reference: "credential-id" }], b: false };
    expect(redactSecretReferences(symbolic, 3)).toEqual({ a: ["[REDACTED]"], b: false });
    expect(JSON.stringify(redactSecretReferences(symbolic, 3))).not.toContain("credential-id");
  });

  test("rejects hostile secret-reference containers and recognizes only exact shape", () => {
    expect(isSecretReference(null)).toBe(false);
    expect(isSecretReference([])).toBe(false);
    expect(isSecretReference({ kind: "secretRef", reference: 42 })).toBe(false);
    expect(isSecretReference({ kind: "secretRef", reference: "id" })).toBe(true);
    const dangerous = Object.create(null) as Record<string, unknown>;
    Object.defineProperty(dangerous, "constructor", { value: "pollute", enumerable: true });
    expect(() => redactSecretReferences(dangerous, 2)).toThrow("Dangerous object key");
    expect(() => redactSecretReferences({ a: { b: true } }, 0)).toThrow("nesting depth");
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
    expect(new KernelRuntimeError("MODEL_ERROR", "safe", { cause: source })).toMatchObject({
      name: "KernelRuntimeError",
      code: "MODEL_ERROR",
      message: "safe",
    });
  });
});
