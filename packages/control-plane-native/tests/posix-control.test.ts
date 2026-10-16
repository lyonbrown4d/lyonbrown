import { execFileSync } from "node:child_process";
import {
  chmodSync,
  linkSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  renameSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  PlatformResult,
  SecureDirectoryHandle,
} from "@lyonbrown/local-control-plane/native-adapter";
import { createPosixSecurity } from "@lyonbrown/control-plane-native/internal/posix-provider";
import {
  openDirectory,
  openRegularAt,
} from "@lyonbrown/control-plane-native/internal/posix/directory";
import * as native from "@lyonbrown/control-plane-native/internal/posix/native";
import { NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

const roots: string[] = [];
const handles: SecureDirectoryHandle[] = [];

function root(): string {
  const path = realpathSync(mkdtempSync(join(tmpdir(), "lyonbrown-posix-control-")));
  roots.push(path);
  return path;
}

function unwrap<T>(result: PlatformResult<T>): T {
  if (!result.ok) throw new Error(`unexpected platform failure: ${result.code}`);
  return result.value;
}

async function secure(path: string): Promise<SecureDirectoryHandle> {
  const handle = unwrap(await createPosixSecurity().openSecureRunDirectory(path));
  handles.push(handle);
  return handle;
}

afterEach(async () => {
  vi.restoreAllMocks();
  await Promise.all(handles.splice(0).map((handle) => handle.close()));
  for (const path of roots.splice(0)) rmSync(path, { force: true, recursive: true });
});

describe("POSIX native control plane", () => {
  it("creates private ancestors and atomically replaces bounded descriptors", async () => {
    const path = join(root(), "private", "scope", "run");
    const directory = await secure(path);
    expect(statSync(path).mode & 0o777).toBe(0o700);
    expect(unwrap(await directory.readPrivateFile("agentd.json", 64))).toBeNull();
    unwrap(await directory.atomicReplacePrivateFile("agentd.json", Buffer.from("first")));
    const inode = statSync(join(path, "agentd.json")).ino;
    unwrap(await directory.atomicReplacePrivateFile("agentd.json", Buffer.from("second")));
    expect(statSync(join(path, "agentd.json")).ino).not.toBe(inode);
    expect(statSync(join(path, "agentd.json")).mode & 0o777).toBe(0o600);
    expect(
      Buffer.from(unwrap(await directory.readPrivateFile("agentd.json", 6)) ?? []).toString(),
    ).toBe("second");
    await expect(directory.readPrivateFile("agentd.json", 5)).resolves.toEqual({
      ok: false,
      code: "oversize",
    });
    await expect(directory.readPrivateFile("agentd.json", -1)).resolves.toEqual({
      ok: false,
      code: "io",
    });
    unwrap(await directory.atomicReplacePrivateFile("agentd.json", new Uint8Array()));
    expect(unwrap(await directory.readPrivateFile("agentd.json", 0))).toEqual(Buffer.alloc(0));
    expect(readdirSync(path)).toEqual(["agentd.json"]);
    unwrap(await directory.close());
    unwrap(await directory.close());
    await expect(directory.readPrivateFile("agentd.json", 10)).resolves.toEqual({
      ok: false,
      code: "io",
    });
  });

  it("rejects symlink components and leaf files without touching their targets", async () => {
    const base = root();
    const target = join(base, "target");
    mkdirSync(target, { mode: 0o700 });
    symlinkSync(target, join(base, "alias"));
    const result = await createPosixSecurity().openSecureRunDirectory(join(base, "alias", "run"));
    expect(result.ok).toBe(false);
    expect(readdirSync(target)).toEqual([]);
    const directory = await secure(join(base, "run"));
    const outside = join(base, "outside");
    writeFileSync(outside, "preserved", { mode: 0o600 });
    symlinkSync(outside, join(base, "run", "agentd.json"));
    await expect(directory.readPrivateFile("agentd.json", 100)).resolves.toMatchObject({
      ok: false,
    });
    await expect(
      directory.atomicReplacePrivateFile("agentd.json", Buffer.from("bad")),
    ).resolves.toMatchObject({ ok: false });
    expect(readFileSync(outside, "utf8")).toBe("preserved");
  });

  it("rejects broad permissions, directory files, hardlinks, and a different owner", async () => {
    const base = root();
    const broad = join(base, "broad");
    mkdirSync(broad, { mode: 0o755 });
    await expect(createPosixSecurity().openSecureRunDirectory(broad)).resolves.toEqual({
      ok: false,
      code: "insecure_mode",
    });
    const path = join(base, "run");
    const directory = await secure(path);
    const file = join(path, "agentd.json");
    writeFileSync(file, "private", { mode: 0o644 });
    await expect(directory.readPrivateFile("agentd.json", 100)).resolves.toEqual({
      ok: false,
      code: "insecure_mode",
    });
    chmodSync(file, 0o600);
    linkSync(file, join(base, "hardlink"));
    await expect(directory.readPrivateFile("agentd.json", 100)).resolves.toEqual({
      ok: false,
      code: "wrong_type",
    });
    rmSync(file);
    mkdirSync(file, { mode: 0o700 });
    await expect(directory.readPrivateFile("agentd.json", 100)).resolves.toEqual({
      ok: false,
      code: "wrong_type",
    });
    const uid = process.getuid?.();
    if (uid === undefined) throw new Error("POSIX uid required");
    vi.spyOn(process, "getuid").mockReturnValue(uid + 1);
    await expect(createPosixSecurity().openSecureRunDirectory(path)).resolves.toEqual({
      ok: false,
      code: "wrong_owner",
    });
  });

  it("detects swapped ancestors and retains the opened directory without writing replacements", async () => {
    const base = root();
    const ancestor = join(base, "parent");
    const directory = await secure(join(ancestor, "run"));
    renameSync(ancestor, join(base, "moved"));
    mkdirSync(join(ancestor, "run"), { recursive: true, mode: 0o700 });
    await expect(
      directory.atomicReplacePrivateFile("agentd.json", Buffer.from("bad")),
    ).resolves.toEqual({ ok: false, code: "io" });
    expect(readdirSync(join(ancestor, "run"))).toEqual([]);
    expect(readdirSync(join(base, "moved", "run"))).toEqual([]);
  });

  it("cleans a failed temporary publication and preserves the previous descriptor", async () => {
    const path = join(root(), "run");
    const directory = await secure(path);
    unwrap(await directory.atomicReplacePrivateFile("agentd.json", Buffer.from("preserved")));
    vi.spyOn(native, "fsyncFd").mockImplementationOnce(() => {
      throw new NativeFailure("io");
    });
    await expect(
      directory.atomicReplacePrivateFile("agentd.json", Buffer.from("failed")),
    ).resolves.toEqual({ ok: false, code: "io" });
    expect(readdirSync(path)).toEqual(["agentd.json"]);
    expect(readFileSync(join(path, "agentd.json"), "utf8")).toBe("preserved");
  });

  it("canonicalizes workspace aliases using the native device and inode identity", async () => {
    const base = root();
    const path = join(base, "workspace");
    mkdirSync(path);
    symlinkSync(path, join(base, "alias"));
    const security = createPosixSecurity();
    const first = unwrap(await security.workspaceIdentity(path));
    expect(unwrap(await security.workspaceIdentity(join(base, "alias")))).toEqual(first);
    const stat = statSync(path, { bigint: true });
    expect(first).toEqual({
      canonicalPath: path,
      identity: { kind: "inode", deviceId: stat.dev, inodeNumber: stat.ino },
    });
    await expect(security.workspaceIdentity(join(base, "missing"))).resolves.toEqual({
      ok: false,
      code: "io",
    });
  });

  it("rejects invalid native paths and relative child traversal", () => {
    for (const path of ["relative/run", "/private/../run", "/private/./run", "/private/\0run"]) {
      expect(() => openDirectory(path, true, true)).toThrow(NativeFailure);
    }
    const directory = openDirectory(root(), false, true);
    try {
      for (const name of ["", ".", "..", "../outside", "nested/file", "nul\0file"]) {
        expect(() => openRegularAt(directory, name, true)).toThrow(NativeFailure);
      }
    } finally {
      directory.close();
    }
    directory.close();
    expect(() => directory.fd).toThrow(NativeFailure);
  });

  it.runIf(process.platform === "darwin")(
    "rejects Darwin ACL grants even with owner-only modes",
    async () => {
      const path = join(root(), "run");
      const directory = await secure(path);
      unwrap(await directory.atomicReplacePrivateFile("agentd.json", Buffer.from("private")));
      execFileSync("/bin/chmod", ["+a", "everyone allow read", join(path, "agentd.json")]);
      await expect(directory.readPrivateFile("agentd.json", 100)).resolves.toEqual({
        ok: false,
        code: "insecure_acl",
      });
      execFileSync("/bin/chmod", ["+a", "everyone allow read,search", path]);
      await expect(createPosixSecurity().openSecureRunDirectory(path)).resolves.toEqual({
        ok: false,
        code: "insecure_acl",
      });
    },
  );
});
