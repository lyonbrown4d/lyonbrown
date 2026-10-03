import { describe, expect, it } from "vitest";

import {
  AUTHORIZATION_METADATA_KEY,
  GRPC_CHANNEL_OPTIONS,
  MAX_GRPC_MESSAGE_BYTES,
  MAX_STRING_ID_BYTES,
  MAX_TEXT_BYTES,
  METADATA_KEYS,
  PROTOCOL_VERSION,
  authorizeBearer,
  bearerAuthorization,
  validateStringId,
  validateText,
} from "../src/index.js";

describe("protocol constants", () => {
  it("matches the version, grpc message limit, and metadata names", () => {
    expect(PROTOCOL_VERSION).toBe(1);
    expect(MAX_GRPC_MESSAGE_BYTES).toBe(16 * 1024 * 1024);
    expect(MAX_STRING_ID_BYTES).toBe(128);
    expect(MAX_TEXT_BYTES).toBe(1024 * 1024);
    expect(GRPC_CHANNEL_OPTIONS).toEqual({
      "grpc.max_receive_message_length": 16 * 1024 * 1024,
      "grpc.max_send_message_length": 16 * 1024 * 1024,
    });
    expect(AUTHORIZATION_METADATA_KEY).toBe("authorization");
    expect(METADATA_KEYS).toEqual({
      protocolVersion: "protocol_version",
      clientInstanceId: "client_instance_id",
      commandId: "command_id",
      expectedServerInstanceId: "expected_server_instance_id",
      authorization: "authorization",
    });
  });
});

describe("transport boundary validation", () => {
  it("counts string ID limits in UTF-8 bytes", () => {
    expect(validateStringId("界".repeat(42))).toEqual({ ok: true, bytes: 126 });
    expect(validateStringId("界".repeat(43))).toEqual({
      ok: false,
      code: "STRING_ID_TOO_LARGE",
      bytes: 129,
      limit: 128,
    });
  });

  it("rejects an empty string ID with a machine-readable code", () => {
    expect(validateStringId("")).toEqual({
      ok: false,
      code: "STRING_ID_EMPTY",
      bytes: 0,
      limit: 128,
    });
  });

  it("allows exactly one MiB of UTF-8 text and rejects one byte more", () => {
    expect(validateText("a".repeat(MAX_TEXT_BYTES))).toEqual({
      ok: true,
      bytes: MAX_TEXT_BYTES,
    });
    expect(validateText(`${"a".repeat(MAX_TEXT_BYTES)}a`)).toEqual({
      ok: false,
      code: "TEXT_TOO_LARGE",
      bytes: MAX_TEXT_BYTES + 1,
      limit: MAX_TEXT_BYTES,
    });
  });
});

describe("bearer authentication", () => {
  const token = "A".repeat(43);
  const otherToken = Buffer.alloc(32, 1).toString("base64url");

  it("creates and verifies canonical bearer metadata", () => {
    const authorization = bearerAuthorization(token);

    expect(authorization).toBe(`Bearer ${token}`);
    expect(authorizeBearer(authorization, token)).toEqual({ ok: true });
    expect(authorizeBearer(`bearer ${token}`, token)).toEqual({ ok: true });
  });

  it("returns stable codes for missing, malformed, and unauthorized values", () => {
    expect(authorizeBearer(undefined, token)).toEqual({ ok: false, code: "MISSING" });
    expect(authorizeBearer("Basic abc", token)).toEqual({ ok: false, code: "MALFORMED" });
    expect(authorizeBearer(`Bearer ${otherToken}`, token)).toEqual({
      ok: false,
      code: "UNAUTHORIZED",
    });
  });

  it("rejects metadata injection and non-canonical tokens", () => {
    expect(() => bearerAuthorization(`${token}\r\nattack`)).toThrow();
    expect(authorizeBearer(`Bearer ${token} extra`, token)).toEqual({
      ok: false,
      code: "MALFORMED",
    });
    expect(authorizeBearer(`Bearer ${"=".repeat(43)}`, token)).toEqual({
      ok: false,
      code: "MALFORMED",
    });
  });
});
