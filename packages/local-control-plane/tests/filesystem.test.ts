import { lstat, mkdir, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  ControlPlaneError,
  type DiscoveryDescriptor,
  MAX_DISCOVERY_BYTES,
  SecretToken,
  createDiscoveryDescriptor,
  openSecureRunDirectory,
} from "../src/index.js";
import { RecordingWindowsAclProtector, withTemporaryDirectory } from "./test-support.js";

const SCOPE_ID = "cd".repeat(32);

function descriptor(pid = 101) {
  return createDiscoveryDescriptor({ endpoint: "127.0.0.1:41000", pid, scopeId: SCOPE_ID });
}

describe("secure discovery filesystem", () => {
  it("fails closed on Windows when no DACL adapter is provided", async () => {
    await withTemporaryDirectory(async (root) => {
      await expect(
        openSecureRunDirectory(join(root, "run"), { platform: "win32" }),
      ).rejects.toMatchObject({ code: "WINDOWS_ACL_REQUIRED" });
    });
  });

  it("uses the Windows ACL seam for directories and descriptor files", async () => {
    await withTemporaryDirectory(async (root) => {
      const acl = new RecordingWindowsAclProtector();
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: acl,
      });
      await runDirectory.discovery.publish(descriptor());
      await runDirectory.discovery.read();

      expect(acl.operations.some((entry) => entry.startsWith("protect-directory:"))).toBe(true);
      expect(acl.operations.some((entry) => entry.startsWith("verify-directory:"))).toBe(true);
      expect(acl.operations.some((entry) => entry.startsWith("protect-file:"))).toBe(true);
      expect(acl.operations.some((entry) => entry.startsWith("verify-file:"))).toBe(true);
    });
  });

  it("publishes atomically and removes random temporary files", async () => {
    await withTemporaryDirectory(async (root) => {
      const acl = new RecordingWindowsAclProtector();
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: acl,
      });

      await Promise.all([
        runDirectory.discovery.publish(descriptor(201)),
        runDirectory.discovery.publish(descriptor(202)),
      ]);
      const result = await runDirectory.discovery.read();
      const entries = await readdir(runDirectory.path);

      expect([201, 202]).toContain(result.pid);
      expect(runDirectory.discovery.fileName()).toBe("agentd.json");
      expect(
        entries.filter((entry) => entry.startsWith(".agentd-") && entry.endsWith(".tmp")),
      ).toEqual([]);
    });
  });

  it("propagates typed descriptor errors and cleans temporary files after publish failure", async () => {
    await withTemporaryDirectory(async (root) => {
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: new RecordingWindowsAclProtector(),
      });
      const invalid = { ...descriptor(), schemaVersion: 2 } as unknown as DiscoveryDescriptor;
      await expect(runDirectory.discovery.publish(invalid)).rejects.toMatchObject({
        code: "INVALID_DESCRIPTOR",
      });

      await mkdir(runDirectory.discovery.path);
      await expect(runDirectory.discovery.publish(descriptor())).rejects.toMatchObject({
        code: "IO_FAILURE",
      });
      expect((await readdir(runDirectory.path)).filter((entry) => entry.endsWith(".tmp"))).toEqual(
        [],
      );
    });
  });

  it("fails closed when ACL verification races with descriptor replacement", async () => {
    await withTemporaryDirectory(async (root) => {
      const removingAcl = new RecordingWindowsAclProtector();
      removingAcl.verifyFile = (path: string, fileDescriptor: number): Promise<void> => {
        void fileDescriptor;
        removingAcl.operations.push(`verify-file:${path}`);
        const error = new Error("descriptor disappeared") as Error & { code: string };
        error.code = "ENOENT";
        return Promise.reject(error);
      };
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: removingAcl,
      });
      await writeFile(runDirectory.discovery.path, JSON.stringify({}));

      await expect(runDirectory.discovery.read()).rejects.toMatchObject({
        code: "DESCRIPTOR_NOT_FOUND",
      });
    });
  });

  it("does not leak raw ACL adapter errors", async () => {
    await withTemporaryDirectory(async (root) => {
      const acl = new RecordingWindowsAclProtector();
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: acl,
      });
      await writeFile(runDirectory.discovery.path, JSON.stringify({}));
      acl.verifyFile = (): Promise<void> => Promise.reject(new Error("sensitive adapter detail"));

      await expect(runDirectory.discovery.read()).rejects.toMatchObject({ code: "IO_FAILURE" });
      acl.verifyFile = (): Promise<void> =>
        Promise.reject(new ControlPlaneError("INSECURE_PERMISSIONS"));
      await expect(runDirectory.discovery.read()).rejects.toMatchObject({
        code: "INSECURE_PERMISSIONS",
      });
    });
  });

  it("rejects oversized and symlinked discovery paths", async () => {
    await withTemporaryDirectory(async (root) => {
      const acl = new RecordingWindowsAclProtector();
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: acl,
      });
      await writeFile(runDirectory.discovery.path, Buffer.alloc(MAX_DISCOVERY_BYTES + 1));
      await expect(runDirectory.discovery.read()).rejects.toMatchObject({
        code: "DESCRIPTOR_TOO_LARGE",
      });

      const targetDirectory = join(root, "outside");
      await mkdir(targetDirectory);
      await writeFile(join(targetDirectory, "ignored"), "outside");
      await rm(runDirectory.discovery.path);
      await symlink(targetDirectory, runDirectory.discovery.path, "junction");
      await expect(runDirectory.discovery.read()).rejects.toMatchObject({
        code: "SYMLINK_REJECTED",
      });
    });
  });

  it("rejects missing, malformed, and wrong-type discovery entries", async () => {
    await withTemporaryDirectory(async (root) => {
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: new RecordingWindowsAclProtector(),
      });

      await expect(runDirectory.discovery.read()).rejects.toMatchObject({
        code: "DESCRIPTOR_NOT_FOUND",
      });
      await writeFile(runDirectory.discovery.path, "not-json");
      await expect(runDirectory.discovery.read()).rejects.toMatchObject({
        code: "INVALID_DESCRIPTOR",
      });
      await rm(runDirectory.discovery.path);
      await mkdir(runDirectory.discovery.path);
      await expect(runDirectory.discovery.read()).rejects.toMatchObject({
        code: "WRONG_FILE_TYPE",
      });
    });
  });

  it("rejects a symlink or junction in the run-directory path", async () => {
    await withTemporaryDirectory(async (root) => {
      const outside = join(root, "outside");
      const linked = join(root, "linked");
      await mkdir(outside);
      await symlink(outside, linked, "junction");

      await expect(
        openSecureRunDirectory(join(linked, "scope"), {
          platform: "win32",
          windowsAclProtector: new RecordingWindowsAclProtector(),
        }),
      ).rejects.toMatchObject({ code: "SYMLINK_REJECTED" });
    });
  });

  it.runIf(process.platform !== "win32")("enforces owner-only POSIX modes", async () => {
    await withTemporaryDirectory(async (root) => {
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: process.platform,
      });
      await runDirectory.discovery.publish(descriptor());

      expect((await lstat(runDirectory.path)).mode & 0o777).toBe(0o700);
      expect((await lstat(runDirectory.discovery.path)).mode & 0o777).toBe(0o600);
      expect(await readFile(runDirectory.discovery.path, "utf8")).toContain(
        SecretToken.prototype.constructor.name === "SecretToken" ? '"token"' : "unreachable",
      );
    });
  });
});
