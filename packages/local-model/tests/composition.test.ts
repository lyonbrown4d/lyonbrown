import { describe, expect, it, vi } from "vitest";

import {
  createLocalModelService,
  type NativeInstalledFileResult,
  type NativeModelIdentity,
  type NativeModelStagingFile,
  type NativeModelStorageCapability,
} from "../src/composition.ts";
import { createComposedLocalModelService } from "../src/internal/composed-service.ts";
import { DEFAULT_PRESET } from "../src/internal/catalog.ts";
import { DEFAULT_MODEL_IDENTITY } from "../src/internal/model-identity.ts";
import type { LocalModelLease, LocalModelService } from "../src/types.ts";
import { createFakeLlamaLease, Deferred, flushTasks } from "./support/fakes.ts";

describe("secure local-model composition", () => {
  it("fails closed without a native secure capability", () => {
    expect(() =>
      createLocalModelService(undefined as unknown as NativeModelStorageCapability),
    ).toThrow("secure_model_capability_required");
  });

  it("binds the fixed identity and holds the native anchor through verification and load", async () => {
    const loadCompletion = new Deferred<LocalModelLease>();
    const storage = new MemoryStorage(true);
    const verifier = vi.fn(() => {
      expect(storage.anchorHeld).toBe(true);
      return Promise.resolve(1);
    });
    const nodeLlamaEngine = vi.fn(() => ({
      loadDefault: () => {
        expect(storage.anchorHeld).toBe(true);
        return loadCompletion.promise;
      },
    }));
    const service = createComposedLocalModelService(storage, {
      verifyFile: verifier,
      nodeLlamaEngine,
    });
    await flushTasks();

    const result = service.load();
    expect(storage.anchorHeld).toBe(true);
    loadCompletion.resolve(createFakeLlamaLease());

    await expect(result).resolves.toMatchObject({ kind: "ready" });
    expect(storage.anchorHeld).toBe(false);
    expect(storage.identities).toEqual([DEFAULT_MODEL_IDENTITY, DEFAULT_MODEL_IDENTITY]);
    expect(nodeLlamaEngine).toHaveBeenCalledWith("C:\\anchored\\Qwen.gguf");
  });

  it("fails closed when a provider claims present without invoking the anchored callback", async () => {
    const storage = new MemoryStorage(true);
    storage.skipInstalledCallback = true;
    const service = createComposedLocalModelService(storage, {
      verifyFile: () => Promise.resolve(1),
      nodeLlamaEngine: () => ({
        loadDefault: () => Promise.resolve(createFakeLlamaLease()),
      }),
    });
    await flushTasks();

    expect(service.status().phase).toBe("setup-required");
  });

  it("disposes a loaded lease when installed-anchor teardown rejects", async () => {
    const storage = new MemoryStorage(true);
    storage.rejectAfterCallbackOnAccess = 2;
    const dispose = vi.fn(() => Promise.resolve());
    const service = createComposedLocalModelService(storage, {
      verifyFile: () => Promise.resolve(1),
      nodeLlamaEngine: () => ({
        loadDefault: () => Promise.resolve(createFakeLlamaLease(dispose)),
      }),
    });
    await flushTasks();

    await expect(service.load()).resolves.toEqual({
      kind: "unavailable",
      code: "native_runtime_unavailable",
    });
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("invokes the load callback exactly once and preserves the first lease", async () => {
    const storage = new MemoryStorage(true);
    storage.duplicateCallbackOnAccess = 2;
    const dispose = vi.fn(() => Promise.resolve());
    const nodeLlamaEngine = vi.fn(() => ({
      loadDefault: () => Promise.resolve(createFakeLlamaLease(dispose)),
    }));
    const service = createComposedLocalModelService(storage, {
      verifyFile: () => Promise.resolve(1),
      nodeLlamaEngine,
    });
    await flushTasks();

    await expect(service.load()).resolves.toEqual({
      kind: "unavailable",
      code: "native_runtime_unavailable",
    });
    expect(nodeLlamaEngine).toHaveBeenCalledOnce();
    expect(dispose).toHaveBeenCalledOnce();
  });

  it("redacts disposal failures after installed-anchor teardown rejects", async () => {
    const storage = new MemoryStorage(true);
    storage.rejectAfterCallbackOnAccess = 2;
    const dispose = vi.fn(() => Promise.reject(new Error("C:\\private\\native lease")));
    const service = createComposedLocalModelService(storage, {
      verifyFile: () => Promise.resolve(1),
      nodeLlamaEngine: () => ({
        loadDefault: () => Promise.resolve(createFakeLlamaLease(dispose)),
      }),
    });
    await flushTasks();

    const result = await service.load();

    expect(result).toEqual({ kind: "unavailable", code: "native_runtime_unavailable" });
    expect(JSON.stringify(result)).not.toContain("private");
    expect(dispose).toHaveBeenCalledOnce();
    await flushTasks();
  });

  it("aborts at both downloader and manager boundaries on invalid progress", async () => {
    const storage = new MemoryStorage(false);
    let signalWasAborted = false;
    const service = createComposedLocalModelService(storage, {
      downloaderApi: {
        createModelDownloader: (options) =>
          Promise.resolve({
            download: ({ signal } = {}) => {
              options.onProgress({
                downloadedSize: DEFAULT_PRESET.maxBytes + 1,
                totalSize: DEFAULT_PRESET.maxBytes + 1,
              });
              signalWasAborted = signal?.aborted === true;
              return Promise.reject(new Error("stopped"));
            },
          }),
      },
      verifyFile: () => Promise.resolve(1),
      nodeLlamaEngine: vi.fn(),
      lockProcess: () => Promise.resolve(() => Promise.resolve()),
    });
    await flushTasks();

    service.startDownload();
    await waitForPhase(service, "failed");

    expect(signalWasAborted).toBe(true);
    expect(storage.activateCalls).toBe(0);
  });

  it.each([-1, 1.5, Number.NaN, DEFAULT_PRESET.maxBytes + 1])(
    "rejects invalid verified byte count %s before activation",
    async (verifiedBytes) => {
      const storage = new MemoryStorage(false);
      const service = createComposedLocalModelService(storage, {
        downloaderApi: successfulDownloader(),
        verifyFile: () => Promise.resolve(verifiedBytes),
        nodeLlamaEngine: vi.fn(),
        lockProcess: () => Promise.resolve(() => Promise.resolve()),
      });
      await flushTasks();

      service.startDownload();
      await waitForPhase(service, "failed");

      expect(storage.activateCalls).toBe(0);
    },
  );
});

class MemoryStorage implements NativeModelStorageCapability {
  readonly identities: NativeModelIdentity[] = [];
  anchorHeld = false;
  activateCalls = 0;
  skipInstalledCallback = false;
  rejectAfterCallbackOnAccess: number | undefined;
  duplicateCallbackOnAccess: number | undefined;
  private installedAccesses = 0;

  constructor(private readonly installed: boolean) {}

  acquireInstallLockAnchor(identity: NativeModelIdentity) {
    this.identities.push(identity);
    return Promise.resolve({
      withLockTarget: <T>(use: (path: string) => Promise<T>) => use("C:\\anchored\\lock"),
      release: () => Promise.resolve(),
    });
  }

  createStagingFile(identity: NativeModelIdentity): Promise<NativeModelStagingFile> {
    this.identities.push(identity);
    return Promise.resolve({
      withFilePath: (use) => use("C:\\anchored\\staging\\Qwen.gguf", "C:\\anchored\\staging"),
      activate: () => {
        this.activateCalls += 1;
        return Promise.resolve();
      },
      cleanup: () => Promise.resolve(),
    });
  }

  async withInstalledFile<T>(
    identity: NativeModelIdentity,
    _signal: AbortSignal,
    use: (filePath: string) => Promise<T>,
  ): Promise<NativeInstalledFileResult<T>> {
    void _signal;
    this.identities.push(identity);
    this.installedAccesses += 1;
    if (!this.installed) return { kind: "missing" };
    if (this.skipInstalledCallback) {
      return { kind: "present", value: true as T };
    }
    this.anchorHeld = true;
    try {
      const value = await use("C:\\anchored\\Qwen.gguf");
      if (this.duplicateCallbackOnAccess === this.installedAccesses) {
        try {
          await use("C:\\anchored\\Qwen.gguf");
        } catch {
          // The provider violation is intentionally swallowed to test caller-side fencing.
        }
      }
      if (this.rejectAfterCallbackOnAccess === this.installedAccesses) {
        throw new Error("C:\\private\\anchor teardown failed");
      }
      return { kind: "present", value };
    } finally {
      this.anchorHeld = false;
    }
  }
}

const successfulDownloader = () => ({
  createModelDownloader: (options: { dirPath: string; fileName: string }) =>
    Promise.resolve({
      download: () => Promise.resolve(`${options.dirPath}\\${options.fileName}`),
    }),
});

const waitForPhase = async (
  service: LocalModelService,
  phase: ReturnType<LocalModelService["status"]>["phase"],
): Promise<void> => {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (service.status().phase === phase) return;
    await flushTasks();
  }
  throw new Error(`phase did not reach ${phase}`);
};
