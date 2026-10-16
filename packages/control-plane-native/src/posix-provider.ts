import type { PlatformControlPlaneSecurity } from "@lyonbrown/local-control-plane/native-adapter";
import { fstatSync, realpathSync } from "node:fs";
import { capture, NativeFailure } from "@lyonbrown/control-plane-native/internal/result";

export function createPosixSecurity(): PlatformControlPlaneSecurity {
  return {
    contract: createFailClosedPosixSecurity().contract,
    workspaceIdentity(path) {
      return capture(async () => {
        if (process.platform !== "darwin" && process.platform !== "linux")
          throw new NativeFailure("io");
        const { openDirectory } = await import(
          "@lyonbrown/control-plane-native/internal/posix/directory"
        );
        const canonicalPath = realpathSync(path);
        const directory = openDirectory(canonicalPath);
        try {
          const stat = fstatSync(directory.fd, { bigint: true });
          directory.assertIdentity();
          return {
            canonicalPath,
            identity: { kind: "inode" as const, deviceId: stat.dev, inodeNumber: stat.ino },
          };
        } finally {
          directory.close();
        }
      });
    },
    openSecureRunDirectory(path) {
      return capture(async () => {
        if (process.platform !== "darwin" && process.platform !== "linux")
          throw new NativeFailure("io");
        const { openSecureDirectory } = await import(
          "@lyonbrown/control-plane-native/internal/posix/control-directory"
        );
        return openSecureDirectory(path);
      });
    },
  };
}

export function createFailClosedPosixSecurity(): PlatformControlPlaneSecurity {
  return {
    contract: {
      platform: "posix",
      directoryOpen: "dirfd-openat-O_NOFOLLOW",
      ownerProtection: "current-uid-owner-mode",
      workspaceIdentity: "native-dev-ino-v1",
      kernelLock: "kernel-file-lock-v1",
    },
    workspaceIdentity() {
      return Promise.resolve({ ok: false, code: "io" });
    },
    openSecureRunDirectory() {
      return Promise.resolve({ ok: false, code: "io" });
    },
  };
}
