import { describe, expect, it } from "vitest";

import { PayloadValidationError, decodeEventPayload, encodeEventPayload } from "../src/index.js";

describe("event payload codec", () => {
  it("round-trips safe JSON without inheriting attacker-controlled prototypes", () => {
    const encoded = encodeEventPayload({ nested: { ok: true }, values: [1, "two", null] });
    const decoded = decodeEventPayload(encoded) as Record<string, unknown>;

    expect(decoded).toEqual({ nested: { ok: true }, values: [1, "two", null] });
    expect(Object.getPrototypeOf(decoded)).toBeNull();
    expect(Object.getPrototypeOf(decoded.nested as object)).toBeNull();
  });

  it.each([
    ["bigint", { value: 1n }],
    ["accessor", Object.defineProperty({}, "value", { get: () => "secret", enumerable: true })],
    ["prototype", Object.assign(Object.create({ inherited: true }), { ok: true })],
    ["dangerous key", JSON.parse('{"__proto__":{"polluted":true}}')],
    ["non-finite number", { value: Number.NaN }],
    ["lone surrogate", { value: "\ud800" }],
    ["resolved secret", { secretRef: "vault:item", resolvedValue: "plaintext" }],
  ])("rejects unsafe %s payloads", (_label, payload) => {
    expect(() => encodeEventPayload(payload)).toThrow(PayloadValidationError);
  });

  it("rejects proxies before their traps can serialize data", () => {
    const payload = new Proxy(
      { value: "unsafe" },
      {
        ownKeys: () => {
          throw new Error("trap must not run");
        },
      },
    );

    expect(() => encodeEventPayload(payload)).toThrow(PayloadValidationError);
  });

  it("enforces depth, node and encoded-byte budgets", () => {
    let deep: unknown = "leaf";
    for (let index = 0; index < 66; index += 1) deep = { child: deep };

    expect(() => encodeEventPayload(deep)).toThrow(/depth/i);
    expect(() => encodeEventPayload(new Array(100_001).fill(null))).toThrow(/node/i);
    expect(() => encodeEventPayload("x".repeat(1024 * 1024))).toThrow(/1 MiB/i);
  });

  it("fails closed when persisted JSON is malformed or contains forbidden keys", () => {
    expect(() => decodeEventPayload("{")).toThrow(PayloadValidationError);
    expect(() => decodeEventPayload('{"constructor":{}}')).toThrow(PayloadValidationError);
  });
});
