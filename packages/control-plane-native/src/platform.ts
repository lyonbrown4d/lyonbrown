import type { PlatformControlPlaneSecurity } from "@lyonbrown/local-control-plane/native-adapter";
import {
  createPortablePathVerifier,
  type PathSecurityVerifier,
} from "@lyonbrown/event-store/path-verifier";
import { createFailClosedPosixSecurity } from "./posix-provider.js";
import type { WindowsEventStoreVerifierOptions } from "./windows/store-verifier.js";

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

const windowsStoreVerifierFactory =
  process.platform === "win32"
    ? (await import("./windows/store-verifier.js")).createWindowsEventStorePathVerifier
    : undefined;

export function createPlatformSecurity(): PlatformControlPlaneSecurity {
  return selectPlatformSecurity(process.platform, windowsFactory, createFailClosedPosixSecurity);
}

export function createPlatformEventStorePathVerifier(
  options: WindowsEventStoreVerifierOptions = {},
): PathSecurityVerifier {
  if (process.platform !== "win32") return createPortablePathVerifier();
  if (!windowsStoreVerifierFactory) throw new Error("Windows native provider is unavailable");
  return windowsStoreVerifierFactory(options);
}
