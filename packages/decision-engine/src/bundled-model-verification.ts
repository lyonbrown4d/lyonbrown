import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { lstat } from "node:fs/promises";
import { join } from "node:path";

import type { DecisionModelBundleIdentity } from "@lyonbrown/decision-engine/internal/model-bundle-storage";

export async function verifyDecisionModelBundle(
  directoryPath: string,
  model: DecisionModelBundleIdentity,
  signal: AbortSignal,
): Promise<void> {
  throwIfAborted(signal);
  for (const file of model.files) {
    const filePath = join(directoryPath, ...file.path.split("/"));
    const metadata = await lstat(filePath);
    if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size !== file.maxBytes) {
      throw new Error("decision_model_integrity_failed");
    }
    const hash = createHash("sha256");
    for await (const chunk of createReadStream(filePath, { signal })) {
      throwIfAborted(signal);
      hash.update(chunk as Buffer);
    }
    if (hash.digest("hex") !== file.sha256) {
      throw new Error("decision_model_integrity_failed");
    }
  }
}

function throwIfAborted(signal: AbortSignal): void {
  if (signal.aborted) throw new Error("decision_model_install_cancelled");
}
