import type { PlatformControlPlaneSecurity } from "@lyonbrown/local-control-plane/native-adapter";
import { capture } from "../result.ts";
import { openSecureDirectory } from "./directory.ts";
import { workspaceIdentity } from "./identity.ts";

export function createWindowsSecurity(): PlatformControlPlaneSecurity {
  return {
    contract: {
      platform: "windows",
      directoryOpen: "FILE_FLAG_OPEN_REPARSE_POINT",
      ownerProtection: "current-user-only-dacl",
      workspaceIdentity: "native-file-id-v1",
      kernelLock: "kernel-file-lock-v1",
    },
    workspaceIdentity(path) {
      return capture(() => workspaceIdentity(path));
    },
    openSecureRunDirectory(path) {
      return capture(() => openSecureDirectory(path));
    },
  };
}
