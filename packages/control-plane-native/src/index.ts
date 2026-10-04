import {
  createNativeControlPlaneProvider,
  type NativeControlPlaneProvider,
} from "@lyonbrown/local-control-plane/native-adapter";
import { createPlatformSecurity } from "./platform.js";

let provider: NativeControlPlaneProvider | undefined;

export function createControlPlaneNativeProvider(): NativeControlPlaneProvider {
  provider ??= createNativeControlPlaneProvider(createPlatformSecurity());
  return provider;
}
