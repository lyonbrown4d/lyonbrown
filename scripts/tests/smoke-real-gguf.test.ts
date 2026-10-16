import { describe, expect, test, vi } from "vitest";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  assertProgress,
  assertSafeTemporaryRoot,
  bounded,
  cleanupTemporaryAfterClose,
  parseRealGgufSmokeAuthorization,
  runAllAttemptCleanup,
  type ProgressState,
} from "@lyonbrown/workspace/internal/smoke-real-gguf-lib";
import { verifyInstalledModelCache } from "@lyonbrown/workspace/internal/smoke-real-gguf-cache";

describe("real GGUF smoke authorization", () => {
  test("requires both the environment gate and confirmation flag", () => {
    expect(() => parseRealGgufSmokeAuthorization({}, [])).toThrow("REAL_GGUF_SMOKE_DISABLED");
    expect(() => parseRealGgufSmokeAuthorization({ LYONBROWN_REAL_GGUF_SMOKE: "1" }, [])).toThrow(
      "REAL_GGUF_DOWNLOAD_NOT_CONFIRMED",
    );
    expect(() => parseRealGgufSmokeAuthorization({}, ["--confirm-download"])).toThrow(
      "REAL_GGUF_SMOKE_DISABLED",
    );
  });

  test.each(["win32", "darwin", "linux"] as const)(
    "accepts exact opt-in values on %s",
    (platform) => {
      expect(
        parseRealGgufSmokeAuthorization(
          { LYONBROWN_REAL_GGUF_SMOKE: "1" },
          ["--confirm-download"],
          platform,
        ),
      ).toEqual({ confirmed: true });
      expect(() =>
        parseRealGgufSmokeAuthorization(
          { LYONBROWN_REAL_GGUF_SMOKE: "true" },
          ["--confirm-download"],
          platform,
        ),
      ).toThrow("REAL_GGUF_SMOKE_DISABLED");
    },
  );
});

describe("real GGUF progress validation", () => {
  const initial: ProgressState = {
    operationId: "download-2",
    downloadedBytes: 0n,
    totalBytes: null,
    maxBytes: 2_000n,
  };

  test("accepts monotonic bounded progress", () => {
    expect(assertProgress(initial, 100n, 1_000n)).toEqual({
      ...initial,
      downloadedBytes: 100n,
      totalBytes: 1_000n,
    });
  });

  test.each([
    [99n, 1_000n],
    [101n, 99n],
    [101n, 2_001n],
  ])("rejects invalid progress %s/%s", (downloadedBytes, totalBytes) => {
    const previous = { ...initial, downloadedBytes: 100n, totalBytes: 1_000n };
    expect(() => assertProgress(previous, downloadedBytes, totalBytes)).toThrow(
      "REAL_GGUF_PROGRESS_INVALID",
    );
  });
});

describe("real GGUF cleanup", () => {
  test("keeps a referenced deadline until an unresolved operation times out", async () => {
    const stalled = new Promise<void>((resolve) => {
      void resolve;
    });
    await expect(bounded("stalled-close", 5, stalled)).rejects.toThrow(
      "REAL_GGUF_TIMEOUT:stalled-close",
    );
  });

  test("preserves temporary data when close has not settled successfully", async () => {
    const remove = vi.fn(() => Promise.resolve());
    await expect(
      cleanupTemporaryAfterClose(() => Promise.reject(new Error("close timeout")), remove),
    ).rejects.toThrow("close timeout");
    expect(remove).not.toHaveBeenCalled();
  });

  test("removes temporary data only after close succeeds", async () => {
    const calls: string[] = [];
    await cleanupTemporaryAfterClose(
      () => {
        calls.push("close");
        return Promise.resolve();
      },
      () => {
        calls.push("remove");
        return Promise.resolve();
      },
    );
    expect(calls).toEqual(["close", "remove"]);
  });

  test("accepts only the dedicated real-GGUF temporary directory", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-real-gguf-"));
    try {
      await expect(assertSafeTemporaryRoot(root)).resolves.toBeUndefined();
      await expect(assertSafeTemporaryRoot(tmpdir())).rejects.toThrow(
        "REAL_GGUF_CLEANUP_ROOT_INVALID",
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  test("attempts every cleanup and aggregates failures", async () => {
    const calls: string[] = [];
    const cleanup = [
      () => {
        calls.push("client");
        return Promise.reject(new Error("client failed"));
      },
      () => {
        calls.push("server");
        return Promise.resolve();
      },
      () => {
        calls.push("directory");
        return Promise.reject(new Error("directory failed"));
      },
    ];

    const failure = await runAllAttemptCleanup(cleanup).catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(AggregateError);
    expect((failure as AggregateError).errors).toHaveLength(2);
    expect(calls).toEqual(["client", "server", "directory"]);
  });

  test("does nothing for an empty cleanup list", async () => {
    await expect(runAllAttemptCleanup([])).resolves.toBeUndefined();
    expect(vi.fn()).not.toHaveBeenCalled();
  });
});

describe("installed model cache verification", () => {
  test("verifies a fixed digest and rejects staging residue", async () => {
    const root = await mkdtemp(join(tmpdir(), "lyonbrown-smoke-unit-"));
    const models = join(root, "models");
    await mkdir(models);
    await writeFile(join(models, "model-fixed.gguf"), "verified fixture");
    const digest = "f9adb7d924ed98c558040c910600d7363d749e7d20e8d355626edd53b4fb929f";
    try {
      await expect(verifyInstalledModelCache(models, digest)).resolves.toBeUndefined();
      await mkdir(join(models, ".staging-residue"));
      await expect(verifyInstalledModelCache(models, digest)).rejects.toThrow(
        "REAL_GGUF_STAGING_RESIDUE",
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});
