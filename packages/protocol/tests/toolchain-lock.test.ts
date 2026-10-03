import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..", "..");

describe("hermetic protobuf toolchain", () => {
  it("pins Buf and its supported platform artifacts with lockfile integrity", async () => {
    const lockfile = await readFile(resolve(repositoryRoot, "pnpm-lock.yaml"), "utf8");
    const platformPackages = [
      "darwin-arm64",
      "darwin-x64",
      "linux-aarch64",
      "linux-armv7",
      "linux-x64",
      "win32-arm64",
      "win32-x64",
    ];

    expect(lockfile).toMatch(/'@bufbuild\/buf@1\.73\.0':\r?\n\s+resolution: \{integrity: sha512-/u);
    for (const platformPackage of platformPackages) {
      expect(lockfile).toMatch(
        new RegExp(
          `'@bufbuild/buf-${platformPackage}@1\\.73\\.0':\\r?\\n\\s+resolution: \\{integrity: sha512-`,
          "u",
        ),
      );
    }
    expect(lockfile).not.toContain("grpc-tools@");
    expect(lockfile).not.toContain("node-pre-gyp");
  });
});
