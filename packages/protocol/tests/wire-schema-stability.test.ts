import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { Root } from "protobufjs";
import { describe, expect, it } from "vitest";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const protoRoot = join(packageRoot, "proto");
const protoFiles = ["agent.proto", "commands.proto", "snapshot.proto"] as const;

interface WireSchemaFixture {
  readonly version: 1;
  readonly normalizedSourceSha256: Readonly<Record<(typeof protoFiles)[number], string>>;
}

function normalizedSourceBytes(path: string): Buffer {
  return Buffer.from(readFileSync(path, "utf8").replaceAll("\r\n", "\n"), "utf8");
}

const fixture = JSON.parse(
  readFileSync(join(packageRoot, "tests", "fixtures", "wire-schema-v1.json"), "utf8"),
) as WireSchemaFixture;

describe("versioned wire schema source", () => {
  it("uses the supported fixture version", () => {
    expect(fixture.version).toBe(1);
  });

  it.each(protoFiles)("keeps %s source bytes stable after newline normalization", (file) => {
    const digest = createHash("sha256")
      .update(normalizedSourceBytes(join(protoRoot, "agent", "v1", file)))
      .digest("hex");
    expect(digest).toBe(fixture.normalizedSourceSha256[file]);
  });
});

describe("wire schema stability", () => {
  const root = new Root();
  root.resolvePath = (_origin, target) => join(protoRoot, target);
  root.loadSync("agent/v1/agent.proto", { keepCase: true });

  it("exports the complete AgentService method list", () => {
    const service = root.lookupService("lyonbrown.agent.v1.AgentService");

    expect(Object.keys(service.methods)).toEqual([
      "Health",
      "Handshake",
      "StartTurn",
      "SteerCurrentTurn",
      "QueueFollowUp",
      "CancelTurn",
      "ForceStop",
      "RespondApproval",
      "SelectSessionModel",
      "StartModelDownload",
      "CancelModelDownload",
      "GetSnapshot",
      "SubscribeEvents",
    ]);
    expect(service.methods.SubscribeEvents?.responseStream).toBe(true);
  });

  it("keeps compatibility-sensitive RequestMetadata field tags", () => {
    const metadata = root.lookupType("lyonbrown.agent.v1.RequestMetadata");

    expect(metadata.fields.protocol_version?.id).toBe(1);
    expect(metadata.fields.client_instance_id?.id).toBe(2);
    expect(metadata.fields.command_id?.id).toBe(3);
    expect(metadata.fields.expected_server_instance_id?.id).toBe(4);
  });

  it("keeps command error enum numbers", () => {
    const commandError = root.lookupEnum("lyonbrown.agent.v1.CommandErrorCode");

    expect(commandError.values).toMatchObject({
      COMMAND_ERROR_CODE_UNSPECIFIED: 0,
      COMMAND_ERROR_CODE_EMPTY_TEXT: 4,
      COMMAND_ERROR_CODE_MODEL_SETUP_REQUIRED: 9,
      COMMAND_ERROR_CODE_EPOCH_MISMATCH: 15,
    });
  });

  it("keeps additive managed-model recommendation field tags and enum values", () => {
    const managedModel = root.lookupType("lyonbrown.agent.v1.ManagedModel");
    const reason = root.lookupEnum("lyonbrown.agent.v1.ModelRecommendationReason");

    expect(managedModel.fields.display_name?.id).toBe(6);
    expect(managedModel.fields.recommendation_reason?.id).toBe(7);
    expect(reason.values).toMatchObject({
      MODEL_RECOMMENDATION_REASON_UNSPECIFIED: 0,
      MODEL_RECOMMENDATION_REASON_INSTALLED_PREFERRED: 1,
      MODEL_RECOMMENDATION_REASON_HARDWARE_COMPATIBLE: 2,
      MODEL_RECOMMENDATION_REASON_CONSERVATIVE_FALLBACK: 3,
    });
  });
  it("keeps decision installation purpose and snapshot tags separate from chat", () => {
    expect(root.lookupType("lyonbrown.agent.v1.StartModelDownloadRequest").fields.purpose?.id).toBe(
      3,
    );
    expect(
      root.lookupType("lyonbrown.agent.v1.CancelModelDownloadRequest").fields.purpose?.id,
    ).toBe(3);
    expect(root.lookupType("lyonbrown.agent.v1.RuntimeSnapshot").fields.decision_model?.id).toBe(
      14,
    );
    expect(root.lookupEnum("lyonbrown.agent.v1.ModelPurpose").values).toEqual({
      MODEL_PURPOSE_UNSPECIFIED: 0,
      MODEL_PURPOSE_CHAT: 1,
      MODEL_PURPOSE_DECISION: 2,
    });
    expect(
      root.lookupType("lyonbrown.agent.v1.DecisionModelSnapshot").fields.advisory_only?.id,
    ).toBe(10);
    expect(
      root.lookupEnum("lyonbrown.agent.v1.DecisionModelPhase").values
        .DECISION_MODEL_PHASE_CONFIGURED,
    ).toBe(7);
  });
});
