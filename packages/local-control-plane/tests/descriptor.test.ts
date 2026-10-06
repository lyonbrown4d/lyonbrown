import { describe, expect, it } from "vitest";
import {
  ControlPlaneError,
  MAX_DISCOVERY_BYTES,
  SecretToken,
  createDiscoveryDescriptor,
  parseDiscoveryDescriptor,
  serializeDiscoveryDescriptor,
  verifySecretToken,
} from "@lyonbrown/local-control-plane/internal/index";

const SCOPE_ID = "ab".repeat(32);

function validWire(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    schema_version: 1,
    transport: "grpc_tcp",
    address: "127.0.0.1:41000",
    pid: 101,
    server_instance_id: "76c903a4-08d2-46f7-aacd-b1a549094576",
    token: SecretToken.generate().exposeForTransport(),
    scope_fingerprint: SCOPE_ID,
    ...overrides,
  };
}

describe("SecretToken", () => {
  it("generates canonical 32-byte base64url tokens and verifies in constant-time code", () => {
    const token = SecretToken.generate();
    const same = SecretToken.parse(token.exposeForTransport());
    const other = SecretToken.generate();

    expect(token.exposeForTransport()).toMatch(/^[A-Za-z0-9_-]{43}$/u);
    expect(verifySecretToken(token, same)).toBe(true);
    expect(verifySecretToken(token, other)).toBe(false);
  });

  it("redacts tokens from inspection, strings, JSON, and validation errors", () => {
    const token = SecretToken.generate();
    const exposed = token.exposeForTransport();

    expect(String(token)).toBe("[REDACTED]");
    expect(JSON.stringify({ token })).not.toContain(exposed);
    expect(() => SecretToken.parse(`${exposed}=`)).toThrow(ControlPlaneError);
    try {
      SecretToken.parse(`${exposed}=`);
    } catch (error) {
      expect(String(error)).not.toContain(exposed);
    }
  });
});

describe("discovery descriptor", () => {
  it("reads the canonical versioned wire field names", () => {
    const token = SecretToken.generate().exposeForTransport();
    const wire = JSON.stringify({
      schema_version: 1,
      transport: "grpc_tcp",
      address: "127.0.0.1:41000",
      pid: 101,
      server_instance_id: "76c903a4-08d2-46f7-aacd-b1a549094576",
      token,
      scope_fingerprint: SCOPE_ID,
    });

    expect(parseDiscoveryDescriptor(wire)).toMatchObject({
      endpoint: "127.0.0.1:41000",
      scopeId: SCOPE_ID,
    });
  });

  it("round-trips a frozen, versioned descriptor", () => {
    const descriptor = createDiscoveryDescriptor({
      endpoint: "[::1]:41000",
      pid: 101,
      scopeId: SCOPE_ID,
    });
    const serialized = serializeDiscoveryDescriptor(descriptor);
    const parsed = parseDiscoveryDescriptor(serialized);

    expect(parsed.serverInstanceId).toBe(descriptor.serverInstanceId);
    expect(parsed.token.exposeForTransport()).toBe(descriptor.token.exposeForTransport());
    expect(Object.isFrozen(parsed)).toBe(true);
    expect(Buffer.byteLength(serialized)).toBeLessThanOrEqual(MAX_DISCOVERY_BYTES);
  });

  it("accepts every numeric loopback form without DNS resolution", () => {
    const ipv4 = parseDiscoveryDescriptor(
      JSON.stringify(validWire({ address: "127.255.1.2:65535" })),
    );
    const ipv6 = parseDiscoveryDescriptor(
      JSON.stringify(validWire({ address: "[0:0:0:0:0:0:0:1]:1" })),
    );

    expect(ipv4.endpoint).toBe("127.255.1.2:65535");
    expect(ipv6.endpoint).toBe("[0:0:0:0:0:0:0:1]:1");
  });

  it("uses caller-provided epoch credentials only after strict validation", () => {
    const token = SecretToken.generate();
    const descriptor = createDiscoveryDescriptor({
      endpoint: "127.0.0.1:1234",
      pid: 42,
      scopeId: SCOPE_ID,
      serverInstanceId: "76c903a4-08d2-46f7-aacd-b1a549094576",
      token,
    });

    expect(descriptor.token.exposeForTransport()).toBe(token.exposeForTransport());
    expect(() =>
      createDiscoveryDescriptor({ endpoint: "localhost:1", pid: 1, scopeId: SCOPE_ID }),
    ).toThrow(expect.objectContaining({ code: "INVALID_ENDPOINT" }));
    expect(() =>
      createDiscoveryDescriptor({ endpoint: "127.0.0.1:1", pid: 0, scopeId: SCOPE_ID }),
    ).toThrow(expect.objectContaining({ code: "INVALID_DESCRIPTOR" }));
  });

  it.each([
    ["0.0.0.0:41000", "INVALID_ENDPOINT"],
    ["192.168.1.5:41000", "INVALID_ENDPOINT"],
    ["localhost:41000", "INVALID_ENDPOINT"],
    ["127.0.0.1:0", "INVALID_ENDPOINT"],
    ["[::]:41000", "INVALID_ENDPOINT"],
  ])("rejects a non-loopback or ambiguous endpoint", (endpoint, code) => {
    expect(() =>
      parseDiscoveryDescriptor(JSON.stringify(validWire({ address: endpoint }))),
    ).toThrow(expect.objectContaining({ code }));
  });

  it.each([
    [{ pid: 0 }, "INVALID_DESCRIPTOR"],
    [{ pid: -1 }, "INVALID_DESCRIPTOR"],
    [{ pid: Number.MAX_SAFE_INTEGER + 1 }, "INVALID_DESCRIPTOR"],
    [{ server_instance_id: "00000000-0000-0000-0000-000000000000" }, "INVALID_DESCRIPTOR"],
    [{ token: "not-a-token" }, "INVALID_DESCRIPTOR"],
    [{ schema_version: 2 }, "INVALID_DESCRIPTOR"],
    [{ unexpected: true }, "INVALID_DESCRIPTOR"],
  ])("strictly rejects malformed descriptors", (override, code) => {
    expect(() => parseDiscoveryDescriptor(JSON.stringify(validWire(override)))).toThrow(
      expect.objectContaining({ code }),
    );
    expect(({} as { polluted?: boolean }).polluted).toBeUndefined();
  });

  it("rejects prototype-pollution keys without mutating Object.prototype", () => {
    const serialized = JSON.stringify(validWire());
    const polluted = `${serialized.slice(0, -1)},"__proto__":{"polluted":true}}`;

    expect(() => parseDiscoveryDescriptor(polluted)).toThrow(
      expect.objectContaining({ code: "INVALID_DESCRIPTOR" }),
    );
    expect(({} as { polluted?: boolean }).polluted).toBeUndefined();
  });

  it("rejects duplicate keys, including escaped duplicates", () => {
    const token = SecretToken.generate().exposeForTransport();
    const duplicate = `{"schema_version":1,"transport":"grpc_tcp","address":"127.0.0.1:1","pid":1,"server_instance_id":"76c903a4-08d2-46f7-aacd-b1a549094576","token":"${token}","to\\u006ben":"${token}","scope_fingerprint":"${SCOPE_ID}"}`;

    expect(() => parseDiscoveryDescriptor(duplicate)).toThrow(
      expect.objectContaining({ code: "DUPLICATE_DESCRIPTOR_KEY" }),
    );
  });

  it("rejects malformed UTF-8 and descriptors larger than 4 KiB", () => {
    expect(() => parseDiscoveryDescriptor(Buffer.from([0xc3, 0x28]))).toThrow(
      expect.objectContaining({ code: "INVALID_DESCRIPTOR" }),
    );
    expect(() => parseDiscoveryDescriptor(Buffer.alloc(MAX_DISCOVERY_BYTES + 1, 0x20))).toThrow(
      expect.objectContaining({ code: "DESCRIPTOR_TOO_LARGE" }),
    );
  });

  it.each(["", "[]", "null", "{", '{"key" 1}', '{"unknown":{"nested":true}}'])(
    "rejects malformed or non-object JSON without echoing its content",
    (serialized) => {
      expect(() => parseDiscoveryDescriptor(serialized)).toThrow(ControlPlaneError);
      try {
        parseDiscoveryDescriptor(serialized);
      } catch (error) {
        expect(String(error)).not.toContain(
          serialized.length === 0 ? "impossible sentinel" : serialized,
        );
      }
    },
  );
});
