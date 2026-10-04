import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

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
});
