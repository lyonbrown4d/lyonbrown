import { createDiscoveryClient, type DiscoveryClient } from "./discovery-store.js";
import { controlPlaneError } from "./errors.js";
import {
  invokePlatform,
  resolveNativeControlPlaneProvider,
  type NativeControlPlaneProvider,
  type SecureDirectoryHandle,
} from "./platform-security.js";

declare const secureRunDirectoryBrand: unique symbol;

export interface SecureRunDirectory {
  readonly [secureRunDirectoryBrand]: "SecureRunDirectory";
  readonly discovery: DiscoveryClient;
  close(): Promise<void>;
}

const RUN_DIRECTORY_HANDLES = new WeakMap<SecureRunDirectory, SecureDirectoryHandle>();

class OpaqueSecureRunDirectory implements SecureRunDirectory {
  declare public readonly [secureRunDirectoryBrand]: "SecureRunDirectory";
  public readonly discovery: DiscoveryClient;

  public constructor(private readonly nativeHandle: SecureDirectoryHandle) {
    this.discovery = createDiscoveryClient(nativeHandle);
    RUN_DIRECTORY_HANDLES.set(this, nativeHandle);
  }

  public close(): Promise<void> {
    return invokePlatform("close_directory", () => this.nativeHandle.close());
  }
}

export function resolveRunDirectoryHandle(directory: SecureRunDirectory): SecureDirectoryHandle {
  const handle = RUN_DIRECTORY_HANDLES.get(directory);
  if (!handle) throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation: "kernel_lock" });
  return handle;
}

export async function openSecureRunDirectory(
  path: string,
  provider?: NativeControlPlaneProvider,
): Promise<SecureRunDirectory> {
  if (!provider) throw controlPlaneError("NATIVE_SECURITY_PROVIDER_REQUIRED");
  const security = resolveNativeControlPlaneProvider(provider);
  const handle = await invokePlatform("open_run_directory", () =>
    security.openSecureRunDirectory(path),
  );
  return new OpaqueSecureRunDirectory(handle);
}
