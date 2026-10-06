import { describe, expect, it, vi } from "vitest";

import { createProxyEnvironmentManager } from "@lyonbrown/agentd/internal/proxy-environment";

describe("agentd proxy environment", () => {
  it("passes only supported proxy variables to Node", () => {
    const restore = vi.fn();
    const configure = vi.fn(() => restore);
    const manager = createProxyEnvironmentManager(configure, {
      HTTPS_PROXY: "http://127.0.0.1:7897",
      NO_PROXY: "localhost,127.0.0.1",
      SECRET_TOKEN: "must-not-cross-the-boundary",
    });

    const release = manager.acquire();

    expect(configure).toHaveBeenCalledWith({
      HTTPS_PROXY: "http://127.0.0.1:7897",
      NO_PROXY: "localhost,127.0.0.1",
    });
    release();
    expect(restore).toHaveBeenCalledOnce();
  });

  it("shares one global configuration until every runtime releases it", () => {
    const restore = vi.fn();
    const configure = vi.fn(() => restore);
    const manager = createProxyEnvironmentManager(configure, {});

    const releaseFirst = manager.acquire();
    const releaseSecond = manager.acquire();
    expect(configure).toHaveBeenCalledOnce();

    releaseFirst();
    releaseFirst();
    expect(restore).not.toHaveBeenCalled();

    releaseSecond();
    expect(restore).toHaveBeenCalledOnce();
  });

  it("maps invalid proxy configuration without retaining the original error", () => {
    const secret = "http://user:password@example.invalid";
    const manager = createProxyEnvironmentManager(
      () => {
        throw new Error(secret);
      },
      { HTTPS_PROXY: secret },
    );

    expect(() => manager.acquire()).toThrow("PROXY_CONFIGURATION_INVALID");
    try {
      manager.acquire();
    } catch (error: unknown) {
      expect(error).toBeInstanceOf(Error);
      expect((error as Error).message).not.toContain(secret);
      expect((error as Error & { cause?: unknown }).cause).toBeUndefined();
    }
  });
});
