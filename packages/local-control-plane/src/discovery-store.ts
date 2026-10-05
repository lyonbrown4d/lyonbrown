import {
  MAX_DISCOVERY_BYTES,
  type DiscoveryDescriptor,
  parseDiscoveryDescriptor,
  serializeDiscoveryDescriptor,
} from "./descriptor.ts";
import { controlPlaneError } from "./errors.ts";
import {
  invokePlatform,
  type PlatformResult,
  type SecureDirectoryHandle,
} from "./platform-security.ts";

const DISCOVERY_FILE = "agentd.json";
const DISCOVERY_CONSTRUCTION_TOKEN = Object.freeze({});

export interface DiscoveryClient {
  publish(descriptor: DiscoveryDescriptor): Promise<void>;
  read(): Promise<DiscoveryDescriptor>;
  fileName(): string;
}

class AnchoredDiscoveryStore implements DiscoveryClient {
  public constructor(constructionToken: object, directory: SecureDirectoryHandle) {
    if (constructionToken !== DISCOVERY_CONSTRUCTION_TOKEN) {
      throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation: "read_descriptor" });
    }
    DISCOVERY_HANDLES.set(this, directory);
  }

  public async publish(descriptor: DiscoveryDescriptor): Promise<void> {
    const bytes = Buffer.from(serializeDiscoveryDescriptor(descriptor), "utf8");
    const directory = resolveDiscoveryHandle(this, "atomic_replace");
    await invokePlatform("atomic_replace", () =>
      directory.atomicReplacePrivateFile(DISCOVERY_FILE, bytes),
    );
  }

  public async read(): Promise<DiscoveryDescriptor> {
    const directory = resolveDiscoveryHandle(this, "read_descriptor");
    let result: PlatformResult<Uint8Array | null>;
    try {
      result = await directory.readPrivateFile(DISCOVERY_FILE, MAX_DISCOVERY_BYTES);
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

const DISCOVERY_HANDLES = new WeakMap<AnchoredDiscoveryStore, SecureDirectoryHandle>();

function resolveDiscoveryHandle(
  store: AnchoredDiscoveryStore,
  operation: "atomic_replace" | "read_descriptor",
): SecureDirectoryHandle {
  const handle = DISCOVERY_HANDLES.get(store);
  if (!handle) throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation });
  return handle;
}

export function createDiscoveryClient(directory: SecureDirectoryHandle): DiscoveryClient {
  return new AnchoredDiscoveryStore(DISCOVERY_CONSTRUCTION_TOKEN, directory);
}

function invokePlatformResult<T>(result: PlatformResult<T>): T {
  if (!result.ok)
    throw controlPlaneError("PLATFORM_SECURITY_FAILURE", { operation: "read_descriptor" });
  return result.value;
}
