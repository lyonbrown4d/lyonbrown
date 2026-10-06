import { readFile } from "node:fs/promises";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const repositoryRoot = resolve(import.meta.dirname, "../../..");

describe("OpenJev supply-chain policy", () => {
  it("pins the provider and Transformers runtime exactly", async () => {
    const manifest = JSON.parse(
      await readFile(resolve(repositoryRoot, "packages/decision-engine/package.json"), "utf8"),
    ) as { readonly dependencies?: Readonly<Record<string, string>> };

    expect(manifest.dependencies?.["open-jev"]).toBe("0.1.2");
    expect(manifest.dependencies?.["@huggingface/transformers"]).toBe("4.3.0");
  });

  it("explicitly skips the ONNX package install script", async () => {
    const workspace = await readFile(resolve(repositoryRoot, "pnpm-workspace.yaml"), "utf8");

    expect(workspace).toMatch(/ignoredBuiltDependencies:[\s\S]*- onnxruntime-node/u);
  });
});
