import { resolve } from "node:path";
import { execFile } from "node:child_process";
import { mkdtemp, mkdir, rename, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { promisify } from "node:util";

import { describe, expect, it } from "vitest";

import {
  resolveReleaseTarget,
  windowsAgentLauncher,
  posixAgentLauncher,
  releaseLauncherName,
  releaseRuntimeName,
  resolveArtifactSmokeDirectory,
} from "@lyonbrown/workspace/internal/release-artifact-lib";

describe("release artifact contract", () => {
  const workspace = resolve("C:/workspace/lyonbrown");

  it("uses a platform and architecture specific artifact directory", () => {
    expect(resolveReleaseTarget(workspace, [], "win32", "x64")).toEqual({
      outputDirectory: resolve(workspace, "artifacts/lyonbrown-win32-x64"),
      platform: "win32",
      architecture: "x64",
    });
  });

  it("accepts a named child of artifacts", () => {
    expect(
      resolveReleaseTarget(
        workspace,
        ["--output", "artifacts/lyonbrown-win32-x64-candidate"],
        "win32",
        "x64",
      ).outputDirectory,
    ).toBe(resolve(workspace, "artifacts/lyonbrown-win32-x64-candidate"));
  });

  it("accepts the documented pnpm argument separator for package and smoke commands", () => {
    expect(
      resolveReleaseTarget(workspace, ["--", "--output", "artifacts/candidate"], "darwin", "arm64")
        .outputDirectory,
    ).toBe(resolve(workspace, "artifacts/candidate"));
    expect(resolveArtifactSmokeDirectory(["--", "--artifact", "artifacts/candidate"])).toBe(
      resolve("artifacts/candidate"),
    );
    expect(() => resolveArtifactSmokeDirectory(["--", "--artifact"])).toThrow("Usage:");
  });

  it.each([
    ["--output", "elsewhere"],
    ["--output", "artifacts"],
    ["--output", "artifacts/../elsewhere"],
  ])("rejects output outside the artifact boundary: %s %s", (...arguments_) => {
    expect(() => resolveReleaseTarget(workspace, arguments_, "win32", "x64")).toThrow(
      /artifacts directory/u,
    );
  });

  it("rejects unsupported release targets and malformed arguments", () => {
    expect(() => resolveReleaseTarget(workspace, [], "freebsd", "x64")).toThrow(
      "Unsupported release target",
    );
    expect(() => resolveReleaseTarget(workspace, ["--output"], "win32", "x64")).toThrow("Usage:");
  });

  it.each(["win32", "darwin", "linux"] as const)(
    "supports %s without cross-compiling the host runtime",
    (platform) => {
      for (const architecture of ["x64", "arm64"] as const) {
        expect(resolveReleaseTarget(workspace, [], platform, architecture)).toMatchObject({
          platform,
          architecture,
          outputDirectory: resolve(workspace, `artifacts/lyonbrown-${platform}-${architecture}`),
        });
      }
      expect(releaseRuntimeName(platform)).toBe(platform === "win32" ? "node.exe" : "node");
      expect(releaseLauncherName(platform)).toBe(platform === "win32" ? "agent.cmd" : "agent");
    },
  );

  it("creates a relocatable launcher using the bundled runtime", () => {
    expect(windowsAgentLauncher()).toContain('"%~dp0runtime\\node.exe"');
    expect(windowsAgentLauncher()).toContain('"%~dp0dist\\main.js" %*');
  });

  it("rejects standalone artifact inspection on unsupported platforms", () => {
    expect(() => releaseRuntimeName("freebsd")).toThrow("Unsupported release platform");
    expect(() => releaseLauncherName("freebsd")).toThrow("Unsupported release platform");
  });

  it.runIf(process.platform !== "win32")(
    "runs the relocated POSIX launcher with literal argv",
    async () => {
      const scratch = await mkdtemp(resolve(tmpdir(), "lyonbrown launcher "));
      try {
        const original = resolve(scratch, "before move");
        const relocated = resolve(scratch, "after move 中文");
        await mkdir(resolve(original, "runtime"), { recursive: true });
        await mkdir(resolve(original, "dist"));
        await symlink(process.execPath, resolve(original, "runtime/node"));
        await writeFile(
          resolve(original, "dist/main.js"),
          "process.stdout.write(JSON.stringify(process.argv.slice(2)));\n",
        );
        await writeFile(resolve(original, "agent"), posixAgentLauncher(), { mode: 0o755 });
        await rename(original, relocated);
        const arguments_ = ["space value", "中文", "$(echo unexpected)", "a'b", "*"];
        const { stdout } = await promisify(execFile)(resolve(relocated, "agent"), arguments_, {
          timeout: 10_000,
        });
        expect(JSON.parse(stdout)).toEqual(arguments_);
      } finally {
        await rm(scratch, { recursive: true, force: true });
      }
    },
  );
});
