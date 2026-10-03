import { describe, expect, it } from "vitest";

import {
  MAX_IDENTIFIER_BYTES,
  MAX_TEXT_BYTES,
  asClientInstanceId,
  asCommandId,
  asSessionId,
  validateText,
} from "../src/index.js";
import { captureError } from "./test-error.js";

describe("opaque identifiers", () => {
  it("accepts non-UUID identifiers without trimming or rewriting them", () => {
    expect(asSessionId("session/中文/01")).toBe("session/中文/01");
  });

  it("rejects empty identifiers", () => {
    expect(captureError(() => asCommandId(""))).toMatchObject({
      code: "invalid_argument",
      details: { field: "commandId", reason: "empty" },
    });
  });

  it("measures the 128-byte limit in UTF-8 bytes", () => {
    expect(MAX_IDENTIFIER_BYTES).toBe(128);
    expect(asClientInstanceId("😀".repeat(32))).toBe("😀".repeat(32));
    expect(captureError(() => asClientInstanceId(`a${"😀".repeat(32)}`))).toMatchObject({
      code: "invalid_argument",
      details: { field: "clientInstanceId", reason: "too_large", limitBytes: 128 },
    });
  });

  it("rejects ill-formed Unicode instead of normalizing an isolated surrogate", () => {
    expect(captureError(() => asSessionId("session-\uD800"))).toMatchObject({
      code: "invalid_argument",
      details: { field: "sessionId", reason: "ill_formed_unicode" },
    });
  });
});

describe("bounded text", () => {
  it("accepts exactly 1 MiB and rejects one additional UTF-8 byte", () => {
    const atLimit = "a".repeat(MAX_TEXT_BYTES);
    expect(validateText(atLimit, "prompt")).toBe(atLimit);
    expect(captureError(() => validateText(`${atLimit}a`, "prompt"))).toMatchObject({
      code: "resource_exhausted",
      details: { resource: "prompt", limitBytes: 1024 * 1024 },
    });
  });

  it("measures multi-byte text by UTF-8 bytes", () => {
    expect(validateText("😀".repeat(MAX_TEXT_BYTES / 4), "prompt")).toHaveLength(
      MAX_TEXT_BYTES / 2,
    );
    expect(() => validateText(`😀${"a".repeat(MAX_TEXT_BYTES - 3)}`, "prompt")).toThrow();
  });

  it("rejects ill-formed Unicode text", () => {
    expect(captureError(() => validateText("prompt-\uD800", "prompt"))).toMatchObject({
      code: "invalid_argument",
      details: { field: "prompt", reason: "ill_formed_unicode" },
    });
  });
});
