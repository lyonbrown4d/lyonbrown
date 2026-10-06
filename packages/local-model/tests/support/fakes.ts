import type {
  DownloadArtifact,
  DownloadProgress,
  LocalModelIo,
  LlamaEngineAdapter,
  LlamaLease,
  ModelPreset,
} from "@lyonbrown/local-model/internal/internal/contracts";
import type { LocalModelInferenceEvent } from "@lyonbrown/local-model/internal/types";

export const createFakeLlamaLease = (
  dispose: () => Promise<void> = () => Promise.resolve(),
): LlamaLease => ({
  stream(): AsyncIterable<LocalModelInferenceEvent> {
    return {
      [Symbol.asyncIterator](): AsyncIterator<LocalModelInferenceEvent> {
        return {
          next: () => Promise.resolve({ value: undefined, done: true }),
        };
      },
    };
  },
  dispose,
});

export class Deferred<T> {
  readonly promise: Promise<T>;
  private resolvePromise!: (value: T) => void;
  private rejectPromise!: (reason?: unknown) => void;

  constructor() {
    this.promise = new Promise<T>((resolve, reject) => {
      this.resolvePromise = resolve;
      this.rejectPromise = reject;
    });
  }

  resolve(value: T): void {
    this.resolvePromise(value);
  }

  reject(reason: unknown): void {
    this.rejectPromise(reason);
  }
}

export class FakeModelIo implements LocalModelIo {
  installed = false;
  inspectCalls = 0;
  downloadCalls = 0;
  verifyCalls = 0;
  activateCalls = 0;
  cleanupCalls = 0;
  lockCalls = 0;
  lockReleaseCalls = 0;
  readonly inspectedPresetIds: string[] = [];
  readonly downloadedPresetIds: string[] = [];
  readonly installedPresetIds = new Set<string>();
  inspection: Deferred<boolean> | undefined;
  readonly downloads: {
    operationId: string;
    signal: AbortSignal;
    onProgress: (progress: DownloadProgress) => void;
    completion: Deferred<DownloadArtifact>;
  }[] = [];

  inspectInstalled(preset: ModelPreset, signal: AbortSignal): Promise<boolean> {
    this.inspectedPresetIds.push(preset.id);
    this.inspectCalls += 1;
    if (this.inspection !== undefined) {
      const inspection = this.inspection;
      signal.addEventListener(
        "abort",
        () => {
          inspection.reject(new Error("inspection cancelled"));
        },
        { once: true },
      );
      return inspection.promise;
    }
    return Promise.resolve(this.installed || this.installedPresetIds.has(preset.id));
  }

  acquireInstallLock(preset: ModelPreset, signal: AbortSignal) {
    void preset;
    void signal;
    this.lockCalls += 1;
    return Promise.resolve({
      release: () => {
        this.lockReleaseCalls += 1;
        return Promise.resolve();
      },
    });
  }

  download(
    preset: ModelPreset,
    request: {
      operationId: string;
      signal: AbortSignal;
      onProgress: (progress: DownloadProgress) => void;
    },
  ): Promise<DownloadArtifact> {
    this.downloadedPresetIds.push(preset.id);
    this.downloadCalls += 1;
    const completion = new Deferred<DownloadArtifact>();
    this.downloads.push({ ...request, completion });
    return completion.promise;
  }

  verify(artifact: DownloadArtifact, preset: ModelPreset, signal: AbortSignal): Promise<number> {
    void artifact;
    void preset;
    void signal;
    this.verifyCalls += 1;
    return Promise.resolve(111);
  }

  activate(artifact: DownloadArtifact, preset: ModelPreset): Promise<void> {
    void artifact;
    void preset;
    this.activateCalls += 1;
    this.installed = true;
    return Promise.resolve();
  }

  cleanup(artifact: DownloadArtifact | undefined): Promise<void> {
    void artifact;
    this.cleanupCalls += 1;
    return Promise.resolve();
  }
}

export class FakeLlamaEngine implements LlamaEngineAdapter {
  loadCalls = 0;
  error: Error | undefined;
  readonly lease: LlamaLease = createFakeLlamaLease();

  readonly loadedPresetIds: string[] = [];

  load(preset: ModelPreset, signal: AbortSignal): Promise<LlamaLease> {
    void signal;
    this.loadedPresetIds.push(preset.id);
    this.loadCalls += 1;
    if (this.error !== undefined) return Promise.reject(this.error);
    return Promise.resolve(this.lease);
  }

  loadDefault(signal: AbortSignal): Promise<LlamaLease> {
    return this.load(
      {
        id: "legacy-test-default",
        displayName: "Legacy test default",
        owner: "Fixture",
        repository: "Fixture-GGUF",
        revision: "1111111111111111111111111111111111111111",
        filename: "legacy.gguf",
        sha256: "a".repeat(64),
        expectedDownloadBytes: 1,
        maxBytes: 1,
        minimumMemoryBytes: 1,
        minimumContextTokens: 1,
        qualityRank: 1,
        license: "apache-2.0",
      },
      signal,
    );
  }
}

export const artifact = (operationId: string): DownloadArtifact => ({
  operationId,
  token: Symbol(operationId),
});

export const flushTasks = async (): Promise<void> => {
  await new Promise<void>((resolve) => setImmediate(resolve));
};
