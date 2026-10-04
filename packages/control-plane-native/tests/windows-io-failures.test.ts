import { mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "../src/windows/api.js";
import { atomicReplace, readPrivateFile, tryLock } from "../src/windows/files.js";
import { openDirectory, type OwnedHandle } from "../src/windows/handles.js";
import { canonicalPath, workspaceIdentity } from "../src/windows/identity.js";
import { openSecureDirectory } from "../src/windows/directory.js";

interface Context {
  readonly root: string;
  readonly path: string;
  readonly handle: OwnedHandle;
  readonly canonical: string;
  close(): Promise<void>;
}

const openContexts: Context[] = [];

async function context(): Promise<Context> {
  const root = await mkdtemp(join(tmpdir(), "lyonbrown-native-failure-"));
  const path = join(root, "run");
  const directory = openSecureDirectory(path);
  const handle = openDirectory(path, true);
  const value: Context = {
    root,
    path,
    handle,
    canonical: canonicalPath(handle),
    async close() {
      handle.close();
      await directory.close();
    },
  };
  openContexts.push(value);
  return value;
}

afterEach(async () => {
  vi.restoreAllMocks();
  for (const item of openContexts.splice(0)) {
    await item.close();
    await rm(item.root, { force: true, recursive: true });
  }
});

describe.runIf(process.platform === "win32")("Windows native I/O failure cleanup", () => {
  it("rejects failed and short reads", async () => {
    const value = await context();
    atomicReplace(value.handle, value.canonical, "agentd.json", Buffer.from("payload"));
    const read = vi.spyOn(api, "ReadFile").mockImplementation(() => 0);
    expect(() => readPrivateFile(`${value.canonical}\\agentd.json`, 64)).toThrow();
    read.mockImplementation((...arguments_: unknown[]) => {
      (arguments_[3] as unknown[])[0] = 1;
      return 1;
    });
    expect(() => readPrivateFile(`${value.canonical}\\agentd.json`, 64)).toThrow();
  });

  it("closes and removes the temporary after write, flush, or rename failure", async () => {
    const value = await context();
    const write = vi.spyOn(api, "WriteFile").mockImplementation(() => 0);
    expect(() => {
      atomicReplace(value.handle, value.canonical, "agentd.json", Buffer.from("x"));
    }).toThrow();
    write.mockRestore();

    const flush = vi.spyOn(api, "FlushFileBuffers").mockImplementation(() => 0);
    expect(() => {
      atomicReplace(value.handle, value.canonical, "agentd.json", Buffer.from("x"));
    }).toThrow();
    flush.mockRestore();

    const rename = vi.spyOn(api, "NtSetInformationFile").mockImplementation(() => 0xc000_0022 | 0);
    expect(() => {
      atomicReplace(value.handle, value.canonical, "agentd.json", Buffer.from("x"));
    }).toThrow(expect.objectContaining({ failureCode: "access_denied" }));
    rename.mockRestore();
    expect(await readdir(value.path)).toEqual([]);
  });

  it("reports disposition cleanup failure and still closes the temporary handle", async () => {
    const value = await context();
    const nativeCreate = api.CreateFileW;
    const nativeClose = api.CloseHandle;
    let temporaryHandle: unknown;
    vi.spyOn(api, "CreateFileW").mockImplementation((...arguments_: unknown[]) => {
      const handle = nativeCreate(...arguments_);
      if (String(arguments_[0]).includes(".agentd-")) temporaryHandle = handle;
      return handle;
    });
    const close = vi
      .spyOn(api, "CloseHandle")
      .mockImplementation((...arguments_: unknown[]) => nativeClose(...arguments_));
    vi.spyOn(api, "NtSetInformationFile").mockImplementation(() => 0xc000_0001 | 0);
    vi.spyOn(api, "SetFileInformationByHandle").mockImplementation(() => {
      vi.spyOn(api, "GetLastError").mockImplementation(() => 5);
      return 0;
    });

    expect(() => {
      atomicReplace(value.handle, value.canonical, "agentd.json", Buffer.from("x"));
    }).toThrow(expect.objectContaining({ failureCode: "access_denied" }));
    expect(temporaryHandle).toBeDefined();
    expect(close).toHaveBeenCalledWith(temporaryHandle);
  });

  it("closes the lock handle for non-contention and unlock failures", async () => {
    const value = await context();
    const lockFile = `${value.canonical}\\agentd.lock`;
    const lock = vi.spyOn(api, "LockFileEx").mockImplementation(() => 0);
    vi.spyOn(api, "GetLastError").mockImplementation(() => 87);
    expect(() => tryLock(lockFile)).toThrow();
    lock.mockRestore();
    vi.restoreAllMocks();

    const acquired = tryLock(lockFile);
    expect(acquired).not.toBeNull();
    vi.spyOn(api, "UnlockFileEx").mockImplementation(() => 0);
    vi.spyOn(api, "GetLastError").mockImplementation(() => 5);
    await expect(acquired?.release()).resolves.toEqual({ ok: false, code: "access_denied" });
    await expect(acquired?.release()).resolves.toEqual({ ok: false, code: "access_denied" });
    vi.restoreAllMocks();
    const replacement = tryLock(lockFile);
    expect(replacement).not.toBeNull();
    await replacement?.release();
  });

  it("closes workspace handles when identity or canonical lookup fails", async () => {
    const value = await context();
    let calls = 0;
    const information = vi
      .spyOn(api, "GetFileInformationByHandleEx")
      .mockImplementation((...arguments_: unknown[]) => {
        calls += 1;
        if (calls === 1) {
          (arguments_[2] as Buffer).writeUInt32LE(0x10, 0);
          return 1;
        }
        return 0;
      });
    const lowResolution = vi.spyOn(api, "GetFileInformationByHandle").mockImplementation(() => 0);
    expect(() => workspaceIdentity(value.path)).toThrow();
    information.mockRestore();
    lowResolution.mockRestore();

    const canonical = vi.spyOn(api, "GetFinalPathNameByHandleW").mockImplementation(() => 0);
    expect(() => workspaceIdentity(value.path)).toThrow();
    canonical.mockRestore();
  });

  it("matches Rust low-resolution identity when FileIdInfo is unavailable", async () => {
    const value = await context();
    const nativeInformation = api.GetFileInformationByHandleEx;
    vi.spyOn(api, "GetFileInformationByHandleEx").mockImplementation((...arguments_: unknown[]) =>
      Number(arguments_[1]) === 18 ? 0 : nativeInformation(...arguments_),
    );
    const mutableApi = api as unknown as Record<string, unknown>;
    const previous = mutableApi.GetFileInformationByHandle;
    mutableApi.GetFileInformationByHandle = vi.fn((...arguments_: unknown[]) => {
      const output = arguments_[1] as Buffer;
      output.writeUInt32LE(0x1234_5678, 28);
      output.writeUInt32LE(0x1122_3344, 44);
      output.writeUInt32LE(0x5566_7788, 48);
      return 1;
    });
    try {
      expect(workspaceIdentity(value.path)).toEqual({
        canonicalPath: value.canonical,
        identity: {
          kind: "windows-low-res",
          volumeSerialNumber: 0x1234_5678n,
          fileIndex: 0x1122_3344_5566_7788n,
        },
      });
    } finally {
      if (previous === undefined) delete mutableApi.GetFileInformationByHandle;
      else mutableApi.GetFileInformationByHandle = previous;
    }
  });
});
