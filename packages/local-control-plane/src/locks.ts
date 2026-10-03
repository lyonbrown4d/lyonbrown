import { join } from "node:path";
import lockfile from "proper-lockfile";
import { controlPlaneError } from "./errors.js";
import type { SecureRunDirectory } from "./run-directory.js";

const DEFAULT_STALE_MS = 10_000;
const MINIMUM_STALE_MS = 5_000;

export interface LockOptions {
  readonly staleMs?: number;
}

export interface DaemonLease {
  readonly kind: "startup" | "lifetime";
  readonly path: string;
  release(): Promise<void>;
}

interface LockError extends Error {
  readonly code?: string;
}

async function acquireLease(
  runDirectory: SecureRunDirectory,
  kind: DaemonLease["kind"],
  options: LockOptions,
): Promise<DaemonLease | null> {
  const stale = Math.max(options.staleMs ?? DEFAULT_STALE_MS, MINIMUM_STALE_MS);
  const targetName = kind === "startup" ? "agentd.start" : "agentd";
  const targetPath = join(runDirectory.path, targetName);
  const lockPath = `${targetPath}.lock`;
  let releaseLock: (() => Promise<void>) | undefined;
  try {
    releaseLock = await lockfile.lock(targetPath, {
      realpath: false,
      retries: 0,
      stale,
      update: Math.max(1_000, Math.floor(stale / 2)),
      lockfilePath: lockPath,
    });
  } catch (error) {
    if ((error as LockError).code === "ELOCKED") return null;
    throw controlPlaneError("IO_FAILURE", error);
  }

  try {
    await runDirectory.securityPolicy.protectDirectory(lockPath);
    await runDirectory.securityPolicy.verifyDirectory(lockPath);
  } catch (error) {
    await releaseLock().catch(() => undefined);
    throw controlPlaneError("IO_FAILURE", error);
  }

  let released = false;
  return Object.freeze({
    kind,
    path: lockPath,
    async release(): Promise<void> {
      if (released) return;
      released = true;
      await releaseLock().catch((error: unknown) => {
        throw controlPlaneError("IO_FAILURE", error);
      });
    },
  });
}

export function acquireStartupLock(
  runDirectory: SecureRunDirectory,
  options: LockOptions = {},
): Promise<DaemonLease | null> {
  return acquireLease(runDirectory, "startup", options);
}

export function acquireLifecycleLock(
  runDirectory: SecureRunDirectory,
  options: LockOptions = {},
): Promise<DaemonLease | null> {
  return acquireLease(runDirectory, "lifetime", options);
}
