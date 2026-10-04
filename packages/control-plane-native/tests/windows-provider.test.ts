import { lstat, mkdir, mkdtemp, rename, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  ControlPlaneError,
  SecretToken,
  acquireLifecycleLock,
  createDiscoveryDescriptor,
  deriveScope,
  openSecureRunDirectory,
} from "@lyonbrown/local-control-plane";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createControlPlaneNativeProvider } from "../src/index.js";
import { api } from "../src/windows/api.js";

const windows = process.platform === "win32";
const roots: string[] = [];

async function temporaryRoot(): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), "lyonbrown-native-"));
  roots.push(root);
  return root;
}

afterEach(async () => {
  await Promise.all(roots.splice(0).map((path) => rm(path, { force: true, recursive: true })));
});

describe.runIf(windows)("Windows native control plane", () => {
  it("creates a protected directory and atomically publishes a bounded descriptor", async () => {
    const root = await temporaryRoot();
    const path = join(root, "run");
    const directory = await openSecureRunDirectory(path, createControlPlaneNativeProvider());
    const descriptor = createDiscoveryDescriptor({
      endpoint: "127.0.0.1:43123",
      pid: process.pid,
      scopeId: "a".repeat(64),
      token: SecretToken.generate(),
    });

    await directory.discovery.publish(descriptor);
    await expect(directory.discovery.read()).resolves.toEqual(descriptor);
    expect((await lstat(join(path, "agentd.json"))).isFile()).toBe(true);
    await directory.close();
    await directory.close();
    await expect(directory.discovery.read()).rejects.toBeInstanceOf(ControlPlaneError);
  });

  it("rejects junctions and existing directories with inherited broad ACLs", async () => {
    const root = await temporaryRoot();
    const target = join(root, "target");
    const junction = join(root, "junction");
    await mkdir(target);
    await symlink(target, junction, "junction");
    const provider = createControlPlaneNativeProvider();

    await expect(openSecureRunDirectory(junction, provider)).rejects.toBeInstanceOf(
      ControlPlaneError,
    );
    await expect(openSecureRunDirectory(target, provider)).rejects.toBeInstanceOf(
      ControlPlaneError,
    );
  });

  it("creates every missing protected component and rejects an intermediate junction", async () => {
    const root = await temporaryRoot();
    const nested = join(root, "protected", "scope", "run");
    const directory = await openSecureRunDirectory(nested, createControlPlaneNativeProvider());
    await directory.close();
    await expect(lstat(nested)).resolves.toBeDefined();

    const outside = join(root, "outside");
    const junction = join(root, "escape");
    await mkdir(outside);
    await symlink(outside, junction, "junction");
    await expect(
      openSecureRunDirectory(join(junction, "run"), createControlPlaneNativeProvider()),
    ).rejects.toBeInstanceOf(ControlPlaneError);
    await expect(lstat(join(outside, "run"))).rejects.toBeDefined();
  });

  it("anchors the directory against rename or deletion while open", async () => {
    const root = await temporaryRoot();
    const path = join(root, "run");
    const directory = await openSecureRunDirectory(path, createControlPlaneNativeProvider());

    await expect(rm(path, { recursive: true })).rejects.toBeDefined();
    await directory.close();
    await expect(rm(path, { recursive: true })).resolves.toBeUndefined();
  });

  it("prevents a parent path swap while the child directory is anchored", async () => {
    const root = await temporaryRoot();
    const parent = join(root, "parent");
    const path = join(parent, "run");
    const directory = await openSecureRunDirectory(path, createControlPlaneNativeProvider());
    await expect(rename(parent, join(root, "moved"))).rejects.toBeDefined();
    await directory.close();
  });

  it("prevents an ancestor path swap while the descendant directory is open", async () => {
    const root = await temporaryRoot();
    const ancestor = join(root, "ancestor");
    const path = join(ancestor, "parent", "run");
    const directory = await openSecureRunDirectory(path, createControlPlaneNativeProvider());
    await expect(rename(ancestor, join(root, "moved"))).rejects.toBeDefined();
    await directory.close();
  });

  it("attempts every anchored handle close and returns one stable failure", async () => {
    const root = await temporaryRoot();
    const path = join(root, "ancestor", "parent", "run");
    const directory = await openSecureRunDirectory(path, createControlPlaneNativeProvider());
    const nativeClose = api.CloseHandle;
    let calls = 0;
    const close = vi.spyOn(api, "CloseHandle").mockImplementation((...arguments_: unknown[]) => {
      calls += 1;
      const result = nativeClose(...arguments_);
      return calls === 1 ? 0 : result;
    });

    await expect(directory.close()).rejects.toMatchObject({
      code: "PLATFORM_SECURITY_FAILURE",
      details: { operation: "close_directory" },
    });
    const firstCloseCalls = calls;
    expect(firstCloseCalls).toBeGreaterThan(2);
    await expect(directory.close()).rejects.toMatchObject({
      code: "PLATFORM_SECURITY_FAILURE",
      details: { operation: "close_directory" },
    });
    expect(calls).toBe(firstCloseCalls);
    close.mockRestore();
  });

  it("uses exclusive kernel locks and makes release idempotent", async () => {
    const root = await temporaryRoot();
    const path = join(root, "run");
    const provider = createControlPlaneNativeProvider();
    const firstDirectory = await openSecureRunDirectory(path, provider);
    const secondDirectory = await openSecureRunDirectory(path, provider);
    const first = await acquireLifecycleLock(firstDirectory);
    expect(first).not.toBeNull();
    await expect(acquireLifecycleLock(secondDirectory)).resolves.toBeNull();

    await first?.release();
    const second = await acquireLifecycleLock(secondDirectory);
    expect(second).not.toBeNull();
    await first?.release();
    await expect(acquireLifecycleLock(firstDirectory)).resolves.toBeNull();
    await second?.release();
    await firstDirectory.close();
    await secondDirectory.close();
  });

  it("prevents lock-file rename, deletion, and ABA replacement while leased", async () => {
    const root = await temporaryRoot();
    const path = join(root, "run");
    const lockPath = join(path, "agentd.lock");
    const movedPath = join(path, "agentd.moved.lock");
    const provider = createControlPlaneNativeProvider();
    const firstDirectory = await openSecureRunDirectory(path, provider);
    const secondDirectory = await openSecureRunDirectory(path, provider);
    const first = await acquireLifecycleLock(firstDirectory);
    let second: Awaited<ReturnType<typeof acquireLifecycleLock>> = null;
    let renameFailed = false;
    let deleteFailed = false;
    let recreateFailed = false;
    try {
      try {
        await rename(lockPath, movedPath);
      } catch {
        renameFailed = true;
      }
      try {
        await rm(lockPath);
      } catch {
        deleteFailed = true;
      }
      try {
        await writeFile(lockPath, "replacement", { flag: "wx" });
      } catch {
        recreateFailed = true;
      }
      second = await acquireLifecycleLock(secondDirectory);
      expect(renameFailed).toBe(true);
      expect(deleteFailed).toBe(true);
      expect(recreateFailed).toBe(true);
      expect(second).toBeNull();
    } finally {
      await second?.release();
      await first?.release();
      await firstDirectory.close();
      await secondDirectory.close();
    }

    const afterReleaseDirectory = await openSecureRunDirectory(path, provider);
    const afterRelease = await acquireLifecycleLock(afterReleaseDirectory);
    expect(afterRelease).not.toBeNull();
    await afterRelease?.release();
    await afterReleaseDirectory.close();
  });

  it("rejects an insecure descriptor swapped into a protected directory", async () => {
    const root = await temporaryRoot();
    const path = join(root, "run");
    const directory = await openSecureRunDirectory(path, createControlPlaneNativeProvider());
    await writeFile(join(path, "agentd.json"), "{}", "utf8");
    await expect(directory.discovery.read()).rejects.toBeInstanceOf(ControlPlaneError);
    await directory.close();
  });

  it("enforces the descriptor byte bound before allocating the read", async () => {
    const root = await temporaryRoot();
    const path = join(root, "run");
    const directory = await openSecureRunDirectory(path, createControlPlaneNativeProvider());
    const descriptor = createDiscoveryDescriptor({
      endpoint: "127.0.0.1:43123",
      pid: process.pid,
      scopeId: "c".repeat(64),
      token: SecretToken.generate(),
    });
    await directory.discovery.publish(descriptor);

    const handle = await import("../src/windows/directory.js");
    const direct = handle.openSecureDirectory(path);
    await expect(direct.readPrivateFile("agentd.json", 1)).resolves.toEqual({
      ok: false,
      code: "oversize",
    });
    await direct.close();
    await directory.close();
  });

  it("returns not-found for an absent private file and supports zero-byte I/O", async () => {
    const root = await temporaryRoot();
    const path = join(root, "run");
    const { openSecureDirectory } = await import("../src/windows/directory.js");
    const handle = openSecureDirectory(path);
    await expect(handle.readPrivateFile("agentd.json", 64)).resolves.toEqual({
      ok: true,
      value: null,
    });
    await expect(handle.atomicReplacePrivateFile("agentd.json", new Uint8Array())).resolves.toEqual(
      { ok: true, value: undefined },
    );
    const emptyRead = await handle.readPrivateFile("agentd.json", 0);
    await handle.close();
    expect(emptyRead).toEqual({
      ok: true,
      value: Buffer.alloc(0),
    });
  });

  it("matches a stable Rust workspace identity for aliases of one directory", async () => {
    const root = await temporaryRoot();
    const workspace = join(root, "workspace");
    await mkdir(workspace);
    const provider = createControlPlaneNativeProvider();
    const alternateCase = workspace.toUpperCase();
    expect(alternateCase).not.toBe(workspace);
    const first = await deriveScope(workspace, "fixture", provider, "win32");
    const second = await deriveScope(alternateCase, "fixture", provider, "win32");
    expect(first.scopeId).toBe(second.scopeId);
    expect(first.canonicalWorkspace).toMatch(/^\\\\\?\\/u);
  });

  it("follows a final junction for Rust-compatible workspace identity", async () => {
    const root = await temporaryRoot();
    const workspace = join(root, "workspace");
    const alias = join(root, "workspace-alias");
    await mkdir(workspace);
    await symlink(workspace, alias, "junction");
    const provider = createControlPlaneNativeProvider();

    const target = await deriveScope(workspace, "fixture", provider, "win32");
    const throughAlias = await deriveScope(alias, "fixture", provider, "win32");
    expect(throughAlias).toEqual(target);
  });
});
