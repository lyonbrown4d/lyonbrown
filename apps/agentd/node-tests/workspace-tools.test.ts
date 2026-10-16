import { mkdtemp, readFile, rename, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { __setFsSafeTestHooksForTest } from "@openclaw/fs-safe/test-hooks";

import { createWorkspaceToolRegistry } from "@lyonbrown/agentd/internal/workspace-tools";
import { MAX_WORKSPACE_FILE_BYTES } from "@lyonbrown/agentd/internal/workspace-paths";

describe("workspace tool registry", () => {
  const temporaryDirectories: string[] = [];

  afterEach(async () => {
    __setFsSafeTestHooksForTest();
    await Promise.all(temporaryDirectories.splice(0).map((path) => rm(path, { recursive: true })));
  });

  async function workspace(): Promise<string> {
    const path = await mkdtemp(join(tmpdir(), "lyonbrown-tools-"));
    temporaryDirectories.push(path);
    return path;
  }

  it("adapts mature read, write, and edit tools with least-privilege metadata", async () => {
    const root = await workspace();
    const registry = await createWorkspaceToolRegistry(root);

    expect(registry.list().map((tool) => [tool.name, tool.risk, tool.permissions])).toEqual([
      ["read", "low", ["workspace.read"]],
      ["write", "medium", ["workspace.write"]],
      ["edit", "medium", ["workspace.write"]],
      ["list_directory", "low", ["workspace.read"]],
      ["search_files", "low", ["workspace.read"]],
      ["process_execute", "high", ["process.execute"]],
    ]);
    expect(registry.hasPermission("workspace.read")).toBe(true);
    expect(registry.hasPermission("workspace.write")).toBe(true);
    expect(registry.hasPermission("process.execute")).toBe(true);
  });

  it("executes workspace file tools but denies traversal outside the workspace", async () => {
    const root = await workspace();
    const registry = await createWorkspaceToolRegistry(root);
    const signal = new AbortController().signal;

    await registry.get("write")?.execute({
      callId: "write-1",
      arguments: { path: "notes.txt", content: "hello" },
      signal,
    });
    await expect(readFile(join(root, "notes.txt"), "utf8")).resolves.toBe("hello");
    await expect(
      registry.get("read")?.execute({
        callId: "read-1",
        arguments: { path: "notes.txt" },
        signal,
      }),
    ).resolves.toMatchObject({ content: "hello" });

    await expect(
      registry.get("write")?.execute({
        callId: "write-escape",
        arguments: { path: "../escape.txt", content: "denied" },
        signal,
      }),
    ).rejects.toThrow();
  });

  it("creates nested parents, edits existing files, and rejects missing or absolute paths", async () => {
    const root = await workspace();
    const registry = await createWorkspaceToolRegistry(root);
    const signal = new AbortController().signal;

    await registry.get("write")?.execute({
      callId: "nested-write",
      arguments: { path: "nested/notes.txt", content: "before" },
      signal,
    });
    await registry.get("edit")?.execute({
      callId: "edit-existing",
      arguments: {
        path: "nested/notes.txt",
        edits: [{ oldText: "before", newText: "after" }],
      },
      signal,
    });
    await expect(readFile(join(root, "nested", "notes.txt"), "utf8")).resolves.toBe("after");

    const outside = join(await workspace(), "outside.txt");
    await writeFile(outside, "outside", "utf8");
    await expect(
      registry.get("read")?.execute({
        callId: "absolute-read",
        arguments: { path: outside },
        signal,
      }),
    ).rejects.toThrow("workspace path denied");
    await expect(
      registry.get("read")?.execute({
        callId: "missing-read",
        arguments: { path: "missing.txt" },
        signal,
      }),
    ).rejects.toThrow();
  });

  it("rejects oversized reads and edits without mutating the file", async () => {
    const root = await workspace();
    const path = join(root, "large.txt");
    const original = "x".repeat(MAX_WORKSPACE_FILE_BYTES + 1);
    await writeFile(path, original, "utf8");
    const registry = await createWorkspaceToolRegistry(root);
    const signal = new AbortController().signal;

    await expect(
      registry.get("read")?.execute({ callId: "large-read", arguments: { path }, signal }),
    ).rejects.toThrow();
    await expect(
      registry.get("edit")?.execute({
        callId: "large-edit",
        arguments: { path, edits: [{ oldText: "x", newText: "y" }] },
        signal,
      }),
    ).rejects.toThrow();
    await expect(readFile(path, "utf8")).resolves.toBe(original);
  });

  it("rejects a directory link that escapes the workspace for reads and writes", async () => {
    const root = await workspace();
    const outside = await workspace();
    await writeFile(join(outside, "outside.txt"), "outside", "utf8");
    await symlink(outside, join(root, "escape"), process.platform === "win32" ? "junction" : "dir");
    const registry = await createWorkspaceToolRegistry(root);
    const signal = new AbortController().signal;

    await expect(
      registry.get("read")?.execute({
        callId: "linked-read",
        arguments: { path: "escape/outside.txt" },
        signal,
      }),
    ).rejects.toThrow();
    await expect(
      registry.get("write")?.execute({
        callId: "linked-write",
        arguments: { path: "escape/created.txt", content: "denied" },
        signal,
      }),
    ).rejects.toThrow();
    await expect(readFile(join(outside, "outside.txt"), "utf8")).resolves.toBe("outside");
    await expect(readFile(join(outside, "created.txt"), "utf8")).rejects.toThrow();
  });

  it("fails closed when a parent is swapped after write-path admission", async () => {
    const root = await workspace();
    const outside = await workspace();
    const admitted = join(root, "admitted");
    await writeFile(join(outside, "sentinel.txt"), "unchanged", "utf8");
    await createWorkspaceToolRegistry(root).then(async (registry) => {
      await registry.get("write")?.execute({
        callId: "seed-parent",
        arguments: { path: "admitted/seed.txt", content: "seed" },
        signal: new AbortController().signal,
      });
    });
    const registry = await createWorkspaceToolRegistry(root);
    let swapped = false;
    __setFsSafeTestHooksForTest({
      async beforePinnedWriteParentAdmission(targetPath) {
        if (swapped || !targetPath.endsWith("raced.txt")) return;
        swapped = true;
        await rename(admitted, join(root, "admitted-original"));
        await symlink(outside, admitted, process.platform === "win32" ? "junction" : "dir");
      },
    });

    await expect(
      registry.get("write")?.execute({
        callId: "raced-write",
        arguments: { path: "admitted/raced.txt", content: "denied" },
        signal: new AbortController().signal,
      }),
    ).rejects.toThrow();
    expect(swapped).toBe(true);
    await expect(readFile(join(outside, "sentinel.txt"), "utf8")).resolves.toBe("unchanged");
    await expect(readFile(join(outside, "raced.txt"), "utf8")).rejects.toThrow();
  });

  it.runIf(process.platform === "win32")(
    "executes tools when the workspace uses the Windows canonical namespace",
    async () => {
      const root = await workspace();
      const canonicalRoot = `\\\\?\\${resolve(root)}`;
      const registry = await createWorkspaceToolRegistry(canonicalRoot);

      await registry.get("write")?.execute({
        callId: "canonical-write",
        arguments: { path: "canonical.txt", content: "canonical" },
        signal: new AbortController().signal,
      });

      await expect(readFile(join(root, "canonical.txt"), "utf8")).resolves.toBe("canonical");
    },
  );
});
