import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { readdir } from "node:fs/promises";
import { join } from "node:path";

const SHA256 = /^[a-f0-9]{64}$/u;

export async function verifyInstalledModelCache(
  modelsDirectory: string,
  expectedSha256: string,
): Promise<void> {
  if (!SHA256.test(expectedSha256)) throw new Error("REAL_GGUF_EXPECTED_DIGEST_INVALID");
  const entries = await readdir(modelsDirectory, { withFileTypes: true });
  if (entries.some((entry) => entry.name.startsWith(".staging-"))) {
    throw new Error("REAL_GGUF_STAGING_RESIDUE");
  }
  const installed = entries.filter(
    (entry) => entry.isFile() && entry.name.startsWith("model-") && entry.name.endsWith(".gguf"),
  );
  if (installed.length !== 1) throw new Error("REAL_GGUF_INSTALLED_MODEL_COUNT_INVALID");
  const file = installed[0];
  if (file === undefined) throw new Error("REAL_GGUF_INSTALLED_MODEL_COUNT_INVALID");
  const digest = await sha256File(join(modelsDirectory, file.name));
  if (digest !== expectedSha256) throw new Error("REAL_GGUF_INSTALLED_DIGEST_MISMATCH");
}

async function sha256File(path: string): Promise<string> {
  const hash = createHash("sha256");
  const input = createReadStream(path);
  for await (const chunk of input) hash.update(chunk as Buffer);
  return hash.digest("hex");
}
