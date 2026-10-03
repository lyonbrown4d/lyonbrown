import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";

import { generate, generatedDirectory } from "./generation.js";

function generatedFiles(root: string): string[] {
  return readdirSync(root, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith(".ts"))
    .map((entry) => relative(root, join(entry.parentPath, entry.name)))
    .sort();
}

function normalizedSource(path: string): string {
  return readFileSync(path, "utf8").replaceAll("\r\n", "\n");
}

const temporaryRoot = mkdtempSync(join(tmpdir(), "lyonbrown-generated-"));
try {
  generate(temporaryRoot);
  const expectedFiles = generatedFiles(temporaryRoot);
  const actualFiles = generatedFiles(generatedDirectory);

  if (expectedFiles.join("\n") !== actualFiles.join("\n")) {
    throw new Error("generated protocol file list is stale; run pnpm generate");
  }

  for (const file of expectedFiles) {
    if (
      normalizedSource(join(temporaryRoot, file)) !==
      normalizedSource(join(generatedDirectory, file))
    ) {
      throw new Error(`generated protocol source is stale: ${file}; run pnpm generate`);
    }
  }
} finally {
  rmSync(temporaryRoot, { force: true, recursive: true });
}
