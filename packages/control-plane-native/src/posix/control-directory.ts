import type {
  PlatformResult,
  SecureDirectoryHandle,
} from "@lyonbrown/local-control-plane/native-adapter";
import {
  openDirectory,
  type PosixDirectory,
} from "@lyonbrown/control-plane-native/internal/posix/directory";
import {
  atomicReplace,
  readPrivateFile,
  tryLock,
} from "@lyonbrown/control-plane-native/internal/posix/files";
import { capture } from "@lyonbrown/control-plane-native/internal/result";

class PosixSecureDirectory implements SecureDirectoryHandle {
  #close: Promise<PlatformResult<void>> | undefined;

  public constructor(private readonly directory: PosixDirectory) {}

  public readPrivateFile(name: "agentd.json", maximumBytes: number) {
    return capture(() => readPrivateFile(this.directory, name, maximumBytes));
  }

  public atomicReplacePrivateFile(name: "agentd.json", bytes: Uint8Array) {
    return capture(() => {
      atomicReplace(this.directory, name, bytes);
    });
  }

  public tryAcquireKernelLock(name: "agentd.start.lock" | "agentd.lock") {
    return capture(() => tryLock(this.directory, name));
  }

  public close(): Promise<PlatformResult<void>> {
    this.#close ??= capture(() => {
      this.directory.close();
    });
    return this.#close;
  }
}

export function openSecureDirectory(path: string): SecureDirectoryHandle {
  return new PosixSecureDirectory(openDirectory(path, true, true));
}
