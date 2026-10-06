import { cp, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { runBufWithZx, protocolPackageRoot } from "@lyonbrown/protocol/script/generation";

const FORMAT_TIMEOUT_MS = 30_000;

async function assertFormattedCopyMatches(source: string, formatted: string): Promise<void> {
  const relativeFiles = (await readdir(source, { recursive: true }))
    .filter((relativePath) => relativePath.endsWith(".proto"))
    .sort();

  for (const relativePath of relativeFiles) {
    const [sourceBytes, formattedBytes] = await Promise.all([
      readFile(join(source, relativePath)),
      readFile(join(formatted, relativePath)),
    ]);
    if (!sourceBytes.equals(formattedBytes)) {
      throw new Error(`Proto file is not formatted: ${relativePath}`);
    }
  }
}

const mode = process.argv[2];
if (mode !== "write" && mode !== "check") {
  throw new Error("Usage: proto-format <write|check>");
}

const sourceProto = join(protocolPackageRoot, "proto");
const temporaryRoot = await mkdtemp(join(tmpdir(), "lyonbrown-proto-format-"));
const formattedProto = join(temporaryRoot, "formatted");
try {
  await runBufWithZx({
    args: ["format", "proto", "--output", formattedProto],
    cwd: protocolPackageRoot,
    timeoutMs: FORMAT_TIMEOUT_MS,
  });
  if (mode === "write") {
    await cp(formattedProto, sourceProto, { force: true, recursive: true });
  } else {
    await assertFormattedCopyMatches(sourceProto, formattedProto);
  }
} finally {
  await rm(temporaryRoot, { force: true, recursive: true });
}
