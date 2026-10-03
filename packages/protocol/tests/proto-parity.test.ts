import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { Root } from "protobufjs";
import { describe, expect, it } from "vitest";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = resolve(packageRoot, "../..");
const nodeProtoRoot = join(packageRoot, "proto");
const rustProtoRoot = join(repositoryRoot, "crates", "agent-protocol", "proto");
const protoFiles = ["agent.proto", "commands.proto", "snapshot.proto"] as const;

function normalizedBytes(path: string): Buffer {
  return Buffer.from(readFileSync(path, "utf8").replaceAll("\r\n", "\n"), "utf8");
}

function descriptorSet(protoRoot: string): Buffer {
  const directory = mkdtempSync(join(tmpdir(), "lyonbrown-proto-"));
  const output = join(directory, "agent.pb");
  const require = createRequire(import.meta.url);
  const protocScript = require.resolve("grpc-tools/bin/protoc.js");

  try {
    const result = spawnSync(
      process.execPath,
      [
        protocScript,
        `--proto_path=${protoRoot}`,
        `--descriptor_set_out=${output}`,
        "--include_imports",
        "agent/v1/agent.proto",
      ],
      { encoding: "utf8" },
    );

    expect(result.status, result.stderr).toBe(0);
    return readFileSync(output);
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
}

describe("Rust protocol parity", () => {
  it.each(protoFiles)("keeps %s byte-equivalent after newline normalization", (file) => {
    const rust = normalizedBytes(join(rustProtoRoot, "agent", "v1", file));
    const node = normalizedBytes(join(nodeProtoRoot, "agent", "v1", file));

    expect(node).toEqual(rust);
  });

  it("produces the same protobuf descriptor set as the Rust oracle", () => {
    expect(descriptorSet(nodeProtoRoot)).toEqual(descriptorSet(rustProtoRoot));
  });
});

describe("wire schema stability", () => {
  const root = new Root();
  root.resolvePath = (_origin, target) => join(nodeProtoRoot, target);
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
});
