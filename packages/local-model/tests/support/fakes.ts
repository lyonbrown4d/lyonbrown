import type {
  DownloadArtifact,
  DownloadProgress,
  LocalModelIo,
  LlamaEngineAdapter,
  LlamaLease,
  ModelPreset,
} from "../../src/internal/contracts.js";

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
  inspection: Deferred<boolean> | undefined;
  readonly downloads: {
    operationId: string;
    signal: AbortSignal;
    onProgress: (progress: DownloadProgress) => void;
    completion: Deferred<DownloadArtifact>;
  }[] = [];

  inspectInstalled(preset: ModelPreset, signal: AbortSignal): Promise<boolean> {
    void preset;
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
    return Promise.resolve(this.installed);
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
    void preset;
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
  readonly lease: LlamaLease = { dispose: () => Promise.resolve() };

  loadDefault(): Promise<LlamaLease> {
    this.loadCalls += 1;
    if (this.error !== undefined) return Promise.reject(this.error);
    return Promise.resolve(this.lease);
  }
}

export const artifact = (operationId: string): DownloadArtifact => ({
  operationId,
  token: Symbol(operationId),
});

export const flushTasks = async (): Promise<void> => {
  await new Promise<void>((resolve) => setImmediate(resolve));
};
