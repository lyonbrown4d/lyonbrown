import type { PlatformControlPlaneSecurity } from "@lyonbrown/local-control-plane/native-adapter";
import { createFailClosedPosixSecurity } from "./posix-provider.js";

const windowsFactory =
  process.platform === "win32"
    ? (await import("./windows/provider.js")).createWindowsSecurity
    : null;

export function createPlatformSecurity(): PlatformControlPlaneSecurity {
  return windowsFactory ? windowsFactory() : createFailClosedPosixSecurity();
}
