import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspaceRoot = resolve(packageRoot, "../..");

function installedKoffiManifest(): string {
  return resolve(dirname(require.resolve("koffi")), "package.json");
}

describe("native package policy", () => {
  it("pins and loads the supported prebuilt Koffi runtime", async () => {
    const manifest = JSON.parse(await readFile(resolve(packageRoot, "package.json"), "utf8")) as {
      dependencies: Record<string, string>;
    };
    const koffiManifest = JSON.parse(await readFile(installedKoffiManifest(), "utf8")) as {
      optionalDependencies: Record<string, string>;
      scripts?: Record<string, string>;
      version: string;
    };
    const payload = `@koromix/koffi-${process.platform}-${process.arch}`;

    expect(manifest.dependencies.koffi).toBe("3.3.2");
    expect(koffiManifest.version).toBe("3.3.2");
    expect(koffiManifest.scripts?.install).toContain("--prebuild");
    expect(koffiManifest.optionalDependencies[payload]).toBe("3.3.2");
    expect(require.resolve(payload)).toBeTruthy();
    expect(() => {
      require("koffi");
    }).not.toThrow();
  });

  it("contains no source-build or path-fallback dependency", async () => {
    const manifest = await readFile(resolve(packageRoot, "package.json"), "utf8");
    const files = await readFile(resolve(packageRoot, "src/windows/files.ts"), "utf8");
    expect(manifest).not.toContain("proper-lockfile");
    expect(manifest).not.toContain("node-gyp");
    expect(manifest).not.toContain("prebuild-install");
    expect(files).toContain("api.NtSetInformationFile");
    expect(files).not.toContain("MoveFile");
    expect(files).not.toContain("rename(");
  });

  it("explicitly blocks Koffi install scripts at the workspace boundary", async () => {
    const workspace = await readFile(resolve(workspaceRoot, "pnpm-workspace.yaml"), "utf8");
    const ignoredSection = /ignoredBuiltDependencies:\s*\n((?: {2}- .+\n?)*)/u.exec(workspace)?.[1];
    expect(ignoredSection).toContain("  - koffi");
  });

  it("collects only portable tests and sources away from Windows", async () => {
    const config = (await import("@lyonbrown/control-plane-native/config/vitest")) as {
      testPlan?: (platform: NodeJS.Platform) => {
        coverageInclude: string[];
        include: string[];
      };
    };
    const plan = config.testPlan?.("linux");
    expect(plan).toEqual({
      coverageInclude: [
        "src/index.ts",
        "src/platform.ts",
        "src/posix-provider.ts",
        "src/result.ts",
      ],
      include: [
        "tests/package-policy.test.ts",
        "tests/portable-contracts.test.ts",
        "tests/provider.test.ts",
      ],
    });
    for (const file of plan?.include ?? []) {
      expect(await readFile(resolve(packageRoot, file), "utf8")).not.toMatch(
        /(?:from|import\()\s*["'][^"']*\/windows\//u,
      );
    }
  });
});
