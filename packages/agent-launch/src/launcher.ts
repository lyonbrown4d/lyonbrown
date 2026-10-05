import { deriveScope, type ScopeIdentity } from "@lyonbrown/local-control-plane";
import { validateOptions, type NormalizedConfiguration } from "./configuration.ts";
import { DaemonProbe } from "./daemon-probe.ts";
import { isAgentLaunchError, launchError } from "./errors.ts";
import { SpawnQuarantine } from "./spawn-quarantine.ts";
import { StageRunner } from "./stage-runner.ts";
import type {
  AgentLaunchConnection,
  AgentLauncher,
  ConnectAgentInput,
  CreateAgentLauncherOptions,
  DaemonExit,
  DiscoveryContext,
  SpawnAttempt,
  StartupLease,
} from "./types.ts";

export class DefaultAgentLauncher implements AgentLauncher {
  readonly #configuration: NormalizedConfiguration;
  readonly #options: CreateAgentLauncherOptions;
  readonly #probe: DaemonProbe;
  readonly #quarantine: SpawnQuarantine;
  readonly #stages: StageRunner;

  public constructor(options: CreateAgentLauncherOptions) {
    this.#options = options;
    this.#configuration = validateOptions(options);
    this.#stages = new StageRunner(
      options.ports.clock,
      options.ports.sleeper,
      this.#configuration.pollIntervalMs,
      this.#configuration.cleanupMs,
    );
    this.#probe = new DaemonProbe(options, this.#stages);
    this.#quarantine = new SpawnQuarantine(this.#stages);
  }

  public async connect(input: ConnectAgentInput = {}): Promise<AgentLaunchConnection> {
    this.#stages.throwIfAborted(input.signal);
    const deadline = this.#stages.now() + this.#configuration.startupMs;
    const scope = await this.#deriveScope(deadline, input.signal);
    const context: DiscoveryContext = {
      nativeProvider: this.#options.nativeProvider,
      scope,
    };
    const existing = await this.#probe.probe(context, deadline, "STARTUP_TIMEOUT", input.signal);
    if (existing.kind === "ready") return { ...existing.connection, source: "reused" };
    if (input.autoStart === false) {
      throw launchError("unavailable", "AUTO_START_DISABLED", "no reusable daemon is available");
    }
    return this.#electAndStart(context, deadline, input.signal);
  }

  async #electAndStart(
    context: DiscoveryContext,
    deadline: number,
    signal?: AbortSignal,
  ): Promise<AgentLaunchConnection> {
    for (;;) {
      this.#stages.throwIfAborted(signal);
      this.#stages.throwIfExpired(deadline, "STARTUP_TIMEOUT");
      const lease = await this.#acquireLease(context, deadline, signal);
      if (lease) return this.#runAsWinner(context, deadline, lease, signal);
      await this.#stages.poll(deadline, "STARTUP_TIMEOUT", signal);
      const existing = await this.#probe.probe(context, deadline, "STARTUP_TIMEOUT", signal);
      if (existing.kind === "ready") return { ...existing.connection, source: "reused" };
    }
  }

  async #runAsWinner(
    context: DiscoveryContext,
    deadline: number,
    lease: StartupLease,
    signal?: AbortSignal,
  ): Promise<AgentLaunchConnection> {
    let outcome:
      | { readonly kind: "ready"; readonly connection: AgentLaunchConnection }
      | { readonly kind: "failed"; readonly error: unknown }
      | { readonly kind: "quarantined"; readonly error: unknown };
    try {
      const existing = await this.#probe.probe(context, deadline, "READINESS_TIMEOUT", signal);
      if (existing.kind === "ready") {
        outcome = {
          kind: "ready",
          connection: { ...existing.connection, source: "reused" },
        };
      } else {
        this.#stages.throwIfExpired(deadline, "READINESS_TIMEOUT");
        const attempt = this.#beginSpawn(context.scope);
        try {
          outcome = {
            kind: "ready",
            connection: await this.#waitForReadiness(context, deadline, attempt, signal),
          };
        } catch (error: unknown) {
          this.#quarantine.start(lease, attempt);
          await this.#abandonQuietly(attempt);
          outcome = { kind: "quarantined", error };
        }
      }
    } catch (error: unknown) {
      outcome = { kind: "failed", error };
    }
    if (outcome.kind === "quarantined") throw outcome.error;
    try {
      await this.#releaseLease(lease);
    } catch (error: unknown) {
      if (outcome.kind === "ready") await this.#closeQuietly(outcome.connection.client);
      throw error;
    }
    if (outcome.kind === "failed") throw outcome.error;
    return outcome.connection;
  }

  async #waitForReadiness(
    context: DiscoveryContext,
    deadline: number,
    attempt: SpawnAttempt,
    signal?: AbortSignal,
  ): Promise<AgentLaunchConnection> {
    const exit = this.#observeExit(attempt);
    for (;;) {
      this.#stages.throwIfAborted(signal);
      this.#stages.throwIfExpired(deadline, "READINESS_TIMEOUT");
      const ready = await this.#probe.probe(context, deadline, "READINESS_TIMEOUT", signal);
      if (ready.kind === "ready") return { ...ready.connection, source: "spawned" };
      const outcome = await Promise.race([
        exit,
        this.#stages
          .poll(deadline, "READINESS_TIMEOUT", signal)
          .then(() => ({ kind: "poll" as const })),
      ]);
      if (outcome.kind === "exit") this.#throwDaemonExit(outcome.exit);
      if (outcome.kind === "exit-failed") this.#throwDaemonExit(undefined);
    }
  }

  async #deriveScope(deadline: number, signal?: AbortSignal): Promise<ScopeIdentity> {
    try {
      return await this.#stages.waitFor(
        () =>
          deriveScope(
            this.#options.workspace,
            this.#options.effectiveConfigFingerprint,
            this.#options.nativeProvider,
          ),
        deadline,
        "STARTUP_TIMEOUT",
        signal,
      );
    } catch (error: unknown) {
      if (isAgentLaunchError(error)) throw error;
      throw launchError(
        "unavailable",
        "CONTROL_PLANE_FAILED",
        "secure control plane is unavailable",
      );
    }
  }

  async #acquireLease(
    context: DiscoveryContext,
    deadline: number,
    signal?: AbortSignal,
  ): Promise<StartupLease | null> {
    try {
      return await this.#stages.waitFor(
        (operationSignal) =>
          this.#options.ports.discovery.tryAcquireStartupLease(context, operationSignal),
        deadline,
        "STARTUP_TIMEOUT",
        signal,
        (lateLease, cleanupSignal) => this.#releaseLateLease(lateLease, cleanupSignal),
      );
    } catch (error: unknown) {
      if (isAgentLaunchError(error)) throw error;
      throw launchError("unavailable", "CONTROL_PLANE_FAILED", "startup election failed");
    }
  }

  async #releaseLease(lease: StartupLease): Promise<void> {
    const released = await this.#stages.cleanup((cleanupSignal) =>
      this.#releaseLeaseOperation(lease, cleanupSignal),
    );
    if (!released) {
      throw launchError("unavailable", "CONTROL_PLANE_FAILED", "startup lease release failed");
    }
  }

  #beginSpawn(scope: ScopeIdentity): SpawnAttempt {
    try {
      return this.#options.ports.spawner.begin({
        canonicalWorkspace: scope.canonicalWorkspace,
        effectiveConfigFingerprint: this.#options.effectiveConfigFingerprint,
        scopeId: scope.scopeId,
      });
    } catch (error: unknown) {
      if (isAgentLaunchError(error)) throw error;
      throw launchError("unavailable", "SPAWN_FAILED", "daemon process could not be started");
    }
  }

  #throwDaemonExit(exit: DaemonExit | undefined): never {
    let exitCode: unknown;
    try {
      exitCode = exit?.exitCode;
    } catch {
      exitCode = undefined;
    }
    const details =
      typeof exitCode === "number" && Number.isSafeInteger(exitCode) ? { exitCode } : undefined;
    throw launchError("unavailable", "DAEMON_EXITED", "daemon exited before readiness", details);
  }

  async #closeQuietly(connectionClient: AgentLaunchConnection["client"]): Promise<void> {
    await this.#stages.cleanup(() => connectionClient.close());
  }

  #observeExit(
    attempt: SpawnAttempt,
  ): Promise<
    { readonly kind: "exit"; readonly exit: DaemonExit } | { readonly kind: "exit-failed" }
  > {
    try {
      return Promise.resolve(attempt.exited).then(
        (exit) => ({ kind: "exit" as const, exit }),
        () => ({ kind: "exit-failed" as const }),
      );
    } catch {
      return Promise.resolve({ kind: "exit-failed" as const });
    }
  }

  async #releaseLateLease(lease: StartupLease | null, signal: AbortSignal): Promise<void> {
    if (!lease) return;
    await this.#releaseLeaseOperation(lease, signal);
  }

  async #releaseLeaseOperation(lease: StartupLease, signal: AbortSignal): Promise<void> {
    await lease.release(signal);
  }

  async #abandonQuietly(attempt: SpawnAttempt): Promise<void> {
    await this.#stages.cleanup((cleanupSignal) => this.#abandonAttempt(attempt, cleanupSignal));
  }

  async #abandonAttempt(attempt: SpawnAttempt, signal: AbortSignal): Promise<void> {
    await attempt.abandon(signal);
  }
}

export function createAgentLauncher(options: CreateAgentLauncherOptions): AgentLauncher {
  return new DefaultAgentLauncher(options);
}
