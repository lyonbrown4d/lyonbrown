import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";

import { generate, generatedDirectory } from "./generation.js";

export type GenerateSources = (outputBaseDirectory: string) => Promise<void>;

export interface CheckGeneratedOptions {
  readonly committedDirectory?: string;
  readonly generateSources?: GenerateSources;
}

async function generatedFiles(root: string): Promise<string[]> {
  const entries = await readdir(root, { recursive: true, withFileTypes: true });
  return entries
    .filter((entry) => entry.isFile() && entry.name.endsWith(".ts"))
    .map((entry) => relative(root, join(entry.parentPath, entry.name)))
    .sort();
}

async function normalizedSource(path: string): Promise<string> {
  return (await readFile(path, "utf8")).replaceAll("\r\n", "\n");
}

export async function checkGenerated(options: CheckGeneratedOptions = {}): Promise<void> {
  const committedDirectory = options.committedDirectory ?? generatedDirectory;
  const generateSources = options.generateSources ?? generate;
  const temporaryRoot = await mkdtemp(join(tmpdir(), "lyonbrown-generated-"));

  try {
    await generateSources(temporaryRoot);
    const temporaryGenerated = join(temporaryRoot, "generated");
    const [expectedFiles, actualFiles] = await Promise.all([
      generatedFiles(temporaryGenerated),
      generatedFiles(committedDirectory),
    ]);

    if (expectedFiles.join("\n") !== actualFiles.join("\n")) {
      throw new Error("generated protocol file list is stale; run pnpm generate");
    }

    const sourcePairs = await Promise.all(
      expectedFiles.map(async (file) =>
        Promise.all([
          normalizedSource(join(temporaryGenerated, file)),
          normalizedSource(join(committedDirectory, file)),
        ]),
      ),
    );
    const staleIndex = sourcePairs.findIndex(([expected, actual]) => expected !== actual);
    if (staleIndex !== -1) {
      const staleFile = expectedFiles[staleIndex];
      if (staleFile === undefined) {
        throw new Error("generated protocol file list is stale; run pnpm generate");
      }
      throw new Error(`generated protocol source is stale: ${staleFile}; run pnpm generate`);
    }
  } finally {
    await rm(temporaryRoot, { force: true, recursive: true });
  }
}
