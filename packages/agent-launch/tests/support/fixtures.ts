import type {
  AgentClient,
  AgentCommand,
  ClientDeadlines,
  CommandReceiptDto,
  HandshakeDto,
  HandshakeInput,
  HealthDto,
  RuntimeEventDto,
  RuntimeSnapshotDto,
  SubscribeEventsInput,
} from "@lyonbrown/agent-client";
import type { NativeControlPlaneProvider } from "@lyonbrown/local-control-plane";
import { createNativeControlPlaneProvider } from "@lyonbrown/local-control-plane/native-adapter";
import type {
  AgentClientFactory,
  Clock,
  DiscoveredDaemonCapability,
  DiscoveryClient,
  DiscoveryContext,
  Sleeper,
  StartupLease,
} from "../../src/types.ts";

export { FakeSpawner } from "./fake-spawner.ts";

export const WORKSPACE = process.platform === "win32" ? "C:\\workspace" : "/workspace";
export const CONFIG = "runtime-config-v1";

function rejectUnknown<T>(value: unknown): Promise<T> {
  return Promise.resolve().then(() => {
    // Deliberately models hostile boundary values that are not Error instances.
    throw value;
  });
}

export function nativeProvider(): NativeControlPlaneProvider {
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
    workspaceIdentity(path) {
      return Promise.resolve({
        ok: true,
        value: {
          canonicalPath: path,
          identity: { kind: "inode", deviceId: 1n, inodeNumber: 2n },
        },
      });
    },
    openSecureRunDirectory() {
      return Promise.resolve({ ok: false, code: "not_found" });
    },
  });
}

export class Candidate implements DiscoveredDaemonCapability {
  public constructor(
    public readonly serverInstanceId: string,
    public readonly scopeId: string,
  ) {}
}

export function snapshot(revision = 1n): RuntimeSnapshotDto {
  return {
    protocolVersion: 1,
    revision,
    phase: "ready",
    sessionId: null,
    turns: [],
    queuedFollowUps: [],
    pendingApproval: null,
    lastError: null,
    modelSelection: { kind: "auto" },
    route: null,
    modelSetup: null,
    modelCatalog: null,
    modelPolicy: null,
  };
}

export class FakeAgentClient implements AgentClient {
  public enqueueCalls = 0;
  public handshakeCalls = 0;
  public closed = false;
  public snapshotHook: (() => void) | undefined;
  public snapshotRevision: (() => bigint) | undefined;
  public snapshotFailure: unknown;
  public closeFailure: unknown;

  public constructor(
    public candidate: Candidate,
    public handshakeFailure?: unknown,
  ) {}

  public health(): Promise<HealthDto> {
    return Promise.resolve({ protocolVersion: 1, status: "serving" });
  }

  public handshake(input: HandshakeInput): Promise<HandshakeDto> {
    void input;
    this.handshakeCalls += 1;
    if (this.handshakeFailure !== undefined) return rejectUnknown(this.handshakeFailure);
    return Promise.resolve({
      protocolVersion: 1,
      serverVersion: "test",
      serverInstanceId: this.candidate.serverInstanceId,
      capabilities: [],
      scopeId: this.candidate.scopeId,
    });
  }

  public enqueue(command: AgentCommand): Promise<CommandReceiptDto> {
    void command;
    this.enqueueCalls += 1;
    return Promise.resolve({ commandId: "not-used", accepted: true });
  }

  public getSnapshot(): Promise<RuntimeSnapshotDto> {
    this.snapshotHook?.();
    if (this.snapshotFailure !== undefined) return rejectUnknown(this.snapshotFailure);
    return Promise.resolve(snapshot(this.snapshotRevision?.() ?? 1n));
  }

  public subscribeEvents(input: SubscribeEventsInput): AsyncIterable<RuntimeEventDto> {
    return (async function* (): AsyncIterable<RuntimeEventDto> {
      const afterRevision = await Promise.resolve(input.afterRevision);
      if (afterRevision < 0n) {
        yield { protocolVersion: 1, revision: 0n, snapshot: snapshot(0n) };
      }
    })();
  }

  public replaceBinding(): void {
    return undefined;
  }

  public close(): Promise<void> {
    this.closed = true;
    if (this.closeFailure !== undefined) return rejectUnknown(this.closeFailure);
    return Promise.resolve();
  }
}

export class FakeClientFactory implements AgentClientFactory {
  public readonly clients: FakeAgentClient[] = [];
  public readonly replacements: Candidate[] = [];
  public handshakeFailure: unknown;
  public snapshotFailure: unknown;
  public closeFailure: unknown;
  public onSnapshot: (() => void) | undefined;
  public revisionFor: ((candidate: Candidate) => bigint) | undefined;

  public create(candidate: DiscoveredDaemonCapability, deadlines?: ClientDeadlines): AgentClient {
    void deadlines;
    const client = new FakeAgentClient(candidate as Candidate, this.handshakeFailure);
    client.snapshotHook = this.onSnapshot;
    client.snapshotRevision = () => this.revisionFor?.(client.candidate) ?? 1n;
    client.snapshotFailure = this.snapshotFailure;
    client.closeFailure = this.closeFailure;
    this.clients.push(client);
    return client;
  }

  public replaceBinding(client: AgentClient, candidate: DiscoveredDaemonCapability): void {
    const concrete = candidate as Candidate;
    (client as FakeAgentClient).candidate = concrete;
    this.replacements.push(concrete);
  }
}

export class ManualClock implements Clock {
  public time = 0;
  public now(): number {
    return this.time;
  }
}

export class AdvancingSleeper implements Sleeper {
  public onSleep: (() => void) | undefined;
  public resolveDeadlineSleeps = false;
  public readonly resolveDeadlineSleepCalls = new Set<number>();
  public deadlineSleepCalls = 0;
  public failure: unknown;
  public constructor(private readonly clock: ManualClock) {}
  public async sleep(durationMs: number, signal?: AbortSignal): Promise<void> {
    if (signal?.aborted) throw new Error("aborted");
    if (this.failure !== undefined) return rejectUnknown(this.failure);
    if (durationMs > 5) this.deadlineSleepCalls += 1;
    if (
      durationMs > 5 &&
      !this.resolveDeadlineSleeps &&
      !this.resolveDeadlineSleepCalls.has(this.deadlineSleepCalls)
    ) {
      await new Promise<void>((resolve, reject) => {
        const abort = () => {
          reject(new Error("aborted"));
        };
        signal?.addEventListener("abort", abort, { once: true });
        if (signal?.aborted) abort();
        void resolve;
      });
      return;
    }
    this.clock.time += durationMs;
    this.onSleep?.();
    await Promise.resolve();
    if (signal?.aborted) throw new Error("aborted");
  }
}

export class FakeDiscovery implements DiscoveryClient {
  public candidate: Candidate | null = null;
  public leaseHeld = false;
  public lockAttempts = 0;
  public reads = 0;
  public readFailure: unknown;
  public lockFailure: unknown;

  public read(
    _context: DiscoveryContext,
    _signal: AbortSignal,
  ): Promise<DiscoveredDaemonCapability | null> {
    void _context;
    void _signal;
    this.reads += 1;
    if (this.readFailure !== undefined) return rejectUnknown(this.readFailure);
    return Promise.resolve(this.candidate);
  }

  public tryAcquireStartupLease(
    _context: DiscoveryContext,
    _signal: AbortSignal,
  ): Promise<StartupLease | null> {
    void _context;
    void _signal;
    this.lockAttempts += 1;
    if (this.lockFailure !== undefined) return rejectUnknown(this.lockFailure);
    if (this.leaseHeld) return Promise.resolve(null);
    this.leaseHeld = true;
    let released = false;
    return Promise.resolve({
      release: (_releaseSignal: AbortSignal) => {
        void _releaseSignal;
        if (!released) this.leaseHeld = false;
        released = true;
        return Promise.resolve();
      },
    });
  }
}
