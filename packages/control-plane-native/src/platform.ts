import type { PlatformControlPlaneSecurity } from "@lyonbrown/local-control-plane/native-adapter";
import { createFailClosedPosixSecurity } from "./posix-provider.js";

type SecurityFactory = () => PlatformControlPlaneSecurity;

interface WindowsProviderModule {
  readonly createWindowsSecurity: SecurityFactory;
}

export function selectPlatformSecurity(
  platform: NodeJS.Platform,
  windows: SecurityFactory,
  posix: SecurityFactory,
): PlatformControlPlaneSecurity {
  return (platform === "win32" ? windows : posix)();
}

export async function selectWindowsFactory(
  platform: NodeJS.Platform,
  loadWindows: () => Promise<WindowsProviderModule>,
  unavailable: SecurityFactory,
): Promise<SecurityFactory> {
  if (platform !== "win32") return unavailable;
  return (await loadWindows()).createWindowsSecurity;
}

const unavailableWindows: SecurityFactory = () => {
  throw new Error("Windows native provider is unavailable");
};

const windowsFactory = await selectWindowsFactory(
  process.platform,
  () => import("./windows/provider.js"),
  unavailableWindows,
);

export function createPlatformSecurity(): PlatformControlPlaneSecurity {
  return selectPlatformSecurity(process.platform, windowsFactory, createFailClosedPosixSecurity);
}
