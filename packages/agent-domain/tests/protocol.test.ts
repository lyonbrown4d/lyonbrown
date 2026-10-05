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
} from "../src/index.ts";
import { captureError } from "./test-error.ts";

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

  it.each([null, [], "metadata", 1])("rejects non-record metadata: %j", (value) => {
    expect(captureError(() => validateCommandMetadata(value))).toMatchObject({
      code: "invalid_argument",
      details: { field: "metadata", reason: "invalid_record" },
    });
  });

  it("rejects missing metadata fields", () => {
    expect(
      captureError(() =>
        validateCommandMetadata({
          protocolVersion: PROTOCOL_VERSION,
          clientInstanceId: "client-01",
        }),
      ),
    ).toMatchObject({
      code: "invalid_argument",
      details: { field: "metadata.commandId", reason: "missing_required_field" },
    });
  });

  it.each([null, undefined, ""])("rejects an invalid optional server epoch: %j", (epoch) => {
    expect(
      captureError(() =>
        validateCommandMetadata({
          protocolVersion: PROTOCOL_VERSION,
          clientInstanceId: "client-01",
          commandId: "command-01",
          expectedServerInstanceId: epoch,
        }),
      ),
    ).toMatchObject({ code: "invalid_argument" });
  });

  it("rejects metadata accessors and proxies without invoking them", () => {
    let getterInvoked = false;
    const accessorMetadata = Object.defineProperty(
      { protocolVersion: PROTOCOL_VERSION, clientInstanceId: "client-01" },
      "commandId",
      {
        enumerable: true,
        get() {
          getterInvoked = true;
          return "command-01";
        },
      },
    );
    expect(captureError(() => validateCommandMetadata(accessorMetadata))).toMatchObject({
      code: "invalid_argument",
      details: { field: "metadata.commandId", reason: "accessor_property" },
    });
    expect(getterInvoked).toBe(false);

    let trapCount = 0;
    const proxyMetadata = new Proxy(
      {
        protocolVersion: PROTOCOL_VERSION,
        clientInstanceId: "client-01",
        commandId: "command-01",
      },
      {
        getPrototypeOf() {
          trapCount += 1;
          return Object.prototype;
        },
        ownKeys() {
          trapCount += 1;
          return [];
        },
      },
    );
    expect(captureError(() => validateCommandMetadata(proxyMetadata))).toMatchObject({
      code: "invalid_argument",
      details: { field: "metadata", reason: "proxy_record" },
    });
    expect(trapCount).toBe(0);
  });

  it("rejects metadata records with dangerous prototypes", () => {
    const metadata = Object.create({ inherited: true }) as Record<string, unknown>;
    metadata.protocolVersion = PROTOCOL_VERSION;
    metadata.clientInstanceId = "client-01";
    metadata.commandId = "command-01";
    expect(captureError(() => validateCommandMetadata(metadata))).toMatchObject({
      code: "invalid_argument",
      details: { field: "metadata", reason: "invalid_record_prototype" },
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

  it.each([null, [], { protocolVersion: PROTOCOL_VERSION }, { accepted: true }])(
    "rejects malformed receipts: %j",
    (value) => {
      expect(captureError(() => validateCommandReceipt(value))).toMatchObject({
        code: "invalid_argument",
      });
    },
  );

  it("rejects receipt accessors without invoking them", () => {
    let invoked = false;
    const value = Object.defineProperty(
      { protocolVersion: PROTOCOL_VERSION, accepted: true },
      "commandId",
      {
        enumerable: true,
        get() {
          invoked = true;
          return "command-01";
        },
      },
    );
    expect(captureError(() => validateCommandReceipt(value))).toMatchObject({
      code: "invalid_argument",
      details: { field: "receipt.commandId", reason: "accessor_property" },
    });
    expect(invoked).toBe(false);
  });
});
