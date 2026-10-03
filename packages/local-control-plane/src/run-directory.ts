import { DiscoveryStore } from "./discovery-store.js";
import { controlPlaneError } from "./errors.js";
import {
  invokePlatform,
  type KernelFileLock,
  type PlatformControlPlaneSecurity,
  type PlatformResult,
  type SecureDirectoryHandle,
} from "./platform-security.js";

export class SecureRunDirectory implements SecureDirectoryHandle {
  public readonly discovery: DiscoveryStore;

  public constructor(private readonly nativeHandle: SecureDirectoryHandle) {
    this.discovery = new DiscoveryStore(this);
  }

  public async readPrivateFile(
    name: "agentd.json",
    maximumBytes: number,
  ): Promise<PlatformResult<Uint8Array | null>> {
    try {
      return await this.nativeHandle.readPrivateFile(name, maximumBytes);
    } catch {
      return { ok: false, code: "io" };
    }
  }

  public async atomicReplacePrivateFile(
    name: "agentd.json",
    bytes: Uint8Array,
  ): Promise<PlatformResult<void>> {
    try {
      return await this.nativeHandle.atomicReplacePrivateFile(name, bytes);
    } catch {
      return { ok: false, code: "io" };
    }
  }

  public async tryAcquireKernelLock(
    name: "agentd.start.lock" | "agentd.lock",
  ): Promise<PlatformResult<KernelFileLock | null>> {
    try {
      const result = await this.nativeHandle.tryAcquireKernelLock(name);
      if (!result.ok || result.value === null) return result;
      const nativeLock = result.value;
      return {
        ok: true,
        value: Object.freeze({
          async release(): Promise<PlatformResult<void>> {
            try {
              return await nativeLock.release();
            } catch {
              return { ok: false, code: "io" };
            }
          },
        }),
      };
    } catch {
      return { ok: false, code: "io" };
    }
  }

  public async close(): Promise<PlatformResult<void>> {
    try {
      await invokePlatform("close_directory", () => this.nativeHandle.close());
      return { ok: true, value: undefined };
    } catch {
      return { ok: false, code: "io" };
    }
  }

}

export async function openSecureRunDirectory(
  path: string,
  security?: PlatformControlPlaneSecurity,
): Promise<SecureRunDirectory> {
  if (!security) throw controlPlaneError("NATIVE_SECURITY_PROVIDER_REQUIRED");
  const handle = await invokePlatform("open_run_directory", () =>
    security.openSecureRunDirectory(path),
  );
  return new SecureRunDirectory(handle);
}
