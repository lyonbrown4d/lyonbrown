import { readFile } from "node:fs/promises";

import { describe, expect, it } from "vitest";

const packageRoot = new URL("../", import.meta.url);
const repositoryRoot = new URL("../../../", import.meta.url);

describe("node-llama-cpp supply-chain policy", () => {
  it("keeps the root entry path-free and requires an explicit secure composition capability", async () => {
    const rootEntry = await readFile(new URL("src/index.ts", packageRoot), "utf8");
    const composition = await readFile(
      new URL("src/internal/composed-service.ts", packageRoot),
      "utf8",
    );
    const declarationCheck = await readFile(
      new URL("scripts/check-public-api.ts", packageRoot),
      "utf8",
    );
    const manifest = JSON.parse(await readFile(new URL("package.json", packageRoot), "utf8")) as {
      exports?: Record<string, unknown>;
    };

    expect(rootEntry).not.toContain("createDefaultLocalModelService");
    expect(rootEntry).not.toContain("factory");
    expect(manifest.exports?.["./composition"]).toBeDefined();
    expect(composition).toContain("new NodeModelIo(");
    expect(composition).toContain("createNodeLlamaEngine");
    expect(declarationCheck).toContain("dist/index.d.ts");
    expect(declarationCheck).toContain("filePath");
  });

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

  it("keeps an explicit build-never Windows prebuild smoke", async () => {
    const smoke = await readFile(new URL("scripts/prebuild-smoke.ts", packageRoot), "utf8");

    expect(smoke).toContain('build: "never"');
    expect(smoke).toContain("skipDownload: true");
    expect(smoke).toContain("usePrebuiltBinaries: true");
    expect(smoke).toContain("dryRun: true");
  });
});
