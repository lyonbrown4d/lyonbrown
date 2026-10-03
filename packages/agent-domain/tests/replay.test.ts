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

  it("prevents isolated-surrogate and replacement-character fingerprint collisions", () => {
    const replacementFingerprint = commandFingerprint("Command", { value: "\uFFFD" });
    expect(replacementFingerprint).toMatch(/^[a-f0-9]{64}$/u);
    expect(captureError(() => commandFingerprint("Command", { value: "\uD800" }))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "ill_formed_unicode" },
    });
    expect(captureError(() => canonicalEncode({ "\uD800": "value" }))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "ill_formed_unicode" },
    });
  });

  it.each([
    ["plain-object", {}],
    ["date", new Date(0)],
  ])("rejects a %s proxy before invoking reflection traps", (_label, target) => {
    let trapCount = 0;
    const value = new Proxy(target, {
      getPrototypeOf() {
        trapCount += 1;
        return Object.prototype;
      },
      ownKeys() {
        trapCount += 1;
        return [];
      },
      getOwnPropertyDescriptor() {
        trapCount += 1;
        return undefined;
      },
      get() {
        trapCount += 1;
        return undefined;
      },
    });
    expect(captureError(() => canonicalEncode(value))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "proxy_value" },
    });
    expect(trapCount).toBe(0);
  });

  it("rejects array accessors without invoking them", () => {
    let invoked = false;
    const value = [1];
    Object.defineProperty(value, "0", {
      configurable: true,
      enumerable: true,
      get() {
        invoked = true;
        return 1;
      },
    });
    expect(captureError(() => canonicalEncode(value))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "accessor_property", key: "0" },
    });
    expect(invoked).toBe(false);
  });

  it("rejects array expando properties without invoking accessors", () => {
    let invoked = false;
    const value = [1];
    Object.defineProperty(value, "extra", {
      configurable: true,
      enumerable: true,
      get() {
        invoked = true;
        return "unsafe";
      },
    });
    expect(captureError(() => canonicalEncode(value))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "array_extra_property", key: "extra" },
    });
    expect(invoked).toBe(false);
  });

  it("rejects huge sparse arrays without iterating their declared length", () => {
    expect(captureError(() => canonicalEncode(new Array(4_294_967_295)))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "sparse_array" },
    });
  });

  it("rejects Array subclasses", () => {
    class CustomArray extends Array<number> {}
    expect(captureError(() => canonicalEncode(new CustomArray(1, 2)))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "unsupported_array" },
    });
  });

  it("supports exact Uint8Array and Buffer values", () => {
    expect(canonicalEncode(new Uint8Array([0, 127, 255]))).toBe("y3:007fff");
    expect(canonicalEncode(Buffer.from([0, 127, 255]))).toBe("y3:007fff");
  });

  it("rejects Uint8Array subclasses", () => {
    class CustomBytes extends Uint8Array {}
    expect(captureError(() => canonicalEncode(new CustomBytes([1, 2])))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "unsupported_byte_array" },
    });
  });

  it("rejects Buffer values with a non-standard prototype", () => {
    const value = Buffer.from([1, 2]);
    const bufferPrototype = Reflect.getPrototypeOf(value);
    expect(bufferPrototype).not.toBeNull();
    const alteredPrototype: object = {};
    expect(Reflect.setPrototypeOf(alteredPrototype, bufferPrototype)).toBe(true);
    expect(Reflect.setPrototypeOf(value, alteredPrototype)).toBe(true);
    expect(captureError(() => canonicalEncode(value))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "unsupported_byte_array" },
    });
  });

  it("rejects byte-array expandos and accessors without invoking them", () => {
    let invoked = false;
    const value = new Uint8Array([1, 2]);
    Object.defineProperty(value, "extra", {
      configurable: true,
      enumerable: true,
      get() {
        invoked = true;
        return "unsafe";
      },
    });
    expect(captureError(() => canonicalEncode(value))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "byte_array_extra_property", key: "extra" },
    });
    expect(invoked).toBe(false);
  });

  it("rejects proxied byte arrays before invoking traps", () => {
    let trapCount = 0;
    const value = new Proxy(new Uint8Array([1]), {
      getPrototypeOf() {
        trapCount += 1;
        return Uint8Array.prototype;
      },
      ownKeys() {
        trapCount += 1;
        return ["0"];
      },
      get() {
        trapCount += 1;
        return 1;
      },
    });
    expect(captureError(() => canonicalEncode(value))).toMatchObject({
      code: "invalid_argument",
      details: { reason: "proxy_value" },
    });
    expect(trapCount).toBe(0);
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
