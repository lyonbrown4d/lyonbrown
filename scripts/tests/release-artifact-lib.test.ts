import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import { resolveReleaseTarget, windowsAgentLauncher } from "../release-artifact-lib.ts";

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
    expect(() => resolveReleaseTarget(workspace, [], "linux", "x64")).toThrow(
      "Unsupported release target",
    );
    expect(() => resolveReleaseTarget(workspace, ["--output"], "win32", "x64")).toThrow("Usage:");
  });

  it("creates a relocatable launcher using the bundled runtime", () => {
    expect(windowsAgentLauncher()).toContain('"%~dp0runtime\\node.exe"');
    expect(windowsAgentLauncher()).toContain('"%~dp0dist\\main.js" %*');
  });
});
