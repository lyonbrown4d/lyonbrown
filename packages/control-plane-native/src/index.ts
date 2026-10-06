import {
  createNativeControlPlaneProvider,
  type NativeControlPlaneProvider,
} from "@lyonbrown/local-control-plane/native-adapter";
import { createPlatformSecurity } from "@lyonbrown/control-plane-native/internal/platform";
import { createPlatformEventStorePathVerifier } from "@lyonbrown/control-plane-native/internal/platform";
import { createPlatformModelStorageCapability } from "@lyonbrown/control-plane-native/internal/platform";
import type { PathSecurityVerifier } from "@lyonbrown/event-store/path-verifier";
import type { NativeModelStorageCapability } from "@lyonbrown/local-model/composition";
import type { WindowsEventStoreVerifierOptions } from "@lyonbrown/control-plane-native/internal/windows/store-verifier";

let provider: NativeControlPlaneProvider | undefined;

export function createControlPlaneNativeProvider(): NativeControlPlaneProvider {
  provider ??= createNativeControlPlaneProvider(createPlatformSecurity());
  return provider;
}

export function createNativeEventStorePathVerifier(
  options: WindowsEventStoreVerifierOptions = {},
): PathSecurityVerifier {
  return createPlatformEventStorePathVerifier(options);
}

export function createNativeModelStorageCapability(
  modelsDirectory: string,
): NativeModelStorageCapability {
  return createPlatformModelStorageCapability(modelsDirectory);
}

export type { WindowsEventStoreVerifierOptions as NativeEventStorePathVerifierOptions };
