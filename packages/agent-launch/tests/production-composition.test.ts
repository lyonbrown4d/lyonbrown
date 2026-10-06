import { tmpdir } from "node:os";
import { createNativeControlPlaneProvider } from "@lyonbrown/local-control-plane/native-adapter";
import { describe, expect, test } from "vitest";
import {
  AbortableSleeper,
  MonotonicClock,
  createProductionAgentLauncher,
} from "@lyonbrown/agent-launch/internal/production";

function nativeProvider() {
  return createNativeControlPlaneProvider({
    contract:
      process.platform === "win32"
        ? {
            platform: "windows",
            directoryOpen: "FILE_FLAG_OPEN_REPARSE_POINT",
            ownerProtection: "current-user-only-dacl",
            workspaceIdentity: "native-file-id-v1",
            kernelLock: "kernel-file-lock-v1",
          }
        : {
            platform: "posix",
            directoryOpen: "dirfd-openat-O_NOFOLLOW",
            ownerProtection: "current-uid-owner-mode",
            workspaceIdentity: "native-dev-ino-v1",
            kernelLock: "kernel-file-lock-v1",
          },
    workspaceIdentity: (path) =>
      Promise.resolve({
        ok: true,
        value: {
          canonicalPath: path,
          identity: { kind: "inode", deviceId: 1n, inodeNumber: 2n },
        },
      }),
    openSecureRunDirectory: () => Promise.resolve({ ok: false, code: "not_found" }),
  });
}

describe("production launcher composition", () => {
  test("constructs a client-only launcher without exposing its native capabilities", () => {
    const launcher = createProductionAgentLauncher({
      nativeProvider: nativeProvider(),
      homeDirectory: tmpdir(),
      workspace: process.platform === "win32" ? "C:\\workspace" : "/workspace",
      effectiveConfigFingerprint: "production",
      agentd: { command: process.execPath, arguments: ["agentd.js"] },
      client: { name: "agent-cli", version: "1.0.0" },
    });

    expect(Object.keys(launcher)).toEqual([]);
    expect(JSON.stringify(launcher)).not.toMatch(/nativeProvider|agentd\.js/u);
  });

  test("uses a monotonic system clock", () => {
    const clock = new MonotonicClock();
    const first = clock.now();
    const second = clock.now();

    expect(Number.isFinite(first)).toBe(true);
    expect(second).toBeGreaterThanOrEqual(first);
  });

  test("cancels an abortable sleep promptly", async () => {
    const controller = new AbortController();
    controller.abort();

    await expect(new AbortableSleeper().sleep(60_000, controller.signal)).rejects.toMatchObject({
      name: "AbortError",
    });
  });
});
