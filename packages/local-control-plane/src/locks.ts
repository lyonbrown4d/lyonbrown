import {
  invokePlatform,
  type KernelFileLock,
  type SecureDirectoryHandle,
} from "./platform-security.js";

export interface DaemonLease {
  readonly kind: "startup" | "lifetime";
  release(): Promise<void>;
}

async function acquireLease(
  runDirectory: SecureDirectoryHandle,
  kind: DaemonLease["kind"],
): Promise<DaemonLease | null> {
  const fileName = kind === "startup" ? "agentd.start.lock" : "agentd.lock";
  const lock = await invokePlatform("kernel_lock", () =>
    runDirectory.tryAcquireKernelLock(fileName),
  );
  if (lock === null) return null;

  let released = false;
  return Object.freeze({
    kind,
    async release(): Promise<void> {
      if (released) return;
      released = true;
      await releaseKernelLock(lock);
    },
  });
}

async function releaseKernelLock(lock: KernelFileLock): Promise<void> {
  await invokePlatform("release_lock", () => lock.release());
}

export function acquireStartupLock(
  runDirectory: SecureDirectoryHandle,
): Promise<DaemonLease | null> {
  return acquireLease(runDirectory, "startup");
}

export function acquireLifecycleLock(
  runDirectory: SecureDirectoryHandle,
): Promise<DaemonLease | null> {
  return acquireLease(runDirectory, "lifetime");
}
