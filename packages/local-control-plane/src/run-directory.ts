import { createDiscoveryClient, type DiscoveryClient } from "./discovery-store.js";
import { controlPlaneError, type ControlPlaneOperation } from "./errors.js";
import {
  invokePlatform,
  resolveNativeControlPlaneProvider,
  type NativeControlPlaneProvider,
  type SecureDirectoryHandle,
} from "./platform-security.js";

declare const secureRunDirectoryBrand: unique symbol;
const RUN_DIRECTORY_CONSTRUCTION_TOKEN = Object.freeze({});

export interface SecureRunDirectory {
  readonly [secureRunDirectoryBrand]: "SecureRunDirectory";
  readonly discovery: DiscoveryClient;
  close(): Promise<void>;
}

const RUN_DIRECTORY_HANDLES = new WeakMap<SecureRunDirectory, SecureDirectoryHandle>();

class OpaqueSecureRunDirectory implements SecureRunDirectory {
  declare public readonly [secureRunDirectoryBrand]: "SecureRunDirectory";
  public readonly discovery: DiscoveryClient;

  public constructor(constructionToken: object, handle: SecureDirectoryHandle) {
    if (constructionToken !== RUN_DIRECTORY_CONSTRUCTION_TOKEN) {
      throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation: "open_run_directory" });
    }
    this.discovery = createDiscoveryClient(handle);
    RUN_DIRECTORY_HANDLES.set(this, handle);
  }

  public close(): Promise<void> {
    const handle = resolveRunDirectoryHandle(this, "close_directory");
    return invokePlatform("close_directory", () => handle.close());
  }
}

export function resolveRunDirectoryHandle(
  directory: SecureRunDirectory,
  operation: ControlPlaneOperation = "kernel_lock",
): SecureDirectoryHandle {
  const handle = RUN_DIRECTORY_HANDLES.get(directory);
  if (!handle) throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation });
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
  return new OpaqueSecureRunDirectory(RUN_DIRECTORY_CONSTRUCTION_TOKEN, handle);
}
