import { describe, expect, it } from "vitest";

import {
  asClientInstanceId,
  asCommandId,
  canonicalEncode,
  commandFingerprint,
  createReplayKey,
} from "../src/index.js";
import { captureError } from "./test-error.js";

describe("replay keys", () => {
  it("is an unambiguous tuple of client and command identifiers", () => {
    expect(createReplayKey(asClientInstanceId("a:b"), asCommandId("c"))).not.toBe(
      createReplayKey(asClientInstanceId("a"), asCommandId("b:c")),
    );
    expect(createReplayKey(asClientInstanceId("client"), asCommandId("command"))).toBe(
      "6:client7:command",
    );
  });
});

describe("canonical command fingerprints", () => {
  it("is stable across object insertion order", () => {
    const first = commandFingerprint("StartTurn", { text: "hello", options: { b: 2, a: 1 } });
    const second = commandFingerprint("StartTurn", { options: { a: 1, b: 2 }, text: "hello" });
    expect(first).toBe(second);
    expect(first).toMatch(/^[a-f0-9]{64}$/u);
  });

  it("distinguishes missing, null, oneof alternatives, and scalar types", () => {
    const values: unknown[] = [
      {},
      { value: null },
      { text: "1" },
      { number: 1 },
      { first: { text: "same" } },
      { second: { text: "same" } },
      { revision: 1n },
    ];
    const fingerprints = values.map((value) => commandFingerprint("Command", value));
    expect(new Set(fingerprints).size).toBe(values.length);
  });

  it("uses a deterministic encoding for Unicode, bytes, bigint, and negative zero", () => {
    expect(canonicalEncode(["中😀", new Uint8Array([0, 255]), 12n, -0])).toMatchInlineSnapshot(
      `"a4[s7:中😀y2:00ffi2:12d2:-0]"`,
    );
  });

  it.each([
    ["undefined", undefined],
    ["non-finite", Number.POSITIVE_INFINITY],
    ["unsafe integer", Number.MAX_SAFE_INTEGER + 1],
    ["function", () => undefined],
    ["symbol", Symbol("unsafe")],
    ["date", new Date(0)],
    ["sparse array", Array(1)],
  ])("rejects unsupported %s values", (_label, value) => {
    expect(captureError(() => canonicalEncode(value))).toMatchObject({
      code: "invalid_argument",
    });
  });

  it("rejects cyclic objects", () => {
    const cyclic: { self?: unknown } = {};
    cyclic.self = cyclic;
    expect(captureError(() => canonicalEncode(cyclic))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "cyclic_value" },
    });
  });

  it.each(["__proto__", "prototype", "constructor"])(
    "rejects prototype-polluting key %s",
    (dangerousKey) => {
      const value = Object.create(null) as Record<string, unknown>;
      value[dangerousKey] = "blocked";
      expect(captureError(() => canonicalEncode(value))).toMatchObject({
        code: "invalid_argument",
        details: { reason: "dangerous_key", key: dangerousKey },
      });
      expect(({} as { blocked?: string }).blocked).toBeUndefined();
    },
  );

  it("rejects accessors without invoking them", () => {
    let invoked = false;
    const value = Object.defineProperty({}, "secret", {
      enumerable: true,
      get() {
        invoked = true;
        return "unsafe";
      },
    });
    expect(captureError(() => canonicalEncode(value))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "accessor_property" },
    });
    expect(invoked).toBe(false);
  });
});
