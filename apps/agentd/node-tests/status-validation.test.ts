import { status } from "@grpc/grpc-js";
import {
  CommandErrorCode,
  CommandErrorDetail,
  PROTOCOL_VERSION,
  RequestMetadata,
} from "@lyonbrown/protocol";
import { describe, expect, it } from "vitest";
import { commandFingerprint } from "../node-src/fingerprint.ts";
import { ReplayCache, ReplayCapacityError } from "../node-src/replay-cache.ts";
import type { RuntimeFailure } from "../node-src/runtime-host.ts";
import { runtimeFailureStatus } from "../node-src/status.ts";
import {
  requireBoundedText,
  requireEpoch,
  requireMetadata,
  requireText,
  validateServerIdentity,
} from "../node-src/validation.ts";

function decodeStatusDetail(error: ReturnType<typeof runtimeFailureStatus>): CommandErrorDetail {
  const raw = error.metadata?.get("grpc-status-details-bin")[0];
  if (!Buffer.isBuffer(raw)) throw new Error("missing command detail");
  return CommandErrorDetail.decode(raw);
}

describe("typed statuses and boundary validation", () => {
  it.each<[RuntimeFailure, number, CommandErrorCode]>([
    [
      { code: "RUNTIME_STOPPED" },
      status.UNAVAILABLE,
      CommandErrorCode.COMMAND_ERROR_CODE_RUNTIME_STOPPED,
    ],
    [
      { code: "TURN_ALREADY_ACTIVE" },
      status.FAILED_PRECONDITION,
      CommandErrorCode.COMMAND_ERROR_CODE_TURN_ALREADY_ACTIVE,
    ],
    [
      { code: "NO_ACTIVE_TURN" },
      status.FAILED_PRECONDITION,
      CommandErrorCode.COMMAND_ERROR_CODE_NO_ACTIVE_TURN,
    ],
    [
      { code: "EMPTY_TEXT" },
      status.INVALID_ARGUMENT,
      CommandErrorCode.COMMAND_ERROR_CODE_EMPTY_TEXT,
    ],
    [
      { code: "STEERING", reason: "late" },
      status.FAILED_PRECONDITION,
      CommandErrorCode.COMMAND_ERROR_CODE_STEERING,
    ],
    [
      { code: "MODEL_SETUP_REQUIRED" },
      status.FAILED_PRECONDITION,
      CommandErrorCode.COMMAND_ERROR_CODE_MODEL_SETUP_REQUIRED,
    ],
  ])("maps $0.code to a raw typed detail", (failure, grpcCode, detailCode) => {
    const error = runtimeFailureStatus(failure);
    expect(error.code).toBe(grpcCode);
    expect(decodeStatusDetail(error).code).toBe(detailCode);
  });

  it("rejects malformed service identities and request metadata", () => {
    expect(() => validateServerIdentity("", "a".repeat(64))).toThrow("serverInstanceId");
    expect(() => validateServerIdentity("server", "not-a-hash")).toThrow("scopeFingerprint");
    expect(() => requireMetadata(undefined)).toThrow("metadata is required");
    expect(() =>
      requireMetadata(RequestMetadata.create({ protocolVersion: PROTOCOL_VERSION + 1 })),
    ).toThrow("unsupported protocol_version");
    expect(() =>
      requireMetadata(
        RequestMetadata.create({
          protocolVersion: PROTOCOL_VERSION,
          clientInstanceId: "",
          commandId: "command",
        }),
      ),
    ).toThrow("client_instance_id");
    expect(() =>
      requireMetadata(
        RequestMetadata.create({
          protocolVersion: PROTOCOL_VERSION,
          clientInstanceId: "client",
          commandId: "",
        }),
      ),
    ).toThrow("command_id");
  });

  it("validates text, epochs, and framed fingerprints", () => {
    expect(() => requireText("text", " ")).toThrow("required");
    expect(() => requireText("text", "\ud800")).toThrow("invalid");
    expect(() => requireBoundedText("name", "abc", 2)).toThrow("exceeds 2 bytes");
    expect(() => requireBoundedText("name", " ", 2)).toThrow("required");
    expect(() => requireBoundedText("name", "\ud800", 2)).toThrow("invalid");
    expect(() =>
      requireEpoch(RequestMetadata.create({ expectedServerInstanceId: "old" }), "current"),
    ).toThrow("server instance changed");
    expect(commandFingerprint("kind", ["ab", "c"])).not.toBe(
      commandFingerprint("kind", ["a", "bc"]),
    );
  });

  it("bounds pending replay entries and validates capacity", async () => {
    expect(() => new ReplayCache(0)).toThrow(RangeError);
    const cache = new ReplayCache<number>(1);
    let release = (): void => undefined;
    const first = cache.execute(
      "one",
      "one",
      () =>
        new Promise<number>((resolve) => {
          release = () => resolve(1);
        }),
    );
    await expect(cache.execute("two", "two", () => Promise.resolve(2))).rejects.toBeInstanceOf(
      ReplayCapacityError,
    );
    release();
    await expect(first).resolves.toBe(1);
  });
});
