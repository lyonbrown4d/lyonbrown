import { lstat, mkdir, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  createSecurityPolicy,
  prepareSecureDirectory,
  rejectLinkOrWrongType,
  type PosixFileSystem,
  type PosixMetadata,
} from "../src/security.js";
import { RecordingWindowsAclProtector, withTemporaryDirectory } from "./test-support.js";

describe("owner-only security policies", () => {
  it("selects and delegates to the injected Windows DACL protector", async () => {
    const protector = new RecordingWindowsAclProtector();
    const policy = createSecurityPolicy({ platform: "win32", windowsAclProtector: protector });

    await policy.protectDirectory("directory");
    await policy.verifyDirectory("directory");
    const fakeFileMetadata: PosixMetadata = {
      mode: 0o600,
      uid: 1,
      isDirectory: () => false,
      isFile: () => true,
      isSymbolicLink: () => false,
    };
    await policy.protectFile("file", 1);
    await policy.verifyFile("file", { fileDescriptor: 1, metadata: fakeFileMetadata });

    expect(policy.platform).toBe("win32");
    expect(protector.operations).toEqual([
      "protect-directory:directory",
      "verify-directory:directory",
      "protect-file:file",
      "verify-file:file",
    ]);
  });

  it("enforces POSIX directory/file mode, owner, and file type with injected identity", async () => {
    await withTemporaryDirectory(async (root) => {
      const uid = (await lstat(root)).uid;
      const virtualModes = new Map<string, number>();
      const fileSystem: PosixFileSystem = {
        chmodDirectory(path, mode) {
          virtualModes.set(path, mode);
          return Promise.resolve();
        },
        chmodFile(path, fileDescriptor, mode) {
          void fileDescriptor;
          virtualModes.set(path, mode);
          return Promise.resolve();
        },
        async lstat(path): Promise<PosixMetadata> {
          const actual = await lstat(path);
          return {
            uid,
            mode: virtualModes.get(path) ?? actual.mode,
            isDirectory: () => actual.isDirectory(),
            isFile: () => actual.isFile(),
            isSymbolicLink: () => actual.isSymbolicLink(),
          };
        },
      };
      const policy = createSecurityPolicy({
        platform: "linux",
        posixEffectiveUid: uid,
        posixFileSystem: fileSystem,
      });
      const runPath = await prepareSecureDirectory(join(root, "nested", "run"), policy);
      const filePath = join(runPath, "descriptor.json");
      await writeFile(filePath, "{}", { mode: 0o666 });
      await policy.protectFile(filePath, 1);

      await expect(policy.verifyDirectory(runPath)).resolves.toBeUndefined();
      const opened = { fileDescriptor: 1, metadata: await fileSystem.lstat(filePath) };
      await expect(policy.verifyFile(filePath, opened)).resolves.toBeUndefined();
      expect((virtualModes.get(runPath) ?? 0) & 0o777).toBe(0o700);
      expect((virtualModes.get(filePath) ?? 0) & 0o777).toBe(0o600);
      await expect(policy.verifyDirectory(filePath)).rejects.toMatchObject({
        code: "WRONG_FILE_TYPE",
      });
      await expect(
        createSecurityPolicy({
          platform: "linux",
          posixEffectiveUid: uid + 1,
          posixFileSystem: fileSystem,
        }).verifyFile(filePath, opened),
      ).rejects.toMatchObject({ code: "INSECURE_OWNER" });

      virtualModes.set(filePath, 0o644);
      await expect(
        policy.verifyFile(filePath, {
          fileDescriptor: 1,
          metadata: await fileSystem.lstat(filePath),
        }),
      ).rejects.toMatchObject({
        code: "INSECURE_PERMISSIONS",
      });
    });
  });

  it("rejects wrong types, missing paths, and reparse points before use", async () => {
    await withTemporaryDirectory(async (root) => {
      const file = join(root, "file");
      const directory = join(root, "directory");
      const link = join(root, "link");
      await writeFile(file, "x");
      await mkdir(directory);
      await symlink(directory, link, "junction");

      await expect(rejectLinkOrWrongType(file, "directory")).rejects.toMatchObject({
        code: "WRONG_FILE_TYPE",
      });
      await expect(rejectLinkOrWrongType(directory, "file")).rejects.toMatchObject({
        code: "WRONG_FILE_TYPE",
      });
      await expect(rejectLinkOrWrongType(link, "directory")).rejects.toMatchObject({
        code: "SYMLINK_REJECTED",
      });
      await expect(rejectLinkOrWrongType(join(root, "missing"), "file")).rejects.toMatchObject({
        code: "DESCRIPTOR_NOT_FOUND",
      });
    });
  });
});
