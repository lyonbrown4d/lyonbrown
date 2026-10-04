import { readFile } from "node:fs/promises";

import { describe, expect, it } from "vitest";

const packageRoot = new URL("../", import.meta.url);
const repositoryRoot = new URL("../../../", import.meta.url);

describe("node-llama-cpp supply-chain policy", () => {
  it("pins node-llama-cpp as a required runtime dependency", async () => {
    const manifest = JSON.parse(await readFile(new URL("package.json", packageRoot), "utf8")) as {
      dependencies?: Record<string, string>;
      optionalDependencies?: Record<string, string>;
    };

    expect(manifest.dependencies?.["node-llama-cpp"]).toBe("3.22.1");
    expect(manifest.optionalDependencies?.["node-llama-cpp"]).toBeUndefined();
  });

  it("keeps dependency scripts disabled and explicitly skips upstream postinstall", async () => {
    const workspace = await readFile(new URL("pnpm-workspace.yaml", repositoryRoot), "utf8");
    const rootManifest = JSON.parse(
      await readFile(new URL("package.json", repositoryRoot), "utf8"),
    ) as { config?: { nodeLlamaCppPostinstall?: string } };

    expect(workspace).toMatch(/ignoredBuiltDependencies:[\s\S]*- node-llama-cpp/);
    expect(rootManifest.config?.nodeLlamaCppPostinstall).toBe("skip");
  });

  it("locks the matching official Windows CPU prebuild", async () => {
    const lockfile = await readFile(new URL("pnpm-lock.yaml", repositoryRoot), "utf8");

    expect(lockfile).toContain("'@node-llama-cpp/win-x64@3.22.1':");
    expect(lockfile).toContain("node-llama-cpp@3.22.1");
  });
});
