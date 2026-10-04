import type { PlatformControlPlaneSecurity } from "@lyonbrown/local-control-plane/native-adapter";
import { capture } from "../result.js";
import { openSecureDirectory } from "./directory.js";
import { workspaceIdentity } from "./identity.js";

export function createWindowsSecurity(): PlatformControlPlaneSecurity {
  return {
    contract: {
      platform: "windows",
      directoryOpen: "FILE_FLAG_OPEN_REPARSE_POINT",
      ownerProtection: "current-user-only-dacl",
      workspaceIdentity: "rust-file-id",
      kernelLock: "rust-compatible-file-lock",
    },
    workspaceIdentity(path) {
      return capture(() => workspaceIdentity(path));
    },
    openSecureRunDirectory(path) {
      return capture(() => openSecureDirectory(path));
    },
  };
}
