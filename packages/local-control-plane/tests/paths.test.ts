import { describe, expect, it } from "vitest";

import { deriveScopedDaemonPaths } from "@lyonbrown/local-control-plane/internal/index";
import {
  FakePlatformSecurity,
  INODE_IDENTITY,
} from "@lyonbrown/local-control-plane/test/test-support";

describe("scoped daemon paths", () => {
  it("derives the stable workspace cache layout and runtime scope from one native identity", async () => {
    const security = new FakePlatformSecurity();
    security.setIdentity("C:\\Alias", INODE_IDENTITY, "C:\\Canonical\\Agent");

    const paths = await deriveScopedDaemonPaths(
      "C:\\Users\\alice",
      "C:\\Alias",
      "config-v1",
      security.provider,
      "win32",
    );

    expect(paths).toEqual({
      canonicalWorkspace: "C:\\Canonical\\Agent",
      workspaceCacheDirectory: "C:\\Users\\alice\\.agent\\workspaces\\c26c42ff8127d664",
      dataDirectory: "C:\\Users\\alice\\.agent\\workspaces\\c26c42ff8127d664\\data",
      runDirectory:
        "C:\\Users\\alice\\.agent\\workspaces\\c26c42ff8127d664\\run\\79df268c65204a2607a7334254c5e69c4eba3425e1fb9557ba74e8e777abcc37",
      scopeId: "79df268c65204a2607a7334254c5e69c4eba3425e1fb9557ba74e8e777abcc37",
    });
    expect(security.operations).toEqual(["identity:C:\\Alias"]);
    expect(Object.isFrozen(paths)).toBe(true);
    expect(Object.keys(paths)).not.toContain("token");
    expect(Object.keys(paths)).not.toContain("handle");
  });

  it.each(["relative", "..\\home", "C:\\bad\0home", "C:\\bad\ud800home"])(
    "rejects invalid home path %j before invoking native code",
    async (homeDirectory) => {
      const security = new FakePlatformSecurity();

      await expect(
        deriveScopedDaemonPaths(
          homeDirectory,
          "C:\\workspace",
          "config",
          security.provider,
          "win32",
        ),
      ).rejects.toBeInstanceOf(Error);
      expect(security.operations).toEqual([]);
    },
  );

  it("rejects mutable identity fields that drift while being consumed", async () => {
    const security = new FakePlatformSecurity();
    let reads = 0;
    security.identities.set("/workspace", {
      canonicalPath: "/workspace",
      identity: {
        kind: "inode",
        get deviceId() {
          reads += 1;
          return reads === 1 ? 1n : 2n;
        },
        inodeNumber: 3n,
      },
    });

    await expect(
      deriveScopedDaemonPaths("/home/alice", "/workspace", "config", security.provider, "linux"),
    ).rejects.toMatchObject({ code: "PLATFORM_SECURITY_FAILURE" });
    expect(security.operations).toEqual(["identity:/workspace"]);
  });

  it("rejects invalid configuration before invoking native code", async () => {
    const security = new FakePlatformSecurity();

    await expect(
      deriveScopedDaemonPaths(
        "/home/alice",
        "/workspace",
        "bad\0config",
        security.provider,
        "linux",
      ),
    ).rejects.toMatchObject({ code: "INVALID_CONFIG_FINGERPRINT" });
    expect(security.operations).toEqual([]);
  });
});
