import { describe, expect, it, vi } from "vitest";

import {
  createLocalModelService,
  type NativeInstalledFileResult,
  type NativeModelIdentity,
  type NativeModelStagingFile,
  type NativeModelStorageCapability,
} from "../src/composition.js";
import { createComposedLocalModelService } from "../src/internal/composed-service.js";
import { DEFAULT_PRESET } from "../src/internal/catalog.js";
import { DEFAULT_MODEL_IDENTITY } from "../src/internal/model-identity.js";
import type { LocalModelService } from "../src/types.js";
import { Deferred, flushTasks } from "./support/fakes.js";

describe("secure local-model composition", () => {
  it("fails closed without a native secure capability", () => {
    expect(() =>
      createLocalModelService(undefined as unknown as NativeModelStorageCapability),
    ).toThrow("secure_model_capability_required");
  });

  it("binds the fixed identity and holds the native anchor through verification and load", async () => {
    const loadCompletion = new Deferred<{ dispose(): Promise<void> }>();
    const storage = new MemoryStorage(true);
    const verifier = vi.fn(async () => {
      expect(storage.anchorHeld).toBe(true);
      return 1;
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
    loadCompletion.resolve({ dispose: () => Promise.resolve() });

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
        loadDefault: () => Promise.resolve({ dispose: () => Promise.resolve() }),
      }),
    });
    await flushTasks();

    expect(service.status().phase).toBe("setup-required");
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

  constructor(private readonly installed: boolean) {}

  acquireInstallLockAnchor(identity: NativeModelIdentity) {
    this.identities.push(identity);
    return Promise.resolve({
      withLockTarget: <T>(use: (path: string) => Promise<T>) => use("C:\\anchored\\lock"),
      release: () => Promise.resolve(),
    });
  }

  createStagingFile(
    identity: NativeModelIdentity,
    _operationId: string,
    _signal: AbortSignal,
  ): Promise<NativeModelStagingFile> {
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
    this.identities.push(identity);
    if (!this.installed) return { kind: "missing" };
    if (this.skipInstalledCallback) {
      return { kind: "present", value: true as T };
    }
    this.anchorHeld = true;
    try {
      return { kind: "present", value: await use("C:\\anchored\\Qwen.gguf") };
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
