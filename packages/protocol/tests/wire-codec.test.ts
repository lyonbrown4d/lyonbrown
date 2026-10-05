import { describe, expect, it } from "vitest";

import {
  AgentServiceService,
  CommandErrorDetail,
  ModelOption,
  ModelOptionDisabledReason,
  ModelSelection,
  ModelSetupSnapshot,
  RuntimeEvent,
} from "../src/index.ts";

describe("generated wire contracts", () => {
  it("round-trips uint64 values beyond Number.MAX_SAFE_INTEGER as bigint", () => {
    const revision = BigInt(Number.MAX_SAFE_INTEGER) + 42n;
    const encoded = RuntimeEvent.encode(
      RuntimeEvent.fromPartial({ protocolVersion: 1, revision }),
    ).finish();

    expect(RuntimeEvent.decode(encoded).revision).toBe(revision);
  });

  it("preserves absent versus explicitly present optional enum fields", () => {
    const absent = ModelOption.decode(ModelOption.encode(ModelOption.fromPartial({})).finish());
    const present = ModelOption.decode(
      ModelOption.encode(
        ModelOption.fromPartial({
          disabledReason: ModelOptionDisabledReason.MODEL_OPTION_DISABLED_REASON_UNSPECIFIED,
        }),
      ).finish(),
    );

    expect(absent.disabledReason).toBeUndefined();
    expect(present.disabledReason).toBe(
      ModelOptionDisabledReason.MODEL_OPTION_DISABLED_REASON_UNSPECIFIED,
    );
  });

  it("preserves absent versus explicitly present optional uint64 fields", () => {
    const absent = ModelSetupSnapshot.decode(
      ModelSetupSnapshot.encode(ModelSetupSnapshot.fromPartial({})).finish(),
    );
    const present = ModelSetupSnapshot.decode(
      ModelSetupSnapshot.encode(ModelSetupSnapshot.fromPartial({ totalBytes: 0n })).finish(),
    );

    expect(absent.totalBytes).toBeUndefined();
    expect(present.totalBytes).toBe(0n);
  });

  it("preserves oneof presence even when the scalar carries its default value", () => {
    const selection = ModelSelection.fromPartial({
      selection: { $case: "auto", auto: false },
    });
    const decoded = ModelSelection.decode(ModelSelection.encode(selection).finish());

    expect(decoded.selection).toEqual({ $case: "auto", auto: false });
  });

  it("preserves oneof enum context at value zero", () => {
    const detail = CommandErrorDetail.fromPartial({
      context: { $case: "unsupportedCommand", unsupportedCommand: 0 },
    });
    const decoded = CommandErrorDetail.decode(CommandErrorDetail.encode(detail).finish());

    expect(decoded.context).toEqual({ $case: "unsupportedCommand", unsupportedCommand: 0 });
  });

  it("exposes generated grpc-js client and server method descriptors", () => {
    expect(Object.keys(AgentServiceService)).toEqual([
      "health",
      "handshake",
      "startTurn",
      "steerCurrentTurn",
      "queueFollowUp",
      "cancelTurn",
      "forceStop",
      "respondApproval",
      "selectSessionModel",
      "startModelDownload",
      "cancelModelDownload",
      "getSnapshot",
      "subscribeEvents",
    ]);
    expect(AgentServiceService.subscribeEvents.responseStream).toBe(true);
  });
});
