import { createAgentClient, type AgentClient, type ClientDeadlines } from "@lyonbrown/agent-client";
import {
  ControlPlaneError,
  acquireStartupLock,
  bindDiscoveredEpoch,
  deriveScopedDaemonPaths,
  openSecureRunDirectory,
  type EpochBinding,
  type NativeControlPlaneProvider,
  type ScopeIdentity,
  type SecureRunDirectory,
} from "@lyonbrown/local-control-plane";
import type {
  AgentClientFactory,
  DiscoveredDaemonCapability,
  DiscoveryClient,
  DiscoveryContext,
  StartupLease,
} from "@lyonbrown/agent-launch/internal/types";

export interface ProductionDiscoveryOptions {
  readonly nativeProvider: NativeControlPlaneProvider;
  readonly homeDirectory: string;
  readonly effectiveConfigFingerprint: string;
}

const BINDINGS = new WeakMap<DiscoveredDaemonCapability, EpochBinding>();

class OpaqueDaemonCapability implements DiscoveredDaemonCapability {
  public readonly serverInstanceId: string;
  public readonly scopeId: string;

  public constructor(binding: EpochBinding) {
    this.serverInstanceId = binding.serverInstanceId;
    this.scopeId = binding.scopeId;
    BINDINGS.set(this, binding);
    Object.freeze(this);
  }
}

class NativeStartupLease implements StartupLease {
  #releasePromise: Promise<void> | undefined;

  public constructor(
    private readonly directory: SecureRunDirectory,
    private readonly releaseNative: () => Promise<void>,
  ) {}

  public release(signal: AbortSignal): Promise<void> {
    this.#releasePromise ??= this.#startRelease();
    return waitForRelease(this.#releasePromise, signal);
  }

  #startRelease(): Promise<void> {
    const operation = this.#release();
    void operation.catch(() => undefined);
    return operation;
  }

  async #release(): Promise<void> {
    try {
      await this.releaseNative();
    } finally {
      await this.directory.close();
    }
  }
}

function waitForRelease(completion: Promise<void>, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(releaseAbortError());
  return new Promise<void>((resolve, reject) => {
    const abort = (): void => {
      signal.removeEventListener("abort", abort);
      reject(releaseAbortError());
    };
    signal.addEventListener("abort", abort, { once: true });
    void completion.then(
      () => {
        signal.removeEventListener("abort", abort);
        resolve();
      },
      () => {
        signal.removeEventListener("abort", abort);
        reject(new Error("startup lease release failed"));
      },
    );
    if (signal.aborted) abort();
  });
}

function releaseAbortError(): DOMException {
  return new DOMException("startup lease release was cancelled", "AbortError");
}

class ScopedRunDirectoryResolver {
  #resolved:
    | {
        readonly scope: ScopeIdentity;
        readonly path: Promise<string>;
      }
    | undefined;

  public constructor(private readonly options: ProductionDiscoveryOptions) {}

  public resolve(context: DiscoveryContext, signal: AbortSignal): Promise<string> {
    signal.throwIfAborted();
    if (context.nativeProvider !== this.options.nativeProvider) {
      throw new ControlPlaneError("NATIVE_SECURITY_PROVIDER_REQUIRED");
    }
    const current = this.#resolved;
    if (current !== undefined) {
      if (
        current.scope.scopeId !== context.scope.scopeId ||
        current.scope.canonicalWorkspace !== context.scope.canonicalWorkspace
      ) {
        throw new ControlPlaneError("PLATFORM_SECURITY_FAILURE", {
          operation: "open_run_directory",
        });
      }
      return current.path;
    }
    const path = this.#derive(context.scope);
    const resolution = { scope: context.scope, path };
    this.#resolved = resolution;
    void path.catch(() => {
      if (this.#resolved === resolution) this.#resolved = undefined;
    });
    return path;
  }

  async #derive(expected: ScopeIdentity): Promise<string> {
    const paths = await deriveScopedDaemonPaths(
      this.options.homeDirectory,
      expected.canonicalWorkspace,
      this.options.effectiveConfigFingerprint,
      this.options.nativeProvider,
    );
    if (
      paths.scopeId !== expected.scopeId ||
      paths.canonicalWorkspace !== expected.canonicalWorkspace
    ) {
      throw new ControlPlaneError("PLATFORM_SECURITY_FAILURE", {
        operation: "open_run_directory",
      });
    }
    return paths.runDirectory;
  }
}

class ProductionDiscoveryClient implements DiscoveryClient {
  readonly #directories: ScopedRunDirectoryResolver;

  public constructor(private readonly options: ProductionDiscoveryOptions) {
    this.#directories = new ScopedRunDirectoryResolver(options);
  }

  public async read(
    context: DiscoveryContext,
    signal: AbortSignal,
  ): Promise<DiscoveredDaemonCapability | null> {
    const directory = await this.#open(context, signal);
    try {
      signal.throwIfAborted();
      try {
        const descriptor = await directory.discovery.read();
        signal.throwIfAborted();
        return new OpaqueDaemonCapability(bindDiscoveredEpoch(descriptor));
      } catch (error: unknown) {
        if (error instanceof ControlPlaneError && error.code === "DESCRIPTOR_NOT_FOUND") {
          return null;
        }
        throw error;
      }
    } finally {
      await directory.close();
    }
  }

  public async tryAcquireStartupLease(
    context: DiscoveryContext,
    signal: AbortSignal,
  ): Promise<StartupLease | null> {
    const directory = await this.#open(context, signal);
    let lease: Awaited<ReturnType<typeof acquireStartupLock>> | undefined;
    try {
      signal.throwIfAborted();
      lease = await acquireStartupLock(directory);
      signal.throwIfAborted();
      if (lease === null) {
        await directory.close();
        return null;
      }
      const acquiredLease = lease;
      return new NativeStartupLease(directory, () => acquiredLease.release());
    } catch (error: unknown) {
      try {
        if (lease !== undefined && lease !== null) await lease.release();
      } finally {
        await directory.close();
      }
      throw error;
    }
  }

  async #open(context: DiscoveryContext, signal: AbortSignal): Promise<SecureRunDirectory> {
    const path = await this.#directories.resolve(context, signal);
    signal.throwIfAborted();
    return openSecureRunDirectory(path, this.options.nativeProvider);
  }
}

class ProductionAgentClientFactory implements AgentClientFactory {
  public create(candidate: DiscoveredDaemonCapability, deadlines?: ClientDeadlines): AgentClient {
    const binding = requireBinding(candidate);
    return createAgentClient({
      binding,
      ...(deadlines === undefined ? {} : { deadlines }),
    });
  }

  public replaceBinding(client: AgentClient, candidate: DiscoveredDaemonCapability): void {
    client.replaceBinding(requireBinding(candidate));
  }
}

function requireBinding(candidate: DiscoveredDaemonCapability): EpochBinding {
  const binding = BINDINGS.get(candidate);
  if (binding === undefined) {
    throw new ControlPlaneError("INVALID_DESCRIPTOR");
  }
  return binding;
}

export function createProductionDiscoveryClient(
  options: ProductionDiscoveryOptions,
): DiscoveryClient {
  return new ProductionDiscoveryClient(options);
}

export function createProductionAgentClientFactory(): AgentClientFactory {
  return new ProductionAgentClientFactory();
}
