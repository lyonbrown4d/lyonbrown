import { describe, expect, it } from "vitest";

import {
  PROTOCOL_VERSION,
  asClientInstanceId,
  asCommandId,
  asServerInstanceId,
  createCommandMetadata,
  createCommandReceipt,
  validateCommandMetadata,
  validateCommandReceipt,
} from "../src/index.js";
import { captureError } from "./test-error.js";

describe("command contracts", () => {
  const commandId = asCommandId("command-01");

  it("creates epoch-bound metadata without inventing an epoch", () => {
    expect(
      createCommandMetadata({
        clientInstanceId: asClientInstanceId("client-01"),
        commandId,
        expectedServerInstanceId: asServerInstanceId("epoch-01"),
      }),
    ).toEqual({
      protocolVersion: PROTOCOL_VERSION,
      clientInstanceId: "client-01",
      commandId: "command-01",
      expectedServerInstanceId: "epoch-01",
    });
  });

  it("preserves absence of the optional epoch", () => {
    expect(
      createCommandMetadata({
        clientInstanceId: asClientInstanceId("client-01"),
        commandId,
      }),
    ).not.toHaveProperty("expectedServerInstanceId");
  });

  it("validates metadata at the untrusted adapter boundary", () => {
    const metadata = {
      protocolVersion: PROTOCOL_VERSION,
      clientInstanceId: "client-01",
      commandId: "command-01",
      expectedServerInstanceId: "epoch-01",
    };
    expect(validateCommandMetadata(metadata)).toEqual(metadata);
    expect(
      captureError(() =>
        validateCommandMetadata({ ...metadata, protocolVersion: PROTOCOL_VERSION + 1 }),
      ),
    ).toMatchObject({
      code: "failed_precondition",
      details: { reason: "protocol_version_mismatch" },
    });
    expect(
      captureError(() => validateCommandMetadata({ ...metadata, expectedServerInstanceId: "" })),
    ).toMatchObject({
      code: "invalid_argument",
      details: { field: "serverInstanceId", reason: "empty" },
    });
  });

  it("validates accepted receipts against protocol and command identity", () => {
    const receipt = createCommandReceipt(commandId, true);
    expect(validateCommandReceipt(receipt, commandId)).toEqual(receipt);
    expect(
      captureError(() =>
        validateCommandReceipt({ ...receipt, protocolVersion: PROTOCOL_VERSION + 1 }, commandId),
      ),
    ).toMatchObject({ code: "failed_precondition" });
    expect(captureError(() => validateCommandReceipt(receipt, asCommandId("other")))).toMatchObject(
      {
        code: "failed_precondition",
        details: { reason: "command_id_mismatch" },
      },
    );
  });
});
