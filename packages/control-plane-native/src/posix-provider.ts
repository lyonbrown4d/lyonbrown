import type { PlatformControlPlaneSecurity } from "@lyonbrown/local-control-plane/native-adapter";

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
