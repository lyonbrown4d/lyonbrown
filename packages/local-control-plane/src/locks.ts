import { invokePlatform, type KernelFileLock } from "./platform-security.ts";
import { resolveRunDirectoryHandle, type SecureRunDirectory } from "./run-directory.ts";

export interface DaemonLease {
  readonly kind: "startup" | "lifetime";
  release(): Promise<void>;
}

async function acquireLease(
  runDirectory: SecureRunDirectory,
  kind: DaemonLease["kind"],
): Promise<DaemonLease | null> {
  const fileName = kind === "startup" ? "agentd.start.lock" : "agentd.lock";
  const handle = resolveRunDirectoryHandle(runDirectory);
  const lock = await invokePlatform("kernel_lock", () => handle.tryAcquireKernelLock(fileName));
  if (lock === null) return null;

  let releasePromise: Promise<void> | undefined;
  return Object.freeze({
    kind,
    release(): Promise<void> {
      releasePromise ??= releaseKernelLock(lock);
      return releasePromise;
    },
  });
}

async function releaseKernelLock(lock: KernelFileLock): Promise<void> {
  await invokePlatform("release_lock", () => lock.release());
}

export function acquireStartupLock(runDirectory: SecureRunDirectory): Promise<DaemonLease | null> {
  return acquireLease(runDirectory, "startup");
}

export function acquireLifecycleLock(
  runDirectory: SecureRunDirectory,
): Promise<DaemonLease | null> {
  return acquireLease(runDirectory, "lifetime");
}
