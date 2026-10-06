import { describe, expect, it } from "vitest";

import {
  REPLAY_FINGERPRINT_MAX_BYTE_ARRAY_BYTES,
  REPLAY_FINGERPRINT_MAX_CONTAINER_WIDTH,
  REPLAY_FINGERPRINT_MAX_DEPTH,
  REPLAY_FINGERPRINT_MAX_ENCODED_BYTES,
  REPLAY_FINGERPRINT_MAX_NODES,
  canonicalEncode,
} from "@lyonbrown/agent-domain/internal/index";
import { captureError } from "@lyonbrown/agent-domain/test/test-error";

function nestedArray(depth: number): unknown {
  let value: unknown = null;
  for (let index = 0; index < depth; index += 1) value = [value];
  return value;
}

function nodeBudgetTree(extraLeafCount: number): unknown[] {
  const children = Array.from({ length: REPLAY_FINGERPRINT_MAX_CONTAINER_WIDTH - 1 }, () => [
    null,
    null,
    null,
  ]);
  const first = children[0];
  if (first === undefined) throw new Error("node budget fixture requires a non-empty tree");
  for (let index = 0; index < 3 + extraLeafCount; index += 1) first.push(null);
  return children;
}

function payloadLengthForEncodedLimit(limit: number): number {
  for (let digits = 1; digits <= String(limit).length; digits += 1) {
    const payloadLength = limit - digits - 2;
    if (String(payloadLength).length === digits) return payloadLength;
  }
  throw new Error("unable to construct encoded-size fixture");
}

describe("replay fingerprint budgets", () => {
  it("enforces the depth budget at the exact boundary", () => {
    expect(() => canonicalEncode(nestedArray(REPLAY_FINGERPRINT_MAX_DEPTH))).not.toThrow();
    expect(
      captureError(() => canonicalEncode(nestedArray(REPLAY_FINGERPRINT_MAX_DEPTH + 1))),
    ).toMatchObject({
      code: "resource_exhausted",
      details: {
        resource: "replay_fingerprint_depth",
        limit: BigInt(REPLAY_FINGERPRINT_MAX_DEPTH),
      },
    });
    expect(captureError(() => canonicalEncode(nestedArray(5_000)))).toMatchObject({
      code: "resource_exhausted",
      details: { resource: "replay_fingerprint_depth" },
    });
  });

  it("enforces container width before reflecting wide arrays", () => {
    expect(() =>
      canonicalEncode(new Array(REPLAY_FINGERPRINT_MAX_CONTAINER_WIDTH).fill(null)),
    ).not.toThrow();
    expect(
      captureError(() =>
        canonicalEncode(new Array(REPLAY_FINGERPRINT_MAX_CONTAINER_WIDTH + 1).fill(null)),
      ),
    ).toMatchObject({
      code: "resource_exhausted",
      details: {
        resource: "replay_fingerprint_container_width",
        limit: BigInt(REPLAY_FINGERPRINT_MAX_CONTAINER_WIDTH),
      },
    });
  });

  it("enforces typed-array bytes at the exact boundary", () => {
    expect(() =>
      canonicalEncode(new Uint8Array(REPLAY_FINGERPRINT_MAX_BYTE_ARRAY_BYTES)),
    ).not.toThrow();
    expect(
      captureError(() =>
        canonicalEncode(new Uint8Array(REPLAY_FINGERPRINT_MAX_BYTE_ARRAY_BYTES + 1)),
      ),
    ).toMatchObject({
      code: "resource_exhausted",
      details: {
        resource: "replay_fingerprint_byte_array_bytes",
        limitBytes: REPLAY_FINGERPRINT_MAX_BYTE_ARRAY_BYTES,
      },
    });
  });

  it("enforces total nodes at the exact boundary", () => {
    expect(REPLAY_FINGERPRINT_MAX_NODES).toBe(16_384);
    expect(() => canonicalEncode(nodeBudgetTree(0))).not.toThrow();
    expect(captureError(() => canonicalEncode(nodeBudgetTree(1)))).toMatchObject({
      code: "resource_exhausted",
      details: {
        resource: "replay_fingerprint_nodes",
        limit: BigInt(REPLAY_FINGERPRINT_MAX_NODES),
      },
    });
  });

  it("enforces encoded UTF-8 bytes at the exact boundary", () => {
    const payloadLength = payloadLengthForEncodedLimit(REPLAY_FINGERPRINT_MAX_ENCODED_BYTES);
    const atLimit = "a".repeat(payloadLength);
    expect(Buffer.byteLength(canonicalEncode(atLimit), "utf8")).toBe(
      REPLAY_FINGERPRINT_MAX_ENCODED_BYTES,
    );
    expect(captureError(() => canonicalEncode(`${atLimit}a`))).toMatchObject({
      code: "resource_exhausted",
      details: {
        resource: "replay_fingerprint_encoded_bytes",
        limitBytes: REPLAY_FINGERPRINT_MAX_ENCODED_BYTES,
      },
    });
  });
});
