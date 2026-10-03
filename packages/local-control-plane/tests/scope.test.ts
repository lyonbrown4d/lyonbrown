import { mkdir, realpath, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { deriveScope, normalizeCanonicalWorkspacePath } from "../src/index.js";
import type { ControlPlaneError } from "../src/index.js";
import { withTemporaryDirectory } from "./test-support.js";

describe("deriveScope", () => {
  it("derives a stable scope from the canonical workspace and effective config", async () => {
    await withTemporaryDirectory(async (root) => {
      const workspace = join(root, "workspace");
      await mkdir(workspace);

      const first = await deriveScope(workspace, "runtime-config-v1");
      const second = await deriveScope(await realpath(workspace), "runtime-config-v1");

      expect(first).toEqual(second);
      expect(first.scopeId).toMatch(/^[a-f0-9]{64}$/u);
      expect(Object.isFrozen(first)).toBe(true);
    });
  });

  it.each([
    ["relative/path", "WORKSPACE_NOT_ABSOLUTE"],
    [resolve("missing-workspace-for-control-plane-tests"), "WORKSPACE_NOT_FOUND"],
    [`${resolve("workspace")}\0suffix`, "INVALID_WORKSPACE_PATH"],
    [`${resolve("workspace")}${String.fromCharCode(0xd800)}`, "INVALID_WORKSPACE_PATH"],
  ] as const)("rejects an invalid workspace path", async (workspace, code) => {
    await expect(deriveScope(workspace, "runtime-config-v1")).rejects.toMatchObject({
      name: "ControlPlaneError",
      code,
    } satisfies Partial<ControlPlaneError>);
  });

  it("rejects an empty or malformed effective config fingerprint", async () => {
    await expect(deriveScope(resolve("."), " \t ")).rejects.toMatchObject({
      code: "INVALID_CONFIG_FINGERPRINT",
    });
    await expect(
      deriveScope(resolve("."), `bad${String.fromCharCode(0xd800)}`),
    ).rejects.toMatchObject({ code: "INVALID_CONFIG_FINGERPRINT" });
    await expect(
      deriveScope(resolve("."), `bad${String.fromCharCode(0xdc00)}`),
    ).rejects.toMatchObject({ code: "INVALID_CONFIG_FINGERPRINT" });
    await expect(deriveScope(resolve("."), "bad\0fingerprint")).rejects.toMatchObject({
      code: "INVALID_CONFIG_FINGERPRINT",
    });
  });

  it("rejects a workspace path that is not a directory", async () => {
    await withTemporaryDirectory(async (root) => {
      const file = join(root, "workspace.txt");
      await writeFile(file, "not a directory");

      await expect(deriveScope(file, "runtime-config-v1")).rejects.toMatchObject({
        code: "WORKSPACE_NOT_DIRECTORY",
      });
    });
  });

  it("frames configuration fingerprints and accepts well-formed surrogate pairs", async () => {
    const workspace = resolve(".");
    const first = await deriveScope(workspace, "runtime😀");
    const second = await deriveScope(workspace, "runtime😁");

    expect(first.scopeId).not.toBe(second.scopeId);
  });
});

describe("normalizeCanonicalWorkspacePath", () => {
  it("normalizes Windows case and separators without changing POSIX case", () => {
    expect(normalizeCanonicalWorkspacePath("C:\\Users\\Alice\\Repo", "win32")).toBe(
      "c:/users/alice/repo",
    );
    expect(normalizeCanonicalWorkspacePath("c:/USERS/alice/repo", "win32")).toBe(
      "c:/users/alice/repo",
    );
    expect(normalizeCanonicalWorkspacePath("/Work/Repo", "linux")).toBe("/Work/Repo");
  });
});
