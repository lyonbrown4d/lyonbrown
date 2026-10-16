import { fstatSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import koffi from "koffi";
import { describe, expect, it, vi } from "vitest";
import { createPosixSecurity } from "@lyonbrown/control-plane-native/internal/posix-provider";
import { posixConfiguration } from "@lyonbrown/control-plane-native/internal/posix/constants";
import {
  openDirectory,
  openDirectoryAt,
} from "@lyonbrown/control-plane-native/internal/posix/directory";
import {
  closeFd,
  failureForErrno,
  fdPath,
  FLAGS,
  flock,
  LOCK,
  mkdirAt,
  openAt,
  renameBetween,
  unlinkAt,
} from "@lyonbrown/control-plane-native/internal/posix/native";
import { NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

describe("POSIX platform ABI and native error boundary", () => {
  it("uses explicit Darwin and Linux ABI constants and rejects other platforms", () => {
    expect(posixConfiguration("darwin")).toEqual({
      flags: {
        readOnly: 0,
        readWrite: 2,
        create: 0x200,
        exclusive: 0x800,
        noFollow: 0x100,
        directory: 0x100000,
        closeOnExec: 0x1000000,
        nonBlock: 4,
      },
      libc: "/usr/lib/libSystem.B.dylib",
      workingDirectoryFd: -2,
      removeDirectoryFlag: 0x80,
      descriptorRoot: "/dev/fd",
    });
    expect(posixConfiguration("linux", "x64")).toEqual({
      flags: {
        readOnly: 0,
        readWrite: 2,
        create: 0x40,
        exclusive: 0x80,
        noFollow: 0x20000,
        directory: 0x10000,
        closeOnExec: 0x80000,
        nonBlock: 0x800,
      },
      libc: null,
      workingDirectoryFd: -100,
      removeDirectoryFlag: 0x200,
      descriptorRoot: "/proc/self/fd",
    });
    expect(posixConfiguration("linux", "arm64").flags).toMatchObject({
      noFollow: 0x8000,
      directory: 0x4000,
    });
    expect(() => posixConfiguration("linux", "unknown")).toThrow(NativeFailure);
    expect(() => posixConfiguration("freebsd")).toThrow(NativeFailure);
  });

  it("coarsens native errno values without exposing filesystem paths", () => {
    const errors = koffi.os.errno;
    for (const [name, code] of [
      ["ENOENT", "not_found"],
      ["EACCES", "access_denied"],
      ["EPERM", "access_denied"],
      ["ELOOP", "reparse_point"],
      ["ENOTDIR", "not_directory"],
      ["EIO", "io"],
    ] as const) {
      expect(failureForErrno(errors[name])).toMatchObject({
        failureCode: code,
        message: "native control-plane operation failed",
      });
    }
    expect(() => {
      closeFd(-1);
    }).toThrow(expect.objectContaining({ failureCode: "io" }));
    expect(() => {
      mkdirAt(-1, "child");
    }).toThrow(NativeFailure);
    expect(() => {
      unlinkAt(-1, "child");
    }).toThrow(NativeFailure);
    expect(() => flock(-1, LOCK.exclusive | LOCK.nonBlocking)).toThrow(NativeFailure);
  });

  it("anchors cross-directory rename and directory removal to held descriptors", () => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), "lyonbrown-posix-primitives-")));
    const directory = openDirectory(root, false, true);
    const child = openDirectoryAt(directory, "child", true);
    try {
      mkdirAt(directory.fd, "child");
      const file = openAt(child.fd, "content", FLAGS.create | FLAGS.exclusive | FLAGS.readWrite);
      try {
        writeFileSync(file, "anchored");
        expect(fstatSync(file).isFile()).toBe(true);
        expect(fdPath(file)).toBe(
          `${posixConfiguration(process.platform).descriptorRoot}/${String(file)}`,
        );
        renameBetween(child.fd, "content", directory.fd, "installed");
      } finally {
        closeFd(file);
      }
      unlinkAt(directory.fd, "installed");
      expect(() => {
        renameBetween(child.fd, "missing", directory.fd, "installed");
      }).toThrow(expect.objectContaining({ failureCode: "not_found" }));
      unlinkAt(directory.fd, "child", true);
      expect(() => {
        child.assertIdentity();
      }).toThrow(NativeFailure);
      expect(() => openDirectoryAt(directory, "absent")).toThrow(
        expect.objectContaining({ failureCode: "not_found" }),
      );
    } finally {
      child.close();
      directory.close();
      rmSync(root, { recursive: true, force: true });
    }
  });

  it("fails closed on unsupported hosts before invoking native operations", async () => {
    vi.stubGlobal("process", { ...process, platform: "freebsd" });
    try {
      await expect(createPosixSecurity().workspaceIdentity("/unused")).resolves.toEqual({
        ok: false,
        code: "io",
      });
      await expect(createPosixSecurity().openSecureRunDirectory("/unused")).resolves.toEqual({
        ok: false,
        code: "io",
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
