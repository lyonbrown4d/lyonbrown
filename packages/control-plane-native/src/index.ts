import {
  createNativeControlPlaneProvider,
  type NativeControlPlaneProvider,
} from "@lyonbrown/local-control-plane/native-adapter";
import { createPlatformSecurity } from "./platform.js";
import { createPlatformEventStorePathVerifier } from "./platform.js";
import type { PathSecurityVerifier } from "@lyonbrown/event-store/path-verifier";
import type { WindowsEventStoreVerifierOptions } from "./windows/store-verifier.js";

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

export type { WindowsEventStoreVerifierOptions as NativeEventStorePathVerifierOptions };
