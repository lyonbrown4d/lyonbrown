import { describe, expect, test } from "vitest";

import { SecretRedactor } from "../src/redaction.js";
import { sanitizeJson } from "../src/sanitization.js";

const limits = {
  maxDepth: 3,
  maxStringBytes: 64,
  maxCollectionEntries: 16,
  maxNodes: 32,
  maxTotalBytes: 256,
  totalBytesErrorCode: "INPUT_BUDGET_EXCEEDED" as const,
};

describe("normalized secret redaction", () => {
  test("normalizes secrets and replaces overlapping variants longest-first", () => {
    const redactor = new SecretRedactor();
    redactor.add("secret");
    redactor.add("secret-long");
    redactor.add("pre\u001b[31m\u0001\u202Epost");
    redactor.add("prepost");
    redactor.add("bad\ud800value");

    expect(redactor.text("secret-long secret prepost bad�value")).toBe(
      "[REDACTED] [REDACTED] [REDACTED] [REDACTED]",
    );
    expect(
      redactor.json(
        {
          "prepost-first": "secret-long",
          "bad�value-second": "secret",
        },
        limits,
      ),
    ).toEqual({
      "[REDACTED]-first": "[REDACTED]",
      "[REDACTED]-second": "[REDACTED]",
    });
  });

  test("rejects empty secrets after output normalization", () => {
    const redactor = new SecretRedactor();
    expect(() => {
      redactor.add("");
    }).toThrow("Secret value cannot be empty");
    expect(() => {
      redactor.add("\u001b[31m\u202E\u0001");
    }).toThrow("Secret value cannot be empty");
  });
});

describe("aggregate sanitization budget", () => {
  test("rejects keys that become dangerous after normalization", () => {
    expect(() => sanitizeJson({ "constr\u0001uctor": true }, limits)).toThrow(
      "Dangerous object key",
    );
    expect(() => sanitizeJson({ "__pro\u202eto__": true }, limits)).toThrow("Dangerous object key");
  });

  test("accounts cumulative UTF-8 and structural bytes across small values", () => {
    const aggregateLimits = { ...limits, maxStringBytes: 16, maxTotalBytes: 24 };
    expect(() => sanitizeJson({ a: "12345678", b: "12345678", c: "1" }, aggregateLimits)).toThrow(
      expect.objectContaining({ code: "INPUT_BUDGET_EXCEEDED" }),
    );
    expect(() => sanitizeJson([null, null, null, null, null], aggregateLimits)).toThrow(
      expect.objectContaining({ code: "INPUT_BUDGET_EXCEEDED" }),
    );
  });

  test("rejects a wide object before copying all of its small keys and values", () => {
    const wideObject = Object.fromEntries(
      Array.from({ length: 12 }, (_, index) => [`key-${String(index)}`, "v"]),
    );
    expect(() =>
      sanitizeJson(wideObject, { ...limits, maxCollectionEntries: 16, maxTotalBytes: 48 }),
    ).toThrow(expect.objectContaining({ code: "INPUT_BUDGET_EXCEEDED" }));
  });

  test("accounts JSON escape expansion without relying on a later stringify", () => {
    expect(() =>
      sanitizeJson(['"', '"', '"', '"', '"'], {
        ...limits,
        maxStringBytes: 16,
        maxTotalBytes: 24,
      }),
    ).toThrow(expect.objectContaining({ code: "INPUT_BUDGET_EXCEEDED" }));
  });
});
