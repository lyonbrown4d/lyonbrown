import { spawnSync } from "node:child_process";
import { mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import type { PlatformResult } from "@lyonbrown/local-control-plane/native-adapter";
import { createPosixSecurity } from "@lyonbrown/control-plane-native/internal/posix-provider";

function unwrap<T>(result: PlatformResult<T>): T {
  if (!result.ok) throw new Error(`unexpected platform failure: ${result.code}`);
  return result.value;
}

function childAttempt(path: string): boolean {
  const script = `
    import { createPosixSecurity } from "@lyonbrown/control-plane-native/internal/posix-provider";
    const directory = await createPosixSecurity().openSecureRunDirectory(process.argv[1]);
    if (!directory.ok) throw new Error(directory.code);
    const lock = await directory.value.tryAcquireKernelLock("agentd.lock");
    if (!lock.ok) throw new Error(lock.code);
    console.log(JSON.stringify(lock.value !== null));
    if (lock.value) await lock.value.release();
    await directory.value.close();
  `;
  const result = spawnSync(
    process.execPath,
    ["--import", "tsx", "--input-type=module", "-e", script, path],
    { encoding: "utf8", timeout: 15_000 },
  );
  expect(result.error).toBeUndefined();
  expect(result.stderr).toBe("");
  expect(result.status).toBe(0);
  return JSON.parse(result.stdout.trim()) as boolean;
}

describe("POSIX kernel lock process boundary", () => {
  it("excludes separate processes, retains locks across directory close, and releases once", async () => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), "lyonbrown-posix-lock-")));
    const path = join(root, "run");
    const first = unwrap(await createPosixSecurity().openSecureRunDirectory(path));
    const second = unwrap(await createPosixSecurity().openSecureRunDirectory(path));
    const lock = unwrap(await first.tryAcquireKernelLock("agentd.lock"));
    expect(lock).not.toBeNull();
    try {
      expect(unwrap(await second.tryAcquireKernelLock("agentd.lock"))).toBeNull();
      expect(childAttempt(path)).toBe(false);
      unwrap(await first.close());
      expect(childAttempt(path)).toBe(false);
      if (!lock) throw new Error("lock required");
      unwrap(await lock.release());
      const replacement = unwrap(await second.tryAcquireKernelLock("agentd.lock"));
      expect(replacement).not.toBeNull();
      unwrap(await lock.release());
      expect(childAttempt(path)).toBe(false);
      if (!replacement) throw new Error("replacement lock required");
      unwrap(await replacement.release());
      expect(childAttempt(path)).toBe(true);
    } finally {
      await lock?.release();
      await first.close();
      await second.close();
      rmSync(root, { recursive: true, force: true });
    }
  });
});
