import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { WindowsAclProtector } from "../src/index.js";

export async function withTemporaryDirectory<T>(run: (path: string) => Promise<T>): Promise<T> {
  const path = await mkdtemp(join(tmpdir(), "lyonbrown-control-plane-"));
  try {
    return await run(path);
  } finally {
    await rm(path, { recursive: true, force: true });
  }
}

export class RecordingWindowsAclProtector implements WindowsAclProtector {
  readonly operations: string[] = [];

  protectDirectory(path: string): Promise<void> {
    this.operations.push(`protect-directory:${path}`);
    return Promise.resolve();
  }

  verifyDirectory(path: string): Promise<void> {
    this.operations.push(`verify-directory:${path}`);
    return Promise.resolve();
  }

  protectFile(path: string, fileDescriptor: number): Promise<void> {
    void fileDescriptor;
    this.operations.push(`protect-file:${path}`);
    return Promise.resolve();
  }

  verifyFile(path: string, fileDescriptor: number): Promise<void> {
    void fileDescriptor;
    this.operations.push(`verify-file:${path}`);
    return Promise.resolve();
  }
}
