import { mkdir, utimes } from "node:fs/promises";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { acquireLifecycleLock, acquireStartupLock, openSecureRunDirectory } from "../src/index.js";
import { RecordingWindowsAclProtector, withTemporaryDirectory } from "./test-support.js";

describe("daemon leases", () => {
  it("allows exactly one concurrent first-start winner", async () => {
    await withTemporaryDirectory(async (root) => {
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: new RecordingWindowsAclProtector(),
      });
      const [first, second] = await Promise.all([
        acquireStartupLock(runDirectory),
        acquireStartupLock(runDirectory),
      ]);
      const winners = [first, second].filter((lease) => lease !== null);

      expect(winners).toHaveLength(1);
      await winners[0]?.release();
      await winners[0]?.release();
    });
  });

  it("keeps startup and lifetime ownership independent", async () => {
    await withTemporaryDirectory(async (root) => {
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: new RecordingWindowsAclProtector(),
      });
      const startup = await acquireStartupLock(runDirectory);
      const lifetime = await acquireLifecycleLock(runDirectory);

      expect(startup).not.toBeNull();
      expect(lifetime).not.toBeNull();
      expect(await acquireStartupLock(runDirectory)).toBeNull();
      expect(await acquireLifecycleLock(runDirectory)).toBeNull();
      await startup?.release();
      await lifetime?.release();
    });
  });

  it("recovers a stale lease without invoking process-kill behavior", async () => {
    await withTemporaryDirectory(async (root) => {
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: new RecordingWindowsAclProtector(),
      });
      const stalePath = join(runDirectory.path, "agentd.start.lock");
      await mkdir(stalePath);
      const old = new Date(Date.now() - 60_000);
      await utimes(stalePath, old, old);

      const lease = await acquireStartupLock(runDirectory, { staleMs: 5_000 });

      expect(lease).not.toBeNull();
      await lease?.release();
    });
  });

  it("releases the underlying lock if owner-only protection fails", async () => {
    await withTemporaryDirectory(async (root) => {
      const acl = new RecordingWindowsAclProtector();
      const runDirectory = await openSecureRunDirectory(join(root, "run"), {
        platform: "win32",
        windowsAclProtector: acl,
      });
      acl.protectDirectory = (path: string): Promise<void> => {
        return path.endsWith(".lock")
          ? Promise.reject(new Error("acl failure"))
          : Promise.resolve();
      };

      await expect(acquireStartupLock(runDirectory)).rejects.toMatchObject({ code: "IO_FAILURE" });

      acl.protectDirectory = (): Promise<void> => Promise.resolve();
      const retry = await acquireStartupLock(runDirectory);
      expect(retry).not.toBeNull();
      await retry?.release();
    });
  });
});
