import { describe, expect, it } from "vitest";

import type { LocalModelPhase, LocalModelService } from "../src/index.ts";
import { DEFAULT_PRESET } from "../src/internal/catalog.ts";
import { createLocalModelManager } from "../src/internal/manager.ts";
import { artifact, Deferred, FakeLlamaEngine, FakeModelIo, flushTasks } from "./support/fakes.ts";

describe("explicit background download lifecycle", () => {
  it("starts only on command and publishes bounded progress", async () => {
    const io = new FakeModelIo();
    const service = await createMissingService(io, () => "op-1");
    const observed: string[] = [];
    service.subscribe((status) => observed.push(status.phase));

    expect(service.startDownload()).toEqual({ kind: "started", operationId: "op-1" });
    expect(service.status()).toMatchObject({ phase: "waiting-for-lock", operationId: "op-1" });
    await flushTasks();
    expect(service.status()).toMatchObject({ phase: "downloading", operationId: "op-1" });
    io.downloads[0]?.onProgress({ downloadedBytes: 45, totalBytes: 100 });
    expect(service.status()).toMatchObject({ downloadedBytes: 45, totalBytes: 100 });

    io.downloads[0]?.completion.resolve(artifact("op-1"));
    await flushTasks();

    expect(service.status()).toMatchObject({ phase: "ready", operationId: null });
    expect(observed).toEqual([
      "waiting-for-lock",
      "downloading",
      "downloading",
      "verifying",
      "installing",
      "ready",
    ]);
    expect(io.verifyCalls).toBe(1);
    expect(io.activateCalls).toBe(1);
  });

  it("coalesces repeated starts while an operation is active", async () => {
    const io = new FakeModelIo();
    const service = await createMissingService(io, () => "op-1");

    service.startDownload();

    expect(service.startDownload()).toEqual({ kind: "in-progress", operationId: "op-1" });
    await waitForPhase(service, "downloading");
    expect(io.downloadCalls).toBe(1);
  });

  it("aborts invalid progress, fails closed, and isolates observers", async () => {
    const io = new FakeModelIo();
    const service = await createMissingService(io, () => "op-1");
    const unsubscribe = service.subscribe(() => {
      throw new Error("observer failure");
    });
    service.startDownload();
    await waitForPhase(service, "downloading");
    io.downloads[0]?.onProgress({ downloadedBytes: -1, totalBytes: 100 });
    expect(io.downloads[0]?.signal.aborted).toBe(true);
    unsubscribe();
    io.downloads[0]?.completion.resolve(artifact("op-1"));
    await waitForPhase(service, "failed");
    io.downloads[0]?.onProgress({ downloadedBytes: 50, totalBytes: 100 });
    expect(service.status().phase).toBe("failed");
    expect(service.cancelDownload("op-1")).toEqual({ kind: "no-active-operation" });
  });

  it("keeps a manager-owned hard limit when a malicious I/O adapter bypasses its boundary", async () => {
    const io = new FakeModelIo();
    const service = await createMissingService(io, () => "op-malicious");
    service.startDownload();
    await waitForPhase(service, "downloading");

    io.downloads[0]?.onProgress({
      downloadedBytes: DEFAULT_PRESET.maxBytes + 1,
      totalBytes: DEFAULT_PRESET.maxBytes + 1,
    });

    expect(io.downloads[0]?.signal.aborted).toBe(true);
    io.downloads[0]?.completion.resolve(artifact("op-malicious"));
    await waitForPhase(service, "failed");
    expect(io.activateCalls).toBe(0);
  });

  it("cancels a matching operation before installation", async () => {
    const io = new FakeModelIo();
    const service = await createMissingService(io, () => "op-1");
    service.startDownload();
    await waitForPhase(service, "downloading");

    expect(service.cancelDownload("op-1")).toEqual({ kind: "cancelled" });
    expect(io.downloads[0]?.signal.aborted).toBe(true);
    io.downloads[0]?.completion.reject(new Error("secret transport failure"));
    await flushTasks();

    expect(service.status()).toMatchObject({
      phase: "cancelled",
      failure: null,
      allowedActions: ["retry"],
    });
  });

  it("rejects stale cancellation so an old operation cannot cancel a newer one", async () => {
    const ids = ["op-old", "op-new"];
    const io = new FakeModelIo();
    const service = await createMissingService(io, () => ids.shift() ?? "bad");
    service.startDownload();
    await waitForPhase(service, "downloading");
    service.cancelDownload("op-old");
    io.downloads[0]?.completion.reject(new Error("cancelled"));
    await waitForPhase(service, "cancelled");
    service.startDownload();
    await waitForPhase(service, "downloading");

    expect(service.cancelDownload("op-old")).toEqual({
      kind: "operation-mismatch",
      activeOperationId: "op-new",
    });
    expect(io.downloads[1]?.signal.aborted).toBe(false);
  });

  it("makes cancellation too late before atomic activation", async () => {
    const io = new FakeModelIo();
    let releaseActivation: (() => void) | undefined;
    io.activate = async () => {
      io.activateCalls += 1;
      await new Promise<void>((resolve) => {
        releaseActivation = resolve;
      });
      io.installed = true;
    };
    const service = await createMissingService(io, () => "op-1");
    service.startDownload();
    await waitForPhase(service, "downloading");
    io.downloads[0]?.completion.resolve(artifact("op-1"));
    await waitForPhase(service, "installing");

    expect(service.status().phase).toBe("installing");
    expect(service.cancelDownload("op-1")).toEqual({ kind: "too-late" });
    releaseActivation?.();
    await flushTasks();
    expect(service.status().phase).toBe("ready");
  });

  it("sanitizes failures and allows a fresh retry", async () => {
    const ids = ["op-bad", "op-good"];
    const io = new FakeModelIo();
    io.verify = () => {
      io.verifyCalls += 1;
      return Promise.reject(new Error("checksum mismatch at C:\\Users\\alice\\private"));
    };
    const service = await createMissingService(io, () => ids.shift() ?? "bad");
    service.startDownload();
    await waitForPhase(service, "downloading");
    io.downloads[0]?.completion.resolve(artifact("op-bad"));
    await waitForPhase(service, "failed");

    expect(service.status()).toMatchObject({
      phase: "failed",
      operationId: "op-bad",
      failure: { code: "download_failed", retryable: true },
    });
    expect(JSON.stringify(service.status())).not.toContain("alice");
    expect(service.startDownload()).toEqual({ kind: "started", operationId: "op-good" });
  });

  it("dispose aborts and joins an active background download", async () => {
    const io = new FakeModelIo();
    const service = await createMissingService(io, () => "op-1");
    service.startDownload();
    await waitForPhase(service, "downloading");

    const disposed = service.dispose();
    expect(io.downloads[0]?.signal.aborted).toBe(true);
    io.downloads[0]?.completion.reject(new Error("cancelled"));
    await expect(disposed).resolves.toBeUndefined();
    expect(service.startDownload()).toEqual({ kind: "disposed" });
  });

  it("joins an older operation when a failure listener synchronously retries", async () => {
    const ids = ["op-old", "op-new"];
    const oldCleanup = new Deferred<void>();
    const oldRelease = new Deferred<void>();
    const io = new FakeModelIo();
    io.verify = (downloadedArtifact) => {
      io.verifyCalls += 1;
      return downloadedArtifact.operationId === "op-old"
        ? Promise.reject(new Error("first attempt failed"))
        : Promise.resolve(111);
    };
    io.cleanup = async (downloadedArtifact) => {
      io.cleanupCalls += 1;
      if (downloadedArtifact?.operationId === "op-old") await oldCleanup.promise;
    };
    let lockSequence = 0;
    io.acquireInstallLock = () => {
      io.lockCalls += 1;
      lockSequence += 1;
      const sequence = lockSequence;
      return Promise.resolve({
        release: async () => {
          io.lockReleaseCalls += 1;
          if (sequence === 1) await oldRelease.promise;
        },
      });
    };
    const service = await createMissingService(io, () => ids.shift() ?? "unexpected");
    service.subscribe((status) => {
      if (status.phase === "failed") service.startDownload();
    });

    service.startDownload();
    await waitForPhase(service, "downloading");
    io.downloads[0]?.completion.resolve(artifact("op-old"));
    await waitForOperation(service, "op-new");
    expect(service.status().operationId).toBe("op-new");

    const disposal = service.dispose();
    io.downloads[1]?.completion.reject(new Error("cancelled"));
    let disposalFinished = false;
    void disposal.then(() => {
      disposalFinished = true;
    });
    await flushTasks();
    expect(disposalFinished).toBe(false);

    oldCleanup.resolve();
    await flushTasks();
    expect(disposalFinished).toBe(false);
    oldRelease.resolve();
    await expect(disposal).resolves.toBeUndefined();
  });

  it("rechecks the verified cache after acquiring the process lock", async () => {
    const io = new FakeModelIo();
    const service = await createMissingService(io, () => "op-1");
    io.installed = true;

    expect(service.startDownload()).toEqual({ kind: "started", operationId: "op-1" });
    await waitForPhase(service, "ready");

    expect(service.status().operationId).toBeNull();
    expect(io.lockCalls).toBe(1);
    expect(io.lockReleaseCalls).toBe(1);
    expect(io.downloadCalls).toBe(0);
  });
});

const waitForPhase = async (
  service: LocalModelService,
  expected: LocalModelPhase,
): Promise<void> => {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (service.status().phase === expected) return;
    await flushTasks();
  }
  throw new Error(`phase did not reach ${expected}`);
};

const waitForOperation = async (service: LocalModelService, operationId: string): Promise<void> => {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (service.status().operationId === operationId) return;
    await flushTasks();
  }
  throw new Error(`operation did not reach ${operationId}`);
};

const createMissingService = async (
  io: FakeModelIo,
  operationId: () => string,
): Promise<LocalModelService> => {
  const service = createLocalModelManager(io, new FakeLlamaEngine(), operationId);
  await waitForPhase(service, "setup-required");
  return service;
};
