import type { PlatformControlPlaneSecurity } from "@lyonbrown/local-control-plane/native-adapter";
import {
  createPortablePathVerifier,
  type PathSecurityVerifier,
} from "@lyonbrown/event-store/path-verifier";
import { createFailClosedPosixSecurity } from "@lyonbrown/control-plane-native/internal/posix-provider";
import type { WindowsEventStoreVerifierOptions } from "@lyonbrown/control-plane-native/internal/windows/store-verifier";
import type { NativeModelStorageCapability } from "@lyonbrown/local-model/composition";
import type { NativeModelBundleStorageCapability } from "@lyonbrown/control-plane-native/internal/model-bundle-storage";

type SecurityFactory = () => PlatformControlPlaneSecurity;

interface WindowsProviderModule {
  readonly createWindowsSecurity: SecurityFactory;
}

type ModelStorageFactory = (modelsDirectory: string) => NativeModelStorageCapability;

interface WindowsModelStorageModule {
  readonly createWindowsModelStorageCapability: ModelStorageFactory;
}

type ModelBundleStorageFactory = (modelsDirectory: string) => NativeModelBundleStorageCapability;

interface WindowsModelBundleStorageModule {
  readonly createWindowsModelBundleStorageCapability: ModelBundleStorageFactory;
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

export async function selectWindowsModelStorageFactory(
  platform: NodeJS.Platform,
  loadWindows: () => Promise<WindowsModelStorageModule>,
  unavailable: ModelStorageFactory,
): Promise<ModelStorageFactory> {
  if (platform !== "win32") return unavailable;
  return (await loadWindows()).createWindowsModelStorageCapability;
}

export async function selectWindowsModelBundleStorageFactory(
  platform: NodeJS.Platform,
  loadWindows: () => Promise<WindowsModelBundleStorageModule>,
  unavailable: ModelBundleStorageFactory,
): Promise<ModelBundleStorageFactory> {
  if (platform !== "win32") return unavailable;
  return (await loadWindows()).createWindowsModelBundleStorageCapability;
}

const unavailableWindows: SecurityFactory = () => {
  throw new Error("Windows native provider is unavailable");
};

const windowsFactory = await selectWindowsFactory(
  process.platform,
  () => import("@lyonbrown/control-plane-native/internal/windows/provider"),
  unavailableWindows,
);

const windowsStoreVerifierFactory =
  process.platform === "win32"
    ? (await import("@lyonbrown/control-plane-native/internal/windows/store-verifier"))
        .createWindowsEventStorePathVerifier
    : undefined;

const unavailableModelStorage: ModelStorageFactory = () => {
  throw new Error("native_model_storage_unavailable");
};

const windowsModelStorageFactory = await selectWindowsModelStorageFactory(
  process.platform,
  () => import("@lyonbrown/control-plane-native/internal/windows/model-storage"),
  unavailableModelStorage,
);

const unavailableModelBundleStorage: ModelBundleStorageFactory = () => {
  throw new Error("native_model_bundle_storage_unavailable");
};

const windowsModelBundleStorageFactory = await selectWindowsModelBundleStorageFactory(
  process.platform,
  () => import("@lyonbrown/control-plane-native/internal/windows/model-bundle-storage"),
  unavailableModelBundleStorage,
);

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

export function createPlatformModelStorageCapability(
  modelsDirectory: string,
): NativeModelStorageCapability {
  return windowsModelStorageFactory(modelsDirectory);
}

export function createPlatformModelBundleStorageCapability(
  modelsDirectory: string,
): NativeModelBundleStorageCapability {
  return windowsModelBundleStorageFactory(modelsDirectory);
}
