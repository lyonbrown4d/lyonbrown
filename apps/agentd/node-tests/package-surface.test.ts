import { access, readFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

interface PackageManifest {
  readonly exports?: unknown;
  readonly main?: unknown;
}

const packageRoot = fileURLToPath(new URL("..", import.meta.url));

function importFromPackage(specifier: string) {
  return spawnSync(
    process.execPath,
    ["--input-type=module", "--eval", `await import(${JSON.stringify(specifier)})`],
    {
      cwd: packageRoot,
      encoding: "utf8",
      shell: false,
    },
  );
}

describe("agentd package surface", () => {
  it("does not expose a package-specifier API that bypasses production composition", async () => {
    const manifest = JSON.parse(
      await readFile(new URL("../package.json", import.meta.url), "utf8"),
    ) as PackageManifest;
    expect(manifest.exports).toEqual({});
    expect(manifest.main).toBeUndefined();
    expect(importFromPackage("@lyonbrown/agentd").status).not.toBe(0);
    expect(importFromPackage("@lyonbrown/agentd/dist/server.js").status).not.toBe(0);
  });

  it("has no source index that aggregates internal server constructors", async () => {
    await expect(access(`${packageRoot}/node-src/index.ts`)).rejects.toBeDefined();
  });
});
