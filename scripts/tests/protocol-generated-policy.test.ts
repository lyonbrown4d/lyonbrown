import { execFileSync, spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, test } from "vitest";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const generatedDirectory = "packages/protocol/src/generated";

describe("protocol generated-source policy", () => {
  test("keeps generated TypeScript ignored and out of Git", () => {
    const trackedFiles = execFileSync("git", ["ls-files", "--", generatedDirectory], {
      cwd: repositoryRoot,
      encoding: "utf8",
    }).trim();
    const ignoredProbe = spawnSync(
      "git",
      ["check-ignore", "--no-index", "--quiet", "--", `${generatedDirectory}/policy-probe.ts`],
      { cwd: repositoryRoot },
    );

    expect(trackedFiles).toBe("");
    expect(ignoredProbe.status).toBe(0);
  });
});
