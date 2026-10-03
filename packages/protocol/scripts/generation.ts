import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const protoRoot = join(packageRoot, "proto");
const require = createRequire(import.meta.url);
const grpcToolsRoot = dirname(require.resolve("grpc-tools/package.json"));
const protoc = join(grpcToolsRoot, "bin", process.platform === "win32" ? "protoc.exe" : "protoc");
const plugin = join(
  packageRoot,
  "node_modules",
  ".bin",
  process.platform === "win32" ? "protoc-gen-ts_proto.CMD" : "protoc-gen-ts_proto",
);

const TS_PROTO_OPTIONS = [
  "env=node",
  "exportCommonSymbols=false",
  "forceLong=bigint",
  "importSuffix=.js",
  "oneof=unions",
  "outputJsonMethods=false",
  "outputServices=grpc-js",
  "outputTypeRegistry=true",
  "useExactTypes=true",
].join(",");

export function generate(outputDirectory: string): void {
  mkdirSync(outputDirectory, { recursive: true });
  const result = spawnSync(
    protoc,
    [
      `--plugin=protoc-gen-ts_proto=${plugin}`,
      `--ts_proto_out=${outputDirectory}`,
      `--ts_proto_opt=${TS_PROTO_OPTIONS}`,
      `--proto_path=${protoRoot}`,
      "agent/v1/agent.proto",
    ],
    { encoding: "utf8" },
  );

  if (result.error !== undefined) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`protoc failed with exit code ${String(result.status)}: ${result.stderr}`);
  }
}

export const generatedDirectory = join(packageRoot, "src", "generated");
