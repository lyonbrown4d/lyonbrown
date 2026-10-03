import { constants } from "node:fs";
import { open, rename, rm, type FileHandle } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import {
  MAX_DISCOVERY_BYTES,
  type DiscoveryDescriptor,
  parseDiscoveryDescriptor,
  serializeDiscoveryDescriptor,
} from "./descriptor.js";
import { ControlPlaneError, controlPlaneError, isNodeErrorWithCode } from "./errors.js";
import { rejectLinkOrWrongType, type SecurityPolicy } from "./security.js";
import { SecretToken } from "./token.js";

const DISCOVERY_FILE = "agentd.json";

async function verifyOpenedFile(path: string, handle: FileHandle, policy: SecurityPolicy) {
  const metadata = await handle.stat();
  await policy.verifyFile(path, { fileDescriptor: handle.fd, metadata });
  return metadata;
}

async function syncDirectory(path: string, platform: NodeJS.Platform): Promise<void> {
  if (platform === "win32") return;
  const handle = await open(path, constants.O_RDONLY);
  try {
    await handle.sync();
  } finally {
    await handle.close();
  }
}

export class DiscoveryStore {
  public readonly path: string;

  public constructor(
    runDirectory: string,
    private readonly policy: SecurityPolicy,
  ) {
    this.path = join(runDirectory, DISCOVERY_FILE);
  }

  public async publish(descriptor: DiscoveryDescriptor): Promise<void> {
    const bytes = Buffer.from(serializeDiscoveryDescriptor(descriptor), "utf8");
    const nonce = SecretToken.generate().exposeForTransport().slice(0, 20);
    const temporaryPath = join(dirname(this.path), `.agentd-${nonce}.tmp`);
    let handle;
    try {
      handle = await open(
        temporaryPath,
        constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | constants.O_NOFOLLOW,
        0o600,
      );
      await handle.writeFile(bytes);
      await handle.sync();
      await this.policy.protectFile(temporaryPath, handle.fd);
      await verifyOpenedFile(temporaryPath, handle, this.policy);
      await handle.close();
      handle = undefined;
      await rename(temporaryPath, this.path);
      await syncDirectory(dirname(this.path), this.policy.platform);
    } catch (error) {
      if (error instanceof ControlPlaneError) throw error;
      throw controlPlaneError("IO_FAILURE", error);
    } finally {
      await handle?.close().catch(() => undefined);
      await rm(temporaryPath, { force: true }).catch(() => undefined);
    }
  }

  public async read(): Promise<DiscoveryDescriptor> {
    await rejectLinkOrWrongType(this.path, "file");
    let handle;
    try {
      handle = await open(this.path, constants.O_RDONLY | constants.O_NOFOLLOW);
      const metadata = await verifyOpenedFile(this.path, handle, this.policy);
      if (!metadata.isFile()) throw controlPlaneError("WRONG_FILE_TYPE");
      if (metadata.size > MAX_DISCOVERY_BYTES) throw controlPlaneError("DESCRIPTOR_TOO_LARGE");
      const bytes = Buffer.alloc(metadata.size);
      const { bytesRead } = await handle.read(bytes, 0, bytes.length, 0);
      return parseDiscoveryDescriptor(bytes.subarray(0, bytesRead));
    } catch (error) {
      if (error instanceof ControlPlaneError) throw error;
      if (isNodeErrorWithCode(error, "ENOENT")) throw controlPlaneError("DESCRIPTOR_NOT_FOUND");
      throw controlPlaneError("IO_FAILURE", error);
    } finally {
      await handle?.close().catch(() => undefined);
    }
  }

  public fileName(): string {
    return basename(this.path);
  }
}
