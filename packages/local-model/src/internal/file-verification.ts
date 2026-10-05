import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { lstat } from "node:fs/promises";

import type { ModelPreset } from "./contracts.ts";

export const verifyModelFile = async (
  filePath: string,
  preset: ModelPreset,
  signal: AbortSignal,
): Promise<number> => {
  throwIfAborted(signal);
  const metadata = await lstat(filePath);
  if (
    !metadata.isFile() ||
    metadata.isSymbolicLink() ||
    !Number.isSafeInteger(metadata.size) ||
    metadata.size < 0 ||
    metadata.size > preset.maxBytes
  ) {
    throw new Error("model_integrity_failed");
  }
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(filePath)) {
    throwIfAborted(signal);
    hash.update(chunk as Buffer);
  }
  throwIfAborted(signal);
  if (hash.digest("hex") !== preset.sha256) throw new Error("model_integrity_failed");
  return metadata.size;
};

const throwIfAborted = (signal: AbortSignal): void => {
  if (signal.aborted) throw new Error("model_download_cancelled");
};
