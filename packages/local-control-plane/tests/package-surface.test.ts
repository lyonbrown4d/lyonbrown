import { execFile } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { describe, expect, it } from "vitest";
import * as publicApi from "../src/index.js";
import * as nativeAdapterApi from "../src/native-adapter.js";

const execute = promisify(execFile);
const packageRoot = resolve(fileURLToPath(new URL("..", import.meta.url)));
const packageJsonPath = resolve(packageRoot, "package.json");

function executePnpm(arguments_: readonly string[]) {
  if (process.platform === "win32") {
    return execute(process.env.ComSpec ?? "cmd.exe", ["/d", "/s", "/c", "pnpm", ...arguments_], {
      cwd: packageRoot,
    });
  }
  return execute("pnpm", [...arguments_], { cwd: packageRoot });
}

interface PackManifest {
  readonly files: readonly { readonly path: string }[];
}

describe("package surface", () => {
  it("keeps capability internals out of the root runtime entry", () => {
    expect(Object.keys(publicApi)).not.toEqual(
      expect.arrayContaining([
        "createNativeControlPlaneProvider",
        "deriveScopeFromIdentity",
        "encodeWorkspaceIdentity",
        "invokePlatform",
        "resolveNativeControlPlaneProvider",
        "resolveRunDirectoryHandle",
        "unwrapPlatformResult",
      ]),
    );
    expect(Object.keys(nativeAdapterApi)).toEqual(["createNativeControlPlaneProvider"]);
  });

  it("cleans stale output before build and packs only the explicit manifest", async () => {
    const stalePath = resolve(packageRoot, "dist", "security.js");
    await mkdir(resolve(packageRoot, "dist"), { recursive: true });
    await writeFile(stalePath, "export const stale = true;\n", "utf8");

    await executePnpm(["build"]);
    const { stdout } = await executePnpm(["pack", "--dry-run", "--json"]);
    const manifest = JSON.parse(stdout) as PackManifest;
    const paths = manifest.files.map(({ path }) => path);

    expect(paths).toContain("dist/index.d.ts");
    expect(paths).toContain("dist/index.js");
    expect(paths).toContain("dist/native-adapter.d.ts");
    expect(paths).toContain("dist/native-adapter.js");
    expect(paths).not.toContain("dist/security.js");
    expect(
      paths.every((path) => /^(?:dist\/.+\.(?:d\.ts|js)|README\.md|package\.json)$/u.test(path)),
    ).toBe(true);

    const declaration = await readFile(resolve(packageRoot, "dist", "index.d.ts"), "utf8");
    expect(declaration).not.toMatch(
      /SecureDirectoryHandle|invokePlatform|unwrapPlatformResult|encodeWorkspaceIdentity|deriveScopeFromIdentity/u,
    );

    const packageJson = JSON.parse(await readFile(packageJsonPath, "utf8")) as {
      readonly exports: Readonly<Record<string, unknown>>;
      readonly files: readonly string[];
      readonly scripts: Readonly<Record<string, string>>;
    };
    expect(Object.keys(packageJson.exports)).toEqual([".", "./native-adapter"]);
    expect(packageJson.files).toEqual(["dist/**/*.js", "dist/**/*.d.ts", "README.md"]);
    expect(packageJson.scripts.build).toBe("pnpm clean && tsc -p tsconfig.build.json");
  }, 20_000);
});
