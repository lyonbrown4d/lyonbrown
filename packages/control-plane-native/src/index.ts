import {
  createNativeControlPlaneProvider,
  type NativeControlPlaneProvider,
} from "@lyonbrown/local-control-plane/native-adapter";
import { createPlatformSecurity } from "./platform.ts";
import { createPlatformEventStorePathVerifier } from "./platform.ts";
import { createPlatformModelStorageCapability } from "./platform.ts";
import type { PathSecurityVerifier } from "@lyonbrown/event-store/path-verifier";
import type { NativeModelStorageCapability } from "@lyonbrown/local-model/composition";
import type { WindowsEventStoreVerifierOptions } from "./windows/store-verifier.ts";

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
