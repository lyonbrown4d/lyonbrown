import {
  MAX_DISCOVERY_BYTES,
  type DiscoveryDescriptor,
  parseDiscoveryDescriptor,
  serializeDiscoveryDescriptor,
} from "./descriptor.js";
import { controlPlaneError } from "./errors.js";
import {
  invokePlatform,
  type PlatformResult,
  type SecureDirectoryHandle,
} from "./platform-security.js";

const DISCOVERY_FILE = "agentd.json";

export interface DiscoveryClient {
  publish(descriptor: DiscoveryDescriptor): Promise<void>;
  read(): Promise<DiscoveryDescriptor>;
  fileName(): string;
}

class AnchoredDiscoveryStore implements DiscoveryClient {
  public constructor(private readonly directory: SecureDirectoryHandle) {}

  public async publish(descriptor: DiscoveryDescriptor): Promise<void> {
    const bytes = Buffer.from(serializeDiscoveryDescriptor(descriptor), "utf8");
    await invokePlatform("atomic_replace", () =>
      this.directory.atomicReplacePrivateFile(DISCOVERY_FILE, bytes),
    );
  }

  public async read(): Promise<DiscoveryDescriptor> {
    let result: PlatformResult<Uint8Array | null>;
    try {
      result = await this.directory.readPrivateFile(DISCOVERY_FILE, MAX_DISCOVERY_BYTES);
    } catch {
      throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation: "read_descriptor" });
    }
    if (!result.ok && result.code === "oversize") throw controlPlaneError("DESCRIPTOR_TOO_LARGE");
    const bytes = invokePlatformResult(result);
    if (bytes === null) throw controlPlaneError("DESCRIPTOR_NOT_FOUND");
    return parseDiscoveryDescriptor(bytes);
  }

  public fileName(): string {
    return DISCOVERY_FILE;
  }
}

export function createDiscoveryClient(directory: SecureDirectoryHandle): DiscoveryClient {
  return new AnchoredDiscoveryStore(directory);
}

function invokePlatformResult<T>(result: PlatformResult<T>): T {
  if (!result.ok)
    throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation: "read_descriptor" });
  return result.value;
}
