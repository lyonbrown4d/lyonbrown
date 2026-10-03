import { randomBytes } from "node:crypto";
import type {
  KernelFileLock,
  PlatformControlPlaneSecurity,
  PlatformResult,
  SecureDirectoryHandle,
  WorkspaceIdentity,
} from "../src/platform-security.js";

interface DirectoryState {
  readonly files: Map<string, Uint8Array>;
  readonly locks: Map<string, symbol>;
}

function ok<T>(value: T): PlatformResult<T> {
  return { ok: true, value };
}

export class FakePlatformSecurity implements PlatformControlPlaneSecurity {
  readonly contract = {
    platform: "posix",
    directoryOpen: "dirfd-openat-O_NOFOLLOW",
    ownerProtection: "current-uid-owner-mode",
    workspaceIdentity: "rust-dev-ino",
    kernelLock: "rust-compatible-file-lock",
  } as const;
  readonly identities = new Map<string, WorkspaceIdentity>();
  readonly directories = new Map<string, DirectoryState>();
  readonly operations: string[] = [];
  readonly temporaryNames: string[] = [];
  readonly liveTemporaryNames = new Set<string>();
  nextFailure: PlatformResult<never> | undefined;
  handleException: "atomic" | "close" | "lock" | "read" | "release" | undefined;

  setIdentity(path: string, identity: WorkspaceIdentity): void {
    this.identities.set(path, identity);
  }

  replaceDirectoryPath(path: string): void {
    this.directories.set(path, { files: new Map(), locks: new Map() });
  }

  workspaceIdentity(path: string): Promise<PlatformResult<WorkspaceIdentity>> {
    this.operations.push(`identity:${path}`);
    const failed = this.consumeFailure<WorkspaceIdentity>();
    if (failed) return Promise.resolve(failed);
    const identity = this.identities.get(path);
    return Promise.resolve(identity ? ok(identity) : { ok: false, code: "not_found" });
  }

  openSecureRunDirectory(path: string): Promise<PlatformResult<SecureDirectoryHandle>> {
    this.operations.push(`open-directory:${path}`);
    const failed = this.consumeFailure<SecureDirectoryHandle>();
    if (failed) return Promise.resolve(failed);
    let state = this.directories.get(path);
    if (!state) {
      state = { files: new Map(), locks: new Map() };
      this.directories.set(path, state);
    }
    return Promise.resolve(ok(this.makeHandle(state)));
  }

  private consumeFailure<T>(): PlatformResult<T> | undefined {
    const failure = this.nextFailure;
    this.nextFailure = undefined;
    return failure;
  }

  private makeHandle(state: DirectoryState): SecureDirectoryHandle {
    return Object.freeze({
      readPrivateFile: (name: "agentd.json", maximumBytes: number) => {
        if (this.handleException === "read") throw new Error("native read secret");
        this.operations.push(`read:${name}`);
        const bytes = state.files.get(name);
        if (!bytes) return Promise.resolve(ok(null));
        if (bytes.byteLength > maximumBytes) {
          return Promise.resolve({ ok: false as const, code: "oversize" as const });
        }
        return Promise.resolve(ok(Uint8Array.from(bytes)));
      },
      atomicReplacePrivateFile: (name: "agentd.json", bytes: Uint8Array) => {
        if (this.handleException === "atomic") throw new Error("native write secret");
        this.operations.push(`atomic-replace:${name}`);
        const temporaryName = `.agentd-${randomBytes(16).toString("hex")}.tmp`;
        this.temporaryNames.push(temporaryName);
        this.liveTemporaryNames.add(temporaryName);
        try {
          state.files.set(name, Uint8Array.from(bytes));
        } finally {
          this.liveTemporaryNames.delete(temporaryName);
        }
        return Promise.resolve(ok(undefined));
      },
      tryAcquireKernelLock: (name: "agentd.start.lock" | "agentd.lock") => {
        if (this.handleException === "lock") throw new Error("native lock secret");
        this.operations.push(`lock:${name}`);
        if (state.locks.has(name)) return Promise.resolve(ok(null));
        const owner = Symbol(name);
        state.locks.set(name, owner);
        const lock: KernelFileLock = Object.freeze({
          release: () => {
            if (this.handleException === "release") throw new Error("native release secret");
            if (state.locks.get(name) === owner) state.locks.delete(name);
            return Promise.resolve(ok(undefined));
          },
        });
        return Promise.resolve(ok(lock));
      },
      close: () => {
        if (this.handleException === "close") throw new Error("native close secret");
        return Promise.resolve(ok(undefined));
      },
    });
  }
}

export const INODE_IDENTITY: WorkspaceIdentity = Object.freeze({
  kind: "inode",
  deviceId: 0x0102_0304_0506_0708n,
  inodeNumber: 0x1112_1314_1516_1718n,
});
